//! Non-destructive multiplexer import and interactive links to existing terminals.
use crate::{
    project_state::{self, ProjectState, Space},
    runtime::Config,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};
/// Identifies imported multiplexer sources in source references, health
/// reports and the automation API.
pub const SOURCE: &str = "agent-mux";
/// The project-file field holding an import binding.
pub const KEY: &str = "agent_mux";
/// Collections created by an import are named `{COLLECTION_PREFIX}-{session}`.
pub const COLLECTION_PREFIX: &str = "starflux";

/// The external agent multiplexer's executable: `STARFLUX_AGENT_MUX`, or the
/// first line of the user's local `$XDG_CONFIG_HOME/starflux/agent-mux` file.
/// Import is unavailable when neither names one.
pub fn executable() -> Option<String> {
    if let Ok(value) = std::env::var("STARFLUX_AGENT_MUX")
        && !value.trim().is_empty()
    {
        return Some(value.trim().to_owned());
    }
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join(".config")))?;
    let text = fs::read_to_string(root.join("starflux").join("agent-mux")).ok()?;
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
}

/// The configured executable's file name, which also names its
/// configuration directory, socket and environment prefix.
pub fn program() -> Option<String> {
    let executable = executable()?;
    std::path::Path::new(&executable)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
}

fn require_executable() -> Result<String> {
    executable().context(
        "No multiplexer is configured for import; set STARFLUX_AGENT_MUX or write its \
         program to ~/.config/starflux/agent-mux",
    )
}

fn require_program() -> Result<String> {
    program().context("No multiplexer is configured for import")
}

/// Generic label for imported multiplexer terminals.
pub fn display_name() -> String {
    "Multiplexer".into()
}

/// Keeps a project's import binding under [`KEY`]. Files written before the
/// key was neutral used the configured program's name, which is still read.
pub mod saved {
    use super::{KEY, Source};
    use serde::{Deserialize, Deserializer, Serializer, ser::SerializeMap};

    pub fn serialize<S: Serializer>(
        source: &Option<Source>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(usize::from(source.is_some())))?;
        if let Some(source) = source {
            map.serialize_entry(KEY, source)?;
        }
        map.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Source>, D::Error> {
        let mut rest =
            std::collections::BTreeMap::<String, serde_json::Value>::deserialize(deserializer)?;
        let legacy = super::program().and_then(|program| rest.remove(&program));
        rest.remove(KEY)
            .filter(|value| !value.is_null())
            .or(legacy.filter(|value| !value.is_null()))
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pane {
    #[serde(default)]
    pub terminal_id: String,
    pub pane_id: String,
    pub tab_id: String,
    pub label: String,
    pub cwd: String,
    pub agent: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub session: String,
    pub workspace_id: String,
    pub order: usize,
    pub executable: String,
    pub panes: BTreeMap<usize, Pane>,
}
impl Source {
    pub fn observed_status(&self, pane: &str, profile: Option<&str>) -> Option<(String, String)> {
        if !matches!(profile, Some("codex" | "claude")) {
            return None;
        }
        crate::activity::screen_observation(self.command(&[
            "pane",
            "read",
            pane,
            "--source",
            "detection",
            "--lines",
            "32",
            "--format",
            "text",
            "--raw",
        ]))
    }
    pub fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(&self.executable);
        c.args(["--session", &self.session])
            .args(args)
            .stdin(Stdio::null());
        c
    }
    fn pane_snapshot(&self, pane: usize) -> Result<Value> {
        let p = self.panes.get(&pane).context("Imported pane is missing")?;
        let out = crate::read_query::output(self.command(&["pane", "get", &p.pane_id]))?;
        let data: Value = serde_json::from_slice(&out)?;
        Ok(data["result"]["pane"].clone())
    }
    pub fn inspect(&self, pane: usize) -> Result<Value> {
        Ok(self.health_from_pane(pane, &self.pane_snapshot(pane)?))
    }
    pub(crate) fn history_identity(&self, pane: usize) -> Result<(Value, String)> {
        let stored = self.panes.get(&pane).context("Imported pane is missing")?;
        let socket = socket_path(&self.session)?;
        let before = crate::session_import::agent_mux_server_stamp(&socket)?;
        let current = self.pane_snapshot(pane)?;
        let terminal = current["terminal_id"]
            .as_str()
            .context("Multiplexer terminal identity unavailable")?;
        ensure!(
            current["pane_id"] == stored.pane_id
                && (stored.terminal_id.is_empty() || terminal == stored.terminal_id),
            "Multiplexer pane or terminal identity changed"
        );
        ensure!(
            before == crate::session_import::agent_mux_server_stamp(&socket)?,
            "Multiplexer source server changed"
        );
        Ok((before, terminal.to_owned()))
    }
    pub(crate) fn inspect_identity(&self, pane: usize) -> Result<Value> {
        let stored = self.panes.get(&pane).context("Imported pane is missing")?;
        let socket = socket_path(&self.session)?;
        let before = crate::session_import::agent_mux_server_stamp(&socket)?;
        let output = crate::read_query::output(self.command(&[
            "pane",
            "process-info",
            "--pane",
            &stored.pane_id,
        ]));
        let mut health = self.inspect(pane)?;
        ensure!(
            health["pane_id"] == stored.pane_id
                && (stored.terminal_id.is_empty() || health["terminal_id"] == stored.terminal_id),
            "Multiplexer pane or terminal identity changed"
        );
        let after = crate::session_import::agent_mux_server_stamp(&socket)?;
        ensure!(before == after, "Multiplexer source server changed");
        if stored.terminal_id.is_empty() {
            health["identity_error"] =
                json!("Reimport the multiplexer session to bind this terminal");
            return Ok(health);
        }
        let info = output.and_then(|bytes| Ok(serde_json::from_slice::<Value>(&bytes)?));
        match info {
            Ok(response) if response["result"]["process_info"]["pane_id"] == stored.pane_id => {
                let info = &response["result"]["process_info"];
                if let Some(pid) = info["shell_pid"]
                    .as_u64()
                    .and_then(|p| u32::try_from(p).ok())
                    && let Some(ticks) = crate::agent_state::process_start(pid)
                {
                    health["pid"] = json!(pid);
                    health["pid_start_ticks"] = json!(ticks);
                    health["foreground_process_group"] =
                        info["foreground_process_group_id"].clone();
                } else {
                    health["identity_error"] =
                        json!("Multiplexer did not report a live shell process");
                }
            }
            _ => {
                health["identity_error"] =
                    json!("Multiplexer pane process information is unavailable");
            }
        }
        Ok(health)
    }
    pub(crate) fn health_from_pane(&self, pane: usize, p: &Value) -> Value {
        let observed = self.observed_status(&self.panes[&pane].pane_id, p["agent"].as_str());
        let state = observed.as_ref().map(|v| v.0.as_str()).unwrap_or_else(|| {
            if matches!(p["agent"].as_str(), Some("codex" | "claude")) {
                "unknown"
            } else {
                p["agent_status"].as_str().unwrap_or("unknown")
            }
        });
        let cwd = p["foreground_cwd"]
            .as_str()
            .filter(|cwd| !cwd.is_empty())
            .or_else(|| p["cwd"].as_str());
        json!({"alive":true,"cwd":cwd,"command":p["agent"].as_str().map_or_else(|| format!("{} shell", display_name()), String::from),"agent":p["agent"],"state":state,"reported_state":p["agent_status"],"source":crate::agent_mux::SOURCE,"read_only":false,"pane_id":p["pane_id"],"terminal_id":p["terminal_id"]})
    }
}
pub fn import(session: &str) -> Result<Config> {
    import_snapshot(session, live_snapshot(session)?)
}
pub(crate) fn socket_path(session: &str) -> Result<PathBuf> {
    ensure!(
        !session.is_empty()
            && session.len() <= 16
            && session
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "Invalid multiplexer session name"
    );
    let program = require_program()?;
    let home = std::env::home_dir().context("No home directory")?;
    let config_path = std::env::var_os(format!("{}_CONFIG_PATH", program.to_ascii_uppercase()))
        .map(PathBuf::from)
        .unwrap_or(home.join(".config").join(&program).join("config.toml"));
    Ok(if session == "default" {
        config_path
            .parent()
            .context("No multiplexer config directory")?
            .join(format!("{program}.sock"))
    } else {
        config_path
            .parent()
            .context("No multiplexer config directory")?
            .join("sessions")
            .join(session)
            .join(format!("{program}.sock"))
    })
}
pub(crate) fn live_snapshot(session: &str) -> Result<Value> {
    use std::os::unix::{
        fs::{FileTypeExt, MetadataExt},
        net::UnixStream,
    };
    let socket = socket_path(session)?;
    let metadata = fs::symlink_metadata(&socket)?;
    ensure!(
        metadata.file_type().is_socket() && metadata.uid() == unsafe { libc::geteuid() },
        "Multiplexer session socket is not owned by this user"
    );
    drop(UnixStream::connect(&socket).context("Multiplexer session is not running")?);
    let executable = require_executable()?;
    let mut command = Command::new(executable);
    command.args(["--session", session, "api", "snapshot"]);
    Ok(serde_json::from_slice(&crate::read_query::output(
        command,
    )?)?)
}
pub(crate) fn import_snapshot(session: &str, snapshot: Value) -> Result<Config> {
    let executable = require_executable()?;
    let socket = socket_path(session)?;
    let saved_path = socket.with_file_name("session.json");
    let saved: Value = match fs::read(&saved_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("Read saved multiplexer layout")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(error).context("Read saved multiplexer layout"),
    };
    let snap = &snapshot["result"]["snapshot"];
    let workspaces = snap["workspaces"]
        .as_array()
        .context("Multiplexer snapshot has no workspaces")?;
    let panes = snap["panes"]
        .as_array()
        .context("Multiplexer snapshot has no panes")?;
    let tabs = snap["tabs"]
        .as_array()
        .context("Multiplexer snapshot has no tabs")?;
    let collection = format!("{COLLECTION_PREFIX}-{session}");
    let root = project_state::data_root()?;
    let mut selected = None;
    let mut count = 0;
    let mut pane_count = 0;
    let mut imported = vec![];
    for (order, w) in workspaces.iter().enumerate() {
        let wid = w["workspace_id"]
            .as_str()
            .context("Multiplexer workspace has no identity")?;
        let workspace = format!("{collection}-{wid}");
        let saved_workspace = saved["workspaces"]
            .as_array()
            .and_then(|ws| ws.iter().find(|w| w["id"] == wid));
        let ws_panes = panes
            .iter()
            .filter(|p| p["workspace_id"] == wid)
            .collect::<Vec<_>>();
        if ws_panes.is_empty() {
            continue;
        }
        ensure!(
            ws_panes.len() <= 30,
            "Multiplexer workspace has more than 30 panes"
        );
        let label = w["label"].as_str().unwrap_or(wid).to_owned();
        let directory = saved_workspace
            .and_then(|w| w["identity_cwd"].as_str())
            .or_else(|| w["identity_cwd"].as_str())
            .or_else(|| ws_panes[0]["cwd"].as_str())
            .unwrap_or("~")
            .to_owned();
        let mut source = Source {
            session: session.into(),
            workspace_id: wid.into(),
            order,
            executable: executable.clone(),
            panes: BTreeMap::new(),
        };
        let mut selected_tab = 0;
        for (index, pane) in ws_panes.iter().enumerate() {
            let pid = pane["pane_id"]
                .as_str()
                .context("Multiplexer pane has no identity")?
                .to_owned();
            let tid = pane["tab_id"]
                .as_str()
                .context("Multiplexer pane has no tab identity")?
                .to_owned();
            let tab = tabs.iter().find(|t| t["tab_id"] == tid);
            let name = tab.and_then(|t| t["label"].as_str()).unwrap_or("Terminal");
            let name = if tab.is_some_and(|t| t["pane_count"].as_u64().unwrap_or(1) > 1) {
                format!("{name} / {pid}")
            } else {
                name.into()
            };
            if w["active_tab_id"] == tid {
                selected_tab = index;
            }
            source.panes.insert(
                index,
                Pane {
                    terminal_id: pane["terminal_id"]
                        .as_str()
                        .context("Multiplexer pane has no terminal identity")?
                        .into(),
                    pane_id: pid,
                    tab_id: tid,
                    label: name,
                    cwd: pane["foreground_cwd"]
                        .as_str()
                        .or_else(|| pane["cwd"].as_str())
                        .unwrap_or(&directory)
                        .into(),
                    agent: pane["agent"].as_str().map(str::to_owned),
                },
            );
        }
        let primary = source
            .panes
            .get(&selected_tab)
            .filter(|p| p.agent.is_some())
            .or_else(|| source.panes.values().find(|p| p.agent.is_some()))
            .unwrap_or(&source.panes[&selected_tab])
            .clone();
        source.panes.insert(31, primary.clone());
        let config = Config {
            workspace: workspace.clone(),
            label: Some(label.clone()),
            collection: Some(collection.clone()),
            agent_mux: Some(source),
            tmux: None,
            adopted: Default::default(),
            native_terminals: false,
            theme: crate::appearance::default_theme(),
            host: None,
            ssh_config: None,
            directory: Some(directory.clone()),
            agent: primary.agent.clone(),
            executable: std::env::current_exe()?.display().to_string(),
            project_host: true,
            desktop_helper: crate::runtime::default_desktop_helper(),
        };
        config.validate()?;
        let path = root.join(format!("{workspace}.json"));
        if path.exists() {
            let old: Config = serde_json::from_slice(&fs::read(&path)?)?;
            ensure!(
                old.agent_mux
                    .as_ref()
                    .is_some_and(|s| s.session == session && s.workspace_id == wid),
                "Import would overwrite an unrelated project"
            );
        }
        let state = ProjectState {
            space: Space::Terminals,
            agent: 31,
            terminal: selected_tab,
            terminals: (0..ws_panes.len()).collect(),
            ..ProjectState::default()
        };
        if !root.join(format!("{workspace}.layout.json")).exists() {
            state.save(&config)?;
        } else {
            starcore::fs::write_private(&path, &serde_json::to_vec_pretty(&config)?)?;
        }
        starcore::fs::write_private(
            &crate::runtime::config_path(&workspace)?,
            &serde_json::to_vec_pretty(&config)?,
        )?;
        if selected.is_none() || snap["focused_workspace_id"] == wid {
            selected = Some(config.clone());
        }
        count += 1;
        pane_count += ws_panes.len();
        imported.push(json!({"workspace":workspace,"source_workspace":wid,"label":label,"directory":directory,"panes":ws_panes.len(),"order":order}));
    }
    let config = selected.context("Multiplexer has no workspaces to import")?;
    let report = json!({"source":display_name(),"session":session,"workspaces":count,"tabs":tabs.len(),"panes":pane_count,"agents":snap["agents"].as_array().map_or(0,Vec::len),"selected":config.workspace,"read_only":false,"live_terminal_attachments":true,"source_processes_restarted":false,"collection":collection,"projects":imported});
    starcore::fs::write_private(
        &root.join(format!("{collection}.import.json")),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    starcore::fs::write_private(
        &root.join(format!("{collection}.snapshot.json")),
        &serde_json::to_vec_pretty(&snapshot)?,
    )?;
    eprintln!(
        "Imported {count} multiplexer workspaces, {} tabs and {pane_count} panes as interactive live terminal references.\nOpen with: starflux --workspace {}",
        tabs.len(),
        config.workspace
    );
    Ok(config)
}
pub fn viewer(session: &str, pane: &str) -> Result<()> {
    ensure!(
        pane.len() < 64
            && pane
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b":_-".contains(&b)),
        "Invalid multiplexer pane identity"
    );
    let executable = require_executable()?;
    starcore::crossterm::terminal::enable_raw_mode()?;
    let _guard = Restore;
    let mut last = Vec::new();
    loop {
        let rows = starcore::crossterm::terminal::size()
            .map(|(_, r)| r.saturating_sub(3).max(5))
            .unwrap_or(40)
            .to_string();
        let out = Command::new(&executable)
            .args([
                "--session",
                session,
                "pane",
                "read",
                pane,
                "--source",
                "visible",
                "--format",
                "ansi",
                "--lines",
                &rows,
            ])
            .stdin(Stdio::null())
            .output()?;
        let bytes = if out.status.success() {
            out.stdout
        } else {
            out.stderr
        };
        if bytes != last {
            let mut stdout = std::io::stdout().lock();
            write!(
                stdout,
                "\x1b[?25l\x1b[?7l\x1b[0m\x1b[H\x1b[2J\x1b[38;2;139;164;220m{} / {session} / {pane} / READ-ONLY LIVE VIEW\x1b[0m\r\n\r\n",
                display_name()
            )?;
            let text = String::from_utf8_lossy(&bytes).replace('\n', "\r\n");
            stdout.write_all(text.as_bytes())?;
            stdout.flush()?;
            last = bytes;
        }
        if starcore::crossterm::event::poll(Duration::from_millis(650))? {
            let _ = starcore::crossterm::event::read()?;
        }
    }
}
struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = starcore::crossterm::terminal::disable_raw_mode();
        print!("\x1b[?7h\x1b[?25h\x1b[0m");
    }
}
pub fn reject_write() -> Result<Value> {
    bail!("Multiplexer owns the imported session lifecycle; use its live attachment for input")
}

#[cfg(test)]
mod saved_tests {
    #[test]
    fn project_files_keep_their_binding_under_the_neutral_key() {
        let source = super::Source {
            session: "default".into(),
            workspace_id: "w1".into(),
            order: 0,
            executable: "mux".into(),
            panes: Default::default(),
        };
        let mut value = serde_json::json!({"workspace": "proof"});
        value[super::KEY] = serde_json::to_value(&source).unwrap();
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Holder {
            workspace: String,
            #[serde(flatten, with = "super::saved")]
            agent_mux: Option<super::Source>,
        }
        let loaded: Holder = serde_json::from_value(value).unwrap();
        assert_eq!(loaded.agent_mux.as_ref().unwrap().workspace_id, "w1");
        let saved = serde_json::to_value(&loaded).unwrap();
        assert_eq!(saved[super::KEY]["session"], "default");
        let empty: Holder =
            serde_json::from_value(serde_json::json!({"workspace": "p", super::KEY: null}))
                .unwrap();
        assert!(empty.agent_mux.is_none());
        assert!(
            serde_json::to_value(&empty)
                .unwrap()
                .get(super::KEY)
                .is_none()
        );
    }
}
