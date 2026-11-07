//! Retained native PTYs behind a private local control socket. A controller's
//! connection owns neither the PTY nor the source parser. Not yet selected by
//! the application's production backend or capability advertisement.
use crate::{
    graphics_decode::{Completion, Pool},
    graphics_owner::Owner,
    graphics_resources::{Budget, Lease, Resources},
    graphics_store::{Generation, Geometry},
    graphics_transfer::Error,
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::{
            fs::{MetadataExt, OpenOptionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
#[path = "source_broker/render.rs"]
mod render;
#[path = "source_broker/service.rs"]
pub mod service;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
const INPUT: usize = 1024 * 1024;
const REQUEST: usize = 2 * 1024 * 1024;
const OUTPUT: usize = 4 * 1024 * 1024;
const PTY_QUEUE: usize = 2 * 1024 * 1024;
const SOURCES: usize = 1024;
const CLIENTS: usize = 16;
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub id: String,
    pub epoch: String,
    pub boot: String,
    pub pid: u32,
    pub start_ticks: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub rows: u16,
    pub columns: u16,
    pub cell: [u16; 2],
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub ansi: [[u8; 3]; 16],
}
impl Palette {
    fn color(&self, color: vt100::Color, foreground: bool) -> [u8; 3] {
        match color {
            vt100::Color::Default => {
                if foreground {
                    self.foreground
                } else {
                    self.background
                }
            }
            vt100::Color::Rgb(r, g, b) => [r, g, b],
            vt100::Color::Idx(i) if i < 16 => self.ansi[usize::from(i)],
            vt100::Color::Idx(i) if i >= 232 => [8 + (i - 232) * 10; 3],
            vt100::Color::Idx(i) => {
                let n = i - 16;
                let level = |v| if v == 0 { 0 } else { 55 + 40 * v };
                [level(n / 36), level(n / 6 % 6), level(n % 6)]
            }
        }
    }
}
impl Size {
    fn geometry(&self) -> Result<Geometry> {
        ensure!(
            (1..=192).contains(&self.rows)
                && (1..=512).contains(&self.columns)
                && self.cell.iter().all(|n| (1..=256).contains(n)),
            "Invalid source geometry"
        );
        ensure!(
            u32::from(self.rows) * u32::from(self.cell[1]) <= u32::from(u16::MAX)
                && u32::from(self.columns) * u32::from(self.cell[0]) <= u32::from(u16::MAX),
            "Source pixel geometry exceeds kernel PTY bounds"
        );
        Ok(Geometry {
            rows: self.rows,
            columns: self.columns,
            cell: self.cell,
        })
    }
    fn pty(&self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.columns,
            pixel_width: self.columns.saturating_mul(self.cell[0]),
            pixel_height: self.rows.saturating_mul(self.cell[1]),
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub id: String,
    pub epoch: String,
    pub argv: Vec<String>,
    pub directory: PathBuf,
    pub size: Size,
    pub palette: Palette,
}
impl Launch {
    fn validate_shape(&self) -> Result<()> {
        canonical_uuid(&self.id)?;
        canonical_uuid(&self.epoch)?;
        self.size.geometry()?;
        ensure!(
            !self.argv.is_empty()
                && self.argv.len() <= 64
                && self
                    .argv
                    .iter()
                    .all(|a| a.len() <= 4096 && !a.contains('\0'))
                && self.argv.iter().map(String::len).sum::<usize>() <= 32768,
            "Invalid source command"
        );
        ensure!(
            self.directory.is_absolute(),
            "Source directory must be absolute"
        );
        Ok(())
    }
    fn validate(&self) -> Result<()> {
        self.validate_shape()?;
        ensure!(self.directory.is_dir(), "Source directory is unavailable");
        Ok(())
    }
    fn digest(&self) -> Result<String> {
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Hello,
    Open {
        launch: Launch,
    },
    Inspect {
        source: Reference,
    },
    List,
    Input {
        source: Reference,
        data: String,
    },
    Resize {
        source: Reference,
        size: Size,
    },
    Cells {
        source: Reference,
        size: Size,
        palette: Palette,
    },
    Watch {
        source: Reference,
        view: String,
        epoch: u64,
        generation: u64,
        focused: bool,
        size: Size,
        palette: Palette,
    },
    Ack {
        view: String,
        epoch: u64,
        sequence: u64,
    },
    Unwatch {
        view: String,
        epoch: u64,
    },
    Close {
        source: Reference,
    },
    Shutdown {
        instance: String,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u8,
    pub id: u64,
    pub token: String,
    pub action: Action,
}
fn canonical_uuid(value: &str) -> Result<uuid::Uuid> {
    let id = uuid::Uuid::parse_str(value)?;
    ensure!(id.to_string() == value, "Noncanonical source identity");
    Ok(id)
}
fn start_ticks(pid: u32) -> Result<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    stat.rsplit_once(')')
        .context("Process identity unavailable")?
        .1
        .split_whitespace()
        .nth(19)
        .context("Process start unavailable")?
        .parse()
        .map_err(Into::into)
}
fn nonblocking(fd: RawFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    ensure!(flags >= 0, "Cannot inspect descriptor flags");
    ensure!(
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
        "Cannot set nonblocking source I/O"
    );
    Ok(())
}
fn source_environment(command: &mut CommandBuilder) {
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    for name in [
        "TMUX",
        "TMUX_PANE",
        "KITTY_PID",
        "KITTY_WINDOW_ID",
        "KITTY_LISTEN_ON",
        "TERM_PROGRAM",
        "TERM_PROGRAM_VERSION",
        "WINDOWID",
        "STARFLUX_PLUGIN_JOB_ID",
        "STARFLUX_PLUGIN_PANE_ID",
    ] {
        command.env_remove(name);
    }
    // Match the packaged host helper: application-private media tools and
    // configuration must not shadow the project's host tools.
    if let Some(path) = command
        .get_env("STARFLUX_SYSTEM_PATH")
        .map(std::ffi::OsStr::to_owned)
    {
        command.env("PATH", path);
        for name in [
            "STARFLUX_SYSTEM_PATH",
            "GST_PLUGIN_SYSTEM_PATH_1_0",
            "GST_PLUGIN_SCANNER",
            "GST_REGISTRY",
            "ALSA_CONFIG_DIR",
            "ALSA_CONFIG_PATH",
            "ALSA_PLUGIN_DIR",
        ] {
            command.env_remove(name);
        }
    }
}
struct Source {
    reference: Reference,
    digest: String,
    owner: Owner,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    waiting: Option<Vec<u8>>,
    out: VecDeque<PendingWrite>,
    queued: usize,
    io_limits: IoLimits,
    eof: bool,
    input_failed: bool,
    pidfd: OwnedFd,
    stopping_at: Option<Instant>,
    kill_sent: bool,
    exit: bool,
    error: Option<String>,
    size: Size,
    revision: u64,
}
struct StartingChild(Option<Box<dyn Child + Send + Sync>>);
impl Drop for StartingChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}
struct PendingWrite {
    bytes: Vec<u8>,
    offset: usize,
    _memory: Lease,
}
#[derive(Clone)]
struct IoLimits {
    input: Budget,
    replies: Budget,
}
impl Source {
    fn launch(
        spec: &Launch,
        boot: &str,
        receipts: &Path,
        resources: Resources,
        io_limits: IoLimits,
    ) -> Result<Self> {
        spec.validate()?;
        let digest = spec.digest()?;
        // A consumed ID is never replayed after daemon loss, failed spawn or an
        // uncertain response. A new explicit launch needs a fresh source ID.
        let receipt = receipts.join(format!("{}.json", spec.id));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(receipt)
            .context(
                "Source launch already consumed; reconnect or explicitly create a fresh source",
            )?;
        file.write_all(&serde_json::to_vec(&json!({"version":1,"id":spec.id,"epoch":spec.epoch,"digest":digest,"boot":boot,"broker_pid":std::process::id()}))?)?;
        file.sync_all()?;
        fs::File::open(receipts)?.sync_all()?;
        let pair = native_pty_system().openpty(spec.size.pty())?;
        let fd = pair
            .master
            .as_raw_fd()
            .context("Source platform has no native PTY descriptor")?;
        nonblocking(fd)?;
        let mut command = CommandBuilder::new(&spec.argv[0]);
        command.args(&spec.argv[1..]);
        command.cwd(&spec.directory);
        source_environment(&mut command);
        let mut child = StartingChild(Some(pair.slave.spawn_command(command)?));
        drop(pair.slave);
        let pid = child
            .0
            .as_ref()
            .unwrap()
            .process_id()
            .context("Source PID unavailable")?;
        let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        ensure!(pidfd >= 0, "Native source ownership requires pidfd support");
        let pidfd = unsafe { OwnedFd::from_raw_fd(pidfd as RawFd) };
        let reference = Reference {
            id: spec.id.clone(),
            epoch: spec.epoch.clone(),
            boot: boot.into(),
            pid,
            start_ticks: start_ticks(pid)?,
        };
        let mut owner = Owner::new(
            Generation {
                host: "local".into(),
                boot: canonical_uuid(boot)?,
                pid,
                start_ticks: reference.start_ticks,
                epoch: canonical_uuid(&spec.epoch)?,
            },
            spec.size.geometry()?,
            resources,
        )
        .map_err(|e| anyhow::anyhow!("Source parser: {e:?}"))?;
        owner.set_palette([spec.palette.foreground, spec.palette.background]);
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        Ok(Self {
            reference,
            digest,
            owner,
            master: pair.master,
            child: child.0.take().unwrap(),
            reader,
            writer,
            waiting: None,
            out: VecDeque::new(),
            queued: 0,
            io_limits,
            eof: false,
            input_failed: false,
            pidfd,
            stopping_at: None,
            kill_sent: false,
            exit: false,
            error: None,
            size: spec.size.clone(),
            revision: 0,
        })
    }
    fn guard(&self, reference: &Reference) -> Result<()> {
        ensure!(
            *reference == self.reference,
            "Source incarnation changed; action rejected"
        );
        Ok(())
    }
    fn info(&self) -> Value {
        json!({"source":self.reference,"launch_digest":self.digest,"alive":!self.exit,"exited":self.exit,"closing":self.stopping_at.is_some(),"eof":self.eof,"input_available":!self.exit&&!self.eof&&!self.input_failed&&self.stopping_at.is_none(),"input_modes":self.modes(),"error":self.error,"size":self.size,"revision":self.revision,"graphics_usage":self.owner.store().usage(),"buffered_output":self.owner.buffered(),"queued_input":self.queued})
    }
    fn modes(&self) -> Value {
        let screen = self.owner.screen();
        json!({"bracketed_paste":screen.bracketed_paste(),"application_cursor":screen.application_cursor(),"application_keypad":screen.application_keypad(),"mouse":format!("{:?}",screen.mouse_protocol_mode()),"mouse_encoding":format!("{:?}",screen.mouse_protocol_encoding())})
    }
    fn pollfd(&self) -> libc::pollfd {
        libc::pollfd {
            fd: if self.eof && self.out.is_empty() {
                -1
            } else {
                self.master.as_raw_fd().unwrap()
            },
            events: if !self.eof && self.waiting.is_none() {
                libc::POLLIN
            } else {
                0
            } | if !self.out.is_empty() {
                libc::POLLOUT
            } else {
                0
            },
            revents: 0,
        }
    }
    fn step(&mut self, events: i16) -> Result<()> {
        let previous_status = (self.exit, self.eof, self.input_failed);
        self.exit = self.child.try_wait()?.is_some();
        if !self.exit
            && !self.kill_sent
            && self
                .stopping_at
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(200))
        {
            self.signal(libc::SIGKILL)?;
            self.kill_sent = true;
        }
        if self.waiting.is_none()
            && !self.eof
            && events & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0
        {
            let mut bytes = [0; 8192];
            match self.reader.read(&mut bytes) {
                Ok(0) => self.eof = true,
                Ok(n) => self.waiting = Some(bytes[..n].to_vec()),
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.raw_os_error() == Some(libc::EIO) => self.eof = true,
                Err(e) => {
                    self.eof = true;
                    return Err(e.into());
                }
            }
        }
        if let Some(bytes) = &self.waiting {
            match self.owner.ingest(bytes) {
                Ok(true) => self.waiting = None,
                Ok(false) => (),
                Err(e) => {
                    self.error = Some(format!("Source parser unavailable: {e:?}"));
                    self.waiting = None;
                }
            }
        }
        match self.owner.pump(8192, Instant::now()) {
            Ok(n) if n > 0 => self.revision = self.revision.saturating_add(1),
            Ok(_) => (),
            Err(e) => self.error = Some(format!("Source parser unavailable: {e:?}")),
        }
        if self.queued + 8192 <= PTY_QUEUE
            && self.out.len() < 256
            && let Some(mut memory) = self.io_limits.replies.reserve(16384)
        {
            let bytes = self.owner.drain_replies(8192);
            if !bytes.is_empty() {
                memory.shrink_to(bytes.capacity());
                self.queued += bytes.len();
                self.out.push_back(PendingWrite {
                    bytes,
                    offset: 0,
                    _memory: memory,
                });
            }
        }
        if events & libc::POLLOUT != 0 && !self.out.is_empty() {
            let message = self.out.front_mut().unwrap();
            let end = (message.offset + 8192).min(message.bytes.len());
            match self.writer.write(&message.bytes[message.offset..end]) {
                Ok(0) => {
                    self.error = Some("Source input closed".into());
                    self.input_failed = true;
                    self.out.clear();
                    self.queued = 0;
                }
                Ok(n) => {
                    message.offset += n;
                    self.queued -= n;
                    if message.offset == message.bytes.len() {
                        self.out.pop_front();
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => {
                    self.error = Some(format!("Source input unavailable: {e}"));
                    self.out.clear();
                    self.queued = 0;
                    self.input_failed = true;
                }
            }
        }
        if previous_status != (self.exit, self.eof, self.input_failed) {
            self.revision = self.revision.saturating_add(1);
        }
        Ok(())
    }
    fn signal(&self, signal: i32) -> Result<()> {
        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.pidfd.as_raw_fd(),
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if result < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        self.exit = self.child.try_wait()?.is_some();
        if !self.exit && self.stopping_at.is_none() {
            self.signal(libc::SIGHUP)?;
        }
        self.stopping_at.get_or_insert_with(Instant::now);
        Ok(())
    }
}
struct Peer {
    socket: UnixStream,
    input: Vec<u8>,
    out: VecDeque<render::Packet>,
    closed: bool,
    input_closed: bool,
    last: Instant,
    view_epochs: BTreeMap<String, u64>,
}
struct SocketGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
}
struct Opening {
    digest: String,
    waiters: Vec<(u64, u64)>,
}
struct Opened {
    id: String,
    result: std::result::Result<Source, String>,
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.dev() == self.device && m.ino() == self.inode)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
struct Rendering {
    peer: u64,
    source: Reference,
    cancelled: Arc<AtomicBool>,
    target: render::Target,
}
struct Watch {
    source: Reference,
    epoch: u64,
    generation: u64,
    focused: bool,
    size: Size,
    palette: Palette,
    sequence: u64,
    revision: Option<u64>,
    pending: Option<u64>,
    awaiting: bool,
    base: Option<(u64, Arc<render::Picture>)>,
    sent: Option<Arc<render::Picture>>,
    retry_at: Instant,
    error: Option<String>,
}
pub struct Broker {
    listener: UnixListener,
    _socket: SocketGuard,
    receipts: PathBuf,
    token: String,
    instance: String,
    boot: String,
    resources: Resources,
    io_limits: IoLimits,
    pool: Pool,
    sources: BTreeMap<String, Source>,
    peers: BTreeMap<u64, Peer>,
    next_peer: u64,
    inflight: BTreeMap<u64, String>,
    rotation: usize,
    stopping: bool,
    open_jobs: crossbeam_channel::Sender<Launch>,
    opened: crossbeam_channel::Receiver<Opened>,
    opening: BTreeMap<String, Opening>,
    renderer: render::Renderer,
    renders: BTreeMap<u64, Rendering>,
    watches: BTreeMap<(u64, String), Watch>,
    next_render: u64,
    transport: Budget,
}
impl Broker {
    pub fn bind(socket: &Path, receipts: &Path, token: String) -> Result<Self> {
        ensure!(
            socket.is_absolute() && receipts.is_absolute(),
            "Broker paths must be absolute"
        );
        ensure!(
            token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid broker token"
        );
        starkit::terminal_graphics::session::private_root(
            socket.parent().context("Socket parent unavailable")?,
        )?;
        starkit::terminal_graphics::session::private_root(receipts)?;
        let listener = service::bind_socket(socket)
            .context("Broker socket already exists or cannot be created")?;
        listener.set_nonblocking(true)?;
        let metadata = fs::symlink_metadata(socket)?;
        let resources = Resources::new(128 * 1024 * 1024, 512 * 1024 * 1024);
        let pool = Pool::new(resources.clone());
        let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?
            .trim()
            .to_owned();
        let io_limits = IoLimits {
            input: Budget::new(16 * 1024 * 1024),
            replies: Budget::new(16 * 1024 * 1024),
        };
        let (open_jobs, jobs) = crossbeam_channel::bounded::<Launch>(2);
        let (finished, opened) = crossbeam_channel::bounded(4);
        for _ in 0..2 {
            let jobs = jobs.clone();
            let finished = finished.clone();
            let boot = boot.clone();
            let receipts = receipts.to_owned();
            let resources = resources.clone();
            let io_limits = io_limits.clone();
            std::thread::spawn(move || {
                for launch in jobs {
                    let result = Source::launch(
                        &launch,
                        &boot,
                        &receipts,
                        resources.clone(),
                        io_limits.clone(),
                    )
                    .map_err(|e| format!("{e:#}"));
                    if finished
                        .send(Opened {
                            id: launch.id,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
        let transport = Budget::new(64 * 1024 * 1024);
        let renderer = render::Renderer::new(resources.clone(), transport.clone());
        Ok(Self {
            listener,
            _socket: SocketGuard {
                path: socket.into(),
                device: metadata.dev(),
                inode: metadata.ino(),
            },
            receipts: receipts.into(),
            token,
            instance: uuid::Uuid::new_v4().to_string(),
            boot,
            resources,
            pool,
            io_limits,
            sources: BTreeMap::new(),
            peers: BTreeMap::new(),
            next_peer: 0,
            inflight: BTreeMap::new(),
            rotation: 0,
            stopping: false,
            open_jobs,
            opened,
            opening: BTreeMap::new(),
            renderer,
            renders: BTreeMap::new(),
            watches: BTreeMap::new(),
            next_render: 0,
            transport,
        })
    }
    fn authenticated(&self, token: &str) -> bool {
        token.len() == self.token.len()
            && token
                .bytes()
                .zip(self.token.bytes())
                .fold(0_u8, |a, (b, c)| a | (b ^ c))
                == 0
    }
    fn source(&mut self, reference: &Reference) -> Result<&mut Source> {
        let source = self
            .sources
            .get_mut(&reference.id)
            .context("Source is not retained by this broker")?;
        source.guard(reference)?;
        Ok(source)
    }
    fn action(&mut self, action: Action) -> Result<Value> {
        if self.stopping {
            ensure!(
                matches!(
                    &action,
                    Action::Hello
                        | Action::List
                        | Action::Inspect { .. }
                        | Action::Cells { .. }
                        | Action::Shutdown { .. }
                ),
                "Broker is stopping; action rejected"
            );
        }
        match action {
            Action::Hello => Ok(
                json!({"version":1,"instance":self.instance,"boot":self.boot,"broker_pid":std::process::id(),"broker_start_ticks":start_ticks(std::process::id())?,"capabilities":["retained-pty-v1","cells-stream-v1","launch-digest-v1"],"sources":self.sources.len(),"opening":self.opening.len(),"renders":self.renders.len(),"watches":self.watches.len(),"presentation_usage":self.resources.presentation.used(),"transport_usage":self.transport.used(),"io_usage":[self.io_limits.input.used(),self.io_limits.replies.used()]}),
            ),
            Action::List => Ok(json!(
                self.sources.values().map(Source::info).collect::<Vec<_>>()
            )),
            Action::Open { .. } => Err(anyhow::anyhow!(
                "Source opening must use the bounded initializer"
            )),
            Action::Inspect { source } => Ok(self.source(&source)?.info()),
            Action::Input { source, data } => {
                ensure!(
                    data.len() <= INPUT.div_ceil(3) * 4,
                    "Input request exceeds limit"
                );
                let bytes = STANDARD.decode(data)?;
                ensure!(bytes.len() <= INPUT, "Input exceeds limit");
                let source = self.source(&source)?;
                ensure!(
                    !source.exit
                        && !source.eof
                        && !source.input_failed
                        && source.stopping_at.is_none(),
                    "Source terminal input is unavailable"
                );
                ensure!(
                    source.queued + bytes.len() <= PTY_QUEUE - 16384 && source.out.len() < 240,
                    "Source input is busy"
                );
                if !bytes.is_empty() {
                    let memory = source
                        .io_limits
                        .input
                        .reserve(bytes.capacity())
                        .context("Broker input is busy")?;
                    source.queued += bytes.len();
                    source.out.push_back(PendingWrite {
                        bytes,
                        offset: 0,
                        _memory: memory,
                    });
                }
                Ok(json!({"queued":true}))
            }
            Action::Resize { source, size } => {
                let geometry = size.geometry()?;
                let source = self.source(&source)?;
                ensure!(
                    !source.exit && source.stopping_at.is_none(),
                    "Source terminal has exited or is closing"
                );
                ensure!(
                    source
                        .owner
                        .resize(geometry)
                        .map_err(|e| anyhow::anyhow!("Resize: {e:?}"))?,
                    "Source output is busy; retry resize"
                );
                source.master.resize(size.pty())?;
                source.size = size;
                source.revision = source.revision.saturating_add(1);
                Ok(source.info())
            }
            Action::Cells { .. }
            | Action::Watch { .. }
            | Action::Ack { .. }
            | Action::Unwatch { .. } => Err(anyhow::anyhow!(
                "Presentation must use the bounded renderer"
            )),
            Action::Close { source } => {
                self.source(&source)?.stop()?;
                Ok(json!({"closing":true}))
            }
            Action::Shutdown { instance } => {
                ensure!(instance == self.instance, "Broker incarnation changed");
                for source in self.sources.values_mut() {
                    source.stop()?;
                }
                self.stopping = true;
                Ok(json!({"stopping":true}))
            }
        }
    }
    fn respond(&mut self, peer: u64, request: Request) {
        if request.version != 1 || !self.authenticated(&request.token) {
            self.reply(
                peer,
                request.id,
                Err(anyhow::anyhow!("Invalid broker protocol or authentication")),
            );
            return;
        }
        match request.action {
            Action::Open { launch } => {
                if let Err(error) = self.begin_open(peer, request.id, launch) {
                    self.reply(peer, request.id, Err(error));
                }
            }
            Action::Cells {
                source,
                size,
                palette,
            } => {
                let result = self.begin_render(
                    render::Target::Request {
                        peer,
                        id: request.id,
                    },
                    source,
                    size,
                    palette,
                    None,
                );
                if let Err(e) = result {
                    self.reply(peer, request.id, Err(e));
                }
            }
            Action::Watch {
                source,
                view,
                epoch,
                generation,
                focused,
                size,
                palette,
            } => {
                let result = self.watch(
                    peer, source, view, epoch, generation, focused, size, palette,
                );
                self.reply(peer, request.id, result);
            }
            Action::Ack {
                view,
                epoch,
                sequence,
            } => {
                let sent = self.peers.get(&peer).is_some_and(|p| {
                    !p.out.iter().any(|packet| {
                        packet
                            .watch
                            .as_ref()
                            .is_some_and(|(v, e)| v == &view && *e == epoch)
                    })
                });
                let accepted = self.watches.get_mut(&(peer, view)).is_some_and(|w| {
                    if sent && w.epoch == epoch && w.sequence == sequence && w.awaiting {
                        w.awaiting = false;
                        w.base = w.sent.take().map(|p| (sequence, p));
                        true
                    } else {
                        false
                    }
                });
                self.reply(peer, request.id, Ok(json!({"accepted":accepted})));
            }
            Action::Unwatch { view, epoch } => {
                let key = (peer, view);
                let accepted = self.watches.get(&key).is_some_and(|w| w.epoch == epoch);
                if accepted {
                    self.remove_watch(&key);
                }
                self.reply(peer, request.id, Ok(json!({"accepted":accepted})));
            }
            action => {
                let result = self.action(action);
                self.reply(peer, request.id, result);
            }
        }
    }
    fn begin_render(
        &mut self,
        target: render::Target,
        reference: Reference,
        size: Size,
        palette: Palette,
        base: Option<(u64, Arc<render::Picture>)>,
    ) -> Result<u64> {
        let peer = target.peer();
        ensure!(self.peers.contains_key(&peer), "View disconnected");
        ensure!(
            self.renders.len() < 8 && self.renders.values().filter(|r| r.peer == peer).count() < 4,
            "Presentation queue is busy"
        );
        let resources = self.resources.clone();
        let source = self.source(&reference)?;
        ensure!(source.stopping_at.is_none(), "Source is closing");
        let snapshot = render::Snapshot::capture(source, size, palette, &resources)?;
        self.next_render = self
            .next_render
            .checked_add(1)
            .context("Presentation sequence exhausted")?;
        let tag = self.next_render;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.renderer.submit(render::Job {
            tag,
            target: target.clone(),
            snapshot,
            cancelled: cancelled.clone(),
            base,
        })?;
        self.renders.insert(
            tag,
            Rendering {
                peer,
                source: reference,
                cancelled,
                target,
            },
        );
        Ok(tag)
    }
    #[allow(clippy::too_many_arguments)]
    fn watch(
        &mut self,
        peer: u64,
        source: Reference,
        view: String,
        epoch: u64,
        generation: u64,
        focused: bool,
        size: Size,
        palette: Palette,
    ) -> Result<Value> {
        ensure!(!self.stopping, "Broker is stopping");
        canonical_uuid(&view)?;
        ensure!(
            epoch > 0 && generation > 0,
            "Invalid view epoch or generation"
        );
        let geometry = size.geometry()?;
        let retained = self.source(&source)?;
        ensure!(retained.stopping_at.is_none(), "Source is closing");
        ensure!(
            geometry.rows <= retained.size.rows && geometry.columns <= retained.size.columns,
            "View exceeds the retained grid"
        );
        let p = self.peers.get(&peer).context("View disconnected")?;
        ensure!(
            p.view_epochs.get(&view).is_none_or(|old| epoch > *old),
            "View epoch is stale"
        );
        ensure!(
            p.view_epochs.contains_key(&view) || p.view_epochs.len() < 64,
            "Too many view identities; reconnect transport"
        );
        let key = (peer, view.clone());
        ensure!(
            self.watches.contains_key(&key)
                || (self.watches.len() < 128
                    && self.watches.keys().filter(|(p, _)| *p == peer).count() < 8),
            "Too many watched views"
        );
        self.remove_watch(&key);
        self.peers
            .get_mut(&peer)
            .unwrap()
            .view_epochs
            .insert(view, epoch);
        self.watches.insert(
            key,
            Watch {
                source,
                epoch,
                generation,
                focused,
                size,
                palette,
                sequence: 0,
                revision: None,
                pending: None,
                awaiting: false,
                base: None,
                sent: None,
                retry_at: Instant::now(),
                error: None,
            },
        );
        Ok(json!({"watching":true,"epoch":epoch}))
    }
    fn remove_watch(&mut self, key: &(u64, String)) {
        if let Some(w) = self.watches.remove(key) {
            if let Some(tag) = w.pending
                && let Some(r) = self.renders.remove(&tag)
            {
                r.cancelled.store(true, Ordering::Release);
            }
            if let Some(p) = self.peers.get_mut(&key.0) {
                // A partially written JSON line must finish. Its old epoch lets
                // the client reject it without corrupting subsequent messages.
                p.out.retain(|packet| {
                    packet.offset > 0
                        || packet
                            .watch
                            .as_ref()
                            .is_none_or(|(view, epoch)| view != &key.1 || *epoch != w.epoch)
                });
            }
        }
    }
    fn notify_view(&mut self, peer: u64, event: Value) {
        let Ok(mut bytes) = serde_json::to_vec(&event) else {
            return;
        };
        if bytes.len() > 16384 {
            return;
        }
        bytes.push(b'\n');
        if let Some(memory) = self.transport.reserve(bytes.capacity()) {
            self.enqueue_packet(
                peer,
                render::Packet {
                    bytes,
                    offset: 0,
                    watch: None,
                    _memory: memory,
                },
            );
        } else if let Some(p) = self.peers.get_mut(&peer) {
            p.closed = true;
        }
    }
    fn close_watch(&mut self, key: &(u64, String)) {
        if let Some(w) = self.watches.get(key) {
            let event = json!({"version":1,"event":"source_closed","view":key.1,"epoch":w.epoch,"source":w.source});
            self.remove_watch(key);
            self.notify_view(key.0, event);
        }
    }
    fn enqueue_packet(&mut self, peer: u64, packet: render::Packet) {
        if let Some(p) = self.peers.get_mut(&peer) {
            let queued: usize = p.out.iter().map(|v| v.bytes.len() - v.offset).sum();
            if queued + packet.bytes.len() > OUTPUT || p.out.len() >= 128 {
                p.closed = true;
            } else {
                p.out.push_back(packet);
            }
        }
    }
    fn collect_rendered(&mut self) {
        while let Some(done) = self.renderer.receive() {
            if self.renders.remove(&done.tag).is_none() {
                continue;
            }
            let valid = self
                .sources
                .get(&done.source.id)
                .is_some_and(|s| s.reference == done.source && s.stopping_at.is_none());
            match done.target {
                render::Target::Request { peer, id } => {
                    if !valid {
                        self.reply(
                            peer,
                            id,
                            Err(anyhow::anyhow!("Source changed before rendering finished")),
                        );
                        continue;
                    }
                    match done.result {
                        Ok(rendered) => self.enqueue_packet(peer, rendered.packet),
                        Err(e) => self.reply(peer, id, Err(anyhow::anyhow!(e))),
                    }
                }
                render::Target::Watch {
                    peer,
                    view,
                    epoch,
                    sequence,
                    ..
                } => {
                    let key = (peer, view);
                    let Some(w) = self.watches.get_mut(&key) else {
                        continue;
                    };
                    if w.epoch != epoch || w.pending != Some(done.tag) {
                        continue;
                    }
                    w.pending = None;
                    if !valid {
                        self.close_watch(&key);
                        continue;
                    }
                    match done.result {
                        Ok(rendered) => {
                            w.awaiting = true;
                            w.error = None;
                            w.sent = Some(rendered.picture);
                            self.enqueue_packet(peer, rendered.packet);
                        }
                        Err(error) => self.watch_failed(&key, error),
                    }
                    // Sequence is recorded before dispatch, so stale/duplicate
                    // acknowledgements cannot release credit for another frame.
                    debug_assert!(
                        self.watches
                            .get(&key)
                            .is_none_or(|w| w.sequence == sequence)
                    );
                }
            }
        }
    }
    fn watch_failed(&mut self, key: &(u64, String), error: String) {
        let Some(w) = self.watches.get_mut(key) else {
            return;
        };
        w.revision = None;
        w.retry_at = Instant::now() + Duration::from_millis(250);
        if w.error.as_ref() != Some(&error) {
            w.error = Some(error.clone());
            let event = json!({"version":1,"event":"view_error","view":key.1,"epoch":w.epoch,"source":w.source,"error":error});
            self.notify_view(key.0, event);
        }
    }
    fn schedule_watches(&mut self) {
        let keys = self.watches.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            let w = &self.watches[&key];
            let Some(source) = self
                .sources
                .get(&w.source.id)
                .filter(|s| s.reference == w.source && s.stopping_at.is_none())
            else {
                self.close_watch(&key);
                continue;
            };
            if Instant::now() < w.retry_at
                || w.pending.is_some()
                || w.awaiting
                || w.revision == Some(source.revision)
            {
                continue;
            }
            let revision = source.revision;
            let Some(sequence) = w.sequence.checked_add(1) else {
                self.remove_watch(&key);
                continue;
            };
            let target = render::Target::Watch {
                peer: key.0,
                view: key.1.clone(),
                epoch: w.epoch,
                sequence,
                generation: w.generation,
                focused: w.focused,
            };
            let result = self.begin_render(
                target,
                w.source.clone(),
                w.size.clone(),
                w.palette.clone(),
                w.base.clone(),
            );
            match result {
                Ok(tag) => {
                    let w = self.watches.get_mut(&key).unwrap();
                    w.sequence = sequence;
                    w.pending = Some(tag);
                    w.revision = Some(revision);
                }
                Err(error) => self.watch_failed(&key, format!("{error:#}")),
            }
        }
    }
    fn begin_open(&mut self, peer: u64, request: u64, launch: Launch) -> Result<()> {
        ensure!(!self.stopping, "Broker is stopping; action rejected");
        launch.validate_shape()?;
        let digest = launch.digest()?;
        if let Some(source) = self.sources.get(&launch.id) {
            ensure!(
                source.digest == digest,
                "Source ID already belongs to a different launch"
            );
            let info = source.info();
            self.reply(peer, request, Ok(info));
            return Ok(());
        }
        if let Some(opening) = self.opening.get_mut(&launch.id) {
            ensure!(
                opening.digest == digest,
                "Source ID is opening a different launch"
            );
            ensure!(opening.waiters.len() < 64, "Too many source waiters");
            opening.waiters.push((peer, request));
            return Ok(());
        }
        ensure!(
            self.sources.len() + self.opening.len() < SOURCES,
            "Native source capacity reached"
        );
        let id = launch.id.clone();
        self.open_jobs
            .try_send(launch)
            .map_err(|_| anyhow::anyhow!("Source initializer is busy; retry the same source ID"))?;
        self.opening.insert(
            id,
            Opening {
                digest,
                waiters: vec![(peer, request)],
            },
        );
        Ok(())
    }
    fn collect_opened(&mut self) -> Result<()> {
        while let Ok(opened) = self.opened.try_recv() {
            let Some(opening) = self.opening.remove(&opened.id) else {
                continue;
            };
            match opened.result {
                Ok(mut source) => {
                    if self.stopping {
                        source.stop()?;
                    }
                    let info = source.info();
                    self.sources.insert(opened.id, source);
                    for (peer, id) in opening.waiters {
                        self.reply(peer, id, Ok(info.clone()));
                    }
                }
                Err(error) => {
                    for (peer, id) in opening.waiters {
                        self.reply(peer, id, Err(anyhow::anyhow!(error.clone())));
                    }
                }
            }
        }
        Ok(())
    }
    fn reply(&mut self, peer: u64, id: u64, result: Result<Value>) {
        let mut bytes = serde_json::to_vec(&match result {
            Ok(value) => json!({"version":1,"id":id,"ok":true,"result":value}),
            Err(e) => json!({"version":1,"id":id,"ok":false,"error":format!("{e:#}")}),
        })
        .unwrap();
        bytes.push(b'\n');
        if bytes.len() > OUTPUT {
            bytes = serde_json::to_vec(
                &json!({"version":1,"id":id,"ok":false,"error":"Broker response exceeds limit"}),
            )
            .unwrap();
            bytes.push(b'\n');
        }
        if let Some(peer) = self.peers.get_mut(&peer) {
            let queued: usize = peer.out.iter().map(|p| p.bytes.len() - p.offset).sum();
            if queued + bytes.len() > OUTPUT || peer.out.len() >= 128 {
                peer.closed = true;
            } else if let Some(memory) = self.transport.reserve(bytes.capacity()) {
                peer.out.push_back(render::Packet {
                    bytes,
                    offset: 0,
                    watch: None,
                    _memory: memory,
                });
            } else {
                peer.closed = true;
            }
        }
    }
    fn accept(&mut self) -> Result<()> {
        for _ in 0..CLIENTS {
            let (socket, _) = match self.listener.accept() {
                Ok(v) => v,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            };
            if self.peers.len() >= CLIENTS {
                continue;
            }
            let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
            let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
            if unsafe {
                libc::getsockopt(
                    socket.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PEERCRED,
                    (&mut credentials as *mut libc::ucred).cast(),
                    &mut length,
                )
            } != 0
                || credentials.uid != unsafe { libc::getuid() }
            {
                continue;
            }
            socket.set_nonblocking(true)?;
            self.next_peer = self
                .next_peer
                .checked_add(1)
                .context("Broker peer sequence exhausted")?;
            self.peers.insert(
                self.next_peer,
                Peer {
                    socket,
                    input: Vec::new(),
                    out: VecDeque::new(),
                    closed: false,
                    input_closed: false,
                    last: Instant::now(),
                    view_epochs: BTreeMap::new(),
                },
            );
        }
        Ok(())
    }
    /// One fair nonblocking service turn. Disconnected or slow clients never
    /// remove their source. The shared pool performs decoding outside this loop.
    pub fn step(&mut self) -> Result<bool> {
        self.accept()?;
        self.collect_opened()?;
        self.collect_rendered();
        self.schedule_watches();
        let peer_ids = self.peers.keys().copied().collect::<Vec<_>>();
        let mut source_ids = self.sources.keys().cloned().collect::<Vec<_>>();
        if !source_ids.is_empty() {
            let n = self.rotation % source_ids.len();
            source_ids.rotate_left(n);
            self.rotation = self.rotation.wrapping_add(1);
        }
        let mut descriptors = vec![libc::pollfd {
            fd: self.listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        for id in &peer_ids {
            let p = &self.peers[id];
            descriptors.push(libc::pollfd {
                fd: p.socket.as_raw_fd(),
                events: if p.input_closed { 0 } else { libc::POLLIN }
                    | if p.out.is_empty() { 0 } else { libc::POLLOUT },
                revents: 0,
            });
        }
        for id in &source_ids {
            descriptors.push(self.sources[id].pollfd());
        }
        let ready = self.peers.values().any(|p| p.input.contains(&b'\n'))
            || self
                .sources
                .values()
                .any(|s| s.owner.buffered() > 0 && !s.owner.pending());
        let timeout = if ready {
            0
        } else if self.inflight.is_empty() && self.opening.is_empty() && self.renders.is_empty() {
            16
        } else {
            2
        };
        let result = unsafe {
            libc::poll(
                descriptors.as_mut_ptr(),
                descriptors.len() as libc::nfds_t,
                timeout,
            )
        };
        if result < 0 {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e.into());
            }
        }
        for (index, id) in peer_ids.iter().enumerate() {
            let events = descriptors[index + 1].revents;
            let Some(peer) = self.peers.get_mut(id) else {
                continue;
            };
            if !peer.input_closed && events & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                let mut buffer = [0; 8192];
                match peer.socket.read(&mut buffer) {
                    Ok(0) => peer.input_closed = true,
                    Ok(n) => {
                        peer.last = Instant::now();
                        if peer.input.len() + n > REQUEST {
                            peer.closed = true;
                        } else {
                            peer.input.extend_from_slice(&buffer[..n]);
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => peer.closed = true,
                }
            }
            if events & libc::POLLOUT != 0 && !peer.out.is_empty() {
                let packet = peer.out.front_mut().unwrap();
                let bytes = &packet.bytes[packet.offset..];
                match peer.socket.write(&bytes[..bytes.len().min(65536)]) {
                    Ok(0) => peer.closed = true,
                    Ok(n) => {
                        packet.offset += n;
                        if packet.offset == packet.bytes.len() {
                            peer.out.pop_front();
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => peer.closed = true,
                }
            }
            for _ in 0..4 {
                let peer = self.peers.get_mut(id).unwrap();
                if peer.closed {
                    break;
                }
                let Some(end) = peer.input.iter().position(|b| *b == b'\n') else {
                    break;
                };
                let line = peer.input.drain(..=end).collect::<Vec<_>>();
                match serde_json::from_slice(&line) {
                    Ok(request) => self.respond(*id, request),
                    Err(_) => self.peers.get_mut(id).unwrap().closed = true,
                }
            }
        }
        while let Ok(completion) = self.pool.receive(Duration::ZERO) {
            if let Some(id) = self.inflight.remove(&completion.tag)
                && let Some(source) = self.sources.get_mut(&id)
            {
                match source.owner.complete(completion) {
                    Ok(true) => source.revision = source.revision.saturating_add(1),
                    Ok(false) => (),
                    Err(e) => source.error = Some(format!("Source completion: {e:?}")),
                }
            }
        }
        for (index, id) in source_ids.iter().enumerate() {
            let Some(source) = self.sources.get_mut(id) else {
                continue;
            };
            if let Err(e) = source.step(descriptors[1 + peer_ids.len() + index].revents) {
                source.error = Some(format!("Source I/O: {e:#}"));
            }
            if let Some((tag, job)) = source.owner.take_job() {
                if let Err(job) = self.pool.try_submit(tag, job) {
                    source
                        .owner
                        .complete(Completion {
                            tag,
                            job: *job,
                            result: Err(Error::Busy),
                        })
                        .map_err(|e| anyhow::anyhow!("Source admission: {e:?}"))?;
                } else {
                    self.inflight.insert(tag, id.clone());
                }
            }
        }
        let opening = &self.opening;
        let renders = &self.renders;
        self.peers.retain(|id, p| {
            !p.closed
                && (!p.input_closed
                    || !p.out.is_empty()
                    || p.input.contains(&b'\n')
                    || renders.values().any(|r| r.peer == *id)
                    || opening
                        .values()
                        .any(|job| job.waiters.iter().any(|(peer, _)| peer == id)))
                && p.last.elapsed() < Duration::from_secs(300)
        });
        self.sources
            .retain(|_, s| s.stopping_at.is_none() || !s.exit);
        let stale = self
            .watches
            .keys()
            .filter(|(p, _)| !self.peers.contains_key(p))
            .cloned()
            .collect::<Vec<_>>();
        for key in stale {
            self.remove_watch(&key);
        }
        let stale = self
            .renders
            .iter()
            .filter(|(_, r)| {
                !self.peers.contains_key(&r.peer)
                    || !self
                        .sources
                        .get(&r.source.id)
                        .is_some_and(|s| s.reference == r.source)
            })
            .map(|(tag, _)| *tag)
            .collect::<Vec<_>>();
        for tag in stale {
            if let Some(r) = self.renders.remove(&tag) {
                r.cancelled.store(true, Ordering::Release);
                if let render::Target::Request { peer, id } = r.target {
                    self.reply(
                        peer,
                        id,
                        Err(anyhow::anyhow!("Source changed before rendering finished")),
                    );
                }
            }
        }
        Ok(!self.stopping
            || !self.sources.is_empty()
            || !self.opening.is_empty()
            || !self.renders.is_empty()
            || self.peers.values().any(|p| !p.out.is_empty()))
    }
    pub fn run(mut self) -> Result<()> {
        while self.step()? {}
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_project_commands_restore_host_path_and_do_not_inherit_presenter_protocols() {
        let mut command = CommandBuilder::new("private-fixture");
        command.env("PATH", "/private/bundle/bin");
        command.env("STARFLUX_SYSTEM_PATH", "/host/bin");
        for key in [
            "GST_PLUGIN_SYSTEM_PATH_1_0",
            "GST_PLUGIN_SCANNER",
            "GST_REGISTRY",
            "ALSA_CONFIG_DIR",
            "ALSA_CONFIG_PATH",
            "ALSA_PLUGIN_DIR",
            "KITTY_PID",
            "TMUX",
            "TERM_PROGRAM",
        ] {
            command.env(key, "must-not-leak");
        }
        command.env("NATIVE_TEST_KEEP", "retained");
        source_environment(&mut command);
        assert_eq!(
            command.get_env("PATH"),
            Some(std::ffi::OsStr::new("/host/bin"))
        );
        for key in [
            "STARFLUX_SYSTEM_PATH",
            "GST_PLUGIN_SYSTEM_PATH_1_0",
            "GST_PLUGIN_SCANNER",
            "GST_REGISTRY",
            "ALSA_CONFIG_DIR",
            "ALSA_CONFIG_PATH",
            "ALSA_PLUGIN_DIR",
            "KITTY_PID",
            "TMUX",
            "TERM_PROGRAM",
        ] {
            assert!(command.get_env(key).is_none(), "{key}");
        }
        assert_eq!(
            command.get_env("NATIVE_TEST_KEEP"),
            Some(std::ffi::OsStr::new("retained"))
        );
    }

    use super::*;
    #[test]
    fn concurrent_open_waiters_share_one_initializer_and_reject_changed_intent() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let mut broker = Broker::bind(
            &root.path().join("source.sock"),
            &root.path().join("receipts"),
            "a".repeat(64),
        )
        .unwrap();
        // Hold initialization at its queue boundary, without starting a child.
        let (sender, receiver) = crossbeam_channel::bounded(2);
        broker.open_jobs = sender;
        let launch = Launch {
            id: uuid::Uuid::new_v4().to_string(),
            epoch: uuid::Uuid::new_v4().to_string(),
            argv: vec!["/bin/sh".into()],
            directory: root.path().into(),
            size: Size {
                rows: 24,
                columns: 80,
                cell: [8, 16],
            },
            palette: Palette {
                foreground: [255; 3],
                background: [0; 3],
                ansi: [[0; 3]; 16],
            },
        };
        broker.begin_open(1, 1, launch.clone()).unwrap();
        broker.begin_open(2, 1, launch.clone()).unwrap();
        assert_eq!(receiver.len(), 1);
        assert_eq!(broker.opening[&launch.id].waiters.len(), 2);
        let mut changed = launch.clone();
        changed.argv.push("different".into());
        assert!(broker.begin_open(3, 1, changed).is_err());
        assert_eq!(receiver.len(), 1);
        assert!(
            !root
                .path()
                .join("receipts")
                .join(format!("{}.json", launch.id))
                .exists()
        );
    }
    #[test]
    fn socket_cleanup_cannot_unlink_a_replacement_listener() {
        let root = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.path().join("source.sock");
        let broker = Broker::bind(&path, &root.path().join("receipts"), "a".repeat(64)).unwrap();
        fs::rename(&path, root.path().join("old.sock")).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();
        drop(broker);
        assert!(path.exists());
        assert!(UnixStream::connect(&path).is_ok());
        drop(replacement);
    }
    #[test]
    fn binding_an_existing_socket_preserves_its_owner() {
        let root = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.path().join("source.sock");
        let listener = UnixListener::bind(&path).unwrap();
        assert!(Broker::bind(&path, &root.path().join("receipts"), "a".repeat(64)).is_err());
        assert!(UnixStream::connect(&path).is_ok());
        drop(listener);
    }
    #[test]
    fn palette_keeps_basic_cube_gray_and_true_colors_independent() {
        let palette = Palette {
            foreground: [1, 2, 3],
            background: [4, 5, 6],
            ansi: [[7, 8, 9]; 16],
        };
        assert_eq!(palette.color(vt100::Color::Idx(1), true), [7, 8, 9]);
        assert_eq!(palette.color(vt100::Color::Idx(196), true), [255, 0, 0]);
        assert_eq!(palette.color(vt100::Color::Idx(255), false), [238; 3]);
        assert_eq!(
            palette.color(vt100::Color::Rgb(20, 30, 40), false),
            [20, 30, 40]
        );
        assert_eq!(palette.color(vt100::Color::Default, true), [1, 2, 3]);
        assert_eq!(palette.color(vt100::Color::Default, false), [4, 5, 6]);
    }
}
