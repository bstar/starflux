// Adapted from STAR/SCREEN, MIT, Copyright (c) 2026 bstar.
use crate::project::{Launch, Settings, WindowLayout};
use crossbeam_channel::{Receiver, Sender};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Clone)]
pub struct Frame {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub time: Instant,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Input {
    Paste {
        text: String,
        mime: String,
    },
    Motion {
        x: f64,
        y: f64,
    },
    Button {
        button: u32,
        pressed: bool,
    },
    Key {
        code: u32,
        pressed: bool,
    },
    Modifiers {
        modifiers: u8,
    },
    NamedKey {
        code: String,
        pressed: bool,
        modifiers: u8,
    },
    Scroll {
        x: f64,
        y: f64,
    },
    Release,
}
pub enum DesktopCommand {
    Input(Input),
    Launch(Launch),
    Visible(Vec<String>),
    FocusApp(String),
    FocusId(u64),
    MaximizeId(u64),
    CloseId(u64),
    Resize(Settings),
    Background(starkit::native_surface::style::workspace::Wallpaper),
    RelayAudio(bool),
    Focus(usize),
    Maximize(usize),
    Close(usize),
    Restore(Vec<WindowLayout>),
    Layout,
    Tile,
    Shutdown,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WindowInfo {
    pub id: u64,
    pub name: String,
    pub app_id: String,
}
pub enum DesktopEvent {
    Ready(String),
    Windows(Vec<WindowInfo>),
    Selection(Option<(usize, [i32; 4])>),
    Layout(Vec<WindowLayout>),
    Error(String),
    CaptureStopped(String),
    Locked(bool),
}

pub struct Desktop {
    pub commands: Sender<DesktopCommand>,
    pub events: Receiver<DesktopEvent>,
    pub latest: Arc<Mutex<Option<Frame>>>,
    join: Option<std::thread::JoinHandle<()>>,
}
impl Desktop {
    pub fn start(settings: Settings) -> Self {
        Self::start_with_audio(settings, None)
    }
    pub fn start_with_audio(settings: Settings, audio_sink: Option<String>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (etx, erx) = crossbeam_channel::unbounded();
        let latest = Arc::new(Mutex::new(None));
        let frames = latest.clone();
        let join = std::thread::spawn(move || {
            if let Err(e) = super::run(settings, audio_sink, rx, etx.clone(), frames) {
                let _ = etx.send(DesktopEvent::Error(format!("Demo desktop: {e:#}")));
            }
        });
        Self {
            commands: tx,
            events: erx,
            latest,
            join: Some(join),
        }
    }
    pub fn send(&self, c: DesktopCommand) {
        let _ = self.commands.send(c);
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        self.send(DesktopCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
