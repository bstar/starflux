//! Persistent discovery for the native owner. Never upgrades by killing a live
//! worker. Source launch intents and per-pane references live outside runtime.
use super::*;
use std::{
    ffi::CString,
    fs::File,
    io::{BufRead, BufReader},
    os::unix::{fs::FileTypeExt, process::CommandExt},
    process::{Command, Stdio},
};
const SERVICE_VERSION: u8 = 1;
const META_LIMIT: u64 = 32768;
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Identity {
    pub instance: String,
    pub boot: String,
    pub pid: u32,
    pub start_ticks: u64,
}
impl Identity {
    fn validate(&self) -> Result<()> {
        canonical_uuid(&self.instance)?;
        canonical_uuid(&self.boot)?;
        ensure!(
            self.pid > 1 && self.start_ticks > 0,
            "Invalid native owner identity"
        );
        Ok(())
    }
    fn alive(&self) -> bool {
        self.boot == boot().unwrap_or_default()
            && start_ticks(self.pid).ok() == Some(self.start_ticks)
            && fs::read_to_string(format!("/proc/{}/stat", self.pid)).is_ok_and(|s| {
                s.rsplit_once(')')
                    .is_some_and(|(_, s)| s.split_whitespace().next() != Some("Z"))
            })
    }
}
#[derive(Deserialize, Serialize)]
struct Descriptor {
    version: u8,
    owner: Identity,
}
struct Directory {
    path: PathBuf,
    file: File,
    stamp: (u64, u64),
}
impl Directory {
    fn open(path: &Path) -> Result<Self> {
        ensure!(
            path.is_absolute(),
            "Native source directory must be absolute"
        );
        starkit::terminal_graphics::session::private_root(path)?;
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        let m = file.metadata()?;
        ensure!(
            m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o077 == 0,
            "Native source directory must be private and owned"
        );
        Ok(Self {
            path: path.to_owned(),
            file,
            stamp: (m.dev(), m.ino()),
        })
    }
    fn current(&self) -> bool {
        fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.is_dir() && (m.dev(), m.ino()) == self.stamp)
    }
    fn child(&self, name: &str) -> Result<Self> {
        valid_name(name)?;
        ensure!(self.current(), "Native source namespace changed");
        let c = CString::new(name)?;
        if unsafe { libc::mkdirat(self.file.as_raw_fd(), c.as_ptr(), 0o700) } != 0
            && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists
        {
            return Err(io::Error::last_os_error().into());
        }
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        ensure!(
            fd >= 0,
            "Cannot open native source subdirectory: {}",
            io::Error::last_os_error()
        );
        let file = unsafe { File::from_raw_fd(fd) };
        let m = file.metadata()?;
        ensure!(
            m.is_dir()
                && m.uid() == unsafe { libc::geteuid() }
                && m.mode() & 0o077 == 0
                && self.current(),
            "Native source namespace changed or is not private"
        );
        Ok(Self {
            path: self.path.join(name),
            file,
            stamp: (m.dev(), m.ino()),
        })
    }
    fn file(&self, name: &str, create: bool) -> Result<File> {
        valid_name(name)?;
        ensure!(self.current(), "Native source namespace changed");
        let c = CString::new(name)?;
        let flags = libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if create {
                libc::O_CREAT | libc::O_RDWR
            } else {
                libc::O_RDONLY
            };
        let fd = unsafe { libc::openat(self.file.as_raw_fd(), c.as_ptr(), flags, 0o600) };
        if fd < 0 {
            return Err(io::Error::last_os_error().into());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let m = file.metadata()?;
        ensure!(
            m.is_file()
                && m.uid() == unsafe { libc::geteuid() }
                && m.mode() & 0o077 == 0
                && m.nlink() == 1,
            "Native source metadata must be private, owned and unlinked elsewhere"
        );
        Ok(file)
    }
    fn read<T: serde::de::DeserializeOwned>(&self, name: &str) -> Result<Option<T>> {
        let file = match self.file(name, false) {
            Ok(f) => f,
            Err(e)
                if e.downcast_ref::<io::Error>()
                    .is_some_and(|e| e.kind() == io::ErrorKind::NotFound) =>
            {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        let mut bytes = Vec::new();
        file.take(META_LIMIT + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= META_LIMIT,
            "Native source metadata exceeds limit"
        );
        Ok(Some(serde_json::from_slice(&bytes)?))
    }
    fn write<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        valid_name(name)?;
        ensure!(self.current(), "Native source namespace changed");
        let bytes = serde_json::to_vec(value)?;
        ensure!(
            bytes.len() as u64 <= META_LIMIT,
            "Native source metadata exceeds limit"
        );
        let tmp = CString::new(format!(".{}.tmp", uuid::Uuid::new_v4().simple()))?;
        let name = CString::new(name)?;
        let dir = self.file.as_raw_fd();
        let fd = unsafe {
            libc::openat(
                dir,
                tmp.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        ensure!(
            fd >= 0,
            "Cannot stage native metadata: {}",
            io::Error::last_os_error()
        );
        let mut file = unsafe { File::from_raw_fd(fd) };
        let result = (|| -> Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            ensure!(self.current(), "Native source namespace changed");
            ensure!(
                unsafe { libc::renameat(dir, tmp.as_ptr(), dir, name.as_ptr()) } == 0,
                "Cannot commit native metadata: {}",
                io::Error::last_os_error()
            );
            self.file.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            unsafe { libc::unlinkat(dir, tmp.as_ptr(), 0) };
        }
        result
    }
    fn lock(&self, name: &str, deadline: Duration) -> Result<File> {
        let file = self.file(name, true)?;
        let began = Instant::now();
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(file);
            }
            let e = io::Error::last_os_error();
            ensure!(
                e.kind() == io::ErrorKind::WouldBlock,
                "Native source lock: {e}"
            );
            ensure!(
                began.elapsed() < deadline,
                "Native source initialization is busy"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn endpoint(&self) -> PathBuf {
        PathBuf::from(format!(
            "/proc/self/fd/{}/broker.sock",
            self.file.as_raw_fd()
        ))
    }
}
fn valid_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && !matches!(name, "." | "..")
            && name.len() <= 96
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
        "Invalid native source metadata name"
    );
    Ok(())
}
fn boot() -> Result<String> {
    Ok(fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .to_owned())
}
fn root(data: &Path) -> Result<Directory> {
    Directory::open(data)?.child("native-sources")
}
/// Resolving a short descriptor-relative pathname permits deep data directories
/// without moving the live socket into the ephemeral runtime namespace.
pub(super) fn bind_socket(path: &Path) -> Result<UnixListener> {
    let parent = Directory::open(path.parent().context("Socket parent unavailable")?)?;
    let name = path.file_name().context("Socket name unavailable")?;
    let short = PathBuf::from(format!("/proc/self/fd/{}", parent.file.as_raw_fd())).join(name);
    UnixListener::bind(short).map_err(Into::into)
}
fn peer(socket: &UnixStream, owner: &Identity) -> Result<()> {
    let mut credential: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    ensure!(
        unsafe {
            libc::getsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credential as *mut libc::ucred).cast(),
                &mut length,
            )
        } == 0,
        "Native source peer credentials unavailable"
    );
    ensure!(
        credential.uid == unsafe { libc::geteuid() }
            && u32::try_from(credential.pid).ok() == Some(owner.pid)
            && owner.alive(),
        "Native source socket owner changed"
    );
    Ok(())
}
fn token(directory: &Directory, create: bool) -> Result<String> {
    if let Some(token) = directory.read::<String>("token.json")? {
        ensure!(
            token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid native source token"
        );
        return Ok(token);
    }
    ensure!(create, "Native source token is unavailable");
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    directory.write("token.json", &token)?;
    Ok(token)
}
pub struct Handle {
    data: PathBuf,
    pub owner: Identity,
    token: String,
}
impl Handle {
    pub fn connect(&self) -> Result<Connection> {
        let directory = root(&self.data)?;
        let socket = UnixStream::connect(directory.endpoint())?;
        peer(&socket, &self.owner)?;
        socket.set_read_timeout(Some(Duration::from_secs(10)))?;
        socket.set_write_timeout(Some(Duration::from_secs(10)))?;
        Ok(Connection {
            reader: BufReader::new(socket),
            token: self.token.clone(),
            sequence: 0,
        })
    }
    fn verify(&self) -> Result<()> {
        let hello = self.connect()?.call(Action::Hello)?;
        ensure!(
            hello["version"] == 1
                && hello["instance"] == self.owner.instance
                && hello["boot"] == self.owner.boot
                && hello["broker_pid"] == self.owner.pid
                && hello["broker_start_ticks"] == self.owner.start_ticks,
            "Native source owner descriptor changed"
        );
        ensure!(
            hello["capabilities"].as_array().is_some_and(|c| c
                .iter()
                .any(|v| v == "retained-pty-v1")
                && c.iter().any(|v| v == "cells-stream-v1")
                && c.iter().any(|v| v == "launch-digest-v1")),
            "Retained native owner is incompatible; it was kept running"
        );
        Ok(())
    }
}
pub struct Connection {
    reader: BufReader<UnixStream>,
    token: String,
    sequence: u64,
}
impl Connection {
    /// A write failure is an indeterminate delivery, never permission to replay.
    pub fn call(&mut self, action: Action) -> Result<Value> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .context("Native request sequence exhausted")?;
        let request = Request {
            version: 1,
            id: self.sequence,
            token: self.token.clone(),
            action,
        };
        let bytes = serde_json::to_vec(&request)?;
        ensure!(bytes.len() < REQUEST, "Native request exceeds limit");
        self.reader.get_mut().write_all(&bytes)?;
        self.reader.get_mut().write_all(b"\n")?;
        let mut bytes = Vec::new();
        let n = self
            .reader
            .by_ref()
            .take((OUTPUT + 1) as u64)
            .read_until(b'\n', &mut bytes)?;
        ensure!(
            n > 0 && n <= OUTPUT && bytes.last() == Some(&b'\n'),
            "Native response unavailable or exceeds limit"
        );
        let value: Value = serde_json::from_slice(&bytes)?;
        ensure!(
            value["version"] == 1 && value["id"] == self.sequence,
            "Native response identity changed"
        );
        ensure!(
            value["ok"] == true,
            "Native source: {}",
            value["error"].as_str().unwrap_or("invalid reply")
        );
        Ok(value.get("result").cloned().unwrap_or(Value::Null))
    }
}
fn descriptor(directory: &Directory) -> Result<Option<Descriptor>> {
    let descriptor = directory.read::<Descriptor>("owner.json")?;
    if let Some(d) = &descriptor {
        ensure!(
            d.version == SERVICE_VERSION,
            "Native owner record is newer/incompatible; kept unchanged"
        );
        d.owner.validate()?;
    }
    Ok(descriptor)
}
fn clean_stale_socket(directory: &Directory) -> Result<()> {
    let path = directory.path.join("broker.sock");
    let m = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    ensure!(
        m.file_type().is_socket() && m.uid() == unsafe { libc::geteuid() },
        "Native source socket path is not an owned socket"
    );
    match UnixStream::connect(directory.endpoint()) {
        Ok(_) => anyhow::bail!("An unregistered retained native owner is reachable; kept running"),
        Err(e) if e.raw_os_error() == Some(libc::ECONNREFUSED) => (),
        Err(e) => return Err(e.into()),
    }
    ensure!(
        directory.current()
            && fs::symlink_metadata(&path)
                .is_ok_and(|new| (new.dev(), new.ino()) == (m.dev(), m.ino())),
        "Native source socket changed"
    );
    fs::remove_file(path)?;
    Ok(())
}
/// Only this explicit discovery path may create a new worker. Reattachment of a
/// saved pane must use its recorded owner and must not implicitly call ensure.
pub fn ensure(data: &Path, executable: &Path) -> Result<Handle> {
    let directory = root(data)?;
    let _startup = directory.lock("startup.lock", Duration::from_secs(5))?;
    if let Some(d) = descriptor(&directory)?
        && d.owner.alive()
    {
        let handle = Handle {
            data: data.to_owned(),
            owner: d.owner,
            token: token(&directory, false)?,
        };
        handle.verify()?;
        return Ok(handle);
    }
    // A worker can be between acquiring its lifetime lock and publishing its
    // descriptor. Wait for it; never launch a competing owner after a timeout.
    let lifetime = directory.lock("owner.lock", Duration::from_secs(5))?;
    if let Some(d) = descriptor(&directory)?
        && d.owner.alive()
    {
        drop(lifetime);
        let handle = Handle {
            data: data.to_owned(),
            owner: d.owner,
            token: token(&directory, false)?,
        };
        handle.verify()?;
        return Ok(handle);
    }
    clean_stale_socket(&directory)?;
    drop(lifetime);
    let mut log = directory.file("service.log", true)?;
    std::io::Seek::seek(&mut log, std::io::SeekFrom::End(0))?;
    let mut command = Command::new(executable);
    command
        .arg("--native-source-service")
        .arg(data)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // A separate session prevents closing an SSH launcher from hanging up its
    // retained owner. No inherited PTY or stdio stream is kept alive by it.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn()?;
    let began = Instant::now();
    loop {
        if let Some(d) = descriptor(&directory)?
            && d.owner.alive()
        {
            let handle = Handle {
                data: data.to_owned(),
                owner: d.owner,
                token: token(&directory, false)?,
            };
            handle.verify()?;
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(handle);
        }
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("Native source initializer exited ({status})");
        }
        if began.elapsed() >= Duration::from_secs(10) {
            // Its launch state is uncertain. Leave it running and let the next
            // discovery validate the same lock/descriptor instead of restarting.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            anyhow::bail!("Native source initialization has not published an owner; kept running");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub fn existing(data: &Path, owner: &Identity) -> Result<Handle> {
    let directory = root(data)?;
    let d =
        descriptor(&directory)?.context("Saved native owner is unavailable; recover explicitly")?;
    ensure!(
        d.owner == *owner && owner.alive(),
        "Saved native owner ended; recover explicitly"
    );
    let handle = Handle {
        data: data.to_owned(),
        owner: owner.clone(),
        token: token(&directory, false)?,
    };
    handle.verify()?;
    Ok(handle)
}
pub fn run(data: &Path) -> Result<()> {
    let directory = root(data)?;
    let _lifetime = directory.lock("owner.lock", Duration::ZERO)?;
    if let Some(d) = descriptor(&directory)? {
        ensure!(!d.owner.alive(), "Retained native owner is already running");
    }
    clean_stale_socket(&directory)?;
    let token = token(&directory, true)?;
    let receipts = directory.child("launches")?;
    let broker = Broker::bind(&directory.path.join("broker.sock"), &receipts.path, token)?;
    let owner = Identity {
        instance: broker.instance.clone(),
        boot: broker.boot.clone(),
        pid: std::process::id(),
        start_ticks: start_ticks(std::process::id())?,
    };
    directory.write(
        "owner.json",
        &Descriptor {
            version: SERVICE_VERSION,
            owner,
        },
    )?;
    broker.run()
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Binding {
    pub version: u8,
    pub project: String,
    pub pane: usize,
    pub terminal_epoch: String,
    pub owner: Identity,
    pub source_id: String,
    pub source_epoch: String,
    pub launch_digest: String,
    /// None means the intent was persisted but its delivery/outcome is unknown.
    pub source: Option<Reference>,
}
fn slot_shape(project: &str, pane: usize, epoch: &str) -> Result<()> {
    ensure!(
        !project.is_empty()
            && project.len() <= 32
            && project
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            && pane < 32,
        "Invalid native pane slot"
    );
    ensure!(
        !epoch.is_empty()
            && epoch.len() <= 64
            && epoch
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "Invalid native pane epoch"
    );
    Ok(())
}
impl Binding {
    fn validate(&self) -> Result<()> {
        slot_shape(&self.project, self.pane, &self.terminal_epoch)?;
        self.owner.validate()?;
        ensure!(self.version == 1, "Unsupported native pane record");
        canonical_uuid(&self.source_id)?;
        canonical_uuid(&self.source_epoch)?;
        ensure!(
            self.launch_digest.len() == 64
                && self.launch_digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid native pane launch digest"
        );
        if let Some(source) = &self.source {
            canonical_uuid(&source.boot)?;
            ensure!(
                source.id == self.source_id
                    && source.epoch == self.source_epoch
                    && source.boot == self.owner.boot
                    && source.pid > 1
                    && source.start_ticks > 0,
                "Native pane source identity changed"
            );
        }
        Ok(())
    }
}
fn slots(data: &Path, project: &str) -> Result<Directory> {
    root(data)?.child("panes")?.child(project)
}
fn read_binding(directory: &Directory, project: &str, pane: usize) -> Result<Option<Binding>> {
    let record = directory.read::<Binding>(&format!("p{pane}.json"))?;
    if let Some(r) = &record {
        r.validate()?;
        ensure!(
            r.project == project && r.pane == pane,
            "Native pane record belongs to another slot"
        );
    }
    Ok(record)
}
pub fn read_slot(data: &Path, project: &str, pane: usize) -> Result<Option<Binding>> {
    slot_shape(project, pane, "lookup")?;
    read_binding(&slots(data, project)?, project, pane)
}
impl Handle {
    /// Called only for a newly authorized launch, never from saved-screen or
    /// ordinary reconnect paths. Persist identity BEFORE delivery to the owner.
    pub fn open_slot(
        &self,
        project: &str,
        pane: usize,
        epoch: &str,
        launch: &Launch,
        expected: Option<&Binding>,
    ) -> Result<Binding> {
        slot_shape(project, pane, epoch)?;
        launch.validate_shape()?;
        let directory = slots(&self.data, project)?;
        let mut wanted = Binding {
            version: 1,
            project: project.into(),
            pane,
            terminal_epoch: epoch.into(),
            owner: self.owner.clone(),
            source_id: launch.id.clone(),
            source_epoch: launch.epoch.clone(),
            launch_digest: launch.digest()?,
            source: None,
        };
        {
            let _lock = directory.lock("slots.lock", Duration::from_secs(5))?;
            let current = read_binding(&directory, project, pane)?;
            if let Some(r) = current.as_ref().filter(|r| r.terminal_epoch == epoch) {
                let mut intent = r.clone();
                intent.source = None;
                ensure!(
                    intent == wanted,
                    "Native pane launch intent changed; not replayed"
                );
                if let Some(source) = &r.source {
                    let ready = r.clone();
                    let source = source.clone();
                    drop(_lock);
                    let info = self.connect()?.call(Action::Inspect { source })?;
                    ensure!(
                        info["launch_digest"] == ready.launch_digest,
                        "Native source launch identity changed"
                    );
                    return Ok(ready);
                }
            } else {
                ensure!(
                    current.as_ref() == expected,
                    "Native pane changed before launch"
                );
                directory.write(&format!("p{pane}.json"), &wanted)?;
            }
        }
        let result = self.connect()?.call(Action::Open {
            launch: launch.clone(),
        })?;
        ensure!(
            result["launch_digest"] == wanted.launch_digest,
            "Native source launch identity changed"
        );
        wanted.source = Some(serde_json::from_value(result["source"].clone())?);
        wanted.validate()?;
        let _lock = directory.lock("slots.lock", Duration::from_secs(5))?;
        let mut pending = wanted.clone();
        pending.source = None;
        let current = read_binding(&directory, project, pane)?;
        ensure!(
            current.as_ref() == Some(&pending) || current.as_ref() == Some(&wanted),
            "Native source was created but its pane changed; source kept running"
        );
        directory.write(&format!("p{pane}.json"), &wanted)?;
        Ok(wanted)
    }
}
/// Read-only recovery. It can discover a source which survived an uncertain Open
/// reply, but cannot send a saved command or start a replacement broker.
pub fn reattach_slot(data: &Path, project: &str, pane: usize, epoch: &str) -> Result<Binding> {
    slot_shape(project, pane, epoch)?;
    let directory = slots(data, project)?;
    let mut record =
        read_binding(&directory, project, pane)?.context("Native pane has no saved source")?;
    ensure!(record.terminal_epoch == epoch, "Native pane epoch changed");
    let handle = existing(data, &record.owner)?;
    let mut connection = handle.connect()?;
    let info = if let Some(reference) = &record.source {
        connection.call(Action::Inspect {
            source: reference.clone(),
        })?
    } else {
        let sources = connection.call(Action::List)?;
        let info = sources
            .as_array()
            .and_then(|items| {
                items.iter().find(|s| {
                    s["source"]["id"] == record.source_id
                        && s["source"]["epoch"] == record.source_epoch
                })
            })
            .context(
                "Native launch outcome is unavailable; recover explicitly without replaying it",
            )?;
        info.clone()
    };
    ensure!(
        info["launch_digest"] == record.launch_digest,
        "Native source launch identity changed; not reattached"
    );
    let source = serde_json::from_value::<Reference>(info["source"].clone())?;
    let previous = record.clone();
    record.source = Some(source);
    record.validate()?;
    let _lock = directory.lock("slots.lock", Duration::from_secs(5))?;
    let current = read_binding(&directory, project, pane)?;
    ensure!(
        current.as_ref() == Some(&previous) || current.as_ref() == Some(&record),
        "Native pane changed during reattachment"
    );
    if previous != record {
        directory.write(&format!("p{pane}.json"), &record)?;
    }
    Ok(record)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Incoming {
    version: u8,
    id: u64,
    action: Action,
}
/// Remote/local streaming connector. The token stays on the source machine;
/// bounded incoming lines are authenticated here instead of shell commands.
pub fn stdio(data: &Path, executable: &Path, owner: Option<&Identity>) -> Result<()> {
    let handle = match owner {
        Some(owner) => existing(data, owner)?,
        None => ensure(data, executable)?,
    };
    let mut socket = handle.connect()?.reader.into_inner();
    socket.set_read_timeout(None)?;
    let mut input = socket.try_clone()?;
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let mut stdin = io::stdin().lock();
            loop {
                let mut line = Vec::new();
                let n = stdin
                    .by_ref()
                    .take((REQUEST + 1) as u64)
                    .read_until(b'\n', &mut line)?;
                if n == 0 {
                    input.shutdown(std::net::Shutdown::Write)?;
                    return Ok(());
                }
                ensure!(
                    n <= REQUEST && line.last() == Some(&b'\n'),
                    "Native connector request exceeds limit"
                );
                let request: Incoming = serde_json::from_slice(&line)?;
                ensure!(
                    request.version == 1,
                    "Native connector protocol is incompatible"
                );
                let request = Request {
                    version: 1,
                    id: request.id,
                    token: handle.token.clone(),
                    action: request.action,
                };
                let bytes = serde_json::to_vec(&request)?;
                ensure!(
                    bytes.len() < REQUEST,
                    "Native connector request exceeds limit"
                );
                input.write_all(&bytes)?;
                input.write_all(b"\n")?;
            }
        })();
        let failed = result.is_err();
        let _ = tx.send(result);
        if failed {
            let _ = input.shutdown(std::net::Shutdown::Both);
        }
    });
    let mut output = io::stdout().lock();
    let mut bytes = [0; 8192];
    loop {
        let n = socket.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        output.write_all(&bytes[..n])?;
        output.flush()?;
    }
    if let Ok(result) = rx.try_recv() {
        result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn private_temp() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        temp
    }
    #[test]
    fn descriptor_relative_socket_supports_deep_durable_data_paths() {
        let tmp = private_temp();
        let data = tmp.path().join("x".repeat(80)).join("y".repeat(80));
        let directory = root(&data).unwrap();
        let path = directory.path.join("broker.sock");
        assert!(path.as_os_str().as_encoded_bytes().len() > 108);
        let listener = bind_socket(&path).unwrap();
        let client = UnixStream::connect(directory.endpoint()).unwrap();
        let (server, _) = listener.accept().unwrap();
        assert!(clean_stale_socket(&directory).is_err());
        drop(client);
        drop(server);
        drop(listener);
        clean_stale_socket(&directory).unwrap();
        assert!(!path.exists());
    }
    #[test]
    fn directory_replacement_never_resurrects_or_writes_into_the_new_namespace() {
        let tmp = private_temp();
        let path = tmp.path().join("data");
        let directory = Directory::open(&path).unwrap();
        fs::rename(&path, tmp.path().join("old")).unwrap();
        Directory::open(&path).unwrap();
        assert!(directory.write("owner.json", &json!({"x":1})).is_err());
        assert!(directory.child("panes").is_err());
        assert!(!path.join("owner.json").exists() && !path.join("panes").exists());
    }
    #[test]
    fn private_records_reject_symlinks_hardlinks_oversize_and_held_locks() {
        let tmp = private_temp();
        let directory = Directory::open(tmp.path()).unwrap();
        directory.write("token.json", &"private").unwrap();
        fs::hard_link(
            tmp.path().join("token.json"),
            tmp.path().join("linked.json"),
        )
        .unwrap();
        assert!(directory.read::<String>("token.json").is_err());
        std::os::unix::fs::symlink(tmp.path().join("token.json"), tmp.path().join("alias.json"))
            .unwrap();
        assert!(directory.read::<String>("alias.json").is_err());
        let mut file = directory.file("large.json", true).unwrap();
        file.write_all(&vec![b' '; META_LIMIT as usize + 1])
            .unwrap();
        assert!(directory.read::<Value>("large.json").is_err());
        let lock = directory.lock("lock", Duration::ZERO).unwrap();
        assert!(directory.lock("lock", Duration::ZERO).is_err());
        drop(lock);
        assert!(directory.lock("lock", Duration::ZERO).is_ok());
    }
    #[test]
    fn newer_owner_record_cannot_trigger_a_downgrade_or_worker_spawn() {
        let tmp = private_temp();
        let directory = root(tmp.path()).unwrap();
        let record = json!({"version":2,"owner":{"instance":uuid::Uuid::new_v4().to_string(),"boot":boot().unwrap(),"pid":std::process::id(),"start_ticks":start_ticks(std::process::id()).unwrap()}});
        directory.write("owner.json", &record).unwrap();
        let before = fs::read(directory.path.join("owner.json")).unwrap();
        assert!(ensure(tmp.path(), Path::new("/must-not-spawn-native-downgrade")).is_err());
        assert_eq!(fs::read(directory.path.join("owner.json")).unwrap(), before);
        assert!(!directory.path.join("token.json").exists());
    }
}
