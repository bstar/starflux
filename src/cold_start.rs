//! Bounded, private screen previews. Saved bytes are parsed for display only.
use crate::{project_state::ProjectState, runtime::Config};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const LIMIT: usize = 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Screen {
    pub version: u8,
    pub project: String,
    pub pane: usize,
    pub epoch: Option<String>,
    pub host: Option<String>,
    pub boot_id: Option<String>,
    pub pid: Option<u32>,
    pub start_ticks: Option<u64>,
    pub captured_ms: u64,
    #[serde(default)]
    pub observer_boot_id: Option<String>,
    #[serde(default)]
    pub captured_monotonic_ns: u64,
    pub rows: u16,
    pub columns: u16,
    pub alternate: bool,
    pub cursor: (u16, u16),
    #[serde(rename = "formatted_base64", with = "screen_bytes")]
    pub formatted: String,
}
mod screen_bytes {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(text: &str, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(text.as_bytes()))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        if encoded.len() > super::LIMIT.div_ceil(3) * 4 {
            return Err(D::Error::custom("Saved screen exceeds its limit"));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(D::Error::custom)?;
        if bytes.len() > super::LIMIT {
            return Err(D::Error::custom("Saved screen exceeds its limit"));
        }
        String::from_utf8(bytes).map_err(D::Error::custom)
    }
}
fn path(project: &str, pane: usize, previous: bool) -> Result<PathBuf> {
    crate::session_catalog::valid(project)?;
    ensure!(project.len() <= 32, "Invalid screen project");
    ensure!(pane < 32, "Invalid screen pane");
    let root = crate::project_state::data_root()?
        .parent()
        .context("No screen data directory")?
        .join("screens");
    starkit::terminal_graphics::session::private_root(&root)?;
    let root = root.join(project);
    starkit::terminal_graphics::session::private_root(&root)?;
    Ok(root.join(format!(
        "p{pane}.{}.json",
        if previous {
            "previous-screen"
        } else {
            "screen"
        }
    )))
}
impl Screen {
    pub fn capture(
        config: &Config,
        pane: usize,
        epoch: Option<String>,
        health: &serde_json::Value,
        screen: &vt100::Screen,
    ) -> Result<Self> {
        let visible = screen;
        let (rows, columns) = visible.size();
        let result = Self {
            version: 1,
            project: config.workspace.clone(),
            pane,
            epoch,
            host: config.remote(pane).then(|| config.host.clone()).flatten(),
            boot_id: health["boot_id"].as_str().map(str::to_owned),
            pid: health["pid"].as_u64().and_then(|p| p.try_into().ok()),
            start_ticks: health["pid_start_ticks"].as_u64(),
            captured_ms: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64,
            observer_boot_id: crate::agent_state::boot_id().map(|id| id.to_string()),
            captured_monotonic_ns: crate::runtime::monotonic_ns(),
            rows,
            columns,
            alternate: visible.alternate_screen(),
            cursor: visible.cursor_position(),
            formatted: String::from_utf8(
                visible
                    .contents_formatted_bounded(LIMIT)
                    .context("Saved screen exceeds its formatted byte limit")?,
            )?,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported saved screen version");
        crate::session_catalog::valid(&self.project)?;
        ensure!(self.project.len() <= 32, "Invalid screen project");
        ensure!(
            self.epoch
                .as_ref()
                .is_none_or(|s| s.len() <= 128 && !s.chars().any(char::is_control))
                && self
                    .host
                    .as_ref()
                    .is_none_or(|s| s.len() <= 1024 && !s.chars().any(char::is_control))
                && self
                    .boot_id
                    .as_ref()
                    .is_none_or(|s| uuid::Uuid::parse_str(s).is_ok())
                && self
                    .observer_boot_id
                    .as_ref()
                    .is_none_or(|s| uuid::Uuid::parse_str(s).is_ok()),
            "Invalid saved screen source"
        );
        ensure!(
            self.pane < 32
                && self.rows > 0
                && self.columns > 0
                && u32::from(self.rows) * u32::from(self.columns) <= 500_000,
            "Invalid saved screen geometry"
        );
        ensure!(
            self.formatted.len() <= LIMIT
                && self.cursor.0 < self.rows
                // vt100 retains column == width while the next character is
                // waiting to wrap. This is a valid terminal cursor state.
                && self.cursor.1 <= self.columns,
            "Invalid saved screen size or cursor"
        );
        Ok(())
    }
    fn same_owner(&self, other: &Self) -> bool {
        self.epoch == other.epoch
            && self.host == other.host
            && self.boot_id == other.boot_id
            && self.pid == other.pid
            && self.start_ticks == other.start_ticks
    }
    fn older_than(&self, current: &Self) -> bool {
        if self.observer_boot_id.is_some() && self.observer_boot_id == current.observer_boot_id {
            self.captured_monotonic_ns < current.captured_monotonic_ns
        } else if self.observer_boot_id.is_none() && current.observer_boot_id.is_none() {
            self.captured_ms < current.captured_ms
        } else {
            false
        }
    }
    pub fn parse(&self) -> Result<vt100::Screen> {
        self.validate()?;
        // No terminal callbacks, PTY, clipboard, queries, titles or notifications.
        let mut parser = vt100::Parser::new(self.rows, self.columns, 0);
        parser.process(self.formatted.as_bytes());
        Ok(parser.screen().clone())
    }
}
pub fn load(project: &str, pane: usize, previous: bool) -> Result<Screen> {
    let path = path(project, pane, previous)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .context("No saved screen is available yet")?
        .take((LIMIT * 2 + 4097) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= LIMIT * 2 + 4096,
        "Saved screen file exceeds its limit"
    );
    let screen: Screen = serde_json::from_slice(&bytes).context("Read saved screen")?;
    screen.validate()?;
    ensure!(
        screen.project == project && screen.pane == pane,
        "Saved screen identity does not match"
    );
    Ok(screen)
}
pub(crate) fn save(config: &Config, screen: &Screen) -> Result<()> {
    screen.validate()?;
    let Some(_lock) = crate::project_sync::Transaction::try_acquire(
        crate::project_state::data_root()?.join(format!("{}.model.lock", config.workspace)),
    )?
    else {
        // A busy project must not stall every project's bounded snapshot queue.
        return Ok(());
    };
    let saved = crate::project_state::data_root()?.join(format!("{}.json", config.workspace));
    if !saved.exists() {
        return Ok(());
    }
    let current: Config = serde_json::from_slice(&fs::read(&saved)?)?;
    let current = serde_json::to_value(&current)?;
    let expected = serde_json::to_value(config)?;
    if [
        "host",
        "ssh_config",
        "project_host",
        "herdr",
        "tmux",
        "adopted",
    ]
    .iter()
    .any(|field| current[*field] != expected[*field])
    {
        return Ok(());
    }
    let state = ProjectState::load(config)?;
    if !state.save_terminal_screens {
        return Ok(());
    }
    if screen.project != config.workspace
        || !state.terminal_ids().contains(&screen.pane)
        || state.terminal_epochs.get(&screen.pane) != screen.epoch.as_ref()
    {
        return Ok(());
    }
    if path(&screen.project, screen.pane, false)?.exists() {
        let current = load(&screen.project, screen.pane, false)?;
        if screen.older_than(&current) {
            return Ok(());
        }
        if !current.same_owner(screen) {
            crate::automation::persist(
                &path(&screen.project, screen.pane, true)?,
                &serde_json::to_vec(&current)?,
            )?;
        } else if current.formatted == screen.formatted
            && current.rows == screen.rows
            && current.columns == screen.columns
        {
            return Ok(());
        }
    }
    crate::automation::persist(
        &path(&screen.project, screen.pane, false)?,
        &serde_json::to_vec(screen)?,
    )
}
pub fn clear(project: &str, pane: usize) -> Result<()> {
    for previous in [false, true] {
        match fs::remove_file(path(project, pane, previous)?) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
pub fn remove(project: &str, pane: usize) -> Result<()> {
    clear(project, pane)?;
    let receipt = launch_path(project, pane)?;
    match fs::remove_file(receipt) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
fn launch_path(project: &str, pane: usize) -> Result<PathBuf> {
    Ok(path(project, pane, false)?.with_file_name(format!("p{pane}.launch.json")))
}
/// Caller holds the project transaction. A receipt survives stale layout saves
/// and uncertain remote outcomes; retries must not launch an agent twice.
pub fn consume_launch(config: &Config, pane: usize, epoch: Option<&str>) -> Result<bool> {
    #[derive(Serialize, Deserialize)]
    struct Receipt {
        version: u8,
        epoch: Option<String>,
    }
    let path = launch_path(&config.workspace, pane)?;
    match fs::File::open(&path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(4097).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 4096, "Launch receipt exceeds its limit");
            let previous: Receipt =
                serde_json::from_slice(&bytes).context("Read launch receipt")?;
            ensure!(previous.version == 1, "Unsupported launch receipt version");
            if previous.epoch.as_deref() == epoch {
                return Ok(false);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    crate::automation::persist(
        &path,
        &serde_json::to_vec(&Receipt {
            version: 1,
            epoch: epoch.map(str::to_owned),
        })?,
    )?;
    Ok(true)
}
pub struct Writer {
    tx: crossbeam_channel::Sender<(Config, Screen)>,
    errors: crossbeam_channel::Receiver<String>,
}
impl Writer {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::bounded::<(Config, Screen)>(32);
        let (error_tx, errors) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            for (config, screen) in rx {
                if let Err(e) = save(&config, &screen) {
                    let _ = error_tx.try_send(format!("Saved screen: {e:#}"));
                }
            }
        });
        Self { tx, errors }
    }
    pub fn submit(&self, config: Config, screen: Screen) -> bool {
        self.tx.try_send((config, screen)).is_ok()
    }
    pub fn error(&self) -> Option<String> {
        self.errors.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_preserves_styled_unicode_cursor_and_ignores_effects() {
        let mut parser = vt100::Parser::new(12, 40, 10);
        parser.process(b"\x1b[?1049h\x1b[1;38;2;20;40;60m");
        parser.process("Plan café 界\r\nsecond row".as_bytes());
        parser.process(b"\x1b]52;c;c2VjcmV0\x07\x1b]0;untrusted title\x07\x1b[4;8H");
        let original = parser.screen();
        let screen = Screen {
            version: 1,
            project: "private-test".into(),
            pane: 0,
            epoch: None,
            host: None,
            boot_id: None,
            pid: None,
            start_ticks: None,
            captured_ms: 1,
            observer_boot_id: Some("7a307521-f2e5-4647-8d7e-b1330366037e".into()),
            captured_monotonic_ns: 100,
            rows: 12,
            columns: 40,
            alternate: original.alternate_screen(),
            cursor: original.cursor_position(),
            formatted: String::from_utf8(original.contents_formatted()).unwrap(),
        };
        let restored = screen.parse().unwrap();
        assert_eq!(restored.contents(), original.contents());
        assert_eq!(restored.cursor_position(), original.cursor_position());
        assert!(restored.cell(0, 0).unwrap().bold());
        assert_eq!(
            restored.cell(0, 0).unwrap().fgcolor(),
            vt100::Color::Rgb(20, 40, 60)
        );
        assert!(!screen.formatted.contains("52;"));
        assert!(!screen.formatted.contains("untrusted title"));
        assert!(screen.alternate);
        let mut clock_rollback = screen.clone();
        clock_rollback.captured_ms = 0;
        clock_rollback.captured_monotonic_ns = 101;
        assert!(!clock_rollback.older_than(&screen));
        let mut delayed = screen.clone();
        delayed.captured_ms = 9999;
        delayed.captured_monotonic_ns = 99;
        assert!(delayed.older_than(&screen));
        let mut rebooted = screen.clone();
        rebooted.observer_boot_id = Some("088f251c-a7de-43cd-a07c-3032d2c12f71".into());
        rebooted.captured_monotonic_ns = 1;
        assert!(!rebooted.older_than(&screen));
        let encoded = serde_json::to_value(&screen).unwrap();
        let roundtrip: Screen = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(roundtrip.parse().unwrap().contents(), original.contents());
        // A full final column leaves the parser's cursor one column past
        // the grid until another character causes a wrap. Capture and load
        // must accept that state, including wide Unicode characters.
        for text in ["x".repeat(40), "界".repeat(20)] {
            let mut boundary = vt100::Parser::new(12, 40, 0);
            boundary.process(text.as_bytes());
            assert_eq!(boundary.screen().cursor_position(), (0, 40));
            let mut saved = screen.clone();
            saved.cursor = boundary.screen().cursor_position();
            saved.formatted = String::from_utf8(boundary.screen().contents_formatted()).unwrap();
            let saved: Screen =
                serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
            assert_eq!(saved.parse().unwrap().contents(), text);
            assert_eq!(saved.parse().unwrap().cursor_position(), (0, 40));
            let mut invalid = saved.clone();
            invalid.cursor.1 = 41;
            assert!(invalid.parse().is_err());
            invalid = saved;
            invalid.cursor.0 = 12;
            assert!(invalid.parse().is_err());
        }
        let mut invalid = encoded;
        invalid["formatted_base64"] = serde_json::json!("invalid!");
        assert!(serde_json::from_value::<Screen>(invalid).is_err());
        let mut bad = screen;
        bad.columns = u16::MAX;
        bad.rows = u16::MAX;
        assert!(bad.parse().is_err());
    }
}
