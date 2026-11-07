//! Bounded CLI reads/private staging whose watchdog survives caller cancellation.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs::File,
    io::{Read, Seek, Write},
    os::{fd::AsFd, unix::process::CommandExt},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const LIMIT: u64 = 2 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
struct Query {
    parent: u32,
    parent_start: u64,
    program: OsString,
    args: Vec<OsString>,
    environment: Vec<(OsString, Option<OsString>)>,
    directory: Option<PathBuf>,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
}
fn default_timeout() -> u64 {
    5000
}
pub fn output(command: Command) -> Result<Vec<u8>> {
    output_with_timeout(command, Duration::from_secs(5))
}
pub fn output_with_timeout(command: Command, timeout: Duration) -> Result<Vec<u8>> {
    ensure!(
        (1000..=90000).contains(&timeout.as_millis()),
        "Invalid command timeout"
    );
    let parent = std::process::id();
    let query = Query {
        parent,
        parent_start: crate::agent_state::process_start(parent)
            .context("Query owner unavailable")?,
        program: command.get_program().to_owned(),
        args: command.get_args().map(Into::into).collect(),
        environment: command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(Into::into)))
            .collect(),
        directory: command.get_current_dir().map(Into::into),
        timeout_ms: timeout.as_millis() as u64,
    };
    let mut file = tempfile::tempfile()?;
    // Keep the loaded helper protocol available after an on-disk upgrade.
    let executable = if cfg!(target_os = "linux") {
        PathBuf::from("/proc/self/exe")
    } else {
        std::env::current_exe()?
    };
    let mut worker = Command::new(executable)
        .arg("--read-query-worker")
        .stdin(Stdio::piped())
        .stdout(file.try_clone()?)
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let began = Instant::now();
    let mut reaped = false;
    let result = (|| -> Result<Vec<u8>> {
        let input = worker.stdin.take().context("No query command pipe")?;
        serde_json::to_writer(input, &query)?;
        loop {
            ensure!(
                began.elapsed() < timeout && file.metadata()?.len() <= LIMIT,
                "Read query exceeded its size or time limit"
            );
            if worker.try_wait()?.is_some() {
                reaped = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut control = Vec::new();
        worker
            .stderr
            .take()
            .context("No query status pipe")?
            .take(17)
            .read_to_end(&mut control)?;
        ensure!(
            control == b"ok\n",
            "Read query source unavailable or query cancelled"
        );
        file.rewind()?;
        let mut bytes = Vec::new();
        file.take(LIMIT + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= LIMIT,
            "Read query output exceeds its limit"
        );
        Ok(bytes)
    })();
    if !reaped {
        // The unreaped leader still reserves this group identity.
        unsafe {
            libc::kill(-(worker.id() as i32), libc::SIGKILL);
        }
        let _ = worker.wait();
    }
    result
}
fn stop_group() -> ! {
    // run() requires its own private group before it starts any query.
    unsafe {
        libc::kill(-libc::getpid(), libc::SIGKILL);
        libc::_exit(1);
    }
}
pub fn run() -> Result<()> {
    ensure!(
        unsafe { libc::getpgrp() == libc::getpid() },
        "Read query requires a private process group"
    );
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 1024 * 1024,
        "Read query command exceeds its limit"
    );
    let query: Query = serde_json::from_slice(&bytes)?;
    ensure!(
        (1000..=90000).contains(&query.timeout_ms),
        "Invalid command timeout"
    );
    let owner=|| unsafe{libc::getppid()}==query.parent as i32 && crate::agent_state::process_start(query.parent)==Some(query.parent_start);
    if !owner() {
        return Ok(());
    }
    let file = File::from(std::io::stdout().as_fd().try_clone_to_owned()?);
    ensure!(
        file.metadata()?.is_file(),
        "Read query needs private file output"
    );
    let mut command = Command::new(&query.program);
    command
        .args(&query.args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::null());
    for (key, value) in &query.environment {
        if let Some(value) = value {
            command.env(key, value);
        } else {
            command.env_remove(key);
        }
    }
    if let Some(directory) = &query.directory {
        command.current_dir(directory);
    }
    // The CLI and any helper children inherit this guardian's group; existing
    // source servers communicate over sockets and are never members of it.
    let mut child = command.spawn()?;
    let began = Instant::now();
    loop {
        if !owner()
            || began.elapsed() >= Duration::from_millis(query.timeout_ms.saturating_sub(1000))
            || !file.metadata().is_ok_and(|meta| meta.len() <= LIMIT)
        {
            stop_group();
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    let _ = std::io::stderr().write_all(b"ok\n");
                }
                // Report completion on the private status pipe, then remove
                // the entire group, including children left by CLI wrappers.
                stop_group();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => stop_group(),
        }
    }
}
