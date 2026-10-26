//! Asynchronous, read-only Git inspection on the project host.
use crate::runtime::Config;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const HELPER: &str = include_str!("../scripts/git-status.py");
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Lines {
    pub additions: u64,
    pub deletions: u64,
    pub binary: u64,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct File {
    pub path: String,
    pub original: Option<String>,
    pub index: String,
    pub worktree: String,
    pub conflict: bool,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Status {
    pub kind: String,
    pub root: String,
    pub branch: String,
    pub upstream: String,
    pub ahead: u64,
    pub behind: u64,
    pub unborn: bool,
    pub staged: u64,
    pub unstaged: u64,
    pub untracked: u64,
    pub conflicts: u64,
    pub stash: u64,
    pub operation: String,
    pub last_commit: String,
    pub staged_lines: Lines,
    pub unstaged_lines: Lines,
    pub files: Vec<File>,
    pub total_files: usize,
    pub truncated: bool,
    pub error: String,
}
impl Status {
    pub fn clean(&self) -> bool {
        self.total_files == 0
    }
    pub fn summary(&self) -> String {
        match self.kind.as_str() {
            "repo" => format!(
                "{} · {} staged · {} modified · {} new · +{} −{}{}{}",
                self.branch,
                self.staged,
                self.unstaged,
                self.untracked,
                self.staged_lines.additions + self.unstaged_lines.additions,
                self.staged_lines.deletions + self.unstaged_lines.deletions,
                if self.upstream.is_empty() {
                    " · no upstream".into()
                } else {
                    format!(" · ↑{} ↓{}", self.ahead, self.behind)
                },
                if self.conflicts > 0 {
                    format!(" · {} CONFLICTS", self.conflicts)
                } else if !self.operation.is_empty() {
                    format!(" · {}", self.operation)
                } else if self.clean() {
                    " · clean".into()
                } else {
                    String::new()
                }
            ),
            "none" => "No Git repository".into(),
            _ => "Git unavailable · click for details".into(),
        }
    }
}
pub fn read(config: &Config) -> Result<Status> {
    let directory = config.directory.clone().unwrap_or_else(|| ".".into());
    let mut command = if let Some(host) = &config.host {
        let mut cmd = Command::new("ssh");
        if let Some(path) = &config.ssh_config {
            cmd.arg("-F").arg(path);
        }
        cmd.args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "ServerAliveInterval=5",
            "-o",
            "ServerAliveCountMax=1",
            "--",
            host,
        ]);
        cmd.arg(format!(
            "python3 -c {} {}",
            shell_words::quote(HELPER),
            shell_words::quote(&directory)
        ));
        cmd
    } else {
        let mut cmd = Command::new("python3");
        cmd.args(["-c", HELPER, &directory]);
        cmd
    };
    // Spool bounded-time helper output into a private temporary file rather than
    // blocking on a full pipe while waiting for a stalled SSH process.
    let mut output = tempfile::tempfile()?;
    let mut error = tempfile::tempfile()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(error.try_clone()?)
        .spawn()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Git inspection timed out");
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    use std::io::{Read, Seek};
    error.rewind()?;
    let mut message = String::new();
    error.take(1024).read_to_string(&mut message)?;
    ensure!(status.success(), "Git host: {}", message.trim());
    output.rewind()?;
    Ok(serde_json::from_reader(output.take(2 * 1024 * 1024))?)
}
