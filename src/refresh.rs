//! Frontend refresh re-execs the installed build after the old controller exits.
//! Persistent terminal backends and private app workers are never stopped here.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::os::unix::process::CommandExt;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub const SIGNAL: &str = "STARFLUX_RELOAD_UI";
pub const SWITCH_SIGNAL: &str = "STARFLUX_SWITCH_MACHINE";
#[derive(Serialize, Deserialize)]
struct LocalPresenter {
    pid: u32,
    start: u64,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct MachineSwitch {
    pub machine: String,
    pub revision: String,
    pub project: String,
    pub pane: Option<u8>,
    pub pane_epoch: Option<String>,
    pub graphical: bool,
}
#[derive(Serialize, Deserialize)]
struct SwitchRequest {
    presenter: u32,
    created: u128,
    destination: MachineSwitch,
}

fn switch_path(view: &str, suffix: &str) -> Result<PathBuf> {
    let root = crate::runtime::root()?;
    starcore::terminal_graphics::session::socket_path(&root, view)?;
    Ok(root.join(format!("{view}.{suffix}.json")))
}

fn local_presenter(view: &str) -> Result<LocalPresenter> {
    let presenter: LocalPresenter =
        serde_json::from_slice(&fs::read(switch_path(view, "local-presenter")?)?)?;
    ensure!(
        crate::agent_state::process_start(presenter.pid) == Some(presenter.start),
        "Local presenter has exited"
    );
    Ok(presenter)
}

pub fn can_switch(view: &str) -> bool {
    local_presenter(view).is_ok()
}

pub fn request_switch(view: &str, destination: MachineSwitch) -> Result<()> {
    let machine = crate::machines::get(&destination.machine)?;
    ensure!(
        machine.enabled && machine.revision == destination.revision,
        "Machine changed"
    );
    let presenter = local_presenter(view)?;
    starcore::fs::write_private(
        &switch_path(view, "machine-switch")?,
        &serde_json::to_vec(&SwitchRequest {
            presenter: presenter.pid,
            created: now(),
            destination,
        })?,
    )?;
    Ok(())
}

/// A remote controller's request that this presenter switch to another of the
/// machines saved on this computer, carried in the session's closing error.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RemoteSwitch {
    pub machine: String,
    pub revision: String,
    pub graphical: bool,
}

/// The closing message a remote controller sends to switch its presenter.
pub fn remote_switch_signal(request: &RemoteSwitch) -> Result<String> {
    Ok(format!(
        "{SWITCH_SIGNAL}:{}",
        serde_json::to_string(request)?
    ))
}

fn remote_switch_request(result: &Result<()>) -> Option<RemoteSwitch> {
    let error = result.as_ref().err()?;
    error.chain().find_map(|cause| {
        let text = cause.to_string();
        let text = text.strip_prefix("Graphical session: ").unwrap_or(&text);
        let payload = text.strip_prefix(SWITCH_SIGNAL)?.strip_prefix(':')?;
        (payload.len() <= 1024)
            .then(|| serde_json::from_str(payload).ok())
            .flatten()
    })
}

/// Whether this frontend presents a remote controller's session.
pub fn is_remote_view() -> bool {
    FRONTEND
        .get()
        .is_some_and(|frontend| frontend.remote.is_some())
}

/// The machines saved on this computer that a remote controller may offer as
/// switch destinations: identities and labels only, at most 16.
pub fn local_machines_notice() -> Option<String> {
    if !is_remote_view() {
        return None;
    }
    let machines = crate::machines::list()
        .ok()?
        .into_iter()
        .filter(|machine| machine.enabled)
        .take(16)
        .map(|machine| {
            serde_json::json!({
                "id": machine.id,
                "revision": machine.revision,
                "label": machine.label.chars().take(64).collect::<String>(),
                "target": machine.target.chars().take(128).collect::<String>(),
            })
        })
        .collect::<Vec<_>>();
    if machines.is_empty() {
        return None;
    }
    Some(format!(
        "local-machines:{}",
        serde_json::to_string(&machines).ok()?
    ))
}

/// Open another saved machine in this terminal at a remote controller's
/// request. Only a profile saved on this computer, unchanged since it was
/// offered, is accepted; the controller never names a host or command. The
/// original local view, if any, stays the return target.
fn switch_remote(frontend: &Frontend, request: RemoteSwitch) -> Result<()> {
    let launched = (|| -> Result<std::io::Error> {
        let machine = crate::machines::get(&request.machine)?;
        ensure!(
            machine.enabled && machine.revision == request.revision,
            "Machine changed"
        );
        let project = crate::machines::check_workspace(&machine)?;
        let next_view = format!("v-{}", &uuid::Uuid::new_v4().simple().to_string()[..16]);
        let mut command = std::process::Command::new(&frontend.executable);
        if let Some(origin) = RETURN.get() {
            command.env(RETURN_ENV, serde_json::to_string(origin)?);
        }
        command.args([
            "--machine",
            &machine.id,
            "--workspace",
            &project,
            "--frontend",
            &next_view,
            if request.graphical {
                "--graphical"
            } else {
                "--cells"
            },
        ]);
        Ok(command.exec())
    })();
    let error = match launched {
        Ok(error) => {
            anyhow::Error::from(error).context("Could not open saved machine in this terminal")
        }
        Err(error) => error.context("Machine switch"),
    };
    return_local(Err(error))
}

const RETURN_ENV: &str = "STARFLUX_LOCAL_RETURN";
static RETURN: OnceLock<ReturnLaunch> = OnceLock::new();

#[derive(Serialize, Deserialize)]
struct ReturnLaunch {
    executable: PathBuf,
    args: Vec<OsString>,
}

pub fn load_return() -> Result<()> {
    if let Some(value) = std::env::var_os(RETURN_ENV) {
        // Read and remove the local handoff before any threads or child processes.
        unsafe { std::env::remove_var(RETURN_ENV) };
        let value = value.to_str().context("Invalid local return handoff")?;
        let handoff = serde_json::from_str(value)?;
        RETURN
            .set(handoff)
            .map_err(|_| anyhow::anyhow!("Local return already initialized"))?;
    }
    Ok(())
}

pub fn has_local_return() -> bool {
    RETURN.get().is_some()
}

fn return_local(result: Result<()>) -> Result<()> {
    let Some(origin) = RETURN.get() else {
        return result;
    };
    if let Err(error) = result {
        eprintln!("Remote view ended: {error:#}");
    }
    let error = std::process::Command::new(&origin.executable)
        .args(&origin.args)
        .exec();
    Err(error).context("Could not restore the local STAR/FLUX view")
}
static FRONTEND: OnceLock<Frontend> = OnceLock::new();
struct Frontend {
    executable: PathBuf,
    args: Vec<OsString>,
    workspace: String,
    remote: Option<Remote>,
    started: u128,
}
struct Remote {
    host: String,
    config: Option<PathBuf>,
    executable: String,
}
#[derive(Serialize, Deserialize)]
struct Request {
    controller: u32,
    created: u128,
}
#[derive(Serialize, Deserialize)]
pub struct View {
    pub project: String,
    pub app: Option<String>,
    pub window: Option<u64>,
}
fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
fn marker(workspace: &str) -> Result<PathBuf> {
    let root = crate::runtime::root()?;
    let _ = starcore::terminal_graphics::session::socket_path(&root, workspace)?;
    Ok(root.join(format!("{workspace}.refresh")))
}
pub fn register_local(
    workspace: &str,
    frontend: &str,
    mode: crate::presentation::Mode,
) -> Result<()> {
    let mut args = vec![
        "--workspace".into(),
        workspace.into(),
        "--frontend".into(),
        frontend.into(),
    ];
    args.extend(mode.args().iter().map(OsString::from));
    register(frontend, args, None)?;
    let pid = std::process::id();
    if let Some(start) = crate::agent_state::process_start(pid)
        && let Ok(path) = switch_path(frontend, "local-presenter")
    {
        let _ = starcore::fs::write_private(
            &path,
            &serde_json::to_vec(&LocalPresenter { pid, start })?,
        );
    }
    Ok(())
}
pub fn register_remote(
    host: &str,
    workspace: &str,
    executable: &str,
    config: Option<&Path>,
    frontend: &str,
    mode: crate::presentation::Mode,
    ssh_notifications: bool,
) -> Result<()> {
    let mut args = vec![
        "--ssh".into(),
        host.into(),
        "--workspace".into(),
        workspace.into(),
        "--frontend".into(),
        frontend.into(),
        "--remote-executable".into(),
        executable.into(),
    ];
    args.extend(mode.args().iter().map(OsString::from));
    if ssh_notifications {
        args.push("--ssh-notifications".into());
    }
    if let Some(path) = config {
        args.extend(["--ssh-config".into(), path.as_os_str().to_owned()]);
    }
    register(
        frontend,
        args,
        Some(Remote {
            host: host.into(),
            executable: executable.into(),
            config: config.map(Path::to_path_buf),
        }),
    )
}
pub fn request_remote(
    host: &str,
    workspace: &str,
    executable: &str,
    config: Option<&Path>,
) -> Result<()> {
    let mut command = std::process::Command::new("ssh");
    if let Some(config) = config {
        command.arg("-F").arg(config);
    }
    let output = command
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=5",
            "-o",
            "ServerAliveCountMax=3",
            "--",
            host,
        ])
        .arg(shell_words::join([
            executable,
            "--workspace",
            workspace,
            "--refresh",
        ]))
        .output()?;
    ensure!(
        output.status.success(),
        "UI refresh: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
fn register(workspace: &str, args: Vec<OsString>, remote: Option<Remote>) -> Result<()> {
    // Capture the stable pathname before a later build replaces the running inode.
    let executable = std::env::current_exe()?;
    FRONTEND
        .set(Frontend {
            executable,
            args,
            workspace: workspace.into(),
            remote,
            started: now(),
        })
        .map_err(|_| anyhow::anyhow!("Frontend already registered"))
}
pub fn request(workspace: &str, view: &View) -> Result<()> {
    let executable = std::env::current_exe()?;
    // Linux appends this suffix to an executable replaced by a rebuild.
    let name = executable.to_string_lossy();
    let installed = Path::new(name.strip_suffix(" (deleted)").unwrap_or(&name));
    ensure!(
        installed.is_file(),
        "The updated STAR/FLUX executable is missing; rebuild before refreshing"
    );
    starcore::fs::write_private(
        &crate::project_state::data_root()?.join(format!("{workspace}.refresh-view.json")),
        &serde_json::to_vec(view)?,
    )?;
    starcore::fs::write_private(
        &marker(workspace)?,
        &serde_json::to_vec(&Request {
            controller: std::process::id(),
            created: now(),
        })?,
    )?;
    Ok(())
}
pub fn take_view(workspace: &str) -> Option<View> {
    let path = crate::project_state::data_root()
        .ok()?
        .join(format!("{workspace}.refresh-view.json"));
    let view = serde_json::from_slice(&fs::read(&path).ok()?).ok();
    let _ = fs::remove_file(path);
    view
}
pub fn wait(workspace: &str) -> Result<()> {
    let path = marker(workspace)?;
    let Ok(bytes) = fs::read(&path) else {
        return Ok(());
    };
    let request: Request = serde_json::from_slice(&bytes)?;
    wait_for_controller(request.controller, Duration::from_secs(20))?;
    // Consume only this request; a newer refresh must retain its own marker.
    if fs::read(&path).ok().as_deref() == Some(bytes.as_slice()) {
        fs::remove_file(path)?;
    }
    Ok(())
}
fn controller_running(controller: u32) -> bool {
    let pid = controller as libc::pid_t;
    // A bridge frontend can own the controller directly. kill(pid, 0) alone
    // reports an exited, unreaped child as alive forever; reap it first.
    let waited = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
    if waited == pid {
        return false;
    }
    if waited == 0 {
        return true;
    }
    #[cfg(target_os = "linux")]
    if let Ok(stat) = fs::read_to_string(format!("/proc/{controller}/stat"))
        && stat
            .rsplit_once(") ")
            .is_some_and(|(_, fields)| fields.starts_with(['Z', 'X']))
    {
        return false;
    }
    let alive = unsafe { libc::kill(pid, 0) == 0 };
    alive || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}
fn wait_for_controller(controller: u32, timeout: Duration) -> Result<()> {
    ensure!(
        controller > 1 && controller <= i32::MAX as u32,
        "Invalid refresh controller"
    );
    let start = Instant::now();
    while controller_running(controller) {
        ensure!(
            start.elapsed() < timeout,
            "Previous UI has not finished closing; sessions remain running"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    Ok(())
}
pub fn finish(result: Result<()>) -> Result<()> {
    let Some(frontend) = FRONTEND.get() else {
        return return_local(result);
    };
    if frontend.remote.is_some()
        && let Some(request) = remote_switch_request(&result)
    {
        return switch_remote(frontend, request);
    }
    if frontend.remote.is_none()
        && local_presenter(&frontend.workspace).is_ok_and(|lease| lease.pid == std::process::id())
        && let Ok(path) = switch_path(&frontend.workspace, "local-presenter")
    {
        let _ = fs::remove_file(path);
    }
    if frontend.remote.is_none()
        && result.as_ref().err().is_some_and(|error| {
            error.chain().any(|cause| {
                let text = cause.to_string();
                text == SWITCH_SIGNAL || text == format!("Graphical session: {SWITCH_SIGNAL}")
            })
        })
    {
        let path = switch_path(&frontend.workspace, "machine-switch")?;
        let request: SwitchRequest = serde_json::from_slice(&fs::read(&path)?)?;
        fs::remove_file(path)?;
        ensure!(
            request.presenter == std::process::id() && request.created >= frontend.started,
            "Stale machine switch"
        );
        let destination = request.destination;
        let machine = crate::machines::get(&destination.machine)?;
        ensure!(
            machine.revision == destination.revision && machine.enabled,
            "Machine changed"
        );
        let next_view = format!("v-{}", &uuid::Uuid::new_v4().simple().to_string()[..16]);
        let return_launch = ReturnLaunch {
            executable: frontend.executable.clone(),
            args: frontend.args.clone(),
        };
        let mut command = std::process::Command::new(&frontend.executable);
        command
            .env(RETURN_ENV, serde_json::to_string(&return_launch)?)
            .args([
                "--machine",
                &machine.id,
                "--workspace",
                &destination.project,
                "--frontend",
                &next_view,
                if destination.graphical {
                    "--graphical"
                } else {
                    "--cells"
                },
            ]);
        if let Some(pane) = destination.pane {
            command.args(["--focus-pane", &pane.to_string()]);
        }
        if let Some(epoch) = &destination.pane_epoch {
            command.args(["--focus-pane-epoch", epoch]);
        }
        let error = command.exec();
        return Err(error).context("Could not open saved machine in this terminal");
    }
    let requested = result.as_ref().err().is_some_and(|e| {
        e.chain().any(|cause| {
            let text = cause.to_string();
            text == SIGNAL || text == format!("Graphical session: {SIGNAL}")
        })
    });
    let local_request = frontend.remote.is_none()
        && marker(&frontend.workspace)
            .ok()
            .and_then(|p| fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice::<Request>(&bytes).ok())
            .is_some_and(|r| r.created >= frontend.started);
    if !requested && !local_request {
        return return_local(result);
    }
    if let Some(remote) = &frontend.remote {
        let mut command = std::process::Command::new("ssh");
        if let Some(config) = &remote.config {
            command.arg("-F").arg(config);
        }
        let output = command
            .args([
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "ServerAliveInterval=5",
                "-o",
                "ServerAliveCountMax=3",
                "--",
                &remote.host,
            ])
            .arg(shell_words::join([
                remote.executable.as_str(),
                "--wait-ui-refresh",
                frontend.workspace.as_str(),
            ]))
            .output()?;
        ensure!(
            output.status.success(),
            "UI refresh: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        wait(&frontend.workspace)?;
    }
    ensure!(
        frontend.executable.is_file(),
        "Updated executable is missing: {}",
        frontend.executable.display()
    );
    let mut command = std::process::Command::new(&frontend.executable);
    if let Some(origin) = RETURN.get() {
        command.env(RETURN_ENV, serde_json::to_string(origin)?);
    }
    let error = command.args(&frontend.args).exec();
    Err(error).context("Could not load updated STAR/FLUX build")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_switch_signals_round_trip_through_the_closing_error() {
        let request = RemoteSwitch {
            machine: "m-1".into(),
            revision: "r-2".into(),
            graphical: true,
        };
        let message = remote_switch_signal(&request).unwrap();
        for text in [message.clone(), format!("Graphical session: {message}")] {
            assert_eq!(
                remote_switch_request(&Err(anyhow::anyhow!(text))),
                Some(request.clone())
            );
        }
        // The plain local switch signal and unrelated errors carry no request.
        for text in [
            SWITCH_SIGNAL.to_string(),
            format!("{SWITCH_SIGNAL}:{{}}"),
            "Remote view ended".into(),
        ] {
            assert_eq!(remote_switch_request(&Err(anyhow::anyhow!(text))), None);
        }
        assert_eq!(remote_switch_request(&Ok(())), None);
    }
    use std::process::Command;

    #[test]
    fn refresh_reaps_its_exited_controller_instead_of_waiting_forever() {
        let mut child = Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
        wait_for_controller(child.id(), Duration::from_secs(2)).unwrap();
        assert!(!controller_running(child.id()));
        let _ = child.wait();
    }

    #[test]
    fn refresh_waits_for_a_running_controller_without_terminating_it() {
        let mut child = Command::new("sh")
            .args(["-c", "read line"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        assert!(wait_for_controller(child.id(), Duration::from_millis(30)).is_err());
        assert!(child.try_wait().unwrap().is_none());
        drop(child.stdin.take());
        wait_for_controller(child.id(), Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn refresh_rejects_process_groups_and_invalid_process_ids() {
        for controller in [0, 1, u32::MAX] {
            assert!(wait_for_controller(controller, Duration::ZERO).is_err());
        }
    }
}
