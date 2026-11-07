//! Explicitly enabled executable workflow packages; no terminal input injection.
use crate::{
    automation::fault,
    plugin_store::{Store, read},
    runtime::Config,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub id: String,
    pub title: String,
    pub argv: Vec<String>,
    #[serde(default = "default_cwd")]
    pub cwd: String,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
}
fn default_cwd() -> String {
    "package".into()
}
fn timeout() -> u64 {
    30000
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    pub id: String,
    pub on: String,
    pub action: String,
    #[serde(default)]
    pub states: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkHandler {
    pub id: String,
    pub title: String,
    pub action: String,
    pub schemes: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub actions: Vec<Action>,
    #[serde(default)]
    pub hooks: Vec<Hook>,
    #[serde(default)]
    pub link_handlers: Vec<LinkHandler>,
    #[serde(default)]
    pub panes: Vec<crate::plugin_panes::Entrypoint>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: PathBuf,
    pub enabled: bool,
    pub approved: Option<String>,
    pub activation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed: Option<crate::plugin_packages::Source>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub version: u32,
    pub revision: String,
    pub plugins: BTreeMap<String, Entry>,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            version: 1,
            revision: "initial".into(),
            plugins: BTreeMap::new(),
        }
    }
}
pub fn valid(id: &str) -> bool {
    !id.is_empty()
        && !matches!(id, "." | "..")
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn local_id(id: &str) -> bool {
    valid(id) && !id.contains('.')
}
fn config_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".config")
        })
        .join("starflux/plugins.json")
}
pub fn data_path() -> PathBuf {
    crate::project_state::data_path()
        .parent()
        .unwrap()
        .join("plugins")
}
pub fn store() -> Result<Store> {
    let path = data_path();
    starkit::terminal_graphics::session::private_root(&path)?;
    Store::open(path)
}
pub fn registry() -> Result<Registry> {
    let path = config_path();
    if !path.exists() {
        return Ok(Registry::default());
    }
    let r: Registry = read(&path, 1024 * 1024)?;
    ensure!(
        r.version == 1 && r.plugins.len() <= 64 && r.plugins.keys().all(|s| valid(s)),
        "Invalid plugin registry"
    );
    Ok(r)
}
pub fn package(path: &Path) -> Result<(Manifest, String)> {
    let file = path.join("starflux-plugin.json");
    let mut bytes = Vec::new();
    fs::File::open(file)?.take(65537).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 65536, "Plugin manifest exceeds 64 KiB");
    let m: Manifest = serde_json::from_slice(&bytes)?;
    ensure!(
        m.version == 1
            && valid(&m.id)
            && !m.name.is_empty()
            && m.name.len() <= 128
            && !m.name.chars().any(char::is_control)
            && m.description.len() <= 2048,
        "Invalid plugin identity or version"
    );
    ensure!(
        (!m.actions.is_empty() || !m.panes.is_empty())
            && m.actions.len() <= 32
            && m.panes.len() <= 32
            && m.hooks.len() <= 32
            && m.link_handlers.len() <= 32,
        "Plugin entrypoint count exceeds its limit"
    );
    let mut ids = BTreeSet::new();
    for a in &m.actions {
        ensure!(
            local_id(&a.id)
                && ids.insert(a.id.clone())
                && !a.title.is_empty()
                && a.title.len() <= 128
                && !a.title.chars().any(char::is_control),
            "Invalid or duplicate plugin action"
        );
        ensure!(
            !a.argv.is_empty()
                && a.argv.len() <= 64
                && a.argv.iter().all(|s| !s.contains('\0'))
                && a.argv.iter().map(String::len).sum::<usize>() <= 16384
                && !a.argv[0].is_empty(),
            "Invalid plugin argv"
        );
        ensure!(
            matches!(a.cwd.as_str(), "project" | "package")
                && (100..=3600000).contains(&a.timeout_ms),
            "Invalid plugin working directory or timeout"
        );
    }
    let mut panes = BTreeSet::new();
    for pane in &m.panes {
        ensure!(
            local_id(&pane.id) && panes.insert(pane.id.clone()),
            "Invalid or duplicate plugin pane identity"
        );
        crate::project_state::validate_label(&pane.title)?;
        crate::plugin_popup::Options {
            width: pane.width.clone(),
            height: pane.height.clone(),
        }
        .validate()?;
        ensure!(
            !pane.argv.is_empty()
                && pane.argv.len() <= 64
                && pane.argv.iter().all(|s| !s.contains('\0'))
                && pane.argv.iter().map(String::len).sum::<usize>() <= 16384
                && !pane.argv[0].is_empty()
                && matches!(pane.cwd.as_str(), "package" | "project"),
            "Invalid plugin pane command or working directory"
        );
    }
    let mut hooks = BTreeSet::new();
    let mut links = BTreeSet::new();
    for handler in &m.link_handlers {
        ensure!(
            local_id(&handler.id)
                && links.insert(handler.id.clone())
                && ids.contains(&handler.action),
            "Invalid or duplicate link handler identity/action"
        );
        crate::plugin_links::validate_handler(handler)?;
    }
    for h in &m.hooks {
        ensure!(
            local_id(&h.id) && hooks.insert(h.id.clone()) && ids.contains(&h.action),
            "Invalid hook identity/action"
        );
        ensure!(
            matches!(
                h.on.as_str(),
                "startup"
                    | "project.created"
                    | "project.changed"
                    | "project.removed"
                    | "pane.created"
                    | "pane.changed"
                    | "pane.removed"
                    | "agent.reported.created"
                    | "agent.reported.changed"
                    | "agent.reported.removed"
                    | "attention.created"
                    | "attention.changed"
                    | "attention.removed"
            ),
            "Unknown plugin hook event"
        );
        ensure!(
            h.states.len() <= 16
                && h.states.iter().all(|s| matches!(
                    s.as_str(),
                    "unknown"
                        | "idle"
                        | "working"
                        | "needs_input"
                        | "waiting_for_approval"
                        | "completed"
                        | "failed"
                        | "stopped"
                        | "backend_unavailable"
                )),
            "Unknown hook state"
        );
    }
    Ok((m, format!("{:x}", Sha256::digest(bytes))))
}
pub fn package_metadata(entry: &Entry) -> Result<(Manifest, String)> {
    let (manifest, digest) = package(&entry.path)?;
    if let Some(source) = &entry.managed {
        Ok((
            manifest,
            format!(
                "{:x}",
                Sha256::digest(format!("{digest}:{}", source.tree_digest))
            ),
        ))
    } else {
        Ok((manifest, digest))
    }
}
pub fn package_for(entry: &Entry) -> Result<(Manifest, String)> {
    let (manifest, digest) = package_metadata(entry)?;
    if entry.managed.is_some() {
        crate::plugin_packages::verify(entry, &manifest.id)?;
    }
    Ok((manifest, digest))
}
pub fn list() -> Result<Value> {
    let registry = registry()?;
    let mut plugins = Vec::new();
    for (id, entry) in &registry.plugins {
        let package = package_for(entry).and_then(|(manifest, digest)| {
            ensure!(
                manifest.id == *id,
                "Plugin manifest has a different identity"
            );
            Ok((manifest, digest))
        });
        match package {
            Ok((manifest,digest))=>plugins.push(json!({"id":id,"path":entry.path,"enabled":entry.enabled&&entry.approved.as_deref()==Some(&digest),"configured_enabled":entry.enabled,"approved":entry.approved.as_deref()==Some(&digest),"digest":digest,"activation":entry.activation,"managed":entry.managed,"manifest":manifest,"warning":if entry.enabled&&entry.approved.as_deref()!=Some(&digest){Some("Manifest changed; review and enable it again")}else{None}})),
            Err(error)=>plugins.push(json!({"id":id,"path":entry.path,"managed":entry.managed,"enabled":false,"configured_enabled":entry.enabled,"warning":format!("Package unavailable: {error:#}")})),
        }
    }
    Ok(
        json!({"revision":registry.revision,"plugins":plugins,"execution_host":"workspace_controller"}),
    )
}
pub(crate) fn registry_lock_path() -> Result<PathBuf> {
    let path = config_path();
    let parent = path.parent().context("No plugin registry directory")?;
    starkit::terminal_graphics::session::private_root(parent)?;
    Ok(parent.join("plugins.lock"))
}
pub(crate) fn edit(
    expected: &str,
    change: impl FnOnce(&mut Registry) -> Result<()>,
) -> Result<Value> {
    let path = config_path();
    let parent = path.parent().unwrap();
    starkit::terminal_graphics::session::private_root(parent)?;
    let _lock = crate::project_sync::Transaction::acquire(parent.join("plugins.lock"))?;
    let mut r = registry()?;
    if r.revision != expected {
        return Err(fault(
            "stale_revision",
            "Plugin registry changed; list plugins again",
        ));
    }
    change(&mut r)?;
    r.revision = uuid::Uuid::new_v4().to_string();
    crate::automation::persist(&path, &serde_json::to_vec(&r)?)?;
    let _ = crate::plugin_runner::ensure();
    list()
}
pub fn link(path: &str, expected: &str) -> Result<Value> {
    let path = fs::canonicalize(path)?;
    ensure!(path.is_dir(), "Choose a plugin directory");
    let (m, _) = package(&path)?;
    edit(expected, |r| {
        ensure!(r.plugins.len() < 64, "Plugin registry is full");
        ensure!(
            !r.plugins.contains_key(&m.id),
            "Plugin is already linked; unlink it before replacing its source"
        );
        r.plugins.insert(
            m.id,
            Entry {
                path,
                enabled: false,
                approved: None,
                activation: uuid::Uuid::new_v4().to_string(),
                managed: None,
            },
        );
        Ok(())
    })
}
pub fn unlink(id: &str, expected: &str) -> Result<Value> {
    edit(expected, |r| {
        ensure!(r.plugins.remove(id).is_some(), "Plugin is not linked");
        Ok(())
    })
}
pub fn configure(id: &str, enabled: bool, digest: &str, expected: &str) -> Result<Value> {
    edit(expected, |r| {
        let e = r.plugins.get_mut(id).context("Plugin is not linked")?;
        if enabled {
            let (m, current) = package_for(e)?;
            ensure!(
                m.id == id && current == digest,
                "Plugin manifest changed; review its current commands"
            );
            e.approved = Some(current);
        }
        e.enabled = enabled;
        e.activation = uuid::Uuid::new_v4().to_string();
        // Arm at the current journal position; enabling never replays old events.
        if enabled {
            let cursor = crate::automation::journal::snapshot()?.1;
            store()?.write(
                &format!("cursor-{id}.json"),
                &serde_json::to_vec(&json!({"activation":e.activation,"cursor":cursor}))?,
            )?;
        }
        Ok(())
    })
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub host: Option<String>,
    pub directory: Option<String>,
    pub session: String,
}
pub fn project(config: &Config) -> Project {
    Project {
        id: config.workspace.clone(),
        host: config.host.clone(),
        directory: config.directory.clone(),
        session: crate::session_catalog::group(config),
    }
}
pub fn directories(store: &Store, id: &str) -> Result<(PathBuf, PathBuf)> {
    ensure!(valid(id), "Invalid plugin ID");
    let config = config_path()
        .parent()
        .unwrap()
        .join("plugin-config")
        .join(id);
    let state = data_path().join(format!("state-{id}"));
    starkit::terminal_graphics::session::private_root(&config)?;
    store.child(&format!("state-{id}"))?;
    Ok((config, state))
}
