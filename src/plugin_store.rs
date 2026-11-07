//! Private plugin data writes bound to an existing directory incarnation.
use anyhow::{Context, Result, ensure};
use serde::de::DeserializeOwned;
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::{Path, PathBuf},
};
pub struct Store {
    pub path: PathBuf,
    directory: File,
    stamp: [u64; 2],
}
impl Store {
    pub fn open(path: PathBuf) -> Result<Self> {
        let directory = File::open(&path)?;
        let m = directory.metadata()?;
        ensure!(
            m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o077 == 0,
            "Plugin data directory must be private and owned"
        );
        Ok(Self {
            path,
            directory,
            stamp: [m.dev(), m.ino()],
        })
    }
    pub fn current(&self) -> bool {
        fs::metadata(&self.path).is_ok_and(|m| [m.dev(), m.ino()] == self.stamp)
    }
    pub fn child(&self, name: &str) -> Result<Self> {
        ensure!(
            self.current()
                && !name.is_empty()
                && !matches!(name, "." | "..")
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "Invalid plugin state directory"
        );
        let c = std::ffi::CString::new(name)?;
        let dir = self.directory.as_raw_fd();
        if unsafe { libc::mkdirat(dir, c.as_ptr(), 0o700) } != 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let fd = unsafe {
            libc::openat(
                dir,
                c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        ensure!(fd >= 0, "Cannot open plugin state directory");
        let directory = unsafe { File::from_raw_fd(fd) };
        let m = directory.metadata()?;
        ensure!(
            m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o077 == 0,
            "Plugin state directory must be private and owned"
        );
        Ok(Self {
            path: self.path.join(name),
            directory,
            stamp: [m.dev(), m.ino()],
        })
    }
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<()> {
        ensure!(self.current(), "Plugin data directory changed");
        ensure!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "Invalid plugin metadata name"
        );
        let name = std::ffi::CString::new(name)?;
        let temp = std::ffi::CString::new(format!(".{}.tmp", uuid::Uuid::new_v4().simple()))?;
        let dir = self.directory.as_raw_fd();
        let fd = unsafe {
            libc::openat(
                dir,
                temp.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        ensure!(
            fd >= 0,
            "Cannot write plugin metadata: {}",
            std::io::Error::last_os_error()
        );
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(bytes)?;
        file.sync_all()?;
        ensure!(
            unsafe { libc::renameat(dir, temp.as_ptr(), dir, name.as_ptr()) } == 0,
            "Cannot commit plugin metadata: {}",
            std::io::Error::last_os_error()
        );
        self.directory.sync_all()?;
        Ok(())
    }
    pub fn lock(&self, name: &str, wait: bool) -> Result<Option<File>> {
        ensure!(self.current(), "Plugin data directory changed");
        let name = std::ffi::CString::new(name)?;
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_CREAT | libc::O_RDWR | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        ensure!(fd >= 0, "Cannot open plugin lock");
        let file = unsafe { File::from_raw_fd(fd) };
        let code = unsafe { libc::flock(fd, libc::LOCK_EX | if wait { 0 } else { libc::LOCK_NB }) };
        if code == 0 {
            return Ok(Some(file));
        }
        let error = std::io::Error::last_os_error();
        if !wait && error.kind() == std::io::ErrorKind::WouldBlock {
            Ok(None)
        } else {
            Err(error.into())
        }
    }
}
pub fn read<T: DeserializeOwned>(path: &Path, limit: u64) -> Result<T> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Plugin metadata exceeds its limit"
    );
    serde_json::from_slice(&bytes).context("Read plugin metadata")
}
