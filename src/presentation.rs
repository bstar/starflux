//! Local presentation choice, shared with the presentation-only terminal client.
use anyhow::Result;
use starcore::{
    crossterm::event::{Event, KeyCode, KeyModifiers},
    terminal_graphics::{
        client::{self, Launch, PresentationOptions, PresentationPreference},
        protocol::Input,
    },
};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
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
static ANNOUNCE_LOCAL_RETURN: AtomicBool = AtomicBool::new(false);
static ANNOUNCE_LOCAL_MACHINES: Mutex<Option<String>> = Mutex::new(None);
static REQUIRE_GRAPHICAL: AtomicBool = AtomicBool::new(false);
static FOCUS_NOTICE: AtomicU8 = AtomicU8::new(0);
static KEYS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The SSH host serving this window, which cannot read this computer's
/// clipboard itself. Ctrl+V with an image copies it there.
static REMOTE: Mutex<Option<crate::clipboard_image::Remote>> = Mutex::new(None);
static SENDING: AtomicBool = AtomicBool::new(false);

fn queue(message: String) {
    let mut keys = KEYS.lock().unwrap();
    if keys.len() < 32 {
        keys.push(message);
    }
}

/// Ctrl+V with only an image on the clipboard, in a window served over SSH:
/// copy the image to the host and deliver it there like a dropped file.
fn remote_clipboard_image(key: &starcore::crossterm::event::KeyEvent) -> Option<Input> {
    if key.code != KeyCode::Char('v')
        || key.modifiers != KeyModifiers::CONTROL
        || key.kind != starcore::crossterm::event::KeyEventKind::Press
    {
        return None;
    }
    let remote = REMOTE.lock().unwrap();
    let remote = remote.as_ref()?;
    let (mime, extension) = crate::clipboard_image::offered()?;
    if SENDING.swap(true, Ordering::AcqRel) {
        // One image is already on its way; this press is part of it.
        return Some(Input::Notice {
            message: "clipboard-image:sending".into(),
        });
    }
    let remote = crate::clipboard_image::Remote {
        host: remote.host.clone(),
        ssh_config: remote.ssh_config.clone(),
        executable: remote.executable.clone(),
    };
    std::thread::spawn(move || {
        let sent = crate::clipboard_image::send(&remote, mime, extension);
        SENDING.store(false, Ordering::Release);
        queue(match sent {
            Ok(path) => format!("drop:{}", serde_json::json!([path])),
            Err(error) => format!("clipboard-image:failed:{error:#}"),
        });
    });
    Some(Input::Notice {
        message: "clipboard-image:sending".into(),
    })
}

pub fn event(event: &Event) -> Option<Input> {
    if let Event::Key(key) = event
        && let Some(input) = remote_clipboard_image(key)
    {
        return Some(input);
    }
    match event {
        // CORE reserves Ctrl+L for repaint before dispatching custom input. Queue
        // the key as a reliable notice as well, so a focused shell receives it.
        Event::Key(key)
            if key.code == KeyCode::Char('l')
                && key.modifiers == KeyModifiers::CONTROL
                && key.kind == starcore::crossterm::event::KeyEventKind::Press =>
        {
            queue("terminal-repaint-key".into());
            None
        }
        Event::Osc72(text) => {
            crate::font_frontend::event(text).or_else(|| Some(Input::Osc72 { text: text.clone() }))
        }
        Event::FocusLost => {
            FOCUS_NOTICE.store(1, Ordering::Relaxed);
            None
        }
        Event::FocusGained => {
            FOCUS_NOTICE.store(2, Ordering::Relaxed);
            Some(Input::Notice {
                message: "focus-canvas:terminals".into(),
            })
        }
        _ => None,
    }
}
fn notice() -> Option<String> {
    if ANNOUNCE_LOCAL_RETURN.swap(false, Ordering::Relaxed) {
        return Some("local-return-available".into());
    }
    if let Some(machines) = ANNOUNCE_LOCAL_MACHINES.lock().unwrap().take() {
        return Some(machines);
    }

    // CORE handles pointer cancellation separately from application focus.
    match FOCUS_NOTICE.swap(0, Ordering::Relaxed) {
        1 => return Some("window-focus:lost".into()),
        2 => return Some("window-focus:gained".into()),
        _ => {}
    }

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
    // No switch is advertised by the controller, so CORE never writes this file.
    // Launch choices are window-local and explicitly retained by refresh argv.
    PresentationPreference {
        cells: mode == Mode::Cells,
        path: std::path::PathBuf::from("/dev/null"),
    }
}
pub fn run(launch: Launch, mode: Mode) -> Result<()> {
    *REMOTE.lock().unwrap() = launch
        .host
        .clone()
        .map(|host| crate::clipboard_image::Remote {
            host,
            ssh_config: launch.ssh_config.clone(),
            executable: launch.executable.clone(),
        });
    let _font_output = crate::font_frontend::LocalOutput::start()?;
    ANNOUNCE_LOCAL_RETURN.store(crate::refresh::has_local_return(), Ordering::Relaxed);
    *ANNOUNCE_LOCAL_MACHINES.lock().unwrap() = crate::refresh::local_machines_notice();
    REQUIRE_GRAPHICAL.store(mode == Mode::Graphical, Ordering::Relaxed);
    // `event` maps complete OSC 72 messages to `Input::Osc72`, so this frontend
    // opts in to desktop file-drop negotiation (STAR/CORE keeps it off otherwise).
    client::run_with_osc72_preference(
        launch,
        event,
        notice,
        PresentationOptions::default(),
        preference(mode),
    )
}
