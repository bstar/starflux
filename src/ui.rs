use crate::{
    app_desktop::{Client as AppClient, Request as AppRequest},
    compositor::{Desktop, DesktopCommand, DesktopEvent, Input as DesktopInput, WindowInfo},
    project::{Launch, Settings, WindowLayout},
    project_state::{self, AppTarget, ProjectState, Space},
    runtime::{Config, root},
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use crossbeam_channel::{Receiver, Sender, bounded};
use serde_json::{Value, json};
use starkit::{
    content_view::ContentView,
    input::TextInput,
    native_surface::{HitRegion, PixelRect},
    terminal_graphics::{
        embedded_input::{EmbeddedKind, KeyPhase},
        live_canvas::{Frame, LiveFrames, PixelFormat, Update},
        protocol::{Component, Input, Rect, Scene, ServerMessage, Viewport},
        session::Controller,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Debug)]
enum Event {
    Ready(Box<Config>, usize),
    Git(String, String, Box<crate::git::Status>),
    Activity(BTreeMap<String, crate::activity::Activity>),
    Selected(u64, String),
    Health(String, usize, Value),
    Status(String),
    Delivered(String, usize, String),
    AppDelivered(String, u64, String),
    Error(String),
    TransferFailed(String),
}
struct AppHost {
    config: Config,
    client: AppClient,
    windows: Vec<WindowInfo>,
    selected: Option<u64>,
}
#[derive(Clone)]
enum Prompt {
    Launch,
    Project,
    RunCommand,
    RenameProject(String),
    RenameTab(usize),
    RenameAgent(usize),
}
struct Modal {
    kind: Prompt,
    fields: Vec<(String, TextInput)>,
    selected: usize,
    error: String,
}
pub struct Workspace {
    controller_id: String,
    config: Config,
    state: ProjectState,
    projects: Vec<Config>,
    project_offset: usize,
    project_page_size: usize,
    states: BTreeMap<String, ProjectState>,
    leases: BTreeMap<String, fs::File>,
    appearance: crate::appearance::Appearance,
    typography: Option<crate::terminal::FontMetrics>,
    chrome_hits: Vec<HitRegion>,
    desktop: Option<Desktop>,
    terminals: BTreeMap<(String, usize), crate::terminal::Terminal>,
    frames: LiveFrames,
    rx: Receiver<Event>,
    tx: Sender<Event>,
    health: BTreeMap<(String, usize), Value>,
    registered: BTreeSet<(String, usize)>,
    launched: BTreeMap<(String, usize), Instant>,
    windows: Vec<WindowInfo>,
    app_hosts: BTreeMap<String, AppHost>,
    active_app: Option<String>,
    view: Option<ContentView>,
    viewport: Viewport,
    settings: Settings,
    terminal_fps: u32,
    status: String,
    history: Vec<String>,
    effects: Vec<ServerMessage>,
    quit: bool,
    last_inspect: Instant,
    started: Instant,
    receiving: Option<(usize, Vec<u8>)>,
    drop_enabled: bool,
    hover: Option<usize>,
    offered: Vec<String>,
    effects_id: u64,
    transfer: Option<Arc<AtomicBool>>,
    last_frame: Option<(String, Arc<Vec<u8>>)>,
    drop_target: Option<(Config, Option<(String, u64)>)>,
    modal: Option<Modal>,
    context_menu: Option<crate::shelf::ContextMenu>,
    drag_split: bool,
    text_gesture: crate::selection::Gesture,
    selection_target: Option<(String, usize)>,
    selection_serial: u64,
    activity: BTreeMap<String, crate::activity::Activity>,
    activity_pending: bool,
    activity_at: Instant,
    wave_at: Instant,
    wave_phase: u16,
    last_typing: Instant,
    reduced_motion: bool,
    git: BTreeMap<String, (String, crate::git::Status, Instant)>,
    git_pending: BTreeSet<String>,
    git_open: bool,
    git_scroll: usize,
}
const URI_PATH: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'#')
    .add(b'?')
    .add(b'%');
fn lease(name: &str) -> Result<fs::File> {
    use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};
    let path = root()?.join(format!("{name}.project.lock"));
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    ensure!(
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "Project is already controlled by another workspace; detach it there before opening it here"
    );
    Ok(file)
}
fn app_key(config: &Config) -> String {
    format!(
        "{}@{}",
        config.workspace,
        config.host.as_deref().unwrap_or("local")
    )
}
fn app_id(project: &str, pane: usize) -> String {
    format!("starmux-{project}-p{pane}")
}
impl Workspace {
    pub fn new(config: Config) -> Result<Self> {
        let controller_id = config.workspace.clone();
        let selected =
            fs::read(project_state::data_root()?.join(format!("{controller_id}.workspace.json")))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<String>(&bytes).ok());
        let config = selected
            .filter(|id| id != &controller_id)
            .and_then(|id| {
                project_state::catalog(&config)
                    .into_iter()
                    .find(|c| c.workspace == id)
            })
            .unwrap_or(config);
        let appearance = crate::appearance::Appearance::load(&config.theme)?;
        let state = ProjectState::load(&config)?;
        state.save(&config)?;
        let projects = project_state::catalog(&config);
        let (controller_tx, rx) = bounded(128);
        let terminal_fps = if std::env::var("STARMUX_TERMINAL_PACING").as_deref() == Ok("legacy") {
            30
        } else {
            60
        };
        let settings = Settings {
            width: 1200,
            height: 650,
            fps: terminal_fps,
        };
        let desktop = (!crate::terminal::direct()).then(|| Desktop::start(settings.clone()));
        let mut leases = BTreeMap::new();
        leases.insert(controller_id.clone(), lease(&controller_id)?);
        if config.workspace != controller_id {
            leases.insert(config.workspace.clone(), lease(&config.workspace)?);
        }
        let mut s = Self {
            controller_id,
            config,
            state,
            projects,
            project_offset: 0,
            project_page_size: 0,
            states: BTreeMap::new(),
            leases,
            appearance,
            typography: None,
            chrome_hits: vec![],
            desktop,
            terminals: BTreeMap::new(),
            frames: LiveFrames::default(),
            rx,
            tx: controller_tx,
            health: BTreeMap::new(),
            registered: BTreeSet::new(),
            launched: BTreeMap::new(),
            windows: vec![],
            app_hosts: BTreeMap::new(),
            active_app: None,
            view: None,
            viewport: Viewport::default(),
            settings,
            terminal_fps,
            status: "Connecting project sessions…".into(),
            history: vec![],
            effects: vec![],
            quit: false,
            last_inspect: Instant::now(),
            started: Instant::now(),
            receiving: None,
            drop_enabled: false,
            hover: None,
            offered: vec![],
            effects_id: 0,
            transfer: None,
            last_frame: None,
            drop_target: None,
            modal: None,
            context_menu: None,
            drag_split: false,
            text_gesture: Default::default(),
            selection_target: None,
            selection_serial: 0,
            activity: BTreeMap::new(),
            activity_pending: false,
            activity_at: Instant::now() - Duration::from_secs(3),
            wave_at: Instant::now(),
            wave_phase: 0,
            last_typing: Instant::now(),
            reduced_motion: std::env::var("STARMUX_REDUCED_MOTION").as_deref() == Ok("1"),
            git: BTreeMap::new(),
            git_pending: BTreeSet::new(),
            git_open: false,
            git_scroll: 0,
        };
        s.ensure_sessions();
        s.reconnect_saved_apps();
        Ok(s)
    }
    fn note(&mut self, text: String) {
        self.status = text.clone();
        self.history.push(text);
        if self.history.len() > 8 {
            self.history.remove(0);
        }
    }
    fn save(&mut self) {
        if let Ok(root) = project_state::data_root() {
            let _ = starkit::fs::write_private(
                &root.join(format!("{}.workspace.json", self.controller_id)),
                &serde_json::to_vec(&self.config.workspace).unwrap(),
            );
        }
        if let Err(e) = self.state.save(&self.config) {
            self.note(format!("Could not save project: {e:#}"));
        }
    }
    fn desktop_send(&self, command: DesktopCommand) {
        if let Some(desktop) = &self.desktop {
            desktop.send(command);
        }
    }
    fn refresh_kitty_fonts(&self) {
        if self.desktop.is_none() {
            return;
        }
        let inherited = self.typography.map(|t| f64::from(t.millipixels) / 1000.);
        let requests = self
            .registered
            .iter()
            .map(|(project, pane)| {
                let size = if project == &self.config.workspace {
                    self.state.terminal_font_size
                } else {
                    self.states
                        .get(project)
                        .map(|s| s.terminal_font_size)
                        .unwrap_or(0)
                };
                (
                    project.clone(),
                    *pane,
                    if size == 0 {
                        inherited
                    } else {
                        Some(f64::from(size))
                    },
                )
            })
            .collect::<Vec<_>>();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            for (project, pane, pixels) in requests {
                let Some(pixels) = pixels else {
                    continue;
                };
                let result = (|| -> Result<()> {
                    let socket = root()?.join(format!("kitty-{project}-{pane}.sock"));
                    if !socket.exists() {
                        return Ok(());
                    }
                    let result = std::process::Command::new(
                        std::env::var("STARMUX_KITTEN")
                            .unwrap_or_else(|_| "/usr/bin/kitten".into()),
                    )
                    .args([
                        "@",
                        "--to",
                        &format!("unix:{}", socket.display()),
                        "set-font-size",
                        &(pixels * 0.75).to_string(),
                    ])
                    .output()?;
                    ensure!(
                        result.status.success(),
                        "{}",
                        String::from_utf8_lossy(&result.stderr)
                    );
                    Ok(())
                })();
                if let Err(error) = result {
                    let _ = tx.send(Event::Error(format!("Terminal font: {error:#}")));
                }
            }
        });
    }
    fn font_pixels(&self) -> f64 {
        if self.state.terminal_font_size == 0 {
            self.typography
                .map(|m| f64::from(m.millipixels) / 1000.)
                .unwrap_or(16.)
        } else {
            f64::from(self.state.terminal_font_size)
        }
    }
    fn font_label(&self) -> String {
        let size = self.font_pixels();
        if (size - size.round()).abs() < 0.01 {
            format!("{size:.0}px")
        } else {
            format!("{size:.1}px")
        }
    }
    fn release(&mut self) {
        self.text_gesture.cancel();
        self.selection_target = None;
        for terminal in self.terminals.values_mut() {
            terminal.release();
        }
        self.desktop_send(DesktopCommand::Input(DesktopInput::Release));
        for host in self.app_hosts.values() {
            let _ = host.client.send(AppRequest::Input {
                input: DesktopInput::Release,
            });
        }
        self.drag_split = false;
    }
    fn refresh_activity(&mut self) {
        if !self.activity_pending && self.activity_at.elapsed() >= Duration::from_secs(2) {
            self.activity_pending = true;
            let projects = self.projects.clone();
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Event::Activity(crate::activity::inspect(&projects)));
            });
        }
        if !self.reduced_motion
            && self.last_typing.elapsed() >= Duration::from_millis(150)
            && self.wave_at.elapsed() >= Duration::from_millis(110)
            && self
                .projects
                .iter()
                .any(|p| self.project_activity(p).working > 0)
        {
            self.wave_phase = (self.wave_phase + 1) % 48;
            self.wave_at = Instant::now();
        }
    }
    fn project_activity(&self, project: &Config) -> crate::activity::Activity {
        let mut result = if self.activity_at.elapsed() < Duration::from_secs(8) {
            self.activity
                .get(&project.workspace)
                .cloned()
                .unwrap_or_default()
        } else {
            Default::default()
        };
        // Current local/native health is newer than the background batch.
        let state = if project.workspace == self.config.workspace {
            Some(&self.state)
        } else {
            self.states.get(&project.workspace)
        };
        if let Some(state) = state {
            let mut native = crate::activity::Activity::default();
            let mut any = false;
            for pane in state.agent_ids().into_iter().filter(|id| {
                !project
                    .herdr
                    .as_ref()
                    .is_some_and(|h| h.panes.contains_key(id))
            }) {
                if let Some(health) = self.health.get(&(project.workspace.clone(), pane)) {
                    any = true;
                    if health["alive"] == true {
                        native.observe(health["state"].as_str().unwrap_or("unknown"));
                    }
                }
            }
            if any && project.herdr.is_none() {
                result = native;
            }
        }
        result
    }
    fn agent_command(&self, pane: usize) -> String {
        let command = self.config.agent.as_deref().unwrap_or("codex");
        if self.state.agent_ids().first() == Some(&pane) {
            return command.into();
        }
        crate::runtime::fresh_agent_command(command)
    }
    fn agent_label(&self, pane: usize) -> String {
        self.state
            .agent_labels
            .get(&pane)
            .cloned()
            .unwrap_or_else(|| {
                let n = self
                    .state
                    .agent_ids()
                    .iter()
                    .position(|i| *i == pane)
                    .unwrap_or(0);
                format!("Agent {}", n + 1)
            })
    }
    fn ensure_sessions(&mut self) {
        let agents = self.state.agent_ids();
        let panes: Vec<usize> = if self.config.herdr.is_some() {
            self.state
                .visible()
                .into_iter()
                .chain(agents.iter().copied())
                .map(|pane| self.config.canonical_pane(pane))
                .collect()
        } else {
            agents
                .iter()
                .copied()
                .chain(self.state.terminals.clone())
                .collect()
        };
        for pane in panes {
            let key = (self.config.workspace.clone(), pane);
            if !self.registered.insert(key) {
                continue;
            }
            let command = agents
                .iter()
                .find(|id| self.config.canonical_pane(**id) == pane)
                .map(|id| self.agent_command(*id));
            let config = self.config.clone();
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                if let Err(e) = config.ensure(pane, command.as_deref()) {
                    let _ = tx.send(Event::Error(format!(
                        "{} / session {}: {e:#}",
                        config.workspace,
                        pane + 1
                    )));
                    return;
                }
                let _ = tx.send(Event::Ready(Box::new(config.clone()), pane));
                loop {
                    let value = config
                        .call(pane, &config.request(pane, "inspect"))
                        .unwrap_or_else(
                            |e| json!({"state":"disconnected","error":format!("{e:#}")}),
                        );
                    if tx
                        .send(Event::Health(config.workspace.clone(), pane, value))
                        .is_err()
                    {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
            });
        }
    }
    fn reconnect_saved_apps(&mut self) {
        for target in self.state.app_targets.clone() {
            let mut config = self.config.clone();
            config.host = target.host;
            config.directory = Some(target.directory);
            config.desktop_helper = target.helper;
            let _ = self.connect_app(config);
        }
    }
    fn connect_app(&mut self, mut config: Config) -> Result<String> {
        config.herdr = None;
        let key = app_key(&config);
        if !self.app_hosts.contains_key(&key) {
            let client = AppClient::connect(&config)?;
            client.send(AppRequest::Resize {
                settings: Settings {
                    width: self.settings.width.clamp(128, 2560),
                    height: self.settings.height.clamp(64, 1440),
                    fps: 30,
                },
            })?;
            self.app_hosts.insert(
                key.clone(),
                AppHost {
                    config,
                    client,
                    windows: vec![],
                    selected: None,
                },
            );
        }
        self.active_app = Some(key.clone());
        Ok(key)
    }
    fn launch(&mut self, config: Config, pane: usize) {
        if self.desktop.is_none() {
            let key = (config.workspace.clone(), pane);
            if self.terminals.contains_key(&key) {
                return;
            }
            let result = crate::terminal::socket(&config.workspace, pane).and_then(|socket| {
                crate::terminal::Terminal::spawn(&config.attach_argv(pane), socket)
            });
            match result {
                Ok(mut terminal) => {
                    terminal.set_palette(&self.appearance);
                    self.terminals.insert(key, terminal);
                }
                Err(error) => {
                    self.note(format!("Direct terminal: {error:#}"));
                }
            }
            return;
        }

        let mut argv = vec![
            std::env::var("STARMUX_KITTY").unwrap_or_else(|_| "/usr/bin/kitty".into()),
            "--class".into(),
            app_id(&config.workspace, pane),
            "--title".into(),
            format!("{} / session {}", config.workspace, pane + 1),
        ];
        let socket = root()
            .unwrap()
            .join(format!("kitty-{}-{pane}.sock", config.workspace));
        let _ = fs::remove_file(&socket);
        argv.extend(["--listen-on".into(), format!("unix:{}", socket.display())]);
        for setting in [
            "hide_window_decorations=yes",
            "linux_display_server=wayland",
            "remember_window_size=no",
            "initial_window_width=900",
            "initial_window_height=650",
            "allow_remote_control=socket-only",
            "window_padding_width=12",
            "confirm_os_window_close=0",
            "enable_audio_bell=no",
            // Selection gestures synthesize Shift to bypass an app's mouse
            // capture. Start a new selection in either Kitty mouse mode.
            "mouse_map=shift+left press ungrabbed mouse_selection normal",
        ] {
            argv.extend(["-o".into(), setting.into()]);
        }
        if self.terminal_fps >= 60 {
            // Synchronize once at the presenting terminal, not at both outputs.
            for setting in ["sync_to_monitor=no", "repaint_delay=4", "input_delay=1"] {
                argv.extend(["-o".into(), setting.into()]);
            }
        }
        let font_size = ProjectState::load(&config)
            .map(|s| s.terminal_font_size)
            .unwrap_or(16);
        let pixels = if font_size != 0 {
            Some(f64::from(font_size))
        } else {
            self.typography.map(|t| f64::from(t.millipixels) / 1000.)
        };
        if let Some(pixels) = pixels {
            // The private Wayland output is scale 1 / 96 DPI. Keep the same
            // physical font size as the presenting Kitty, including its zoom.
            argv.extend(["-o".into(), format!("font_size={}", pixels * 0.75)]);
        }
        for setting in self.appearance.kitty_colors() {
            argv.extend(["-o".into(), setting]);
        }
        argv.extend(config.attach_argv(pane));
        self.desktop_send(DesktopCommand::Launch(Launch {
            name: format!("{}-{pane}", config.workspace),
            command: shell_words::join(argv),
            directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
        }));
        self.launched
            .insert((config.workspace, pane), Instant::now());
        self.layout();
    }
    fn visible_panes(&self) -> Vec<(usize, PixelRect)> {
        let ids = self
            .state
            .visible()
            .into_iter()
            .map(|id| self.config.canonical_pane(id))
            .collect::<Vec<_>>();
        if ids.len() == 2 && self.settings.width > 24 {
            let gap = 12u16;
            let left =
                ((f64::from(self.settings.width) - f64::from(gap)) * self.state.split_ratio) as u16;
            vec![
                (
                    ids[0],
                    PixelRect::new(0, 0, left, self.settings.height as u16),
                ),
                (
                    ids[1],
                    PixelRect::new(
                        left + gap,
                        0,
                        self.settings.width as u16 - left - gap,
                        self.settings.height as u16,
                    ),
                ),
            ]
        } else {
            ids.into_iter()
                .take(1)
                .map(|id| {
                    (
                        id,
                        PixelRect::new(
                            0,
                            0,
                            self.settings.width as u16,
                            self.settings.height as u16,
                        ),
                    )
                })
                .collect()
        }
    }
    fn layout(&mut self) {
        let panes = self.visible_panes();
        self.desktop_send(DesktopCommand::Restore(
            panes
                .iter()
                .map(|(i, r)| WindowLayout {
                    app_id: app_id(&self.config.workspace, *i),
                    x: i32::from(r.x),
                    y: i32::from(r.y),
                    width: i32::from(r.width),
                    height: i32::from(r.height),
                })
                .collect(),
        ));
        self.desktop_send(DesktopCommand::Visible(
            panes
                .iter()
                .map(|(i, _)| app_id(&self.config.workspace, *i))
                .collect(),
        ));
        if self.state.space != Space::Apps {
            self.desktop_send(DesktopCommand::FocusApp(app_id(
                &self.config.workspace,
                self.config.canonical_pane(self.state.selected()),
            )));
        }
    }
    fn set_space(&mut self, space: Space) {
        self.release();
        self.state.space = space;
        if self.config.herdr.is_some() {
            self.ensure_sessions();
        }
        self.hover = None;
        self.layout();
        self.save();
    }
    fn switch_project(&mut self, id: &str) -> Result<()> {
        if id == self.config.workspace {
            return Ok(());
        }
        let config = self
            .projects
            .iter()
            .find(|c| c.workspace == id)
            .context("Project not found")?
            .clone();
        if !self.leases.contains_key(id) {
            let lease = lease(id)?;
            let socket = starkit::terminal_graphics::session::socket_path(&root()?, id)?;
            ensure!(
                std::os::unix::net::UnixStream::connect(socket).is_err(),
                "Project has its own running controller; close that workspace before opening it here"
            );
            self.leases.insert(id.into(), lease);
        }
        let state = self
            .states
            .remove(id)
            .unwrap_or(ProjectState::load(&config)?);
        self.release();
        self.save();
        self.states
            .insert(self.config.workspace.clone(), self.state.clone());
        self.active_app = None;
        self.config = config;
        self.state = state;
        if let Some(index) = self
            .projects
            .iter()
            .position(|p| p.workspace == self.config.workspace)
        {
            let size = self.project_page_size.max(1);
            if index < self.project_offset || index >= self.project_offset + size {
                self.project_offset = if index < self.project_offset {
                    index
                } else {
                    index + 1 - size
                };
            }
        }
        self.appearance = crate::appearance::Appearance::load(&self.config.theme)?;
        self.ensure_sessions();
        self.reconnect_saved_apps();
        self.layout();
        self.note(format!(
            "Project {} / sessions retained",
            self.config.workspace
        ));
        Ok(())
    }
    fn apply_theme(&mut self, theme: crate::appearance::Appearance) {
        self.appearance = theme;
        for ((project, _), terminal) in &mut self.terminals {
            if project == &self.config.workspace {
                terminal.set_palette(&self.appearance);
            }
        }
        self.config.theme = self.appearance.id.clone();
        let config = self.config.clone();
        let colors = self.appearance.kitty_colors();
        std::thread::spawn(move || {
            for pane in 0..32 {
                if let Ok(root) = root() {
                    let socket = root.join(format!("kitty-{}-{pane}.sock", config.workspace));
                    if !socket.exists() {
                        continue;
                    }
                    let _ = std::process::Command::new(
                        std::env::var("STARMUX_KITTEN")
                            .unwrap_or_else(|_| "/usr/bin/kitten".into()),
                    )
                    .args([
                        "@",
                        "--to",
                        &format!("unix:{}", socket.display()),
                        "set-colors",
                        "--configured",
                    ])
                    .args(&colors)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                }
            }
        });
        self.save();
        let _ = starkit::fs::write_private(
            &crate::runtime::config_path(&self.config.workspace).unwrap(),
            &serde_json::to_vec_pretty(&self.config).unwrap(),
        );
    }
    fn cycle_theme(&mut self) -> Result<()> {
        let theme = self.appearance.cycle()?;
        self.apply_theme(theme);
        Ok(())
    }
    fn osc(&mut self, meta: &str, payload: Option<String>) {
        self.effects_id += 1;
        self.effects.push(ServerMessage::Osc72 {
            id: self.effects_id,
            meta: meta.into(),
            payload,
        });
    }
    fn drop_pane(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || self.modal.is_some() || self.context_menu.is_some() || self.git_open {
            return None;
        }
        let v = self.viewport;
        let point = [
            f64::from(x) * f64::from(v.width) / f64::from(v.columns.max(1)),
            f64::from(y) * f64::from(v.height) / f64::from(v.rows.max(1)),
        ];
        let local = self.view?.to_content(point, false)?;
        if self.state.space == Space::Apps {
            return Some(32);
        }
        self.visible_panes()
            .iter()
            .find(|(_, r)| {
                local[0] >= f64::from(r.x)
                    && local[0] < f64::from(r.x + r.width)
                    && local[1] >= f64::from(r.y)
                    && local[1] < f64::from(r.y + r.height)
            })
            .map(|(id, _)| *id)
    }
    fn drop_message(&mut self, text: &str) -> Result<()> {
        ensure!(text.len() <= 1024 * 1024, "Drop message too large");
        let (header, payload) = text.split_once(';').unwrap_or((text, ""));
        let fields = header
            .split(':')
            .filter_map(|s| s.split_once('='))
            .collect::<std::collections::HashMap<_, _>>();
        let n = |k: &str| {
            fields
                .get(k)
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(-1)
        };
        match fields.get("t").copied() {
            Some("q") => {
                self.drop_enabled = true;
                self.osc("t=a:i=1", Some("text/uri-list".into()));
                self.note("Desktop file drops ready · copy to the target session".into());
            }
            Some("m") => {
                if !payload.is_empty() {
                    self.offered = payload.split_whitespace().map(str::to_owned).collect();
                }
                let pane = self.drop_pane(n("x"), n("y"));
                let accepts = self.offered.iter().any(|m| m == "text/uri-list") && n("o") & 1 != 0;
                let hover = if accepts { pane } else { None };
                if self.hover != hover {
                    self.hover = hover;
                    self.osc(
                        if hover.is_some() {
                            "t=m:o=1:i=1"
                        } else {
                            "t=m:o=0:i=1"
                        },
                        hover.map(|_| "text/uri-list".into()),
                    );
                }
            }
            Some("M") => {
                if let Some(pane) = self.drop_pane(n("x"), n("y")) {
                    if !payload.is_empty() {
                        self.offered = payload.split_whitespace().map(str::to_owned).collect();
                    }
                    if let Some(index) = self.offered.iter().position(|s| s == "text/uri-list") {
                        ensure!(
                            self.receiving.is_none(),
                            "A desktop drop is already being received"
                        );
                        self.receiving = Some((pane, vec![]));
                        self.drop_target = Some((
                            self.config.clone(),
                            self.active_app.as_ref().and_then(|key| {
                                self.app_hosts
                                    .get(key)
                                    .and_then(|h| h.selected.map(|id| (key.clone(), id)))
                            }),
                        ));
                        self.osc(&format!("t=r:x={}:i=1", index + 1), None);
                    } else {
                        self.osc("t=r:o=0:i=1", None);
                    }
                } else {
                    self.osc("t=r:o=0:i=1", None);
                }
                self.hover = None;
            }
            Some("r") | None => {
                if let Some((_, data)) = &mut self.receiving {
                    let chunk = STANDARD.decode(payload).or_else(|_| {
                        base64::engine::general_purpose::STANDARD_NO_PAD.decode(payload)
                    })?;
                    ensure!(
                        data.len() + chunk.len() <= 1024 * 1024,
                        "URI list exceeds 1 MiB"
                    );
                    data.extend(chunk);
                    if payload.is_empty() && n("m") <= 0 {
                        let (pane, data) = self.receiving.take().unwrap();
                        let text = String::from_utf8(data)?;
                        let paths = uri_paths(&text)?;
                        self.osc("t=r:o=1:i=1", None);
                        let destination = self.drop_target.take();
                        self.deliver_to(pane, paths, destination);
                    }
                }
            }
            Some("R") | Some("E") => {
                self.receiving = None;
                self.hover = None;
                self.note(format!("Drop: {payload}"));
            }
            _ => {}
        }
        Ok(())
    }
    fn deliver(&mut self, pane: usize, paths: Vec<PathBuf>) {
        self.deliver_to(pane, paths, None)
    }
    fn deliver_to(
        &mut self,
        pane: usize,
        paths: Vec<PathBuf>,
        destination: Option<(Config, Option<(String, u64)>)>,
    ) {
        if self.transfer.is_some() {
            self.note("A transfer is active; cancel it or wait before dropping again".into());
            return;
        }
        let (frozen_config, frozen_app) = destination.unwrap_or_else(|| {
            (
                self.config.clone(),
                self.active_app.as_ref().and_then(|key| {
                    self.app_hosts
                        .get(key)
                        .and_then(|h| h.selected.map(|id| (key.clone(), id)))
                }),
            )
        });
        let app_target = if pane == 32 {
            frozen_app.and_then(|(key, id)| {
                self.app_hosts
                    .get(&key)
                    .map(|h| (key, h.config.clone(), id))
            })
        } else {
            None
        };
        if pane == 32 && app_target.is_none() {
            self.note("Select an app window before dropping a file".into());
            return;
        }
        let config = app_target
            .as_ref()
            .map(|(_, c, _)| c.clone())
            .unwrap_or(frozen_config);
        let dest_pane = if pane == 32 { 1 } else { pane };
        let canceled = Arc::new(AtomicBool::new(false));
        self.transfer = Some(canceled.clone());
        let tx = self.tx.clone();
        self.note(format!(
            "Copying {} file(s) to {}…",
            paths.len(),
            config.host.as_deref().unwrap_or("local")
        ));
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                let mut references = vec![];
                for path in paths {
                    ensure!(!canceled.load(Ordering::Acquire), "Transfer canceled");
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let response = config.upload(dest_pane, &path, |count, total| {
                        let _ = tx.try_send(Event::Status(format!(
                            "Copying {name} / {count}/{total} bytes"
                        )));
                        !canceled.load(Ordering::Acquire)
                    })?;
                    references.push(
                        response["path"]
                            .as_str()
                            .context("Host returned no destination")?
                            .to_owned(),
                    );
                }
                ensure!(!canceled.load(Ordering::Acquire), "Transfer canceled");
                if let Some((key, _, window)) = app_target {
                    let uris = references
                        .iter()
                        .map(|p| {
                            format!(
                                "file://{}\r\n",
                                percent_encoding::utf8_percent_encode(p, URI_PATH)
                            )
                        })
                        .collect::<String>();
                    let _ = tx.send(Event::AppDelivered(key, window, uris));
                } else {
                    config.paste(
                        dest_pane,
                        &shell_words::join(references.iter().map(String::as_str)),
                    )?;
                    let _ = tx.send(Event::Delivered(
                        config.workspace,
                        dest_pane,
                        references.join(", "),
                    ));
                }
                Ok(())
            })();
            if let Err(e) = result {
                let _ = tx.send(Event::TransferFailed(format!("File delivery: {e:#}")));
            }
        });
    }
    fn paste(&mut self, text: String) {
        if self.state.space == Space::Apps {
            if let Some(host) = self.active_app.as_ref().and_then(|k| self.app_hosts.get(k))
                && let Err(e) = host.client.send(AppRequest::Input {
                    input: DesktopInput::Paste {
                        text,
                        mime: "text/plain;charset=utf-8".into(),
                    },
                })
            {
                self.note(e.to_string())
            }
            return;
        }
        let config = self.config.clone();
        let pane = self.state.selected();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            if let Err(e) = config.paste(pane, &text) {
                let _ = tx.send(Event::Error(format!("Paste: {e:#}")));
            }
        });
    }
    fn desktop_input(&mut self, input: DesktopInput) {
        if self.state.space == Space::Apps {
            if let Some(host) = self.active_app.as_ref().and_then(|k| self.app_hosts.get(k))
                && let Err(e) = host.client.send(AppRequest::Input { input })
            {
                self.note(e.to_string());
            }
        } else {
            self.desktop_send(DesktopCommand::Input(input));
        }
    }
    fn copy_kitty_selection(&self, target: (String, usize)) {
        let serial = self.selection_serial;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            // Wayland pointer delivery is asynchronous to Kitty's control socket.
            std::thread::sleep(Duration::from_millis(25));
            for _ in 0..4 {
                match crate::kitty_selection::read(&target.0, target.1) {
                    Ok(text) if !text.is_empty() => {
                        let _ = tx.send(Event::Selected(serial, text));
                        return;
                    }
                    Ok(_) => std::thread::sleep(Duration::from_millis(20)),
                    Err(_) => return,
                }
            }
        });
    }
    fn embedded(&mut self, kind: EmbeddedKind) {
        if let EmbeddedKind::Pointer {
            action,
            button,
            position,
            modifiers,
        } = &kind
            && self.state.space != Space::Apps
            && self.modal.is_none()
        {
            let panes = self.visible_panes();
            let separator = self.state.space == Space::Terminals
                && self.state.split
                && panes.len() == 2
                && position[0] >= f64::from(panes[0].1.width)
                && position[0] < f64::from(panes[0].1.width + 12);
            if !self.drag_split && !(action == "down" && separator) {
                if action == "down" && *button == 0 && *modifiers == 0 {
                    self.selection_serial = self.selection_serial.wrapping_add(1);
                    self.selection_target = panes
                        .iter()
                        .find(|(_, r)| {
                            position[0] >= f64::from(r.x) && position[0] < f64::from(r.x + r.width)
                        })
                        .map(|(pane, _)| (self.config.workspace.clone(), *pane));
                    if let Some((project, pane)) = &self.selection_target {
                        if self.state.space == Space::Terminals {
                            self.state.terminal = *pane;
                        }
                        self.desktop_send(DesktopCommand::FocusApp(app_id(project, *pane)));
                    }
                }
                let (events, copied) = self.text_gesture.route(crate::selection::Pointer {
                    action: action.clone(),
                    button: *button,
                    position: *position,
                    modifiers: *modifiers,
                });
                for event in events {
                    self.embedded_raw(EmbeddedKind::Pointer {
                        action: event.action,
                        button: event.button,
                        position: event.position,
                        modifiers: event.modifiers,
                    });
                }
                if copied
                    && self.desktop.is_some()
                    && let Some(target) = self.selection_target.take()
                {
                    self.copy_kitty_selection(target);
                }
                return;
            }
        }
        self.embedded_raw(kind);
    }
    fn embedded_raw(&mut self, kind: EmbeddedKind) {
        if matches!(&kind, EmbeddedKind::Key { .. } | EmbeddedKind::Text { .. }) {
            self.last_typing = Instant::now();
        }
        if self.modal.is_some() {
            if matches!(kind, EmbeddedKind::ReleaseAll) {
                self.release()
            }
            return;
        }
        if self.desktop.is_none() && self.state.space != Space::Apps {
            self.direct_input(kind);
            return;
        }
        match kind {
            EmbeddedKind::Pointer {
                action,
                button,
                position,
                modifiers,
            } => {
                if self.state.space == Space::Terminals && self.state.split {
                    let panes = self.visible_panes();
                    if panes.len() == 2 {
                        let left = panes[0].1;
                        if action == "down"
                            && position[0] >= f64::from(left.width)
                            && position[0] < f64::from(left.width + 12)
                        {
                            self.drag_split = true;
                            return;
                        }
                        if self.drag_split {
                            if action == "up" {
                                self.drag_split = false
                            } else {
                                self.state.split_ratio =
                                    (position[0] / f64::from(self.settings.width)).clamp(0.2, 0.8);
                                self.layout();
                            }
                            return;
                        }
                    }
                }
                if action == "down"
                    && self.state.space != Space::Apps
                    && let Some((pane, _)) = self.visible_panes().into_iter().find(|(_, r)| {
                        position[0] >= f64::from(r.x) && position[0] < f64::from(r.x + r.width)
                    })
                {
                    if self.state.space == Space::Terminals {
                        self.state.terminal = pane;
                    }
                    self.desktop_send(DesktopCommand::FocusApp(app_id(
                        &self.config.workspace,
                        pane,
                    )));
                }
                self.desktop_input(DesktopInput::Modifiers { modifiers });
                self.desktop_input(DesktopInput::Motion {
                    x: position[0],
                    y: position[1],
                });
                if action == "down" || action == "up" {
                    self.desktop_input(DesktopInput::Button {
                        button: match button {
                            1 => 273,
                            2 => 274,
                            _ => 272,
                        },
                        pressed: action == "down",
                    });
                }
                if action.starts_with("scroll_") {
                    self.desktop_input(DesktopInput::Scroll {
                        x: if action == "scroll_left" {
                            -1.
                        } else if action == "scroll_right" {
                            1.
                        } else {
                            0.
                        },
                        y: if action == "scroll_up" {
                            -1.
                        } else if action == "scroll_down" {
                            1.
                        } else {
                            0.
                        },
                    });
                }
            }
            EmbeddedKind::Key {
                code,
                phase,
                modifiers,
            } => self.desktop_input(DesktopInput::NamedKey {
                code,
                pressed: phase != KeyPhase::Release,
                modifiers,
            }),
            EmbeddedKind::Text { text } => self.paste(text),
            EmbeddedKind::ReleaseAll => self.release(),
        }
    }
    fn direct_input(&mut self, kind: EmbeddedKind) {
        if let EmbeddedKind::Pointer {
            action, position, ..
        } = &kind
        {
            let panes = self.visible_panes();
            if self.state.space == Space::Terminals && self.state.split && panes.len() == 2 {
                let left = panes[0].1;
                if action == "down"
                    && position[0] >= f64::from(left.width)
                    && position[0] < f64::from(left.width + 12)
                {
                    self.drag_split = true;
                    return;
                }
                if self.drag_split {
                    if action == "up" {
                        self.drag_split = false;
                        self.save();
                    } else {
                        self.state.split_ratio =
                            (position[0] / f64::from(self.settings.width)).clamp(0.2, 0.8);
                    }
                    return;
                }
            }
            if action == "down"
                && self.state.space == Space::Terminals
                && let Some((pane, _)) = panes.iter().find(|(_, r)| {
                    position[0] >= f64::from(r.x) && position[0] < f64::from(r.x + r.width)
                })
            {
                self.state.terminal = *pane;
            }
        }
        let pane = self.config.canonical_pane(self.state.selected());
        let rect = self
            .visible_panes()
            .into_iter()
            .find(|(p, _)| *p == pane)
            .map(|(_, r)| r);
        let Some(terminal) = self
            .terminals
            .get_mut(&(self.config.workspace.clone(), pane))
        else {
            return;
        };
        let result = match kind {
            EmbeddedKind::Key {
                code,
                phase,
                modifiers,
            } if phase != KeyPhase::Release => terminal.key(&code, modifiers),
            EmbeddedKind::Text { text } => terminal.paste(&text),
            EmbeddedKind::Pointer {
                action,
                button,
                position,
                modifiers,
            } => {
                if let Some(r) = rect {
                    terminal.pointer(
                        if action == "drag" { "move" } else { &action },
                        button,
                        [position[0] - f64::from(r.x), position[1] - f64::from(r.y)],
                        modifiers,
                    )
                } else {
                    Ok(())
                }
            }
            EmbeddedKind::ReleaseAll => {
                terminal.release();
                self.drag_split = false;
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.note(format!("Terminal input: {error:#}"));
        }
    }
    fn open_prompt(&mut self, kind: Prompt) {
        self.release();
        self.context_menu = None;
        let field = |name: &str, text: String| {
            (
                name.into(),
                TextInput::single().with_max_chars(2048).with_text(text),
            )
        };
        let directory = self.config.directory.clone().unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .display()
                .to_string()
        });
        let fields = match &kind {
            Prompt::RenameProject(id) => vec![field(
                "Project display name",
                self.projects
                    .iter()
                    .find(|p| &p.workspace == id)
                    .map(|p| p.label.as_deref().unwrap_or(&p.workspace))
                    .unwrap_or(id)
                    .to_owned(),
            )],
            Prompt::RenameTab(id) => vec![field("Terminal tab name", self.terminal_label(*id))],
            Prompt::RenameAgent(id) => vec![field("Agent name", self.agent_label(*id))],
            Prompt::Launch => vec![
                field("Application command and arguments", String::new()),
                field("Working directory on the app host", directory),
                field(
                    "App host / local or an SSH alias",
                    self.config.host.clone().unwrap_or("local".into()),
                ),
                field(
                    "S T A R / M U X helper on SSH host",
                    self.config.desktop_helper.clone(),
                ),
            ],
            Prompt::RunCommand => vec![
                field("Project app command", self.state.app_command.clone()),
                field("Project directory", directory),
            ],
            Prompt::Project => vec![
                field(
                    "Project name / letters, numbers, dash or underscore",
                    String::new(),
                ),
                field("Project host / local or an SSH alias", "local".into()),
                field("Project directory on that host", directory),
                field("Agent command", "codex".into()),
                field("Run app command", String::new()),
                field("S T A R / M U X helper on SSH host", "starmux".into()),
            ],
        };
        self.modal = Some(Modal {
            kind,
            fields,
            selected: 0,
            error: String::new(),
        });
    }
    fn launch_app(&mut self, config: Config, command: String, directory: String) -> Result<()> {
        ensure!(!command.trim().is_empty(), "Enter an application command");
        let args = shell_words::split(&command)?;
        ensure!(!args.is_empty(), "Enter an application command");
        let key = self.connect_app(config.clone())?;
        self.app_hosts[&key].client.send(AppRequest::Launch {
            launch: Launch {
                name: args[0].clone(),
                command,
                directory: PathBuf::from(&directory),
            },
        })?;
        if !self.state.app_targets.iter().any(|t| t.host == config.host) {
            self.state.app_targets.push(AppTarget {
                host: config.host.clone(),
                directory,
                helper: config.desktop_helper.clone(),
            });
        }
        self.active_app = Some(key);
        self.set_space(Space::Apps);
        self.note(format!(
            "Starting app on {}…",
            config.host.as_deref().unwrap_or("local")
        ));
        Ok(())
    }
    fn accept_prompt(&mut self) -> Result<()> {
        let modal = self.modal.take().context("No dialog is open")?;
        let values: Vec<String> = modal
            .fields
            .iter()
            .map(|(_, f)| f.text().trim().to_owned())
            .collect();
        let result = (|| -> Result<()> {
            match &modal.kind {
                Prompt::RenameProject(id) => {
                    project_state::validate_label(&values[0])?;
                    let mut config = self
                        .projects
                        .iter()
                        .find(|c| &c.workspace == id)
                        .context("Project no longer exists")?
                        .clone();
                    config.label = Some(values[0].clone());
                    let state = if id == &self.config.workspace {
                        self.state.clone()
                    } else {
                        self.states
                            .get(id)
                            .cloned()
                            .unwrap_or(ProjectState::load(&config)?)
                    };
                    state.save(&config)?;
                    starkit::fs::write_private(
                        &crate::runtime::config_path(id)?,
                        &serde_json::to_vec_pretty(&config)?,
                    )?;
                    if id == &self.config.workspace {
                        self.config = config.clone();
                    }
                    if let Some(project) = self.projects.iter_mut().find(|c| &c.workspace == id) {
                        *project = config;
                    }
                    self.note("Project renamed / sessions retained".into());
                }
                Prompt::RenameAgent(id) => {
                    project_state::validate_label(&values[0])?;
                    ensure!(
                        self.state.agent_ids().contains(id),
                        "Agent no longer exists"
                    );
                    let mut state = self.state.clone();
                    state.agent_labels.insert(*id, values[0].clone());
                    state.save(&self.config)?;
                    self.state = state;
                }
                Prompt::RenameTab(id) => {
                    project_state::validate_label(&values[0])?;
                    ensure!(
                        self.state.terminals.contains(id),
                        "Terminal no longer exists"
                    );
                    let mut state = self.state.clone();
                    state.terminal_labels.insert(*id, values[0].clone());
                    state.save(&self.config)?;
                    self.state = state;
                    self.note("Tab renamed / session retained".into());
                }
                Prompt::RunCommand => {
                    ensure!(
                        !values[0].is_empty() && !values[1].is_empty(),
                        "Command and directory are required"
                    );
                    self.state.app_command = values[0].clone();
                    self.config.directory = Some(values[1].clone());
                    self.save();
                    self.launch_app(self.config.clone(), values[0].clone(), values[1].clone())?
                }
                Prompt::Launch => {
                    ensure!(
                        !values[1].is_empty(),
                        "Enter the working directory on the app host"
                    );
                    let mut config = self.config.clone();
                    config.host = if values[2] == "local" || values[2].is_empty() {
                        None
                    } else {
                        Some(values[2].clone())
                    };
                    config.desktop_helper = values[3].clone();
                    config.project_host = true;
                    config.validate()?;
                    ensure!(
                        !config.desktop_helper.is_empty(),
                        "Enter the remote helper executable"
                    );
                    self.launch_app(config, values[0].clone(), values[1].clone())?
                }
                Prompt::Project => {
                    let config = Config {
                        ssh_config: self.config.ssh_config.clone(),
                        workspace: values[0].clone(),
                        label: None,
                        collection: self.config.collection.clone(),
                        herdr: None,
                        theme: self.config.theme.clone(),
                        host: if values[1] == "local" || values[1].is_empty() {
                            None
                        } else {
                            Some(values[1].clone())
                        },
                        directory: Some(values[2].clone()),
                        agent: Some(values[3].clone()),
                        executable: std::env::current_exe()?.display().to_string(),
                        project_host: true,
                        desktop_helper: values[5].clone(),
                    };
                    config.validate()?;
                    ensure!(
                        !values[2].is_empty() && !values[3].is_empty(),
                        "Project directory and agent command are required"
                    );
                    ensure!(
                        !self
                            .projects
                            .iter()
                            .any(|c| c.workspace == config.workspace),
                        "A project with that name already exists"
                    );
                    shell_words::split(&values[3])?;
                    let state = ProjectState {
                        app_command: values[4].clone(),
                        ..ProjectState::default()
                    };
                    state.save(&config)?;
                    starkit::fs::write_private(
                        &crate::runtime::config_path(&config.workspace)?,
                        &serde_json::to_vec_pretty(&config)?,
                    )?;
                    let id = config.workspace.clone();
                    self.projects.push(config);
                    self.switch_project(&id)?;
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            let mut modal = modal;
            modal.error = format!("{e:#}");
            self.modal = Some(modal);
        }
        Ok(())
    }
    fn modal_key(&mut self, code: String, modifiers: u8) -> Result<()> {
        if code == "escape" {
            self.modal = None;
            return Ok(());
        }
        if code == "enter"
            && (modifiers & 2 != 0
                || self
                    .modal
                    .as_ref()
                    .is_some_and(|m| m.selected + 1 == m.fields.len()))
        {
            return self.accept_prompt();
        }
        let modal = self.modal.as_mut().unwrap();
        if code == "tab" || code == "enter" {
            modal.selected = (modal.selected + 1) % modal.fields.len();
            return Ok(());
        }
        if code == "backtab" {
            modal.selected = (modal.selected + modal.fields.len() - 1) % modal.fields.len();
            return Ok(());
        }
        use starkit::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = match code.as_str() {
            "backspace" => KeyCode::Backspace,
            "delete" => KeyCode::Delete,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            _ => {
                if let Some(c) = code.strip_prefix("char:").and_then(|s| s.chars().next()) {
                    KeyCode::Char(c)
                } else {
                    return Ok(());
                }
            }
        };
        let mut mods = KeyModifiers::empty();
        if modifiers & 1 != 0 {
            mods |= KeyModifiers::SHIFT
        }
        if modifiers & 2 != 0 {
            mods |= KeyModifiers::CONTROL
        }
        if modifiers & 4 != 0 {
            mods |= KeyModifiers::ALT
        }
        modal.fields[modal.selected]
            .1
            .handle(KeyEvent::new(key, mods));
        Ok(())
    }
    fn git_location(&self) -> String {
        format!(
            "{}:{}",
            self.config.host.as_deref().unwrap_or("local"),
            self.config.directory.as_deref().unwrap_or(".")
        )
    }
    fn git_status(&self) -> Option<&crate::git::Status> {
        self.git
            .get(&self.config.workspace)
            .filter(|(location, _, _)| location == &self.git_location())
            .map(|(_, status, _)| status)
    }
    fn refresh_git(&mut self, force: bool) {
        let id = self.config.workspace.clone();
        let location = self.git_location();
        if self.git_pending.contains(&id) {
            return;
        }
        if !force
            && self.git.get(&id).is_some_and(|(l, _, time)| {
                l == &location && time.elapsed() < Duration::from_secs(3)
            })
        {
            return;
        }
        self.git_pending.insert(id.clone());
        let config = self.config.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let status = crate::git::read(&config).unwrap_or_else(|e| crate::git::Status {
                kind: "error".into(),
                error: format!("{e:#}"),
                ..Default::default()
            });
            let _ = tx.send(Event::Git(id, location, Box::new(status)));
        });
    }
    fn terminal_label(&self, id: usize) -> String {
        self.state
            .terminal_labels
            .get(&id)
            .cloned()
            .or_else(|| {
                self.config
                    .herdr
                    .as_ref()
                    .and_then(|h| h.panes.get(&id))
                    .map(|p| p.label.clone())
            })
            .unwrap_or_else(|| {
                let n = self
                    .state
                    .terminals
                    .iter()
                    .position(|i| *i == id)
                    .unwrap_or(0);
                if n == 0 {
                    "Shell".into()
                } else {
                    format!("Shell {}", n + 1)
                }
            })
    }
    fn open_context_menu(&mut self, target: Option<&str>, point: [u16; 2]) {
        use crate::shelf::{ContextMenu, MenuItem};
        let item = |label: &str, hint: &str, action: String| MenuItem {
            label: label.into(),
            hint: hint.into(),
            action,
        };
        let (title, subtitle, items) =
            if let Some(id) = target.and_then(|t| t.strip_prefix("project:")) {
                let Some(project) = self.projects.iter().find(|p| p.workspace == id) else {
                    return;
                };
                (
                    project.label.as_deref().unwrap_or(id).to_owned(),
                    format!("PROJECT / {}", project.host.as_deref().unwrap_or("local")),
                    vec![
                        item("Rename project…", "", format!("rename-project:{id}")),
                        item("Open agent", "", format!("project-space:agent:{id}")),
                        item(
                            "Open terminals",
                            "",
                            format!("project-space:terminals:{id}"),
                        ),
                        item("Open app space", "", format!("project-space:apps:{id}")),
                        item("New project…", "+", "project:new".into()),
                    ],
                )
            } else if let Some(id) = target
                .and_then(|t| t.strip_prefix("agent:"))
                .and_then(|s| s.parse::<usize>().ok())
            {
                if !self.state.agent_ids().contains(&id) {
                    return;
                }
                (
                    self.agent_label(id),
                    "AGENT / INDEPENDENT SESSION".into(),
                    vec![
                        item("Rename agent…", "", format!("rename-agent:{id}")),
                        item("Focus agent", "", format!("agent:{id}")),
                        item("New agent", "+", "agent:new".into()),
                    ],
                )
            } else if let Some(id) = target
                .and_then(|t| t.strip_prefix("terminal:"))
                .and_then(|id| id.parse::<usize>().ok())
            {
                if !self.state.terminals.contains(&id) {
                    return;
                }
                (
                    self.terminal_label(id),
                    "TERMINAL / SESSION RETAINED".into(),
                    vec![
                        item("Rename tab…", "", format!("rename-tab:{id}")),
                        item("Focus tab", "", format!("terminal:{id}")),
                        item(
                            if self.state.split {
                                "Unsplit terminals"
                            } else {
                                "Split with another tab"
                            },
                            "",
                            format!("split-tab:{id}"),
                        ),
                        item("New terminal tab", "+", "terminal:new".into()),
                    ]
                    .into_iter()
                    .chain(self.state.split.then(|| {
                        item(
                            "Close split pane (keep tab)",
                            "",
                            format!("close-pane:{id}"),
                        )
                    }))
                    .collect(),
                )
            } else {
                self.context_menu = None;
                return;
            };
        self.release();
        self.context_menu = Some(ContextMenu {
            position: point,
            title,
            subtitle,
            items,
            selected: None,
        });
    }
    fn menu_key(&mut self, code: &str) -> Result<()> {
        let Some(menu) = &mut self.context_menu else {
            return Ok(());
        };
        let len = menu.items.len();
        match code {
            "escape" => self.context_menu = None,
            "down" | "tab" => menu.selected = Some(menu.selected.map_or(0, |i| (i + 1) % len)),
            "up" | "backtab" => {
                menu.selected = Some(menu.selected.map_or(len - 1, |i| (i + len - 1) % len))
            }
            "home" => menu.selected = Some(0),
            "end" => menu.selected = Some(len - 1),
            "enter" => {
                if let Some(i) = menu.selected {
                    self.action(&format!("menu:item:{i}"))?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn action(&mut self, action: &str) -> Result<()> {
        if action == "menu:dismiss" {
            self.context_menu = None;
            return Ok(());
        }
        if action == "menu:panel" {
            return Ok(());
        }
        if let Some(index) = action.strip_prefix("menu:item:") {
            let next = self
                .context_menu
                .as_ref()
                .and_then(|m| m.items.get(index.parse::<usize>().ok()?))
                .map(|item| item.action.clone());
            self.context_menu = None;
            if let Some(next) = next {
                self.action(&next)?;
            }
            return Ok(());
        }
        if action == "modal:cancel" {
            self.modal = None;
            return Ok(());
        }
        if action == "modal:accept" {
            return self.accept_prompt();
        }
        if let Some(index) = action.strip_prefix("modal:field:") {
            if let (Some(modal), Ok(index)) = (&mut self.modal, index.parse::<usize>()) {
                modal.selected = index.min(modal.fields.len() - 1)
            }
            return Ok(());
        }
        if self.modal.is_some() {
            return Ok(());
        }
        if let Some(id) = action.strip_prefix("rename-project:") {
            self.open_prompt(Prompt::RenameProject(id.into()));
            return Ok(());
        }
        if let Some(id) = action.strip_prefix("rename-agent:") {
            self.open_prompt(Prompt::RenameAgent(id.parse()?));
            return Ok(());
        }
        if let Some(id) = action.strip_prefix("rename-tab:") {
            self.open_prompt(Prompt::RenameTab(id.parse()?));
            return Ok(());
        }
        if let Some((space, id)) = action
            .strip_prefix("project-space:")
            .and_then(|s| s.split_once(':'))
        {
            self.switch_project(id)?;
            self.set_space(match space {
                "terminals" => Space::Terminals,
                "apps" => Space::Apps,
                _ => Space::Agent,
            });
            return Ok(());
        }
        if let Some(id) = action.strip_prefix("split-tab:") {
            self.action(&format!("terminal:{id}"))?;
            return self.action("terminal:split");
        }
        if let Some(id) = action.strip_prefix("close-pane:") {
            let id = id.parse::<usize>()?;
            ensure!(
                self.state.terminals.contains(&id),
                "Terminal is not in this project"
            );
            self.state.terminal = id;
            return self.action("terminal:close-pane");
        }
        match action {
            "git:details" => {
                self.release();
                self.context_menu = None;
                self.git_open = true;
                self.git_scroll = 0;
                self.refresh_git(false);
            }
            "git:close" => self.git_open = false,
            "git:panel" => {}
            "git:refresh" => self.refresh_git(true),
            "git:scroll:up" => self.git_scroll = self.git_scroll.saturating_sub(3),
            "git:scroll:down" => {
                let rows = crate::shelf::git_rows(self.viewport.height as u16);
                let max = self
                    .git_status()
                    .map_or(0, |s| s.files.len().saturating_sub(rows));
                self.git_scroll = (self.git_scroll + 3).min(max);
            }
            "theme:next" => self.cycle_theme()?,
            "view:agent" => self.set_space(Space::Agent),
            "view:terminals" => self.set_space(Space::Terminals),
            "view:apps" => self.set_space(Space::Apps),
            "projects:scroll:up" => {
                self.project_offset = self.project_offset.saturating_sub(3);
            }
            "projects:scroll:down" => {
                self.project_offset = (self.project_offset + 3).min(
                    self.projects
                        .len()
                        .saturating_sub(self.project_page_size.max(1)),
                );
            }
            "project:page:left" => {
                self.project_offset = self
                    .project_offset
                    .saturating_sub(self.project_page_size.max(1));
            }
            "project:page:right" => {
                self.project_offset = (self.project_offset + self.project_page_size.max(1)).min(
                    self.projects
                        .len()
                        .saturating_sub(self.project_page_size.max(1)),
                );
            }
            "project:new" => self.open_prompt(Prompt::Project),
            "terminal:font:increase" | "terminal:font:decrease" | "terminal:font:reset" => {
                self.state.terminal_font_size = match action {
                    "terminal:font:increase" => {
                        ((self.font_pixels().round() as u16) + 1).clamp(10, 32)
                    }
                    "terminal:font:decrease" => (self.font_pixels().round() as u16)
                        .saturating_sub(1)
                        .clamp(10, 32),
                    _ => crate::project_state::default_terminal_font_size(),
                };
                self.state.terminal_font_explicit = action != "terminal:font:reset";
                self.save();
                self.refresh_kitty_fonts();
            }
            "agent:new" => {
                self.release();
                let mut state = self.state.clone();
                state.add_agent()?;
                state.save(&self.config)?;
                self.state = state;
                self.ensure_sessions();
                self.layout();
                self.note("New agent / fresh session in the project directory".into());
            }
            "agent:previous" | "agent:next" => {
                let ids = self.state.agent_ids();
                let n = ids
                    .iter()
                    .position(|id| *id == self.state.agent)
                    .unwrap_or(0);
                let next = if action == "agent:next" {
                    (n + 1) % ids.len()
                } else {
                    (n + ids.len() - 1) % ids.len()
                };
                self.state.agent = ids[next];
                self.set_space(Space::Agent);
            }
            "terminal:new" => {
                self.release();
                self.state.add_terminal()?;
                self.ensure_sessions();
                self.layout();
                self.save()
            }
            "terminal:split" => {
                self.release();
                if self.state.terminals.len() == 1 {
                    self.state.add_terminal()?;
                    self.ensure_sessions();
                }
                self.state.split = !self.state.split;
                self.state.space = Space::Terminals;
                self.layout();
                self.save()
            }
            "terminal:close-pane" => {
                if self.state.split {
                    self.release();
                    if let Some(other) = self
                        .state
                        .terminals
                        .iter()
                        .find(|id| **id != self.state.terminal)
                    {
                        self.state.terminal = *other;
                    }
                    self.state.split = false;
                    self.layout();
                    self.save();
                    self.note("Pane closed / terminal still available in its tab".into());
                }
            }
            "launch-app" => self.open_prompt(Prompt::Launch),
            "run-app" => {
                if self.state.app_command.trim().is_empty() {
                    self.open_prompt(Prompt::RunCommand)
                } else {
                    let directory = self.config.directory.clone().unwrap_or_else(|| {
                        std::env::current_dir()
                            .unwrap_or_default()
                            .display()
                            .to_string()
                    });
                    self.launch_app(
                        self.config.clone(),
                        self.state.app_command.clone(),
                        directory,
                    )?;
                }
            }
            "agent:attach" => {
                let key = (
                    self.config.workspace.clone(),
                    self.config.canonical_pane(self.state.agent),
                );
                if self.desktop.is_none()
                    && !self
                        .terminals
                        .get(&key)
                        .is_some_and(|terminal| !terminal.exited)
                {
                    self.terminals.remove(&key);
                    self.registered.remove(&key);
                }
                self.set_space(Space::Agent);
                let pane = self.state.agent;
                if self
                    .health
                    .get(&(self.config.workspace.clone(), pane))
                    .is_some_and(|v| v["alive"] == false)
                {
                    let command = self.agent_command(pane);
                    let config = self.config.clone();
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        let result = (|| -> Result<()> {
                            let mut req = config.request(pane, "restart");
                            req["argv"] = json!(shell_words::split(&command)?);
                            req["directory"] = json!(config.directory);
                            config.call(pane, &req)?;
                            Ok(())
                        })();
                        let _ = tx.send(match result {
                            Ok(()) => {
                                Event::Status("Agent restarted in its existing session".into())
                            }
                            Err(e) => Event::Error(format!("Agent: {e:#}")),
                        });
                    });
                } else if !self
                    .registered
                    .contains(&(self.config.workspace.clone(), pane))
                {
                    self.ensure_sessions();
                }
            }
            "apps:reconnect" => {
                self.release();
                let config = self
                    .active_app
                    .as_ref()
                    .and_then(|key| self.app_hosts.remove(key))
                    .map(|h| h.config)
                    .unwrap_or_else(|| self.config.clone());
                let _ = self.connect_app(config)?;
                self.note("Reconnecting to the existing app desktop…".into());
            }
            "apps:tile" | "apps:maximize" | "apps:close" => {
                if let Some(host) = self
                    .active_app
                    .as_ref()
                    .and_then(|key| self.app_hosts.get(key))
                {
                    let request = if action == "apps:tile" {
                        Some(AppRequest::Tile)
                    } else {
                        host.selected.map(|window| {
                            if action == "apps:close" {
                                AppRequest::Close { window }
                            } else {
                                AppRequest::Maximize { window }
                            }
                        })
                    };
                    if let Some(request) = request {
                        host.client.send(request)?;
                    }
                }
            }
            "cancel-transfer" => {
                if let Some(cancel) = &self.transfer {
                    cancel.store(true, Ordering::Release);
                    self.note("Canceling transfer…".into())
                }
            }
            _ => {
                if let Some(id) = action.strip_prefix("project:") {
                    self.switch_project(id)?
                } else if let Some(id) = action.strip_prefix("agent:") {
                    let id = id.parse::<usize>()?;
                    ensure!(
                        self.state.agent_ids().contains(&id),
                        "Agent is not in this project"
                    );
                    self.state.agent = id;
                    self.set_space(Space::Agent);
                } else if let Some(id) = action.strip_prefix("terminal:") {
                    let id = id.parse::<usize>()?;
                    ensure!(
                        self.state.terminals.contains(&id),
                        "Terminal is not in this project"
                    );
                    self.state.terminal = id;
                    self.set_space(Space::Terminals);
                } else if let Some(command) = action.strip_prefix("run:") {
                    let directory = self.config.directory.clone().unwrap_or_else(|| {
                        std::env::current_dir()
                            .unwrap_or_default()
                            .display()
                            .to_string()
                    });
                    self.launch_app(self.config.clone(), command.into(), directory)?;
                } else if let Some((key, id)) =
                    action.strip_prefix("app:").and_then(|s| s.rsplit_once(':'))
                {
                    let id = id.parse::<u64>()?;
                    ensure!(
                        key.starts_with(&format!("{}@", self.config.workspace)),
                        "App belongs to another project"
                    );
                    if let Some(host) = self.app_hosts.get_mut(key) {
                        ensure!(
                            host.windows.iter().any(|w| w.id == id),
                            "App window no longer exists"
                        );
                        host.selected = Some(id);
                        host.client.send(AppRequest::Focus { window: id })?;
                        self.active_app = Some(key.into());
                        self.set_space(Space::Apps);
                    }
                } else {
                    anyhow::bail!("Unknown action: {action}")
                }
            }
        }
        Ok(())
    }
    fn draw(&mut self, v: Viewport) -> Scene {
        self.viewport = v;
        let layout = crate::shelf::Layout::new(v.width as u16, v.height as u16, self.state.space);
        let content = layout.canvas;
        let direct = self.desktop.is_none() && self.state.space != Space::Apps;
        let width = if self.state.space != Space::Apps {
            u32::from(content.width)
        } else {
            u32::from(content.width).clamp(128, 2560)
        };
        let height = if self.state.space != Space::Apps {
            u32::from(content.height)
        } else {
            u32::from(content.height).clamp(64, 1440)
        };
        let fps = if self.state.space == Space::Apps {
            30
        } else {
            self.terminal_fps
        };
        if self.settings.width != width
            || self.settings.height != height
            || self.settings.fps != fps
        {
            self.settings.width = width;
            self.settings.height = height;
            self.settings.fps = fps;
            self.desktop_send(DesktopCommand::Resize(self.settings.clone()));
            self.layout();
            for host in self.app_hosts.values() {
                if self.state.space != Space::Apps {
                    continue;
                }
                let _ = host.client.send(AppRequest::Resize {
                    settings: self.settings.clone(),
                });
            }
        }
        let host = self.config.host.as_deref().unwrap_or("local");
        let directory = self.config.directory.clone().unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .display()
                .to_string()
        });
        let health = self.health.get(&(
            self.config.workspace.clone(),
            self.config.canonical_pane(self.state.agent),
        ));
        let agent_name = health
            .and_then(|v| v["command"].as_str())
            .unwrap_or(self.config.agent.as_deref().unwrap_or("codex"));
        let raw_agent_state = health
            .and_then(|v| v["state"].as_str())
            .unwrap_or("connecting");
        let agent_state = if raw_agent_state == "unknown" {
            if health.is_some_and(|v| v["alive"] == true) {
                "running"
            } else {
                "stopped"
            }
        } else {
            raw_agent_state
        };
        let page_size = crate::shelf::project_rows(v.height as u16);
        if page_size != self.project_page_size {
            self.project_page_size = page_size;
            self.project_offset = self
                .project_offset
                .min(self.projects.len().saturating_sub(page_size));
            if let Some(index) = self
                .projects
                .iter()
                .position(|p| p.workspace == self.config.workspace)
            {
                if index < self.project_offset {
                    self.project_offset = index;
                } else if index >= self.project_offset + page_size {
                    self.project_offset = index + 1 - page_size;
                }
            }
        }
        let projects = self
            .projects
            .iter()
            .map(|c| crate::shelf::ProjectTab {
                id: &c.workspace,
                name: c.label.as_deref().unwrap_or(&c.workspace),
                host: c.host.as_deref().unwrap_or("local"),
                working: self.project_activity(c).working,
                waiting: self.project_activity(c).waiting,
            })
            .collect();
        let active_app = &self.active_app;
        let apps = self
            .app_hosts
            .iter()
            .filter(|(_, h)| h.config.workspace == self.config.workspace)
            .flat_map(|(key, h)| {
                h.windows.iter().map(move |w| crate::shelf::AppCard {
                    action: format!("app:{key}:{}", w.id),
                    name: &w.name,
                    host: h.config.host.as_deref().unwrap_or("local"),
                    selected: active_app.as_ref() == Some(key) && h.selected == Some(w.id),
                })
            })
            .collect();
        let modal = self.modal.as_ref().map(|m| crate::shelf::Modal {
            title: match &m.kind {
                Prompt::RenameProject(_) => "Rename project",
                Prompt::RenameTab(_) => "Rename terminal tab",
                Prompt::RenameAgent(_) => "Rename agent",
                Prompt::Project => "New project container",
                Prompt::Launch => "Launch app in this project",
                Prompt::RunCommand => "Set the project app command",
            },
            fields: m
                .fields
                .iter()
                .map(|(label, input)| crate::shelf::ModalField {
                    label,
                    text: input.text(),
                    cursor: input.cursor(),
                })
                .collect(),
            selected: m.selected,
            error: &m.error,
        });
        let connected = self
            .active_app
            .as_ref()
            .and_then(|key| self.app_hosts.get(key))
            .is_some_and(|h| h.client.connected.load(Ordering::Acquire));
        let (chrome, foreground) = crate::shelf::surfaces(
            layout,
            &self.appearance,
            &crate::shelf::Model {
                project: &self.config.workspace,
                project_label: self
                    .config
                    .label
                    .as_deref()
                    .unwrap_or(&self.config.workspace),
                project_offset: self.project_offset,
                project_total: self.projects.len(),
                wave_phase: self.wave_phase,
                reduced_motion: self.reduced_motion,
                directory: &directory,
                host,
                theme: &self.appearance.name,
                space: self.state.space,
                projects,
                terminals: self
                    .state
                    .terminals
                    .iter()
                    .map(|id| (*id, self.terminal_label(*id)))
                    .collect(),
                terminal: self.state.terminal,
                terminal_font_size: self.font_label(),
                split: self.state.split,
                agent_name,
                agent_state,
                agent: self.state.agent,
                agents: self
                    .state
                    .agent_ids()
                    .iter()
                    .map(|id| crate::shelf::AgentTab {
                        id: *id,
                        name: self.agent_label(*id),
                        state: self
                            .health
                            .get(&(
                                self.config.workspace.clone(),
                                self.config.canonical_pane(*id),
                            ))
                            .map(|v| {
                                if v["alive"] == false {
                                    "stopped"
                                } else {
                                    v["state"]
                                        .as_str()
                                        .filter(|s| *s != "unknown")
                                        .unwrap_or("running")
                                }
                            })
                            .unwrap_or("starting")
                            .to_owned(),
                    })
                    .collect(),
                status: &self.status,
                apps,
                connected,
                desktop_host: self
                    .active_app
                    .as_ref()
                    .and_then(|key| self.app_hosts.get(key))
                    .and_then(|h| h.config.host.as_deref())
                    .unwrap_or("local"),
                modal,
                context_menu: self.context_menu.as_ref(),
                git: self.git_status(),
                git_open: self.git_open,
                git_scroll: self.git_scroll,
                transferring: self.transfer.is_some(),
                hover: self.hover.is_some(),
            },
        );
        self.chrome_hits = chrome
            .hits
            .iter()
            .chain(&foreground.hits)
            .cloned()
            .collect();
        let mut scene = Scene::from_buffer(
            &starkit::ratatui::buffer::Buffer::empty(starkit::ratatui::layout::Rect::new(
                0, 0, v.columns, v.rows,
            )),
            v,
            0,
        );
        scene.background = self.appearance.bg.clone();
        let full = Rect {
            x: 0,
            y: 0,
            width: v.columns,
            height: v.rows,
        };
        scene.components.push(Component::Surface {
            rect: full,
            surface: chrome,
        });
        let view = ContentView::new([self.settings.width, self.settings.height], content).unwrap();
        self.view = Some(view);
        let kitty_frame_matches = self.desktop.as_ref().is_some_and(|d| {
            d.latest
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|f| f.width == width && f.height == height)
        });
        if (self.state.space == Space::Apps && connected)
            || (self.state.space != Space::Apps && (direct || kitty_frame_matches))
        {
            scene.components.push(Component::LiveCanvas {
                rect: Rect {
                    x: (u32::from(content.x) * u32::from(v.columns) / v.width.max(1)) as u16,
                    y: (u32::from(content.y) * u32::from(v.rows) / v.height.max(1)) as u16,
                    width: ((u32::from(content.width) * u32::from(v.columns) / v.width.max(1))
                        as u16)
                        .max(1),
                    height: ((u32::from(content.height) * u32::from(v.rows) / v.height.max(1))
                        as u16)
                        .max(1),
                },
                id: if self.state.space == Space::Apps {
                    "apps"
                } else {
                    "terminals"
                }
                .into(),
                view,
                interactive: self.modal.is_none(),
            });
        }
        if direct {
            for (pane, r) in self.visible_panes() {
                if let Some(terminal) = self
                    .terminals
                    .get_mut(&(self.config.workspace.clone(), pane))
                {
                    terminal.set_metrics(self.typography);
                    terminal.set_font_size(self.state.terminal_font_size);
                    if let Err(error) = terminal.resize(r.width, r.height) {
                        self.status = format!("Terminal resize: {error:#}");
                    }
                    let mut surfaces = vec![];
                    terminal.paint(r, &self.appearance, &mut surfaces);
                    for mut surface in surfaces {
                        for node in &mut surface.nodes {
                            let bounds = match node {
                                starkit::native_surface::Primitive::Fill { rect, .. }
                                | starkit::native_surface::Primitive::Border { rect, .. }
                                | starkit::native_surface::Primitive::Text { rect, .. } => rect,
                                _ => continue,
                            };
                            bounds.x += r.x + content.x;
                            bounds.y += r.y + content.y;
                        }
                        surface.width = v.width as u16;
                        surface.height = v.height as u16;
                        // Explicit pane background; subsequent chunks are transparent.
                        if surface.background != "transparent" {
                            let background = surface.background.clone();
                            surface.background = "transparent".into();
                            surface.nodes.insert(
                                0,
                                starkit::native_surface::Primitive::Fill {
                                    rect: PixelRect::new(
                                        r.x + content.x,
                                        r.y + content.y,
                                        r.width,
                                        r.height,
                                    ),
                                    color: background,
                                    radius: 0,
                                },
                            );
                        }
                        scene.components.push(Component::Surface {
                            rect: full,
                            surface,
                        });
                    }
                }
            }
        }
        scene.components.push(Component::Surface {
            rect: full,
            surface: foreground,
        });
        scene
    }
}
impl Controller for Workspace {
    fn capabilities(
        &mut self,
        capabilities: starkit::terminal_graphics::capabilities::Capabilities,
    ) {
        if std::env::var_os("STAR_GRAPHICS_FONT_DEBUG").is_some() {
            eprintln!(
                "Controller font report: {}",
                serde_json::to_value(capabilities).unwrap()
            );
        }
        self.typography = serde_json::to_value(capabilities)
            .ok()
            .and_then(|v| serde_json::from_value(v["terminal_typography"].clone()).ok())
            .filter(crate::terminal::FontMetrics::valid);
        self.refresh_kitty_fonts();
    }
    fn tick(&mut self) {
        self.refresh_git(false);
        self.refresh_activity();
        for event in self.rx.try_iter().collect::<Vec<_>>() {
            match event {
                Event::Selected(serial, text) => {
                    if serial == self.selection_serial {
                        self.effects.push(ServerMessage::Clipboard { text });
                    }
                }
                Event::Activity(activity) => {
                    self.activity = activity;
                    self.activity_pending = false;
                    self.activity_at = Instant::now();
                }
                Event::Git(id, location, status) => {
                    self.git_pending.remove(&id);
                    self.git.insert(id, (location, *status, Instant::now()));
                }
                Event::Ready(config, pane) => {
                    self.launch(*config, pane);
                    self.note("Project session connected / process retained".into())
                }
                Event::Health(project, pane, value) => {
                    self.health.insert((project, pane), value);
                }
                Event::Status(status) => self.status = status,
                Event::Delivered(project, pane, path) => {
                    self.transfer = None;
                    self.note(format!(
                        "Copied to {project} / session {}: {path} / press Enter when ready",
                        pane + 1
                    ));
                }
                Event::AppDelivered(key, window, uris) => {
                    self.transfer = None;
                    if let Some(host) = self.app_hosts.get(&key)
                        && host.windows.iter().any(|w| w.id == window)
                    {
                        let result =
                            host.client
                                .send(AppRequest::Focus { window })
                                .and_then(|_| {
                                    host.client.send(AppRequest::Input {
                                        input: DesktopInput::Paste {
                                            text: uris,
                                            mime: "text/uri-list".into(),
                                        },
                                    })
                                });
                        match result {
                            Ok(()) => self.note(
                                "File copied to app host / URI clipboard offered to selected app"
                                    .into(),
                            ),
                            Err(e) => self.note(format!("App delivery: {e:#}")),
                        }
                    } else {
                        self.note("File staged on app host; original destination app closed before delivery".into())
                    }
                }
                Event::TransferFailed(message) => {
                    self.transfer = None;
                    self.note(message)
                }
                Event::Error(message) => self.note(message),
            }
        }
        for terminal in self.terminals.values_mut() {
            if let Some(text) = terminal.take_clipboard() {
                self.effects.push(ServerMessage::Clipboard { text });
            }
            if let Err(error) = terminal.poll() {
                self.status = format!("Terminal: {error:#}");
            }
        }
        let desktop_events = self
            .desktop
            .as_ref()
            .map(|d| d.events.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in desktop_events {
            match event {
                DesktopEvent::Error(message) => self.note(message),
                DesktopEvent::Windows(windows) => {
                    self.windows = windows;
                    self.layout()
                }
                _ => {}
            }
        }
        let mut errors = vec![];
        for host in self.app_hosts.values_mut() {
            for event in host.client.events.try_iter() {
                match event {
                    DesktopEvent::Windows(windows) => {
                        if !host
                            .selected
                            .is_some_and(|id| windows.iter().any(|w| w.id == id))
                        {
                            host.selected = windows.last().map(|w| w.id)
                        }
                        host.windows = windows;
                    }
                    DesktopEvent::Error(message) => errors.push(message),
                    _ => {}
                }
            }
        }
        for error in errors {
            self.note(error)
        }
        let missing = self
            .launched
            .iter()
            .filter(|((project, pane), time)| {
                time.elapsed() > Duration::from_secs(5)
                    && !self
                        .windows
                        .iter()
                        .any(|w| w.app_id == app_id(project, *pane))
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for (project, pane) in missing {
            if let Some(config) = self
                .projects
                .iter()
                .find(|c| c.workspace == project)
                .cloned()
            {
                self.launch(config, pane);
            }
        }
        let frame = if self.state.space == Space::Apps {
            self.active_app
                .as_ref()
                .and_then(|key| self.app_hosts.get(key))
                .and_then(|host| host.client.latest.lock().unwrap().clone())
        } else {
            self.desktop
                .as_ref()
                .and_then(|d| d.latest.lock().unwrap().clone())
        };
        let source = if self.state.space == Space::Apps {
            self.active_app.clone().unwrap_or_default()
        } else {
            format!("terminals:{}:{:?}", self.config.workspace, self.state.space)
        };
        if let Some(frame) = frame
            && (self.state.space == Space::Apps
                || (frame.width == self.settings.width && frame.height == self.settings.height))
            && !self
                .last_frame
                .as_ref()
                .is_some_and(|(key, pixels)| key == &source && Arc::ptr_eq(pixels, &frame.pixels))
        {
            if self.state.space != Space::Apps
                && std::env::var_os("STARMUX_RENDER_TIMING").is_some()
            {
                eprintln!(
                    "Terminal submit: at_ns={} age_us={}",
                    crate::runtime::monotonic_ns(),
                    frame.time.elapsed().as_micros()
                );
            }
            self.last_frame = Some((source, frame.pixels.clone()));
            let stream = if self.state.space == Space::Apps {
                "apps"
            } else {
                "terminals"
            };
            if let Err(e) = self.frames.submit(
                stream,
                1,
                Frame {
                    width: frame.width,
                    height: frame.height,
                    stride: frame.width * 4,
                    format: PixelFormat::Bgra8,
                    presentation_ns: self.started.elapsed().as_nanos() as u64,
                    pixels: frame.pixels,
                },
            ) {
                self.note(format!("Desktop frame: {e:#}"))
            }
        }
        if self.last_inspect.elapsed() > Duration::from_millis(500) {
            self.last_inspect = Instant::now();
            if let Ok(root) = root() {
                if let Ok(bytes) = fs::read(root.join(format!("{}.json", self.config.workspace)))
                    && let Ok(config) = serde_json::from_slice::<Config>(&bytes)
                    && let Ok(theme) = crate::appearance::Appearance::load(&config.theme)
                    && theme != self.appearance
                {
                    self.apply_theme(theme)
                }
                let panes = (0..2)
                    .map(|i| {
                        self.health
                            .get(&(self.config.workspace.clone(), i))
                            .cloned()
                            .unwrap_or(json!({"state":"starting"}))
                    })
                    .collect::<Vec<_>>();
                let apps=self.app_hosts.iter().filter(|(_,h)|h.config.workspace==self.config.workspace).map(|(key,h)|json!({"key":key,"host":h.config.host,"connected":h.client.connected.load(Ordering::Acquire),"transport_pid":h.client.transport_pid(),"worker_pid":*h.client.pid.lock().unwrap(),"windows":h.windows,"selected":h.selected})).collect::<Vec<_>>();
                let sessions=self.state.terminals.iter().copied().chain(self.state.agent_ids()).map(|i|json!({"pane":i,"agent":self.state.agent_ids().contains(&i),"health":self.health.get(&(self.config.workspace.clone(),self.config.canonical_pane(i)))})).collect::<Vec<_>>();
                let snapshot = json!({"workspace":self.controller_id,"active_project":self.config.workspace,"project_label":self.config.label,"project_activity":self.projects.iter().map(|p|(&p.workspace,self.project_activity(p))).collect::<BTreeMap<_,_>>(),"wave_phase":self.wave_phase,"reduced_motion":self.reduced_motion,"collection":self.config.collection,"project_order":self.projects.iter().map(|p|&p.workspace).collect::<Vec<_>>(),"project_page":self.project_offset,"project_navigation":{"kind":"sidebar","scroll":self.project_offset,"visible":self.projects.iter().skip(self.project_offset).take(self.project_page_size).map(|p|&p.workspace).collect::<Vec<_>>(),"sidebar_width":crate::shelf::Layout::new(self.viewport.width as u16,self.viewport.height as u16,self.state.space).sidebar.width},"herdr_read_only":false,"herdr_live_attachment":self.config.herdr.is_some(),"controller_pid":std::process::id(),"panes":panes,"active":self.state.selected(),"status":self.status,"terminal_presentation":{"fps":self.settings.fps,"burst_fps":if self.settings.fps>=60{125}else{30},"canvas":[crate::shelf::Layout::new(self.viewport.width as u16,self.viewport.height as u16,self.state.space).canvas.width,crate::shelf::Layout::new(self.viewport.width as u16,self.viewport.height as u16,self.state.space).canvas.height],"frame":self.desktop.as_ref().and_then(|d|d.latest.lock().unwrap().as_ref().map(|f|[f.width,f.height])),"kitty_configuration":"user"},"terminal_typography":self.typography,"terminal_renderer":if self.desktop.is_none(){"direct"}else{"kitty"},"direct_terminals":self.terminals.iter().filter(|((p,_),_)|p==&self.config.workspace).map(|((_,pane),t)|json!({"pane":pane,"attachment_pid":t.pid,"exited":t.exited,"text":t.text()})).collect::<Vec<_>>(),"desktop":{"width":self.settings.width,"height":self.settings.height,"windows":self.windows.len(),"project_windows":self.windows.iter().filter(|w|w.app_id.starts_with(&format!("starmux-{}-p",self.config.workspace))).collect::<Vec<_>>()},"sessions":sessions,"project_state":self.state,"apps":apps,"drop_enabled":self.drop_enabled,"history":self.history,"git":self.git_status(),"git_open":self.git_open,"git_scroll":self.git_scroll,"context_menu":self.context_menu.as_ref().map(|m|json!({"title":m.title,"bounds":m.bounds(self.viewport.width as u16,self.viewport.height as u16),"selected":m.selected,"items":m.items.iter().map(|i|json!({"label":i.label,"action":i.action})).collect::<Vec<_>>()})),"modal_open":self.modal.is_some(),"modal_field":self.modal.as_ref().map(|m|m.selected),"appearance":{"treatment":"app-shelf","borders":"single","theme":self.config.theme,"tokens":self.appearance},"viewport":self.viewport});
                let _ = starkit::fs::write_private(
                    &root.join(format!("{}.state.json", self.controller_id)),
                    &serde_json::to_vec_pretty(&snapshot).unwrap(),
                );
                if root.join(format!("{}.close", self.controller_id)).exists() {
                    self.quit = true;
                    let _ = fs::remove_file(root.join(format!("{}.close", self.controller_id)));
                }
                let actions = root.join(format!("{}.actions", self.controller_id));
                if let Ok(entries) = fs::read_dir(actions) {
                    let mut entries = entries.flatten().map(|e| e.path()).collect::<Vec<_>>();
                    entries.sort();
                    for path in entries.into_iter().take(32) {
                        if let Ok(bytes) = fs::read(&path)
                            && bytes.len() <= 8192
                            && let Ok(action) = serde_json::from_slice::<String>(&bytes)
                            && let Err(e) = self.action(&action)
                        {
                            self.note(format!("{e:#}"));
                        }
                        let _ = fs::remove_file(path);
                    }
                }
            }
        }
    }
    fn scene(&mut self, v: Viewport) -> Scene {
        self.draw(v)
    }
    fn input(&mut self, input: Input) {
        let result = (|| -> Result<()> {
            // An overlay can also open via an action while a canvas owns keys.
            // Normalize those keys before dispatch, never forwarding them to a PTY.
            let input = match input {
                Input::Embedded { event, .. }
                    if self.git_open || self.context_menu.is_some() || self.modal.is_some() =>
                {
                    match event.kind {
                        EmbeddedKind::Key {
                            code,
                            modifiers,
                            phase,
                        } if phase != KeyPhase::Release => Input::Key { code, modifiers },
                        EmbeddedKind::Text { text } => Input::Paste { text },
                        _ => return Ok(()),
                    }
                }
                input => input,
            };
            if self.git_open {
                match input {
                    Input::Key { code, .. } => match code.as_str() {
                        "escape" => self.git_open = false,
                        "down" | "pagedown" => self.action("git:scroll:down")?,
                        "up" | "pageup" => self.action("git:scroll:up")?,
                        "char:r" => self.refresh_git(true),
                        _ => {}
                    },
                    Input::Pointer {
                        action,
                        button,
                        pixel,
                        x,
                        y,
                        ..
                    } => {
                        if action == "scroll_up" || action == "scroll_down" {
                            self.action(if action == "scroll_up" {
                                "git:scroll:up"
                            } else {
                                "git:scroll:down"
                            })?;
                        } else if action == "down" && button == 0 {
                            let point = pixel.unwrap_or([
                                u32::from(x) * self.viewport.width
                                    / u32::from(self.viewport.columns.max(1)),
                                u32::from(y) * self.viewport.height
                                    / u32::from(self.viewport.rows.max(1)),
                            ]);
                            let hit = self
                                .chrome_hits
                                .iter()
                                .rev()
                                .find(|h| h.rect.contains(point[0] as u16, point[1] as u16))
                                .map(|h| h.action.clone());
                            if let Some(hit) = hit.filter(|h| h.starts_with("git:")) {
                                self.action(&hit)?;
                            }
                        }
                    }
                    Input::Notice { message } => {
                        if let Some(action) = message.strip_prefix("action:") {
                            self.action(action)?;
                        }
                    }
                    Input::Osc72 { text } => self.drop_message(&text)?,
                    _ => {}
                }
                return Ok(());
            }
            if self.context_menu.is_some() {
                match &input {
                    Input::Key { code, .. } => {
                        self.menu_key(code)?;
                        return Ok(());
                    }
                    Input::Embedded { .. } | Input::Paste { .. } => return Ok(()),
                    Input::CancelPointer => {
                        self.context_menu = None;
                        self.release();
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if self.modal.is_none()
                && let Input::Pointer {
                    action,
                    button,
                    pixel,
                    x,
                    y,
                    ..
                } = &input
                && (self.context_menu.is_some() || (action == "down" && *button == 1))
            {
                let point = pixel.unwrap_or([
                    u32::from(*x) * self.viewport.width / u32::from(self.viewport.columns.max(1)),
                    u32::from(*y) * self.viewport.height / u32::from(self.viewport.rows.max(1)),
                ]);
                let hit = self
                    .chrome_hits
                    .iter()
                    .rev()
                    .find(|h| h.rect.contains(point[0] as u16, point[1] as u16))
                    .map(|h| h.action.clone());
                if self.context_menu.is_some() {
                    if action == "down" {
                        if *button == 0 {
                            if let Some(hit) = hit {
                                self.action(&hit)?;
                            }
                        } else {
                            self.context_menu = None;
                        }
                    } else if action == "move" {
                        let selected = hit
                            .as_deref()
                            .and_then(|h| h.strip_prefix("menu:item:"))
                            .and_then(|i| i.parse().ok());
                        if let Some(menu) = &mut self.context_menu {
                            menu.selected = selected;
                        }
                    }
                } else {
                    self.open_context_menu(hit.as_deref(), [point[0] as u16, point[1] as u16]);
                }
                return Ok(());
            }
            if self.modal.is_some() {
                match input {
                    Input::Key { code, modifiers } => self.modal_key(code, modifiers)?,
                    Input::Paste { text } => {
                        let modal = self.modal.as_mut().unwrap();
                        modal.fields[modal.selected].1.paste(&text)
                    }
                    Input::Pointer {
                        action,
                        pixel,
                        x,
                        y,
                        ..
                    } if action == "down" => {
                        let point = pixel.unwrap_or([
                            u32::from(x) * self.viewport.width
                                / u32::from(self.viewport.columns.max(1)),
                            u32::from(y) * self.viewport.height
                                / u32::from(self.viewport.rows.max(1)),
                        ]);
                        let action = self
                            .chrome_hits
                            .iter()
                            .rev()
                            .find(|h| h.rect.contains(point[0] as u16, point[1] as u16))
                            .map(|h| h.action.clone());
                        if let Some(action) = action {
                            self.action(&action)?
                        }
                    }
                    Input::Osc72 { text } => self.drop_message(&text)?,
                    _ => {}
                }
                return Ok(());
            }
            match input {
                Input::Embedded { event, .. } => self.embedded(event.kind),
                Input::Osc72 { text } => self.drop_message(&text)?,
                Input::Paste { text } => self.paste(text),
                Input::Key { code, .. } => match code.as_str() {
                    "char:q" => self.quit = true,
                    "char:t" => self.cycle_theme()?,
                    "char:1" => self.set_space(Space::Agent),
                    "char:2" => self.set_space(Space::Terminals),
                    "char:3" => self.set_space(Space::Apps),
                    "char:n" => self.action("terminal:new")?,
                    "char:a" => self.action("launch-app")?,
                    "char:r" => self.action("run-app")?,
                    "char:p" => self.open_prompt(Prompt::Project),
                    "char:[" | "char:]" => {
                        if let Some(index) = self
                            .projects
                            .iter()
                            .position(|c| c.workspace == self.config.workspace)
                        {
                            let next = if code == "char:[" {
                                (index + self.projects.len() - 1) % self.projects.len()
                            } else {
                                (index + 1) % self.projects.len()
                            };
                            let id = self.projects[next].workspace.clone();
                            self.switch_project(&id)?;
                        }
                    }
                    "escape" => self.action("cancel-transfer")?,
                    _ => {}
                },
                Input::Pointer {
                    action,
                    pixel,
                    x,
                    y,
                    ..
                } if matches!(action.as_str(), "scroll_up" | "scroll_down") => {
                    let point = pixel.unwrap_or([
                        u32::from(x) * self.viewport.width
                            / u32::from(self.viewport.columns.max(1)),
                        u32::from(y) * self.viewport.height / u32::from(self.viewport.rows.max(1)),
                    ]);
                    let layout = crate::shelf::Layout::new(
                        self.viewport.width as u16,
                        self.viewport.height as u16,
                        self.state.space,
                    );
                    if point[0] < u32::from(layout.sidebar.width) {
                        self.action(if action == "scroll_up" {
                            "projects:scroll:up"
                        } else {
                            "projects:scroll:down"
                        })?;
                    }
                }
                Input::Pointer {
                    action,
                    pixel,
                    x,
                    y,
                    ..
                } if action == "down" => {
                    let point = pixel.unwrap_or([
                        u32::from(x) * self.viewport.width
                            / u32::from(self.viewport.columns.max(1)),
                        u32::from(y) * self.viewport.height / u32::from(self.viewport.rows.max(1)),
                    ]);
                    let action = self
                        .chrome_hits
                        .iter()
                        .rev()
                        .find(|h| h.rect.contains(point[0] as u16, point[1] as u16))
                        .map(|h| h.action.clone());
                    if let Some(action) = action {
                        self.action(&action)?;
                    }
                }
                Input::Notice { message } => {
                    if let Some(paths) = message.strip_prefix("drop:") {
                        self.deliver(self.state.selected(), serde_json::from_str(paths)?)
                    } else if let Some(action) = message.strip_prefix("action:") {
                        self.action(action)?
                    }
                }
                Input::CancelPointer => self.release(),
                _ => {}
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.receiving = None;
            self.osc("t=r:o=0:i=1", None);
            self.note(format!("{e:#}"));
        }
    }
    fn frame_interval(&self) -> Duration {
        Duration::from_millis(33)
    }
    fn live_frames(&mut self) -> Vec<Update> {
        self.frames.drain()
    }
    fn effects(&mut self) -> Vec<ServerMessage> {
        std::mem::take(&mut self.effects)
    }
    fn closed(&self) -> bool {
        self.quit
    }
    fn shutdown(&mut self) {
        self.release();
        if let Some(cancel) = &self.transfer {
            cancel.store(true, Ordering::Release)
        }
        self.save();
    }
}
pub fn uri_paths(text: &str) -> Result<Vec<PathBuf>> {
    let mut paths = vec![];
    for line in text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let raw = line
            .strip_prefix("file://")
            .context("Only local file URI drops are supported")?;
        let owned;
        let raw = if raw.starts_with('/') {
            raw
        } else {
            owned = format!(
                "/{}",
                raw.strip_prefix("localhost/")
                    .context("File offer is from another machine")?
            );
            &owned
        };
        let decoded = percent_encoding::percent_decode_str(raw).decode_utf8()?;
        let p = PathBuf::from(decoded.as_ref());
        ensure!(p.is_absolute(), "Drop path must be absolute");
        paths.push(p);
        ensure!(paths.len() <= 32, "Drop limit is 32 files");
    }
    ensure!(!paths.is_empty(), "No files in drop");
    Ok(paths)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_uri_names_and_authorities() {
        assert_eq!(
            uri_paths(
                "# offer\nfile:///tmp/screen%20shot.png\r\nfile://localhost/tmp/%CE%BB.txt\r\n"
            )
            .unwrap(),
            vec![
                PathBuf::from("/tmp/screen shot.png"),
                PathBuf::from("/tmp/λ.txt")
            ]
        );
        assert!(uri_paths("file://remote/tmp/a").is_err());
        assert!(uri_paths("https://example.com/a").is_err());
        assert!(uri_paths("").is_err());
    }
}
