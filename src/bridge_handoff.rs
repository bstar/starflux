//! Keep terminal-owned transport replies out of the shell during teardown.
//!
//! KIT's relay restores its saved termios before its asynchronous Kitty stop
//! reply arrives. Save an echo-free baseline for the relay, then consume that
//! last reply before restoring the shell's actual settings.
use anyhow::{Context, Result};
use std::{
    fs::{File, OpenOptions},
    io::{IsTerminal, Read},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    time::{Duration, Instant},
};

pub struct Handoff {
    tty: File,
    original: libc::termios,
    attached: bool,
}
impl Handoff {
    pub fn acquire() -> Result<Option<Self>> {
        if std::env::var_os("SSH_CONNECTION").is_none()
            || !std::io::stdin().is_terminal()
            || !std::io::stdout().is_terminal()
        {
            return Ok(None);
        }
        Self::from_tty(
            OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC)
                .open("/dev/tty")?,
        )
        .map(Some)
    }
    fn from_tty(tty: File) -> Result<Self> {
        let mut original = std::mem::MaybeUninit::uninit();
        if unsafe { libc::tcgetattr(tty.as_raw_fd(), original.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error()).context("Read bridge terminal settings");
        }
        let original = unsafe { original.assume_init() };
        let mut muted = original;
        muted.c_lflag &= !(libc::ECHO | libc::ECHONL);
        if unsafe { libc::tcsetattr(tty.as_raw_fd(), libc::TCSANOW, &muted) } != 0 {
            return Err(std::io::Error::last_os_error()).context("Mute bridge terminal replies");
        }
        Ok(Self {
            tty,
            original,
            attached: false,
        })
    }
    pub fn attached(&mut self) {
        self.attached = true;
    }
    fn drain(&mut self) {
        let mut raw = self.original;
        unsafe { libc::cfmakeraw(&mut raw) };
        if unsafe { libc::tcsetattr(self.tty.as_raw_fd(), libc::TCSANOW, &raw) } != 0 {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut pending = Vec::new();
        let mut chunk = [0; 8192];
        while Instant::now() < deadline {
            let mut poll = libc::pollfd {
                fd: self.tty.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let timeout = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(50) as i32;
            if unsafe { libc::poll(&mut poll, 1, timeout) } < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            match self.tty.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => {
                    pending.extend_from_slice(&chunk[..count]);
                    while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                        let line = pending.drain(..=end).collect::<Vec<_>>();
                        if line.starts_with(b"STAR_KIT_BRIDGE ") && line.ends_with(b" gone\n") {
                            return;
                        }
                    }
                    // Closing transport data is disposable; never grow a queue
                    // on a terminal controlled by another process.
                    if pending.len() > 64 * 1024 {
                        pending.clear();
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    }
}
impl Drop for Handoff {
    fn drop(&mut self) {
        // An unsupported terminal never started a transport. Leave ordinary
        // typeahead available for the fallback frontend.
        let action = if self.attached {
            self.drain();
            libc::TCSAFLUSH
        } else {
            libc::TCSANOW
        };
        unsafe {
            libc::tcsetattr(self.tty.as_raw_fd(), action, &self.original);
        }
    }
}
