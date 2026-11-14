//! Durable category review ownership. Providers never receive a project terminal.
use crate::{
    category_agent::{self, Profile},
    runtime::Config,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Runtime {
    pub active: Option<String>,
    pub history: Vec<String>,
    pub capabilities: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Review {
    pub version: u32,
    pub id: String,
    pub project: String,
    pub category: String,
    pub mode: String,
    pub prompt: String,
    pub profile: Profile,
    pub configuration_revision: u64,
    pub membership_revision: u64,
    pub scope: String,
    pub status: String,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub pid: Option<u32>,
    pub start_ticks: Option<u64>,
    pub boot: String,
    pub text: String,
    pub result: Value,
    pub sources: Vec<Value>,
    #[serde(default)]
    pub evidence_bytes: usize,
    pub error: Option<String>,
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn key(id: &str) -> Result<String> {
    Ok(uuid::Uuid::parse_str(id)?.simple().to_string())
}
fn job_path(root: &Path, id: &str) -> Result<PathBuf> {
    Ok(root.join(format!("review-{}.json", key(id)?)))
}
fn runtime(root: &Path) -> Result<Runtime> {
    if !root.join("runtime.json").exists() {
        return Ok(Runtime::default());
    }
    crate::plugin_store::read(&root.join("runtime.json"), 2 * 1024 * 1024)
}
fn write_runtime(root: &Path, state: &Runtime) -> Result<()> {
    crate::automation::persist(&root.join("runtime.json"), &serde_json::to_vec(state)?)
}
fn save(root: &Path, review: &Review) -> Result<()> {
    crate::automation::persist(&job_path(root, &review.id)?, &serde_json::to_vec(review)?)
}
fn load(root: &Path, id: &str) -> Result<Review> {
    crate::plugin_store::read(&job_path(root, id)?, 4 * 1024 * 1024)
}
fn running(r: &Review) -> bool {
    matches!(r.status.as_str(), "starting" | "running" | "cancelling")
}
fn alive(r: &Review) -> bool {
    r.boot
        == crate::agent_state::boot_id()
            .unwrap_or_default()
            .to_string()
        && r.pid.is_some_and(|pid| {
            crate::agent_state::process_start(pid) == r.start_ticks
                && fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|v| {
                    v.split(|b| *b == 0)
                        .any(|a| a == b"--category-agent-worker")
                })
        })
}
pub fn scope(config: &Config, category: &str) -> Result<String> {
    let mut members = category_agent::members(config, category)?;
    members.sort_by(|a, b| a.workspace.cmp(&b.workspace));
    Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(&members.iter().map(|p|json!({"id":p.workspace,"host":p.host,"directory":p.directory,"executable":p.executable,"ssh_config":p.ssh_config})).collect::<Vec<_>>())?)))
}
pub fn snapshot(config: &Config, category: &str) -> Result<Value> {
    snapshot_page(config, category, 0)
}
pub fn snapshot_page(config: &Config, category: &str, offset: usize) -> Result<Value> {
    let root = category_agent::read_root(config, category)?;
    if !root.exists() {
        return Ok(json!({"history":[],"capabilities":null}));
    }
    let rt = runtime(&root)?;
    let current_scope = scope(config, category).ok();
    let mut reviews = Vec::new();
    for id in rt.history.iter().rev().skip(offset).take(6) {
        if let Ok(mut r) = load(&root, id) {
            if running(&r) && r.created_ms + 15000 < now() && !alive(&r) {
                r.status = "indeterminate".into();
                r.error = Some("The review worker stopped; no automatic retry was made".into());
            }
            r.text = r
                .text
                .chars()
                .rev()
                .take(12000)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            let mut value = serde_json::to_value(&r)?;
            value["stale"] = json!(
                Some(&r.scope) != current_scope.as_ref()
                    || r.configuration_revision != category_agent::load_path(&root)?.revision
            );
            reviews.push(value);
        }
    }
    Ok(
        json!({"active":rt.active,"history":reviews,"total":rt.history.len(),"offset":offset,"capabilities":rt.capabilities}),
    )
}
pub fn revoke(root: &Path) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    let rt = runtime(root)?;
    if let Some(id) = rt
        .active
        .filter(|id| load(root, id).is_ok_and(|r| running(&r)))
    {
        crate::automation::persist(&root.join(format!("cancel-{}", key(&id)?)), b"1")?;
    }
    Ok(())
}
pub fn cancel(config: &Config, category: &str) -> Result<Value> {
    let root = category_agent::root_path(config, category)?;
    revoke(&root)?;
    snapshot(config, category)
}
pub fn start(config: &Config, category: &str, mode: &str, prompt: &str) -> Result<Value> {
    ensure!(
        matches!(mode, "connect" | "review" | "chat"),
        "Unknown portfolio operation"
    );
    ensure!(
        prompt.len() <= 32768 && !prompt.contains('\0'),
        "Portfolio message exceeds its limit"
    );
    let root = category_agent::root_path(config, category)?;
    starkit::terminal_graphics::session::private_root(&root)?;
    let _configuration = crate::project_sync::Transaction::acquire(root.join("coordinator.lock"))?;
    let _lock = crate::project_sync::Transaction::acquire(root.join("runtime.lock"))?;
    let mut state = category_agent::load_path(&root)?;
    ensure!(
        state.profile.enabled,
        "Enable the portfolio coordinator first"
    );
    if mode != "connect" {
        ensure!(
            state
                .profile
                .model
                .as_ref()
                .is_some_and(|m| !m.trim().is_empty()),
            "Connect the provider and choose a model before starting a review"
        );
    }
    let mut rt = runtime(&root)?;
    if let Some(active) = &rt.active {
        let old = load(&root, active)?;
        ensure!(
            !running(&old) || (!alive(&old) && old.created_ms + 15000 < now()),
            "A portfolio review is already running in another window"
        );
    }
    let current_scope = scope(config, category)?;
    let previous_scope = rt
        .history
        .iter()
        .rev()
        .filter_map(|id| load(&root, id).ok())
        .find(|r| r.mode != "connect")
        .map(|r| r.scope);
    if previous_scope
        .as_ref()
        .is_some_and(|previous| previous != &current_scope)
    {
        if let Some(conversation) = state.conversation.take() {
            state.archived_conversations.push(json!({"provider":state.profile.provider,"conversation":conversation,"reason":"category_scope_changed"}));
        }
        state.revision = state
            .revision
            .checked_add(1)
            .context("Coordinator revision exhausted")?;
        crate::automation::persist(&root.join("coordinator.json"), &serde_json::to_vec(&state)?)?;
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let r = Review {
        version: 1,
        id: id.clone(),
        project: config.workspace.clone(),
        category: category.into(),
        mode: mode.into(),
        prompt: prompt.into(),
        profile: state.profile,
        configuration_revision: state.revision,
        membership_revision: crate::categories::Catalog::load(config)?.revision,
        scope: scope(config, category)?,
        status: "starting".into(),
        created_ms: now(),
        updated_ms: now(),
        pid: None,
        start_ticks: None,
        boot: crate::agent_state::boot_id()
            .context("Boot identity unavailable")?
            .to_string(),
        text: String::new(),
        result: Value::Null,
        sources: vec![],
        evidence_bytes: 0,
        error: None,
    };
    save(&root, &r)?;
    rt.active = Some(id.clone());
    rt.history.push(id.clone());
    write_runtime(&root, &rt)?;
    let executable = std::env::current_exe()?;
    let mut child = Command::new(executable)
        .args(["--category-agent-worker", &config.workspace, category, &id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(json!({"id":id,"status":"starting"}))
}
fn valid(config: &Config, r: &Review) -> Result<()> {
    let root = category_agent::root_path(config, &r.category)?;
    let state = category_agent::load_path(&root)?;
    ensure!(
        state.profile.enabled && state.revision == r.configuration_revision,
        "Portfolio configuration changed or was disabled"
    );
    ensure!(
        scope(config, &r.category)? == r.scope,
        "Portfolio membership or repository identity changed"
    );
    ensure!(
        !root.join(format!("cancel-{}", r.id)).exists(),
        "Review cancelled"
    );
    Ok(())
}
pub fn tool(config: &Config, r: &mut Review, arguments: &Value) -> Result<Value> {
    valid(config, r)?;
    let project = arguments["project"]
        .as_str()
        .context("Tool requires a member project")?;
    let member = category_agent::members(config, &r.category)?
        .into_iter()
        .find(|p| p.workspace == project)
        .context("Project is outside the portfolio")?;
    let operation = arguments["operation"]
        .as_str()
        .context("Missing repository operation")?;
    let path = arguments["path"].as_str().unwrap_or("");
    let pattern = arguments["pattern"].as_str().unwrap_or("");
    let value = crate::category_agent_files::query(&member, operation, path, pattern)?;
    valid(config, r)?;
    let bytes = serde_json::to_vec(&value)?.len();
    ensure!(
        bytes <= 256 * 1024,
        "Repository response exceeds 256 KiB; narrow the query"
    );
    r.evidence_bytes = r.evidence_bytes.saturating_add(bytes);
    ensure!(
        r.evidence_bytes <= 2 * 1024 * 1024,
        "Portfolio review evidence budget reached"
    );
    for item in value["sources"].as_array().into_iter().flatten() {
        let source = json!({"project":project,"operation":"read","path":item["path"],"sha256":item["sha256"]});
        if !r.sources.contains(&source) {
            ensure!(r.sources.len() < 128, "Review evidence limit reached");
            r.sources.push(source);
        }
    }
    if let Some(hash) = value["sha256"].as_str() {
        let source = json!({"project":project,"operation":operation,"path":path,"sha256":hash});
        if !r.sources.contains(&source) {
            ensure!(r.sources.len() < 128, "Review evidence limit reached");
            r.sources.push(source);
        }
    }
    Ok(value)
}
fn assessment(value: &Value, config: &Config, r: &Review) -> Result<Value> {
    let projects = category_agent::members(config, &r.category)?
        .iter()
        .map(|p| p.workspace.clone())
        .collect::<Vec<_>>();
    ensure!(
        value["summary"].as_str().is_some(),
        "Provider did not return a structured portfolio assessment"
    );
    let mut value = value.clone();
    for item in value["findings"].as_array().into_iter().flatten() {
        ensure!(
            item["project"]
                .as_str()
                .is_some_and(|p| projects.iter().any(|id| id == p)),
            "Finding names a project outside this portfolio"
        );
    }
    for item in value["projects"].as_array().into_iter().flatten() {
        ensure!(
            item["project"]
                .as_str()
                .is_some_and(|p| projects.iter().any(|id| id == p)),
            "Assessment names a project outside the portfolio"
        );
        ensure!(
            item["progress"].is_null() || item["progress"].as_u64().is_some_and(|n| n <= 100),
            "Invalid assessed progress"
        );
    }
    if let Some(proposals) = value["proposals"].as_array_mut() {
        ensure!(proposals.len() <= 64, "Too many proposed tasks");
        let mut keys = std::collections::BTreeMap::new();
        for (index, p) in proposals.iter().enumerate() {
            let key = p["key"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| format!("task-{index}"));
            ensure!(
                keys.insert(key, uuid::Uuid::new_v4().simple().to_string())
                    .is_none(),
                "Duplicate proposal dependency key"
            );
        }
        for (index, proposal) in proposals.iter_mut().enumerate() {
            ensure!(
                proposal["project"]
                    .as_str()
                    .is_some_and(|p| projects.iter().any(|id| id == p)),
                "Proposed task names a project outside this portfolio"
            );
            ensure!(
                proposal["text"]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty()
                        && s.len() <= 32768
                        && !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t')),
                "Invalid task text"
            );
            let key = proposal["key"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| format!("task-{index}"));
            proposal["id"] = json!(keys[&key]);
            let mut dependencies = Vec::new();
            for dependency in proposal["depends_on"].as_array().into_iter().flatten() {
                let name = dependency.as_str().context("Invalid task dependency")?;
                ensure!(name != key, "A task cannot depend on itself");
                dependencies.push(
                    keys.get(name)
                        .context("Task dependency is outside this assessment")?
                        .clone(),
                );
            }
            proposal["depends_on"] = json!(dependencies);
            proposal["status"] = json!("proposed");
        }
    }
    if let Some(proposals) = value["proposals"].as_array() {
        let mut done = std::collections::BTreeSet::new();
        for _ in 0..proposals.len() {
            for p in proposals {
                if p["depends_on"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .all(|d| d.as_str().is_some_and(|id| done.contains(id)))
                    && let Some(id) = p["id"].as_str()
                {
                    done.insert(id.to_owned());
                }
            }
        }
        ensure!(
            done.len() == proposals.len(),
            "Proposed task dependencies contain a cycle"
        );
    }
    ensure!(
        serde_json::to_vec(&value)?.len() <= 128 * 1024,
        "Assessment exceeds 128 KiB"
    );
    Ok(value)
}
pub fn worker(project: &str, category: &str, id: &str) -> Result<()> {
    let config = crate::saved(project)?;
    let root = category_agent::root_path(&config, category)?;
    let _claim = crate::project_sync::Transaction::acquire(root.join("review.lock"))?;
    let mut r = load(&root, id)?;
    if r.status != "starting" {
        return Ok(());
    }
    r.pid = Some(std::process::id());
    r.start_ticks = crate::agent_state::process_start(std::process::id());
    r.status = "running".into();
    save(&root, &r)?;
    let result = (|| -> Result<()> {
        valid(&config, &r)?;
        let workspace = root.join("workspace");
        starkit::terminal_graphics::session::private_root(&workspace)?;
        let mut child = Command::new("python3")
            .args(["-u", "-c", include_str!("../scripts/category-provider.py")])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        let began = Instant::now();
        let result = (|| -> Result<()> {
            let mut stdin = child.stdin.take().context("Provider input unavailable")?;
            let state = category_agent::load_path(&root)?;
            let mut context = if r.mode == "connect" {
                json!({})
            } else {
                category_agent::evidence(&config, category)?
            };
            for project in context["projects"].as_array().into_iter().flatten() {
                if project["coverage"]["sha256"].is_string() {
                    r.sources.push(json!({"project":project["id"],"operation":"git","path":"","sha256":project["coverage"]["sha256"]}));
                }
            }
            if r.mode != "connect" {
                let members = category_agent::members(&config, category)?
                    .iter()
                    .map(|p| p.workspace.clone())
                    .collect::<Vec<_>>();
                let mut ledger = Vec::new();
                for id in runtime(&root)?.history.iter().rev().take(20) {
                    if let Ok(old) = load(&root, id) {
                        for p in old.result["proposals"].as_array().into_iter().flatten() {
                            if p["project"]
                                .as_str()
                                .is_some_and(|id| members.iter().any(|p| p == id))
                            {
                                ledger.push(json!({"id":p["id"],"project":p["project"],"title":p["title"],"status":p["status"],"depends_on":p["depends_on"]}));
                            }
                        }
                    }
                }
                context["work_queue"] = json!(ledger.into_iter().take(128).collect::<Vec<_>>());
            }
            valid(&config, &r)?;
            let bootstrap = json!({"version":1,"mode":r.mode,"profile":r.profile,"workspace":workspace,"conversation":state.conversation,"prompt":r.prompt,"context":context,"worker_pid":std::process::id(),"worker_start":r.start_ticks});
            serde_json::to_writer(&mut stdin, &bootstrap)?;
            stdin.write_all(b"\n")?;
            stdin.flush()?;
            let stdout = child.stdout.take().context("Provider output unavailable")?;
            let (tx, rx) = crossbeam_channel::bounded(32);
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut line = String::new();
                    use std::io::Read;
                    let result = reader.by_ref().take(1024 * 1024 + 1).read_line(&mut line);
                    match result {
                        Ok(0) => break,
                        Ok(_) if line.len() <= 1024 * 1024 => {
                            if tx
                                .send(
                                    serde_json::from_str::<Value>(&line)
                                        .map_err(|_| "Invalid provider event".to_owned()),
                                )
                                .is_err()
                            {
                                break;
                            }
                        }
                        _ => {
                            let _ = tx.send(Err("Provider event exceeds its limit".into()));
                            break;
                        }
                    }
                }
            });
            let mut complete = false;
            let mut last_save = Instant::now();
            let mut dirty = false;
            loop {
                valid(&config, &r)?;
                ensure!(
                    began.elapsed()
                        < Duration::from_secs(if r.mode == "connect" { 60 } else { 1800 }),
                    "Portfolio operation timed out"
                );
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(event) => {
                        dirty = true;
                        let event = event.map_err(anyhow::Error::msg)?;
                        match event["type"].as_str().unwrap_or("") {
                            "metadata" => {
                                let _guard = crate::project_sync::Transaction::acquire(
                                    root.join("runtime.lock"),
                                )?;
                                valid(&config, &r)?;
                                let mut rt = runtime(&root)?;
                                rt.capabilities = event["capabilities"].clone();
                                write_runtime(&root, &rt)?;
                            }
                            "conversation" => {
                                valid(&config, &r)?;
                                let _guard = crate::project_sync::Transaction::acquire(
                                    root.join("coordinator.lock"),
                                )?;
                                let mut state = category_agent::load_path(&root)?;
                                ensure!(
                                    state.revision == r.configuration_revision,
                                    "Provider configuration changed"
                                );
                                state.conversation = event["id"].as_str().map(String::from);
                                crate::automation::persist(
                                    &root.join("coordinator.json"),
                                    &serde_json::to_vec(&state)?,
                                )?;
                            }
                            "delta" => {
                                r.text.push_str(event["text"].as_str().unwrap_or(""));
                                ensure!(
                                    r.text.len() <= 512 * 1024,
                                    "Review response exceeds 512 KiB"
                                );
                            }
                            "tool_call" => {
                                let result = tool(&config, &mut r, &event["arguments"]);
                                let reply = match result {
                                    Ok(v) => json!({"id":event["id"],"result":v}),
                                    Err(e) => json!({"id":event["id"],"error":format!("{e:#}")}),
                                };
                                serde_json::to_writer(&mut stdin, &reply)?;
                                stdin.write_all(b"\n")?;
                                stdin.flush()?;
                            }
                            "complete" => {
                                if r.mode != "connect" {
                                    r.result = assessment(&event["result"], &config, &r)?;
                                }
                                complete = true;
                                break;
                            }
                            "error" => anyhow::bail!(
                                "{}",
                                event["message"]
                                    .as_str()
                                    .unwrap_or("Portfolio provider failed")
                            ),
                            _ => anyhow::bail!("Unknown provider event"),
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(_) => break,
                }
                if dirty && last_save.elapsed() > Duration::from_millis(150) {
                    r.updated_ms = now();
                    save(&root, &r)?;
                    last_save = Instant::now();
                    dirty = false;
                }
            }
            ensure!(
                complete,
                "Provider disconnected before reporting a result; outcome is indeterminate"
            );
            Ok(())
        })();
        // The unreaped leader reserves its private process-group identity.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGTERM);
        }
        for _ in 0..10 {
            if child.try_wait()?.is_some() {
                return result;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.wait();
        result
    })();
    r.updated_ms = now();
    match result {
        Ok(()) => r.status = "completed".into(),
        Err(e) => {
            let message = format!("{e:#}");
            r.status = if valid(&config, &r).is_err() {
                "cancelled"
            } else if message.contains("indeterminate") || message.contains("disconnected") {
                "indeterminate"
            } else {
                "failed"
            }
            .into();
            r.error = Some(message);
        }
    }
    save(&root, &r)?;
    Ok(())
}

fn api(method: &str, params: Value) -> Result<Value> {
    let response = crate::automation::execute(&crate::automation::Request {
        version: 1,
        id: json!("category-agent"),
        method: method.into(),
        params,
        operation_id: None,
        deadline_ms: 10000,
    });
    ensure!(
        response.get("error").is_none(),
        "{}",
        response["error"]["message"]
            .as_str()
            .unwrap_or("Project agent is unavailable")
    );
    Ok(response["result"].clone())
}
pub fn targets(config: &Config, category: &str, project: &str) -> Result<Value> {
    ensure!(
        category_agent::members(config, category)?
            .iter()
            .any(|p| p.workspace == project),
        "Project is outside the portfolio"
    );
    let member = crate::saved(project)?;
    let mut choices = Vec::new();
    for record in crate::agent_state::records(&member)? {
        let observed = api("panes.get", json!({"project":project,"pane":record.pane+1}))?;
        if observed["target"]["run_id"] == record.report.run_id {
            choices.push(json!({"pane":record.pane+1,"target":observed["target"],"conversation":record.report.conversation_id,"native_queue":record.report.harness=="codex" && member.herdr.is_none() && member.tmux.is_none(),"provider":record.report.harness,"state":record.report.state}));
        }
    }
    Ok(json!({"targets":choices}))
}
fn verify_sources(config: &Config, category: &str, sources: &[Value]) -> Result<()> {
    let members = category_agent::members(config, category)?;
    for source in sources {
        let member = members
            .iter()
            .find(|p| p.workspace == source["project"])
            .context("Evidence project left the portfolio")?;
        let current = crate::category_agent_files::query(
            member,
            source["operation"].as_str().unwrap_or("read"),
            source["path"].as_str().unwrap_or(""),
            "",
        )?;
        ensure!(
            current["sha256"] == source["sha256"],
            "Repository evidence changed; run a new portfolio review"
        );
    }
    Ok(())
}
pub fn approve(
    config: &Config,
    category: &str,
    review: &str,
    proposal: &str,
    target: &Value,
    conversation: &str,
) -> Result<Value> {
    let root = category_agent::root_path(config, category)?;
    let _lock = crate::project_sync::Transaction::acquire(root.join("runtime.lock"))?;
    let mut r = load(&root, review)?;
    valid(config, &r)?;
    ensure!(
        r.status == "completed",
        "Only completed assessments can propose a handoff"
    );
    for source in &r.sources {
        let member = category_agent::members(config, category)?
            .into_iter()
            .find(|p| p.workspace == source["project"])
            .context("Evidence project left this portfolio")?;
        let current = crate::category_agent_files::query(
            &member,
            source["operation"].as_str().unwrap_or("read"),
            source["path"].as_str().unwrap_or(""),
            "",
        )?;
        ensure!(
            current["sha256"] == source["sha256"],
            "Repository evidence changed; run a new portfolio review before approving work"
        );
    }
    let p = r.result["proposals"]
        .as_array_mut()
        .context("No proposals")?
        .iter_mut()
        .find(|p| p["id"] == proposal)
        .context("Proposal is unavailable")?;
    ensure!(
        p["status"] == "proposed",
        "This task has already been approved or dispatched"
    );
    ensure!(
        p["project"] == target["project"],
        "Choose an agent in the proposed project"
    );
    let choices = targets(
        config,
        category,
        p["project"].as_str().context("Missing project")?,
    )?;
    let choice = choices["targets"]
        .as_array()
        .context("No agent targets")?
        .iter()
        .find(|v| v["target"] == *target && v["conversation"] == conversation)
        .context("Agent identity changed; choose it again")?;
    p["target"] = target.clone();
    p["conversation"] = json!(conversation);
    p["provider"] = choice["provider"].clone();
    p["status"] = json!(if choice["native_queue"] == true {
        "queued"
    } else {
        "approved_manual"
    });
    p["approved_ms"] = json!(now());
    save(&root, &r)?;
    drop(_lock);
    resume_queue(config, category)?;
    Ok(json!({"approved":proposal}))
}
pub fn resume_queue(config: &Config, category: &str) -> Result<Value> {
    let root = category_agent::root_path(config, category)?;
    ensure!(
        category_agent::load_path(&root)?.profile.enabled,
        "Portfolio Coordinator is disabled"
    );
    let mut child = Command::new(std::env::current_exe()?)
        .args(["--category-agent-handoffs", &config.workspace, category])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(json!({"queue":"resuming"}))
}
pub fn proposal_update(
    config: &Config,
    category: &str,
    review: &str,
    proposal: &str,
    action: &str,
) -> Result<Value> {
    let root = category_agent::root_path(config, category)?;
    let _lock = crate::project_sync::Transaction::acquire(root.join("runtime.lock"))?;
    let mut r = load(&root, review)?;
    let p = r.result["proposals"]
        .as_array_mut()
        .context("No proposals")?
        .iter_mut()
        .find(|p| p["id"] == proposal)
        .context("Proposal is unavailable")?;
    match action {
        "reject" => {
            ensure!(
                matches!(
                    p["status"].as_str(),
                    Some(
                        "proposed"
                            | "queued"
                            | "approved_manual"
                            | "needs_review"
                            | "needs_reapproval"
                    )
                ),
                "Task is no longer cancellable"
            );
            p["status"] = json!("rejected");
        }
        "verified" => {
            ensure!(
                matches!(p["status"].as_str(), Some("delivered" | "approved_manual")),
                "Delivery is not complete"
            );
            p["status"] = json!("verified_by_user");
            p["verified_ms"] = json!(now());
        }
        _ => anyhow::bail!("Unknown task action"),
    }
    save(&root, &r)?;
    Ok(json!({"updated":proposal}))
}
pub fn handoffs(project: &str, category: &str) -> Result<()> {
    let config = crate::saved(project)?;
    let root = category_agent::root_path(&config, category)?;
    let Some(_owner) = crate::project_sync::Transaction::try_acquire(root.join("handoffs.lock"))?
    else {
        return Ok(());
    };
    loop {
        let mut pending = false;
        let rt = runtime(&root)?;
        for id in rt.history.iter().rev() {
            let _guard = crate::project_sync::Transaction::acquire(root.join("runtime.lock"))?;
            let mut r = load(&root, id)?;
            let revoked = valid(&config, &r).is_err();
            let mut chosen = None;
            let verified = r.result["proposals"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["status"] == "verified_by_user")
                .filter_map(|p| p["id"].as_str().map(String::from))
                .collect::<std::collections::BTreeSet<_>>();
            if let Some(proposals) = r.result["proposals"].as_array_mut() {
                for (i, p) in proposals.iter_mut().enumerate() {
                    if p["status"] == "dispatching" {
                        p["status"] = json!("indeterminate");
                        p["error"] =
                            json!("Delivery may have occurred; no automatic retry was made");
                    }
                    if p["status"] != "queued" {
                        continue;
                    }
                    if revoked {
                        p["status"] = json!("cancelled");
                        continue;
                    }
                    if !p["depends_on"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .all(|d| d.as_str().is_some_and(|id| verified.contains(id)))
                    {
                        pending = true;
                        continue;
                    }
                    let target = p["target"].clone();
                    let current = api(
                        "panes.get",
                        json!({"project":target["project"],"pane":target["pane"]}),
                    );
                    let Ok(current) = current else {
                        pending = true;
                        continue;
                    };
                    if current["target"] != target {
                        p["status"] = json!("needs_reapproval");
                        p["error"] = json!("Project conversation or process identity changed");
                        continue;
                    }
                    let member =
                        crate::saved(target["project"].as_str().context("Missing project")?)?;
                    let record = crate::agent_state::records(&member)?
                        .into_iter()
                        .find(|v| Some(v.report.run_id.as_str()) == target["run_id"].as_str());
                    let Some(record) = record else {
                        pending = true;
                        continue;
                    };
                    if record.report.conversation_id.as_deref() != p["conversation"].as_str() {
                        p["status"] = json!("needs_reapproval");
                        continue;
                    }
                    if !matches!(
                        record.report.state,
                        crate::agent_state::Lifecycle::Idle
                            | crate::agent_state::Lifecycle::Completed
                    ) {
                        pending = true;
                        continue;
                    }
                    p["status"] = json!("dispatching");
                    p["dispatch_ms"] = json!(now());
                    chosen = Some((i, member, p.clone()));
                    break;
                }
            }
            if let Some((index, _, _)) = &chosen
                && let Err(error) = verify_sources(&config, category, &r.sources)
            {
                r.result["proposals"][*index]["status"] = json!("needs_review");
                r.result["proposals"][*index]["error"] = json!(format!("{error:#}"));
                chosen = None;
            }
            save(&root, &r)?;
            if let Some((index, member, p)) = chosen {
                // Commit the dispatch receipt before crossing the process/SSH boundary.
                // A lost acknowledgement is indeterminate and is never replayed.
                drop(_guard);
                let mut request = member.request(
                    p["target"]["pane"].as_u64().unwrap_or(1) as usize - 1,
                    "queue_conversation",
                );
                request["text"] = p["text"].clone();
                request["conversation"] = p["conversation"].clone();
                request["expected_boot_id"] = p["target"]["boot_id"].clone();
                request["expected_agent_pid"] = p["target"]["agent_pid"].clone();
                request["expected_agent_start_ticks"] = p["target"]["agent_start_ticks"].clone();
                let result = crate::read_query::output(member.command(
                    p["target"]["pane"].as_u64().unwrap_or(1) as usize - 1,
                    &request,
                    false,
                ));
                let _guard = crate::project_sync::Transaction::acquire(root.join("runtime.lock"))?;
                let mut current = load(&root, id)?;
                let proposal = &mut current.result["proposals"][index];
                match result {
                    Ok(bytes)
                        if serde_json::from_slice::<Value>(&bytes)
                            .is_ok_and(|v| v["queued"] == true) =>
                    {
                        proposal["status"] = json!("delivered")
                    }
                    _ => {
                        proposal["status"] = json!("indeterminate");
                        proposal["error"] = json!(
                            "Native queue did not acknowledge delivery; check the project conversation before retrying"
                        );
                    }
                }
                save(&root, &current)?;
            }
        }
        if !pending {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

pub fn badges(config: &Config) -> Result<std::collections::BTreeMap<String, String>> {
    let mut badges = std::collections::BTreeMap::new();
    for category in crate::categories::Catalog::load(config)?.all() {
        let root = category_agent::root_path(config, &category.id)?;
        let Ok(state) = category_agent::load_path(&root) else {
            continue;
        };
        if !state.profile.enabled {
            continue;
        }
        let status = runtime(&root)
            .ok()
            .and_then(|rt| rt.active)
            .and_then(|id| load(&root, &id).ok())
            .map(|r| {
                if running(&r) && alive(&r) {
                    if r.mode == "connect" {
                        "AI connecting"
                    } else {
                        "AI working"
                    }
                } else if running(&r) && r.created_ms + 15000 > now() {
                    "AI starting"
                } else if matches!(r.status.as_str(), "failed" | "indeterminate") || running(&r) {
                    "AI needs attention"
                } else {
                    "AI enabled"
                }
            })
            .unwrap_or("AI enabled");
        badges.insert(category.id, status.into());
    }
    Ok(badges)
}

pub fn clear_capabilities(root: &Path) -> Result<()> {
    let _guard = crate::project_sync::Transaction::acquire(root.join("runtime.lock"))?;
    let mut state = runtime(root)?;
    state.capabilities = Value::Null;
    write_runtime(root, &state)
}

pub fn reset_conversation(config: &Config, category: &str) -> Result<Value> {
    let root = category_agent::root_path(config, category)?;
    starkit::terminal_graphics::session::private_root(&root)?;
    let _guard = crate::project_sync::Transaction::acquire(root.join("coordinator.lock"))?;
    revoke(&root)?;
    let mut state = category_agent::load_path(&root)?;
    if let Some(conversation) = state.conversation.take() {
        state.archived_conversations.push(json!({"provider":state.profile.provider,"conversation":conversation,"reason":"user_started_new_conversation"}));
    }
    state.revision = state
        .revision
        .checked_add(1)
        .context("Coordinator revision exhausted")?;
    crate::automation::persist(&root.join("coordinator.json"), &serde_json::to_vec(&state)?)?;
    drop(_guard);
    category_agent::read(config, category)
}
