//! Persistent plugin terminals. Opening is explicit; restoring a view never
//! replays a plugin command. Records bind ownership to a project slot/epoch.
use crate::{
    plugin_store::{Store, read},
    plugins,
    project_state::ProjectState,
    runtime::Config,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, os::unix::process::CommandExt, path::PathBuf, process::Command};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    #[default]
    Overlay,
    Split,
    Tab,
    Zoomed,
    Popup,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entrypoint {
    pub id: String,
    pub title: String,
    pub argv: Vec<String>,
    #[serde(default = "cwd")]
    pub cwd: String,
    #[serde(default)]
    pub placement: Placement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<crate::plugin_popup::Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<crate::plugin_popup::Dimension>,
}
fn cwd() -> String {
    "package".into()
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub version: u32,
    pub id: String,
    pub plugin: String,
    pub entrypoint: Entrypoint,
    pub source: PathBuf,
    pub digest: String,
    pub activation: String,
    pub project: plugins::Project,
    pub pane: usize,
    pub epoch: String,
    pub boot_id: String,
    pub status: String,
    pub pid: Option<u32>,
    pub start_ticks: Option<u64>,
    pub error: Option<String>,
}
fn store() -> Result<Store> {
    plugins::store()?.child("panes")
}
fn name(id: &str) -> Result<String> {
    Ok(format!("pane-{}.json", uuid::Uuid::parse_str(id)?.simple()))
}
fn save(store: &Store, record: &Record) -> Result<()> {
    let bytes = serde_json::to_vec(record)?;
    ensure!(bytes.len() <= 65536, "Plugin pane metadata exceeds 64 KiB");
    store.write(&name(&record.id)?, &bytes)
}
pub fn get(id: &str) -> Result<Record> {
    let store = store()?;
    let record: Record = read(&store.path.join(name(id)?), 65536)?;
    ensure!(
        record.version == 1 && record.id == id && record.pane < 32,
        "Invalid plugin pane record"
    );
    Ok(record)
}
fn records() -> Result<Vec<Record>> {
    let store = store()?;
    let mut records = Vec::new();
    for entry in fs::read_dir(&store.path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("pane-") && name.ends_with(".json") {
            let r: Record = read(&entry.path(), 65536)?;
            ensure!(
                r.version == 1
                    && r.pane < 32
                    && name == format!("pane-{}.json", uuid::Uuid::parse_str(&r.id)?.simple()),
                "Invalid plugin pane record"
            );
            records.push(r);
        }
    }
    Ok(records)
}
fn process_alive(record: &Record) -> bool {
    crate::agent_state::boot_id().is_some_and(|b| b.to_string() == record.boot_id)
        && record.pid.is_some_and(|p| {
            crate::agent_state::process_start(p) == record.start_ticks
                && fs::read_to_string(format!("/proc/{p}/stat")).is_ok_and(|s| {
                    s.rsplit_once(')').is_some_and(|(_, s)| {
                        !matches!(s.split_whitespace().next(), Some("Z" | "X"))
                    })
                })
        })
}
pub fn package_busy(plugin: &str) -> Result<bool> {
    Ok(records()?.iter().any(|r| {
        r.plugin == plugin
            && (process_alive(r)
                || (r.status == "launching"
                    && crate::agent_state::boot_id().is_some_and(|b| b.to_string() == r.boot_id)))
    }))
}
pub fn list(plugin: Option<&str>) -> Result<Value> {
    let mut items = Vec::new();
    for record in records()?
        .into_iter()
        .filter(|r| plugin.is_none_or(|p| r.plugin == p))
    {
        let alive = process_alive(&record);
        let status = if alive {
            "running"
        } else if record.status == "launching" {
            "indeterminate"
        } else {
            "exited"
        };
        items.push(json!({"record":record,"alive":alive,"status":status}));
    }
    Ok(json!({"panes":items}))
}
fn binding_name(project: &str, pane: usize) -> String {
    format!("binding-{project}-{pane}.json")
}
pub fn owner(config: &Config, pane: usize) -> Result<Option<Record>> {
    let state = ProjectState::load(config)?;
    if !state.terminal_ids().contains(&pane) {
        return Ok(None);
    }
    let id = if let Some(id) = state.terminal_plugins.get(&pane) {
        id.clone()
    } else {
        let path = plugins::data_path()
            .join("panes")
            .join(binding_name(&config.workspace, pane));
        if !path.exists() {
            return Ok(None);
        }
        let binding: Value = read(&path, 4096)?;
        if binding["epoch"].as_str() != state.terminal_epochs.get(&pane).map(String::as_str) {
            return Ok(None);
        }
        binding["id"]
            .as_str()
            .context("Invalid plugin pane binding")?
            .to_owned()
    };
    let record = get(&id)?;
    ensure!(
        record.project.id == config.workspace
            && record.pane == pane
            && state.terminal_epochs.get(&pane) == Some(&record.epoch),
        "Plugin pane ownership changed"
    );
    Ok(Some(record))
}
/// A replaced Linux image can report a " (deleted)" suffix while its stable
/// pathname already contains the compatible replacement. Resolve that path
/// before allocating a terminal; a missing launcher must not leave a dead tab.
pub(crate) fn launcher() -> Result<PathBuf> {
    let loaded = std::env::current_exe()?;
    if loaded.is_file() {
        return Ok(loaded);
    }
    if let Some(path) = loaded.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }
    anyhow::bail!(
        "STAR/FLUX launcher is unavailable; refresh this window before opening a plugin terminal"
    )
}
/// Creates the persistent terminal. Placement/focus is performed by the
/// requesting presentation, never by changing another window's layout.
pub fn open(
    plugin: &str,
    entrypoint: &str,
    digest: &str,
    project: &str,
    placement: Option<Placement>,
) -> Result<Record> {
    let config = crate::saved(project)?;
    let store = store()?;
    let _registry_lock = crate::project_sync::Transaction::acquire(plugins::registry_lock_path()?)?;
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
        "Plugin is disabled or changed; review it again"
    );
    let mut declared = manifest
        .panes
        .into_iter()
        .find(|p| p.id == entrypoint)
        .context("Plugin pane entrypoint is unavailable")?;
    if let Some(placement) = placement {
        declared.placement = placement;
    }
    ensure!(
        declared.placement != Placement::Popup,
        "Terminal popup presentation is not implemented yet"
    );
    let launcher = launcher()?;
    let _project_lock = crate::project_sync::transaction(&config)?;
    let _lease = crate::worktrees::ensure_allowed(&config)?;
    let mut state = ProjectState::load(&config)?;
    let pane = state.add_background_terminal()?;
    ensure!(
        !config.externally_owned(pane) && config.adopted_terminal(pane).is_none(),
        "Plugin panes require an unused native terminal slot"
    );
    let prior = config.call(pane, &config.request(pane, "inspect"))?;
    ensure!(
        prior["alive"] != true && prior["exited"] != true,
        "A backend already occupies this terminal slot; it was left unchanged"
    );
    ensure!(
        !config.remote(pane),
        "Plugin panes run on this controller; choose a project on this host"
    );
    let mut record = Record {
        version: 1,
        id: uuid::Uuid::new_v4().to_string(),
        plugin: plugin.into(),
        entrypoint: declared,
        source: entry.path.clone(),
        digest: current,
        activation: entry.activation.clone(),
        project: plugins::project(&config),
        pane,
        epoch: state.terminal_epochs[&pane].clone(),
        boot_id: crate::agent_state::boot_id()
            .context("Boot identity unavailable")?
            .to_string(),
        status: "launching".into(),
        pid: None,
        start_ticks: None,
        error: None,
    };
    // Preserve this presentation's focus until it explicitly applies placement.
    state.terminal_plugins.insert(pane, record.id.clone());
    state
        .terminal_labels
        .insert(pane, record.entrypoint.title.clone());
    save(&store, &record)?;
    store.write(
        &binding_name(&config.workspace, pane),
        &serde_json::to_vec(&json!({"version":1,"id":record.id,"epoch":record.epoch}))?,
    )?;
    state.save(&config)?;
    let mut request = config.request(pane, "ensure");
    request["directory"] = json!(record.source);
    request["argv"] = json!([
        launcher.to_string_lossy(),
        "--plugin-pane",
        &store.path.to_string_lossy(),
        &record.id
    ]);
    if let Err(error) = config.call(pane, &request) {
        record.status = "indeterminate".into();
        record.error = Some(format!("{error:#}"));
        save(&store, &record)?;
        return Err(error);
    }
    Ok(record)
}
pub fn close(id: &str, expected_epoch: &str) -> Result<Value> {
    let store = store()?;
    let _claim = store.lock(
        &format!("claim-{}.lock", uuid::Uuid::parse_str(id)?.simple()),
        true,
    )?;
    let mut record = get(id)?;
    if record.status == "closed" {
        return Ok(json!({"id":id,"closed":true}));
    }
    ensure!(record.epoch == expected_epoch, "Plugin pane changed");
    let config = crate::saved(&record.project.id)?;
    let _transaction = crate::project_sync::transaction(&config)?;
    ensure!(
        owner(&config, record.pane)?.is_some_and(|r| r.id == record.id),
        "Terminal no longer belongs to this plugin pane"
    );
    let health = config.call(record.pane, &config.request(record.pane, "inspect"))?;
    if health["alive"] == true {
        let identity = config.terminal_identity(record.pane, &health)?;
        let pending_launcher = record.status == "launching"
            && record.boot_id == identity.2.to_string()
            && fs::read(format!("/proc/{}/cmdline", identity.0)).is_ok_and(|bytes| {
                let args: Vec<_> = bytes.split(|b| *b == 0).collect();
                args.contains(&b"--plugin-pane".as_slice()) && args.contains(&record.id.as_bytes())
            });
        ensure!(
            pending_launcher
                || (record.pid == Some(identity.0)
                    && record.start_ticks == Some(identity.1)
                    && record.boot_id == identity.2.to_string()),
            "Plugin terminal process changed; it was left running"
        );
        record.status = "closing".into();
        save(&store, &record)?;
        config.terminate_terminal(record.pane, Some(&record.epoch), identity)?;
    } else {
        let reaped = config.call(record.pane, &config.request(record.pane, "reap"))?;
        ensure!(
            reaped["reaped"] == true,
            "Plugin terminal became live; it was left running"
        );
        let mut state = ProjectState::load(&config)?;
        state.remove_terminal(record.pane);
        state.save(&config)?;
    }
    record.status = "closed".into();
    save(&store, &record)?;
    Ok(json!({"id":id,"closed":true}))
}
/// Called only by the newly-created terminal, using its original private data
/// directory. A consumed receipt is never executable a second time.
pub fn worker(path: &str, id: &str) -> Result<()> {
    let store = Store::open(PathBuf::from(path))?;
    ensure!(
        store.path == plugins::data_path().join("panes"),
        "Plugin pane namespace changed"
    );
    // Serialize the final approval check with configure/uninstall. The lock's
    // close-on-exec descriptor releases at the actual program replacement.
    let _registry_lock = crate::project_sync::Transaction::acquire(plugins::registry_lock_path()?)?;
    let _claim = store.lock(
        &format!("claim-{}.lock", uuid::Uuid::parse_str(id)?.simple()),
        true,
    )?;
    let mut record = get(id)?;
    ensure!(
        record.status == "launching"
            && record.boot_id
                == crate::agent_state::boot_id()
                    .context("Boot identity unavailable")?
                    .to_string(),
        "Plugin pane launch was already consumed or belongs to another boot"
    );
    let registry = plugins::registry()?;
    let entry = registry
        .plugins
        .get(&record.plugin)
        .context("Plugin was unlinked")?;
    let (manifest, digest) = plugins::package_for(entry)?;
    ensure!(
        entry.enabled
            && entry.activation == record.activation
            && entry.path == record.source
            && digest == record.digest
            && entry.approved.as_deref() == Some(&digest)
            && manifest.panes.iter().any(|p| p.id == record.entrypoint.id),
        "Plugin changed before pane launch"
    );
    let declared = manifest
        .panes
        .iter()
        .find(|p| p.id == record.entrypoint.id)
        .context("Pane entrypoint disappeared")?;
    ensure!(
        declared.argv == record.entrypoint.argv
            && declared.cwd == record.entrypoint.cwd
            && declared.title == record.entrypoint.title
            && manifest.id == record.plugin,
        "Pane command metadata no longer matches the approved manifest"
    );
    let config = crate::saved(&record.project.id)?;
    ensure!(
        plugins::project(&config) == record.project
            && owner(&config, record.pane)?.is_some_and(|r| r.id == record.id),
        "Plugin pane project changed before launch"
    );
    let _lease = crate::worktrees::ensure_allowed(&config)?;
    let health = config.call(record.pane, &config.request(record.pane, "inspect"))?;
    let identity = config.terminal_identity(record.pane, &health)?;
    record.pid = Some(identity.0);
    record.start_ticks = Some(identity.1);
    record.status = "running".into();
    save(&store, &record)?;
    let plugin_store = plugins::store()?;
    let (config_dir, state_dir) = plugins::directories(&plugin_store, &record.plugin)?;
    let binary = launcher()?;
    let context = json!({"version":1,"plugin_id":record.plugin,"entrypoint_id":record.entrypoint.id,"plugin_pane_id":record.id,"project":record.project,"pane":record.pane+1,"epoch":record.epoch,"config_dir":config_dir,"state_dir":state_dir,"bin_path":binary});
    let program = record.entrypoint.argv[0]
        .strip_prefix("package:")
        .map_or_else(
            || PathBuf::from(&record.entrypoint.argv[0]),
            |p| record.source.join(p),
        );
    let cwd = if record.entrypoint.cwd == "project" {
        PathBuf::from(config.directory.context("Project has no directory")?)
    } else {
        record.source.clone()
    };
    let mut command = Command::new(program);
    command
        .args(&record.entrypoint.argv[1..])
        .current_dir(cwd)
        .env("STARFLUX_PLUGIN_ID", &record.plugin)
        .env("STARFLUX_PLUGIN_ENTRYPOINT_ID", &record.entrypoint.id)
        .env("STARFLUX_PLUGIN_PANE_ID", &record.id)
        .env("STARFLUX_PROJECT", &record.project.id)
        .env("STARFLUX_PANE", (record.pane + 1).to_string())
        .env("STARFLUX_PLUGIN_CONFIG_DIR", config_dir)
        .env("STARFLUX_PLUGIN_STATE_DIR", state_dir)
        .env("STARFLUX_BIN_PATH", binary)
        .env(
            "STARFLUX_PLUGIN_CONTEXT_JSON",
            serde_json::to_string(&context)?,
        );
    // The tmux-owned terminal, not a finite job worker, owns these standard
    // streams. No timeout, stdin redirection or terminal-history commands.
    let error = command.exec();
    record.status = "failed".into();
    record.error = Some(error.to_string());
    save(&store, &record)?;
    Err(error.into())
}

/// View-local restoration data; project membership and running processes are
/// deliberately not snapshotted, so restoring an overlay cannot erase new tabs.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    pub pane: usize,
    pub previous_terminal: usize,
    pub previous_space: crate::project_state::Space,
    pub previous_tree: Option<crate::panes::Tree>,
    pub previous_zoom: bool,
}
impl ProjectState {
    pub fn restore_plugin_overlay(&mut self) -> bool {
        let Some(overlay) = self.plugin_overlay.take() else {
            return false;
        };
        self.terminal = overlay.previous_terminal;
        self.space = overlay.previous_space;
        self.pane_tree = overlay.previous_tree;
        self.split = self.pane_tree.as_ref().is_some_and(|t| t.ids().len() > 1);
        self.split_panes = self
            .pane_tree
            .as_ref()
            .and_then(|t| t.ids().try_into().ok());
        self.pane_zoom = overlay.previous_zoom;
        self.repair_view();
        true
    }
    pub fn present_plugin_pane(&mut self, pane: usize, placement: Placement) -> Result<()> {
        ensure!(
            self.terminal_ids().contains(&pane),
            "Plugin pane no longer exists"
        );
        ensure!(
            placement != Placement::Popup,
            "Terminal popup presentation is not implemented yet"
        );
        self.restore_plugin_overlay();
        if placement == Placement::Overlay {
            self.plugin_overlay = Some(Overlay {
                pane,
                previous_terminal: self.terminal,
                previous_space: self.space,
                previous_tree: self.unzoomed_tree(),
                previous_zoom: self.pane_zoom,
            });
            self.pane_tree = None;
            self.split = false;
            self.split_panes = None;
            self.pane_zoom = true;
        } else if matches!(placement, Placement::Split | Placement::Zoomed) {
            let mut tree = self
                .unzoomed_tree()
                .context("No focused terminal for plugin split")?;
            if !tree.ids().contains(&pane) {
                tree.split(self.terminal, pane, crate::panes::Axis::Right);
            }
            self.split = tree.ids().len() > 1;
            self.split_panes = tree.ids().try_into().ok();
            self.pane_tree = Some(tree);
            self.pane_zoom = placement == Placement::Zoomed;
        } else {
            self.select_terminal(pane);
        }
        self.terminal = pane;
        self.space = crate::project_state::Space::Terminals;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlay_restores_layout_and_keeps_concurrent_terminal_membership() {
        let mut state = ProjectState::default();
        state.split_pane(crate::panes::Axis::Down).unwrap();
        state.pane_zoom = true;
        let original = state.unzoomed_tree();
        let focus = state.terminal;
        let pane = state.add_background_terminal().unwrap();
        state.present_plugin_pane(pane, Placement::Overlay).unwrap();
        assert_eq!(state.visible(), vec![pane]);
        let concurrent = state.add_background_terminal().unwrap();
        state.close_pane();
        assert_eq!(state.unzoomed_tree(), original);
        assert_eq!(state.terminal, focus);
        assert!(state.pane_zoom);
        assert!(state.terminal_ids().contains(&pane));
        assert!(state.terminal_ids().contains(&concurrent));
        state.validate().unwrap();
    }
    #[test]
    fn plugin_split_zoom_and_tab_reuse_existing_terminal_ids() {
        let mut state = ProjectState::default();
        let pane = state.add_terminal().unwrap();
        state.terminal = 0;
        state.present_plugin_pane(pane, Placement::Split).unwrap();
        assert_eq!(state.visible(), vec![0, pane]);
        state.present_plugin_pane(pane, Placement::Zoomed).unwrap();
        assert_eq!(state.visible(), vec![pane]);
        assert_eq!(state.unzoomed_tree().unwrap().ids(), vec![0, pane]);
        state.present_plugin_pane(0, Placement::Tab).unwrap();
        assert_eq!(state.terminal, 0);
        assert_eq!(state.terminal_ids(), vec![0, pane]);
        assert!(state.present_plugin_pane(pane, Placement::Popup).is_err());
    }
}
