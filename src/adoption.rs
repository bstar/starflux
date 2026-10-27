//! Live terminal ownership transferred from Herdr, independent of its runtime.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::{net::UnixStream, process::CommandExt},
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Terminal {
    pub broker: PathBuf,
    pub key: String,
    pub server: String,
    pub pid: u32,
    pub epoch: String,
    pub proxy: PathBuf,
}
impl Terminal {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.broker.is_absolute() && self.proxy.is_absolute(),
            "Transferred PTY paths must be absolute"
        );
        ensure!(
            self.pid > 1
                && !self.key.is_empty()
                && self.key.len() <= 64
                && self
                    .key
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "Invalid transferred terminal identity"
        );
        ensure!(
            self.server.starts_with("starmux-")
                && self.server.len() <= 96
                && self
                    .server
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
            "Invalid transferred terminal server"
        );
        Ok(())
    }
    pub fn inspect(&self) -> Result<Value> {
        let mut socket = UnixStream::connect(&self.broker)
            .context("Transferred PTY owner is unavailable; original process was not restarted")?;
        socket.set_read_timeout(Some(Duration::from_secs(2)))?;
        socket.set_write_timeout(Some(Duration::from_secs(2)))?;
        writeln!(socket, "{}", json!({"op":"inspect","key":self.key}))?;
        let mut line = String::new();
        BufReader::new(socket)
            .take(1024 * 1024)
            .read_line(&mut line)?;
        serde_json::from_str(&line).context("Invalid transferred PTY status")
    }
    pub fn argv(&self) -> Vec<String> {
        vec![
            self.proxy.to_string_lossy().into_owned(),
            self.broker.to_string_lossy().into_owned(),
            self.key.clone(),
        ]
    }
}
fn python(script: &str, args: &[&str]) -> Result<()> {
    Err(Command::new("python3")
        .args(["-c", script])
        .args(args)
        .exec())
    .context("Could not start native PTY migration helper")
}
pub fn receive(plan: &str, socket: &str, token: &str) -> Result<()> {
    python(
        include_str!("../scripts/adopt-pty.py"),
        &["receive", plan, socket, token],
    )
}
pub fn migrate(prepare_only: bool) -> Result<()> {
    ensure!(
        cfg!(target_os = "linux"),
        "Live Herdr PTY migration currently supports Linux hosts"
    );
    let executable = std::env::current_exe()?;
    let proxy = executable.with_file_name("starmux-pty-proxy");
    ensure!(
        proxy.is_file(),
        "Build the lightweight PTY helper first: ./scripts/build-pty-proxy"
    );
    python(
        include_str!("../scripts/migrate-herdr.py"),
        &[
            if prepare_only { "prepare" } else { "migrate" },
            &executable.to_string_lossy(),
            &proxy.to_string_lossy(),
        ],
    )
}
pub fn validate_plan(path: &str) -> Result<()> {
    let plan: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let documents = plan["documents"]
        .as_array()
        .context("Migration documents missing")?;
    for document in documents {
        let value = &document["value"];
        if document["kind"] == "herdr-retired" {
            ensure!(
                value["workspaces"].as_array().is_some_and(|v| v.is_empty()),
                "Invalid retired Herdr session"
            );
            continue;
        }
        if document["kind"] == "view" {
            continue;
        }
        if value.get("workspace").is_some() {
            let config: crate::runtime::Config = serde_json::from_value(value.clone())?;
            config.validate()?;
        } else {
            let state: crate::project_state::ProjectState = serde_json::from_value(value.clone())?;
            state.validate()?;
        }
    }
    Ok(())
}
pub fn resumed_frontend(workspace: &str) -> Option<String> {
    let path = crate::runtime::root()
        .ok()?
        .join(format!("{workspace}.resume-view.json"));
    let value: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    let identity = value["id"].as_str()?.to_owned();
    if !identity.starts_with("v-")
        || crate::session::socket_path(&crate::runtime::root().ok()?, &identity).is_err()
    {
        return None;
    }
    std::fs::remove_file(path).ok()?;
    Some(identity)
}
pub fn wait(workspace: &str) -> Result<()> {
    let marker = crate::runtime::root()?.join(format!("{workspace}.migration"));
    let start = Instant::now();
    while marker.exists() {
        let gate: Value = serde_json::from_slice(&std::fs::read(&marker)?)?;
        let run = gate["run"]
            .as_str()
            .context("Invalid terminal migration gate")?;
        if let Ok(bytes) = std::fs::read(PathBuf::from(run).join("status.json")) {
            let status: Value = serde_json::from_slice(&bytes)?;
            if matches!(status["phase"].as_str(), Some("active" | "failed")) {
                let _ = std::fs::remove_file(&marker);
                break;
            }
        }
        ensure!(
            start.elapsed() < Duration::from_secs(120),
            "Live terminal migration is still pending; original processes have not been restarted. Check {}",
            run
        );
        std::thread::sleep(Duration::from_millis(40));
    }
    Ok(())
}
