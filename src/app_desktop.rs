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
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
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
    SharedFrames { enabled: bool },
    FrameAck { sequence: u64 },
    TrackCursor,
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
    Hello {
        version: u16,
        pid: u32,
        #[serde(default)]
        shared_frames: bool,
        #[serde(default)]
        cursor_tracking: bool,
        #[serde(default)]
        multi_viewers: bool,
    },
    Cursor {
        hidden: bool,
    },
    SharedFrame {
        frame: SharedFrame,
    },
    Frame {
        png: String,
    },
    Windows {
        windows: Vec<WindowInfo>,
    },
    Layout {
        layout: Vec<WindowLayout>,
    },
    Error {
        message: String,
    },
}
#[derive(Serialize, Deserialize)]
struct SharedFrame {
    width: u32,
    height: u32,
    sequence: u64,
    path: PathBuf,
}
fn shared_frame(frame: &SharedFrame, workspace: &str, local: bool) -> Result<Frame> {
    ensure!(local, "Remote app hosts cannot supply local frame paths");
    ensure!(
        (1..=3840).contains(&frame.width) && (1..=2160).contains(&frame.height),
        "Invalid shared app frame"
    );
    let expected = frame.width as usize * frame.height as usize * 4;
    ensure!(
        frame.path.parent() == Some(directory()?.as_path())
            && frame
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with(&format!("{workspace}.frame-"))),
        "Invalid shared app frame path"
    );
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&frame.path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o077 == 0
            && metadata.len() == expected as u64,
        "Untrusted shared app frame"
    );
    let mut pixels = vec![0; expected];
    file.read_exact(&mut pixels)?;
    Ok(Frame {
        width: frame.width,
        height: frame.height,
        pixels: Arc::new(pixels),
        time: Instant::now(),
    })
}
pub(crate) fn read<T: serde::de::DeserializeOwned>(input: &mut impl Read) -> Result<T> {
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
pub(crate) fn write(value: &impl Serialize, output: &mut impl Write) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX_PACKET, "Desktop packet exceeds limit");
    output.write_all(&(bytes.len() as u32).to_be_bytes())?;
    output.write_all(&bytes)?;
    output.flush()?;
    Ok(())
}
pub(crate) fn directory() -> Result<PathBuf> {
    let path = PathBuf::from(format!("/tmp/starmux-desktops-{}", unsafe {
        libc::geteuid()
    }));
    starkit::terminal_graphics::session::private_root(&path)?;
    Ok(path)
}
pub(crate) fn paths(name: &str) -> Result<(PathBuf, PathBuf)> {
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
pub(crate) fn lock(path: PathBuf) -> Result<fs::File> {
    lock_with_timeout(path, Duration::ZERO)
}
fn lock_with_timeout(path: PathBuf, timeout: Duration) -> Result<fs::File> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    let start = Instant::now();
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(file);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::WouldBlock {
            return Err(error.into());
        }
        ensure!(
            start.elapsed() < timeout,
            "This desktop is starting or has a retained single-viewer worker. Restart that app desktop to enable multiple viewers; its running apps were left intact"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
pub fn internal(mode: &str, name: &str) -> Result<()> {
    crate::app_audio::environment();
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
    let _lease = lock_with_timeout(
        directory()?.join(format!("{name}.attach.lock")),
        Duration::from_secs(15),
    )?;
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
    let hello: Reply = read(&mut stream)?;
    let multi = matches!(
        hello,
        Reply::Hello {
            multi_viewers: true,
            ..
        }
    );
    let _legacy_lease = if multi {
        drop(_lease);
        None
    } else {
        Some(_lease)
    };
    // New workers support fan-out; retained legacy workers keep their lifetime lease.
    // Do not replace a live pre-upgrade viewer or restart the user's applications.
    write(&hello, &mut std::io::stdout().lock())?;
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
        Request::SharedFrames { .. } | Request::FrameAck { .. } | Request::TrackCursor => {
            unreachable!("Frame transport is handled by the app worker")
        }
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
// A slow viewer owns bounded queues and never blocks the compositor or peers.
struct Viewer {
    socket: UnixStream,
    controls: Sender<Arc<Vec<u8>>>,
    frames: Sender<Arc<Vec<u8>>>,
    stale_frames: Receiver<Arc<Vec<u8>>>,
    stopped: Arc<AtomicBool>,
    shared: bool,
    track_cursor: bool,
    settings: Settings,
    last_frame: Option<Arc<Vec<u8>>>,
    shared_flight: Option<(u64, Arc<tempfile::NamedTempFile>)>,
}
fn packet(reply: &impl Serialize) -> Result<Arc<Vec<u8>>> {
    let mut bytes = vec![];
    write(reply, &mut bytes)?;
    Ok(Arc::new(bytes))
}
impl Viewer {
    fn new(socket: UnixStream) -> Result<Self> {
        let mut writer = socket.try_clone()?;
        writer.set_write_timeout(Some(Duration::from_millis(500)))?;
        let (controls, rx) = bounded::<Arc<Vec<u8>>>(16);
        let (frames, frame_rx) = bounded::<Arc<Vec<u8>>>(1);
        let stopped = Arc::new(AtomicBool::new(false));
        let done = stopped.clone();
        let stale_frames = frame_rx.clone();
        std::thread::spawn(move || {
            loop {
                let next =
                    crossbeam_channel::select_biased! { recv(rx)->v=>v,recv(frame_rx)->v=>v };
                match next {
                    Ok(bytes) if writer.write_all(&bytes).is_ok() => {}
                    _ => break,
                }
            }
            done.store(true, Ordering::Release);
            let _ = writer.shutdown(std::net::Shutdown::Both);
        });
        Ok(Self {
            socket,
            controls,
            frames,
            stale_frames,
            stopped,
            shared: false,
            track_cursor: false,
            settings: Settings {
                width: 1280,
                height: 720,
                fps: 60,
            },
            last_frame: None,
            shared_flight: None,
        })
    }
    fn control(&self, bytes: Arc<Vec<u8>>) {
        if self.controls.try_send(bytes).is_err() {
            self.stopped.store(true, Ordering::Release);
        }
    }
    fn frame(&self, bytes: Arc<Vec<u8>>) {
        if self.frames.is_full() {
            let _ = self.stale_frames.try_recv();
        }
        let _ = self.frames.try_send(bytes);
    }
}
impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
    }
}
fn viewer_settings(clients: &BTreeMap<u64, Viewer>) -> Settings {
    let remote = clients.values().any(|v| !v.shared);
    Settings {
        width: clients
            .values()
            .map(|v| v.settings.width)
            .max()
            .unwrap_or(1280)
            .min(if remote { 2560 } else { 3840 }),
        height: clients
            .values()
            .map(|v| v.settings.height)
            .max()
            .unwrap_or(720)
            .min(if remote { 1440 } else { 2160 }),
        fps: 60,
    }
}
fn worker(name: &str) -> Result<()> {
    let (socket, stop) = paths(name)?;
    let _lease = lock(directory()?.join(format!("{name}.worker.lock")))?;
    let _ = fs::remove_file(&socket);
    let _ = fs::remove_file(&stop);
    // Keep app output isolated on its host, even when the viewer is muted or absent.
    if let Err(error) = crate::app_audio::ensure(name, std::process::id()) {
        eprintln!("App audio unavailable: {error:#}");
    }
    let desktop = Desktop::start_with_audio(
        Settings {
            width: 1280,
            height: 720,
            fps: 60,
        },
        Some(crate::app_audio::sink_name(name)),
    );
    desktop.send(DesktopCommand::RelayAudio(true));
    desktop.send(DesktopCommand::InputBurst(false));
    serve_desktop(name, desktop, socket, stop)
}
// Transport can be verified with synthetic frames, without a compositor or audio device.
fn serve_desktop(name: &str, desktop: Desktop, socket: PathBuf, stop: PathBuf) -> Result<()> {
    let control_path = directory()?.join(format!("{name}.control.sock"));
    let _ = fs::remove_file(&control_path);
    let control_listener = UnixListener::bind(&control_path)?;
    control_listener.set_nonblocking(true)?;
    let mut control_launches: Vec<ControlLaunch> = vec![];
    let listener = UnixListener::bind(&socket)?;
    listener.set_nonblocking(true)?;
    let (tx, rx) = bounded::<(u64, Option<Request>)>(128);
    let mut clients: BTreeMap<u64, Viewer> = BTreeMap::new();
    let mut input_owner = None;
    let mut generation = 0;
    let mut windows = vec![];
    let mut layout = vec![];

    let mut cursor_hidden = false;
    let mut requested_settings = Settings {
        width: 1280,
        height: 720,
        fps: 60,
    };
    let mut sequence = 0;

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
            if clients.len() >= 16 {
                continue;
            }
            let handshake = (|| -> Result<()> {
                write(
                    &Reply::Hello {
                        version: VERSION,
                        pid: std::process::id(),
                        shared_frames: true,
                        cursor_tracking: true,
                        multi_viewers: true,
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
            clients.insert(id, Viewer::new(stream)?);
        }
        for (id, request) in rx.try_iter() {
            let Some(viewer) = clients.get_mut(&id) else {
                continue;
            };
            if let Some(request) = request {
                match request {
                    Request::SharedFrames { enabled } => {
                        viewer.shared = enabled;
                        viewer.shared_flight = None;
                        viewer.last_frame = None;
                    }
                    Request::Resize { settings } => {
                        if settings.validate().is_ok()
                            && settings.width <= 3840
                            && settings.height <= 2160
                        {
                            viewer.settings = settings;
                        }
                    }
                    Request::TrackCursor => {
                        viewer.track_cursor = true;
                        viewer.control(packet(&Reply::Cursor {
                            hidden: cursor_hidden,
                        })?);
                    }
                    Request::FrameAck { sequence } => {
                        if viewer
                            .shared_flight
                            .as_ref()
                            .is_some_and(|(n, _)| *n == sequence)
                        {
                            viewer.shared_flight = None;
                        }
                    }
                    Request::Input {
                        input: Input::Release,
                    } => {
                        if input_owner == Some(id) {
                            desktop.send(DesktopCommand::Input(Input::Release));
                            input_owner = None;
                        }
                    }
                    request => {
                        if matches!(&request,Request::Launch{launch} if launch.command.len()>8192||launch.command.contains('\0'))
                        {
                            continue;
                        }
                        if matches!(request, Request::Input { .. } | Request::Focus { .. }) {
                            if input_owner != Some(id) {
                                desktop.send(DesktopCommand::Input(Input::Release));
                            }
                            input_owner = Some(id);
                        }
                        // Restoring a saved layout is only meaningful before live windows exist.
                        if !matches!(request, Request::Restore { .. }) || windows.is_empty() {
                            desktop.send(desktop_command(request));
                        }
                    }
                }
            } else {
                clients.remove(&id);
                if input_owner == Some(id) {
                    desktop.send(DesktopCommand::Input(Input::Release));
                    input_owner = None;
                }
            }
        }
        clients.retain(|_, v| !v.stopped.load(Ordering::Acquire));
        if input_owner.is_some_and(|id| !clients.contains_key(&id)) {
            desktop.send(DesktopCommand::Input(Input::Release));
            input_owner = None;
        }
        let settings = viewer_settings(&clients);
        if !clients.is_empty() && settings != requested_settings {
            desktop.send(DesktopCommand::Resize(settings.clone()));
            requested_settings = settings;
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
                DesktopEvent::CursorHidden(hidden) => {
                    cursor_hidden = hidden;
                    Some(Reply::Cursor { hidden })
                }
                DesktopEvent::Error(message) => Some(Reply::Error { message }),
                _ => None,
            };
            if let Some(reply) = reply {
                let bytes = packet(&reply)?;
                for viewer in clients.values() {
                    if !matches!(reply, Reply::Cursor { .. }) || viewer.track_cursor {
                        viewer.control(bytes.clone());
                    }
                }
            }
        }
        control_launches.retain_mut(|pending|{
            let added=windows.iter().filter(|w|!pending.existing.contains(&w.id)).cloned().collect::<Vec<_>>();
            if !added.is_empty(){let _=write(&Reply::Windows{windows:added},&mut pending.stream);return false}
            if pending.started.elapsed()>Duration::from_secs(20){let _=write(&Reply::Error{message:"No new app window appeared within 20 seconds. Check the command, cartridge and app-host runtime.".into()},&mut pending.stream);return false}true
        });
        // The compositor owns the frame clock. Forward each new frame promptly;
        // sampling it with another 60 Hz clock loses frames when either wakes late.
        if let Some(frame) = desktop.latest.lock().unwrap().clone() {
            let mut remote_packet = None;
            let mut shared_packet = None;
            for viewer in clients.values_mut() {
                if (viewer.shared && viewer.shared_flight.is_some())
                    || viewer
                        .last_frame
                        .as_ref()
                        .is_some_and(|p| Arc::ptr_eq(p, &frame.pixels))
                {
                    continue;
                }
                let bytes = if viewer.shared {
                    if shared_packet.is_none() {
                        let mut file = tempfile::Builder::new()
                            .prefix(&format!("{name}.frame-"))
                            .tempfile_in(directory()?)?;
                        file.write_all(&frame.pixels)?;
                        sequence += 1;
                        let descriptor = SharedFrame {
                            width: frame.width,
                            height: frame.height,
                            sequence,
                            path: file.path().into(),
                        };
                        shared_packet = Some((
                            packet(&Reply::SharedFrame { frame: descriptor })?,
                            Arc::new(file),
                        ));
                    }
                    let (bytes, file) = shared_packet.as_ref().unwrap();
                    viewer.shared_flight = Some((sequence, file.clone()));
                    bytes.clone()
                } else {
                    if remote_packet.is_none() {
                        remote_packet = Some(packet(&Reply::Frame {
                            png: encode(&frame)?,
                        })?);
                    }
                    remote_packet.as_ref().unwrap().clone()
                };
                viewer.frame(bytes);
                viewer.last_frame = Some(frame.pixels.clone());
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(desktop);
    let _ = fs::remove_file(control_path);
    let _ = fs::remove_file(socket);
    let _ = fs::remove_file(stop);
    Ok(())
}
fn command(config: &Config, mode: &str) -> Result<Command> {
    command_extra(config, mode, &[])
}
pub(crate) fn command_extra(config: &Config, mode: &str, extra: &[&str]) -> Result<Command> {
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
        cmd.arg(shell_words::join(
            [config.desktop_helper.as_str(), mode, &config.workspace]
                .into_iter()
                .chain(extra.iter().copied()),
        ));
        Ok(cmd)
    } else {
        let mut cmd = Command::new(std::env::current_exe()?);
        cmd.args([mode, &config.workspace]).args(extra);
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
    pub shared_frames: Arc<AtomicBool>,
    pub cursor_hidden: Arc<AtomicBool>,
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
        let cursor_hidden = Arc::new(AtomicBool::new(false));
        let cursor_state = cursor_hidden.clone();
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
        let shared_frames = Arc::new(AtomicBool::new(false));
        let shared_capable = shared_frames.clone();
        let local = config.host.is_none();
        let workspace = config.workspace.clone();
        let frame_commands = tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                match read::<Reply>(&mut output)? {
                    Reply::Hello {
                        version,
                        pid,
                        shared_frames,
                        cursor_tracking,
                        ..
                    } => {
                        ensure!(version == VERSION, "Incompatible desktop helper version");
                        *worker_pid.lock().unwrap() = Some(pid);
                        if local && shared_frames {
                            frame_commands.send(Request::SharedFrames { enabled: true })?;
                            shared_capable.store(true, Ordering::Release);
                        }
                        if cursor_tracking {
                            frame_commands.send(Request::TrackCursor)?;
                        }
                        connection.store(true, Ordering::Release)
                    }
                    _ => anyhow::bail!("Invalid desktop helper handshake"),
                };
                loop {
                    match read::<Reply>(&mut output)? {
                        Reply::Cursor { hidden } => cursor_state.store(hidden, Ordering::Release),
                        Reply::Frame { png } => *frames.lock().unwrap() = Some(decode(&png)?),
                        Reply::SharedFrame { frame } => {
                            let pixels = shared_frame(&frame, &workspace, local)?;
                            *frames.lock().unwrap() = Some(pixels);
                            frame_commands.send_timeout(
                                Request::FrameAck {
                                    sequence: frame.sequence,
                                },
                                Duration::from_millis(250),
                            )?;
                        }
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
            cursor_hidden,
            latest,
            events: erx,
            connected,
            pid,
            tx,
            child,
            shared_frames,
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
    fn concurrent_viewers_wait_for_startup_instead_of_rejecting_each_other() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("startup.lock");
        let first = lock(path.clone()).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            drop(first);
        });
        let second = lock_with_timeout(path, Duration::from_secs(1)).unwrap();
        drop(second);
        release.join().unwrap();
    }
    #[test]
    fn desktop_transport_serves_multiple_viewers_without_a_display_or_audio_device() {
        let name = format!(
            "view-proof-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        );
        let (socket, stop) = paths(&name).unwrap();
        let (commands, command_rx) = crossbeam_channel::unbounded();
        let (events, event_rx) = crossbeam_channel::unbounded();
        let latest = Arc::new(Mutex::new(None));
        let desktop = Desktop::test_transport(event_rx, commands, latest.clone());
        let identity = name.clone();
        let endpoint = socket.clone();
        let quit = stop.clone();
        let worker = std::thread::spawn(move || serve_desktop(&identity, desktop, endpoint, quit));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !socket.exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let connect = || {
            let mut peer = UnixStream::connect(&socket).unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            assert!(matches!(
                read::<Reply>(&mut peer).unwrap(),
                Reply::Hello {
                    multi_viewers: true,
                    ..
                }
            ));
            for _ in 0..2 {
                read::<Reply>(&mut peer).unwrap();
            }
            write(&Request::SharedFrames { enabled: true }, &mut peer).unwrap();
            peer
        };
        let mut a = connect();
        let mut b = connect();
        // A control round trip ensures both local-frame negotiations were consumed.
        for peer in [&mut a, &mut b] {
            write(&Request::TrackCursor, peer).unwrap();
            assert!(matches!(read::<Reply>(peer).unwrap(), Reply::Cursor { .. }));
        }
        let frame = |color: u8| Frame {
            pixels: Arc::new([color, 2, 3, 255].repeat(32 * 24)),
            width: 32,
            height: 24,
            time: Instant::now(),
        };
        *latest.lock().unwrap() = Some(frame(1));
        let shared = |peer: &mut UnixStream| loop {
            if let Reply::SharedFrame { frame } = read::<Reply>(peer).unwrap() {
                break frame;
            }
        };
        let first_a = shared(&mut a);
        let first_b = shared(&mut b);
        assert_eq!(first_a.path, first_b.path);
        assert!(first_b.path.exists());
        write(
            &Request::FrameAck {
                sequence: first_a.sequence,
            },
            &mut a,
        )
        .unwrap();
        *latest.lock().unwrap() = Some(frame(9));
        let next_a = shared(&mut a);
        assert_ne!(next_a.sequence, first_a.sequence);
        assert!(
            first_b.path.exists(),
            "One viewers ACK freed another viewers frame"
        );
        // A remote viewer receives the same current pixels through the bounded PNG path.
        let mut remote = UnixStream::connect(&socket).unwrap();
        remote
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        for _ in 0..3 {
            read::<Reply>(&mut remote).unwrap();
        }
        let remote_pixels = loop {
            if let Reply::Frame { png } = read::<Reply>(&mut remote).unwrap() {
                break decode(&png).unwrap();
            }
        };
        assert_eq!(remote_pixels.pixels, frame(9).pixels);
        drop(a);
        write(
            &Request::FrameAck {
                sequence: first_b.sequence,
            },
            &mut b,
        )
        .unwrap();
        let next_b = shared(&mut b);
        assert_eq!(
            shared_frame(&next_b, &name, true).unwrap().pixels,
            frame(9).pixels
        );
        events
            .send(DesktopEvent::Windows(vec![WindowInfo {
                id: 7,
                name: "Synthetic app".into(),
                app_id: "fake".into(),
            }]))
            .unwrap();
        loop {
            if let Reply::Windows { windows } = read::<Reply>(&mut b).unwrap() {
                assert_eq!(windows[0].id, 7);
                break;
            }
        }
        loop {
            if let Reply::Windows { windows } = read::<Reply>(&mut remote).unwrap() {
                assert_eq!(windows[0].id, 7);
                break;
            }
        }
        let _ = command_rx.try_iter().count();
        fs::write(&stop, b"stop").unwrap();
        worker.join().unwrap().unwrap();
        assert!(!socket.exists());
    }
    #[test]
    fn a_blocked_app_viewer_does_not_block_other_viewers() {
        let (slow_socket, _slow_peer) = UnixStream::pair().unwrap();
        let (fast_socket, mut fast_peer) = UnixStream::pair().unwrap();
        fast_peer
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let slow = Viewer::new(slow_socket).unwrap();
        let fast = Viewer::new(fast_socket).unwrap();
        let frame = packet(&Reply::Frame {
            png: "x".repeat(2_000_000),
        })
        .unwrap();
        for _ in 0..8 {
            slow.frame(frame.clone());
        }
        fast.control(packet(&Reply::Cursor { hidden: true }).unwrap());
        assert!(matches!(
            read::<Reply>(&mut fast_peer).unwrap(),
            Reply::Cursor { hidden: true }
        ));
        drop(slow);
        fast.control(packet(&Reply::Cursor { hidden: false }).unwrap());
        assert!(matches!(
            read::<Reply>(&mut fast_peer).unwrap(),
            Reply::Cursor { hidden: false }
        ));
    }
    #[test]
    fn shared_frame_lease_survives_another_viewers_ack() {
        let (a, _pa) = UnixStream::pair().unwrap();
        let (b, _pb) = UnixStream::pair().unwrap();
        let mut a = Viewer::new(a).unwrap();
        let mut b = Viewer::new(b).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_owned();
        let file = Arc::new(file);
        a.shared_flight = Some((1, file.clone()));
        b.shared_flight = Some((1, file));
        a.shared_flight = None;
        assert!(path.exists());
        b.shared_flight = None;
        assert!(!path.exists());
    }
    #[test]
    fn cursor_tracking_is_optional_for_retained_legacy_workers() {
        let legacy: Reply = serde_json::from_value(
            serde_json::json!({"type":"hello","version":1,"pid":123,"shared_frames":true}),
        )
        .unwrap();
        assert!(matches!(
            legacy,
            Reply::Hello {
                cursor_tracking: false,
                ..
            }
        ));
        let current: Reply = serde_json::from_value(serde_json::json!({"type":"hello","version":1,"pid":123,"shared_frames":true,"cursor_tracking":true})).unwrap();
        assert!(matches!(
            current,
            Reply::Hello {
                cursor_tracking: true,
                ..
            }
        ));
        assert!(matches!(
            serde_json::from_slice::<Reply>(
                &serde_json::to_vec(&Reply::Cursor { hidden: true }).unwrap()
            )
            .unwrap(),
            Reply::Cursor { hidden: true }
        ));
    }
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

#[cfg(test)]
mod shared_tests {
    use super::*;
    #[test]
    fn immutable_app_frames_require_a_local_private_workspace_file() {
        let mut file = tempfile::Builder::new()
            .prefix("frame-test.frame-")
            .tempfile_in(directory().unwrap())
            .unwrap();
        let bytes: Vec<u8> = (0..64).collect();
        file.write_all(&bytes).unwrap();
        let frame = SharedFrame {
            width: 4,
            height: 4,
            sequence: 1,
            path: file.path().into(),
        };
        assert!(shared_frame(&frame, "frame-test", false).is_err());
        assert!(shared_frame(&frame, "another-project", true).is_err());
        assert_eq!(
            shared_frame(&frame, "frame-test", true)
                .unwrap()
                .pixels
                .as_slice(),
            bytes
        );
    }
}
