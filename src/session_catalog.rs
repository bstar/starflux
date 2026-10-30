//! Saved groups of projects and non-destructive external session discovery.
use crate::{
    project_state::{self, ProjectState},
    runtime::{self, Config},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    process::Command,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub project: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub source: String,
    pub is_default: bool,
    pub project: Option<String>,
    #[serde(skip)]
    pub tmux: Option<crate::tmux_source::Source>,
}
pub fn group(config: &Config) -> String {
    config
        .collection
        .clone()
        .unwrap_or_else(|| "default".into())
}
fn root() -> Result<PathBuf> {
    let path = project_state::data_root()?
        .parent()
        .context("No session directory")?
        .join("sessions");
    starkit::terminal_graphics::session::private_root(&path)?;
    Ok(path)
}
fn valid(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 64
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "Invalid session identity"
    );
    Ok(())
}
pub fn order_path(config: &Config) -> Result<PathBuf> {
    let id = group(config);
    valid(&id)?;
    Ok(root()?.join(format!("{id}.order.json")))
}
pub fn default_id() -> Result<Option<String>> {
    let path = root()?.join("default-session");
    match fs::read(path) {
        Ok(bytes) => {
            let id: Option<String> = serde_json::from_slice(&bytes)?;
            if let Some(id) = &id {
                valid(id)?;
            }
            Ok(id)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
pub fn set_default(id: Option<&str>) -> Result<()> {
    if let Some(id) = id {
        valid(id)?;
        ensure!(
            projects().iter().any(|p| group(p) == id),
            "Session no longer exists"
        );
    }
    starkit::fs::write_private(&root()?.join("default-session"), &serde_json::to_vec(&id)?)?;
    project_state::catalog_changed()
}
pub fn default_project() -> Result<Option<String>> {
    let Some(id) = default_id()? else {
        return Ok(None);
    };
    Ok(list()?
        .into_iter()
        .find(|entry| entry.id == id)
        .and_then(|entry| entry.project))
}
pub fn projects() -> Vec<Config> {
    let mut projects = BTreeMap::new();
    // Durable metadata wins over a retained runtime copy.
    for root in [runtime::root(), project_state::data_root()]
        .into_iter()
        .flatten()
    {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                if entry.path().extension().is_none_or(|s| s != "json") {
                    continue;
                }
                if let Ok(bytes) = fs::read(entry.path())
                    && let Ok(config) = serde_json::from_slice::<Config>(&bytes)
                    && config.validate().is_ok()
                {
                    projects.insert(config.workspace.clone(), config);
                }
            }
        }
    }
    projects.into_values().collect()
}
pub fn save(session: &Session) -> Result<()> {
    valid(&session.id)?;
    project_state::validate_label(&session.name)?;
    starkit::fs::write_private(
        &root()?.join(format!("{}.json", session.id)),
        &serde_json::to_vec_pretty(session)?,
    )?;
    project_state::catalog_changed()
}
pub fn remember(config: &Config) -> Result<()> {
    let id = group(config);
    valid(&id)?;
    let path = root()?.join(format!("{id}.json"));
    let saved = fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Session>(&bytes).ok());
    if saved
        .as_ref()
        .is_some_and(|s| s.project == config.workspace)
    {
        return Ok(());
    }
    let name = saved.map(|s| s.name).unwrap_or_else(|| {
        if id == "default" {
            "Default".into()
        } else {
            id.clone()
        }
    });
    starkit::fs::write_private(
        &path,
        &serde_json::to_vec_pretty(&Session {
            id,
            name,
            project: config.workspace.clone(),
        })?,
    )
}
pub fn list() -> Result<Vec<Entry>> {
    let default = default_id()?;
    let projects = projects();
    let mut entries = BTreeMap::<String, Entry>::new();
    for config in &projects {
        let id = group(config);
        let count = projects.iter().filter(|c| group(c) == id).count();
        entries.entry(id.clone()).or_insert_with(|| Entry {
            id: id.clone(),
            name: if id == "default" {
                "Default".into()
            } else {
                id.clone()
            },
            detail: format!("{count} project{}", if count == 1 { "" } else { "s" }),
            source: "starflux".into(),
            is_default: default.as_deref() == Some(id.as_str()),
            project: Some(config.workspace.clone()),
            tmux: None,
        });
    }
    for entry in fs::read_dir(root()?)? {
        let entry = entry?;
        if let Ok(bytes) = fs::read(entry.path())
            && let Ok(session) = serde_json::from_slice::<Session>(&bytes)
            && let Some(entry) = entries.get_mut(&session.id)
        {
            entry.name = session.name;
            if projects
                .iter()
                .any(|p| p.workspace == session.project && group(p) == session.id)
            {
                entry.project = Some(session.project);
            }
        }
    }
    let mut entries = entries.into_values().collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    Ok(entries)
}
pub fn create(name: &str, directory: &str, theme: &str) -> Result<Config> {
    project_state::validate_label(name)?;
    let directory = fs::canonicalize(directory).context("Choose an existing project directory")?;
    ensure!(directory.is_dir(), "Choose a directory");
    let id = format!(
        "session-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..16]
    );
    let config = Config {
        workspace: format!("work-{}", &uuid::Uuid::new_v4().simple().to_string()[..16]),
        label: Some(name.into()),
        collection: Some(id.clone()),
        herdr: None,
        tmux: None,
        adopted: Default::default(),
        theme: theme.into(),
        host: None,
        ssh_config: None,
        directory: Some(directory.to_string_lossy().into_owned()),
        agent: None,
        executable: std::env::current_exe()?.to_string_lossy().into_owned(),
        project_host: true,
        desktop_helper: runtime::default_desktop_helper(),
    };
    config.validate()?;
    ProjectState::default().save(&config)?;
    starkit::fs::write_private(
        &runtime::config_path(&config.workspace)?,
        &serde_json::to_vec_pretty(&config)?,
    )?;
    save(&Session {
        id,
        name: name.into(),
        project: config.workspace.clone(),
    })?;
    Ok(config)
}
pub fn external() -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut sockets = BTreeSet::new();
    let temp = std::env::var_os("TMUX_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    if let Ok(files) = fs::read_dir(temp.join(format!("tmux-{}", unsafe { libc::getuid() }))) {
        for file in files.flatten() {
            if !file.file_name().to_string_lossy().starts_with("starflux-")
                && !file.file_name().to_string_lossy().starts_with("starmux-")
            {
                sockets.insert(file.path());
            }
        }
    }
    if let Some(socket) = std::env::var("TMUX")
        .ok()
        .and_then(|v| v.split(',').next().map(PathBuf::from))
    {
        sockets.insert(socket);
    }
    for socket in sockets {
        let Ok(out) = Command::new("tmux")
            .args([
                "-u",
                "-S",
                &socket.to_string_lossy(),
                "list-sessions",
                "-F",
                "#{session_id}\t#{session_name}\t#{session_windows}",
            ])
            .output()
        else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let fields = line.split('\t').collect::<Vec<_>>();
            if fields.len() != 3 {
                continue;
            }
            let source = crate::tmux_source::Source {
                socket: socket.clone(),
                session: fields[0].into(),
            };
            if source.validate().is_err() {
                continue;
            }
            entries.push(Entry {
                id: format!("tmux-{}", entries.len()),
                name: fields[1].into(),
                detail: format!("{} windows · {}", fields[2], socket.display()),
                source: "tmux".into(),
                is_default: false,
                project: None,
                tmux: Some(source),
            });
        }
    }
    let herdr = std::env::var("STARFLUX_HERDR").unwrap_or_else(|_| "herdr".into());
    if let Ok(out) = Command::new(herdr)
        .args(["session", "list", "--json"])
        .output()
        && out.status.success()
        && let Ok(data) = serde_json::from_slice::<serde_json::Value>(&out.stdout)
        && let Some(sessions) = data["sessions"].as_array()
    {
        for session in sessions.iter().filter(|s| s["running"] == true) {
            if let Some(name) = session["name"].as_str() {
                entries.push(Entry {
                    id: format!("herdr-{name}"),
                    name: name.into(),
                    detail: "Live Herdr projects and terminals".into(),
                    source: "herdr".into(),
                    is_default: false,
                    project: None,
                    tmux: None,
                });
            }
        }
    }
    entries
}
pub fn import(entry: &Entry) -> Result<Config> {
    if entry.source == "herdr" {
        let config = crate::herdr::import(&entry.name)?;
        save(&Session {
            id: group(&config),
            name: format!("Herdr / {}", entry.name),
            project: config.workspace.clone(),
        })?;
        return Ok(config);
    }
    let source = entry
        .tmux
        .clone()
        .context("Select a live tmux or Herdr session")?;
    source.inspect()?;
    if let Some(config) = projects().into_iter().find(|p| {
        p.tmux
            .as_ref()
            .is_some_and(|s| s.socket == source.socket && s.session == source.session)
    }) {
        return Ok(config);
    }
    let output = source
        .command(&[
            "display-message",
            "-p",
            "-t",
            &source.session,
            "#{pane_current_path}",
        ])
        .output()?;
    let directory = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned();
    let mut config = create(
        &format!("tmux / {}", entry.name),
        &directory,
        &crate::appearance::default_theme(),
    )?;
    config.tmux = Some(source);
    ProjectState::default().save(&config)?;
    starkit::fs::write_private(
        &runtime::config_path(&config.workspace)?,
        &serde_json::to_vec_pretty(&config)?,
    )?;
    Ok(config)
}
pub fn delete(id: &str, presenting: &str, fallback: &Config) -> Result<()> {
    valid(id)?;
    let configs = projects()
        .into_iter()
        .filter(|c| group(c) == id)
        .collect::<Vec<_>>();
    ensure!(!configs.is_empty(), "Session no longer exists");
    ensure!(
        group(fallback) != id,
        "Open another session before deleting this one"
    );
    let identities = configs
        .iter()
        .map(|c| c.workspace.clone())
        .collect::<BTreeSet<_>>();
    let mut closing = Vec::new();
    for entry in fs::read_dir(runtime::root()?)? {
        let path = entry?.path();
        if path.file_name().is_some_and(|n| {
            n.to_string_lossy().starts_with("v-")
                && n.to_string_lossy().ends_with(".state.json")
                && n.to_string_lossy() != format!("{presenting}.state.json")
        }) && let Ok(bytes) = fs::read(&path)
            && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && value["active_project"]
                .as_str()
                .is_some_and(|p| identities.contains(p))
        {
            let name = path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .replace(".state.json", ".close");
            fs::write(path.with_file_name(name), b"close")?;
            closing.push(
                path.with_file_name(
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .replace(".state.json", ".sock"),
                ),
            );
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    for socket in closing {
        while crate::socket_active(&socket)? {
            ensure!(
                std::time::Instant::now() < deadline,
                "A session view did not close; saved projects were left intact"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    // An attached frontend can have been launched through a project that is
    // now being deleted. Retain only a runtime routing alias for that view;
    // it points to a surviving project and is never a saved deleted session.
    let runtime_root = runtime::root()?;
    let mut anchors = BTreeSet::new();
    for entry in fs::read_dir(&runtime_root)? {
        let path = entry?.path();
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with(".frontend.json"))
            && let Ok(bytes) = fs::read(path)
            && let Ok(view) = serde_json::from_slice::<crate::frontend::View>(&bytes)
            && crate::socket_active(&runtime_root.join(format!("{}.sock", view.id)))?
        {
            anchors.insert(view.workspace);
        }
    }
    for config in configs {
        let _transaction = crate::project_sync::transaction(&config)?;
        let state = ProjectState::load(&config)?;
        for pane in state.terminal_ids() {
            config.call(pane, &config.request(pane, "stop"))?;
        }
        for target in state.app_targets {
            let mut app = config.clone();
            app.host = target.host;
            app.desktop_helper = target.helper;
            crate::app_desktop::stop(&app)?;
        }
        for root in [runtime::root()?, project_state::data_root()?] {
            for suffix in [".json", ".layout.json", ".model.json"] {
                let path = root.join(format!("{}{suffix}", config.workspace));
                match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
    }
    for entry in fs::read_dir(&runtime_root)? {
        let path = entry?.path();
        if let Ok(bytes) = fs::read(&path)
            && let Ok(config) = serde_json::from_slice::<Config>(&bytes)
            && identities.contains(config.workspace.as_str())
        {
            if path
                .file_stem()
                .is_some_and(|name| anchors.contains(name.to_string_lossy().as_ref()))
            {
                starkit::fs::write_private(&path, &serde_json::to_vec_pretty(fallback)?)?;
            } else {
                fs::remove_file(path)?;
            }
        }
    }
    for anchor in anchors.intersection(&identities) {
        starkit::fs::write_private(
            &runtime::config_path(anchor)?,
            &serde_json::to_vec_pretty(fallback)?,
        )?;
    }
    let path = root()?.join(format!("{id}.json"));
    if path.exists() {
        fs::remove_file(path)?;
    }
    if default_id()?.as_deref() == Some(id) {
        set_default(None)?;
    }
    project_state::catalog_changed()
}
