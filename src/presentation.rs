//! Local presentation choice, shared with the presentation-only terminal client.
use anyhow::Result;
use starkit::{
    crossterm::event::{Event, KeyCode, KeyModifiers},
    terminal_graphics::{
        client::{self, Launch, PresentationOptions, PresentationPreference},
        protocol::Input,
    },
};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Auto,
    Cells,
    Graphical,
}
impl Mode {
    pub fn cells(self) -> Option<bool> {
        match self {
            Self::Auto => None,
            Self::Cells => Some(true),
            Self::Graphical => Some(false),
        }
    }
    pub fn args(self) -> &'static [&'static str] {
        match self {
            Self::Auto => &[],
            Self::Cells => &["--cells"],
            Self::Graphical => &["--graphical"],
        }
    }
}
static REQUIRE_GRAPHICAL: AtomicBool = AtomicBool::new(false);
static KEYS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn event(event: &Event) -> Option<Input> {
    match event {
        // KIT reserves Ctrl+L for repaint before dispatching custom input. Queue
        // the key as a reliable notice as well, so a focused shell receives it.
        Event::Key(key)
            if key.code == KeyCode::Char('l')
                && key.modifiers == KeyModifiers::CONTROL
                && key.kind == starkit::crossterm::event::KeyEventKind::Press =>
        {
            let mut keys = KEYS.lock().unwrap();
            if keys.len() < 32 {
                keys.push("terminal-repaint-key".into());
            }
            None
        }
        Event::Osc72(text) => Some(Input::Osc72 { text: text.clone() }),
        Event::FocusGained => Some(Input::Notice {
            message: "focus-canvas:terminals".into(),
        }),
        _ => None,
    }
}
fn notice() -> Option<String> {
    if REQUIRE_GRAPHICAL.swap(false, Ordering::Relaxed) {
        Some("require-graphical".into())
    } else {
        let mut keys = KEYS.lock().unwrap();
        if keys.is_empty() {
            None
        } else {
            Some(keys.remove(0))
        }
    }
}
fn preference(mode: Mode) -> PresentationPreference {
    // No switch is advertised by the controller, so KIT never writes this file.
    // Launch choices are window-local and explicitly retained by refresh argv.
    PresentationPreference {
        cells: mode == Mode::Cells,
        path: std::path::PathBuf::from("/dev/null"),
    }
}
pub fn run(launch: Launch, mode: Mode) -> Result<()> {
    REQUIRE_GRAPHICAL.store(mode == Mode::Graphical, Ordering::Relaxed);
    client::run_with_preference(
        launch,
        event,
        notice,
        PresentationOptions::default(),
        preference(mode),
    )
}
