//! Versioned domain automation over JSON Lines. Presentation is not required.
mod events;
pub(crate) mod journal;
mod transport;
use crate::{
    agent_state, project_state::ProjectState, project_sync, runtime::Config, session_catalog,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
pub const LIMIT: usize = 65536;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub id: Value,
    pub method: String,
    #[serde(default = "object")]
    pub params: Value,
    pub operation_id: Option<String>,
    #[serde(default = "deadline")]
    pub deadline_ms: u64,
}
fn object() -> Value {
    json!({})
}
fn deadline() -> u64 {
    30000
}
#[derive(Debug)]
struct Fault {
    code: &'static str,
    message: String,
}
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Fault {}
pub(crate) fn fault(code: &'static str, message: impl Into<String>) -> anyhow::Error {
    Fault {
        code,
        message: message.into(),
    }
    .into()
}
pub fn error(id: Value, code: &str, message: &str, outcome: &str) -> Value {
    json!({"version":1,"id":id,"ok":false,"error":{"code":code,"message":message,"outcome":outcome}})
}
pub(crate) fn failed(id: Value, e: anyhow::Error) -> Value {
    let code = e
        .downcast_ref::<Fault>()
        .map_or("operation_failed", |e| e.code);
    error(
        id,
        code,
        &format!("{e:#}"),
        if matches!(code, "operation_failed" | "operation_indeterminate") {
            "unknown"
        } else {
            "not_applied"
        },
    )
}
fn success(id: Value, result: Value) -> Value {
    json!({"version":1,"id":id,"ok":true,"result":result})
}
fn params<T: DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone()).map_err(|e| fault("invalid_params", e.to_string()))
}
fn project(id: &str) -> Result<Config> {
    let config = crate::saved(id).map_err(|e| fault("project_unavailable", format!("{e:#}")))?;
    ensure!(
        config.workspace == id,
        "Project identity does not match its saved file"
    );
    Ok(config)
}
fn mutating(method: &str) -> bool {
    matches!(
        method,
        "sessions.create"
            | "category_agent.configure"
            | "category_agent.start"
            | "category_agent.cancel"
            | "category_agent.approve"
            | "category_agent.task"
            | "category_agent.resume_queue"
            | "category_agent.reset_conversation"
            | "plugins.install"
            | "plugins.update"
            | "plugins.activate"
            | "plugins.uninstall"
            | "plugins.link"
            | "plugins.unlink"
            | "plugins.configure"
            | "plugins.panes.open"
            | "plugins.panes.close"
            | "plugins.panes.focus"
            | "plugins.popups.close"
            | "plugins.run"
            | "plugins.open_link"
            | "plugins.cancel"
            | "frontends.command"
            | "frontends.cancel_command"
            | "sessions.import"
            | "machines.save"
            | "machines.remove"
            | "machines.project"
            | "worktrees.create"
            | "worktrees.open"
            | "worktrees.remove"
            | "sessions.default"
            | "panes.create"
            | "panes.rename"
            | "panes.send"
            | "panes.terminate"
            | "agents.report"
            | "attention.dismiss"
            | "attention.mark_read"
            | "notifications.configure"
            | "notifications.prune"
    )
}
const METHODS: &[&str] = &[
    "category_agent.get",
    "category_agent.start",
    "category_agent.cancel",
    "category_agent.targets",
    "category_agent.approve",
    "category_agent.task",
    "category_agent.resume_queue",
    "category_agent.reset_conversation",
    "category_agent.configure",
    "category_agent.evidence",
    "category_agent.read_file",
    "plugins.list",
    "plugins.install",
    "plugins.update",
    "plugins.versions",
    "plugins.activate",
    "plugins.uninstall",
    "plugins.link",
    "plugins.unlink",
    "plugins.configure",
    "plugins.panes.list",
    "plugins.panes.open",
    "plugins.panes.close",
    "plugins.panes.focus",
    "plugins.popups.get",
    "plugins.popups.close",
    "plugins.run",
    "plugins.resolve_link",
    "plugins.open_link",
    "plugins.jobs",
    "plugins.job",
    "plugins.cancel",
    "plugins.service",
    "attention.query",
    "attention.mark_read",
    "notifications.settings",
    "notifications.configure",
    "notifications.prune",
    "notifications.delivery",
    "machines.list",
    "machines.save",
    "machines.remove",
    "machines.test",
    "machines.project",
    "worktrees.list",
    "worktrees.create",
    "worktrees.open",
    "worktrees.remove",
    "capabilities",
    "operations.get",
    "sessions.list",
    "sessions.create",
    "sessions.external",
    "sessions.import",
    "sessions.default",
    "projects.list",
    "panes.list",
    "panes.get",
    "panes.capture",
    "panes.saved_screen",
    "snapshots.status",
    "panes.create",
    "panes.rename",
    "panes.send",
    "panes.terminate",
    "panes.wait",
    "agents.report",
    "attention.list",
    "attention.dismiss",
    "frontends.list",
    "frontends.command",
    "frontends.command_status",
    "frontends.cancel_command",
    "events.subscribe",
];
#[derive(Serialize, Deserialize)]
struct Operation {
    fingerprint: String,
    response: Option<Value>,
}
fn operation_root() -> Result<PathBuf> {
    let root = crate::project_state::data_root()?
        .parent()
        .context("No data directory")?
        .join("automation")
        .join("operations");
    private_root(&root)?;
    Ok(root)
}
fn private_root(path: &std::path::Path) -> Result<()> {
    starkit::terminal_graphics::session::private_root(path)?;
    Ok(())
}
pub(crate) fn persist(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    starkit::fs::write_private(path, bytes)?;
    // The SDK flushes file contents; also commit the rename before a write
    // may begin. Losing the pending filename after reboot could repeat input.
    // Include new ancestor links even when another process created the same
    // metadata directory concurrently and has not flushed it yet.
    for directory in path
        .parent()
        .context("No metadata parent directory")?
        .ancestors()
        .filter(|p| !p.as_os_str().is_empty())
    {
        fs::File::open(directory)?.sync_all()?;
    }
    Ok(())
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(fault("unsupported_version", "Supported API version is 1"));
        }
        if !self.id.is_null()
            && !self.id.is_i64()
            && !self.id.is_u64()
            && !self.id.as_str().is_some_and(|s| s.len() <= 128)
        {
            return Err(fault(
                "invalid_request",
                "id must be null, an integer or a string up to 128 bytes",
            ));
        }
        if !(100..=120000).contains(&self.deadline_ms) {
            return Err(fault("invalid_request", "deadline_ms must be 100..120000"));
        }
        if !METHODS.contains(&self.method.as_str()) {
            return Err(fault("method_not_found", "Unknown automation method"));
        }
        if !self.params.is_object() {
            return Err(fault("invalid_params", "params must be an object"));
        }
        if mutating(&self.method) {
            let id = self.operation_id.as_deref().ok_or_else(|| {
                fault(
                    "operation_id_required",
                    "Writes require a UUID operation_id",
                )
            })?;
            uuid::Uuid::parse_str(id)
                .map_err(|_| fault("invalid_request", "operation_id must be a UUID"))?;
        }
        Ok(())
    }
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(if self.method == "panes.wait" {
            self.params["timeout_ms"]
                .as_u64()
                .unwrap_or(30000)
                .min(600000)
                + 1000
        } else {
            self.deadline_ms
        })
    }
}
pub fn execute(request: &Request) -> Value {
    if let Err(e) = request.validate() {
        return failed(request.id.clone(), e);
    }
    let result = (|| -> Result<Value> {
        if !mutating(&request.method) {
            return dispatch(request);
        }
        let id = uuid::Uuid::parse_str(request.operation_id.as_deref().unwrap())?
            .simple()
            .to_string();
        let root = operation_root()?;
        let _lock = project_sync::Transaction::acquire(root.join(format!("{id}.lock")))?;
        let path = root.join(format!("{id}.json"));
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(
                &json!({"method":request.method,"params":request.params,"version":request.version})
            )?)
        );
        if path.exists() {
            let saved: Operation = serde_json::from_slice(&fs::read(&path)?)?;
            if saved.fingerprint != fingerprint {
                return Err(fault(
                    "operation_id_conflict",
                    "This operation_id was used for different parameters",
                ));
            }
            let mut response=saved.response.ok_or_else(||fault("operation_indeterminate","A previous attempt did not record its result. Inspect the target before issuing a new operation; this attempt was not repeated."))?;
            response["id"] = request.id.clone();
            response["replayed"] = json!(true);
            return Ok(response);
        }
        let mut operation = Operation {
            fingerprint,
            response: None,
        };
        persist(&path, &serde_json::to_vec(&operation)?)?;
        let response = match dispatch(request) {
            Ok(result) => success(request.id.clone(), result),
            Err(e) => failed(request.id.clone(), e),
        };
        operation.response = Some(response.clone());
        persist(&path, &serde_json::to_vec(&operation)?)?;
        Ok(response)
    })();
    match result {
        Ok(value) if mutating(&request.method) => value,
        Ok(value) => success(request.id.clone(), value),
        Err(e) => failed(request.id.clone(), e),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Project {
    project: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Target {
    project: String,
    pane: u8,
    epoch: Option<String>,
    pid: u32,
    start_ticks: u64,
    boot_id: String,
    agent_pid: Option<u32>,
    agent_start_ticks: Option<u64>,
    run_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pane {
    project: String,
    pane: u8,
}
fn pane(config: &Config, state: &ProjectState, pane: u8) -> Result<usize> {
    if !(1..=32).contains(&pane) {
        return Err(fault("invalid_params", "pane must be 1..32"));
    }
    let id = usize::from(pane - 1);
    if !state.terminal_ids().contains(&id) || config.canonical_pane(id) != id {
        return Err(fault(
            "pane_unavailable",
            "Pane is missing or is a legacy alias",
        ));
    }
    Ok(id)
}
fn inspect(config: &Config, id: usize) -> Result<Value> {
    let mut health = config.call(id, &config.request(id, "inspect"))?;
    agent_state::enrich(config, id, &mut health);
    if !config.remote(id)
        && let Some(pid) = health["pid"].as_u64().and_then(|p| u32::try_from(p).ok())
    {
        health["pid_start_ticks"] = json!(agent_state::process_start(pid));
    }
    Ok(health)
}
fn current_run(health: &Value) -> Option<&str> {
    (health["health"]["report_process_matches"] == true)
        .then(|| health["agent_record"]["run_id"].as_str())
        .flatten()
}
fn observed(config: &Config, state: &ProjectState, id: usize, health: &Value) -> Value {
    let target = if let Ok((pid, start, boot)) = config.terminal_identity(id, health) {
        json!(Target {
            project: config.workspace.clone(),
            pane: (id + 1) as u8,
            epoch: state.terminal_epochs.get(&id).cloned(),
            pid,
            start_ticks: start,
            boot_id: boot.to_string(),
            agent_pid: health["agent_pid"]
                .as_u64()
                .and_then(|p| u32::try_from(p).ok()),
            agent_start_ticks: health["agent_start_ticks"].as_u64(),
            run_id: current_run(health).map(str::to_owned)
        })
    } else {
        Value::Null
    };
    let mut public = json!({});
    for key in [
        "alive",
        "exited",
        "pid",
        "pid_start_ticks",
        "boot_id",
        "agent",
        "agent_pid",
        "agent_start_ticks",
        "state",
        "source",
        "health",
    ] {
        if let Some(value) = health.get(key) {
            public[key] = value.clone();
        }
    }
    let guardable =
        target.is_object() && (!health["agent"].is_string() || health["agent_pid"].is_number());
    json!({"project":config.workspace,"pane":id+1,"epoch":state.terminal_epochs.get(&id),"label":state.terminal_labels.get(&id),"target":target,
        "capabilities":{"input":guardable && !config.externally_owned(id) && (config.adopted_terminal(id).is_none() || health["bridge_connected"]==true),"terminate":guardable && !config.externally_owned(id)},"status":public})
}
fn guarded(target: &Target) -> Result<(Config, ProjectState, usize, Value)> {
    let config = project(&target.project)?;
    let state = ProjectState::load(&config)?;
    let id = pane(&config, &state, target.pane)?;
    if state.terminal_epochs.get(&id) != target.epoch.as_ref() {
        return Err(fault("stale_target", "Terminal generation changed"));
    }
    let health = inspect(&config, id)?;
    let identity = config
        .terminal_identity(id, &health)
        .map_err(|e| fault("stale_target", format!("{e:#}")))?;
    if identity
        != (
            target.pid,
            target.start_ticks,
            uuid::Uuid::parse_str(&target.boot_id)?,
        )
        || health["agent_pid"].as_u64() != target.agent_pid.map(u64::from)
        || health["agent_start_ticks"].as_u64() != target.agent_start_ticks
        || current_run(&health) != target.run_id.as_deref()
    {
        return Err(fault(
            "stale_target",
            "Terminal or agent process identity changed",
        ));
    }
    if health["agent"].is_string() && target.agent_pid.is_none() {
        return Err(fault(
            "identity_unavailable",
            "This source does not expose the foreground agent process identity",
        ));
    }
    Ok((config, state, id, health))
}
fn target(value: &Value) -> Result<Target> {
    if !["epoch", "agent_pid", "agent_start_ticks", "run_id"]
        .iter()
        .all(|key| value.get(key).is_some())
    {
        return Err(fault(
            "invalid_params",
            "Copy the complete target returned by panes.get, including explicit null identity fields",
        ));
    }
    let target: Target = params(value)?;
    if !(1..=32).contains(&target.pane)
        || target.pid == 0
        || target.start_ticks == 0
        || uuid::Uuid::parse_str(&target.boot_id).is_err()
        || target.agent_pid.is_some() != target.agent_start_ticks.is_some()
        || target.agent_pid == Some(0)
        || target.agent_start_ticks == Some(0)
    {
        return Err(fault(
            "invalid_params",
            "Target contains an invalid pane or process identity",
        ));
    }
    Ok(target)
}
pub(super) fn metadata(project_filter: Option<&str>) -> Result<Value> {
    let mut projects = Vec::new();
    let mut panes = Vec::new();
    let mut agents = Vec::new();
    let mut attention = Vec::new();
    let mut warnings = Vec::new();
    for config in session_catalog::projects()
        .into_iter()
        .filter(|p| project_filter.is_none_or(|id| p.workspace == id))
    {
        projects.push(project_resource(&config));
        let state = (|| -> Result<ProjectState> {
            let root = crate::project_state::data_root()?;
            for suffix in ["model", "layout"] {
                let path = root.join(format!("{}.{suffix}.json", config.workspace));
                if path.exists() {
                    return Ok(serde_json::from_slice(&fs::read(path)?)?);
                }
            }
            bail!("Project model is unavailable")
        })();
        match state {
            Ok(state) => {
                for id in state
                    .terminal_ids()
                    .into_iter()
                    .filter(|id| config.canonical_pane(*id) == *id)
                {
                    panes.push(pane_resource(&config, &state, id));
                }
            }
            Err(e) => warnings.push(
                json!({"project":config.workspace,"source":"model","error":format!("{e:#}")}),
            ),
        }
        match agent_state::records(&config) {
            Ok(records) => {
                for record in records {
                    agents.push(agent_resource(&config, &record));
                }
            }
            Err(e) => warnings.push(
                json!({"project":config.workspace,"source":"agents","error":format!("{e:#}")}),
            ),
        }
        match agent_state::attention(&config) {
            Ok(items) => {
                for item in items {
                    attention.push(attention_resource(&config, &item));
                }
            }
            Err(e) => warnings.push(
                json!({"project":config.workspace,"source":"attention","error":format!("{e:#}")}),
            ),
        }
    }
    Ok(
        json!({"projects":projects,"panes":panes,"agents":agents,"attention":attention,"warnings":warnings}),
    )
}
fn project_resource(config: &Config) -> Value {
    json!({"id":config.workspace,"label":config.label,"session":session_catalog::group(config),"host":config.host,"directory":config.directory})
}
fn pane_resource(config: &Config, state: &ProjectState, id: usize) -> Value {
    json!({"project":config.workspace,"pane":id+1,"epoch":state.terminal_epochs.get(&id),"label":state.terminal_labels.get(&id)})
}
fn agent_resource(config: &Config, record: &agent_state::Record) -> Value {
    json!({"project":config.workspace,"pane":record.pane+1,"epoch":record.report.epoch,"run_id":record.report.run_id,"state":record.report.state,"harness":record.report.harness,"source":record.report.source,"conversation_id":record.report.conversation_id})
}
fn attention_resource(config: &Config, item: &agent_state::Attention) -> Value {
    json!({"project":config.workspace,"pane":item.pane+1,"epoch":item.epoch,"id":item.id,"source":item.source,"run_id":item.run_id,"sequence":item.sequence,"state":item.state,"observed_ms":item.observed_ms,"dismissed":item.dismissed})
}
fn dispatch(request: &Request) -> Result<Value> {
    let p = &request.params;
    match request.method.as_str() {
        "attention.query" => Ok(serde_json::to_value(crate::notifications::query(
            &params(p)?,
        )?)?),
        "attention.mark_read" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                items: Vec<crate::notifications::Reference>,
            }
            let p: P = params(p)?;
            crate::notifications::mark_read(&p.items)
        }
        "notifications.settings" => {
            let _: Empty = params(p)?;
            Ok(
                json!({"settings":crate::notifications::settings()?,"system":crate::notifications::system_capability()}),
            )
        }
        "notifications.configure" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                settings: crate::notifications::Settings,
                expected_revision: String,
            }
            let p: P = params(p)?;
            Ok(json!({"settings":crate::notifications::save(p.settings,&p.expected_revision)?}))
        }
        "notifications.prune" => {
            let _: Empty = params(p)?;
            crate::notifications::apply_retention()
        }
        "notifications.delivery" => {
            let _: Empty = params(p)?;
            crate::notifications::delivery_status()
        }
        "machines.list" => {
            let _: Empty = params(p)?;
            Ok(json!({"machines":crate::machines::list()?}))
        }
        "machines.save" => Ok(serde_json::to_value(crate::machines::save(params(p)?)?)?),
        "machines.test" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                machine: String,
            }
            let p: P = params(p)?;
            crate::machines::test(&crate::machines::get(&p.machine)?)
        }
        "machines.remove" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                machine: String,
                expected_revision: String,
            }
            let p: P = params(p)?;
            crate::machines::remove(&p.machine, &p.expected_revision)?;
            Ok(json!({"removed":p.machine,"remote_processes_retained":true}))
        }
        "machines.project" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                machine: String,
                project: String,
                directory: String,
                session: Option<String>,
                theme: Option<String>,
            }
            let p: P = params(p)?;
            let config = crate::machines::create_project(
                &crate::machines::get(&p.machine)?,
                &p.project,
                &p.directory,
                p.session,
                &p.theme.unwrap_or_else(crate::appearance::default_theme),
            )?;
            Ok(project_resource(&config))
        }
        "worktrees.list" => {
            let p: Project = params(p)?;
            Ok(serde_json::to_value(crate::worktrees::list(&project(
                &p.project,
            )?)?)?)
        }
        "worktrees.create" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                branch: String,
                path: Option<String>,
                base: Option<String>,
            }
            let p: P = params(p)?;
            let target = crate::worktrees::create(
                &project(&p.project)?,
                &p.branch,
                p.path.as_deref(),
                p.base.as_deref(),
            )?;
            Ok(json!({"target":target}))
        }
        "worktrees.open" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                target: Value,
            }
            let p: P = params(p)?;
            let opened = crate::worktrees::open(&project(&p.project)?, &p.target)?;
            Ok(
                json!({"project":opened.workspace,"origin":crate::worktrees::link(&opened)?.map(|l|l.origin),"directory":opened.directory}),
            )
        }
        "worktrees.remove" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                target: Value,
                confirm: String,
            }
            let p: P = params(p)?;
            if p.confirm != "REMOVE" {
                return Err(fault("invalid_params", "confirm must be REMOVE"));
            }
            crate::worktrees::remove(&project(&p.project)?, &p.target)
        }
        "operations.get" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                operation_id: String,
            }
            let p: P = params(p)?;
            let id = uuid::Uuid::parse_str(&p.operation_id)
                .map_err(|_| fault("invalid_params", "operation_id must be a UUID"))?
                .simple()
                .to_string();
            let path = operation_root()?.join(format!("{id}.json"));
            if !path.exists() {
                return Ok(json!({"operation_id":p.operation_id,"status":"unknown"}));
            }
            let record: Operation = serde_json::from_slice(&fs::read(path)?)?;
            Ok(
                json!({"operation_id":p.operation_id,"status":if record.response.is_some(){"completed"}else{"indeterminate"},"response":record.response}),
            )
        }
        "capabilities" => {
            let _: Empty = params(p)?;
            Ok(
                json!({"version":1,"transport":"jsonl","pane_numbering":"one_based","methods":METHODS.iter().map(|m|json!({"method":m,"write":mutating(m)})).collect::<Vec<_>>(),
                "request_bytes":LIMIT,"input_bytes":32768,"operation_replay":true,"events":{"snapshot_on_subscribe":true,"durable_replay":true,"retention_events":2048,"retention_bytes":2097152,"source":"domain_journal"}}),
            )
        }
        "sessions.list" => {
            let _: Empty = params(p)?;
            Ok(json!({"sessions":session_catalog::list()?}))
        }
        "projects.list" => {
            let _: Empty = params(p)?;
            Ok(json!({"projects":metadata(None)?["projects"]}))
        }
        "snapshots.status" => {
            let _: Empty = params(p)?;
            crate::snapshot_service::status()
        }
        "panes.saved_screen" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                pane: u8,
                #[serde(default)]
                previous: bool,
            }
            let p: P = params(p)?;
            let config = project(&p.project)?;
            let state = ProjectState::load(&config)?;
            let id = pane(&config, &state, p.pane)?;
            let screen = crate::cold_start::load(&config.workspace, id, p.previous)?;
            Ok(
                json!({"read_only":true,"historical":true,"text":screen.parse()?.contents(),"screen":screen}),
            )
        }
        "panes.list" => {
            let p: Project = params(p)?;
            let config = project(&p.project)?;
            let state = ProjectState::load(&config)?;
            Ok(
                json!({"project":p.project,"panes":state.terminal_ids().into_iter().filter(|id|config.canonical_pane(*id)==*id).map(|id|json!({"pane":id+1,"epoch":state.terminal_epochs.get(&id),"label":state.terminal_labels.get(&id)})).collect::<Vec<_>>()}),
            )
        }
        "panes.get" | "panes.capture" => {
            let p: Pane = params(p)?;
            let config = project(&p.project)?;
            let state = ProjectState::load(&config)?;
            let id = pane(&config, &state, p.pane)?;
            let health = inspect(&config, id)?;
            let mut result = observed(&config, &state, id, &health);
            if request.method == "panes.capture" {
                let (bytes, source) = config.terminal_history(id)?;
                result["text"] = json!(String::from_utf8_lossy(&bytes));
                result["capture_source"] = json!(source);
            }
            let latest = ProjectState::load(&config)?;
            if latest.terminal_epochs.get(&id) != state.terminal_epochs.get(&id)
                || !latest.terminal_ids().contains(&id)
            {
                return Err(fault("stale_target", "Pane changed while reading"));
            }
            Ok(result)
        }
        "sessions.create" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                name: String,
                directory: String,
                #[serde(default = "crate::appearance::default_theme")]
                theme: String,
            }
            let p: P = params(p)?;
            let config = session_catalog::create(&p.name, &p.directory, &p.theme)?;
            config.ensure(0, None)?;
            Ok(json!({"session":session_catalog::group(&config),"project":config.workspace}))
        }
        "sessions.default" => {
            if p.get("session").is_none() {
                return Err(fault(
                    "invalid_params",
                    "session must be explicit, including null to clear the default",
                ));
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                session: Option<String>,
            }
            let p: P = params(p)?;
            session_catalog::set_default(p.session.as_deref())?;
            Ok(json!({"session":p.session}))
        }
        "sessions.external" => {
            let _: Empty = params(p)?;
            crate::session_import::discover()
        }
        "sessions.import" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                reference: crate::session_import::Reference,
            }
            crate::session_import::import(params::<P>(p)?.reference)
        }
        "panes.create" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                label: Option<String>,
            }
            let p: P = params(p)?;
            let config = project(&p.project)?;
            let _lock = project_sync::transaction(&config)?;
            let mut state = ProjectState::load(&config)?;
            if let Some(label) = &p.label {
                crate::project_state::validate_label(label)?;
            }
            let id = state.add_terminal()?;
            if let Some(label) = p.label {
                state.terminal_labels.insert(id, label);
            }
            state.save(&config)?;
            config.ensure(id, None)?;
            Ok(observed(&config, &state, id, &inspect(&config, id)?))
        }
        "panes.rename" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                target: Value,
                label: String,
            }
            let p: P = params(p)?;
            crate::project_state::validate_label(&p.label)?;
            let t = target(&p.target)?;
            let config = project(&t.project)?;
            let _lock = project_sync::transaction(&config)?;
            let (config, mut state, id, _) = guarded(&t)?;
            state.terminal_labels.insert(id, p.label.clone());
            state.save(&config)?;
            Ok(json!({"target":t,"label":p.label}))
        }
        "panes.send" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                target: Value,
                text: String,
                #[serde(default)]
                submit: bool,
            }
            let p: P = params(p)?;
            ensure!(p.text.len() <= 32768, "Input exceeds 32 KiB");
            if p.text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
            {
                return Err(fault("invalid_params", "Input contains control characters"));
            }
            let t = target(&p.target)?;
            let config = project(&t.project)?;
            let _lock = project_sync::transaction(&config)?;
            let (config, _, id, _) = guarded(&t)?;
            if config.externally_owned(id) {
                return Err(fault(
                    "unsupported_capability",
                    "Imported terminals retain their source input ownership",
                ));
            }
            let mut req = config.request(id, "input");
            req["text"] = json!(p.text);
            req["submit"] = json!(p.submit);
            req["expected_pid"] = json!(t.pid);
            req["expected_start_ticks"] = json!(t.start_ticks);
            req["expected_boot_id"] = json!(t.boot_id);
            req["expected_agent_pid"] = json!(t.agent_pid);
            req["expected_agent_start_ticks"] = json!(t.agent_start_ticks);
            if let Some(terminal) = config.adopted_terminal(id) {
                req["expected_broker"] = json!(terminal.broker);
                req["expected_terminal_key"] = json!(terminal.key);
                req["expected_proxy"] = json!(terminal.proxy);
            }
            config.call(id, &req)
        }
        "panes.terminate" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                target: Value,
                confirm: String,
            }
            let p: P = params(p)?;
            ensure!(p.confirm == "TERMINATE", "confirm must be TERMINATE");
            let t = target(&p.target)?;
            let config = project(&t.project)?;
            {
                let _lock = project_sync::transaction(&config)?;
                guarded(&t)?;
            }
            config.terminate_terminal(
                usize::from(t.pane - 1),
                t.epoch.as_deref(),
                (t.pid, t.start_ticks, uuid::Uuid::parse_str(&t.boot_id)?),
            )?;
            Ok(json!({"target":t,"terminated":true}))
        }
        "panes.wait" => wait(p),
        "agents.report" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                pane: u8,
                report: agent_state::Report,
            }
            let p: P = params(p)?;
            let config = project(&p.project)?;
            let state = ProjectState::load(&config)?;
            let id = pane(&config, &state, p.pane)?;
            let mut record = serde_json::to_value(agent_state::publish(&config, id, p.report)?)?;
            record["pane"] = json!(id + 1);
            record["project"] = json!(config.workspace);
            Ok(record)
        }
        "attention.list" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: Option<String>,
                #[serde(default)]
                include_dismissed: bool,
            }
            let p: P = params(p)?;
            if let Some(id) = &p.project {
                project(id)?;
            }
            let mut data = metadata(p.project.as_deref())?;
            let items = data["attention"].as_array_mut().unwrap();
            if !p.include_dismissed {
                items.retain(|i| i["dismissed"] != true);
            }
            Ok(json!({"items":items,"warnings":data["warnings"]}))
        }
        "attention.dismiss" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                event: String,
            }
            let p: P = params(p)?;
            agent_state::dismiss(&project(&p.project)?, &p.event)?;
            Ok(json!({"dismissed":p.event}))
        }
        "category_agent.start" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                category: String,
                mode: String,
                #[serde(default)]
                message: String,
            }
            let p: P = params(p)?;
            crate::category_agent_runtime::start(
                &project(&p.project)?,
                &p.category,
                &p.mode,
                &p.message,
            )
        }
        "category_agent.cancel" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                category: String,
            }
            let p: P = params(p)?;
            crate::category_agent_runtime::cancel(&project(&p.project)?, &p.category)
        }
        "category_agent.targets" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                category: String,
                member: String,
            }
            let p: P = params(p)?;
            crate::category_agent_runtime::targets(&project(&p.project)?, &p.category, &p.member)
        }
        "category_agent.approve" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                category: String,
                review: String,
                proposal: String,
                target: Value,
                conversation: String,
            }
            let p: P = params(p)?;
            crate::category_agent_runtime::approve(
                &project(&p.project)?,
                &p.category,
                &p.review,
                &p.proposal,
                &p.target,
                &p.conversation,
            )
        }
        "category_agent.task" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                category: String,
                review: String,
                proposal: String,
                action: String,
            }
            let p: P = params(p)?;
            crate::category_agent_runtime::proposal_update(
                &project(&p.project)?,
                &p.category,
                &p.review,
                &p.proposal,
                &p.action,
            )
        }
        "category_agent.reset_conversation" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                category: String,
            }
            let p: P = params(p)?;
            crate::category_agent_runtime::reset_conversation(&project(&p.project)?, &p.category)
        }
        "category_agent.resume_queue" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                category: String,
            }
            let p: P = params(p)?;
            crate::category_agent_runtime::resume_queue(&project(&p.project)?, &p.category)
        }
        "category_agent.read_file" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                category: String,
                member: String,
                path: String,
            }
            let p: P = params(p)?;
            crate::category_agent_files::read(
                &project(&p.project)?,
                &p.category,
                &p.member,
                &p.path,
            )
        }
        "category_agent.get" | "category_agent.evidence" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                category: String,
            }
            let p: P = params(p)?;
            let config = project(&p.project)?;
            if request.method == "category_agent.get" {
                crate::category_agent::read(&config, &p.category)
            } else {
                crate::category_agent::evidence(&config, &p.category)
            }
        }
        "category_agent.configure" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: String,
                category: String,
                expected_revision: u64,
                profile: crate::category_agent::Profile,
            }
            let p: P = params(p)?;
            crate::category_agent::configure(
                &project(&p.project)?,
                &p.category,
                p.expected_revision,
                p.profile,
            )
        }
        "plugins.list" | "plugins.service" => {
            let _: Empty = params(p)?;
            if request.method == "plugins.list" {
                crate::plugins::list()
            } else {
                crate::plugin_runner::service_status()
            }
        }
        "plugins.install" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                repository: String,
                #[serde(default)]
                reference: String,
                #[serde(default)]
                subdirectory: String,
                expected_revision: String,
            }
            let p: P = params(p)?;
            crate::plugin_packages::install(
                &p.repository,
                &p.reference,
                &p.subdirectory,
                &p.expected_revision,
            )
        }
        "plugins.update" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                plugin: String,
                reference: Option<String>,
                expected_revision: String,
            }
            let p: P = params(p)?;
            crate::plugin_packages::update(&p.plugin, p.reference.as_deref(), &p.expected_revision)
        }
        "plugins.versions" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                plugin: String,
            }
            let p: P = params(p)?;
            crate::plugin_packages::versions(&p.plugin)
        }
        "plugins.activate" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                plugin: String,
                release: String,
                expected_revision: String,
            }
            let p: P = params(p)?;
            crate::plugin_packages::activate(&p.plugin, &p.release, &p.expected_revision)
        }
        "plugins.uninstall" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                plugin: String,
                expected_revision: String,
            }
            let p: P = params(p)?;
            crate::plugin_packages::uninstall(&p.plugin, &p.expected_revision)
        }
        "plugins.link" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                path: String,
                expected_revision: String,
            }
            let p: P = params(p)?;
            crate::plugins::link(&p.path, &p.expected_revision)
        }
        "plugins.unlink" | "plugins.configure" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                plugin: String,
                expected_revision: String,
                enabled: Option<bool>,
                expected_digest: Option<String>,
            }
            let p: P = params(p)?;
            if request.method == "plugins.unlink" {
                ensure!(
                    p.enabled.is_none() && p.expected_digest.is_none(),
                    "Unlink does not accept configuration fields"
                );
                crate::plugins::unlink(&p.plugin, &p.expected_revision)
            } else {
                crate::plugins::configure(
                    &p.plugin,
                    p.enabled.context("Missing enabled")?,
                    p.expected_digest.as_deref().unwrap_or(""),
                    &p.expected_revision,
                )
            }
        }
        "plugins.resolve_link" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                url: String,
                offset: Option<usize>,
                limit: Option<usize>,
            }
            let p: P = params(p)?;
            crate::plugin_links::resolve(&p.url, p.offset.unwrap_or(0), p.limit.unwrap_or(64))
        }
        "plugins.open_link" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                plugin: String,
                handler: String,
                url: String,
                expected_digest: String,
                project: Option<String>,
            }
            let p: P = params(p)?;
            let context = p
                .project
                .as_deref()
                .map(project)
                .transpose()?
                .as_ref()
                .map(crate::plugins::project);
            crate::plugin_runner::open_link(
                &p.plugin,
                &p.handler,
                &p.expected_digest,
                &p.url,
                context,
                request
                    .operation_id
                    .as_deref()
                    .context("Missing operation_id")?,
            )
        }
        "plugins.panes.list" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                plugin: Option<String>,
            }
            crate::plugin_panes::list(params::<P>(p)?.plugin.as_deref())
        }
        "plugins.panes.open" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                plugin: String,
                entrypoint: String,
                expected_digest: String,
                project: String,
                frontend: Option<crate::frontend_commands::Target>,
                placement: Option<crate::plugin_panes::Placement>,
                width: Option<crate::plugin_popup::Dimension>,
                height: Option<crate::plugin_popup::Dimension>,
            }
            let p: P = params(p)?;
            let registry = crate::plugins::registry()?;
            let entry = registry
                .plugins
                .get(&p.plugin)
                .context("Plugin is not linked")?;
            let (manifest, _) = crate::plugins::package_metadata(entry)?;
            let declared = manifest
                .panes
                .iter()
                .find(|e| e.id == p.entrypoint)
                .context("Pane entrypoint is unavailable")?;
            if p.placement.unwrap_or(declared.placement) == crate::plugin_panes::Placement::Popup {
                let target = p.frontend.ok_or_else(|| {
                    fault(
                        "frontend_required",
                        "A frontend target is required for a popup",
                    )
                })?;
                let command = crate::frontend_commands::Command::OpenPluginPopup {
                    project: p.project,
                    plugin: p.plugin,
                    entrypoint: p.entrypoint,
                    digest: p.expected_digest,
                    options: crate::plugin_popup::Options {
                        width: p.width,
                        height: p.height,
                    },
                };
                let receipt = crate::frontend_commands::submit(
                    request,
                    crate::frontend_commands::Params::popup(target, command),
                )?;
                return Ok(
                    json!({"kind":"popup","popup":receipt["result"]["popup"],"presentation":receipt}),
                );
            }
            if p.width.is_some() || p.height.is_some() {
                return Err(fault(
                    "invalid_params",
                    "width and height apply only to popup presentation",
                ));
            }
            if let Some(target) = &p.frontend {
                crate::frontend_commands::plugin_target(target, &p.project)?;
            }
            let record = crate::plugin_panes::open(
                &p.plugin,
                &p.entrypoint,
                &p.expected_digest,
                &p.project,
                p.placement,
            )?;
            let presentation = if let Some(target) = p.frontend {
                match crate::frontend_commands::submit(
                    request,
                    crate::frontend_commands::Params::plugin_pane(target, &record, p.placement),
                ) {
                    Ok(receipt) => receipt,
                    Err(error) => {
                        json!({"status":"unconfirmed","operation_id":request.operation_id,"error":failed(Value::Null,error)["error"]})
                    }
                }
            } else {
                json!("not_requested")
            };
            Ok(json!({"record":record,"presentation":presentation}))
        }
        "plugins.popups.get" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                id: String,
            }
            crate::plugin_popup::status(&params::<P>(p)?.id)
        }
        "plugins.popups.close" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                id: String,
                frontend: crate::frontend_commands::Target,
            }
            let p: P = params(p)?;
            crate::frontend_commands::submit(
                request,
                crate::frontend_commands::Params::popup(
                    p.frontend,
                    crate::frontend_commands::Command::ClosePluginPopup { id: p.id },
                ),
            )
        }
        "plugins.panes.focus" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                id: String,
                expected_epoch: String,
                frontend: crate::frontend_commands::Target,
                placement: Option<crate::plugin_panes::Placement>,
            }
            let p: P = params(p)?;
            let record = crate::plugin_panes::get(&p.id)?;
            if record.epoch != p.expected_epoch {
                return Err(fault("stale_target", "Plugin pane epoch changed"));
            }
            crate::frontend_commands::submit(
                request,
                crate::frontend_commands::Params::plugin_pane(p.frontend, &record, p.placement),
            )
        }
        "plugins.panes.close" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                id: String,
                expected_epoch: String,
            }
            let p: P = params(p)?;
            crate::plugin_panes::close(&p.id, &p.expected_epoch)
        }
        "plugins.run" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                plugin: String,
                action: String,
                expected_digest: String,
                project: Option<String>,
            }
            let p: P = params(p)?;
            let context = p
                .project
                .as_deref()
                .map(project)
                .transpose()?
                .as_ref()
                .map(crate::plugins::project);
            crate::plugin_runner::enqueue(
                &p.plugin,
                &p.action,
                &p.expected_digest,
                context,
                request
                    .operation_id
                    .as_deref()
                    .context("Missing operation_id")?,
            )
        }
        "plugins.jobs" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                plugin: Option<String>,
            }
            let p: P = params(p)?;
            crate::plugin_runner::list(p.plugin.as_deref())
        }
        "plugins.job" | "plugins.cancel" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                job_id: String,
            }
            let p: P = params(p)?;
            if request.method == "plugins.cancel" {
                crate::plugin_runner::cancel(&p.job_id)
            } else {
                crate::plugin_runner::get(&p.job_id, true)
            }
        }
        "frontends.list" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                project: Option<String>,
            }
            let p: P = params(p)?;
            if let Some(id) = &p.project {
                project(id)?;
            }
            crate::frontend_commands::list(p.project.as_deref())
        }
        "frontends.command" => crate::frontend_commands::submit(request, params(p)?),
        "frontends.command_status" | "frontends.cancel_command" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct P {
                command_id: String,
            }
            let p: P = params(p)?;
            if request.method == "frontends.command_status" {
                crate::frontend_commands::status(&p.command_id)
            } else {
                crate::frontend_commands::cancel(&p.command_id)
            }
        }
        _ => bail!("This method requires the streaming transport"),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
fn wait(value: &Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct P {
        target: Value,
        state: Option<String>,
        terminal: Option<String>,
        #[serde(default = "reported")]
        source: String,
        #[serde(default = "deadline")]
        timeout_ms: u64,
    }
    fn reported() -> String {
        "reported".into()
    }
    let p: P = params(value)?;
    let t = target(&p.target)?;
    ensure!(
        (100..=600000).contains(&p.timeout_ms),
        "timeout_ms must be 100..600000"
    );
    ensure!(
        matches!(p.source.as_str(), "reported" | "any"),
        "source must be reported or any"
    );
    ensure!(
        p.state.is_some() != p.terminal.is_some(),
        "Provide exactly one of state or terminal"
    );
    if let Some(state) = &p.state {
        let _: agent_state::Lifecycle = serde_json::from_value(json!(state))?;
    }
    if let Some(terminal) = &p.terminal {
        ensure!(
            matches!(terminal.as_str(), "exited" | "removed"),
            "terminal must be exited or removed"
        );
    }
    let start = Instant::now();
    loop {
        let config = project(&t.project)?;
        let state = ProjectState::load(&config)?;
        let id = usize::from(
            t.pane
                .checked_sub(1)
                .filter(|p| *p < 32)
                .context("pane must be 1..32")?,
        );
        if !state.terminal_ids().contains(&id) {
            if p.terminal.as_deref() == Some("removed") {
                return Ok(json!({"matched":true,"terminal":"removed","target":t}));
            }
            return Err(fault("stale_target", "Target pane was removed"));
        }
        if state.terminal_epochs.get(&id) != t.epoch.as_ref() {
            return Err(fault("stale_target", "Target pane was replaced"));
        }
        let health = inspect(&config, id)?;
        if p.terminal.as_deref() == Some("exited")
            && health["exited"] == true
            && health["pid"].as_u64() == Some(u64::from(t.pid))
        {
            return Ok(json!({"matched":true,"observation":observed(&config,&state,id,&health)}));
        }
        if health["alive"] == true {
            let (_, _, _, health) = guarded(&t)?;
            if p.state.as_deref().is_some_and(|s| health["state"] == s)
                && (p.source == "any" || health["health"]["confidence"] == "reported")
            {
                return Ok(
                    json!({"matched":true,"observation":observed(&config,&state,id,&health)}),
                );
            }
        }
        if start.elapsed() >= Duration::from_millis(p.timeout_ms) {
            return Err(fault(
                "wait_timeout",
                "Condition was not observed before the deadline",
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
pub fn run(request: Option<&str>) -> Result<()> {
    transport::run(request)
}
pub fn worker() -> Result<()> {
    transport::worker()
}
pub fn remote(
    host: &str,
    executable: &str,
    config: Option<&std::path::Path>,
    request: Option<&str>,
) -> Result<()> {
    transport::remote(host, executable, config, request)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writes_require_operation_ids_and_identity_fields_are_explicit() {
        let mut health =
            json!({"agent_record":{"run_id":"archived"},"health":{"report_process_matches":false}});
        assert_eq!(current_run(&health), None);
        health["health"]["report_process_matches"] = json!(true);
        assert_eq!(current_run(&health), Some("archived"));
        let mut request: Request =
            serde_json::from_value(json!({"version":1,"id":1,"method":"panes.send"})).unwrap();
        assert_eq!(
            request
                .validate()
                .unwrap_err()
                .downcast_ref::<Fault>()
                .unwrap()
                .code,
            "operation_id_required"
        );
        request.operation_id = Some(uuid::Uuid::new_v4().to_string());
        assert!(request.validate().is_ok());
        let mut reference = json!({"project":"p","pane":1,"pid":123,"start_ticks":5,"boot_id":"7a307521-f2e5-4647-8d7e-b1330366037e","epoch":null,"agent_pid":null,"agent_start_ticks":null,"run_id":null});
        assert!(target(&reference).is_ok());
        reference.as_object_mut().unwrap().remove("epoch");
        assert!(target(&reference).is_err());
        reference["epoch"] = Value::Null;
        reference["agent_pid"] = json!(123);
        assert!(target(&reference).is_err());
        assert_eq!(
            failed(Value::Null, fault("operation_indeterminate", "incomplete"))["error"]["outcome"],
            "unknown"
        );
    }
}
