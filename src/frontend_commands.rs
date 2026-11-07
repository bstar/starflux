//! Incarnation-bound presentation commands with durable at-most-once receipts.
use crate::{
    automation::{fault, persist},
    project_sync::Transaction,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub frontend: String,
    pub workspace: String,
    pub pid: u32,
    pub start_ticks: u64,
    pub boot_id: String,
    pub namespace: [u64; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Space {
    Terminals,
    Apps,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    OpenPluginPopup {
        project: String,
        plugin: String,
        entrypoint: String,
        digest: String,
        options: crate::plugin_popup::Options,
    },
    ClosePluginPopup {
        id: String,
    },
    PresentPluginPane {
        project: String,
        id: String,
        epoch: String,
        placement: Option<crate::plugin_panes::Placement>,
    },
    SelectProject {
        project: String,
    },
    SelectPane {
        project: String,
        pane: u8,
        epoch: Option<String>,
    },
    SelectSpace {
        project: String,
        space: Space,
    },
    OpenSession {
        session: String,
    },
    FocusPane {
        project: String,
        direction: Direction,
    },
    SetZoom {
        project: String,
        enabled: bool,
    },
}
impl Command {
    fn validate(&self) -> Result<()> {
        let valid_project = |project: &str| {
            !project.is_empty()
                && project.len() <= 32
                && project
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        };
        let valid = match self {
            Self::OpenPluginPopup {
                project,
                plugin,
                entrypoint,
                digest,
                options,
            } => {
                options
                    .validate()
                    .map_err(|error| fault("invalid_params", error.to_string()))?;
                valid_project(project)
                    && crate::plugins::valid(plugin)
                    && crate::plugins::valid(entrypoint)
                    && !entrypoint.contains('.')
                    && digest.len() == 64
                    && digest.bytes().all(|b| b.is_ascii_hexdigit())
            }
            Self::ClosePluginPopup { id } => uuid::Uuid::parse_str(id).is_ok(),
            Self::PresentPluginPane {
                project,
                id,
                epoch,
                placement,
            } => {
                valid_project(project)
                    && uuid::Uuid::parse_str(id).is_ok()
                    && uuid::Uuid::parse_str(epoch).is_ok()
                    && *placement != Some(crate::plugin_panes::Placement::Popup)
            }
            Self::OpenSession { session } => crate::session_catalog::valid(session).is_ok(),
            Self::SelectProject { project }
            | Self::SelectSpace { project, .. }
            | Self::FocusPane { project, .. }
            | Self::SetZoom { project, .. } => valid_project(project),
            Self::SelectPane {
                project,
                pane,
                epoch,
            } => {
                valid_project(project)
                    && (1..=32).contains(pane)
                    && epoch.as_ref().is_none_or(|s| {
                        !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
                    })
            }
        };
        if !valid {
            return Err(fault(
                "invalid_params",
                "Invalid command project, session, pane or epoch",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Receipt {
    version: u32,
    operation_id: String,
    target: Target,
    command: Command,
    expires_ns: u64,
    status: String,
    result: Option<Value>,
    error: Option<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Params {
    target: Target,
    command: Command,
    #[serde(default = "ttl")]
    ttl_ms: u64,
    #[serde(default = "wait")]
    wait_ms: u64,
}
impl Params {
    pub fn popup(target: Target, command: Command) -> Self {
        Self {
            target,
            command,
            ttl_ms: ttl(),
            wait_ms: wait(),
        }
    }

    pub fn plugin_pane(
        target: Target,
        record: &crate::plugin_panes::Record,
        placement: Option<crate::plugin_panes::Placement>,
    ) -> Self {
        Self {
            target,
            command: Command::PresentPluginPane {
                project: record.project.id.clone(),
                id: record.id.clone(),
                epoch: record.epoch.clone(),
                placement,
            },
            ttl_ms: ttl(),
            wait_ms: wait(),
        }
    }
}
fn ttl() -> u64 {
    5000
}
fn wait() -> u64 {
    3000
}
fn namespace() -> Result<[u64; 2]> {
    let meta = fs::metadata(crate::runtime::root()?)?;
    Ok([meta.dev(), meta.ino()])
}
fn identity(target: &Target) -> bool {
    namespace().ok() == Some(target.namespace)
        && crate::agent_state::boot_id().is_some_and(|b| b.to_string() == target.boot_id)
        && crate::agent_state::process_start(target.pid) == Some(target.start_ticks)
        && fs::read(format!("/proc/{}/cmdline", target.pid)).is_ok_and(|args| {
            let args: Vec<_> = args.split(|b| *b == 0).collect();
            args.contains(&b"--serve-view".as_slice()) && args.contains(&target.frontend.as_bytes())
        })
}
pub fn current(frontend: &str, workspace: &str) -> Result<Target> {
    Ok(Target {
        frontend: frontend.into(),
        workspace: workspace.into(),
        pid: std::process::id(),
        start_ticks: crate::agent_state::process_start(std::process::id())
            .context("Controller identity unavailable")?,
        boot_id: crate::agent_state::boot_id()
            .context("Boot identity unavailable")?
            .to_string(),
        namespace: namespace()?,
    })
}
fn read<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> Result<T> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Frontend metadata exceeds its limit"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
fn observe(view: &crate::frontend::View) -> Result<Value> {
    let snapshot: Value = read(
        &crate::runtime::root()?.join(format!("{}.state.json", view.id)),
        8 * 1024 * 1024,
    )?;
    let target: Target = serde_json::from_value(
        json!({"frontend":view.id,"workspace":view.workspace,"pid":snapshot["controller_pid"],"start_ticks":snapshot["controller_start_ticks"],"boot_id":snapshot["controller_boot_id"],"namespace":namespace()?}),
    )?;
    ensure!(
        identity(&target)
            && snapshot["frontend"] == view.id
            && snapshot["workspace"] == view.workspace,
        "Frontend incarnation is no longer live"
    );
    let now = crate::runtime::monotonic_ns();
    ensure!(
        snapshot["controller_observed_ns"]
            .as_u64()
            .is_some_and(|time| time <= now && now - time < 15_000_000_000),
        "Frontend heartbeat is stale"
    );
    Ok(
        json!({"frontend":view.id,"workspace":view.workspace,"project":snapshot["active_project"],"session":snapshot["collection"].as_str().unwrap_or("default"),"commands_available":snapshot["frontend_commands"]==1,"plugin_pane_commands":snapshot["frontend_plugin_panes"]==1,"plugin_popup_commands":snapshot["frontend_plugin_popups"]==true,"busy":snapshot["frontend_command_busy"],"presentation":snapshot["presentation"],"app_space_available":snapshot["app_view_available"],"target":target}),
    )
}
pub fn list(project: Option<&str>) -> Result<Value> {
    let mut views = Vec::new();
    for entry in fs::read_dir(crate::runtime::root()?)?.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .ends_with(".frontend.json")
        {
            continue;
        }
        let Ok(view) = read::<crate::frontend::View>(&entry.path(), 8192) else {
            continue;
        };
        if let Ok(observed) = observe(&view)
            && project.is_none_or(|p| p == view.workspace || observed["project"] == p)
        {
            views.push(observed);
        }
    }
    views.sort_by_key(|v| v["frontend"].as_str().unwrap_or("").to_owned());
    Ok(json!({"frontends":views}))
}
fn root() -> Result<PathBuf> {
    let root = crate::project_state::data_root()?
        .parent()
        .context("No automation data directory")?
        .join("automation/frontends");
    starkit::terminal_graphics::session::private_root(&root)?;
    Ok(root)
}
fn id(value: &str) -> Result<String> {
    Ok(uuid::Uuid::parse_str(value)
        .map_err(|_| fault("invalid_params", "command operation ID must be a UUID"))?
        .simple()
        .to_string())
}
fn save(receipt: &Receipt) -> Result<()> {
    let bytes = serde_json::to_vec(receipt)?;
    ensure!(bytes.len() <= 16384, "Frontend receipt exceeds its limit");
    persist(
        &root()?.join(format!("{}.json", receipt.operation_id)),
        &bytes,
    )
}
fn load(operation: &str) -> Result<Receipt> {
    let r: Receipt = read(&root()?.join(format!("{operation}.json")), 16384)?;
    ensure!(
        r.version == 1 && r.operation_id == operation,
        "Invalid frontend command receipt"
    );
    Ok(r)
}
fn lock(operation: &str) -> Result<Transaction> {
    Transaction::acquire(root()?.join(format!("{operation}.lock")))
}
fn reject(receipt: &mut Receipt, status: &str, code: &str, message: &str) {
    receipt.status = status.into();
    receipt.error = Some(
        json!({"code":code,"message":message,"outcome":if status=="indeterminate"{"unknown"}else{"not_applied"}}),
    );
}
pub fn status(operation: &str) -> Result<Value> {
    let operation = id(operation)?;
    let _lock = lock(&operation)?;
    let mut receipt = load(&operation)?;
    if receipt.status == "queued" {
        if !identity(&receipt.target) {
            reject(
                &mut receipt,
                "rejected",
                "stale_frontend",
                "Controller incarnation ended before applying the command",
            );
            save(&receipt)?;
        } else if crate::runtime::monotonic_ns() >= receipt.expires_ns {
            reject(
                &mut receipt,
                "expired",
                "command_expired",
                "Command expired before it was applied",
            );
            save(&receipt)?;
        }
    } else if receipt.status == "applying" && !identity(&receipt.target) {
        reject(
            &mut receipt,
            "indeterminate",
            "acknowledgement_lost",
            "Controller ended after claiming the command; it was not repeated",
        );
        save(&receipt)?;
    }
    Ok(serde_json::to_value(receipt)?)
}
pub fn cancel(operation: &str) -> Result<Value> {
    let operation = id(operation)?;
    let _lock = lock(&operation)?;
    let mut receipt = load(&operation)?;
    if receipt.status != "queued" {
        return Err(fault(
            "command_committed",
            "Only a queued command can be cancelled; inspect its receipt",
        ));
    }
    reject(
        &mut receipt,
        "cancelled",
        "command_cancelled",
        "Cancelled before controller claimed the command",
    );
    save(&receipt)?;
    Ok(serde_json::to_value(receipt)?)
}
fn target_snapshot(target: &Target) -> Result<Value> {
    let view = crate::frontend::View::new(&target.workspace, Some(&target.frontend))?;
    let observed = observe(&view).map_err(|_| {
        fault(
            "stale_frontend",
            "Frontend is unavailable or its heartbeat is stale",
        )
    })?;
    if observed["commands_available"] != true {
        return Err(fault(
            "unsupported_capability",
            "Refresh this frontend to enable acknowledged commands",
        ));
    }
    if serde_json::from_value::<Target>(observed["target"].clone())? != *target {
        return Err(fault(
            "stale_frontend",
            "Frontend incarnation changed; list frontends again",
        ));
    }
    Ok(observed)
}
/// Creation with a target must reject an already stale/busy/wrong-project view
/// before launching anything. The controller checks again when placing it.
pub fn plugin_target(target: &Target, project: &str) -> Result<()> {
    let observed = target_snapshot(target)?;
    if observed["plugin_pane_commands"] != true {
        return Err(fault(
            "unsupported_capability",
            "Refresh this frontend to enable plugin pane placement",
        ));
    }
    if observed["busy"] == true {
        return Err(fault(
            "frontend_busy",
            "The target frontend has a dialog, menu, navigation or history review active",
        ));
    }
    if observed["project"] != project {
        return Err(fault(
            "frontend_project_changed",
            "Select the intended project before opening a plugin pane",
        ));
    }
    Ok(())
}
pub fn submit(request: &crate::automation::Request, params: Params) -> Result<Value> {
    params.command.validate()?;
    if !(100..=30000).contains(&params.ttl_ms)
        || params.wait_ms > 10000
        || params.wait_ms + 500 > request.deadline_ms
    {
        return Err(fault(
            "invalid_params",
            "ttl_ms must be 100..30000 and wait_ms 0..10000, at least 500 ms below the request deadline",
        ));
    }
    let observed = target_snapshot(&params.target)?;
    if matches!(params.command, Command::PresentPluginPane { .. })
        && observed["plugin_pane_commands"] != true
    {
        return Err(fault(
            "unsupported_capability",
            "Refresh this frontend to enable plugin pane placement",
        ));
    }
    if matches!(
        params.command,
        Command::OpenPluginPopup { .. } | Command::ClosePluginPopup { .. }
    ) && observed["plugin_popup_commands"] != true
    {
        return Err(fault(
            "unsupported_capability",
            "Refresh a native terminal frontend to enable plugin popups",
        ));
    }
    let operation = id(request
        .operation_id
        .as_deref()
        .context("Missing operation ID")?)?;
    {
        let _lock = lock(&operation)?;
        let directory =
            crate::runtime::root()?.join(format!("{}.commands", params.target.frontend));
        if root()?.join(format!("{operation}.json")).exists() {
            let saved = load(&operation)?;
            if saved.target != params.target || saved.command != params.command {
                return Err(fault(
                    "operation_id_conflict",
                    "Command operation ID was already used",
                ));
            }
        } else {
            // Serialize capacity checks with publication across API workers.
            let _queue_guard = Transaction::acquire(
                crate::runtime::root()?.join(format!("{}.commands.lock", params.target.frontend)),
            )?;
            if fs::read_dir(&directory)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|entry| entry.path().extension().is_some_and(|e| e == "json"))
                        .count()
                })
                .unwrap_or(0)
                >= 32
            {
                return Err(fault("frontend_backlog", "Frontend command queue is full"));
            }
            let receipt = Receipt {
                version: 1,
                operation_id: operation.clone(),
                target: params.target,
                command: params.command,
                expires_ns: crate::runtime::monotonic_ns() + params.ttl_ms * 1_000_000,
                status: "queued".into(),
                result: None,
                error: None,
            };
            save(&receipt)?;
            persist(
                &directory.join(format!(
                    "{:020}-{operation}.json",
                    crate::runtime::monotonic_ns()
                )),
                b"1",
            )?;
        }
    }
    let began = Instant::now();
    loop {
        let receipt = status(&operation)?;
        if !matches!(receipt["status"].as_str(), Some("queued" | "applying"))
            || began.elapsed() >= Duration::from_millis(params.wait_ms)
        {
            return Ok(receipt);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
pub fn process(
    frontend: &str,
    workspace: &str,
    mut apply: impl FnMut(&Command) -> Result<Value>,
) -> Result<()> {
    let own = current(frontend, workspace)?;
    let directory = crate::runtime::root()?.join(format!("{frontend}.commands"));
    let Ok(files) = fs::read_dir(directory) else {
        return Ok(());
    };
    let mut files: Vec<_> = files
        .flatten()
        .map(|f| f.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    for path in files.into_iter().take(4) {
        let Some(operation) = path
            .file_stem()
            .and_then(|p| p.to_str())
            .and_then(|s| s.split_once('-'))
            .and_then(|(_, s)| id(s).ok())
        else {
            let _ = fs::remove_file(path);
            continue;
        };
        let Some(guard) = Transaction::try_acquire(root()?.join(format!("{operation}.lock")))?
        else {
            continue;
        };
        let Ok(mut receipt) = load(&operation) else {
            let _ = fs::remove_file(path);
            continue;
        };
        if receipt.status == "applying" {
            reject(
                &mut receipt,
                "indeterminate",
                "acknowledgement_lost",
                "Previous claim has no acknowledgement; command was not repeated",
            );
            save(&receipt)?;
        }
        if receipt.status != "queued" {
            let _ = fs::remove_file(path);
            continue;
        }
        if receipt.target != own {
            reject(
                &mut receipt,
                "rejected",
                "stale_frontend",
                "Command names a different controller incarnation",
            );
            save(&receipt)?;
            let _ = fs::remove_file(path);
            continue;
        }
        if crate::runtime::monotonic_ns() >= receipt.expires_ns {
            reject(
                &mut receipt,
                "expired",
                "command_expired",
                "Command expired before it was applied",
            );
            save(&receipt)?;
            let _ = fs::remove_file(path);
            continue;
        }
        receipt.status = "applying".into();
        save(&receipt)?;
        drop(guard);
        let outcome = apply(&receipt.command);
        let _guard = lock(&operation)?;
        match outcome {
            Ok(result) => {
                receipt.status = "applied".into();
                receipt.result = Some(result);
            }
            Err(error) => {
                let response = crate::automation::failed(Value::Null, error);
                receipt.status = if response["error"]["outcome"] == "not_applied" {
                    "rejected"
                } else {
                    "failed"
                }
                .into();
                receipt.error = Some(response["error"].clone());
            }
        }
        save(&receipt)?;
        let _ = fs::remove_file(path);
    }
    Ok(())
}
