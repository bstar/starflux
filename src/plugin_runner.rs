//! Detached finite plugin jobs and bounded event-hook scheduling.
use crate::{
    plugin_store::{Store, read},
    plugins::{self, Action, Project},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Seek},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
// Version 4 understands popup dimensions in persistent pane declarations.
const VERSION: u64 = 4;
fn spawn_detached(command: &mut Command) -> Result<()> {
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
struct Job {
    version: u32,
    id: String,
    plugin: String,
    action: Action,
    path: PathBuf,
    digest: String,
    activation: String,
    project: Option<Project>,
    event: Option<Value>,
    status: String,
    boot: String,
    queued_ns: u64,
    pid: Option<u32>,
    start_ticks: Option<u64>,
    outcome: Option<String>,
    exit_code: Option<i32>,
    error: Option<String>,
}
fn boot() -> Result<String> {
    Ok(crate::agent_state::boot_id()
        .context("Host boot identity unavailable")?
        .to_string())
}
fn id(value: &str) -> Result<String> {
    Ok(uuid::Uuid::parse_str(value)?.simple().to_string())
}
fn name(id: &str) -> String {
    format!("job-{id}.json")
}
fn alive(pid: Option<u32>, ticks: Option<u64>, expected_boot: &str, role: &str) -> bool {
    let Some(pid) = pid else {
        return false;
    };
    boot().ok().as_deref() == Some(expected_boot)
        && crate::agent_state::process_start(pid) == ticks
        && fs::read(format!("/proc/{pid}/cmdline"))
            .is_ok_and(|v| v.split(|b| *b == 0).any(|a| a == role.as_bytes()))
}
fn save(store: &Store, job: &Job) -> Result<()> {
    let bytes = serde_json::to_vec(job)?;
    ensure!(bytes.len() <= 65536, "Plugin job context exceeds 64 KiB");
    store.write(&name(&job.id), &bytes)
}
fn load(store: &Store, id: &str) -> Result<Job> {
    let job: Job = read(&store.path.join(name(id)), 65536)?;
    ensure!(
        (1..=4).contains(&job.version) && job.id == id,
        "Invalid plugin job receipt"
    );
    Ok(job)
}
fn jobs(store: &Store) -> Result<Vec<Job>> {
    let mut jobs = Vec::new();
    for f in fs::read_dir(&store.path)?.flatten() {
        let n = f.file_name();
        let n = n.to_string_lossy();
        if let Some(id) = n.strip_prefix("job-").and_then(|s| s.strip_suffix(".json"))
            && let Ok(job) = load(store, id)
        {
            jobs.push(job);
        }
    }
    jobs.sort_by_key(|j| j.queued_ns);
    Ok(jobs)
}
fn refresh(store: &Store, job: &mut Job) -> Result<()> {
    if matches!(job.status.as_str(), "running" | "finishing")
        && !alive(job.pid, job.start_ticks, &job.boot, "--plugin-job")
    {
        if job.status == "finishing" {
            job.status = job.outcome.clone().unwrap_or("indeterminate".into());
        } else {
            job.status = "indeterminate".into();
            job.error = Some(
                "Plugin worker ended without a committed result; command was not repeated".into(),
            );
        }
        save(store, job)?;
    } else if job.status == "queued"
        && (job.boot != boot()?
            || crate::runtime::monotonic_ns().saturating_sub(job.queued_ns) > 30_000_000_000)
    {
        job.status = "expired".into();
        save(store, job)?;
    }
    Ok(())
}
pub fn list(plugin: Option<&str>) -> Result<Value> {
    let store = plugins::store()?;
    let mut items = Vec::new();
    for mut job in jobs(&store)?
        .into_iter()
        .rev()
        .filter(|j| plugin.is_none_or(|p| p == j.plugin))
        .take(100)
    {
        let _lock = store.lock(&format!("claim-{}.lock", job.id), true)?;
        job = load(&store, &job.id)?;
        refresh(&store, &mut job)?;
        items.push(serde_json::to_value(job)?);
    }
    Ok(json!({"jobs":items}))
}
pub fn package_busy(plugin: &str) -> Result<bool> {
    if crate::plugin_popup::package_busy(plugin)? || crate::plugin_panes::package_busy(plugin)? {
        return Ok(true);
    }
    let store = plugins::store()?;
    for mut job in jobs(&store)?.into_iter().filter(|job| {
        job.plugin == plugin && matches!(job.status.as_str(), "queued" | "running" | "finishing")
    }) {
        let _claim = store.lock(&format!("claim-{}.lock", job.id), true)?;
        job = load(&store, &job.id)?;
        refresh(&store, &mut job)?;
        if matches!(job.status.as_str(), "queued" | "running" | "finishing") {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn get(value: &str, output: bool) -> Result<Value> {
    let id = id(value)?;
    let store = plugins::store()?;
    let _lock = store.lock(&format!("claim-{id}.lock"), true)?;
    let mut job = load(&store, &id)?;
    refresh(&store, &mut job)?;
    let mut result = serde_json::to_value(job)?;
    if output {
        result["output"] = match read::<Value>(
            &store.path.join(format!("output-{id}.json")),
            8 * 1024 * 1024,
        ) {
            Ok(v) => v,
            Err(_) if !store.path.join(format!("output-{id}.json")).exists() => {
                json!({"stdout":"","stderr":"","pending":true})
            }
            Err(e) => return Err(e),
        };
    }
    Ok(result)
}
pub fn cancel(value: &str) -> Result<Value> {
    let id = id(value)?;
    let store = plugins::store()?;
    let _lock = store.lock(&format!("claim-{id}.lock"), true)?;
    let mut job = load(&store, &id)?;
    refresh(&store, &mut job)?;
    if job.status == "queued" {
        job.status = "cancelled".into();
        save(&store, &job)?;
    } else if job.status == "running" {
        store.write(&format!("cancel-{id}"), b"1")?;
    } else {
        anyhow::bail!("Job is no longer cancellable");
    }
    Ok(serde_json::to_value(job)?)
}
fn enqueue_in(
    store: &Store,
    plugin: &str,
    action: &str,
    digest: &str,
    project: Option<Project>,
    event: Option<Value>,
    operation: &str,
) -> Result<Value> {
    let replay_id = id(operation)?;
    if store.path.join(name(&replay_id)).exists() {
        let old = load(store, &replay_id)?;
        ensure!(
            old.plugin == plugin
                && old.action.id == action
                && old.digest == digest
                && old.project == project
                && old.event == event,
            "Plugin job ID conflict"
        );
        return Ok(serde_json::to_value(old)?);
    }
    let registry = plugins::registry()?;
    let entry = registry
        .plugins
        .get(plugin)
        .context("Plugin is not linked")?;
    let (manifest, current) = plugins::package_for(entry)?;
    ensure!(
        entry.enabled
            && entry.approved.as_deref() == Some(&current)
            && current == digest
            && manifest.id == plugin,
        "Plugin is disabled or its manifest changed"
    );
    let action = manifest
        .actions
        .into_iter()
        .find(|a| a.id == action)
        .context("Plugin action is unavailable")?;
    if action.cwd == "project"
        && (event.is_none() || event.as_ref().is_some_and(|v| v["kind"] == "link.opened"))
    {
        let p = project.as_ref().context("This action needs a project")?;
        let config = crate::saved(&p.id)?;
        ensure!(plugins::project(&config) == *p, "Project context changed");
        ensure!(
            !config.remote(0),
            "Project-directory actions need the project on this controller; package actions can explicitly use SSH"
        );
    }
    let _queue = store.lock("queue.lock", true)?;
    let id = id(operation)?;
    if store.path.join(name(&id)).exists() {
        let old = load(store, &id)?;
        ensure!(
            old.plugin == plugin
                && old.action.id == action.id
                && old.digest == digest
                && old.project == project
                && old.event == event,
            "Plugin job ID conflict"
        );
        return Ok(serde_json::to_value(old)?);
    }
    ensure!(
        jobs(store)?.iter().filter(|j| j.status == "queued").count() < 32,
        "Plugin job queue is full"
    );
    let job = Job {
        // Older services skip new receipts rather than rejecting a command
        // whose managed/handler manifest they cannot understand during handoff.
        version: 4,
        id,
        plugin: plugin.into(),
        action,
        path: entry.path.clone(),
        digest: current,
        activation: entry.activation.clone(),
        project,
        event,
        status: "queued".into(),
        boot: boot()?,
        queued_ns: crate::runtime::monotonic_ns(),
        pid: None,
        start_ticks: None,
        outcome: None,
        exit_code: None,
        error: None,
    };
    save(store, &job)?;
    drop(_queue);
    Ok(serde_json::to_value(job)?)
}
pub fn enqueue(
    plugin: &str,
    action: &str,
    digest: &str,
    project: Option<Project>,
    operation: &str,
) -> Result<Value> {
    let store = plugins::store()?;
    let result = enqueue_in(&store, plugin, action, digest, project, None, operation)?;
    ensure()?;
    Ok(result)
}
pub fn open_link(
    plugin: &str,
    handler: &str,
    digest: &str,
    url: &str,
    project: Option<Project>,
    operation: &str,
) -> Result<Value> {
    let uri = crate::plugin_links::parse(url)?;
    let registry = plugins::registry()?;
    let entry = registry
        .plugins
        .get(plugin)
        .context("Plugin is not linked")?;
    let (manifest, current) = plugins::package_for(entry)?;
    ensure!(
        manifest.id == plugin
            && current == digest
            && entry.enabled
            && entry.approved.as_deref() == Some(digest),
        "Link handler is disabled or its package changed"
    );
    let handler = manifest
        .link_handlers
        .iter()
        .find(|h| h.id == handler)
        .context("Link handler is unavailable")?;
    ensure!(
        crate::plugin_links::matches(handler, &uri),
        "URL does not match this link handler"
    );
    let store = plugins::store()?;
    let result = enqueue_in(
        &store,
        plugin,
        &handler.action,
        digest,
        project,
        Some(json!({"kind":"link.opened","handler":handler.id,"url":url})),
        operation,
    )?;
    ensure()?;
    Ok(result)
}
#[derive(Serialize, Deserialize, Default)]
struct Service {
    version: u64,
    pid: u32,
    start_ticks: u64,
    boot: String,
    instance: String,
    scans: u64,
    errors: u64,
    warning: Option<String>,
}
pub fn ensure() -> Result<()> {
    if std::env::var("STARFLUX_PLUGIN_SERVICE").as_deref() == Ok("off") {
        return Ok(());
    }
    let registry = plugins::registry()?;
    let hooks = registry.plugins.values().any(|e| {
        e.enabled
            && plugins::package_metadata(e)
                .is_ok_and(|(m, h)| e.approved.as_deref() == Some(&h) && !m.hooks.is_empty())
    });
    if !hooks && !plugins::data_path().exists() {
        return Ok(());
    }
    let store = plugins::store()?;
    if !hooks && !jobs(&store)?.iter().any(|j| j.status == "queued") {
        return Ok(());
    }
    if store.lock("service.lock", false)?.is_none()
        && let Ok(s) = read::<Service>(&store.path.join("service.json"), 8192)
        && alive(
            Some(s.pid),
            Some(s.start_ticks),
            &s.boot,
            "--plugin-service",
        )
        && s.version >= VERSION
    {
        return Ok(());
    }
    store.write(
        "service-request.json",
        &serde_json::to_vec(&json!({"version":VERSION}))?,
    )?;
    spawn_detached(
        Command::new("/proc/self/exe")
            .arg("--plugin-service")
            .arg(&store.path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0),
    )?;
    Ok(())
}
pub fn service_status() -> Result<Value> {
    if !plugins::data_path().exists() {
        return Ok(json!({"running":false}));
    }
    let store = plugins::store()?;
    match read::<Service>(&store.path.join("service.json"), 8192) {
        Ok(s) => Ok(
            json!({"running":alive(Some(s.pid),Some(s.start_ticks),&s.boot,"--plugin-service"),"service":s}),
        ),
        Err(_) if !store.path.join("service.json").exists() => Ok(json!({"running":false})),
        Err(e) => Err(e),
    }
}
pub fn service(path: &str) -> Result<()> {
    ensure!(*path == plugins::data_path(), "Plugin namespace changed");
    let store = Store::open(PathBuf::from(path))?;
    // The outgoing version observes service-request.json on its next scan.
    // Wait briefly for that cooperative handoff instead of abandoning queued
    // work just because its lock was still held when this process started.
    let deadline = Instant::now() + Duration::from_secs(3);
    let _service = loop {
        if !store.current()
            || read::<Value>(&store.path.join("service-request.json"), 8192)
                .is_ok_and(|r| r["version"].as_u64().is_some_and(|v| v > VERSION))
        {
            return Ok(());
        }
        if let Some(lock) = store.lock("service.lock", false)? {
            break lock;
        }
        if Instant::now() >= deadline {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut service = Service {
        version: VERSION,
        pid: std::process::id(),
        start_ticks: crate::agent_state::process_start(std::process::id())
            .context("Worker identity unavailable")?,
        boot: boot()?,
        instance: uuid::Uuid::new_v4().to_string(),
        ..Default::default()
    };
    store.write("service.json", &serde_json::to_vec(&service)?)?;
    let mut idle = Instant::now();
    let mut reconciled = Instant::now() - Duration::from_secs(5);
    while store.current() {
        if read::<Value>(&store.path.join("service-request.json"), 8192)
            .is_ok_and(|r| r["version"].as_u64().is_some_and(|v| v > VERSION))
        {
            break;
        }
        let result = (|| -> Result<bool> {
            let registry = plugins::registry()?;
            let mut active = false;
            for (plugin, entry) in &registry.plugins {
                if !entry.enabled {
                    continue;
                }
                let Ok((manifest, digest)) = plugins::package_metadata(entry) else {
                    continue;
                };
                if entry.approved.as_deref() != Some(&digest) || manifest.id != *plugin {
                    continue;
                }
                if manifest.hooks.is_empty() {
                    continue;
                }
                active = true;
                let delivered = (|| -> Result<()> {
                    if reconciled.elapsed() >= Duration::from_secs(5) {
                        crate::automation::journal::snapshot()?;
                        reconciled = Instant::now();
                    }
                    let cursor_path = store.path.join(format!("cursor-{plugin}.json"));
                    let mut cursor: Value = read(&cursor_path, 8192).unwrap_or(json!({}));
                    if cursor["activation"] != entry.activation {
                        cursor = json!({"activation":entry.activation,"cursor":crate::automation::journal::snapshot()?.1});
                    }
                    for hook in manifest.hooks.iter().filter(|h| h.on == "startup") {
                        let job = format!(
                            "{:x}",
                            sha2::Sha256::digest(
                                format!(
                                    "{}:{}:{}:{}",
                                    service.instance, plugin, entry.activation, hook.id
                                )
                                .as_bytes()
                            )
                        );
                        let _ = enqueue_in(
                            &store,
                            plugin,
                            &hook.action,
                            &digest,
                            None,
                            Some(json!({"kind":"startup","service":service.instance})),
                            &job[..32],
                        )?;
                    }
                    let after = cursor["cursor"]
                        .as_str()
                        .context("Missing hook cursor")?
                        .to_owned();
                    match crate::automation::journal::read(&after) {
                        Ok(events) => {
                            for event in events {
                                for hook in manifest.hooks.iter().filter(|h| {
                                    h.on == event.kind
                                        && (h.states.is_empty()
                                            || event.data["state"]
                                                .as_str()
                                                .is_some_and(|s| h.states.iter().any(|v| v == s)))
                                }) {
                                    let project_id = event
                                        .resource
                                        .split('/')
                                        .nth(1)
                                        .context("Event has no project")?;
                                    let project = crate::saved(project_id)
                                        .ok()
                                        .filter(|c| c.workspace == project_id)
                                        .map(|c| plugins::project(&c));
                                    let key = format!(
                                        "{}:{}:{}:{}:{}",
                                        plugin,
                                        entry.activation,
                                        hook.id,
                                        after.split(':').next().unwrap_or(""),
                                        event.sequence
                                    );
                                    let job = format!("{:x}", sha2::Sha256::digest(key.as_bytes()));
                                    enqueue_in(
                                        &store,
                                        plugin,
                                        &hook.action,
                                        &digest,
                                        project,
                                        Some(serde_json::to_value(&event)?),
                                        &job[..32],
                                    )?;
                                }
                                cursor["cursor"] = json!(format!(
                                    "{}:{}",
                                    after.split(':').next().unwrap_or(""),
                                    event.sequence
                                ));
                            }
                        }
                        Err(_) => {
                            service.warning = Some(format!(
                                "Event cursor expired for {plugin}; missed events were not replayed"
                            ));
                            cursor["cursor"] = json!(crate::automation::journal::snapshot()?.1);
                        }
                    }
                    store.write(
                        &format!("cursor-{plugin}.json"),
                        &serde_json::to_vec(&cursor)?,
                    )?;
                    Ok(())
                })();
                if let Err(error) = delivered {
                    service.errors += 1;
                    service.warning = Some(format!("{plugin}: {error:#}"));
                }
            }
            let current = jobs(&store)?;
            let running = current
                .iter()
                .filter(|j| {
                    matches!(j.status.as_str(), "running" | "finishing")
                        && alive(j.pid, j.start_ticks, &j.boot, "--plugin-job")
                })
                .count();
            let mut capacity = 2usize.saturating_sub(running);
            for mut job in current {
                let Some(_claim) = store.lock(&format!("claim-{}.lock", job.id), false)? else {
                    continue;
                };
                job = load(&store, &job.id)?;
                refresh(&store, &mut job)?;
                if job.status == "queued" && capacity > 0 {
                    spawn_detached(
                        Command::new("/proc/self/exe")
                            .arg("--plugin-job")
                            .arg(&store.path)
                            .arg(&job.id)
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .process_group(0),
                    )?;
                    capacity -= 1;
                    active = true;
                } else if matches!(job.status.as_str(), "running" | "finishing") {
                    active = true;
                }
            }
            Ok(active)
        })();
        match result {
            Ok(true) => idle = Instant::now(),
            Ok(false) => {}
            Err(error) => {
                service.errors += 1;
                service.warning = Some(format!("{error:#}"));
            }
        }
        service.scans += 1;
        let _ = store.write("service.json", &serde_json::to_vec(&service)?);
        if idle.elapsed() > Duration::from_secs(60) {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}
use sha2::Digest;
struct Cleanup;
impl Drop for Cleanup {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-libc::getpid(), libc::SIGKILL);
        }
    }
}
fn capture(
    reader: impl Read + Send + 'static,
    overflow: Arc<AtomicBool>,
) -> (Arc<Mutex<Vec<u8>>>, Arc<AtomicBool>) {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let target = buffer.clone();
    let done = Arc::new(AtomicBool::new(false));
    let finished = done.clone();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut bytes = [0u8; 8192];
        while let Ok(size) = reader.read(&mut bytes) {
            if size == 0 {
                break;
            }
            let mut out = target.lock().unwrap();
            let remaining = (512 * 1024usize).saturating_sub(out.len());
            out.extend_from_slice(&bytes[..size.min(remaining)]);
            if size > remaining {
                overflow.store(true, Ordering::Release);
            }
        }
        finished.store(true, Ordering::Release);
    });
    (buffer, done)
}
pub fn worker(path: &str, value: &str) -> Result<()> {
    ensure!(
        unsafe { libc::getpgrp() == libc::getpid() },
        "Plugin worker needs its own process group"
    );
    ensure!(*path == plugins::data_path(), "Plugin namespace changed");
    let store = Store::open(PathBuf::from(path))?;
    let id = id(value)?;
    let mut slot = None;
    for index in 0..2 {
        if let Some(file) = store.lock(&format!("slot-{index}.lock"), false)? {
            slot = Some(file);
            break;
        }
    }
    let Some(_slot) = slot else {
        return Ok(());
    };
    let claim = store.lock(&format!("claim-{id}.lock"), true)?;
    let mut job = load(&store, &id)?;
    refresh(&store, &mut job)?;
    if job.status != "queued" {
        return Ok(());
    }
    let prepared = (|| -> Result<_> {
        let r = plugins::registry()?;
        let entry = r.plugins.get(&job.plugin).context("Plugin was unlinked")?;
        let (manifest, digest) = plugins::package_for(entry)?;
        ensure!(
            entry.enabled
                && entry.activation == job.activation
                && entry.approved.as_deref() == Some(&digest)
                && digest == job.digest
                && manifest.id == job.plugin,
            "Plugin was disabled, re-enabled or changed before execution"
        );
        let config = if let Some(project) = &job.project {
            let config = crate::saved(&project.id)?;
            ensure!(
                plugins::project(&config) == *project,
                "Project context changed before execution"
            );
            Some(config)
        } else {
            None
        };
        let lease = if let Some(config) = &config {
            crate::worktrees::ensure_allowed(config)?
        } else {
            None
        };
        let cwd = if job.action.cwd == "project" {
            let config = config.as_ref().context("Action requires a project")?;
            ensure!(!config.remote(0), "Project is on another host");
            PathBuf::from(
                config
                    .directory
                    .as_ref()
                    .context("Project has no directory")?,
            )
        } else {
            job.path.clone()
        };
        ensure!(cwd.is_dir(), "Plugin working directory is unavailable");
        Ok((cwd, lease))
    })();
    let (cwd, _lease) = match prepared {
        Ok(value) => value,
        Err(error) => {
            job.status = "rejected".into();
            job.error = Some(format!("{error:#}"));
            save(&store, &job)?;
            return Ok(());
        }
    };
    job.pid = Some(std::process::id());
    job.start_ticks = crate::agent_state::process_start(std::process::id());
    job.boot = boot()?;
    job.status = "running".into();
    save(&store, &job)?;
    drop(claim);
    let _cleanup = Cleanup;
    let result = (|| -> Result<(String, Option<i32>, Value)> {
        let (config_dir, state_dir) = plugins::directories(&store, &job.plugin)?;
        let binary = std::env::current_exe()?
            .to_string_lossy()
            .trim_end_matches(" (deleted)")
            .to_owned();
        let context = json!({"version":1,"plugin_id":job.plugin,"action_id":job.action.id,"job_id":job.id,"project":job.project,"event":job.event,"config_dir":config_dir,"state_dir":state_dir,"bin_path":binary});
        let mut input = tempfile::tempfile()?;
        serde_json::to_writer(&mut input, &context)?;
        input.rewind()?;
        let program = if let Some(program) = job.action.argv[0].strip_prefix("package:") {
            job.path.join(program)
        } else {
            PathBuf::from(&job.action.argv[0])
        };
        let mut command = Command::new(program);
        command
            .args(&job.action.argv[1..])
            .current_dir(cwd)
            .stdin(input)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("STARFLUX_PLUGIN_ID", &job.plugin)
            .env("STARFLUX_PLUGIN_ACTION_ID", &job.action.id)
            .env("STARFLUX_PLUGIN_JOB_ID", &job.id)
            .env(
                "STARFLUX_PLUGIN_CONTEXT_JSON",
                serde_json::to_string(&context)?,
            )
            .env("STARFLUX_PLUGIN_CONFIG_DIR", config_dir)
            .env("STARFLUX_PLUGIN_STATE_DIR", state_dir)
            .env("STARFLUX_BIN_PATH", binary);
        let parent = unsafe { libc::getpid() };
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
        let mut child = command.spawn()?;
        let overflow = Arc::new(AtomicBool::new(false));
        let (stdout, stdout_done) = capture(child.stdout.take().unwrap(), overflow.clone());
        let (stderr, stderr_done) = capture(child.stderr.take().unwrap(), overflow.clone());
        let began = Instant::now();
        let (mut status, exit) = loop {
            if !store.current() {
                anyhow::bail!("Plugin data directory changed");
            }
            if store.path.join(format!("cancel-{id}")).exists() {
                break ("cancelled", None);
            }
            if overflow.load(Ordering::Acquire) {
                break ("output_limit", None);
            }
            if began.elapsed() >= Duration::from_millis(job.action.timeout_ms) {
                break ("timed_out", None);
            }
            if let Some(exit) = child.try_wait()? {
                let drain = Instant::now();
                while !(stdout_done.load(Ordering::Acquire) && stderr_done.load(Ordering::Acquire))
                    && drain.elapsed() < Duration::from_millis(100)
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                break (
                    if exit.success() {
                        "succeeded"
                    } else {
                        "failed"
                    },
                    exit.code(),
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        if overflow.load(Ordering::Acquire) {
            status = "output_limit";
        }
        let output = json!({"stdout":String::from_utf8_lossy(&stdout.lock().unwrap()).to_string(),"stderr":String::from_utf8_lossy(&stderr.lock().unwrap()).to_string(),"truncated":overflow.load(Ordering::Acquire)});
        Ok((status.into(), exit, output))
    })();
    match result {
        Ok((status, exit, output)) => {
            store.write(&format!("output-{id}.json"), &serde_json::to_vec(&output)?)?;
            job.outcome = Some(status);
            job.exit_code = exit;
        }
        Err(error) => {
            job.outcome = Some("failed".into());
            job.error = Some(format!("{error:#}"));
        }
    }
    job.status = "finishing".into();
    save(&store, &job)?;
    // Cleanup drops before this function can return, removing only this job's
    // private group. API/service loss never kills retained terminal/app groups.
    drop(_cleanup);
    Ok(())
}
