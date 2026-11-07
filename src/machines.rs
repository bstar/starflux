//! Saved SSH endpoints. Profiles never own or stop the remote processes.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Seek},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    pub id: String,
    pub revision: String,
    pub label: String,
    pub target: String,
    pub ssh_config: Option<PathBuf>,
    pub remote_executable: String,
    pub workspace: Option<String>,
    pub enabled: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Save {
    pub id: Option<String>,
    pub expected_revision: Option<String>,
    pub label: String,
    pub target: String,
    pub ssh_config: Option<PathBuf>,
    #[serde(default = "crate::runtime::default_desktop_helper")]
    pub remote_executable: String,
    pub workspace: Option<String>,
    #[serde(default = "enabled")]
    pub enabled: bool,
}
fn enabled() -> bool {
    true
}
impl From<Machine> for Save {
    fn from(m: Machine) -> Self {
        Self {
            id: Some(m.id),
            expected_revision: Some(m.revision),
            label: m.label,
            target: m.target,
            ssh_config: m.ssh_config,
            remote_executable: m.remote_executable,
            workspace: m.workspace,
            enabled: m.enabled,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    version: u8,
    machines: Vec<Machine>,
}
fn path() -> Result<PathBuf> {
    let root = if let Some(p) = std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        PathBuf::from(p)
    } else {
        PathBuf::from(std::env::var_os("HOME").context("HOME is unavailable")?).join(".config")
    }
    .join("starflux");
    ensure!(
        root.is_absolute(),
        "Configuration directory must be absolute"
    );
    starkit::terminal_graphics::session::private_root(&root)?;
    Ok(root.join("machines.json"))
}
fn identifier(id: &str) -> Result<()> {
    ensure!(
        id.starts_with("machine-") && id.len() == 40 && uuid::Uuid::parse_str(&id[8..]).is_ok(),
        "Invalid machine identity"
    );
    Ok(())
}
impl Machine {
    pub fn validate(&self) -> Result<()> {
        identifier(&self.id)?;
        uuid::Uuid::parse_str(&self.revision).context("Invalid machine revision")?;
        crate::project_state::validate_label(&self.label)?;
        ensure!(
            !self.target.is_empty()
                && self.target.len() <= 512
                && !self.target.starts_with('-')
                && !self
                    .target
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control()),
            "Invalid SSH target"
        );
        ensure!(
            !self.remote_executable.is_empty()
                && self.remote_executable.len() <= 2048
                && !self.remote_executable.chars().any(char::is_control),
            "Invalid remote executable"
        );
        if let Some(workspace) = &self.workspace {
            crate::runtime::config_path(workspace)?;
        }
        if let Some(p) = &self.ssh_config {
            ensure!(
                p.is_absolute() && !p.as_os_str().is_empty(),
                "SSH configuration path must be absolute"
            );
        }
        Ok(())
    }
    pub fn setup_command(&self) -> String {
        let mut argv = vec!["ssh".to_owned()];
        if let Some(p) = &self.ssh_config {
            argv.extend(["-F".into(), p.display().to_string()]);
        }
        argv.extend(["--".into(), self.target.clone()]);
        shell_words::join(argv)
    }
    pub fn matches(&self, config: &crate::runtime::Config) -> bool {
        config.host.as_deref() == Some(self.target.as_str()) && config.ssh_config == self.ssh_config
    }
}
pub fn list() -> Result<Vec<Machine>> {
    let file = match fs::File::open(path()?) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 1024 * 1024, "Machine catalog exceeds 1 MiB");
    let catalog: Catalog = serde_json::from_slice(&bytes)
        .context("Read saved machines; existing profiles were preserved")?;
    ensure!(catalog.version == 1, "Unsupported machine catalog version");
    ensure!(
        catalog.machines.len() <= 256,
        "Machine catalog exceeds 256 profiles"
    );
    let mut ids = std::collections::BTreeSet::new();
    for m in &catalog.machines {
        m.validate()?;
        ensure!(ids.insert(&m.id), "Duplicate machine identity");
    }
    Ok(catalog.machines)
}
pub fn get(id: &str) -> Result<Machine> {
    let machines = list()?;
    let matches = machines
        .into_iter()
        .filter(|m| m.id == id || m.label == id)
        .collect::<Vec<_>>();
    ensure!(
        matches.len() <= 1,
        "Ambiguous machine label; use its identity"
    );
    matches
        .into_iter()
        .next()
        .context("Saved machine not found")
}
fn write(machines: Vec<Machine>) -> Result<()> {
    crate::automation::persist(
        &path()?,
        &serde_json::to_vec_pretty(&Catalog {
            version: 1,
            machines,
        })?,
    )
}
pub fn save(input: Save) -> Result<Machine> {
    let _lock = crate::project_sync::Transaction::acquire(path()?.with_extension("lock"))?;
    let mut machines = list()?;
    ensure!(
        machines.len() < 256 || input.id.is_some(),
        "Machine limit reached (256)"
    );
    let id = input
        .id
        .clone()
        .unwrap_or_else(|| format!("machine-{}", uuid::Uuid::new_v4().simple()));
    let old = machines.iter().position(|m| m.id == id);
    if input.id.is_some() {
        let index = old.context("Machine was removed; reload the list")?;
        ensure!(
            input.expected_revision.as_deref() == Some(&machines[index].revision),
            "Machine changed; reload before editing"
        );
    } else {
        ensure!(
            input.expected_revision.is_none(),
            "New machine cannot specify an existing revision"
        );
    }
    let ssh_config = input
        .ssh_config
        .map(|p| fs::canonicalize(p).context("SSH configuration file is unavailable"))
        .transpose()?;
    if let Some(p) = &ssh_config {
        ensure!(p.is_file(), "SSH configuration must be a regular file");
    }
    let m = Machine {
        id,
        revision: uuid::Uuid::new_v4().to_string(),
        label: input.label,
        target: input.target,
        ssh_config,
        remote_executable: input.remote_executable,
        workspace: input.workspace,
        enabled: input.enabled,
    };
    m.validate()?;
    ensure!(
        !machines
            .iter()
            .any(|other| other.id != m.id && other.label == m.label),
        "A machine with this label already exists"
    );
    if let Some(index) = old {
        machines[index] = m.clone();
    } else {
        machines.push(m.clone());
    }
    machines.sort_by_key(|m| (m.label.to_lowercase(), m.id.clone()));
    write(machines)?;
    Ok(m)
}
pub fn remove(id: &str, revision: &str) -> Result<()> {
    let _lock = crate::project_sync::Transaction::acquire(path()?.with_extension("lock"))?;
    let mut machines = list()?;
    let index = machines
        .iter()
        .position(|m| m.id == id)
        .context("Machine already removed")?;
    ensure!(
        machines[index].revision == revision,
        "Machine changed; reload before removal"
    );
    machines.remove(index);
    write(machines)
}
fn ssh(m: &Machine, argv: &[&str]) -> Command {
    let mut c = Command::new("ssh");
    if let Some(p) = &m.ssh_config {
        c.arg("-F").arg(p);
    }
    c.args([
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "ConnectTimeout=5",
        "-o",
        "ServerAliveInterval=5",
        "-o",
        "ServerAliveCountMax=1",
        "--",
        &m.target,
    ]);
    c.arg(shell_words::join(argv));
    c
}
fn output(mut command: Command) -> Result<(bool, String, String)> {
    let mut out = tempfile::tempfile()?;
    let mut err = tempfile::tempfile()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(err.try_clone()?)
        .process_group(0)
        .spawn()?;
    let start = Instant::now();
    let status = loop {
        if start.elapsed() > Duration::from_secs(12)
            || out.metadata()?.len() + err.metadata()?.len() > 1024 * 1024
        {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            bail!(
                "SSH check exceeded its time or output limit; other machine sessions are unaffected"
            );
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    ensure!(
        out.metadata()?.len() + err.metadata()?.len() <= 1024 * 1024,
        "SSH check exceeded its output limit; other machine sessions are unaffected"
    );
    out.rewind()?;
    err.rewind()?;
    let mut stdout = String::new();
    let mut stderr = String::new();
    out.take(1024 * 1024).read_to_string(&mut stdout)?;
    err.take(8192).read_to_string(&mut stderr)?;
    Ok((status.success(), stdout, stderr))
}
/// Read-only onboarding. No server, package, shell, or application is started.
pub fn test(m: &Machine) -> Result<Value> {
    m.validate()?;
    let (ok, stdout, stderr) = output(ssh(
        m,
        &[
            "python3",
            "-c",
            "import json,platform,shutil;print(json.dumps(dict(system=platform.system(),python=platform.python_version(),tmux=shutil.which('tmux'),git=shutil.which('git'))))",
        ],
    ))?;
    ensure!(
        ok,
        "SSH/Python check failed: {}. Run {} interactively to check authentication and the host key; retry after setup",
        stderr.trim(),
        m.setup_command()
    );
    let host: Value = serde_json::from_str(stdout.trim())
        .context("SSH host did not return a clean JSON response; check shell startup output")?;
    let req = r#"{"version":1,"id":"machine-check","method":"capabilities","params":{}}"#;
    let (ok, stdout, stderr) = match output(ssh(m, &[&m.remote_executable, "--api", req])) {
        Ok(result) => result,
        Err(e) => (false, String::new(), format!("{e:#}")),
    };
    let response = serde_json::from_str::<Value>(stdout.trim()).ok();
    let api = ok
        && response.as_ref().is_some_and(|v| {
            v["version"] == 1
                && v["id"] == "machine-check"
                && v["ok"] == true
                && v["result"]["version"] == 1
        });
    Ok(
        json!({"machine":m.id,"revision":m.revision,"connected":true,"host":host,
        "project_terminals_ready":host["system"]=="Linux"&&host["tmux"].is_string(),"workspace_api_ready":api,
        "workspace_error":if api{Value::Null}else{json!(if stderr.trim().is_empty(){"STAR/FLUX automation API is missing or incompatible"}else{stderr.trim()})},
        "checked_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64}),
    )
}
/// Register a new project on an endpoint. Reconnection is handled independently by its existing terminal owner.
pub fn create_project(
    machine: &Machine,
    name: &str,
    directory: &str,
    collection: Option<String>,
    theme: &str,
) -> Result<crate::runtime::Config> {
    if let Some(id) = &collection {
        crate::session_catalog::valid(id)?;
    }
    let current = get(&machine.id)?;
    ensure!(
        current.revision == machine.revision,
        "Machine changed; reopen its menu"
    );
    ensure!(
        current.enabled,
        "Enable the machine before adding a project"
    );
    ensure!(
        !directory.is_empty()
            && directory.len() <= 4096
            && !directory.chars().any(char::is_control),
        "Enter a directory on the SSH host"
    );
    let config = crate::runtime::Config {
        workspace: name.into(),
        label: None,
        collection,
        herdr: None,
        tmux: None,
        adopted: Default::default(),
        theme: theme.into(),
        host: Some(current.target),
        ssh_config: current.ssh_config,
        directory: Some(directory.into()),
        agent: None,
        executable: std::env::current_exe()?.display().to_string(),
        project_host: true,
        desktop_helper: current.remote_executable,
    };
    config.validate()?;
    let _lock = crate::project_sync::transaction(&config)?;
    ensure!(
        !crate::runtime::config_path(name)?.exists()
            && !crate::project_state::data_root()?
                .join(format!("{name}.json"))
                .exists(),
        "A project with this identity already exists"
    );
    crate::project_state::ProjectState {
        app_command: String::new(),
        ..Default::default()
    }
    .save(&config)?;
    Ok(config)
}
