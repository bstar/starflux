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
    InputBurst(bool),
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
    CursorHidden(bool),
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
    #[cfg(test)]
    pub fn test_transport(
        events: Receiver<DesktopEvent>,
        commands: Sender<DesktopCommand>,
        latest: Arc<Mutex<Option<Frame>>>,
    ) -> Self {
        Self {
            commands,
            events,
            latest,
            join: None,
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

/// Ignore unchanged pointer modifier state so adjacent motion can coalesce.
#[derive(Default)]
pub struct PointerModifiers(Option<u8>);
impl PointerModifiers {
    pub fn reset(&mut self) {
        self.0 = None;
    }
    pub fn forward(&mut self, input: &Input) -> bool {
        match input {
            Input::Modifiers { modifiers } => {
                if self.0 == Some(*modifiers) {
                    return false;
                }
                self.0 = Some(*modifiers);
            }
            Input::Motion { .. } | Input::Button { .. } | Input::Scroll { .. } => {}
            _ => self.reset(),
        }
        true
    }
}
#[cfg(test)]
mod pointer_tests {
    use super::*;
    #[test]
    fn motion_bursts_do_not_repeat_modifier_barriers() {
        let mut filter = PointerModifiers::default();
        let mut modifiers = 0;
        for n in 0..1000 {
            modifiers += usize::from(filter.forward(&Input::Modifiers { modifiers: 1 }));
            assert!(filter.forward(&Input::Motion { x: n as f64, y: 2. }));
        }
        assert_eq!(modifiers, 1);
        assert!(filter.forward(&Input::Modifiers { modifiers: 0 }));
        assert!(filter.forward(&Input::NamedKey {
            code: "char:a".into(),
            pressed: true,
            modifiers: 0
        }));
        assert!(filter.forward(&Input::Modifiers { modifiers: 0 }));
        filter.reset();
        assert!(filter.forward(&Input::Modifiers { modifiers: 0 }));
    }
}
