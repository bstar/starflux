//! Display-side font extension. Scenes/media remain byte-for-byte passthrough.
//! Font commands go only to the fixed Kitty watcher, never a host command.
use anyhow::{Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
const META: &str = "starflux-font-v1";
const LIMIT: usize = 16 * 1024 * 1024;

/// One-shot application hint, consumed by the next terminal bridge probe.
/// Only a fixed installed helper is selectable; no host-supplied local path.
pub fn select_terminal_client() -> Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(b"\x1b]1337;SetUserVar=starflux_client=MQ==\x1b\\")?;
    out.flush()?;
    Ok(())
}
pub fn event(text: &str) -> Option<starkit::terminal_graphics::protocol::Input> {
    let encoded = text.strip_prefix("starflux-font-v1;")?;
    if encoded.len() > 128 * 1024 {
        return None;
    }
    let bytes = STANDARD.decode(encoded).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    Some(starkit::terminal_graphics::protocol::Input::Notice {
        message: format!("{META}:{}", value),
    })
}
/// Created on the presenting computer before KIT starts its renderer threads.
/// The relay runs in another process; it must never write escapes straight to
/// /dev/tty, where they could split a renderer escape in the middle.
pub struct LocalOutput {
    _root: tempfile::TempDir,
}
impl LocalOutput {
    pub fn start() -> Result<Self> {
        let root = tempfile::Builder::new()
            .prefix("starflux-font-output-")
            .tempdir()?;
        let path = root.path().join("control");
        let listener = UnixListener::bind(&path)?;
        // This is frontend startup, before any KIT/input/audio threads exist.
        unsafe {
            std::env::set_var("STARFLUX_FONT_OUTPUT", &path);
        }
        std::thread::spawn(move || {
            for peer in listener.incoming().flatten() {
                let _ = peer.set_read_timeout(Some(std::time::Duration::from_secs(1)));
                let mut bytes = Vec::new();
                if BufReader::new(peer)
                    .take(8193)
                    .read_until(b'\n', &mut bytes)
                    .is_err()
                    || bytes.len() > 8192
                {
                    continue;
                }
                let Ok(request) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                let escape = format!(
                    "\x1b]1337;SetUserVar=starflux_font={}\x1b\\",
                    STANDARD.encode(request.to_string())
                );
                // Same stdout lock as KIT's presenter: commands and frames are atomic.
                let mut output = std::io::stdout().lock();
                let _ = output
                    .write_all(escape.as_bytes())
                    .and_then(|_| output.flush());
            }
        });
        Ok(Self { _root: root })
    }
}
struct Control {
    nonce: String,
    generation: Option<u64>,
}
impl Control {
    fn new() -> Self {
        let mut random = [0; 16];
        let _ = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut random));
        Self {
            nonce: random.iter().map(|b| format!("{b:02x}")).collect(),
            generation: None,
        }
    }
    fn emit(&self, mut request: Value) -> Result<()> {
        request["nonce"] = json!(self.nonce);
        let path = std::env::var_os("STARFLUX_FONT_OUTPUT")
            .ok_or_else(|| anyhow::anyhow!("Local font output is unavailable"))?;
        let mut output = UnixStream::connect(path)?;
        serde_json::to_writer(&mut output, &request)?;
        output.write_all(b"\n")?;
        Ok(())
    }
    fn probe(&mut self, generation: u64) {
        if self.generation != Some(generation) {
            self.generation = Some(generation);
            let _ = self.emit(json!({"op":"probe"}));
        }
    }
    fn filter(&mut self, line: &[u8]) -> Result<Option<u64>> {
        // Large pixel/video/scene packets never need a second JSON decode.
        if !line.starts_with(b"{\"type\":\"osc72\"")
            && !line.starts_with(b"{\"type\":\"terminal_font\"")
        {
            return Ok(None);
        }
        let message: Value = serde_json::from_slice(line)?;
        if message["type"] == "terminal_font" {
            let size = message["size_millipoints"].as_u64().unwrap_or(u64::MAX);
            ensure!(
                size == 0 || (6000..=32000).contains(&size),
                "Invalid font size"
            );
            let _ = self.emit(json!({"op":"size", "size":size}));
        } else if message["meta"] == META {
            let payload = message["payload"].as_str().unwrap_or("");
            ensure!(payload.len() <= 4096, "Font request exceeds limit");
            let request: Value = serde_json::from_str(payload)?;
            if let Err(error) = self.emit(request) {
                // An unavailable terminal integration must not disconnect a pane.
                let _ = error;
            }
            return Ok(Some(message["id"].as_u64().unwrap_or(0)));
        }
        Ok(None)
    }
}
fn pump(
    input: impl Read,
    mut output: impl Write,
    upstream: &Arc<Mutex<UnixStream>>,
    control: &Arc<Mutex<Control>>,
) -> Result<()> {
    let mut reader = BufReader::new(input);
    let mut line = Vec::new();
    loop {
        line.clear();
        let count = reader
            .by_ref()
            .take((LIMIT + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        ensure!(
            line.len() <= LIMIT && line.ends_with(b"\n"),
            "Invalid frontend frame"
        );
        if let Some(id) = control.lock().unwrap().filter(&line)? {
            let mut target = upstream.lock().unwrap();
            serde_json::to_writer(&mut *target, &json!({"type":"effect_ack", "id":id}))?;
            target.write_all(b"\n")?;
        } else {
            output.write_all(&line)?;
            output.flush()?;
        }
    }
    Ok(())
}
fn forward_inputs(
    input: impl Read,
    upstream: &Arc<Mutex<UnixStream>>,
    control: &Arc<Mutex<Control>>,
) {
    let mut input = BufReader::new(input);
    let mut line = Vec::new();
    loop {
        line.clear();
        let Ok(n) = input
            .by_ref()
            .take((LIMIT + 1) as u64)
            .read_until(b'\n', &mut line)
        else {
            break;
        };
        if n == 0 || n > LIMIT || !line.ends_with(b"\n") {
            break;
        }
        // A font report before the first presented scene is discarded by KIT's
        // ordinary input readiness gate. Probe only after native rendering owns input.
        if line.starts_with(b"{\"type\":\"presented\"")
            && let Ok(message) = serde_json::from_slice::<Value>(&line)
            && let Some(generation) = message["generation"].as_u64()
        {
            control.lock().unwrap().probe(generation);
        }
        if upstream.lock().unwrap().write_all(&line).is_err() {
            break;
        }
    }
    let _ = upstream.lock().unwrap().shutdown(std::net::Shutdown::Both);
}
/// Local full frontend relay; SSH server relays must never act on server fonts.
pub fn relay(path: &Path) -> Result<()> {
    let socket = UnixStream::connect(path)?;
    let upstream = Arc::new(Mutex::new(socket.try_clone()?));
    let writer = upstream.clone();
    let control = Arc::new(Mutex::new(Control::new()));
    let input_control = control.clone();
    std::thread::spawn(move || forward_inputs(std::io::stdin().lock(), &writer, &input_control));
    pump(socket, std::io::stdout().lock(), &upstream, &control)
}
/// Private socket adapter for the terminal-owned local helper (including SSH).
pub struct Proxy {
    pub path: PathBuf,
    _root: tempfile::TempDir,
}
impl Proxy {
    pub fn new(target: &Path) -> Result<Self> {
        let root = tempfile::Builder::new()
            .prefix("starflux-font-")
            .tempdir()?;
        // KIT authenticates using the filename; preserve the bridge nonce.
        let path = root.path().join(
            target
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Missing terminal socket name"))?,
        );
        let listener = UnixListener::bind(&path)?;
        let target = target.to_owned();
        std::thread::spawn(move || {
            let Ok((client, _)) = listener.accept() else {
                return;
            };
            let Ok(server) = UnixStream::connect(target) else {
                return;
            };
            let Ok(mut incoming) = client.try_clone() else {
                return;
            };
            let Ok(server_writer) = server.try_clone() else {
                return;
            };
            let upstream = Arc::new(Mutex::new(server_writer));
            let writer = upstream.clone();
            let control = Arc::new(Mutex::new(Control::new()));
            let input_control = control.clone();
            std::thread::spawn(move || forward_inputs(&mut incoming, &writer, &input_control));
            let _ = pump(server, client, &upstream, &control);
            let _ = upstream.lock().unwrap().shutdown(std::net::Shutdown::Both);
        });
        Ok(Self { path, _root: root })
    }
}
