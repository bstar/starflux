//! Host-owned Git worktree operations and persistent project associations.
use crate::{
    project_state::{self, ProjectState},
    project_sync,
    runtime::Config,
};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const HELPER: &str = include_str!("../scripts/git-worktrees.py");
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Catalog {
    pub repository: Value,
    pub worktrees: Vec<Value>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum State {
    Active,
    Removing,
    Removed,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Link {
    pub project: String,
    pub origin: String,
    pub host: Option<String>,
    pub ssh_config: Option<PathBuf>,
    pub target: Value,
    state: State,
    #[serde(default)]
    last_app_launch: u64,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn path(id: &str) -> Result<PathBuf> {
    crate::runtime::config_path(id)?;
    Ok(project_state::data_root()?.join(format!("{id}.worktree.json")))
}
pub fn link(config: &Config) -> Result<Option<Link>> {
    match fs::read(path(&config.workspace)?) {
        Ok(bytes) => {
            let link: Link = serde_json::from_slice(&bytes)?;
            ensure!(
                link.project == config.workspace,
                "Worktree association belongs to another project"
            );
            Ok(Some(link))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn links() -> Result<Vec<Link>> {
    Ok(fs::read_dir(project_state::data_root()?)?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".worktree.json"))
        .filter_map(|e| {
            fs::read(e.path())
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        })
        .collect())
}
fn save(link: &Link) -> Result<()> {
    starkit::fs::write_private(&path(&link.project)?, &serde_json::to_vec_pretty(link)?)?;
    project_state::catalog_changed()
}
fn same_host(link: &Link, config: &Config) -> bool {
    link.host == config.host && link.ssh_config == config.ssh_config
}
fn same_directory(a: &Value, b: &Value) -> bool {
    ["path", "device", "inode"].iter().all(|k| a[*k] == b[*k])
        && a["repository"]["common_dir"] == b["repository"]["common_dir"]
}
fn group(config: &Config, repository: &Value) -> Result<project_sync::Transaction> {
    let root = project_state::data_root()?
        .parent()
        .context("No data directory")?
        .join("worktrees");
    starkit::terminal_graphics::session::private_root(&root)?;
    let key = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json!([
            config.host,
            config.ssh_config,
            repository["common_dir"]
        ]))?)
    );
    project_sync::Transaction::acquire(root.join(format!("{key}.lock")))
}
fn host(config: &Config, action: &str, mut request: Value) -> Result<Value> {
    request["action"] = json!(action);
    let mut directory = match &config.directory {
        Some(directory) => directory.clone(),
        None if config.host.is_some() => "~".into(),
        None => std::env::current_dir()
            .context("Project working directory is unavailable")?
            .display()
            .to_string(),
    };
    if config.host.is_none()
        && !directory.starts_with('~')
        && !std::path::Path::new(&directory).is_absolute()
    {
        directory = std::env::current_dir()?
            .join(&directory)
            .display()
            .to_string();
    }
    request["directory"] = json!(directory);
    let encoded = STANDARD.encode(serde_json::to_vec(&request)?);
    let mut command = if let Some(remote) = &config.host {
        let mut c = Command::new("ssh");
        if let Some(path) = &config.ssh_config {
            c.arg("-F").arg(path);
        }
        c.args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "ServerAliveInterval=5",
            "-o",
            "ServerAliveCountMax=1",
            "--",
            remote,
        ])
        .arg(format!(
            "python3 -c {} {}",
            shell_words::quote(HELPER),
            shell_words::quote(&encoded)
        ));
        c
    } else {
        let mut c = Command::new("python3");
        c.args(["-c", HELPER, &encoded]);
        c
    };
    let mut output = tempfile::tempfile()?;
    let mut error = tempfile::tempfile()?;
    let mut child = command
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(error.try_clone()?)
        .process_group(0)
        .spawn()?;
    let start = Instant::now();
    let status = loop {
        if start.elapsed() > Duration::from_secs(35) || output.metadata()?.len() > 2 * 1024 * 1024 {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            bail!(
                "Worktree operation exceeded its time or output limit; refresh to inspect its outcome"
            );
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    error.rewind()?;
    let mut message = String::new();
    error.take(2048).read_to_string(&mut message)?;
    ensure!(status.success(), "Worktree host: {}", message.trim());
    output.rewind()?;
    let response: Value = serde_json::from_reader(output.take(2 * 1024 * 1024))?;
    ensure!(
        response["ok"] == true,
        "{}: {}",
        response["error"]["code"]
            .as_str()
            .unwrap_or("worktree_error"),
        response["error"]["message"]
            .as_str()
            .unwrap_or("Worktree host failed")
    );
    Ok(response["result"].clone())
}
pub fn list(config: &Config) -> Result<Catalog> {
    let mut result: Catalog = serde_json::from_value(host(config, "list", json!({}))?)?;
    let pending = links()?
        .into_iter()
        .filter(|l| {
            same_host(l, config)
                && l.state == State::Removing
                && l.target["repository"]["common_dir"] == result.repository["common_dir"]
        })
        .collect::<Vec<_>>();
    if !pending.is_empty() {
        let _group = group(config, &result.repository)?;
        result = serde_json::from_value(host(config, "list", json!({}))?)?;
        for mut link in pending {
            link.state = if result
                .worktrees
                .iter()
                .any(|t| same_directory(&link.target, t))
            {
                State::Active
            } else {
                State::Removed
            };
            save(&link)?;
        }
    }
    Ok(result)
}
pub fn create(
    config: &Config,
    branch: &str,
    path: Option<&str>,
    base: Option<&str>,
) -> Result<Value> {
    let catalog = list(config)?;
    let _group = group(config, &catalog.repository)?;
    Ok(host(
        config,
        "create",
        json!({"branch":branch,"path":path,"base":base}),
    )?["worktree"]
        .clone())
}
fn repository_origin(config: &Config) -> Result<String> {
    if let Some(link) = link(config)? {
        return Ok(link.origin);
    }
    let catalog: Catalog = serde_json::from_value(host(config, "list", json!({}))?)?;
    let primary = catalog
        .worktrees
        .first()
        .context("Repository has no primary worktree")?;
    if primary["current"] == true {
        return Ok(config.workspace.clone());
    }
    if let Some(existing) = crate::session_catalog::projects().into_iter().find(|p| {
        p.host == config.host
            && p.ssh_config == config.ssh_config
            && p.collection == config.collection
            && p.directory.as_deref() == primary["path"].as_str()
    }) {
        return Ok(existing.workspace);
    }
    let mut root = config.clone();
    root.workspace = format!("tree-{}", &uuid::Uuid::new_v4().simple().to_string()[..16]);
    root.directory = Some(
        primary["path"]
            .as_str()
            .context("Primary worktree has no path")?
            .into(),
    );
    root.label = Some(
        std::path::Path::new(root.directory.as_deref().unwrap())
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .take(64)
            .collect(),
    );
    root.agent = None;
    root.herdr = None;
    root.tmux = None;
    root.adopted.clear();
    root.project_host = true;
    save(&Link {
        project: root.workspace.clone(),
        origin: root.workspace.clone(),
        host: root.host.clone(),
        ssh_config: root.ssh_config.clone(),
        target: primary.clone(),
        state: State::Active,
        last_app_launch: 0,
    })?;
    ProjectState::default().save(&root)?;
    Ok(root.workspace)
}
pub fn open(config: &Config, target: &Value) -> Result<Config> {
    let _group = group(config, &target["repository"])?;
    let target = host(config, "validate", json!({"target":target}))?["worktree"].clone();
    let existing = links()?.into_iter().find(|l| {
        same_host(l, config)
            && same_directory(&l.target, &target)
            && l.state != State::Removed
            && crate::saved(&l.project).is_ok_and(|project| project.collection == config.collection)
    });
    let destination = if let Some(mut association) = existing {
        association.target = target.clone();
        association.state = State::Active;
        save(&association)?;
        crate::saved(&association.project)?
    } else if target["current"] == true {
        if target["main"] != true {
            save(&Link {
                project: config.workspace.clone(),
                origin: repository_origin(config)?,
                host: config.host.clone(),
                ssh_config: config.ssh_config.clone(),
                target: target.clone(),
                state: State::Active,
                last_app_launch: 0,
            })?;
        }
        config.clone()
    } else {
        let origin = repository_origin(config)?;
        let branch = target["branch"]
            .as_str()
            .unwrap_or("")
            .strip_prefix("refs/heads/")
            .unwrap_or_else(|| target["head"].as_str().unwrap_or("detached"));
        let mut destination = config.clone();
        destination.workspace =
            format!("tree-{}", &uuid::Uuid::new_v4().simple().to_string()[..16]);
        destination.label = Some(format!(
            "{} / {}",
            config
                .label
                .as_deref()
                .unwrap_or(&config.workspace)
                .chars()
                .take(24)
                .collect::<String>(),
            branch.chars().take(36).collect::<String>()
        ));
        destination.directory = Some(
            target["path"]
                .as_str()
                .context("Worktree has no path")?
                .into(),
        );
        destination.agent = None;
        destination.herdr = None;
        destination.tmux = None;
        destination.adopted.clear();
        destination.project_host = true;
        let association = Link {
            project: destination.workspace.clone(),
            origin,
            host: destination.host.clone(),
            ssh_config: destination.ssh_config.clone(),
            target: target.clone(),
            state: State::Active,
            last_app_launch: 0,
        };
        save(&association)?;
        ProjectState::default().save(&destination)?;
        destination
    };
    drop(_group);
    // Maintain project -> repository lock order for ordinary terminal startup.
    let _project = project_sync::transaction(&destination)?;
    let mut state = ProjectState::load(&destination)?;
    if state.terminal_ids().is_empty() {
        state.add_terminal()?;
        state.save(&destination)?;
    }
    for pane in state.terminal_ids() {
        destination.ensure(pane, None)?;
    }
    Ok(destination)
}
pub fn remove(config: &Config, target: &Value) -> Result<Value> {
    let _group = group(config, &target["repository"])?;
    host(config, "validate", json!({"target":target}))?;
    let mut associated = links()?
        .into_iter()
        .filter(|l| {
            same_host(l, config) && same_directory(&l.target, target) && l.state != State::Removed
        })
        .collect::<Vec<_>>();
    for link in &associated {
        ensure!(
            now().saturating_sub(link.last_app_launch) > 30000,
            "An app launch may still be pending; wait before removing the worktree"
        );
        let project = crate::saved(&link.project)?;
        let state = ProjectState::load(&project)?;
        for pane in state.terminal_ids() {
            let status = project.call(pane, &project.request(pane, "inspect"))?;
            ensure!(
                status["alive"] == false,
                "Project {} still has a running terminal; terminate it explicitly before removing its worktree",
                project.workspace
            );
            ensure!(
                status["state"] != "disconnected",
                "Terminal health is unavailable; removal was left pending"
            );
        }
    }
    for link in &mut associated {
        link.state = State::Removing;
        save(link)?;
    }
    let outcome = host(config, "remove", json!({"target":target}));
    match outcome {
        Ok(result) => {
            for link in &mut associated {
                link.state = State::Removed;
                save(link)?;
            }
            Ok(result)
        }
        Err(error) => {
            if let Ok(result) = host(config, "list", json!({}))
                && let Ok(catalog) = serde_json::from_value::<Catalog>(result)
            {
                for link in &mut associated {
                    link.state = if catalog
                        .worktrees
                        .iter()
                        .any(|t| same_directory(&link.target, t))
                    {
                        State::Active
                    } else {
                        State::Removed
                    };
                    save(link)?;
                }
            }
            Err(error)
        }
    }
}
pub fn ensure_allowed(config: &Config) -> Result<Option<project_sync::Transaction>> {
    let Some(association) = link(config)? else {
        return Ok(None);
    };
    let lease = group(config, &association.target["repository"])?;
    let association = link(config)?.context("Worktree association vanished")?;
    ensure!(
        association.state == State::Active,
        "Worktree is being removed or is removed; reopen an existing worktree from its repository picker"
    );
    host(config, "directory", json!({"target":association.target}))?;
    Ok(Some(lease))
}
pub fn mark_app_launch(config: &Config) -> Result<()> {
    let _lease = ensure_allowed(config)?;
    if let Some(mut link) = link(config)? {
        link.last_app_launch = now();
        save(&link)?;
    }
    Ok(())
}
pub fn removed(config: &Config) -> bool {
    link(config)
        .ok()
        .flatten()
        .is_some_and(|l| l.state == State::Removed)
}
pub fn removed_origin(config: &Config) -> Result<Option<String>> {
    Ok(link(config)?
        .filter(|l| l.state == State::Removed)
        .map(|l| l.origin))
}
