//! Independent, read-only collection for retained terminals with no open views.
use crate::{backend_snapshot, cold_start, project_state::ProjectState, runtime::Config};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawFd, FromRawFd},
        process::CommandExt,
    },
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// Bump when a deployed collector must hand off to changed collector behavior.
// Version 2 accepts the valid wrap-pending cursor in background screen captures.
// Older, already-running collectors must retire to pick up that validation fix.
const VERSION: u64 = 2;
#[derive(Serialize, Deserialize)]
struct Request {
    version: u64,
    #[serde(default)]
    stop: bool,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Status {
    version: u64,
    pid: u32,
    start_ticks: u64,
    boot_id: Option<String>,
    scans: u64,
    captures: u64,
    errors: u64,
    in_flight: usize,
    last_scan_ms: u64,
    covered: usize,
    registered: usize,
}
struct Paths {
    directory: File,
    root: PathBuf,
    lock: PathBuf,
    request: PathBuf,
    status: PathBuf,
    projects: PathBuf,
    stamp: (u64, u64),
}
impl Paths {
    fn new() -> Result<Self> {
        let root = crate::runtime::root()?;
        Self::at(root)
    }
    fn at(root: PathBuf) -> Result<Self> {
        let directory = File::open(&root)?;
        let projects = crate::project_state::data_path();
        let hash = format!("{:x}", Sha256::digest(projects.as_os_str().as_bytes()));
        let prefix = format!("snapshot-{}", &hash[..16]);
        let meta = directory.metadata()?;
        ensure!(
            meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
            "Invalid snapshot runtime directory"
        );
        Ok(Self {
            directory,
            lock: root.join(format!("{prefix}.lock")),
            request: root.join(format!("{prefix}.request.json")),
            status: root.join(format!("{prefix}.status.json")),
            root,
            projects,
            stamp: (meta.dev(), meta.ino()),
        })
    }
    fn current(&self) -> bool {
        fs::metadata(&self.root).is_ok_and(|m| (m.dev(), m.ino()) == self.stamp)
            && self.projects.is_dir()
    }
    fn lock(&self) -> Result<File> {
        Ok(fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&self.lock)?)
    }
    fn publish(&self, status: &Status) -> Result<()> {
        let name = std::ffi::CString::new(
            self.status
                .file_name()
                .context("No collector status name")?
                .as_bytes(),
        )?;
        let temp = std::ffi::CString::new(format!(".snapshot-{}.tmp", std::process::id()))?;
        let dir = self.directory.as_raw_fd();
        let fd = unsafe {
            libc::openat(
                dir,
                temp.as_ptr(),
                libc::O_CREAT | libc::O_TRUNC | libc::O_WRONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        ensure!(
            fd >= 0,
            "Cannot write snapshot collector status: {}",
            std::io::Error::last_os_error()
        );
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(&serde_json::to_vec(status)?)?;
        file.sync_all()?;
        ensure!(
            unsafe { libc::renameat(dir, temp.as_ptr(), dir, name.as_ptr()) } == 0,
            "Cannot commit snapshot collector status: {}",
            std::io::Error::last_os_error()
        );
        self.directory.sync_all()?;
        Ok(())
    }
}
fn read<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<T> {
    let mut bytes = Vec::new();
    File::open(path)?.take(8193).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 8192,
        "Snapshot collector metadata exceeds its limit"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
fn alive(s: &Status) -> bool {
    s.boot_id == crate::agent_state::boot_id().map(|b| b.to_string())
        && crate::agent_state::process_start(s.pid) == Some(s.start_ticks)
        && fs::read(format!("/proc/{}/cmdline", s.pid)).is_ok_and(|args| {
            args.split(|b| *b == 0)
                .any(|arg| arg == b"--snapshot-worker")
        })
}
pub fn ensure() -> Result<()> {
    if std::env::var("STARFLUX_SNAPSHOT_SERVICE").as_deref() == Ok("off")
        || !crate::project_state::data_path().is_dir()
    {
        return Ok(());
    }
    let paths = Paths::new()?;
    let lock = paths.lock()?;
    let available = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if !available {
        if let Ok(status) = read::<Status>(&paths.status)
            && alive(&status)
            && status.version >= VERSION
        {
            return Ok(());
        }
        let request = read::<Request>(&paths.request).ok();
        let recent = fs::metadata(&paths.request)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(30));
        if request.is_some_and(|r| !r.stop && r.version >= VERSION) && recent {
            return Ok(());
        }
    }
    let requested = read::<Request>(&paths.request)
        .ok()
        .map_or(VERSION, |r| r.version.max(VERSION));
    if requested > VERSION {
        return Ok(());
    }
    crate::automation::persist(
        &paths.request,
        &serde_json::to_vec(&Request {
            version: VERSION,
            stop: false,
        })?,
    )?;
    drop(lock);
    Command::new(std::env::current_exe()?)
        .arg("--snapshot-worker")
        .arg(&paths.root)
        .arg(format!("{}:{}", paths.stamp.0, paths.stamp.1))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .context("Start background snapshot collector")?;
    Ok(())
}
pub fn status() -> Result<Value> {
    let paths = Paths::new()?;
    match read::<Status>(&paths.status) {
        Ok(s) => Ok(json!({"running":alive(&s),"status":s})),
        Err(_) if !paths.status.exists() => Ok(json!({"running":false,"status":null})),
        Err(e) => Err(e),
    }
}
type Key = (String, usize);
fn bounded_bytes(path: &std::path::Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Saved metadata exceeds its limit"
    );
    Ok(bytes)
}
struct Entry {
    config: Config,
    pane: usize,
    epoch: Option<String>,
    signature: String,
    endpoint: String,
    next: Instant,
}
struct Job {
    config: Config,
    pane: usize,
    epoch: Option<String>,
    signature: String,
}
struct Done {
    key: Key,
    signature: String,
    live: bool,
    saved: bool,
    error: bool,
}
fn ready<'a>(
    entries: &'a BTreeMap<Key, Entry>,
    busy: &BTreeSet<Key>,
    covered: &BTreeSet<Key>,
    now: Instant,
) -> Vec<(&'a Key, &'a Entry)> {
    if busy.len() >= 2 {
        return Vec::new();
    }
    let mut due: Vec<_> = entries
        .iter()
        .filter(|(key, entry)| !busy.contains(*key) && !covered.contains(*key) && entry.next <= now)
        .collect();
    due.sort_by(|a, b| a.1.next.cmp(&b.1.next).then_with(|| a.0.cmp(b.0)));
    due.truncate(2 - busy.len());
    due
}
pub fn endpoint(config: &Config) -> Result<String> {
    let value = serde_json::to_value(config)?;
    let source: Vec<_> = [
        "host",
        "ssh_config",
        "project_host",
        "herdr",
        "tmux",
        "adopted",
    ]
    .iter()
    .map(|key| &value[*key])
    .collect();
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&source)?)
    ))
}
fn coverage(paths: &Paths, entries: &BTreeMap<Key, Entry>) -> BTreeSet<Key> {
    let mut covered = BTreeSet::new();
    let Ok(files) = fs::read_dir(&paths.root) else {
        return covered;
    };
    for file in files.flatten() {
        let name = file.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("v-") || !name.ends_with(".state.json") {
            continue;
        }
        if file
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(15))
        {
            continue;
        }
        let Ok(bytes) = bounded_bytes(&file.path(), 8 * 1024 * 1024) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let now = crate::runtime::monotonic_ns();
        if !value["controller_observed_ns"]
            .as_u64()
            .is_some_and(|time| time <= now && now - time < 15_000_000_000)
        {
            continue;
        }
        let Some(pid) = value["controller_pid"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
        else {
            continue;
        };
        if value["controller_boot_id"].as_str()
            != crate::agent_state::boot_id()
                .map(|b| b.to_string())
                .as_deref()
            || value["controller_start_ticks"].as_u64() != crate::agent_state::process_start(pid)
        {
            continue;
        }
        let Some(frontend) = value["frontend"].as_str() else {
            continue;
        };
        let Ok(args) = fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        let args: Vec<_> = args.split(|b| *b == 0).collect();
        if !args.contains(&b"--serve-view".as_slice()) || !args.contains(&frontend.as_bytes()) {
            continue;
        }
        for item in value["snapshot_coverage"].as_array().into_iter().flatten() {
            let (Some(project), Some(pane)) = (
                item["project"].as_str(),
                item["pane"].as_u64().and_then(|p| usize::try_from(p).ok()),
            ) else {
                continue;
            };
            let key = (project.to_owned(), pane);
            if let Some(entry) = entries.get(&key)
                && item["endpoint"].as_str() == Some(entry.endpoint.as_str())
                && item["epoch"].as_str() == entry.epoch.as_deref()
            {
                covered.insert(key);
            }
        }
    }
    covered
}
fn scan(paths: &Paths, entries: &mut BTreeMap<Key, Entry>) -> Result<()> {
    let mut retained = BTreeSet::new();
    for file in fs::read_dir(&paths.projects)? {
        let path = file?.path();
        if !crate::runtime::config_file(&path) {
            continue;
        }
        let Ok(bytes) = bounded_bytes(&path, 1024 * 1024) else {
            continue;
        };
        let Ok(config) = serde_json::from_slice::<Config>(&bytes) else {
            continue;
        };
        let removed = bounded_bytes(
            &paths
                .projects
                .join(format!("{}.worktree.json", config.workspace)),
            8192,
        )
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_some_and(|v| v["state"] == "removed");
        if config.validate().is_err() || removed {
            continue;
        }
        let model = paths
            .projects
            .join(format!("{}.model.json", config.workspace));
        let model = if model.exists() {
            model
        } else {
            paths
                .projects
                .join(format!("{}.layout.json", config.workspace))
        };
        let Ok(bytes) = bounded_bytes(&model, 16 * 1024 * 1024) else {
            continue;
        };
        let Ok(state) = serde_json::from_slice::<ProjectState>(&bytes) else {
            continue;
        };
        if !state.save_terminal_screens || state.validate().is_err() {
            continue;
        }
        let endpoint = endpoint(&config)?;
        for pane in state
            .terminal_ids()
            .into_iter()
            .map(|id| config.canonical_pane(id))
        {
            let key = (config.workspace.clone(), pane);
            retained.insert(key.clone());
            let epoch = state.terminal_epochs.get(&pane).cloned();
            let signature = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&(&config, &epoch))?)
            );
            if entries.get(&key).is_some_and(|e| e.signature == signature) {
                continue;
            }
            entries.insert(
                key,
                Entry {
                    config: config.clone(),
                    pane,
                    epoch,
                    signature,
                    endpoint: endpoint.clone(),
                    next: Instant::now(),
                },
            );
        }
    }
    entries.retain(|key, _| retained.contains(key));
    Ok(())
}
pub fn run(root: &str, stamp: &str) -> Result<()> {
    if !crate::project_state::data_path().is_dir() {
        return Ok(());
    }
    let Ok(paths) = Paths::at(PathBuf::from(root)) else {
        return Ok(());
    };
    if format!("{}:{}", paths.stamp.0, paths.stamp.1) != stamp {
        return Ok(());
    }
    let paths = Arc::new(paths);
    let lock = paths.lock()?;
    let began = Instant::now();
    while unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        if began.elapsed() > Duration::from_secs(20) || !paths.current() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !paths.current() {
        return Ok(());
    }
    let request: Request = read(&paths.request)?;
    if request.stop || request.version > VERSION {
        return Ok(());
    }
    let mut status = Status {
        version: VERSION,
        pid: std::process::id(),
        start_ticks: crate::agent_state::process_start(std::process::id())
            .context("Snapshot collector process identity unavailable")?,
        boot_id: crate::agent_state::boot_id().map(|b| b.to_string()),
        ..Default::default()
    };
    paths.publish(&status)?;
    let (jobs, queue) = crossbeam_channel::bounded::<Job>(2);
    let (done, results) = crossbeam_channel::unbounded::<Done>();
    let mut workers = Vec::new();
    for _ in 0..2 {
        let queue = queue.clone();
        let done = done.clone();
        let paths = paths.clone();
        workers.push(std::thread::spawn(move || {
            for job in queue {
                let mut live = false;
                let mut saved = false;
                let started = crate::runtime::monotonic_ns();
                let result = (|| -> Result<()> {
                    if !paths.current() {
                        return Ok(());
                    }
                    let mut health = backend_snapshot::inspect(&job.config, job.pane)?;
                    live = health["alive"] == true;
                    if !live || !paths.current() {
                        return Ok(());
                    }
                    ensure!(
                        health["screen_error"].is_null(),
                        "Terminal screen capture is unavailable"
                    );
                    if let Some(screen) = backend_snapshot::take(
                        &job.config,
                        job.pane,
                        job.epoch.clone(),
                        &mut health,
                        started,
                    )? && paths.current()
                    {
                        cold_start::save(&job.config, &screen)?;
                        saved = true;
                    }
                    Ok(())
                })();
                let _ = done.send(Done {
                    key: (job.config.workspace, job.pane),
                    signature: job.signature,
                    live,
                    saved,
                    error: result.is_err(),
                });
            }
        }));
    }
    drop(done);
    drop(queue);
    let mut entries: BTreeMap<Key, Entry> = BTreeMap::new();
    let mut busy: BTreeSet<Key> = BTreeSet::new();
    let mut covered = BTreeSet::new();
    let mut scanned = Instant::now() - Duration::from_secs(10);
    let mut published = scanned;
    let mut last_live = Instant::now();
    while paths.current() {
        if read::<Request>(&paths.request).is_ok_and(|r| r.stop || r.version > VERSION) {
            break;
        }
        for result in results.try_iter() {
            busy.remove(&result.key);
            status.captures += u64::from(result.saved);
            status.errors += u64::from(result.error);
            if result.live {
                last_live = Instant::now();
            }
            if let Some(entry) = entries.get_mut(&result.key)
                && entry.signature == result.signature
            {
                entry.next =
                    Instant::now() + Duration::from_secs(if result.live { 10 } else { 60 });
            }
        }
        if scanned.elapsed() >= Duration::from_secs(5) {
            if scan(&paths, &mut entries).is_err() {
                status.errors += 1;
            }
            covered = coverage(&paths, &entries);
            status.covered = covered.len();
            status.registered = entries.len();
            if !covered.is_empty() {
                last_live = Instant::now();
            }
            status.scans += 1;
            status.last_scan_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
            scanned = Instant::now();
        }
        for (key, entry) in ready(&entries, &busy, &covered, Instant::now()) {
            if jobs
                .try_send(Job {
                    config: entry.config.clone(),
                    pane: entry.pane,
                    epoch: entry.epoch.clone(),
                    signature: entry.signature.clone(),
                })
                .is_ok()
            {
                busy.insert(key.clone());
            }
        }
        status.in_flight = busy.len();
        if published.elapsed() >= Duration::from_secs(5) {
            paths.publish(&status)?;
            published = Instant::now();
        }
        if busy.is_empty() && last_live.elapsed() > Duration::from_secs(60) {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    drop(jobs);
    for worker in workers {
        let _ = worker.join();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slow_captures_cannot_starve_later_registered_panes() {
        let began = Instant::now();
        let config: Config = serde_json::from_value(json!({
            "workspace":"fixture", "executable":"starflux"
        }))
        .unwrap();
        let mut entries = BTreeMap::new();
        for pane in 0..12 {
            entries.insert(
                ("fixture".into(), pane),
                Entry {
                    config: config.clone(),
                    pane,
                    epoch: None,
                    signature: String::new(),
                    endpoint: String::new(),
                    next: began,
                },
            );
        }
        let mut seen = BTreeSet::new();
        // Two workers each need four seconds, then wait ten before retrying.
        // Alphabetical scheduling would repeatedly revisit the early panes.
        for round in 0..6 {
            let now = began + Duration::from_secs(round * 4);
            let keys: Vec<_> = ready(&entries, &BTreeSet::new(), &BTreeSet::new(), now)
                .into_iter()
                .map(|(key, _)| key.clone())
                .collect();
            assert_eq!(keys.len(), 2);
            for key in keys {
                seen.insert(key.clone());
                entries.get_mut(&key).unwrap().next = now + Duration::from_secs(14);
            }
        }
        assert_eq!(seen.len(), 12);
        let busy = entries.keys().take(2).cloned().collect();
        assert!(
            ready(
                &entries,
                &busy,
                &BTreeSet::new(),
                began + Duration::from_secs(100)
            )
            .is_empty()
        );
    }
}
