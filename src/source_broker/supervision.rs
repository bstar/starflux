//! One single-threaded subreaper per PTY. Only kernel-owned children are
//! signalled; process groups and global /proc scans are never kill authority.
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const GRACE: Duration = Duration::from_millis(200);

fn checked(value: i32, context: &str) -> Result<i32> {
    if value < 0 {
        Err(io::Error::last_os_error()).context(context.to_owned())
    } else {
        Ok(value)
    }
}

/// Called only by the early helper dispatch, before any application threads.
/// The supervisor's PID remains the source identity until every descendant has
/// exited and been reaped. The actual shell owns its own foreground group.
pub fn run(argv: &[String]) -> Result<i32> {
    let (intent, argv) = if argv.first().is_some_and(|arg| arg == "--retirement-v1") {
        ensure!(
            argv.len() >= 3,
            "Missing retirement intent or supervised command"
        );
        ensure!(argv[1].len() <= 16384, "Retirement intent exceeds limit");
        (
            Some(serde_json::from_str::<super::retirement::Intent>(&argv[1])?),
            &argv[2..],
        )
    } else {
        (None, argv)
    };
    ensure!(!argv.is_empty(), "Missing supervised command");
    ensure!(
        fs::read_dir("/proc/self/task")?.count() == 1,
        "PTY supervisor must start before threads"
    );
    let pid = unsafe { libc::getpid() };
    ensure!(
        unsafe { libc::getsid(0) } == pid && unsafe { libc::isatty(0) } == 1,
        "PTY supervisor requires its own controlling terminal session"
    );
    checked(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
        "Enable child subreaper",
    )?;
    unsafe {
        libc::signal(libc::SIGCHLD, libc::SIG_DFL);
    }
    let probe = checked(
        unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32,
        "PTY supervision requires pidfd support",
    )?;
    drop(unsafe { OwnedFd::from_raw_fd(probe) });
    let mut mask = unsafe { std::mem::zeroed::<libc::sigset_t>() };
    unsafe {
        libc::sigemptyset(&mut mask);
    }
    for signal in [
        libc::SIGCHLD,
        libc::SIGHUP,
        libc::SIGTERM,
        libc::SIGINT,
        libc::SIGQUIT,
    ] {
        unsafe {
            libc::sigaddset(&mut mask, signal);
        }
    }
    checked(
        unsafe { libc::sigprocmask(libc::SIG_BLOCK, &mask, std::ptr::null_mut()) },
        "Block supervisor signals",
    )?;
    let descriptor = checked(
        unsafe { libc::signalfd(-1, &mask, libc::SFD_CLOEXEC | libc::SFD_NONBLOCK) },
        "Create supervisor signal descriptor",
    )?;
    let signals = unsafe { OwnedFd::from_raw_fd(descriptor) };
    let null = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")?;
    let journal = intent.map(super::retirement::Journal::begin).transpose()?;
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // This child hook uses only async-signal-safe libc operations. It cannot
    // wait for the parent: Command::spawn itself waits for the exec result.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            libc::signal(libc::SIGTTOU, libc::SIG_IGN);
            if libc::tcsetpgrp(0, libc::getpid()) < 0 {
                return Err(io::Error::last_os_error());
            }
            for signal in [
                libc::SIGCHLD,
                libc::SIGHUP,
                libc::SIGTERM,
                libc::SIGINT,
                libc::SIGQUIT,
                libc::SIGTTOU,
                libc::SIGTTIN,
                libc::SIGTSTP,
            ] {
                libc::signal(signal, libc::SIG_DFL);
            }
            let mut empty = std::mem::zeroed::<libc::sigset_t>();
            libc::sigemptyset(&mut empty);
            if libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut()) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            if let Some(journal) = &journal {
                journal.started(Some(&error.to_string()))?;
                journal.finish()?;
            }
            return Err(error).context("Start supervised terminal command");
        }
    };
    let root = child.id() as i32;
    drop(child); // waitpid below exclusively owns reaping, including orphans.
    // The supervisor must not hold the slave open after its children exit.
    // From this point failures must never return while children still exist.
    let mut stopping = if journal
        .as_ref()
        .is_some_and(|journal| journal.started(None).is_err())
    {
        Some(Instant::now())
    } else {
        None
    };
    for fd in 0..=2 {
        if unsafe { libc::dup2(null.as_raw_fd(), fd) } < 0 {
            stopping = Some(Instant::now());
        }
    }
    drop(null);
    let mut root_status = None;
    loop {
        let mut empty = false;
        let mut changed = false;
        for _ in 0..2048 {
            let mut status = 0;
            let reaped = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
            changed |= reaped > 0;
            if reaped == root {
                root_status = Some(status);
                // A terminal command's natural exit also retires its remaining
                // descendants, including ones which detached and closed stdio.
                stopping.get_or_insert_with(Instant::now);
            }
            if reaped == 0 {
                break;
            }
            if reaped < 0 {
                if io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD) {
                    empty = true;
                }
                break;
            }
        }
        if empty {
            if let Some(journal) = &journal {
                journal.finish()?;
            }
            let status = root_status.unwrap_or(0);
            return Ok(if libc::WIFEXITED(status) {
                libc::WEXITSTATUS(status)
            } else if libc::WIFSIGNALED(status) {
                128 + libc::WTERMSIG(status)
            } else {
                1
            });
        }
        // Drain all pending signals. No handler, allocation or lock runs from
        // asynchronous signal context, and root/child SIGCHLD cannot be lost.
        loop {
            let mut info = unsafe { std::mem::zeroed::<libc::signalfd_siginfo>() };
            let read = unsafe {
                libc::read(
                    signals.as_raw_fd(),
                    std::ptr::from_mut(&mut info).cast(),
                    std::mem::size_of_val(&info),
                )
            };
            if read != std::mem::size_of_val(&info) as isize {
                break;
            }
            if info.ssi_signo == libc::SIGCHLD as u32 {
                changed = true;
            } else {
                stopping.get_or_insert_with(Instant::now);
            }
        }
        if let Some(at) = stopping {
            let signal = if at.elapsed() >= GRACE {
                libc::SIGKILL
            } else {
                libc::SIGHUP
            };
            // An inspection error leaves the supervisor alive and the source
            // unavailable for replacement. It is never evidence of retirement.
            let _ = signal_children(pid, signal);
        }
        if changed {
            continue;
        }
        let mut poll = [
            libc::pollfd {
                fd: signals.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if stopping.is_none() {
                    journal
                        .as_ref()
                        .map_or(-1, |journal| journal.owner_pidfd.as_raw_fd())
                } else {
                    -1
                },
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        unsafe {
            // Idle supervisors block on signals/owner death; retirement alone
            // has timed escalation. A signalled pidfd is disabled after use.
            libc::poll(
                poll.as_mut_ptr(),
                2,
                if stopping.is_some() { 20 } else { -1 },
            );
        }
        if poll[1].revents != 0 {
            stopping.get_or_insert_with(Instant::now);
        }
    }
}

fn signal_children(supervisor: i32, signal: i32) -> Result<()> {
    let path = format!("/proc/self/task/{supervisor}/children");
    let mut bytes = String::new();
    fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_string(&mut bytes)?;
    ensure!(
        bytes.len() <= 1024 * 1024,
        "Supervisor child list exceeds inspection budget"
    );
    for pid in bytes
        .split_whitespace()
        .take(2048)
        .filter_map(|pid| pid.parse::<i32>().ok())
    {
        let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if descriptor < 0 {
            continue;
        }
        let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor as i32) };
        // No other thread reaps children. A dying child stays our zombie and
        // its PID cannot be reused between enumeration and pidfd_open. Still
        // verify ancestry after opening the stable signal handle.
        let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let parent = stat
            .rsplit_once(')')
            .and_then(|(_, fields)| fields.split_whitespace().nth(1))
            .and_then(|pid| pid.parse::<i32>().ok());
        ensure!(
            parent == Some(supervisor),
            "Child ancestry changed; signal refused"
        );
        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                descriptor.as_raw_fd(),
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if result < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
            return Err(io::Error::last_os_error().into());
        }
    }
    Ok(())
}
