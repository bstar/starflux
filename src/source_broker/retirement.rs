//! Evidence from the single-threaded PTY subreaper, written only after ECHILD.
use super::*;
use crate::native_protocol::{Binding, Identity};
use service::Directory;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    version: u8,
    id: String,
    epoch: String,
    digest: String,
    owner: Identity,
    directory: PathBuf,
    stamp: (u64, u64),
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Certificate {
    intent: Intent,
    source: Reference,
    tree_retired: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Startup {
    intent: Intent,
    source: Reference,
    error: Option<String>,
}
pub struct Journal {
    intent: Intent,
    directory: Directory,
    pub owner_pidfd: OwnedFd,
    source: Reference,
}
/// PID disappearance is authority only with an exact start identity and boot.
pub fn dead(pid: u32, start: u64, expected_boot: &str) -> Result<bool> {
    if service::boot()? != expected_boot {
        return Ok(true);
    }
    let text = match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error.into()),
    };
    let fields = text
        .rsplit_once(')')
        .context("Invalid process identity")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let found: u64 = fields.get(19).context("Missing process start")?.parse()?;
    Ok(found != start || matches!(fields.first().copied(), Some("Z" | "X")))
}
pub fn owner_dead(owner: &Identity) -> Result<bool> {
    owner.validate()?;
    dead(owner.pid, owner.start_ticks, &owner.boot)
}
impl Intent {
    pub fn new(launch: &Launch, owner: &Identity, directory: &Path) -> Result<Self> {
        let handle = Directory::open(directory)?;
        Ok(Self {
            version: 1,
            id: launch.id.clone(),
            epoch: launch.epoch.clone(),
            digest: launch.digest()?,
            owner: owner.clone(),
            directory: directory.into(),
            stamp: handle.stamp(),
        })
    }
    fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported retirement intent");
        canonical_uuid(&self.id)?;
        canonical_uuid(&self.epoch)?;
        self.owner.validate()?;
        ensure!(
            self.digest.len() == 64 && self.digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid retirement launch digest"
        );
        ensure!(self.directory.is_absolute(), "Invalid retirement directory");
        Ok(())
    }
}
impl Journal {
    pub fn started(&self, error: Option<&str>) -> Result<()> {
        self.directory.write(
            &format!("{}.started.json", self.intent.id),
            &Startup {
                intent: self.intent.clone(),
                source: self.source.clone(),
                error: error.map(|message| message.chars().take(512).collect()),
            },
        )
    }
    pub fn begin(intent: Intent) -> Result<Self> {
        intent.validate()?;
        let directory = Directory::open(&intent.directory)?;
        ensure!(
            directory.stamp() == intent.stamp,
            "Retirement directory changed"
        );
        ensure!(
            unsafe { libc::getppid() } == intent.owner.pid as i32 && !owner_dead(&intent.owner)?,
            "Retirement owner changed before supervisor start"
        );
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, intent.owner.pid, 0) };
        ensure!(fd >= 0, "Cannot pin retirement owner");
        let owner_pidfd = unsafe { OwnedFd::from_raw_fd(fd as RawFd) };
        ensure!(
            !owner_dead(&intent.owner)?,
            "Retirement owner ended before supervisor start"
        );
        let pid = std::process::id();
        let source = Reference {
            id: intent.id.clone(),
            epoch: intent.epoch.clone(),
            boot: intent.owner.boot.clone(),
            pid,
            start_ticks: start_ticks(pid)?,
        };
        Ok(Self {
            intent,
            directory,
            owner_pidfd,
            source,
        })
    }
    /// Caller has reaped every child; the additional kernel check prevents a
    /// future refactor from publishing proof merely because the root exited.
    pub fn finish(&self) -> Result<()> {
        ensure!(
            fs::read_dir("/proc/self/task")?.count() == 1,
            "Retirement certification requires a single-threaded supervisor"
        );
        let mut status = 0;
        let child = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
        ensure!(
            child < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD),
            "Cannot certify a live or unverified process tree"
        );
        ensure!(
            self.directory.current() && self.directory.stamp() == self.intent.stamp,
            "Retirement directory changed"
        );
        self.directory.write(
            &format!("{}.retired.json", self.intent.id),
            &Certificate {
                intent: self.intent.clone(),
                source: self.source.clone(),
                tree_retired: true,
            },
        )
    }
}
fn startup(
    receipts: &Path,
    owner: &Identity,
    id: &str,
    epoch: &str,
    digest: &str,
) -> Result<Option<Startup>> {
    let directory = Directory::open(receipts)?;
    let Some(startup) = directory.read::<Startup>(&format!("{id}.started.json"))? else {
        return Ok(None);
    };
    startup.intent.validate()?;
    ensure!(
        startup.intent.owner == *owner
            && startup.intent.id == id
            && startup.intent.epoch == epoch
            && startup.intent.digest == digest
            && startup.intent.directory == receipts
            && startup.intent.stamp == directory.stamp()
            && startup.source.id == id
            && startup.source.epoch == epoch
            && startup.source.boot == owner.boot
            && startup.source.pid > 1
            && startup.source.start_ticks > 0
            && startup
                .error
                .as_ref()
                .is_none_or(|error| error.len() <= 2048),
        "Supervisor startup evidence does not match this launch"
    );
    Ok(Some(startup))
}
pub(super) fn started(
    receipts: &Path,
    owner: &Identity,
    source: &Reference,
    digest: &str,
) -> Result<Option<()>> {
    let Some(startup) = startup(receipts, owner, &source.id, &source.epoch, digest)? else {
        return Ok(None);
    };
    ensure!(
        startup.source == *source,
        "Supervisor startup identity changed"
    );
    if let Some(error) = startup.error {
        anyhow::bail!("Start supervised terminal command: {error}");
    }
    Ok(Some(()))
}
pub(super) fn failed_startup(
    receipts: &Path,
    owner: &Identity,
    id: &str,
    epoch: &str,
    digest: &str,
) -> Result<bool> {
    let Some(startup) = startup(receipts, owner, id, epoch, digest)? else {
        return Ok(false);
    };
    if startup.error.is_none() {
        return Ok(false);
    }
    let directory = Directory::open(receipts)?;
    let Some(certificate) = directory.read::<Certificate>(&format!("{id}.retired.json"))? else {
        return Ok(false);
    };
    ensure!(
        directory.stamp() == startup.intent.stamp
            && certificate.tree_retired
            && certificate.intent == startup.intent
            && certificate.source == startup.source,
        "Failed startup retirement identity changed"
    );
    dead(
        startup.source.pid,
        startup.source.start_ticks,
        &startup.source.boot,
    )
}
pub fn verify(data: &Path, binding: &Binding) -> Result<Reference> {
    binding.validate()?;
    ensure!(
        owner_dead(&binding.owner)?,
        "Native owner is still alive; use its guarded controls"
    );
    let path = data.join("native-sources/launches");
    let directory = Directory::open(&path)?;
    let certificate: Certificate = directory
        .read(&format!("{}.retired.json", binding.source_id))?
        .context("Supervisor retirement evidence is unavailable; descendants remain unverified")?;
    let intent = &certificate.intent;
    intent.validate()?;
    ensure!(
        certificate.tree_retired
            && intent.owner == binding.owner
            && intent.id == binding.source_id
            && intent.epoch == binding.source_epoch
            && intent.digest == binding.launch_digest
            && intent.directory == path
            && intent.stamp == directory.stamp(),
        "Supervisor retirement evidence does not match this pane"
    );
    let source = certificate.source;
    ensure!(
        source.id == binding.source_id
            && source.epoch == binding.source_epoch
            && source.boot == binding.owner.boot
            && source.pid > 1
            && source.start_ticks > 0
            && binding
                .source
                .as_ref()
                .is_none_or(|expected| expected == &source),
        "Supervisor retirement identity changed"
    );
    ensure!(
        dead(source.pid, source.start_ticks, &source.boot)?,
        "Supervisor is still finishing retirement"
    );
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_startup_requires_matching_exit_proof_and_never_accepts_a_live_supervisor() {
        use std::os::unix::fs::PermissionsExt;
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        let directory = Directory::open(path).unwrap();
        let owner = Identity {
            instance: uuid::Uuid::new_v4().to_string(),
            boot: service::boot().unwrap(),
            pid: std::process::id(),
            start_ticks: start_ticks(std::process::id()).unwrap(),
        };
        let source = Reference {
            id: uuid::Uuid::new_v4().to_string(),
            epoch: uuid::Uuid::new_v4().to_string(),
            boot: owner.boot.clone(),
            pid: 4_000_000_001,
            start_ticks: 1,
        };
        let digest = "a".repeat(64);
        let mut startup = Startup {
            intent: Intent {
                version: 1,
                id: source.id.clone(),
                epoch: source.epoch.clone(),
                digest: digest.clone(),
                owner: owner.clone(),
                directory: path.into(),
                stamp: directory.stamp(),
            },
            source: source.clone(),
            error: Some("private nonexistent command".into()),
        };
        let name = format!("{}.started.json", source.id);
        directory.write(&name, &startup).unwrap();
        assert!(started(path, &owner, &source, &digest).is_err());
        assert!(!failed_startup(path, &owner, &source.id, &source.epoch, &digest).unwrap());
        let mut certificate = Certificate {
            intent: startup.intent.clone(),
            source: source.clone(),
            tree_retired: true,
        };
        let retired = format!("{}.retired.json", source.id);
        directory.write(&retired, &certificate).unwrap();
        assert!(failed_startup(path, &owner, &source.id, &source.epoch, &digest).unwrap());
        assert!(failed_startup(path, &owner, &source.id, &source.epoch, &"b".repeat(64)).is_err());
        let mut wrong = owner.clone();
        wrong.instance = uuid::Uuid::new_v4().to_string();
        assert!(failed_startup(path, &wrong, &source.id, &source.epoch, &digest).is_err());
        startup.error = None;
        directory.write(&name, &startup).unwrap();
        assert!(started(path, &owner, &source, &digest).unwrap().is_some());
        assert!(!failed_startup(path, &owner, &source.id, &source.epoch, &digest).unwrap());
        startup.error = Some("private failure".into());
        startup.source.pid = std::process::id();
        startup.source.start_ticks = owner.start_ticks;
        certificate.source = startup.source.clone();
        directory.write(&name, &startup).unwrap();
        directory.write(&retired, &certificate).unwrap();
        assert!(!failed_startup(path, &owner, &source.id, &source.epoch, &digest).unwrap());
        startup.intent.stamp.1 += 1;
        directory.write(&name, &startup).unwrap();
        assert!(failed_startup(path, &owner, &source.id, &source.epoch, &digest).is_err());
    }
    #[test]
    fn retirement_proof_is_bound_to_owner_source_digest_and_directory() {
        use std::os::unix::fs::PermissionsExt;
        let temporary = tempfile::tempdir().unwrap();
        let data = temporary.path().join("data");
        let path = data.join("native-sources/launches");
        fs::create_dir_all(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let directory = Directory::open(&path).unwrap();
        let owner = Identity {
            instance: uuid::Uuid::new_v4().to_string(),
            boot: service::boot().unwrap(),
            pid: 4_000_000_000,
            start_ticks: 1,
        };
        let source = Reference {
            id: uuid::Uuid::new_v4().to_string(),
            epoch: uuid::Uuid::new_v4().to_string(),
            boot: owner.boot.clone(),
            pid: 4_000_000_001,
            start_ticks: 1,
        };
        let binding = Binding {
            version: 1,
            project: "private".into(),
            pane: 0,
            terminal_epoch: uuid::Uuid::new_v4().to_string(),
            owner: owner.clone(),
            source_id: source.id.clone(),
            source_epoch: source.epoch.clone(),
            launch_digest: "a".repeat(64),
            source: Some(source.clone()),
        };
        assert!(verify(&data, &binding).is_err());
        let intent = Intent {
            version: 1,
            id: source.id.clone(),
            epoch: source.epoch.clone(),
            digest: binding.launch_digest.clone(),
            owner,
            directory: path.clone(),
            stamp: directory.stamp(),
        };
        let mut certificate = Certificate {
            intent,
            source: source.clone(),
            tree_retired: false,
        };
        let name = format!("{}.retired.json", source.id);
        directory.write(&name, &certificate).unwrap();
        assert!(verify(&data, &binding).is_err());
        certificate.tree_retired = true;
        directory.write(&name, &certificate).unwrap();
        assert_eq!(verify(&data, &binding).unwrap(), source);
        let mut wrong = binding.clone();
        wrong.launch_digest = "b".repeat(64);
        assert!(verify(&data, &wrong).is_err());
        wrong = binding.clone();
        wrong.owner.instance = uuid::Uuid::new_v4().to_string();
        assert!(verify(&data, &wrong).is_err());
        wrong = binding.clone();
        wrong.source.as_mut().unwrap().start_ticks += 1;
        assert!(verify(&data, &wrong).is_err());
        certificate.intent.stamp.1 += 1;
        directory.write(&name, &certificate).unwrap();
        assert!(verify(&data, &binding).is_err());
        certificate.intent.stamp = directory.stamp();
        certificate.source.pid = std::process::id();
        certificate.source.start_ticks = start_ticks(std::process::id()).unwrap();
        directory.write(&name, &certificate).unwrap();
        wrong = binding;
        wrong.source = None;
        assert!(verify(&data, &wrong).is_err(), "live supervisor accepted");
    }
}
