#[path = "cells.rs"]
mod cells;
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
    Applications(
        String,
        Instant,
        std::result::Result<Vec<crate::applications::Application>, String>,
    ),
    Git(String, String, Box<crate::git::Status>),
    Activity(BTreeMap<String, crate::activity::Activity>),
    Selected(u64, String),
    Health(String, usize, Value, Arc<AtomicBool>),
    Status(String),
    Delivered(String, usize, String),
    AppDelivered(String, u64, String),
    Error(String),
    TransferFailed(String),
}
const AUDIO_SESSION: u64 = 0x535441524d5558;
struct AudioRoute {
    key: String,
    epoch: u64,
    source: crate::app_audio::Source,
    playback: crate::app_audio::Playback,
    sent: u64,
    credits: u64,
}
struct AppHost {
    config: Config,
    client: AppClient,
    windows: Vec<WindowInfo>,
    layout: Vec<WindowLayout>,
    selected: Option<u64>,
}
#[derive(Clone)]
enum Prompt {
    Deliver,
    Launch,
    Project,
    RunCommand,
    RenameProject(String),
    RenameTab(usize),
    RenameAgent(usize),
}
struct Catalog {
    source: String,
    changed: Instant,
    requested: bool,
    apps: Vec<crate::applications::Application>,
    query: TextInput,
    selected: usize,
    offset: usize,
    focused: bool,
    visible: bool,
    status: String,
}
impl Catalog {
    fn matches(&self) -> Vec<usize> {
        self.apps
            .iter()
            .enumerate()
            .filter(|(_, a)| a.matches(self.query.text()))
            .map(|(i, _)| i)
            .collect()
    }
}
struct Modal {
    kind: Prompt,
    catalog: Option<Catalog>,
    fields: Vec<(String, TextInput)>,
    selected: usize,
    error: String,
}
struct ProjectDrag {
    id: String,
    start: [u32; 2],
    gap: Option<usize>,
    moved: bool,
    scrolled_at: Instant,
}
pub struct Workspace {
    cells: bool,
    require_graphical: bool,
    cell_ui: cells::Ui,
    controller_id: String,
    config: Config,
    state: ProjectState,
    projects: Vec<Config>,
    project_offset: usize,
    project_page_size: usize,
    sort_hover: bool,
    sort_mode_hover: bool,
    project_hover: Option<String>,
    project_drag: Option<ProjectDrag>,
    navigation: crate::navigation::Navigation,
    navigation_saved_at: Instant,
    states: BTreeMap<String, ProjectState>,
    workspace_id: String,
    shared_synced_at: Instant,
    catalog_stamp: Option<std::time::SystemTime>,
    appearance: crate::appearance::Appearance,
    typography: Option<crate::terminal::FontMetrics>,
    chrome_hits: Vec<HitRegion>,
    graphical_cache: crate::graphical::Cache,
    desktop: Option<Desktop>,
    terminals: BTreeMap<(String, usize), crate::terminal::Terminal>,
    frames: LiveFrames,
    rx: Receiver<Event>,
    tx: Sender<Event>,
    health: BTreeMap<(String, usize), Value>,
    registered: BTreeSet<(String, usize)>,
    session_monitors: BTreeMap<(String, usize), Arc<AtomicBool>>,
    session_epochs: BTreeMap<(String, usize), Option<String>>,
    launched: BTreeMap<(String, usize), Instant>,
    windows: Vec<WindowInfo>,
    app_hosts: BTreeMap<String, AppHost>,
    app_view_available: bool,
    video_capable: bool,
    video_high_quality: bool,
    video: crate::remote_video::Video,
    video_failed: bool,
    video_source: Option<String>,
    terminal_transport: &'static str,
    audio_capable: bool,
    audio_pcm_v2: bool,
    audio_route: Option<AudioRoute>,
    audio_epoch: u64,
    audio_failed: Option<String>,
    active_app: Option<String>,
    view: Option<ContentView>,
    viewport: Viewport,
    settings: Settings,
    terminal_fps: u32,
    native_terminal_cells: bool,
    native_layout_key: String,
    native_font_points: Option<u32>,
    initial_native_font_points: Option<u32>,
    app_pointer_modifiers: crate::compositor::PointerModifiers,
    pointer_hiding: bool,
    cursor_policy_sent: Option<bool>,
    status: String,
    history: Vec<String>,
    effects: Vec<ServerMessage>,
    quit: bool,
    last_inspect: Instant,
    started: Instant,
    receiving: Option<(usize, Vec<u8>)>,
    receiving_remote: bool,
    receiving_image: Option<&'static str>,
    receiving_mime: u64,
    #[cfg(feature = "lossless-canvas")]
    remote_drop: Option<starkit::terminal_graphics::drop_receive::Incoming>,
    drop_stage: Option<tempfile::TempDir>,
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
fn app_key(config: &Config) -> String {
    format!(
        "{}@{}",
        config.workspace,
        config.host.as_deref().unwrap_or("local")
    )
}
fn app_id(project: &str, pane: usize) -> String {
    format!("starflux-{project}-p{pane}")
}
pub(crate) fn native_terminal_rect(
    pane: PixelRect,
    canvas: PixelRect,
    viewport: Viewport,
) -> (Rect, [u16; 2]) {
    let width = viewport.width.max(1);
    let height = viewport.height.max(1);
    let cols = u32::from(viewport.columns.max(1));
    let rows = u32::from(viewport.rows.max(1));
    let px = u32::from(canvas.x) + u32::from(pane.x);
    let py = u32::from(canvas.y) + u32::from(pane.y);
    let x = (px * cols).div_ceil(width).min(cols.saturating_sub(2));
    let y = (py * rows).div_ceil(height).min(rows.saturating_sub(1));
    let right = ((px + u32::from(pane.width)) * cols / width)
        .max(x + 2)
        .min(cols);
    let bottom = ((py + u32::from(pane.height)) * rows / height)
        .max(y + 1)
        .min(rows);
    (
        Rect {
            x: x as u16,
            y: y as u16,
            width: (right - x).min(512) as u16,
            height: (bottom - y).min(192) as u16,
        },
        [
            (x * width / cols).saturating_sub(px) as u16,
            (y * height / rows).saturating_sub(py) as u16,
        ],
    )
}
impl Workspace {
    pub fn new(config: Config) -> Result<Self> {
        let id = config.workspace.clone();
        Self::new_view(config, id)
    }
    pub fn new_view(config: Config, controller_id: String) -> Result<Self> {
        let workspace_id = config.workspace.clone();
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
        let state = ProjectState::load_view(&config, &controller_id)?;
        state.save(&config)?;
        let mut projects = project_state::catalog(&config);
        let mut navigation = crate::navigation::Navigation::load(&controller_id)?;
        if controller_id != workspace_id
            && !project_state::data_root()?
                .join(format!("{controller_id}.navigation.json"))
                .exists()
        {
            navigation = crate::navigation::Navigation::load(&workspace_id)?;
        }
        navigation.open_project(&config.workspace);
        navigation.filter_open(&mut projects);
        navigation.sort(&mut projects);
        let (controller_tx, rx) = bounded(128);
        let terminal_fps = if std::env::var("STARFLUX_TERMINAL_PACING").as_deref() == Ok("legacy") {
            30
        } else {
            60
        };
        let settings = Settings {
            width: 1200,
            height: 650,
            fps: terminal_fps,
        };
        // Presentation is negotiated before starting any graphical service.
        let desktop = None;
        let mut s = Self {
            cells: false,
            require_graphical: false,
            cell_ui: cells::Ui::default(),
            controller_id,
            config,
            state,
            projects,
            project_offset: 0,
            project_page_size: 0,
            sort_hover: false,
            sort_mode_hover: false,
            project_hover: None,
            project_drag: None,
            navigation,
            navigation_saved_at: Instant::now(),
            states: BTreeMap::new(),
            workspace_id,
            shared_synced_at: Instant::now(),
            catalog_stamp: None,
            appearance,
            typography: None,
            chrome_hits: vec![],
            graphical_cache: Default::default(),
            desktop,
            terminals: BTreeMap::new(),
            frames: LiveFrames::default(),
            rx,
            tx: controller_tx,
            health: BTreeMap::new(),
            registered: BTreeSet::new(),
            session_monitors: BTreeMap::new(),
            session_epochs: BTreeMap::new(),
            launched: BTreeMap::new(),
            windows: vec![],
            app_hosts: BTreeMap::new(),
            app_view_available: false,
            video_capable: false,
            video_high_quality: false,
            video: Default::default(),
            video_failed: false,
            video_source: None,
            terminal_transport: "negotiating",
            audio_capable: false,
            audio_pcm_v2: false,
            audio_route: None,
            audio_epoch: 0,
            audio_failed: None,
            active_app: None,
            view: None,
            viewport: Viewport::default(),
            settings,
            terminal_fps,
            native_terminal_cells: false,
            native_layout_key: String::new(),
            native_font_points: None,
            initial_native_font_points: None,
            app_pointer_modifiers: Default::default(),
            pointer_hiding: false,
            cursor_policy_sent: None,
            status: "Connecting project sessions…".into(),
            history: vec![],
            effects: vec![],
            quit: false,
            last_inspect: Instant::now(),
            started: Instant::now(),
            receiving: None,
            receiving_remote: false,
            receiving_image: None,
            receiving_mime: 1,
            #[cfg(feature = "lossless-canvas")]
            remote_drop: None,
            drop_stage: None,
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
            reduced_motion: std::env::var("STARFLUX_REDUCED_MOTION").as_deref() == Ok("1"),
            git: BTreeMap::new(),
            git_pending: BTreeSet::new(),
            git_open: false,
            git_scroll: 0,
        };
        s.ensure_sessions();
        Ok(s)
    }
    fn require_graphical(&mut self) {
        self.require_graphical = true;
        self.check_presentation();
    }
    fn check_presentation(&mut self) {
        if self.require_graphical && self.cells {
            self.effects.push(ServerMessage::Error {
                message: "Graphical presentation is unavailable; use --cells in this terminal"
                    .into(),
            });
            self.quit = true;
        }
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
        if let Err(e) = self
            .state
            .save(&self.config)
            .and_then(|_| self.state.save_view(&self.config, &self.controller_id))
        {
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
                        std::env::var("STARFLUX_KITTEN").unwrap_or_else(|_| "kitten".into()),
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
        #[cfg(feature = "lossless-canvas")]
        if self.desktop.is_none() {
            return self.native_font_points.map_or_else(
                || "Kitty".into(),
                |size| {
                    if size % 1000 == 0 {
                        format!("{}pt", size / 1000)
                    } else {
                        format!("{:.1}pt", f64::from(size) / 1000.)
                    }
                },
            );
        }
        let size = self.font_pixels();
        if (size - size.round()).abs() < 0.01 {
            format!("{size:.0}px")
        } else {
            format!("{size:.1}px")
        }
    }
    fn release(&mut self) {
        self.app_pointer_modifiers.reset();
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
            && self.state.space != Space::Apps
            && self.last_typing.elapsed() >= Duration::from_millis(150)
            && self.wave_at.elapsed() >= Duration::from_millis(110)
            && self.projects.iter().any(|p| {
                let a = self.project_activity(p);
                a.working > 0
            })
        {
            self.wave_phase = (self.wave_phase + 1) % 192;
            self.wave_at = Instant::now();
        }
    }
    fn project_order_input(&mut self, input: &Input) -> bool {
        if matches!(input, Input::CancelPointer)
            || matches!(input, Input::Key { code, .. } if code == "escape")
        {
            return self.project_drag.take().is_some();
        }
        if self.modal.is_some() || self.context_menu.is_some() || self.git_open {
            self.project_drag = None;
            return false;
        }
        let Input::Pointer {
            action,
            button,
            pixel,
            x,
            y,
            ..
        } = input
        else {
            if self.project_drag.is_some() && matches!(input, Input::Embedded { .. }) {
                // Leaving the sidebar cancels rather than routing the drag into a terminal.
                self.project_drag = None;
                return true;
            }
            return false;
        };
        let point = pixel.unwrap_or([
            u32::from(*x) * self.viewport.width / u32::from(self.viewport.columns.max(1)),
            u32::from(*y) * self.viewport.height / u32::from(self.viewport.rows.max(1)),
        ]);
        if self.project_drag.is_none() {
            if !self.navigation.recent
                && action == "down"
                && *button == 0
                && let Some(id) = self
                    .chrome_hits
                    .iter()
                    .rev()
                    .find(|h| h.rect.contains(point[0] as u16, point[1] as u16))
                    .and_then(|h| h.action.strip_prefix("project:"))
                    .filter(|id| self.projects.iter().any(|p| p.workspace == *id))
                    .map(str::to_owned)
            {
                self.release();
                self.project_drag = Some(ProjectDrag {
                    id,
                    start: point,
                    gap: None,
                    moved: false,
                    scrolled_at: Instant::now(),
                });
                return true;
            }
            return false;
        }
        if *button != 0 && action == "down" {
            self.project_drag = None;
            return true;
        }
        let drag = self.project_drag.as_mut().unwrap();
        drag.moved |= point[0].abs_diff(drag.start[0]) > 6 || point[1].abs_diff(drag.start[1]) > 6;
        let width = crate::shelf::Layout::new(
            self.viewport.width as u16,
            self.viewport.height as u16,
            self.state.space,
        )
        .sidebar
        .width;
        let rows = self.project_page_size.max(1);
        let bottom = 122 + rows as u32 * 34;
        if drag.moved
            && point[0] < u32::from(width)
            && drag.scrolled_at.elapsed() >= Duration::from_millis(120)
            && matches!(
                action.as_str(),
                "drag" | "move" | "scroll_up" | "scroll_down"
            )
        {
            if point[1] < 136 || action == "scroll_up" {
                self.project_offset = self.project_offset.saturating_sub(1);
            } else if point[1] >= bottom.saturating_sub(14) || action == "scroll_down" {
                self.project_offset =
                    (self.project_offset + 1).min(self.projects.len().saturating_sub(rows));
            }
            drag.scrolled_at = Instant::now();
        }
        drag.gap = (point[0] < u32::from(width) && point[1] >= 110 && point[1] <= bottom + 12)
            .then(|| {
                (self.project_offset + ((point[1].saturating_sub(122) + 17) / 34) as usize)
                    .min(self.projects.len())
            });
        if action == "up" {
            let drag = self.project_drag.take().unwrap();
            if drag.moved {
                if let Some(gap) = drag.gap {
                    self.navigation.reconcile(&self.projects);
                    if self.navigation.reorder(&drag.id, gap) {
                        self.navigation.sort(&mut self.projects);
                        if let Err(error) = self.navigation.save(&self.controller_id) {
                            self.note(format!("Could not save project order: {error:#}"));
                        }
                        self.navigation_saved_at = Instant::now();
                    }
                }
            } else if let Err(error) = self.switch_project(&drag.id) {
                self.note(format!("{error:#}"));
            }
        }
        true
    }
    fn sort_projects(&mut self, reset_scroll: bool) {
        self.navigation.filter_open(&mut self.projects);
        if self.project_drag.is_some() {
            return;
        }
        let page = self.project_page_size.max(1);
        let selected_visible = self
            .projects
            .iter()
            .position(|p| p.workspace == self.config.workspace)
            .is_some_and(|i| i >= self.project_offset && i < self.project_offset + page);
        let anchor = self
            .projects
            .get(self.project_offset)
            .map(|p| p.workspace.clone());
        self.navigation.sort(&mut self.projects);
        if reset_scroll {
            self.project_offset = 0;
        } else if selected_visible {
            if let Some(i) = self
                .projects
                .iter()
                .position(|p| p.workspace == self.config.workspace)
            {
                if i < self.project_offset {
                    self.project_offset = i;
                } else if i >= self.project_offset + page {
                    self.project_offset = i + 1 - page;
                }
            }
        } else if let Some(i) =
            anchor.and_then(|a| self.projects.iter().position(|p| p.workspace == a))
        {
            self.project_offset = i;
        }
        self.project_offset = self
            .project_offset
            .min(self.projects.len().saturating_sub(page));
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
            for pane in state.terminal_ids().into_iter().filter(|id| {
                !project
                    .herdr
                    .as_ref()
                    .is_some_and(|h| h.panes.contains_key(id))
            }) {
                if let Some(health) = self.health.get(&(project.workspace.clone(), pane)) {
                    any = true;
                    if health["alive"] == true && health["agent"].as_str().is_some() {
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
    fn remove_exited_terminal(&mut self, project: &str, pane: usize) -> Result<()> {
        let current = self.config.workspace == project;
        let mut config = if current {
            self.config.clone()
        } else {
            self.projects
                .iter()
                .find(|config| config.workspace == project)
                .context("Project not found")?
                .clone()
        };
        let _transaction = crate::project_sync::transaction(&config)?;
        let shared = ProjectState::load(&config)?;
        let expected = self.session_epochs.get(&(project.to_owned(), pane));
        if shared.terminal_ids().contains(&pane)
            && expected.is_some_and(|epoch| epoch != &shared.terminal_epochs.get(&pane).cloned())
        {
            if current {
                self.sync_project()?;
            }
            return Ok(());
        }
        let mut state = if current {
            self.state.clone()
        } else {
            ProjectState::load(&config)?
        };
        if !state.remove_terminal(pane) {
            return Ok(());
        }
        let aliases = config
            .herdr
            .as_ref()
            .map(|source| {
                source
                    .panes
                    .keys()
                    .copied()
                    .filter(|id| config.canonical_pane(*id) == pane)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if let Some(source) = &mut config.herdr {
            source.panes.retain(|id, _| !aliases.contains(id));
        }
        state.reserved_slots.retain(|id| !aliases.contains(id));
        state.save(&config)?;
        let key = (project.to_owned(), pane);
        if let Some(stopped) = self.session_monitors.remove(&key) {
            stopped.store(true, Ordering::Release);
        }
        self.registered.remove(&key);
        self.launched.remove(&key);
        self.health.remove(&key);
        self.terminals.remove(&key);
        if let Some(saved) = self
            .projects
            .iter_mut()
            .find(|saved| saved.workspace == project)
        {
            *saved = config.clone();
        }
        if current {
            if self.state.visible().contains(&pane) {
                self.release();
            }
            self.config = config;
            self.state = state;
            #[cfg(feature = "lossless-canvas")]
            self.native_layout_key.clear();
            self.layout();
            self.note("Terminal closed".into());
        }
        Ok(())
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
            if !self.registered.insert(key.clone()) {
                continue;
            }
            let stopped = Arc::new(AtomicBool::new(false));
            let epoch = self.state.terminal_epochs.get(&pane).cloned();
            self.session_epochs.insert(key.clone(), epoch.clone());
            self.session_monitors.insert(key, stopped.clone());
            let command = agents
                .iter()
                .find(|id| self.config.canonical_pane(**id) == pane)
                .and_then(|id| self.config.agent.as_ref().map(|_| self.agent_command(*id)));
            let config = self.config.clone();
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let ensured = (|| -> Result<bool> {
                    let _transaction = crate::project_sync::transaction(&config)?;
                    let state = ProjectState::load(&config)?;
                    if stopped.load(Ordering::Acquire)
                        || !state.terminal_ids().contains(&pane)
                        || state.terminal_epochs.get(&pane).cloned() != epoch
                    {
                        return Ok(false);
                    }
                    config.ensure(pane, command.as_deref())?;
                    Ok(true)
                })();
                match ensured {
                    Ok(true) => {
                        let _ = tx.send(Event::Ready(Box::new(config.clone()), pane));
                    }
                    Ok(false) => return,
                    Err(e) => {
                        let _ = tx.send(Event::Error(format!(
                            "{} / session {}: {e:#}",
                            config.workspace,
                            pane + 1
                        )));
                        return;
                    }
                }
                while !stopped.load(Ordering::Acquire) {
                    let mut value = config
                        .call(pane, &config.request(pane, "inspect"))
                        .unwrap_or_else(
                            |e| json!({"state":"disconnected","error":format!("{e:#}")}),
                        );
                    if stopped.load(Ordering::Acquire) {
                        break;
                    }
                    let mut closed = false;
                    if value["exited"] == true {
                        // Reap and remove membership under the same model transaction.
                        // Another view cannot reuse this slot before removal is durable.
                        let result = (|| -> Result<bool> {
                            let _transaction = crate::project_sync::transaction(&config)?;
                            let mut state = ProjectState::load(&config)?;
                            if !state.terminal_ids().contains(&pane)
                                || state.terminal_epochs.get(&pane).cloned() != epoch
                            {
                                return Ok(false);
                            }
                            let result = config.call(pane, &config.request(pane, "reap"))?;
                            if result["reaped"] != true {
                                return Ok(false);
                            }
                            state.remove_terminal(pane);
                            state.save(&config)?;
                            Ok(true)
                        })();
                        if matches!(result, Ok(true)) {
                            value["reaped"] = json!(true);
                            closed = true;
                        }
                    }
                    if tx
                        .send(Event::Health(
                            config.workspace.clone(),
                            pane,
                            value,
                            stopped.clone(),
                        ))
                        .is_err()
                    {
                        break;
                    }
                    if closed {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
            });
        }
    }
    fn reconnect_saved_apps(&mut self) {
        if !self.app_view_available || self.state.space != Space::Apps {
            return;
        }
        for target in self.state.app_targets.clone() {
            let mut config = self.config.clone();
            config.host = target.host;
            config.directory = Some(target.directory);
            config.desktop_helper = target.helper;
            let _ = self.connect_app(config);
        }
    }
    fn close_audio(&mut self) {
        if let Some(route) = self.audio_route.take() {
            self.effects.push(ServerMessage::Media {
                message: route.playback.close(AUDIO_SESSION, route.epoch),
            });
        }
    }
    fn sync_audio(&mut self) {
        if !self.app_view_available {
            self.close_audio();
            return;
        }
        let desired = self
            .active_app
            .as_ref()
            .and_then(|key| self.app_hosts.get(key).map(|h| (key, h)))
            .filter(|(_, h)| {
                h.config.workspace == self.config.workspace
                    && !h.windows.is_empty()
                    && h.client.connected.load(Ordering::Acquire)
            })
            .and_then(|(key, h)| {
                h.client
                    .pid
                    .lock()
                    .unwrap()
                    .map(|pid| (key.clone(), h.config.clone(), pid))
            });
        let enabled = self.audio_capable && !self.navigation.audio_muted;
        let key = desired.as_ref().map(|v| &v.0);
        if !enabled
            || self
                .audio_route
                .as_ref()
                .is_some_and(|r| Some(&r.key) != key)
        {
            self.close_audio();
        }
        if key.is_none() {
            self.audio_failed = None;
        }
        if enabled
            && self.audio_route.is_none()
            && let Some((key, config, pid)) = desired
            && self.audio_failed.as_ref() != Some(&key)
        {
            match crate::app_audio::Source::start(&config, pid) {
                Ok(source) => {
                    self.audio_epoch = self.audio_epoch.wrapping_add(1);
                    let playback = if self.audio_pcm_v2 {
                        crate::app_audio::Playback::Pcm
                    } else {
                        crate::app_audio::Playback::Legacy
                    };
                    self.effects.push(ServerMessage::Media {
                        message: playback.open(AUDIO_SESSION, self.audio_epoch),
                    });
                    self.audio_route = Some(AudioRoute {
                        key,
                        epoch: self.audio_epoch,
                        source,
                        playback,
                        sent: 0,
                        credits: 0,
                    });
                }
                Err(error) => {
                    self.audio_failed = Some(key);
                    self.note(format!("App audio: {error:#}"));
                }
            }
        }
        let error = self
            .audio_route
            .as_ref()
            .and_then(|r| r.source.errors.try_recv().ok());
        if let Some(error) = error {
            self.audio_failed = self.audio_route.as_ref().map(|r| r.key.clone());
            self.close_audio();
            self.note(error);
        }
    }
    fn connect_app(&mut self, mut config: Config) -> Result<String> {
        ensure!(
            self.app_view_available,
            "App view is unavailable in this presentation"
        );
        config.herdr = None;
        let key = app_key(&config);
        if !self.app_hosts.contains_key(&key) {
            let client = AppClient::connect(&config)?;
            client.send(AppRequest::Resize {
                settings: Settings {
                    width: self.settings.width.clamp(2, crate::app_view::MAX_WIDTH),
                    height: self.settings.height.clamp(2, crate::app_view::MAX_HEIGHT),
                    fps: 60,
                },
            })?;
            self.app_hosts.insert(
                key.clone(),
                AppHost {
                    config,
                    client,
                    windows: vec![],
                    layout: vec![],
                    selected: None,
                },
            );
        }
        self.active_app = Some(key.clone());
        Ok(key)
    }
    fn launch(&mut self, config: Config, pane: usize) {
        self.launch_inner(config, pane, false);
    }
    fn launch_inner(&mut self, config: Config, pane: usize, ensure_shell: bool) {
        if self.desktop.is_none()
            && self.native_terminal_cells
            && (config.workspace != self.config.workspace
                || !self.visible_panes().iter().any(|(id, _)| *id == pane))
        {
            return;
        }
        let attach = if ensure_shell {
            config.attach_argv_with_shell(pane, true)
        } else {
            config.attach_argv(pane)
        };
        if self.desktop.is_none() {
            let key = (config.workspace.clone(), pane);
            if self.terminals.contains_key(&key) {
                return;
            }
            let result = crate::terminal::socket(&config.workspace, pane)
                .and_then(|socket| crate::terminal::Terminal::spawn(&attach, socket));
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
            std::env::var("STARFLUX_KITTY").unwrap_or_else(|_| "kitty".into()),
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
        argv.extend(attach);
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
        if self.cells {
            return self.cell_visible_panes();
        }
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
        if space == Space::Apps && !self.app_view_available {
            return;
        }
        self.release();
        self.state.space = if space == Space::Agent {
            Space::Terminals
        } else {
            space
        };
        if self.config.herdr.is_some() {
            self.ensure_sessions();
        }
        if self.state.space == Space::Apps {
            self.reconnect_saved_apps();
        }
        self.hover = None;
        self.layout();
        self.save();
    }
    fn reopen_project(&mut self, config: Config) -> Result<()> {
        ensure!(
            config.collection == self.config.collection,
            "Project belongs to a different collection"
        );
        let id = config.workspace.clone();
        self.navigation.open_project(&id);
        if !self.projects.iter().any(|p| p.workspace == id) {
            self.projects.push(config);
        }
        self.switch_project(&id)?;
        self.sort_projects(false);
        self.navigation.save(&self.controller_id)?;
        self.note("Project opened / existing sessions retained".into());
        Ok(())
    }
    fn close_project(&mut self, id: &str) -> Result<()> {
        ensure!(
            self.projects.iter().any(|p| p.workspace == id),
            "Project is not open in this window"
        );
        if self.config.workspace == id {
            if let Some(next) = self
                .projects
                .iter()
                .find(|p| p.workspace != id)
                .map(|p| p.workspace.clone())
            {
                self.switch_project(&next)?;
            } else {
                self.navigation.close_project(id);
                self.navigation.save(&self.controller_id)?;
                self.quit = true;
                return Ok(());
            }
        }
        self.navigation.close_project(id);
        self.projects.retain(|p| p.workspace != id);
        self.terminals.retain(|(project, _), _| project != id);
        self.session_monitors.retain(|(project, _), stopped| {
            if project == id {
                stopped.store(true, Ordering::Release);
                false
            } else {
                true
            }
        });
        self.registered.retain(|(project, _)| project != id);
        self.launched.retain(|(project, _), _| project != id);
        self.session_epochs.retain(|(project, _), _| project != id);
        self.health.retain(|(project, _), _| project != id);
        self.sort_projects(false);
        self.navigation.save(&self.controller_id)?;
        self.save();
        self.note("Project closed in this window / terminals and agents keep running".into());
        Ok(())
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
        let mut state = self
            .states
            .remove(id)
            .unwrap_or(ProjectState::load_view(&config, &self.controller_id)?);
        state.sync_shared(&config)?;
        let now = crate::navigation::now();
        self.navigation
            .presentation_changed(&self.config.workspace, now);
        self.navigation.presentation_changed(id, now);
        self.release();
        self.save();
        self.states
            .insert(self.config.workspace.clone(), self.state.clone());
        self.active_app = None;
        self.config = config;
        self.state = state;
        if !self.app_view_available && self.state.space == Space::Apps {
            self.state.space = Space::Terminals;
        }
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
                        std::env::var("STARFLUX_KITTEN").unwrap_or_else(|_| "kitten".into()),
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
        if self.cells {
            return self.cell_pane_at(x as u16, y as u16);
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
        let result = self.drop_message_inner(text);
        if result.is_err() {
            #[cfg(feature = "lossless-canvas")]
            {
                self.remote_drop = None;
            }
            self.receiving = None;
            self.receiving_image = None;
            self.drop_target = None;
            self.drop_stage = None;
            self.hover = None;
            self.osc("t=r:o=0:i=1", None);
        }
        result
    }
    fn drop_message_inner(&mut self, text: &str) -> Result<()> {
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
        #[cfg(feature = "lossless-canvas")]
        if self.remote_drop.is_some() && fields.get("t").copied() == Some("r") {
            use starkit::terminal_graphics::drop_receive::Progress;
            match self.remote_drop.as_mut().unwrap().receive(text) {
                Ok(Progress::Chunk) => {}
                Ok(Progress::Next(request)) => self.osc(&request, None),
                Ok(Progress::Complete) => {
                    let incoming = self.remote_drop.take().unwrap();
                    let (stage, paths) = incoming.into_files();
                    let pane = self
                        .receiving
                        .take()
                        .context("Drop destination was lost")?
                        .0;
                    self.drop_stage = Some(stage);
                    self.osc("t=r:o=1:i=1", None);
                    let destination = self.drop_target.take();
                    self.deliver_to(pane, paths, destination);
                }
                Err(error) => {
                    self.remote_drop = None;
                    self.receiving = None;
                    self.drop_target = None;
                    self.osc("t=r:o=0:i=1", None);
                    return Err(error);
                }
            }
            return Ok(());
        }
        match fields.get("t").copied() {
            Some("q") => {
                self.drop_enabled = true;
                self.osc(
                    "t=a:i=1",
                    Some("text/uri-list image/png image/jpeg image/webp image/gif".into()),
                );
                #[cfg(feature = "lossless-canvas")]
                if let Some(identity) = starkit::terminal_graphics::drop_receive::machine_identity()
                {
                    self.osc("t=a:x=1:i=1", Some(identity));
                }
                self.note("Desktop file drops ready · copy to the target session".into());
            }
            Some("m") => {
                if !payload.is_empty() {
                    self.offered = payload.split_whitespace().map(str::to_owned).collect();
                }
                let pane = self.drop_pane(n("x"), n("y"));
                let mime = drop_mime(&self.offered);
                let accepts = mime.is_some() && n("o") & 1 != 0;
                let hover = if accepts { pane } else { None };
                if self.hover != hover {
                    self.hover = hover;
                    self.osc(
                        if hover.is_some() {
                            "t=m:o=1:i=1"
                        } else {
                            "t=m:o=0:i=1"
                        },
                        hover.and_then(|_| mime.map(|(index, _)| self.offered[index].clone())),
                    );
                }
            }
            Some("M") => {
                ensure!(self.transfer.is_none(), "A file transfer is already active");
                if let Some(pane) = self.drop_pane(n("x"), n("y")) {
                    if !payload.is_empty() {
                        self.offered = payload.split_whitespace().map(str::to_owned).collect();
                    }
                    if let Some((index, image)) = drop_mime(&self.offered) {
                        ensure!(
                            self.receiving.is_none(),
                            "A desktop drop is already being received"
                        );
                        self.receiving = Some((pane, vec![]));
                        self.receiving_remote = false;
                        self.receiving_image = image;
                        self.receiving_mime = (index + 1) as u64;
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
                    self.receiving_remote |= n("X") == 1;
                    let chunk = STANDARD.decode(payload).or_else(|_| {
                        base64::engine::general_purpose::STANDARD_NO_PAD.decode(payload)
                    })?;
                    ensure!(
                        data.len() + chunk.len()
                            <= if self.receiving_image.is_some() {
                                32 * 1024 * 1024
                            } else {
                                1024 * 1024
                            },
                        "Drop data exceeds limit"
                    );
                    data.extend(chunk);
                    if payload.is_empty() && n("m") <= 0 {
                        let (pane, data) = self.receiving.take().unwrap();
                        if let Some(extension) = self.receiving_image.take() {
                            let stage = tempfile::Builder::new()
                                .prefix("starflux-image-")
                                .tempdir()?;
                            let path = stage.path().join(format!("dropped-image.{extension}"));
                            fs::write(&path, data)?;
                            self.drop_stage = Some(stage);
                            self.osc("t=r:o=1:i=1", None);
                            let destination = self.drop_target.take();
                            self.deliver_to(pane, vec![path], destination);
                            return Ok(());
                        }
                        let text = String::from_utf8(data)?;
                        // Browser image drags may offer a web URL and image bytes.
                        // Keep file URI lists first so multi-file drops retain every item.
                        if text
                            .lines()
                            .filter(|s| !s.is_empty() && !s.starts_with('#'))
                            .any(|s| !s.starts_with("file://"))
                            && let Some((index, image)) = drop_image_mime(&self.offered)
                        {
                            self.receiving = Some((pane, vec![]));
                            self.receiving_image = image;
                            self.receiving_remote = false;
                            self.receiving_mime = (index + 1) as u64;
                            self.osc(&format!("t=r:x={}:i=1", index + 1), None);
                            return Ok(());
                        }
                        if self.receiving_remote {
                            #[cfg(feature = "lossless-canvas")]
                            {
                                let incoming =
                                    starkit::terminal_graphics::drop_receive::Incoming::new(
                                        &text,
                                        self.receiving_mime,
                                    )?;
                                self.osc(&incoming.request(), None);
                                self.remote_drop = Some(incoming);
                                self.receiving = Some((pane, vec![]));
                                self.note("Receiving dropped files from your computer…".into());
                                return Ok(());
                            }
                            #[cfg(not(feature = "lossless-canvas"))]
                            anyhow::bail!(
                                "Remote file drops require the current STAR/KIT candidate"
                            );
                        }
                        let paths = uri_paths(&text)?;
                        self.osc("t=r:o=1:i=1", None);
                        let destination = self.drop_target.take();
                        self.deliver_to(pane, paths, destination);
                    }
                }
            }
            Some("R") | Some("E") => {
                #[cfg(feature = "lossless-canvas")]
                {
                    self.remote_drop = None;
                }
                self.receiving = None;
                self.drop_target = None;
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
        let drop_stage = self.drop_stage.take();
        std::thread::spawn(move || {
            let _drop_stage = drop_stage;
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
        if text.is_empty() {
            return;
        }
        self.navigation
            .touch(&self.config.workspace, crate::navigation::now());
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
            if !self.app_pointer_modifiers.forward(&input) {
                return;
            }
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
        if matches!(
            &kind,
            EmbeddedKind::Key {
                phase: KeyPhase::Press | KeyPhase::Repeat,
                ..
            } | EmbeddedKind::Text { .. }
        ) {
            self.navigation
                .touch(&self.config.workspace, crate::navigation::now());
        }
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
                            self.state.select_terminal(*pane);
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
                        self.state.select_terminal(pane);
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
                self.state.select_terminal(*pane);
            }
        }
        let pane = if let EmbeddedKind::Pointer {
            action, position, ..
        } = &kind
            && action.starts_with("scroll_")
        {
            self.visible_panes()
                .into_iter()
                .find(|(_, rect)| {
                    position[0] >= f64::from(rect.x)
                        && position[0] < f64::from(rect.x + rect.width)
                        && position[1] >= f64::from(rect.y)
                        && position[1] < f64::from(rect.y + rect.height)
                })
                .map(|(pane, _)| pane)
                .unwrap_or_else(|| self.config.canonical_pane(self.state.selected()))
        } else {
            self.config.canonical_pane(self.state.selected())
        };
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
            Prompt::Deliver => vec![field("File path on this workspace host", String::new())],
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
                    "S T A R / F L U X helper on SSH host",
                    self.config.desktop_helper.clone(),
                ),
            ],
            Prompt::RunCommand => vec![
                field("Project app command", self.state.app_command.clone()),
                field("Project directory", directory),
            ],
            Prompt::Project => vec![
                field("Project name (new or saved)", String::new()),
                field("Project host / local or an SSH alias", "local".into()),
                field("Project directory on that host", directory),
            ],
        };
        let catalog = matches!(kind, Prompt::Launch).then(|| Catalog {
            source: String::new(),
            changed: Instant::now(),
            requested: false,
            apps: vec![],
            query: TextInput::single().with_max_chars(256),
            selected: 0,
            offset: 0,
            focused: true,
            visible: true,
            status: "Loading system apps…".into(),
        });
        self.modal = Some(Modal {
            kind,
            catalog,
            fields,
            selected: 0,
            error: String::new(),
        });
    }
    fn refresh_applications(&mut self) {
        let Some(modal) = self.modal.as_mut() else {
            return;
        };
        let Some(catalog) = modal.catalog.as_mut() else {
            return;
        };
        let host = modal.fields[2].1.text().trim().to_owned();
        let helper = modal.fields[3].1.text().trim().to_owned();
        let source = format!("{host}\n{helper}");
        if source != catalog.source {
            if catalog
                .apps
                .iter()
                .any(|a| a.command == modal.fields[0].1.text())
            {
                modal.fields[0].1 = TextInput::single().with_max_chars(2048);
            }
            catalog.source = source.clone();
            catalog.changed = Instant::now();
            catalog.requested = false;
            catalog.apps.clear();
            catalog.selected = 0;
            catalog.offset = 0;
            catalog.status = format!("Loading apps / {host}…");
        }
        if catalog.requested || catalog.changed.elapsed() < Duration::from_millis(350) {
            return;
        }
        catalog.requested = true;
        let token = catalog.changed;
        let mut config = self.config.clone();
        config.host = if host.is_empty() || host == "local" {
            None
        } else {
            Some(host)
        };
        config.desktop_helper = helper;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = config
                .validate()
                .and_then(|_| crate::applications::on_host(&config))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Applications(source, token, result));
        });
    }
    fn choose_application(&mut self, index: usize) {
        let Some(modal) = self.modal.as_mut() else {
            return;
        };
        let Some(catalog) = modal.catalog.as_mut() else {
            return;
        };
        let matches = catalog.matches();
        catalog.selected = index.min(matches.len().saturating_sub(1));
        if let Some(app) = matches
            .get(catalog.selected)
            .and_then(|i| catalog.apps.get(*i))
        {
            modal.fields[0].1 = TextInput::single()
                .with_max_chars(2048)
                .with_text(app.command.clone());
        } else {
            modal.fields[0].1 = TextInput::single().with_max_chars(2048);
        }
        let rows = crate::shelf::application_rows(self.viewport.height as u16);
        if catalog.selected < catalog.offset {
            catalog.offset = catalog.selected;
        }
        if catalog.selected >= catalog.offset + rows {
            catalog.offset = catalog.selected + 1 - rows;
        }
    }
    fn launch_app(&mut self, config: Config, command: String, directory: String) -> Result<()> {
        ensure!(
            self.app_view_available,
            "App view is unavailable in this presentation"
        );
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
                Prompt::Deliver => {
                    let path = PathBuf::from(&values[0]);
                    ensure!(
                        path.is_file(),
                        "Enter an existing regular file on the workspace host"
                    );
                    self.deliver(self.state.selected(), vec![path]);
                }
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
                        self.state.terminal_ids().contains(id),
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
                    let config_path = crate::runtime::config_path(&values[0])?;
                    if config_path.exists()
                        || project_state::data_root()?
                            .join(format!("{}.json", values[0]))
                            .exists()
                    {
                        return self.reopen_project(crate::saved(&values[0])?);
                    }
                    let config = Config {
                        ssh_config: self.config.ssh_config.clone(),
                        workspace: values[0].clone(),
                        label: None,
                        collection: self.config.collection.clone(),
                        herdr: None,
                        adopted: Default::default(),
                        theme: self.config.theme.clone(),
                        host: if values[1] == "local" || values[1].is_empty() {
                            None
                        } else {
                            Some(values[1].clone())
                        },
                        directory: Some(values[2].clone()),
                        agent: None,
                        executable: std::env::current_exe()?.display().to_string(),
                        project_host: true,
                        desktop_helper: "starflux".into(),
                    };
                    config.validate()?;
                    ensure!(!values[2].is_empty(), "Project directory is required");
                    ensure!(
                        !self
                            .projects
                            .iter()
                            .any(|c| c.workspace == config.workspace),
                        "A project with that name already exists"
                    );
                    let state = ProjectState {
                        app_command: String::new(),
                        ..ProjectState::default()
                    };
                    state.save(&config)?;
                    starkit::fs::write_private(
                        &crate::runtime::config_path(&config.workspace)?,
                        &serde_json::to_vec_pretty(&config)?,
                    )?;
                    let id = config.workspace.clone();
                    self.navigation.open_project(&id);
                    self.navigation.touch(&id, crate::navigation::now());
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
        if let Some(catalog) = self
            .modal
            .as_ref()
            .and_then(|m| m.catalog.as_ref())
            .filter(|c| c.visible && c.focused)
        {
            if code == "up" || code == "down" || code == "pageup" || code == "pagedown" {
                let step = if code.starts_with("page") {
                    crate::shelf::application_rows(self.viewport.height as u16)
                } else {
                    1
                };
                let index = if code == "up" || code == "pageup" {
                    catalog.selected.saturating_sub(step)
                } else {
                    catalog.selected + step
                };
                self.choose_application(index);
                return Ok(());
            }
            if code == "enter" {
                return self.accept_prompt();
            }
            if code == "backtab" {
                let modal = self.modal.as_mut().unwrap();
                modal.selected = modal.fields.len() - 1;
                let catalog = modal.catalog.as_mut().unwrap();
                catalog.focused = false;
                if self.viewport.width < 700 {
                    catalog.visible = false;
                }
                return Ok(());
            }
            if code == "tab" {
                let modal = self.modal.as_mut().unwrap();
                modal.selected = if self.viewport.width >= 700 { 1 } else { 0 };
                modal.catalog.as_mut().unwrap().focused = false;
                if self.viewport.width < 700 {
                    modal.catalog.as_mut().unwrap().visible = false;
                }
                return Ok(());
            }
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
            if modal.selected + 1 == modal.fields.len()
                && let Some(catalog) = modal.catalog.as_mut()
            {
                catalog.focused = true;
                catalog.visible = true;
                return Ok(());
            }
            modal.selected = (modal.selected + 1) % modal.fields.len();
            return Ok(());
        }
        if code == "backtab" {
            if (modal.selected == 0
                || (modal.selected == 1 && modal.catalog.as_ref().is_some_and(|c| c.visible)))
                && let Some(catalog) = modal.catalog.as_mut()
            {
                catalog.focused = true;
                catalog.visible = true;
                return Ok(());
            }
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
        if let Some(catalog) = modal.catalog.as_mut().filter(|c| c.visible && c.focused) {
            catalog.query.handle(KeyEvent::new(key, mods));
            self.choose_application(0);
        } else {
            modal.fields[modal.selected]
                .1
                .handle(KeyEvent::new(key, mods));
        }
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
            .or_else(|| self.state.agent_labels.get(&id).cloned())
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
                    .terminal_ids()
                    .iter()
                    .position(|i| *i == id)
                    .unwrap_or(0);
                if n == 0 {
                    "Terminal 1".into()
                } else {
                    format!("Terminal {}", n + 1)
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
        let (title, subtitle, items) = if let Some(id) =
            target.and_then(|t| t.strip_prefix("project:"))
        {
            let Some(project) = self.projects.iter().find(|p| p.workspace == id) else {
                return;
            };
            (
                project.label.as_deref().unwrap_or(id).to_owned(),
                format!("PROJECT / {}", project.host.as_deref().unwrap_or("local")),
                vec![
                    item("Rename project…", "", format!("rename-project:{id}")),
                    item(
                        "Open terminals",
                        "",
                        format!("project-space:terminals:{id}"),
                    ),
                    item("Add project…", "+", "project:new".into()),
                    item("Close project", "", format!("close-project:{id}")),
                ],
            )
        } else if let Some(id) = target
            .and_then(|t| t.strip_prefix("terminal:"))
            .and_then(|id| id.parse::<usize>().ok())
        {
            if !self.state.terminal_ids().contains(&id) {
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
                .chain(
                    self.health
                        .get(&(
                            self.config.workspace.clone(),
                            self.config.canonical_pane(id),
                        ))
                        .filter(|v| v["alive"] == false)
                        .filter(|_| {
                            !self
                                .config
                                .herdr
                                .as_ref()
                                .is_some_and(|h| h.panes.contains_key(&id))
                        })
                        .map(|_| item("Reopen in a shell", "", format!("restart-terminal:{id}"))),
                )
                .collect(),
            )
        } else {
            self.context_menu = None;
            return;
        };
        self.release();
        let mut items = items;
        if self.app_view_available
            && let Some(id) = target.and_then(|t| t.strip_prefix("project:"))
        {
            items.insert(
                2,
                item("Open App Space", "", format!("project-space:apps:{id}")),
            );
        }
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
    fn refresh_ui(&mut self) -> Result<()> {
        ensure!(
            self.transfer.is_none() && self.receiving.is_none(),
            "Finish or cancel the file transfer before refreshing the UI"
        );
        self.release();
        self.save();
        self.navigation.save(&self.controller_id)?;
        let selected = self
            .active_app
            .as_ref()
            .and_then(|key| self.app_hosts.get(key))
            .and_then(|h| h.selected);
        crate::refresh::request(
            &self.controller_id,
            &crate::refresh::View {
                project: self.config.workspace.clone(),
                app: self.active_app.clone(),
                window: selected,
            },
        )?;
        self.effects.push(ServerMessage::Error {
            message: crate::refresh::SIGNAL.into(),
        });
        self.quit = true;
        Ok(())
    }
    fn action(&mut self, action: &str) -> Result<()> {
        let _transaction = if matches!(action, "terminal:new" | "terminal:split" | "agent:new") {
            Some(crate::project_sync::transaction(&self.config)?)
        } else {
            None
        };
        self.sync_project()?;
        self.action_inner(action)
    }
    fn sync_project(&mut self) -> Result<()> {
        if self.state.sync_shared(&self.config)? {
            let valid = self.state.terminal_ids();
            let removed = self
                .registered
                .iter()
                .filter(|(p, id)| {
                    p == &self.config.workspace
                        && (!valid.contains(id)
                            || self.session_epochs.get(&(p.clone(), *id))
                                != Some(&self.state.terminal_epochs.get(id).cloned()))
                })
                .cloned()
                .collect::<Vec<_>>();
            for key in removed {
                if let Some(token) = self.session_monitors.remove(&key) {
                    token.store(true, Ordering::Release)
                }
                self.session_epochs.remove(&key);
                self.registered.remove(&key);
                self.launched.remove(&key);
                self.health.remove(&key);
                self.terminals.remove(&key);
            }
            self.ensure_sessions();
            let selected_app = self.active_app.clone();
            self.reconnect_saved_apps();
            if selected_app.is_some() {
                self.active_app = selected_app;
            }
            self.layout();
        }
        Ok(())
    }
    fn action_inner(&mut self, action: &str) -> Result<()> {
        if action == "presentation:require-graphical" {
            self.require_graphical();
            return Ok(());
        }
        ensure!(
            self.app_view_available || !crate::features::app_action(action),
            "App view is unavailable in this presentation"
        );
        if action == "ui:refresh" {
            return self.refresh_ui();
        }
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
        if action == "modal:catalog" {
            if let Some(catalog) = self.modal.as_mut().and_then(|m| m.catalog.as_mut()) {
                catalog.visible = true;
                catalog.focused = true;
            }
            return Ok(());
        }
        if action == "modal:details" {
            if let Some(catalog) = self.modal.as_mut().and_then(|m| m.catalog.as_mut()) {
                catalog.visible = false;
                catalog.focused = false;
            }
            return Ok(());
        }
        if let Some(index) = action
            .strip_prefix("modal:application:")
            .and_then(|s| s.parse::<usize>().ok())
        {
            self.choose_application(index);
            return Ok(());
        }
        if action == "modal:search" {
            if let Some(catalog) = self.modal.as_mut().and_then(|m| m.catalog.as_mut()) {
                catalog.focused = true;
            }
            return Ok(());
        }
        if action == "modal:apps:up" || action == "modal:apps:down" {
            if let Some(catalog) = self
                .modal
                .as_ref()
                .and_then(|m| m.catalog.as_ref())
                .filter(|c| c.visible)
            {
                let index = if action.ends_with(":up") {
                    catalog.selected.saturating_sub(1)
                } else {
                    catalog.selected + 1
                };
                self.choose_application(index);
            }
            return Ok(());
        }
        if action == "modal:accept" {
            return self.accept_prompt();
        }
        if let Some(index) = action.strip_prefix("modal:field:") {
            if let (Some(modal), Ok(index)) = (&mut self.modal, index.parse::<usize>()) {
                modal.selected = index.min(modal.fields.len() - 1);
                if let Some(catalog) = modal.catalog.as_mut() {
                    catalog.focused = false;
                }
            }
            return Ok(());
        }
        if self.modal.is_some() {
            return Ok(());
        }
        if let Some(id) = action.strip_prefix("close-project:") {
            return self.close_project(id);
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
                self.state.terminal_ids().contains(&id),
                "Terminal is not in this project"
            );
            self.state.select_terminal(id);
            return self.action("terminal:close-pane");
        }
        if let Some(id) = action.strip_prefix("restart-terminal:") {
            self.action(&format!("terminal:{id}"))?;
            return self.action("terminal:restart");
        }
        match action {
            "audio:toggle" | "audio:mute" | "audio:unmute" => {
                if !self.audio_capable && action != "audio:mute" {
                    self.note("Audio needs a local player. Install the STAR/KIT Kitty bridge on the listening machine, or launch starflux --ssh HOST --workspace NAME locally. Restarting only the server frontend cannot enable local audio.".into());
                    return Ok(());
                }
                self.navigation.audio_muted = if action == "audio:toggle" {
                    !self.navigation.audio_muted
                } else {
                    action == "audio:mute"
                };
                self.audio_failed = None;
                self.sync_audio();
                self.navigation.save(&self.controller_id)?;
                if !self.audio_capable && !self.navigation.audio_muted {
                    self.note("Local audio requires an audio-enabled STAR/KIT frontend. From your listening machine run starflux --ssh HOST --workspace NAME, or use the Kitty STAR/KIT bridge".into());
                }
            }
            "projects:sort:mode"
            | "projects:sort:direction"
            | "projects:sort:manual"
            | "projects:sort:activity" => {
                self.project_drag = None;
                if action == "projects:sort:manual" {
                    self.navigation.recent = false;
                } else if action == "projects:sort:activity" {
                    self.navigation.recent = true;
                } else if action == "projects:sort:mode" {
                    self.navigation.recent = !self.navigation.recent;
                } else {
                    self.navigation.high_first = !self.navigation.high_first;
                    self.navigation.recent = true;
                }
                self.sort_projects(true);
                self.navigation.save(&self.controller_id)?;
                self.navigation_saved_at = Instant::now();
            }
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
            "project:close" => self.close_project(&self.config.workspace.clone())?,
            "file:deliver" => self.open_prompt(Prompt::Deliver),
            "terminal:font:increase" | "terminal:font:decrease" | "terminal:font:reset" => {
                ensure!(
                    !self.cells,
                    "Cells mode inherits the parent terminal font; change it in your terminal"
                );
                if self.desktop.is_none() {
                    let current = self.native_font_points.context("Use Kitty's font-size shortcuts, or connect through the updated Kitty bridge for header controls")?;
                    let requested = match action {
                        "terminal:font:increase" => (current + 1000).min(32000),
                        "terminal:font:decrease" => current.saturating_sub(1000).max(6000),
                        _ => 0,
                    };
                    self.native_font_points = if requested == 0 {
                        self.initial_native_font_points
                    } else {
                        Some(requested)
                    };
                    self.effects.push(ServerMessage::TerminalFont {
                        size_millipoints: requested,
                    });
                    return Ok(());
                }
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
            "agent:previous" | "agent:next" | "terminal:previous" | "terminal:next" => {
                let ids = self.state.terminal_ids();
                if ids.is_empty() {
                    return Ok(());
                }
                let n = ids
                    .iter()
                    .position(|id| *id == self.state.terminal)
                    .unwrap_or(0);
                let next = if action.ends_with(":next") {
                    (n + 1) % ids.len()
                } else {
                    (n + ids.len() - 1) % ids.len()
                };
                self.state.select_terminal(ids[next]);
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
                self.state.toggle_split()?;
                self.ensure_sessions();
                self.layout();
                self.save()
            }
            "terminal:close-pane" => {
                if self.state.split {
                    self.release();
                    if let Some(other) = self
                        .state
                        .visible()
                        .iter()
                        .find(|id| **id != self.state.terminal)
                    {
                        self.state.select_terminal(*other);
                    }
                    self.state.split = false;
                    self.state.split_panes = None;
                    self.layout();
                    self.save();
                    self.note("Pane closed / terminal still available in its tab".into());
                }
            }
            "launch-app" => {
                if self.state.space != Space::Apps {
                    self.set_space(Space::Apps);
                }
                self.open_prompt(Prompt::Launch);
            }
            "run-app" => {
                if self.state.space != Space::Apps {
                    self.set_space(Space::Apps);
                }
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
            "agent:attach" | "terminal:restart" => {
                let pane = self.state.terminal;
                ensure!(
                    !self
                        .config
                        .herdr
                        .as_ref()
                        .is_some_and(|h| h.panes.contains_key(&pane)),
                    "Imported terminals remain managed by Herdr"
                );
                let config = self.config.clone();
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let result = config.call(pane, &config.request(pane, "restart"));
                    let _ = tx.send(match result {
                        Ok(_) => Event::Status("Terminal reopened in a shell".into()),
                        Err(error) => Event::Error(format!("Terminal: {error:#}")),
                    });
                });
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
                    self.state.select_terminal(id);
                    self.set_space(Space::Agent);
                } else if let Some(id) = action.strip_prefix("terminal:") {
                    let id = id.parse::<usize>()?;
                    ensure!(
                        self.state.terminal_ids().contains(&id),
                        "Terminal is not in this project"
                    );
                    self.state.select_terminal(id);
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
        if self.cells {
            return self.draw_cells(v);
        }
        self.viewport = v;
        let layout = crate::shelf::Layout::new(v.width as u16, v.height as u16, self.state.space);
        let content = layout.canvas;
        let direct = self.desktop.is_none() && self.state.space != Space::Apps;
        let width = if self.state.space != Space::Apps {
            u32::from(content.width)
        } else {
            u32::from(content.width).max(2)
        };
        let height = if self.state.space != Space::Apps {
            u32::from(content.height)
        } else {
            u32::from(content.height).max(2)
        };
        let fps = if self.state.space == Space::Apps {
            60
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
            self.config.canonical_pane(self.state.terminal),
        ));
        let agent_name = health.and_then(|v| v["agent"].as_str()).unwrap_or("");
        let raw_agent_state = health
            .and_then(|v| v["state"].as_str())
            .unwrap_or("connecting");
        let agent_state = if matches!(
            raw_agent_state,
            "unknown" | "blocked" | "waiting" | "waiting_for_input"
        ) {
            if health.is_some_and(|v| v["alive"] == true) {
                "running"
            } else {
                "stopped"
            }
        } else if matches!(
            raw_agent_state,
            "needs_input" | "question_pending" | "waiting_for_answer"
        ) {
            "question pending"
        } else if matches!(
            raw_agent_state,
            "waiting_for_approval" | "needs_approval" | "needs_permission"
        ) {
            "approval requested"
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
                running: self.project_activity(c).running,
                questions: self.project_activity(c).questions,
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
                    selected: active_app.as_ref() == Some(key) && h.selected == Some(w.id),
                })
            })
            .collect();
        let modal = self.modal.as_ref().map(|m| crate::shelf::Modal {
            catalog_available: m.catalog.is_some(),
            applications: m.catalog.as_ref().filter(|c| c.visible).map(|c| {
                crate::shelf::ApplicationPicker {
                    query: c.query.text(),
                    cursor: c.query.cursor(),
                    focused: c.focused,
                    status: &c.status,
                    selection_name: c
                        .matches()
                        .get(c.selected)
                        .and_then(|i| c.apps.get(*i))
                        .filter(|a| a.command == m.fields[0].1.text())
                        .map(|a| a.name.as_str()),
                    selected: c.selected,
                    count: c.matches().len(),
                    rows: c
                        .matches()
                        .into_iter()
                        .enumerate()
                        .skip(c.offset)
                        .take(crate::shelf::application_rows(v.height as u16))
                        .map(|(row, index)| {
                            (
                                row,
                                c.apps[index].name.as_str(),
                                c.apps[index].executable.as_str(),
                            )
                        })
                        .collect(),
                }
            }),
            title: match &m.kind {
                Prompt::Deliver => "Deliver a file",
                Prompt::RenameProject(_) => "Rename project",
                Prompt::RenameTab(_) => "Rename terminal tab",
                Prompt::RenameAgent(_) => "Rename agent",
                Prompt::Project => "Add / reopen project",
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
        let model = crate::shelf::Model {
            app_view: self.app_view_available,
            project: &self.config.workspace,
            project_label: self
                .config
                .label
                .as_deref()
                .unwrap_or(&self.config.workspace),
            project_offset: self.project_offset,
            project_total: self.projects.len(),
            sort_hover: self.sort_hover,
            sort_mode_hover: self.sort_mode_hover,
            project_hover: self
                .project_hover
                .as_deref()
                .filter(|_| self.modal.is_none() && self.context_menu.is_none() && !self.git_open),
            project_manual: !self.navigation.recent,
            project_drag: self
                .project_drag
                .as_ref()
                .filter(|d| d.moved)
                .map(|d| (d.id.as_str(), d.gap)),
            project_sort_high: self.navigation.high_first,
            wave_phase: self.wave_phase,
            // Streaming apps should not compete with decorative sidebar frames
            // for frontend rasterization/presentation on the same connection.
            reduced_motion: self.reduced_motion || self.state.space == Space::Apps,
            directory: &directory,
            host,
            theme: &self.appearance.name,
            audio_label: if !self.audio_capable {
                "Local audio missing"
            } else if self.navigation.audio_muted {
                "Muted"
            } else if self.audio_failed.is_some() {
                "Audio unavailable"
            } else {
                "Audio on"
            },
            space: self.state.space,
            projects,
            terminals: self
                .state
                .terminal_ids()
                .iter()
                .map(|id| {
                    let label = self.terminal_label(*id);
                    let profile = self
                        .health
                        .get(&(
                            self.config.workspace.clone(),
                            self.config.canonical_pane(*id),
                        ))
                        .and_then(|v| v["agent"].as_str());
                    (
                        *id,
                        profile.map_or(label.clone(), |p| format!("{label} · {p}")),
                    )
                })
                .collect(),
            terminal_status: self
                .state
                .terminal_ids()
                .into_iter()
                .filter_map(|id| {
                    self.health
                        .get(&(
                            self.config.workspace.clone(),
                            self.config.canonical_pane(id),
                        ))
                        .filter(|v| v["agent"].as_str().is_some() || v["alive"] == false)
                        .map(|v| {
                            (
                                id,
                                if v["alive"] == false {
                                    "stopped"
                                } else {
                                    v["state"].as_str().unwrap_or("idle")
                                }
                                .to_owned(),
                            )
                        })
                })
                .collect(),
            terminal: self.state.terminal,
            terminal_font_size: self.font_label(),
            split: self.state.split,
            agent_name,
            agent_state,
            status: &self.status,
            apps,
            app_desktop: self
                .active_app
                .as_ref()
                .and_then(|key| self.app_hosts.get(key))
                .map(|h| crate::shelf::DesktopMap {
                    size: h
                        .client
                        .latest
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map_or([self.settings.width, self.settings.height], |f| {
                            [f.width, f.height]
                        }),
                    windows: &h.layout,
                    selected: h.windows.iter().position(|w| Some(w.id) == h.selected),
                }),
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
        };
        let cache_key = ((direct && self.native_terminal_cells) || self.state.space == Space::Apps)
            .then(|| {
                serde_json::to_vec(&(v, &self.appearance, &model, self.visible_panes()))
                    .expect("Serialize graphical presentation")
            });
        if let Some(scene) = cache_key
            .as_ref()
            .and_then(|key| self.graphical_cache.get(key))
        {
            return scene;
        }
        let (chrome, foreground) = crate::shelf::surfaces(layout, &self.appearance, &model);
        drop(model);

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
        // All graphical text lives in surfaces. Empty-buffer spans otherwise
        // repaint the default grid background beneath a cropped live canvas.
        scene.spans.clear();
        let terminal_rects = if direct && self.native_terminal_cells {
            self.visible_panes()
                .iter()
                .map(|(_, pane)| native_terminal_rect(*pane, content, v).0)
                .collect::<Vec<_>>()
        } else if self.state.space == Space::Apps && connected {
            vec![
                native_terminal_rect(
                    PixelRect::new(0, 0, content.width, content.height),
                    content,
                    v,
                )
                .0,
            ]
        } else {
            vec![]
        };
        scene.components.extend(crate::graphical::chrome(
            chrome,
            v,
            &terminal_rects,
            layout.sidebar.width,
        ));
        let source_size = if self.state.space == Space::Apps {
            self.active_app
                .as_ref()
                .and_then(|key| self.app_hosts.get(key))
                .and_then(|host| {
                    host.client
                        .latest
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|frame| [frame.width, frame.height])
                })
                .unwrap_or([self.settings.width, self.settings.height])
        } else {
            [self.settings.width, self.settings.height]
        };
        let view = if self.state.space == Space::Apps {
            crate::app_view::view(source_size, content)
        } else {
            ContentView::new(source_size, content).unwrap()
        };
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
                interactive: self.modal.is_none() && self.context_menu.is_none() && !self.git_open,
            });
        }
        if direct && self.native_terminal_cells {
            for (_, pane) in self.visible_panes() {
                let (rect, _) = native_terminal_rect(pane, content, v);
                scene.components.push(Component::Terminal { rect });
            }
        }
        // Empty transparent overlays still cost a full-window alpha composite
        // in the native frontend. Add the layer only while an overlay exists.
        if !foreground.nodes.is_empty() {
            scene
                .components
                .push(crate::graphical::foreground(foreground, v));
        }
        if let Some(key) = cache_key {
            self.graphical_cache.store(key, &scene);
        }
        scene
    }
}
impl Controller for Workspace {
    fn capabilities(
        &mut self,
        capabilities: starkit::terminal_graphics::capabilities::Capabilities,
    ) {
        self.close_audio();
        self.cells = capabilities.cell_presentation.unwrap_or(false);
        self.app_view_available = crate::features::app_view(&capabilities);
        if !self.app_view_available && self.state.space == Space::Apps {
            self.state.space = Space::Terminals;
        }
        self.check_presentation();
        let legacy = !self.cells && !crate::terminal::direct();
        if legacy != self.desktop.is_some() {
            self.release();
            self.desktop = legacy.then(|| Desktop::start(self.settings.clone()));
            self.terminals.clear();
            for stopped in self.session_monitors.values() {
                stopped.store(true, Ordering::Release);
            }
            self.session_monitors.clear();
            self.registered.clear();
            self.launched.clear();
            self.ensure_sessions();
        }
        {
            self.native_terminal_cells = capabilities.native_terminal_cells;
            self.native_font_points = capabilities.native_font_millipoints;
            self.initial_native_font_points = capabilities.native_font_millipoints;
            self.native_layout_key.clear();
            for terminal in self.terminals.values_mut() {
                terminal.reset_native();
            }
            if self.desktop.is_none() && !self.native_terminal_cells {
                self.note(
                    "Update the local STAR/KIT terminal frontend to enable native Kitty terminals"
                        .into(),
                );
            }
        }
        self.pointer_hiding = capabilities.pointer_hiding;
        self.cursor_policy_sent = None;
        self.audio_capable = capabilities.audio_relay || capabilities.audio_pcm_v2;
        self.audio_pcm_v2 = capabilities.audio_pcm_v2;
        self.video.stop();
        self.video_failed = false;
        self.video_capable = !self.cells
            && self.app_view_available
            && capabilities.canvas_video
            && !capabilities.local_frames
            && capabilities.streamed_frames;
        self.video_high_quality = serde_json::to_value(capabilities)
            .ok()
            .is_some_and(|v| v["canvas_video_high_quality"] == true);
        self.video.set_high_quality(self.video_high_quality);
        self.video.set_pipelined(
            cfg!(feature = "lossless-canvas")
                && serde_json::to_value(capabilities)
                    .ok()
                    .is_some_and(|v| v["canvas_video_pipelined"] == true),
        );
        self.terminal_transport = if self.cells {
            "parent-terminal-cells"
        } else if self.desktop.is_none() {
            "native-kitty-cells"
        } else if capabilities.local_frames {
            "mapped-rgba"
        } else if serde_json::to_value(capabilities)
            .ok()
            .is_some_and(|v| v["lossless_frames"] == true)
        {
            "lossless-png-delta"
        } else {
            "jpeg-compatibility"
        };
        self.audio_failed = None;
        if std::env::var_os("STAR_GRAPHICS_FONT_DEBUG").is_some() {
            eprintln!(
                "Controller font report: {}",
                serde_json::to_value(capabilities).unwrap()
            );
        }
        self.reconnect_saved_apps();
        if let Some(view) = crate::refresh::take_view(&self.controller_id)
            && view.project == self.config.workspace
            && let Some(key) = view.app
            && let Some(host) = self.app_hosts.get_mut(&key)
        {
            host.selected = view.window;
            self.active_app = Some(key);
        }
        self.typography = serde_json::to_value(capabilities)
            .ok()
            .and_then(|v| serde_json::from_value(v["terminal_typography"].clone()).ok())
            .filter(crate::terminal::FontMetrics::valid);
        self.refresh_kitty_fonts();
    }
    fn tick(&mut self) {
        if self.shared_synced_at.elapsed() >= Duration::from_millis(250) {
            if let Err(e) = self.sync_project() {
                self.note(format!("Shared project: {e:#}"));
            }
            self.shared_synced_at = Instant::now();
        }
        self.sync_audio();
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
                    self.navigation.observe(&activity, crate::navigation::now());
                    self.activity = activity;
                    self.activity_pending = false;
                    self.activity_at = Instant::now();
                    self.sort_projects(false);
                }
                Event::Git(id, location, status) => {
                    self.git_pending.remove(&id);
                    self.git.insert(id, (location, *status, Instant::now()));
                }
                Event::Applications(source, token, result) => {
                    if let Some(modal) = self.modal.as_mut()
                        && let Some(catalog) = modal.catalog.as_mut()
                        && catalog.source == source
                        && format!(
                            "{}\n{}",
                            modal.fields[2].1.text().trim(),
                            modal.fields[3].1.text().trim()
                        ) == source
                        && catalog.changed == token
                    {
                        match result {
                            Ok(apps) => {
                                catalog.status = format!("{} registered applications", apps.len());
                                catalog.apps = apps;
                            }
                            Err(error) => catalog.status = error,
                        }
                        if modal.fields[0].1.text().is_empty() {
                            self.choose_application(0);
                        }
                    }
                }
                Event::Ready(config, pane) => {
                    if self.navigation.closed_projects.contains(&config.workspace) {
                        continue;
                    }
                    self.launch(*config, pane);
                    self.note("Project session connected / process retained".into())
                }
                Event::Health(project, pane, value, stopped) => {
                    if stopped.load(Ordering::Acquire) {
                        continue;
                    }
                    if value["exited"] == true && value["reaped"] == true {
                        if let Err(error) = self.remove_exited_terminal(&project, pane) {
                            self.note(format!("Close exited terminal: {error:#}"));
                        }
                    } else {
                        self.health.insert((project, pane), value);
                    }
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
        // A hidden tmux client retains its initial/default size and can become
        // the latest client when another window creates a tab. Keep attachments
        // only for visible panes; monitors and backend processes stay alive.
        if self.desktop.is_none() && self.native_terminal_cells {
            let visible = self.visible_panes();
            self.terminals.retain(|(project, pane), _| {
                project == &self.config.workspace && visible.iter().any(|(id, _)| id == pane)
            });
            for (pane, _) in visible {
                if self
                    .health
                    .get(&(self.config.workspace.clone(), pane))
                    .is_some_and(|health| health["alive"] == true)
                {
                    self.launch(self.config.clone(), pane);
                }
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
        let ended_imports = self
            .terminals
            .iter()
            .filter(|((project, pane), terminal)| {
                terminal.exited_successfully
                    && self
                        .projects
                        .iter()
                        .find(|config| &config.workspace == project)
                        .and_then(|config| config.herdr.as_ref())
                        .is_some_and(|source| source.panes.contains_key(pane))
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for (project, pane) in ended_imports {
            if let Err(error) = self.remove_exited_terminal(&project, pane) {
                self.note(format!("Close exited terminal: {error:#}"));
            }
        }
        if self.desktop.is_none() && self.native_terminal_cells && self.state.space != Space::Apps {
            let panes = self.visible_panes();
            let content = self.presentation_canvas();
            let key = format!(
                "{}:{}:{}:{panes:?}",
                self.config.workspace,
                self.viewport.generation,
                self.state.selected()
            );
            if key != self.native_layout_key {
                for terminal in self.terminals.values_mut() {
                    terminal.reset_native();
                }
                self.native_layout_key = key;
            }
            for (pane, area) in panes {
                let (rect, padding) = if self.cells {
                    (
                        self.cell_panes()
                            .into_iter()
                            .find(|(id, _)| *id == pane)
                            .unwrap()
                            .1,
                        [0, 0],
                    )
                } else {
                    native_terminal_rect(area, content, self.viewport)
                };
                if let Some(terminal) = self
                    .terminals
                    .get_mut(&(self.config.workspace.clone(), pane))
                {
                    terminal.set_metrics(self.typography);
                    match terminal.resize_native(
                        rect,
                        padding,
                        [
                            (self.viewport.width / u32::from(self.viewport.columns.max(1))).max(1)
                                as u16,
                            (self.viewport.height / u32::from(self.viewport.rows.max(1))).max(1)
                                as u16,
                        ],
                    ) {
                        Ok(()) => {
                            if let Some(frame) = terminal.native_frame(
                                app_id(&self.config.workspace, pane),
                                self.viewport.generation,
                                rect,
                                &self.appearance,
                                pane == self.config.canonical_pane(self.state.selected())
                                    && self.modal.is_none()
                                    && self.context_menu.is_none()
                                    && !self.git_open
                                    && (!self.cells || self.cell_ui.terminal_focus),
                            ) {
                                self.effects.push(ServerMessage::TerminalCells { frame });
                            }
                        }
                        Err(error) => self.status = format!("Terminal resize: {error:#}"),
                    }
                }
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
        self.refresh_applications();
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
                    DesktopEvent::Layout(layout) => host.layout = layout,
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
                && std::env::var_os("STARFLUX_RENDER_TIMING").is_some()
            {
                eprintln!(
                    "Terminal submit: at_ns={} age_us={}",
                    crate::runtime::monotonic_ns(),
                    frame.time.elapsed().as_micros()
                );
            }
            self.last_frame = Some((source, frame.pixels.clone()));
            let frame = if self.state.space == Space::Apps {
                crate::app_view::crop(frame, self.presentation_canvas())
            } else {
                frame
            };
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
        if self.navigation.dirty && self.navigation_saved_at.elapsed() >= Duration::from_secs(2) {
            self.sort_projects(false);
            if let Err(error) = self.navigation.save(&self.controller_id) {
                self.note(format!("Could not save project order: {error:#}"));
            }
            self.navigation_saved_at = Instant::now();
        }
        if self.last_inspect.elapsed() > Duration::from_millis(500) {
            self.last_inspect = Instant::now();
            if let Ok(root) = root() {
                let stamp = project_state::data_root()
                    .ok()
                    .and_then(|r| fs::metadata(r.join("catalog-version")).ok())
                    .and_then(|m| m.modified().ok());
                if stamp != self.catalog_stamp {
                    if let Ok(config) = crate::saved(&self.config.workspace) {
                        self.config = config;
                    }
                    self.projects = project_state::catalog(&self.config);
                    self.navigation.reconcile(&self.projects);
                    self.sort_projects(false);
                    self.catalog_stamp = stamp;
                }
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
                let apps=self.app_hosts.iter().filter(|(_,h)|h.config.workspace==self.config.workspace).map(|(key,h)|json!({"key":key,"host":h.config.host,"connected":h.client.connected.load(Ordering::Acquire),"transport_pid":h.client.transport_pid(),"worker_pid":*h.client.pid.lock().unwrap(),"windows":h.windows,"layout":h.layout,"selected":h.selected})).collect::<Vec<_>>();
                let sessions=self.state.terminal_ids().into_iter().map(|i| { let health=self.health.get(&(self.config.workspace.clone(),self.config.canonical_pane(i))); json!({"pane":i,"agent":health.is_some_and(|h| h["agent"].as_str().is_some() && h["alive"]==true),"health":health}) }).collect::<Vec<_>>();
                let mut snapshot = json!({"workspace":self.workspace_id,"active_project":self.config.workspace,"project_label":self.config.label,"project_activity":self.projects.iter().map(|p|(&p.workspace,self.project_activity(p))).collect::<BTreeMap<_,_>>(),"wave_phase":self.wave_phase,"reduced_motion":self.reduced_motion,"collection":self.config.collection,"project_order":self.projects.iter().map(|p|&p.workspace).collect::<Vec<_>>(),"project_page":self.project_offset,"project_navigation":{"kind":"sidebar","sort":if self.navigation.recent{"recent"}else{"manual"},"high_first":self.navigation.high_first,"hover":self.sort_hover,"mode_hover":self.sort_mode_hover,"manual_order":self.navigation.manual_order,"dragging":self.project_drag.as_ref().filter(|d|d.moved).map(|d|&d.id),"last_activity":self.projects.iter().map(|p|(&p.workspace,self.navigation.last_activity(&p.workspace))).collect::<BTreeMap<_,_>>(),"scroll":self.project_offset,"visible":self.projects.iter().skip(self.project_offset).take(self.project_page_size).map(|p|&p.workspace).collect::<Vec<_>>(),"sidebar_width":crate::shelf::Layout::new(self.viewport.width as u16,self.viewport.height as u16,self.state.space).sidebar.width},"herdr_read_only":false,"herdr_live_attachment":self.config.herdr.is_some(),"controller_pid":std::process::id(),"panes":panes,"active":self.state.selected(),"status":self.status,"terminal_presentation":{"fps":self.settings.fps,"transport":self.terminal_transport,"app_video":if self.video_capable && !self.video_failed{"h264-60fps"}else{"frames"},"app_video_packets":self.video.counters().0,"app_video_credits":self.video.counters().1,"app_shared_frames":self.active_app.as_ref().and_then(|k|self.app_hosts.get(k)).is_some_and(|h|h.client.shared_frames.load(Ordering::Acquire)),"burst_fps":if self.settings.fps>=60{125}else{30},"canvas":[crate::shelf::Layout::new(self.viewport.width as u16,self.viewport.height as u16,self.state.space).canvas.width,crate::shelf::Layout::new(self.viewport.width as u16,self.viewport.height as u16,self.state.space).canvas.height],"frame":self.desktop.as_ref().and_then(|d|d.latest.lock().unwrap().as_ref().map(|f|[f.width,f.height])),"kitty_configuration":"user"},"audio":{"capable":self.audio_capable,"muted":self.navigation.audio_muted,"active":self.audio_route.as_ref().map(|r|&r.key),"epoch":self.audio_epoch,"blocks_sent":self.audio_route.as_ref().map(|r|r.sent),"credits":self.audio_route.as_ref().map(|r|r.credits),"failed":self.audio_failed},"terminal_typography":self.typography,"terminal_renderer":if self.desktop.is_none(){"native-kitty"}else{"kitty-pixels-legacy"},"direct_terminals":self.terminals.iter().filter(|((p,_),_)|p==&self.config.workspace).map(|((_,pane),t)|json!({"pane":pane,"attachment_pid":t.pid,"exited":t.exited,"text":t.text()})).collect::<Vec<_>>(),"desktop":{"width":self.settings.width,"height":self.settings.height,"windows":self.windows.len(),"project_windows":self.windows.iter().filter(|w|w.app_id.starts_with(&format!("starflux-{}-p",self.config.workspace))).collect::<Vec<_>>()},"sessions":sessions,"project_state":self.state,"apps":apps,"drop_enabled":self.drop_enabled,"history":self.history,"git":self.git_status(),"git_open":self.git_open,"git_scroll":self.git_scroll,"context_menu":self.context_menu.as_ref().map(|m|json!({"title":m.title,"bounds":m.bounds(self.viewport.width as u16,self.viewport.height as u16),"selected":m.selected,"items":m.items.iter().map(|i|json!({"label":i.label,"action":i.action})).collect::<Vec<_>>()})),"modal_open":self.modal.is_some(),"modal_field":self.modal.as_ref().map(|m|m.selected),"application_picker":self.modal.as_ref().and_then(|m|m.catalog.as_ref()).map(|c|json!({"source":c.source,"status":c.status,"query":c.query.text(),"count":c.apps.len(),"selected":c.selected,"matches":c.matches().into_iter().map(|i|&c.apps[i]).collect::<Vec<_>>()})),"appearance":{"treatment":"app-shelf","borders":"single","theme":self.config.theme,"tokens":self.appearance},"viewport":self.viewport});
                snapshot["graphical_render"] = self.graphical_cache.statistics();
                snapshot["app_video_render"] = self.video.statistics();
                snapshot["audio"]["playback_protocol"] = json!(if self.audio_pcm_v2 {
                    "pcm-v2"
                } else {
                    "legacy-stereo"
                });
                snapshot["presentation"] = json!(if self.cells { "cells" } else { "graphical" });
                snapshot["app_view_available"] = json!(self.app_view_available);
                snapshot["cell_terminal_focus"] = json!(self.cell_ui.terminal_focus);
                if self.cells {
                    let canvas = self.presentation_canvas();
                    snapshot["terminal_renderer"] = json!("parent-terminal");
                    snapshot["terminal_presentation"]["canvas"] =
                        json!([canvas.width, canvas.height]);
                    snapshot["project_navigation"]["sidebar_width"] = json!(canvas.x);
                    if let Some(bounds) = self.cell_menu_bounds() {
                        snapshot["context_menu"]["bounds"] = json!(bounds);
                    }
                }
                snapshot["ui_refresh"] = json!(true);
                snapshot["frontend"] = json!(self.controller_id);
                if self.desktop.is_none() {
                    snapshot["terminal_presentation"]["fps"] = Value::Null;
                    snapshot["terminal_presentation"]["burst_fps"] = Value::Null;
                    snapshot["terminal_presentation"]["native_cells"] = json!(true);
                    snapshot["terminal_presentation"]["renderer_owner"] = json!("host Kitty");
                    snapshot["terminal_presentation"]["terminal_virtual_desktop"] = json!(false);
                }
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
        if let Input::Notice { message } = &input {
            if message == "require-graphical" {
                self.require_graphical();
                return;
            }
            if message == "terminal-repaint-key" {
                if self.cells && self.cell_ui.terminal_focus && !self.cells_overlay() {
                    self.direct_input(EmbeddedKind::Key {
                        code: "char:l".into(),
                        modifiers: 2,
                        phase: KeyPhase::Press,
                    });
                }
                return;
            }
        }
        let refresh = match &input {
            Input::Key { code, modifiers } => {
                matches!(code.as_str(), "f5" | "f:5")
                    || (matches!(code.as_str(), "char:r" | "char:R") && modifiers & 3 == 3)
            }
            Input::Embedded { event, .. } => {
                matches!(&event.kind,EmbeddedKind::Key{code,modifiers,phase} if *phase!=KeyPhase::Release && matches!(code.as_str(),"char:r"|"char:R") && modifiers&3==3)
            }
            _ => false,
        };
        if refresh {
            if let Err(error) = self.refresh_ui() {
                self.note(format!("Refresh UI: {error:#}"));
            }
            return;
        }

        match &input {
            Input::Pointer {
                action,
                pixel,
                x,
                y,
                ..
            } if action == "move" => {
                let point = pixel.unwrap_or([
                    u32::from(*x) * self.viewport.width / u32::from(self.viewport.columns.max(1)),
                    u32::from(*y) * self.viewport.height / u32::from(self.viewport.rows.max(1)),
                ]);
                self.sort_mode_hover = self.modal.is_none()
                    && self.context_menu.is_none()
                    && !self.git_open
                    && self.chrome_hits.iter().any(|h| {
                        h.action == "projects:sort:mode"
                            && h.rect.contains(point[0] as u16, point[1] as u16)
                    });
                self.sort_hover = self.modal.is_none()
                    && self.context_menu.is_none()
                    && !self.git_open
                    && self.chrome_hits.iter().any(|h| {
                        h.action == "projects:sort:direction"
                            && h.rect.contains(point[0] as u16, point[1] as u16)
                    });
                self.project_hover = self
                    .chrome_hits
                    .iter()
                    .find(|h| {
                        h.rect.x == 0
                            && h.action.starts_with("project:")
                            && h.rect.contains(point[0] as u16, point[1] as u16)
                    })
                    .and_then(|h| h.action.strip_prefix("project:"))
                    .map(str::to_owned);
            }
            Input::CancelPointer | Input::Embedded { .. } => {
                self.sort_hover = false;
                self.sort_mode_hover = false;
                self.project_hover = None;
            }
            _ => {}
        }
        if self.cells {
            if let Err(error) = self.cells_input(input) {
                self.note(format!("{error:#}"));
            }
            return;
        }
        if self.project_order_input(&input) {
            return;
        }
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
                        if let Some(catalog) =
                            modal.catalog.as_mut().filter(|c| c.visible && c.focused)
                        {
                            catalog.query.paste(&text);
                            self.choose_application(0);
                        } else {
                            modal.fields[modal.selected].1.paste(&text)
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
                    Input::Pointer { action, .. }
                        if action == "scroll_up" || action == "scroll_down" =>
                    {
                        self.action(if action == "scroll_up" {
                            "modal:apps:up"
                        } else {
                            "modal:apps:down"
                        })?;
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
                    "char:n" => self.action("terminal:new")?,
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
                    if message == "focus-canvas:terminals" && self.state.space != Space::Apps {
                        self.desktop_send(DesktopCommand::FocusApp(app_id(
                            &self.config.workspace,
                            self.config.canonical_pane(self.state.selected()),
                        )));
                    } else if let Some(paths) = message.strip_prefix("drop:") {
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
    fn tick_interval(&self) -> Duration {
        if self.desktop.is_none() && self.state.space != Space::Apps {
            Duration::from_millis(8)
        } else {
            Duration::from_millis(33)
        }
    }
    fn frame_interval(&self) -> Duration {
        // Live terminal/app frames have their own 60 Hz transport. Periodic
        // chrome follows SCREEN's cadence; input still requests an immediate scene.
        Duration::from_millis(100)
    }
    #[cfg(feature = "lossless-canvas")]
    fn lossless_canvas(&self, stream: &str) -> bool {
        stream == "terminals"
    }
    fn live_frames(&mut self) -> Vec<Update> {
        let source = if self.state.space == Space::Apps && self.video_capable && !self.video_failed
        {
            self.active_app.clone()
        } else {
            None
        };
        if self.video_source != source {
            self.video.stop();
            self.video_source = source.clone();
        }
        let updates = self.frames.drain();
        if source.is_none() {
            return updates;
        }
        let mut other = vec![];
        for update in updates {
            if matches!(&update, Update::Frame{stream,..}|Update::Close{stream,..} if stream=="apps")
            {
                if let Err(e) = self.video.submit(update) {
                    self.video_failed = true;
                    self.video.stop();
                    self.note(format!("App video: {e:#}"));
                }
            } else {
                other.push(update);
            }
        }
        other
    }
    fn detached(&mut self) {
        if self.controller_id != self.workspace_id {
            self.quit = true;
        }
        #[cfg(feature = "lossless-canvas")]
        {
            self.remote_drop = None;
        }
        self.receiving = None;
        self.drop_target = None;
        self.drop_stage = None;
        self.video.stop();
        self.video_capable = false;
        self.close_audio();
        self.audio_capable = false;
    }
    fn media(&mut self, message: starkit::terminal_graphics::media::ToHost) {
        use starkit::terminal_graphics::media::ToHost;
        self.video.receive(&message);
        match message {
            ToHost::AudioCredit {
                session,
                epoch,
                blocks,
            } if session == AUDIO_SESSION => {
                if let Some(route) = self.audio_route.as_mut().filter(|r| r.epoch == epoch) {
                    route.credits = route.credits.saturating_add(blocks as u64);
                    route.source.grant(blocks);
                }
            }
            ToHost::AudioError {
                session,
                epoch,
                message,
            } if session == AUDIO_SESSION
                && self.audio_route.as_ref().is_some_and(|r| r.epoch == epoch) =>
            {
                self.audio_failed = self.audio_route.as_ref().map(|r| r.key.clone());
                self.close_audio();
                self.note(message);
            }
            _ => {}
        }
    }
    fn media_chunks(&mut self) -> Vec<ServerMessage> {
        if let Some(route) = self.audio_route.as_mut()
            && let Ok(samples) = route.source.data.try_recv()
        {
            route.sent += 1;
            return vec![ServerMessage::Media {
                message: route.playback.chunk(AUDIO_SESSION, route.epoch, samples),
            }];
        }
        if let Some(error) = self.video.warning() {
            self.video_failed = true;
            self.note(error);
        }
        match self.video.messages(1) {
            Ok(messages) => messages,
            Err(error) => {
                self.video_failed = true;
                self.video.stop();
                self.note(format!("App video: {error:#}"));
                vec![]
            }
        }
    }
    fn effects(&mut self) -> Vec<ServerMessage> {
        let hidden = self.state.space == Space::Apps
            && self.modal.is_none()
            && self.context_menu.is_none()
            && !self.git_open
            && self
                .active_app
                .as_ref()
                .and_then(|key| self.app_hosts.get(key))
                .is_some_and(|host| {
                    host.client.connected.load(Ordering::Acquire)
                        && host.client.cursor_hidden.load(Ordering::Acquire)
                });
        if self.pointer_hiding && self.cursor_policy_sent != Some(hidden) {
            self.effects.push(ServerMessage::PointerPolicy {
                canvas: "apps".into(),
                hidden,
            });
            self.cursor_policy_sent = Some(hidden);
        }
        std::mem::take(&mut self.effects)
    }
    fn closed(&self) -> bool {
        self.quit
    }
    fn shutdown(&mut self) {
        self.close_audio();
        let _ = self.navigation.save(&self.controller_id);
        // Keep startup preferences when the last window closes; active peers keep theirs.
        if self.controller_id != self.workspace_id
            && crate::frontend::active(&self.workspace_id).is_ok_and(|v| v.len() <= 1)
        {
            let _ = self.navigation.save(&self.workspace_id);
        }
        self.release();
        if let Some(cancel) = &self.transfer {
            cancel.store(true, Ordering::Release)
        }
        self.save();
    }
}
fn drop_image_mime(offered: &[String]) -> Option<(usize, Option<&'static str>)> {
    for (mime, extension) in [
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/webp", "webp"),
        ("image/gif", "gif"),
    ] {
        if let Some(index) = offered.iter().position(|s| s == mime) {
            return Some((index, Some(extension)));
        }
    }
    None
}
fn drop_mime(offered: &[String]) -> Option<(usize, Option<&'static str>)> {
    offered
        .iter()
        .position(|s| s == "text/uri-list")
        .map(|i| (i, None))
        .or_else(|| drop_image_mime(offered))
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

#[cfg(all(test, feature = "lossless-canvas"))]
mod native_terminal_tests {
    use super::*;
    #[test]
    fn terminal_grid_and_pointer_padding_share_the_real_kitty_cells() {
        let viewport = Viewport {
            columns: 248,
            rows: 85,
            width: 1984,
            height: 1615,
            generation: 1,
        };
        let canvas = PixelRect::new(267, 161, 1702, 1297);
        let (rect, pad) = native_terminal_rect(PixelRect::new(0, 0, 1702, 1297), canvas, viewport);
        assert_eq!(
            (u32::from(rect.x) * 8, u32::from(rect.y) * 19),
            (
                u32::from(canvas.x) + u32::from(pad[0]),
                u32::from(canvas.y) + u32::from(pad[1])
            )
        );
        assert!(u32::from(rect.x + rect.width) * 8 <= u32::from(canvas.x + canvas.width));
        assert!(u32::from(rect.y + rect.height) * 19 <= u32::from(canvas.y + canvas.height));
    }
}
