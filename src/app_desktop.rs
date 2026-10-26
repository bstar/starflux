//! A persistent private compositor, with the same bounded wire path locally and over SSH.
use crate::{
    compositor::{Desktop, DesktopCommand, DesktopEvent, Frame, Input, WindowInfo},
    project::{Launch, Settings, WindowLayout},
    runtime::Config,
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use crossbeam_channel::{Receiver, Sender, bounded};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        io::AsRawFd,
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
const VERSION: u16 = 1;
const MAX_PACKET: usize = 24 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Launch { launch: Launch },
    Resize { settings: Settings },
    Input { input: Input },
    Focus { window: u64 },
    Maximize { window: u64 },
    Close { window: u64 },
    Tile,
    Layout,
    Restore { layout: Vec<WindowLayout> },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Reply {
    Hello { version: u16, pid: u32 },
    Frame { png: String },
    Windows { windows: Vec<WindowInfo> },
    Layout { layout: Vec<WindowLayout> },
    Error { message: String },
}
fn read<T: serde::de::DeserializeOwned>(input: &mut impl Read) -> Result<T> {
    let mut size = [0; 4];
    input.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    ensure!(
        size > 0 && size <= MAX_PACKET,
        "Desktop packet exceeds limit"
    );
    let mut bytes = vec![0; size];
    input.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn write(value: &impl Serialize, output: &mut impl Write) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX_PACKET, "Desktop packet exceeds limit");
    output.write_all(&(bytes.len() as u32).to_be_bytes())?;
    output.write_all(&bytes)?;
    output.flush()?;
    Ok(())
}
fn directory() -> Result<PathBuf> {
    let path = PathBuf::from(format!("/tmp/starmux-desktops-{}", unsafe {
        libc::geteuid()
    }));
    starkit::terminal_graphics::session::private_root(&path)?;
    Ok(path)
}
fn paths(name: &str) -> Result<(PathBuf, PathBuf)> {
    ensure!(
        !name.is_empty()
            && name.len() <= 32
            && name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
        "Invalid desktop identity"
    );
    let root = directory()?;
    Ok((
        root.join(format!("{name}.sock")),
        root.join(format!("{name}.stop")),
    ))
}
fn lock(path: PathBuf) -> Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    ensure!(
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "This project desktop is already attached to another workspace"
    );
    Ok(file)
}
pub fn internal(mode: &str, name: &str) -> Result<()> {
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        let runtime = PathBuf::from(format!("/tmp/starmux-runtime-{}", unsafe {
            libc::geteuid()
        }));
        starkit::terminal_graphics::session::private_root(&runtime)?;
        // Internal helper entry runs before creating any threads.
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", runtime);
        }
    }
    match mode {
        "--desktop-worker" => worker(name),
        "--desktop-relay" => relay(name),
        "--desktop-stop" => {
            let (_, stop) = paths(name)?;
            fs::write(stop, b"stop")?;
            Ok(())
        }
        _ => anyhow::bail!("Unknown desktop helper mode"),
    }
}
fn spawn_worker(name: &str) -> Result<()> {
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory()?.join(format!("{name}.log")))?;
    Command::new("dbus-run-session")
        .arg("--")
        .arg(std::env::current_exe()?)
        .args(["--desktop-worker", name])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .process_group(0)
        .spawn()?;
    Ok(())
}
pub fn launch_local(name: &str, launch: Option<Launch>) -> Result<Vec<WindowInfo>> {
    paths(name)?;
    let control = directory()?.join(format!("{name}.control.sock"));
    let mut stream = match UnixStream::connect(&control) {
        Ok(s) => s,
        Err(_) => {
            let (_, stop) = paths(name)?;
            let _ = fs::remove_file(stop);
            spawn_worker(name)?;
            let start = Instant::now();
            loop {
                match UnixStream::connect(&control) {
                    Ok(s) => break s,
                    Err(e) => {
                        ensure!(
                            start.elapsed() < Duration::from_secs(15),
                            "App-space worker did not start: {e}"
                        );
                        std::thread::sleep(Duration::from_millis(40))
                    }
                }
            }
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(25)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    write(
        &if let Some(launch) = launch {
            Request::Launch { launch }
        } else {
            Request::Layout
        },
        &mut stream,
    )?;
    match read::<Reply>(&mut stream)? {
        Reply::Windows { windows } => Ok(windows),
        Reply::Error { message } => anyhow::bail!(message),
        _ => anyhow::bail!("Invalid app-space launch reply"),
    }
}
struct ControlLaunch {
    stream: UnixStream,
    existing: std::collections::BTreeSet<u64>,
    started: Instant,
}
fn relay(name: &str) -> Result<()> {
    let (socket, stop) = paths(name)?;
    let _lease = lock(directory()?.join(format!("{name}.attach.lock")))?;
    let mut existing = UnixStream::connect(&socket).ok();
    if existing.is_none() {
        let _ = fs::remove_file(&stop);
        spawn_worker(name)?;
    }
    let started = Instant::now();
    let mut stream = loop {
        if let Some(stream) = existing.take() {
            break stream;
        }
        match UnixStream::connect(&socket) {
            Ok(s) => break s,
            Err(e) => {
                ensure!(
                    started.elapsed() < Duration::from_secs(15),
                    "Desktop helper did not start: {e}; see {}",
                    directory()?.join(format!("{name}.log")).display()
                );
                std::thread::sleep(Duration::from_millis(30))
            }
        }
    };
    let mut writer = stream.try_clone()?;
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut writer);
        let _ = writer.shutdown(std::net::Shutdown::Both);
    });
    // stdout is line-buffered even when it is an SSH pipe. Small binary
    // handshakes/static frames must be flushed without waiting for a newline.
    let mut output = std::io::stdout().lock();
    let mut buffer = [0u8; 65536];
    while let Ok(n) = stream.read(&mut buffer) {
        if n == 0 {
            break;
        }
        if output
            .write_all(&buffer[..n])
            .and_then(|_| output.flush())
            .is_err()
        {
            break;
        }
    }
    Ok(())
}
fn encode(frame: &Frame) -> Result<String> {
    let mut rgba = frame.pixels.as_ref().clone();
    for p in rgba.as_chunks_mut::<4>().0 {
        p.swap(0, 2)
    }
    let mut bytes = vec![];
    {
        let mut encoder = png::Encoder::new(&mut bytes, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        encoder.write_header()?.write_image_data(&rgba)?;
    }
    Ok(STANDARD.encode(bytes))
}
fn decode(png: &str) -> Result<Frame> {
    let bytes = STANDARD.decode(png)?;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: 64 * 1024 * 1024,
    });
    let mut reader = decoder.read_info()?;
    let info = reader.info();
    ensure!(
        info.width <= 2560
            && info.height <= 1440
            && info.color_type == png::ColorType::Rgba
            && info.bit_depth == png::BitDepth::Eight,
        "Invalid desktop frame"
    );
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels)?;
    pixels.truncate(info.buffer_size());
    for p in pixels.as_chunks_mut::<4>().0 {
        p.swap(0, 2)
    }
    Ok(Frame {
        pixels: Arc::new(pixels),
        width: info.width,
        height: info.height,
        time: Instant::now(),
    })
}
fn desktop_command(request: Request) -> DesktopCommand {
    match request {
        Request::Launch { launch } => DesktopCommand::Launch(launch),
        Request::Resize { settings } => DesktopCommand::Resize(settings),
        Request::Input { input } => DesktopCommand::Input(input),
        Request::Focus { window } => DesktopCommand::FocusId(window),
        Request::Maximize { window } => DesktopCommand::MaximizeId(window),
        Request::Close { window } => DesktopCommand::CloseId(window),
        Request::Tile => DesktopCommand::Tile,
        Request::Layout => DesktopCommand::Layout,
        Request::Restore { layout } => DesktopCommand::Restore(layout),
    }
}
fn worker(name: &str) -> Result<()> {
    let (socket, stop) = paths(name)?;
    let _lease = lock(directory()?.join(format!("{name}.worker.lock")))?;
    let _ = fs::remove_file(&socket);
    let _ = fs::remove_file(&stop);
    let control_path = directory()?.join(format!("{name}.control.sock"));
    let _ = fs::remove_file(&control_path);
    let control_listener = UnixListener::bind(&control_path)?;
    control_listener.set_nonblocking(true)?;
    let mut control_launches: Vec<ControlLaunch> = vec![];
    let listener = UnixListener::bind(&socket)?;
    listener.set_nonblocking(true)?;
    let desktop = Desktop::start(Settings {
        width: 1280,
        height: 720,
        fps: 30,
    });
    let (tx, rx) = bounded::<(u64, Option<Request>)>(128);
    let mut client: Option<UnixStream> = None;
    let mut generation = 0;
    let mut windows = vec![];
    let mut layout = vec![];
    let mut last_frame = None;
    let mut next = Instant::now();
    while !stop.exists() {
        if let Ok((mut stream, _)) = control_listener.accept() {
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            stream.set_write_timeout(Some(Duration::from_secs(2)))?;
            match read::<Request>(&mut stream) {
                Ok(Request::Launch { launch })
                    if !launch.command.is_empty()
                        && launch.command.len() <= 8192
                        && !launch.command.contains('\0') =>
                {
                    if !control_launches.is_empty() {
                        let _ = write(&Reply::Error { message: "Another app launch is waiting for its window; retry after it finishes".into() }, &mut stream);
                        continue;
                    }
                    desktop.send(DesktopCommand::Launch(launch));
                    control_launches.push(ControlLaunch {
                        stream,
                        existing: windows.iter().map(|w: &WindowInfo| w.id).collect(),
                        started: Instant::now(),
                    });
                }
                Ok(Request::Layout) => {
                    let _ = write(
                        &Reply::Windows {
                            windows: windows.clone(),
                        },
                        &mut stream,
                    );
                }
                _ => {
                    let _ = write(
                        &Reply::Error {
                            message: "App control accepts a launch command or window listing"
                                .into(),
                        },
                        &mut stream,
                    );
                }
            }
        }

        if let Ok((mut stream, _)) = listener.accept() {
            generation += 1;
            let id = generation;
            stream.set_write_timeout(Some(Duration::from_secs(3)))?;
            desktop.send(DesktopCommand::Input(Input::Release));
            let handshake = (|| -> Result<()> {
                write(
                    &Reply::Hello {
                        version: VERSION,
                        pid: std::process::id(),
                    },
                    &mut stream,
                )?;
                write(
                    &Reply::Windows {
                        windows: windows.clone(),
                    },
                    &mut stream,
                )?;
                write(
                    &Reply::Layout {
                        layout: layout.clone(),
                    },
                    &mut stream,
                )?;
                Ok(())
            })();
            if handshake.is_err() {
                continue;
            }
            let mut input = stream.try_clone()?;
            let tx = tx.clone();
            std::thread::spawn(move || {
                while let Ok(request) = read::<Request>(&mut input) {
                    if tx.send((id, Some(request))).is_err() {
                        return;
                    }
                }
                let _ = tx.send((id, None));
            });
            client = Some(stream);
            last_frame = None;
        }
        for (id, request) in rx.try_iter() {
            if id != generation {
                continue;
            }
            if let Some(request) = request {
                let valid = match &request {
                    Request::Resize { settings } => {
                        settings.validate().is_ok()
                            && settings.width <= 2560
                            && settings.height <= 1440
                    }
                    Request::Launch { launch } => {
                        launch.command.len() <= 8192 && !launch.command.contains('\0')
                    }
                    _ => true,
                };
                if valid {
                    desktop.send(desktop_command(request));
                }
            } else {
                desktop.send(DesktopCommand::Input(Input::Release));
                client = None;
            }
        }
        for event in desktop.events.try_iter() {
            let reply = match event {
                DesktopEvent::Windows(value) => {
                    windows = value.clone();
                    Some(Reply::Windows { windows: value })
                }
                DesktopEvent::Layout(value) => {
                    layout = value.clone();
                    Some(Reply::Layout { layout: value })
                }
                DesktopEvent::Error(message) => Some(Reply::Error { message }),
                _ => None,
            };
            if let (Some(reply), Some(stream)) = (reply, client.as_mut())
                && write(&reply, stream).is_err()
            {
                client = None;
                desktop.send(DesktopCommand::Input(Input::Release));
            }
        }
        control_launches.retain_mut(|pending|{
            let added=windows.iter().filter(|w|!pending.existing.contains(&w.id)).cloned().collect::<Vec<_>>();
            if !added.is_empty(){let _=write(&Reply::Windows{windows:added},&mut pending.stream);return false}
            if pending.started.elapsed()>Duration::from_secs(20){let _=write(&Reply::Error{message:"No new app window appeared within 20 seconds. Check the command, cartridge and app-host runtime.".into()},&mut pending.stream);return false}true
        });
        if Instant::now() >= next {
            next = Instant::now() + Duration::from_millis(33);
            let frame = desktop.latest.lock().unwrap().clone();
            if let Some(frame) = frame
                && client.is_some()
                && !last_frame
                    .as_ref()
                    .is_some_and(|p| Arc::ptr_eq(p, &frame.pixels))
            {
                let reply = Reply::Frame {
                    png: encode(&frame)?,
                };
                if write(&reply, client.as_mut().unwrap()).is_err() {
                    client = None;
                    desktop.send(DesktopCommand::Input(Input::Release));
                } else {
                    last_frame = Some(frame.pixels);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(desktop);
    let _ = fs::remove_file(control_path);
    let _ = fs::remove_file(socket);
    let _ = fs::remove_file(stop);
    Ok(())
}
fn command(config: &Config, mode: &str) -> Result<Command> {
    if let Some(host) = &config.host {
        let mut cmd = Command::new("ssh");
        if let Some(file) = &config.ssh_config {
            cmd.arg("-F").arg(file);
        }
        cmd.args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "--",
            host,
        ]);
        cmd.arg(shell_words::join([
            config.desktop_helper.as_str(),
            mode,
            &config.workspace,
        ]));
        Ok(cmd)
    } else {
        let mut cmd = Command::new(std::env::current_exe()?);
        cmd.args([mode, &config.workspace]);
        Ok(cmd)
    }
}
pub fn stop(config: &Config) -> Result<()> {
    let result = command(config, "--desktop-stop")?.output()?;
    ensure!(
        result.status.success(),
        "Could not stop app desktop: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
pub struct Client {
    pub latest: Arc<Mutex<Option<Frame>>>,
    pub events: Receiver<DesktopEvent>,
    pub connected: Arc<AtomicBool>,
    pub pid: Arc<Mutex<Option<u32>>>,
    tx: Sender<Request>,
    child: Child,
}
impl Client {
    pub fn connect(config: &Config) -> Result<Self> {
        let mut child = command(config, "--desktop-relay")?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut input = child.stdin.take().context("Desktop stdin unavailable")?;
        let mut output = child.stdout.take().context("Desktop stdout unavailable")?;
        let mut stderr = child.stderr.take().unwrap();
        let (tx, rx) = bounded::<Request>(128);
        let (events, erx) = crossbeam_channel::unbounded();
        let errors = events.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr.by_ref().take(65536).read_to_end(&mut bytes);
            if !bytes.is_empty() {
                let _ = errors.send(DesktopEvent::Error(format!(
                    "App host: {}",
                    String::from_utf8_lossy(&bytes).trim()
                )));
            }
        });
        let connected = Arc::new(AtomicBool::new(false));
        let connection = connected.clone();
        let pid = Arc::new(Mutex::new(None));
        let worker_pid = pid.clone();
        let latest = Arc::new(Mutex::new(None));
        let frames = latest.clone();
        let reader_events = events.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                match read::<Reply>(&mut output)? {
                    Reply::Hello { version, pid } => {
                        ensure!(version == VERSION, "Incompatible desktop helper version");
                        *worker_pid.lock().unwrap() = Some(pid);
                        connection.store(true, Ordering::Release)
                    }
                    _ => anyhow::bail!("Invalid desktop helper handshake"),
                };
                loop {
                    match read::<Reply>(&mut output)? {
                        Reply::Frame { png } => *frames.lock().unwrap() = Some(decode(&png)?),
                        Reply::Windows { windows } => {
                            let _ = reader_events.send(DesktopEvent::Windows(windows));
                        }
                        Reply::Layout { layout } => {
                            let _ = reader_events.send(DesktopEvent::Layout(layout));
                        }
                        Reply::Error { message } => {
                            let _ = reader_events.send(DesktopEvent::Error(message));
                        }
                        _ => {}
                    }
                }
            })();
            connection.store(false, Ordering::Release);
            if let Err(e) = result {
                let _=reader_events.send(DesktopEvent::Error(format!("App desktop disconnected: {e:#}. Reconnect to retain its apps; the app host needs a compatible starmux helper.")));
            }
        });
        std::thread::spawn(move || {
            while let Ok(request) = rx.recv() {
                let mut pending = None;
                let mut batch = vec![request];
                batch.extend(rx.try_iter().take(128));
                for request in batch {
                    if matches!(
                        request,
                        Request::Input {
                            input: Input::Motion { .. }
                        }
                    ) {
                        pending = Some(request);
                        continue;
                    }
                    if let Some(motion) = pending.take()
                        && write(&motion, &mut input).is_err()
                    {
                        return;
                    }
                    if write(&request, &mut input).is_err() {
                        return;
                    }
                }
                if let Some(motion) = pending
                    && write(&motion, &mut input).is_err()
                {
                    return;
                }
            }
        });
        Ok(Self {
            latest,
            events: erx,
            connected,
            pid,
            tx,
            child,
        })
    }
    pub fn transport_pid(&self) -> u32 {
        self.child.id()
    }
    pub fn send(&self, request: Request) -> Result<()> {
        if matches!(
            request,
            Request::Input {
                input: Input::Motion { .. }
            }
        ) {
            let _ = self.tx.try_send(request);
            return Ok(());
        }
        self.tx.try_send(request).map_err(|_| {
            anyhow::anyhow!("App desktop input is backlogged; release input and reconnect")
        })
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_codec_preserves_pixels_and_packet_bounds() {
        let frame = Frame {
            pixels: Arc::new([0, 1, 255, 255].repeat(256)),
            width: 16,
            height: 16,
            time: Instant::now(),
        };
        let decoded = decode(&encode(&frame).unwrap()).unwrap();
        assert_eq!(decoded.pixels, frame.pixels);
        let mut bytes = vec![];
        write(&Request::Tile, &mut bytes).unwrap();
        assert!(matches!(
            read::<Request>(&mut bytes.as_slice()).unwrap(),
            Request::Tile
        ));
        assert!(read::<Request>(&mut (MAX_PACKET as u32 + 1).to_be_bytes().as_slice()).is_err());
    }
}
