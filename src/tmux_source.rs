//! A live tmux import attaches a client; the source server keeps ownership.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub socket: PathBuf,
    pub session: String,
}
impl Source {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.socket.is_absolute(), "tmux socket must be absolute");
        ensure!(
            self.session
                .strip_prefix('$')
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())),
            "Invalid tmux session identity"
        );
        Ok(())
    }
    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("tmux");
        command
            .args(["-u", "-S", &self.socket.to_string_lossy()])
            .args(args)
            .env_remove("TMUX");
        command
    }
    fn output(&self, args: &[&str]) -> Result<Vec<u8>> {
        let output = self.command(args).output()?;
        ensure!(
            output.status.success(),
            "tmux source unavailable: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(output.stdout)
    }
    pub fn inspect(&self) -> Result<Value> {
        let bytes = self.output(&[
            "display-message",
            "-p",
            "-t",
            &self.session,
            "#{pane_pid}\t#{pane_current_command}\t#{pane_dead}",
        ])?;
        let text = String::from_utf8_lossy(&bytes);
        let fields = text.trim_end().split('\t').collect::<Vec<_>>();
        ensure!(fields.len() == 3, "Invalid tmux source status");
        let agent = match fields[1] {
            "codex" => Some("codex"),
            "claude" | "claude-code" => Some("claude"),
            _ => None,
        };
        let observation = agent.and_then(|_| {
            crate::activity::screen_observation(self.command(&[
                "capture-pane",
                "-p",
                "-t",
                &self.session,
            ]))
        });
        Ok(
            json!({"alive":fields[2]=="0","pid":fields[0].parse::<u32>().ok(),"command":fields[1],"agent":agent,"state":observation.as_ref().map(|v|v.0.as_str()).unwrap_or("idle"),"output_fingerprint":observation.map(|v|v.1),"source":"tmux","source_preserved":true}),
        )
    }
    pub fn attach_argv(&self) -> Vec<String> {
        vec![
            "env".into(),
            "-u".into(),
            "TMUX".into(),
            "tmux".into(),
            "-u".into(),
            "-S".into(),
            self.socket.to_string_lossy().into_owned(),
            "attach-session".into(),
            "-t".into(),
            self.session.clone(),
        ]
    }
    pub fn paste(&self, text: &str) -> Result<()> {
        ensure!(text.len() <= 1024 * 1024, "Paste exceeds 1 MiB");
        let name = format!("starflux-{}", uuid::Uuid::new_v4().simple());
        let mut child = self
            .command(&["load-buffer", "-b", &name, "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .context("No tmux paste stream")?
            .write_all(text.as_bytes())?;
        ensure!(
            child.wait_with_output()?.status.success(),
            "tmux rejected paste"
        );
        let result = self.output(&["paste-buffer", "-p", "-d", "-b", &name, "-t", &self.session]);
        if result.is_err() {
            let _ = self.command(&["delete-buffer", "-b", &name]).output();
        }
        result.map(|_| ())
    }
    pub fn call(&self, request: &Value) -> Result<Value> {
        match request["action"].as_str() {
            Some("ensure" | "inspect") => self.inspect(),
            Some("stop") => Ok(json!({"source_preserved":true})),
            Some("reap") => Ok(json!({"reaped":false,"source_preserved":true})),
            Some("paste") => {
                self.paste(request["text"].as_str().unwrap_or(""))?;
                Ok(json!({"pasted":true}))
            }
            Some("key") => {
                self.output(&[
                    "send-keys",
                    "-t",
                    &self.session,
                    request["key"].as_str().context("Missing key")?,
                ])?;
                Ok(json!({"sent":true}))
            }
            _ => anyhow::bail!(
                "Imported tmux sessions remain owned by tmux; open a new terminal tab for a fresh shell"
            ),
        }
    }
}
