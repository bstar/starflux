use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
pub fn monotonic_ns() -> u64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time);
    }
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}
pub fn default_desktop_helper() -> String {
    "starflux".into()
}
const HELPER: &str = include_str!("../scripts/host.py");
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub workspace: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub collection: Option<String>,
    #[serde(default)]
    pub herdr: Option<crate::herdr::Source>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub adopted: std::collections::BTreeMap<usize, crate::adoption::Terminal>,
    #[serde(default = "crate::appearance::default_theme")]
    pub theme: String,
    pub host: Option<String>,
    #[serde(default)]
    pub ssh_config: Option<PathBuf>,
    pub directory: Option<String>,
    pub agent: Option<String>,
    pub executable: String,
    #[serde(default)]
    pub project_host: bool,
    #[serde(default = "default_desktop_helper")]
    pub desktop_helper: String,
}
/// New Codex instances get a fresh conversation rather than inheriting resume/fork arguments.
pub fn fresh_agent_command(command: &str) -> String {
    let argv = shell_words::split(command).unwrap_or_else(|_| vec!["codex".into()]);
    if let Some(index) = argv.iter().position(|arg| {
        std::path::Path::new(arg)
            .file_name()
            .is_some_and(|n| n == "codex")
    }) {
        shell_words::join(&argv[..=index])
    } else {
        command.into()
    }
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        crate::appearance::Appearance::load(&self.theme)?;
        for terminal in self.adopted.values() {
            terminal.validate()?;
        }
        ensure!(
            !self.workspace.is_empty()
                && self.workspace.len() <= 32
                && self
                    .workspace
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
            "Workspace must be 1–32 letters, digits, underscores or hyphens"
        );
        if let Some(h) = &self.host {
            ensure!(
                !h.is_empty()
                    && !h.starts_with('-')
                    && !h.chars().any(char::is_whitespace)
                    && !h.contains('\0'),
                "Invalid SSH host"
            );
        }
        ensure!(
            self.directory.as_ref().is_none_or(|s| !s.contains('\0')),
            "Invalid project directory"
        );
        ensure!(
            self.agent.as_ref().is_none_or(|s| !s.contains('\0')),
            "Invalid agent command"
        );
        ensure!(
            !self.desktop_helper.is_empty() && !self.desktop_helper.contains(['\0', '\n', '\r']),
            "Invalid desktop helper executable"
        );
        Ok(())
    }
    pub fn request(&self, pane: usize, action: &str) -> Value {
        let mut request =
            json!({"workspace":self.workspace,"pane":format!("p{pane}"),"action":action});
        if let Some(terminal) = self.adopted_terminal(pane) {
            request["server"] = json!(terminal.server);
            request["original_pid"] = json!(terminal.pid);
        }
        request
    }
    pub fn adopted_terminal(&self, pane: usize) -> Option<&crate::adoption::Terminal> {
        let terminal = self.adopted.get(&pane)?;
        let path = crate::project_state::data_root()
            .ok()?
            .join(format!("{}.model.json", self.workspace));
        let state: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
        (state["terminal_epochs"][pane.to_string()] == terminal.epoch).then_some(terminal)
    }
    pub fn remote(&self, pane: usize) -> bool {
        self.host.is_some() && (pane != 0 || self.project_host)
    }
    pub fn command(&self, pane: usize, request: &Value, tty: bool) -> Command {
        if let Some(source) = self.herdr.as_ref().filter(|h| h.panes.contains_key(&pane)) {
            let p = &source.panes[&pane];
            return source.command(&[
                "pane", "read", &p.pane_id, "--source", "visible", "--format", "text",
            ]);
        }

        let encoded = STANDARD.encode(serde_json::to_vec(request).unwrap());
        if self.remote(pane) {
            let mut c = Command::new("ssh");
            if let Some(path) = &self.ssh_config {
                c.arg("-F").arg(path);
            }
            c.args([
                if tty { "-tt" } else { "-T" },
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-o",
                "ConnectTimeout=10",
                "--",
                self.host.as_ref().unwrap(),
            ]);
            c.arg(format!(
                "python3 -c {} {}",
                shell_words::quote(HELPER),
                shell_words::quote(&encoded)
            ));
            c
        } else {
            let mut c = Command::new("python3");
            c.args(["-c", HELPER, &encoded]);
            c
        }
    }
    pub fn call(&self, pane: usize, request: &Value) -> Result<Value> {
        if request["action"] == "inspect"
            && let Some(terminal) = self.adopted_terminal(pane)
        {
            return terminal.inspect();
        }
        if let Some(terminal) = self.adopted_terminal(pane) {
            if request["action"] == "restart" && request["argv"].is_null() {
                let mut request = request.clone();
                request["argv"] = json!(terminal.argv());
                return self.call(pane, &request);
            }
            if request["action"] == "stop" && terminal.inspect()?["alive"] == true {
                // An explicit project stop hangs up the original controlling session,
                // rather than stopping only its presentation proxy.
                let stat = fs::read_to_string(format!("/proc/{}/stat", terminal.pid))?;
                let fields = stat
                    .rsplit_once(')')
                    .context("Invalid original terminal process")?
                    .1
                    .split_whitespace()
                    .collect::<Vec<_>>();
                ensure!(
                    fields.get(3).and_then(|v| v.parse::<u32>().ok()) == Some(terminal.pid),
                    "Original terminal session identity changed; it was left running"
                );
                if unsafe { libc::kill(terminal.pid as libc::pid_t, libc::SIGHUP) } != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(error.into());
                    }
                }
            }
        }
        if let Some(source) = self.herdr.as_ref().filter(|h| h.panes.contains_key(&pane)) {
            return match request["action"].as_str() {
                Some("inspect" | "ensure") => source.inspect(pane),
                Some("stop") => Ok(json!({"source_preserved":true})),
                _ => crate::herdr::reject_write(),
            };
        }

        let out = self
            .command(pane, request, false)
            .stdin(Stdio::null())
            .output()?;
        ensure!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(serde_json::from_slice(&out.stdout)?)
    }
    pub fn ensure(&self, pane: usize, agent_command: Option<&str>) -> Result<()> {
        if let Some(source) = self.herdr.as_ref().filter(|h| h.panes.contains_key(&pane)) {
            source.inspect(pane)?;
            return Ok(());
        }
        let mut req = self.request(pane, "ensure");
        req["directory"] = if (pane != 0 || self.project_host) && self.directory.is_some() {
            json!(self.directory)
        } else {
            json!(std::env::current_dir()?.to_string_lossy())
        };
        req["replace_fixture"] = json!(self.agent.is_none());
        if let Some(terminal) = self.adopted_terminal(pane) {
            req["argv"] = json!(terminal.argv());
        } else if let Some(command) = agent_command {
            req["argv"] = json!(shell_words::split(command)?);
        }
        self.call(pane, &req)?;
        Ok(())
    }
    pub fn canonical_pane(&self, pane: usize) -> usize {
        if let Some(source) = &self.herdr
            && let Some(p) = source.panes.get(&pane)
        {
            return source
                .panes
                .iter()
                .find(|(id, candidate)| **id != 31 && candidate.pane_id == p.pane_id)
                .map(|(id, _)| *id)
                .unwrap_or(pane);
        }
        pane
    }
    pub fn paste(&self, pane: usize, text: &str) -> Result<()> {
        let socket = crate::terminal::socket(&self.workspace, self.canonical_pane(pane))?;
        if socket.exists() {
            return crate::terminal::paste(&socket, text);
        }
        // A CLI delivery can run outside the presenting controller process.
        // Every native attachment reaches the same backend terminal, so use an open one.
        if let Ok(entries) = fs::read_dir(root()?) {
            let expected = format!("{}-{}.sock", self.workspace, self.canonical_pane(pane));
            let compact =
                crate::terminal::socket_suffix(&self.workspace, self.canonical_pane(pane));
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name
                    .strip_prefix("terminal-")
                    .or_else(|| name.strip_prefix("t-"))
                    .and_then(|s| s.split_once('-'))
                    .is_some_and(|(pid, target)| {
                        pid.bytes().all(|b| b.is_ascii_digit())
                            && (target == expected || target == compact)
                    })
                {
                    return crate::terminal::paste(&entry.path(), text);
                }
            }
        }
        if self
            .herdr
            .as_ref()
            .is_some_and(|h| h.panes.contains_key(&pane))
        {
            let pane = self.canonical_pane(pane);
            let socket = root()?.join(format!("kitty-{}-{pane}.sock", self.workspace));
            ensure!(
                socket.exists(),
                "Open the imported terminal before pasting into it"
            );
            let mut child =
                Command::new(std::env::var("STARFLUX_KITTEN").unwrap_or_else(|_| "kitten".into()))
                    .args([
                        "@",
                        "--to",
                        &format!("unix:{}", socket.display()),
                        "send-text",
                        "--stdin",
                        "--bracketed-paste",
                        "enable",
                    ])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()?;
            child
                .stdin
                .take()
                .context("No paste stream")?
                .write_all(text.as_bytes())?;
            let result = child.wait_with_output()?;
            ensure!(
                result.status.success(),
                "Herdr attachment paste: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            return Ok(());
        }

        let mut req = self.request(pane, "paste");
        req["text"] = json!(text);
        self.call(pane, &req)?;
        Ok(())
    }
    pub fn attach_argv(&self, pane: usize) -> Vec<String> {
        self.attach_argv_with_shell(pane, false)
    }
    pub fn attach_argv_with_shell(&self, pane: usize, ensure_shell: bool) -> Vec<String> {
        if let Some(source) = self.herdr.as_ref().filter(|h| h.panes.contains_key(&pane)) {
            let p = &source.panes[&pane];
            if !p.terminal_id.is_empty() {
                return vec![
                    source.executable.clone(),
                    "--session".into(),
                    source.session.clone(),
                    "terminal".into(),
                    "attach".into(),
                    p.terminal_id.clone(),
                ];
            }
            if p.agent.is_some() {
                return vec![
                    source.executable.clone(),
                    "--session".into(),
                    source.session.clone(),
                    "agent".into(),
                    "attach".into(),
                    p.pane_id.clone(),
                ];
            }
            return vec![
                std::env::current_exe().unwrap().display().to_string(),
                "--herdr-view".into(),
                source.session.clone(),
                p.pane_id.clone(),
            ];
        }

        let mut request = self.request(pane, "attach");
        if ensure_shell {
            if let Some(terminal) = self.adopted_terminal(pane) {
                request["argv"] = json!(terminal.argv());
            }
            request["ensure_shell"] = json!(true);
            request["replace_fixture"] = json!(self.agent.is_none());
            request["directory"] = if (pane != 0 || self.project_host) && self.directory.is_some() {
                json!(self.directory)
            } else {
                json!(
                    std::env::current_dir()
                        .unwrap_or_default()
                        .to_string_lossy()
                )
            };
        }
        let encoded = STANDARD.encode(serde_json::to_vec(&request).unwrap());
        if self.remote(pane) {
            {
                let mut v = vec!["ssh".into()];
                if let Some(path) = &self.ssh_config {
                    v.extend(["-F".into(), path.to_string_lossy().into_owned()]);
                }
                v.extend(vec![
                    "-tt".into(),
                    "-o".into(),
                    "ServerAliveInterval=15".into(),
                    "-o".into(),
                    "ServerAliveCountMax=3".into(),
                    "--".into(),
                    self.host.clone().unwrap(),
                    format!(
                        "python3 -c {} {}",
                        shell_words::quote(HELPER),
                        shell_words::quote(&encoded)
                    ),
                ]);
                v
            }
        } else {
            vec!["python3".into(), "-c".into(), HELPER.into(), encoded]
        }
    }
    pub fn upload(
        &self,
        pane: usize,
        path: &Path,
        progress: impl Fn(u64, u64) -> bool,
    ) -> Result<Value> {
        let meta = fs::symlink_metadata(path)?;
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "Demo accepts regular files only"
        );
        ensure!(meta.len() <= 64 * 1024 * 1024, "Demo file limit is 64 MiB");
        let mut input = fs::File::open(path)?;
        let mut digest = Sha256::new();
        let mut chunk = [0u8; 65536];
        loop {
            let n = input.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            digest.update(&chunk[..n]);
        }
        use std::io::{Seek, SeekFrom};
        input.seek(SeekFrom::Start(0))?;
        let mut req = self.request(pane, "upload");
        req["name"] = json!(
            path.file_name()
                .context("Missing file name")?
                .to_str()
                .context("File name must be UTF-8")?
        );
        req["size"] = json!(meta.len());
        req["sha256"] = json!(format!("{:x}", digest.finalize()));
        let mut transport = self.clone();
        transport.herdr = None;
        let mut child = transport
            .command(pane, &req, false)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let result = (|| -> Result<()> {
            let mut out = child.stdin.take().context("Missing transfer stream")?;
            let mut count = 0;
            loop {
                let n = input.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                out.write_all(&chunk[..n])?;
                count += n as u64;
                ensure!(progress(count, meta.len()), "Transfer canceled");
            }
            ensure!(count == meta.len(), "File size changed during transfer");
            Ok(())
        })();
        if let Err(e) = result {
            // Dropping stdin sends EOF so the host can clean its partial file.
            // Killing the helper here would bypass that cleanup.
            let _ = child.wait_with_output();
            return Err(e);
        }
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!("Upload: {}", String::from_utf8_lossy(&out.stderr).trim())
        }
        let response: Value = serde_json::from_slice(&out.stdout)?;
        ensure!(response["sha256"] == req["sha256"], "Host checksum differs");
        Ok(response)
    }
}
pub fn initialize_environment() -> Result<()> {
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        use std::os::unix::fs::MetadataExt;
        let uid = unsafe { libc::geteuid() };
        let native = PathBuf::from(format!("/run/user/{uid}"));
        let runtime = if fs::metadata(&native)
            .is_ok_and(|m| m.is_dir() && m.uid() == uid && m.mode() & 0o077 == 0)
        {
            native
        } else {
            PathBuf::from(format!("/tmp/starflux-{uid}"))
        };
        starkit::terminal_graphics::session::private_root(&runtime)?;
        // CLI entry calls this before starting any threads or frontend probes.
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", runtime);
        }
    }
    Ok(())
}
pub fn root() -> Result<PathBuf> {
    let r = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/starflux-{}", unsafe { libc::geteuid() })))
        .join("starflux");
    private_directory(r)
}
/// Resolve an owned migration alias while retaining private-root checks.
/// Live workers keep using their original directory until they exit.
pub fn private_directory(path: PathBuf) -> Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let uid = unsafe { libc::geteuid() };
    let path = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            ensure!(
                metadata.uid() == uid,
                "Runtime alias belongs to another user"
            );
            fs::canonicalize(&path)?
        }
        _ => path,
    };
    starkit::terminal_graphics::session::private_root(&path)?;
    ensure!(
        fs::metadata(&path)?.uid() == uid,
        "Runtime directory belongs to another user"
    );
    Ok(path)
}
pub fn config_path(name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty()
            && name.len() <= 32
            && name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
        "Invalid workspace name"
    );
    Ok(root()?.join(format!("{name}.json")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_aliases_retain_private_runtime_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("retained-worker");
        let resolved = private_directory(directory.clone()).unwrap();
        let alias = temp.path().join("renamed-worker");
        symlink(&directory, &alias).unwrap();
        assert_eq!(private_directory(alias.clone()).unwrap(), resolved);
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(private_directory(alias).is_err());
    }
    #[test]
    fn fresh_shell_attach_combines_setup_and_attach_in_project_directory() {
        let config: Config = serde_json::from_value(json!({
            "workspace":"shell-attach", "host":null,
            "directory":"/tmp/project with spaces", "agent":null, "executable":"starflux"
        }))
        .unwrap();
        let args = config.attach_argv_with_shell(2, true);
        assert_eq!(args[0], "python3");
        let req: Value =
            serde_json::from_slice(&STANDARD.decode(args.last().unwrap()).unwrap()).unwrap();
        assert_eq!(req["action"], "attach");
        assert_eq!(req["ensure_shell"], true);
        assert_eq!(req["directory"], "/tmp/project with spaces");
        assert!(req.get("argv").is_none());
        let ordinary = config.attach_argv(2);
        let req: Value =
            serde_json::from_slice(&STANDARD.decode(ordinary.last().unwrap()).unwrap()).unwrap();
        assert!(req.get("ensure_shell").is_none());
    }
    #[test]
    fn additional_codex_agents_never_reuse_resume_or_fork_arguments() {
        for command in [
            "codex",
            "codex resume --last",
            "codex fork old-session",
            "codex --model selected resume old-session",
        ] {
            assert_eq!(fresh_agent_command(command), "codex");
        }
        assert_eq!(
            fresh_agent_command("/usr/bin/codex resume old-session"),
            "/usr/bin/codex"
        );
        assert_eq!(
            shell_words::split(&fresh_agent_command("env TEST=value codex resume --last")).unwrap(),
            ["env", "TEST=value", "codex"]
        );
        assert_eq!(
            fresh_agent_command("python3 fixture.py"),
            "python3 fixture.py"
        );
    }
    #[test]
    fn imported_agent_and_tab_share_one_live_terminal_without_takeover() {
        let pane = crate::herdr::Pane {
            terminal_id: "term_original".into(),
            pane_id: "w1:p1".into(),
            tab_id: "w1:t1".into(),
            label: "Original".into(),
            cwd: "/tmp".into(),
            agent: Some("codex".into()),
        };
        let mut c:Config=serde_json::from_value(json!({"workspace":"import-test","host":null,"directory":"/tmp","agent":"codex","executable":"starflux"})).unwrap();
        c.herdr = Some(crate::herdr::Source {
            session: "default".into(),
            workspace_id: "w1".into(),
            order: 0,
            executable: "/nonexistent/herdr".into(),
            panes: [(0, pane.clone()), (31, pane)].into_iter().collect(),
        });
        assert_eq!(c.canonical_pane(31), 0);
        assert_eq!(c.canonical_pane(0), 0);
        assert_eq!(
            c.attach_argv(0),
            vec![
                "/nonexistent/herdr",
                "--session",
                "default",
                "terminal",
                "attach",
                "term_original"
            ]
        );
        assert_eq!(c.attach_argv(0), c.attach_argv(31));
        assert!(
            !c.attach_argv(0)
                .iter()
                .any(|arg| arg == "--takeover" || arg == "codex")
        );
        assert_eq!(
            c.call(31, &c.request(31, "stop")).unwrap()["source_preserved"],
            true
        );
    }
    #[test]
    fn saved_workspaces_before_blueprint_use_the_default_palette() {
        let c: Config=serde_json::from_value(json!({"workspace":"legacy","host":null,"directory":null,"agent":null,"executable":"starflux"})).unwrap();
        assert_eq!(c.theme, "kanagawa");
        c.validate().unwrap();
    }
    #[test]
    fn canceled_upload_cleans_partial_and_completed_upload_matches_bytes() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let file = tmp.path().join("screen shot λ.png");
        let bytes = vec![137u8; 2 * 1024 * 1024];
        fs::write(&file, &bytes)?;
        let workspace = format!("upload-test-{}", std::process::id());
        let config = Config {
            workspace: workspace.clone(),
            label: None,
            collection: None,
            herdr: None,
            adopted: Default::default(),
            theme: crate::appearance::default_theme(),
            host: None,
            ssh_config: None,
            directory: None,
            agent: None,
            executable: String::new(),
            project_host: true,
            desktop_helper: default_desktop_helper(),
        };
        let staged = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap()).join(".local/share")
            })
            .join("starflux/attachments")
            .join(&workspace);
        let result = config.upload(0, &file, |count, _| count < 65536);
        assert!(format!("{:#}", result.unwrap_err()).contains("canceled"));
        if staged.join("p0").exists() {
            assert_eq!(
                fs::read_dir(staged.join("p0"))?.count(),
                0,
                "Canceled upload left partial staging"
            );
        }
        let result = config.upload(0, &file, |_, _| true)?;
        assert_eq!(fs::read(result["path"].as_str().unwrap())?, bytes);
        fs::remove_dir_all(staged)?;
        Ok(())
    }
}
