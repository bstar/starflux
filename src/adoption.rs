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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_boot_id: Option<String>,
}
impl Terminal {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.owner_boot_id
                .as_deref()
                .is_none_or(|id| uuid::Uuid::parse_str(id).is_ok()),
            "Invalid transferred terminal boot identity"
        );
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
            self.server.starts_with("starflux-")
                && self.server.len() <= 96
                && self
                    .server
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
            "Invalid transferred terminal server"
        );
        Ok(())
    }
    fn recovery_path(&self, config: &crate::runtime::Config, pane: usize) -> Result<PathBuf> {
        Ok(crate::project_state::data_root()?
            .parent()
            .context("No recovery directory")?
            .join("terminal-recovery")
            .join(&config.workspace)
            .join(format!("p{pane}.json")))
    }
    fn fingerprint(&self) -> Result<String> {
        use sha2::{Digest, Sha256};
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
    pub fn retired(&self, config: &crate::runtime::Config, pane: usize) -> bool {
        let Ok(path) = self.recovery_path(config, pane) else {
            return false;
        };
        let Ok(record) = crate::plugin_store::read::<Value>(&path, 8192) else {
            return false;
        };
        record["version"] == 1 && self.fingerprint().is_ok_and(|id| record["terminal"] == id)
    }
    /// Reboot proof is required: a missing socket alone can be a temporary broker failure.
    /// Keep a durable receipt because a new shell will replace the old screen's boot metadata.
    pub fn retire_after_reboot(
        &self,
        config: &crate::runtime::Config,
        pane: usize,
    ) -> Result<bool> {
        if self.retired(config, pane) {
            return Ok(true);
        }
        let Some(current) = crate::agent_state::boot_id() else {
            return Ok(false);
        };
        let owner = self
            .owner_boot_id
            .as_deref()
            .and_then(|id| uuid::Uuid::parse_str(id).ok())
            .or_else(|| {
                crate::cold_start::load(&config.workspace, pane, false)
                    .ok()
                    .filter(|s| {
                        s.pid == Some(self.pid) && s.epoch.as_deref() == Some(self.epoch.as_str())
                    })
                    .and_then(|s| {
                        s.observer_boot_id
                            .and_then(|id| uuid::Uuid::parse_str(&id).ok())
                    })
            });
        let Some(owner) = owner.filter(|owner| *owner != current) else {
            return Ok(false);
        };
        let path = self.recovery_path(config, pane)?;
        starkit::terminal_graphics::session::private_root(
            path.parent().context("No recovery directory")?,
        )?;
        crate::automation::persist(
            &path,
            &serde_json::to_vec(
                &json!({"version":1,"terminal":self.fingerprint()?,"owner_boot":owner.to_string(),"recovered_boot":current.to_string()}),
            )?,
        )?;
        Ok(true)
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
        let mut args = vec![
            self.proxy.to_string_lossy().into_owned(),
            self.broker.to_string_lossy().into_owned(),
            self.key.clone(),
        ];
        if let Ok(tty) = std::fs::read_link(format!("/proc/{}/fd/1", self.pid))
            && tty.starts_with("/dev/pts")
        {
            args.push(tty.to_string_lossy().into_owned());
            args.push(self.pid.to_string());
        }
        args
    }
    pub fn bridge_alive(&self, pane: usize) -> Result<bool> {
        let result = Command::new("tmux")
            .args([
                "-L",
                &self.server,
                "display-message",
                "-p",
                "-t",
                &format!("p{pane}:0.0"),
                "#{pane_dead}",
            ])
            .output()
            .context("Could not inspect the transferred PTY bridge")?;
        Ok(result.status.success() && result.stdout == b"0\n")
    }
    pub fn observe(&self, pane: usize) -> Result<Value> {
        let mut health = self.inspect()?;
        health["bridge_connected"] = json!(self.bridge_alive(pane)?);
        // The retained broker may predate directory reporting. Read the original
        // foreground agent or shell, never the tmux presentation proxy.
        if health["alive"] == true {
            let pid = health["agent_pid"].as_u64().unwrap_or(u64::from(self.pid));
            let cwd = std::fs::read_link(format!("/proc/{pid}/cwd"))
                .or_else(|_| std::fs::read_link(format!("/proc/{}/cwd", self.pid)));
            if let Ok(cwd) = cwd {
                health["cwd"] = json!(cwd);
            }
        }
        // Retained owners can predate current footer detection. Observe the
        // live screen here instead of trusting their cached interpretation.
        if health["alive"] == true && matches!(health["agent"].as_str(), Some("codex" | "claude")) {
            let mut command = Command::new("tmux");
            command.args([
                "-L",
                &self.server,
                "capture-pane",
                "-p",
                "-t",
                &format!("p{pane}:0.0"),
            ]);
            health["state"] = json!(
                crate::activity::screen_observation(command)
                    .map(|(state, _)| state)
                    .unwrap_or_else(|| "unknown".into())
            );
        }
        Ok(health)
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
    let proxy = executable.with_file_name("starflux-pty-proxy");
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
