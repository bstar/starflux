//! Read-only captures for terminals without a frontend attachment.
use crate::{cold_start::Screen, runtime::Config};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Seek},
    os::unix::{fs::OpenOptionsExt, process::CommandExt},
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

static CAPTURES: AtomicUsize = AtomicUsize::new(0);
pub struct Permit;
impl Permit {
    pub fn acquire() -> Option<Self> {
        CAPTURES
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 2).then_some(n + 1)
            })
            .ok()
            .map(|_| Self)
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        CAPTURES.fetch_sub(1, Ordering::Release);
    }
}

fn output(mut command: Command) -> Result<Vec<u8>> {
    let mut file = tempfile::tempfile()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(file.try_clone()?)
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let began = Instant::now();
    let limit = 2 * 1024 * 1024;
    let status = loop {
        if began.elapsed() > Duration::from_secs(4) || file.metadata()?.len() > limit {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            anyhow::bail!("Background screen capture exceeded its size or time limit");
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    ensure!(status.success(), "Background screen source unavailable");
    file.rewind()?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Background screen exceeds its limit"
    );
    Ok(bytes)
}
#[derive(Deserialize)]
struct Frame {
    rows: u16,
    columns: u16,
    ansi_base64: String,
    cursor: Option<[u16; 2]>,
    alternate: Option<bool>,
    cursor_visible: Option<bool>,
}
pub fn inspect(config: &Config, pane: usize) -> Result<Value> {
    if let Some(source) = config
        .herdr
        .as_ref()
        .filter(|s| s.panes.contains_key(&pane))
    {
        return herdr(source, pane);
    }
    let mut request = config.request(pane, "inspect");
    request["include_screen"] = json!(true);
    if pane == 0
        && let Some(source) = &config.tmux
    {
        request["source_socket"] = json!(source.socket);
        request["source_session"] = json!(source.session);
    }
    let mut health: Value =
        serde_json::from_slice(&output(config.command(pane, &request, false))?)?;
    if pane == 0 && config.tmux.is_some() {
        health["source"] = json!("tmux");
        health["source_preserved"] = json!(true);
    }
    if let Some(terminal) = config.adopted_terminal(pane) {
        let mut original = terminal.observe(pane)?;
        original["pid_start_ticks"] = json!(crate::agent_state::process_start(terminal.pid));
        original["boot_id"] = json!(crate::agent_state::boot_id().map(|b| b.to_string()));
        for field in ["saved_screen", "screen_error"] {
            if let Some(value) = health.get_mut(field) {
                original[field] = value.take();
            }
        }
        health = original;
    }
    Ok(health)
}
pub fn take(
    config: &Config,
    pane: usize,
    epoch: Option<String>,
    health: &mut Value,
    started_ns: u64,
) -> Result<Option<Screen>> {
    let Some(value) = health
        .as_object_mut()
        .and_then(|h| h.remove("saved_screen"))
    else {
        return Ok(None);
    };
    let frame: Frame = serde_json::from_value(value)?;
    ensure!(
        frame.rows > 0
            && frame.columns > 0
            && u32::from(frame.rows) * u32::from(frame.columns) <= 500_000,
        "Invalid background screen geometry"
    );
    ensure!(
        frame.ansi_base64.len() <= 1_398_104,
        "Background screen exceeds 1 MiB"
    );
    let bytes = STANDARD.decode(frame.ansi_base64)?;
    ensure!(
        bytes.len() <= 1024 * 1024,
        "Background screen exceeds 1 MiB"
    );
    let text = std::str::from_utf8(&bytes)?;
    let mut parser = vt100::Parser::new(frame.rows, frame.columns, 0);
    for (index, line) in text
        .strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .enumerate()
    {
        if index > 0 {
            parser.process(b"\r\n");
        }
        parser.process(line.as_bytes());
    }
    if let Some([row, col]) = frame.cursor {
        ensure!(
            row < frame.rows && col < frame.columns,
            "Invalid background screen cursor"
        );
        parser.process(format!("\x1b[{};{}H", row + 1, col + 1).as_bytes());
    }
    if let Some(visible) = frame.cursor_visible {
        parser.process(if visible { b"\x1b[?25h" } else { b"\x1b[?25l" });
    }
    let mut saved = Screen::capture(config, pane, epoch, health, parser.screen())?;
    saved.captured_monotonic_ns = started_ns;
    if let Some(alternate) = frame.alternate {
        saved.alternate = alternate;
    }
    Ok(Some(saved))
}
fn herdr(source: &crate::herdr::Source, pane: usize) -> Result<Value> {
    let id = &source.panes[&pane].pane_id;
    let pane_info: Value = serde_json::from_slice(&output(source.command(&["pane", "get", id]))?)?;
    let info: Value = serde_json::from_slice(&output(source.command(&[
        "pane",
        "process-info",
        "--pane",
        id,
    ]))?)?;
    let process = &info["result"]["process_info"];
    let pid = process["shell_pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .context("Herdr terminal process identity unavailable")?;
    let start =
        crate::agent_state::process_start(pid).context("Herdr terminal process is unavailable")?;
    let device = fs::read_link(format!("/proc/{pid}/fd/0"))?;
    let tty = device
        .to_str()
        .context("Herdr terminal device unavailable")?;
    ensure!(
        tty.starts_with("/dev/pts/")
            && process["tty"]
                .as_str()
                .is_none_or(|reported| reported == tty),
        "Herdr terminal device changed"
    );
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
        .open(tty)?;
    use std::os::fd::AsRawFd;
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe { libc::ioctl(file.as_raw_fd(), libc::TIOCGWINSZ, &mut size) } == 0,
        "Could not read Herdr terminal size"
    );
    ensure!(
        size.ws_row > 0
            && size.ws_col > 0
            && u32::from(size.ws_row) * u32::from(size.ws_col) <= 500_000,
        "Herdr screen exceeds its geometry limit"
    );
    let bytes = output(source.command(&[
        "pane", "read", id, "--source", "visible", "--format", "ansi", "--raw",
    ]))?;
    ensure!(
        bytes.len() <= 1024 * 1024 && crate::agent_state::process_start(pid) == Some(start),
        "Herdr terminal changed during capture"
    );
    let p = &pane_info["result"]["pane"];
    let mut health = source.health_from_pane(pane, p);
    health["pid"] = json!(pid);
    health["pid_start_ticks"] = json!(start);
    health["boot_id"] = json!(crate::agent_state::boot_id().map(|b| b.to_string()));
    health["saved_screen"] = json!({"rows":size.ws_row,"columns":size.ws_col,"ansi_base64":STANDARD.encode(bytes),"cursor":null,"alternate":null,"cursor_visible":null});
    Ok(health)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backend_without_cursor_metadata_can_capture_a_full_final_column() {
        let config: Config =
            serde_json::from_value(json!({"workspace":"screen-test","executable":"/bin/false"}))
                .unwrap();
        let mut health = json!({"alive":true,"pid":123,"saved_screen":{"rows":8,"columns":40,"ansi_base64":STANDARD.encode("X".repeat(40)),"cursor":null,"alternate":null,"cursor_visible":null}});
        let screen = take(&config, 0, None, &mut health, 42).unwrap().unwrap();
        assert_eq!(screen.cursor, (0, 40));
        assert_eq!(screen.parse().unwrap().contents(), "X".repeat(40));
    }
    #[test]
    fn backend_frames_preserve_lines_style_and_cursor_without_exporting_bytes() {
        let config: Config =
            serde_json::from_value(json!({"workspace":"screen-test","executable":"/bin/false"}))
                .unwrap();
        let mut health = json!({"alive":true,"pid":123,"pid_start_ticks":5,"saved_screen":{"rows":8,"columns":40,"ansi_base64":STANDARD.encode("\x1b[31;1mfirst 界\nsecond\n\x1b]52;c;bm8=\x07"),"cursor":[2,3],"alternate":true,"cursor_visible":false}});
        let saved = take(&config, 0, None, &mut health, 42).unwrap().unwrap();
        assert_eq!(saved.captured_monotonic_ns, 42);
        assert!(health.get("saved_screen").is_none());
        let parsed = saved.parse().unwrap();
        assert_eq!(parsed.cell(0, 0).unwrap().contents(), "f");
        assert_eq!(parsed.cell(1, 0).unwrap().contents(), "s");
        assert_eq!(parsed.cell(0, 0).unwrap().fgcolor(), vt100::Color::Idx(1));
        assert!(parsed.cell(0, 0).unwrap().bold());
        assert_eq!(parsed.cursor_position(), (2, 3));
        assert!(parsed.hide_cursor() && saved.alternate);
        assert!(!saved.formatted.contains("52;"));
        let mut invalid = json!({"saved_screen":{"rows":65535,"columns":65535,"ansi_base64":"","cursor":null,"alternate":null,"cursor_visible":null}});
        assert!(take(&config, 0, None, &mut invalid, 43).is_err());
    }
    #[test]
    fn capture_concurrency_is_bounded_and_released() {
        let first = Permit::acquire().unwrap();
        let second = Permit::acquire().unwrap();
        assert!(Permit::acquire().is_none());
        drop(first);
        assert!(Permit::acquire().is_some());
        drop(second);
    }
}
