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
pub const SIGNAL: &str = "STARMUX_RELOAD_UI";
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
    let _ = starkit::terminal_graphics::session::socket_path(&root, workspace)?;
    Ok(root.join(format!("{workspace}.refresh")))
}
pub fn register_local(workspace: &str, frontend: &str) -> Result<()> {
    register(
        frontend,
        vec![
            "--workspace".into(),
            workspace.into(),
            "--frontend".into(),
            frontend.into(),
        ],
        None,
    )
}
pub fn register_remote(
    host: &str,
    workspace: &str,
    executable: &str,
    config: Option<&Path>,
    frontend: &str,
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
        "The updated STAR/MUX executable is missing; rebuild before refreshing"
    );
    starkit::fs::write_private(
        &crate::project_state::data_root()?.join(format!("{workspace}.refresh-view.json")),
        &serde_json::to_vec(view)?,
    )?;
    starkit::fs::write_private(
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
        return result;
    };
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
        return result;
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
    let error = std::process::Command::new(&frontend.executable)
        .args(&frontend.args)
        .exec();
    Err(error).context("Could not load updated STAR/MUX build")
}

#[cfg(test)]
mod tests {
    use super::*;
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
