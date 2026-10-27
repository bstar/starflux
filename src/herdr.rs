//! Non-destructive Herdr import and interactive links to existing terminals.
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
    pub fn inspect(&self, pane: usize) -> Result<Value> {
        let p = self.panes.get(&pane).context("Imported pane is missing")?;
        let out = self.command(&["pane", "get", &p.pane_id]).output()?;
        ensure!(
            out.status.success(),
            "Herdr pane unavailable: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let data: Value = serde_json::from_slice(&out.stdout)?;
        let p = &data["result"]["pane"];
        let observed = self.observed_status(&self.panes[&pane].pane_id, p["agent"].as_str());
        let state = observed.as_ref().map(|v| v.0.as_str()).unwrap_or_else(|| {
            if matches!(p["agent"].as_str(), Some("codex" | "claude")) {
                "unknown"
            } else {
                p["agent_status"].as_str().unwrap_or("unknown")
            }
        });
        Ok(
            json!({"alive":true,"command":p["agent"].as_str().unwrap_or("Herdr shell"),"agent":p["agent"],"state":state,"reported_state":p["agent_status"],"source":"herdr","read_only":false,"pane_id":p["pane_id"],"terminal_id":p["terminal_id"]}),
        )
    }
}
fn checked(command: &mut Command) -> Result<Value> {
    let out = command.output()?;
    ensure!(
        out.status.success(),
        "Herdr read failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(serde_json::from_slice(&out.stdout)?)
}
pub fn import(session: &str) -> Result<Config> {
    ensure!(
        !session.is_empty()
            && session.len() <= 16
            && session
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "Invalid Herdr session name"
    );
    let executable = std::env::var("STARMUX_HERDR").unwrap_or_else(|_| "herdr".into());
    let snapshot =
        checked(Command::new(&executable).args(["--session", session, "api", "snapshot"]))?;
    let home = std::env::home_dir().context("No home directory")?;
    let config_path = std::env::var_os("HERDR_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or(home.join(".config/herdr/config.toml"));
    let saved_path = if session == "default" {
        config_path.parent().unwrap().join("session.json")
    } else {
        config_path
            .parent()
            .unwrap()
            .join("sessions")
            .join(session)
            .join("session.json")
    };
    let saved: Value = serde_json::from_slice(
        &fs::read(&saved_path)
            .with_context(|| format!("Cannot read Herdr layout {}", saved_path.display()))?,
    )?;
    let snap = &snapshot["result"]["snapshot"];
    let workspaces = snap["workspaces"]
        .as_array()
        .context("Herdr snapshot has no workspaces")?;
    let panes = snap["panes"]
        .as_array()
        .context("Herdr snapshot has no panes")?;
    let tabs = snap["tabs"]
        .as_array()
        .context("Herdr snapshot has no tabs")?;
    let collection = format!("herdr-{session}");
    let root = project_state::data_root()?;
    let mut selected = None;
    let mut count = 0;
    let mut pane_count = 0;
    let mut imported = vec![];
    for (order, w) in workspaces.iter().enumerate() {
        let wid = w["workspace_id"]
            .as_str()
            .context("Herdr workspace has no identity")?;
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
            "Herdr workspace has more than 30 panes"
        );
        let label = w["label"].as_str().unwrap_or(wid).to_owned();
        let directory = saved_workspace
            .and_then(|w| w["identity_cwd"].as_str())
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
                .context("Herdr pane has no identity")?
                .to_owned();
            let tid = pane["tab_id"]
                .as_str()
                .context("Herdr pane has no tab identity")?
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
                        .context("Herdr pane has no terminal identity")?
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
            herdr: Some(source),
            adopted: Default::default(),
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
                old.herdr
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
            starkit::fs::write_private(&path, &serde_json::to_vec_pretty(&config)?)?;
        }
        starkit::fs::write_private(
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
    let config = selected.context("Herdr has no workspaces to import")?;
    let report = json!({"source":"Herdr","session":session,"workspaces":count,"tabs":tabs.len(),"panes":pane_count,"agents":snap["agents"].as_array().map_or(0,Vec::len),"selected":config.workspace,"read_only":false,"live_terminal_attachments":true,"source_processes_restarted":false,"collection":collection,"projects":imported});
    starkit::fs::write_private(
        &root.join(format!("{collection}.import.json")),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    starkit::fs::write_private(
        &root.join(format!("{collection}.snapshot.json")),
        &serde_json::to_vec_pretty(&snapshot)?,
    )?;
    println!(
        "Imported {count} Herdr workspaces, {} tabs and {pane_count} panes as interactive live terminal references.\nOpen with: starmux --workspace {}",
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
        "Invalid Herdr pane identity"
    );
    let executable = std::env::var("STARMUX_HERDR").unwrap_or("herdr".into());
    starkit::crossterm::terminal::enable_raw_mode()?;
    let _guard = Restore;
    let mut last = Vec::new();
    loop {
        let rows = starkit::crossterm::terminal::size()
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
                "\x1b[?25l\x1b[?7l\x1b[0m\x1b[H\x1b[2J\x1b[38;2;139;164;220mHerdr / {session} / {pane} / READ-ONLY LIVE VIEW\x1b[0m\r\n\r\n"
            )?;
            let text = String::from_utf8_lossy(&bytes).replace('\n', "\r\n");
            stdout.write_all(text.as_bytes())?;
            stdout.flush()?;
            last = bytes;
        }
        if starkit::crossterm::event::poll(Duration::from_millis(650))? {
            let _ = starkit::crossterm::event::read()?;
        }
    }
}
struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = starkit::crossterm::terminal::disable_raw_mode();
        print!("\x1b[?7h\x1b[?25h\x1b[0m");
    }
}
pub fn reject_write() -> Result<Value> {
    bail!("Herdr owns the imported session lifecycle; use its live attachment for input")
}
