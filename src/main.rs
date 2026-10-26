mod activity;
mod app_desktop;
mod appearance;
#[cfg(test)]
mod blueprint;
mod compositor;
mod fixture;
mod git;
mod herdr;
mod kitty_selection;
mod project;
mod project_state;
mod runtime;
mod selection;
mod shelf;
mod terminal;
mod ui;
use anyhow::{Context, Result, ensure};
use clap::Parser;
use runtime::{Config, config_path, root};
use starkit::terminal_graphics::{
    client::{self, Launch},
    protocol::Input,
    session,
};
use std::{
    fs,
    os::unix::{fs::OpenOptionsExt, io::AsRawFd, net::UnixStream, process::CommandExt},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Parser)]
#[command(
    version,
    about = "S T A R / M U X — native terminals and persistent local/SSH project apps"
)]
struct Args {
    /// Named workspace to create or reopen; unnamed launches are independent
    #[arg(long, default_value = "demo")]
    workspace: String,
    /// List saved workspace names for reattachment
    #[arg(long)]
    sessions: bool,
    /// Import the default Herdr session as an isolated interactive collection
    #[arg(long)]
    import_herdr: bool,
    /// Shared Kitty/chrome theme (see --themes); saved per workspace
    #[arg(long)]
    theme: Option<String>,
    /// List available shared palettes
    #[arg(long)]
    themes: bool,
    /// SSH destination for this project (legacy workspaces retain their sessions)
    #[arg(long)]
    host: Option<String>,
    /// OpenSSH configuration file for the remote host
    #[arg(long)]
    ssh_config: Option<PathBuf>,
    /// Project working directory on its host
    #[arg(long)]
    directory: Option<String>,
    /// Project agent command (default: codex)
    #[arg(long)]
    agent: Option<String>,
    /// Report live session state as JSON
    #[arg(long)]
    inspect: bool,
    /// Saved command for the application under development
    #[arg(long)]
    app: Option<String>,
    /// Launch argv after -- inside a named app desktop on this host
    #[arg(long, conflicts_with = "app_list")]
    app_launch: Option<String>,
    /// List windows in a named app desktop on this host
    #[arg(long)]
    app_list: Option<String>,
    /// Working directory for --app-launch
    #[arg(long, requires = "app_launch")]
    app_directory: Option<PathBuf>,
    /// Application executable and arguments for --app-launch
    #[arg(last = true, requires = "app_launch")]
    app_argv: Vec<String>,
    /// S T A R / M U X executable on the app host (default: starmux)
    #[arg(long)]
    desktop_helper: Option<String>,
    /// Send a workspace action (e.g. view:apps, terminal:new, run-app)
    #[arg(long)]
    action: Option<String>,
    /// Close the workspace while keeping its sessions running
    #[arg(long)]
    close: bool,
    /// Stop this project’s terminal sessions and app desktops
    #[arg(long)]
    stop: bool,
    /// Copy a regular file to the selected pane and paste its destination
    #[arg(long)]
    deliver: Option<PathBuf>,
    #[arg(long,default_value_t=2,value_parser=clap::value_parser!(u8).range(1..=32))]
    pane: u8,
    /// Read the selected terminal screen as text
    #[arg(long)]
    capture: bool,
    #[arg(long)]
    png: Option<PathBuf>,
}
fn terminal_event(event: &starkit::crossterm::event::Event) -> Option<Input> {
    match event {
        starkit::crossterm::event::Event::Osc72(text) => Some(Input::Osc72 { text: text.clone() }),
        _ => None,
    }
}
fn saved(name: &str) -> Result<Config> {
    let path = config_path(name)?;
    let path = if path.exists() {
        path
    } else {
        project_state::data_root()?.join(format!("{name}.json"))
    };
    let c: Config = serde_json::from_slice(&fs::read(path)?)?;
    c.validate()?;
    Ok(c)
}
fn serve(name: &str) -> Result<()> {
    let config = saved(name)?;
    session::serve_exclusive(&root()?, name, ui::Workspace::new(config)?)
}
fn socket_active(path: &std::path::Path) -> Result<bool> {
    match UnixStream::connect(path) {
        Ok(stream) => {
            drop(stream);
            Ok(true)
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            Ok(false)
        }
        Err(e) => Err(e.into()),
    }
}
fn attachment_lease(root: &std::path::Path, name: &str) -> Result<Option<fs::File>> {
    session::socket_path(root, name)?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(root.join(format!("{name}.attach.lock")))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            return Ok(None);
        }
        return Err(error.into());
    }
    Ok(Some(file))
}
fn auto_workspace() -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("session-{}-{stamp:x}", std::process::id())
}
fn relay(name: &str) -> Result<()> {
    let root = root()?;
    let Some(_lease) = attachment_lease(&root, name)? else {
        starkit::terminal_graphics::protocol::write_message(
            &starkit::terminal_graphics::protocol::ServerMessage::Error {
                message: format!(
                    "Workspace '{name}' is already open in another tab or window. Run starmux without --workspace for an independent session, or close the other frontend before reattaching."
                ),
            },
            &mut std::io::stdout(),
        )?;
        return Ok(());
    };
    let socket = session::socket_path(&root, name)?;
    if socket.exists() && !socket_active(&socket)? {
        fs::remove_file(&socket)?;
    }
    if !socket.exists() {
        let _ = fs::remove_file(root.join(format!("{name}.close")));
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join(format!("{name}.log")))?;
        let mut child = Command::new(std::env::current_exe()?)
            .args(["--serve", name])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .process_group(0)
            .spawn()?;
        let started = Instant::now();
        while !socket.exists() {
            if let Some(status) = child.try_wait()? {
                anyhow::bail!(
                    "Workspace exited during startup ({status}); see {}",
                    root.join(format!("{name}.log")).display()
                );
            }
            if started.elapsed() >= Duration::from_secs(20) {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!(
                    "Workspace startup timed out; see {}",
                    root.join(format!("{name}.log")).display()
                );
            }
            ensure!(
                started.elapsed() < Duration::from_secs(20),
                "Workspace startup timed out; see {}",
                root.join(format!("{name}.log")).display()
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    session::relay(&socket)
}
fn main() -> Result<()> {
    let raw: Vec<String> = std::env::args().collect();
    if raw.get(1).is_some_and(|s| s == "--herdr-view") {
        return herdr::viewer(
            raw.get(2).context("Missing Herdr session")?,
            raw.get(3).context("Missing Herdr pane")?,
        );
    }
    if let Some(mode) = raw.get(1).filter(|s| {
        matches!(
            s.as_str(),
            "--desktop-worker" | "--desktop-relay" | "--desktop-stop"
        )
    }) {
        return app_desktop::internal(mode, raw.get(2).context("Missing desktop session")?);
    }
    if raw.get(1).is_some_and(|s| s == "--fixture") {
        return fixture::run(raw.get(2).context("Missing workspace")?);
    }
    if raw
        .get(1)
        .is_some_and(|s| s == "--serve" || s == "--graphical-relay")
    {
        let name = raw.get(2).context("Missing workspace")?;
        return if raw[1] == "--serve" {
            serve(name)
        } else {
            relay(name)
        };
    }
    let mut a = Args::parse();
    if let Some(name) = a.app_launch.as_ref().or(a.app_list.as_ref()) {
        let launch = if a.app_launch.is_some() {
            ensure!(!a.app_argv.is_empty(), "Provide an application after --");
            Some(crate::project::Launch {
                name: a.app_argv[0].clone(),
                command: shell_words::join(&a.app_argv),
                directory: a.app_directory.unwrap_or(std::env::current_dir()?),
            })
        } else {
            None
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&app_desktop::launch_local(name, launch)?)?
        );
        return Ok(());
    }
    if a.import_herdr {
        herdr::import("default")?;
        return Ok(());
    }
    if a.themes {
        println!("system\tFollow the desktop palette (Kanagawa fallback)");
        for builtin in starkit::theme::BUILTINS {
            let theme = appearance::Appearance::load(builtin.id)?;
            println!("{}\t{}", builtin.id, theme.name);
        }
        return Ok(());
    }
    if a.sessions {
        let mut names = Vec::new();
        for entry in [root()?, project_state::data_root()?]
            .into_iter()
            .flat_map(|r| fs::read_dir(r).into_iter().flatten())
        {
            let path = entry?.path();
            if path.extension().is_some_and(|s| s == "json")
                && let Ok(config) = serde_json::from_slice::<Config>(&fs::read(path)?)
            {
                names.push(config.workspace);
            }
        }
        names.sort();
        names.dedup();
        for name in names {
            println!("{name}");
        }
        return Ok(());
    }
    let explicit_workspace = raw
        .iter()
        .any(|s| s == "--workspace" || s.starts_with("--workspace="));
    let management =
        a.inspect || a.close || a.stop || a.capture || a.deliver.is_some() || a.action.is_some();
    if !explicit_workspace && !management {
        a.workspace = auto_workspace();
    }
    let mut config = Config {
        workspace: a.workspace.clone(),
        label: None,
        collection: None,
        herdr: None,
        theme: a.theme.clone().unwrap_or_else(appearance::default_theme),
        host: a.host,
        ssh_config: a.ssh_config,
        directory: a.directory,
        agent: a.agent,
        executable: std::env::current_exe()?.to_string_lossy().into_owned(),
        project_host: true,
        desktop_helper: a
            .desktop_helper
            .clone()
            .unwrap_or_else(runtime::default_desktop_helper),
    };
    config.validate()?;
    let path = config_path(&a.workspace)?;
    if management {
        config = saved(&a.workspace)?;
    }
    if let Some(ssh_config) = &config.ssh_config {
        config.ssh_config =
            Some(fs::canonicalize(ssh_config).context("SSH configuration not found")?);
    }
    if let Some(action) = &a.action {
        ensure!(
            socket_active(&session::socket_path(&root()?, &a.workspace)?)?,
            "Workspace is not running; open it before sending actions"
        );
        let directory = root()?.join(format!("{}.actions", a.workspace));
        fs::create_dir_all(&directory)?;
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        starkit::fs::write_private(
            &directory.join(format!("{id}.json")),
            &serde_json::to_vec(action)?,
        )?;
        return Ok(());
    }
    if a.inspect {
        let snapshot = root()?.join(format!("{}.state.json", a.workspace));
        let mut data: serde_json::Value = if snapshot.exists() {
            serde_json::from_slice(&fs::read(snapshot)?)?
        } else {
            serde_json::json!({})
        };
        if let Some(id) = data["active_project"]
            .as_str()
            .filter(|id| *id != config.workspace)
        {
            config = saved(id)?;
        }
        let health: Vec<_> = (0..2)
            .map(|i| {
                config
                    .call(i, &config.request(i, "inspect"))
                    .unwrap_or_else(|e| serde_json::json!({"error":e.to_string()}))
            })
            .collect();
        data["panes"] = serde_json::json!(health);
        data["controller_running"] = serde_json::json!(socket_active(&session::socket_path(
            &root()?,
            &a.workspace
        )?)?);
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }
    if a.capture {
        let req = config.request((a.pane - 1) as usize, "capture");
        let out = config
            .command((a.pane - 1) as usize, &req, false)
            .output()?;
        ensure!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        print!("{}", String::from_utf8_lossy(&out.stdout));
        return Ok(());
    }
    if a.close || a.stop {
        let socket = session::socket_path(&root()?, &a.workspace)?;
        let marker = root()?.join(format!("{}.close", a.workspace));
        if socket_active(&socket)? {
            fs::write(&marker, b"close")?;
            let started = Instant::now();
            while socket_active(&socket)? {
                ensure!(
                    started.elapsed() < Duration::from_secs(10),
                    "Workspace did not close within ten seconds"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        if socket.exists() {
            fs::remove_file(&socket)?;
        }
        let _ = fs::remove_file(marker);
        if a.stop {
            let state = project_state::ProjectState::load(&config)?;
            for i in state
                .agent_ids()
                .into_iter()
                .chain(state.terminals.iter().copied())
            {
                config.call(i, &config.request(i, "stop"))?;
            }
            for target in &state.app_targets {
                let mut app_config = config.clone();
                app_config.host = target.host.clone();
                app_config.desktop_helper = target.helper.clone();
                app_desktop::stop(&app_config)?;
            }
            fs::remove_file(&path)?;
            let data = project_state::data_root()?;
            let _ = fs::remove_file(data.join(format!("{}.json", config.workspace)));
            let _ = fs::remove_file(data.join(format!("{}.layout.json", config.workspace)));
        }
        return Ok(());
    }
    if let Some(file) = a.deliver {
        let pane = (a.pane - 1) as usize;
        let result = config.upload(pane, &file, |_, _| true)?;
        let remote = result["path"].as_str().context("Missing destination")?;
        config.paste(pane, &shell_words::quote(remote))?;
        println!(
            "Copied and pasted into pane {} without submitting: {remote}",
            a.pane
        );
        return Ok(());
    }
    if !path.exists()
        && project_state::data_root()?
            .join(format!("{}.json", a.workspace))
            .exists()
    {
        let existing = saved(&a.workspace)?;
        starkit::fs::write_private(&path, &serde_json::to_vec_pretty(&existing)?)?;
    }
    if path.exists() {
        let existing = saved(&a.workspace)?;
        ensure!(
            !(config.host.is_some() && config.host != existing.host
                || config.agent.is_some() && config.agent != existing.agent
                || config.directory.is_some() && config.directory != existing.directory
                || config.ssh_config.is_some() && config.ssh_config != existing.ssh_config),
            "Workspace already has a different launch configuration; choose another --workspace or explicitly --stop the existing one first"
        );
        config = existing;
    } else {
        if config.directory.is_none() {
            config.directory = Some(if config.host.is_some() {
                "~".into()
            } else {
                std::env::current_dir()?.display().to_string()
            });
        }
        starkit::fs::write_private(&path, &serde_json::to_vec_pretty(&config)?)?;
    }
    if let Some(helper) = &a.desktop_helper {
        config.desktop_helper = helper.clone();
    }
    if let Some(command) = &a.app {
        let mut state = project_state::ProjectState::load(&config)?;
        state.app_command = command.clone();
        state.save(&config)?;
    }
    if a.desktop_helper.is_some() {
        starkit::fs::write_private(&path, &serde_json::to_vec_pretty(&config)?)?;
    }
    if let Some(theme) = &a.theme {
        config.theme = theme.clone();
        config.validate()?;
        starkit::fs::write_private(&path, &serde_json::to_vec_pretty(&config)?)?;
        if explicit_workspace && socket_active(&session::socket_path(&root()?, &a.workspace)?)? {
            println!(
                "Saved theme '{}' for workspace '{}'. The controller applies it without restarting agents.",
                config.theme, a.workspace
            );
            return Ok(());
        }
    }
    if let Some(path) = a.png {
        let mut ui = ui::Workspace::new(config)?;
        let scene = session::Controller::scene(&mut ui, Default::default());
        starkit::terminal_graphics::renderer::render_reference(&scene)?.save(path)?;
        return Ok(());
    }
    if unsafe { libc::isatty(0) } == 0 || std::env::var_os("KITTY_WINDOW_ID").is_none() {
        let mut c = Command::new(
            std::env::var("STARMUX_KITTY").unwrap_or_else(|_| "/usr/bin/kitty".into()),
        );
        c.args([
            "--title",
            "S T A R / M U X",
            "-o",
            "initial_window_width=1400",
            "-o",
            "initial_window_height=900",
            "-o",
            "background=#101620",
        ]);
        c.arg(std::env::current_exe()?).args(&raw[1..]);
        if !explicit_workspace {
            c.args(["--workspace", &a.workspace]);
        }
        c.spawn()?;
        return Ok(());
    }
    client::run_with_events(
        Launch {
            executable: std::env::current_exe()?.to_string_lossy().into_owned(),
            host: None,
            ssh_config: None,
            session: config.workspace,
            directory: None,
            attach_only: false,
            play: None,
        },
        terminal_event,
    )
}

#[cfg(test)]
mod workspace_tests {
    use super::*;
    #[test]
    fn one_named_workspace_has_one_controlling_attachment() {
        let root = tempfile::tempdir().unwrap();
        let first = attachment_lease(root.path(), "first").unwrap().unwrap();
        assert!(attachment_lease(root.path(), "first").unwrap().is_none());
        assert!(attachment_lease(root.path(), "second").unwrap().is_some());
        drop(first);
        assert!(attachment_lease(root.path(), "first").unwrap().is_some());
        assert!(attachment_lease(root.path(), "../outside").is_err());
    }
    #[test]
    fn generated_workspace_names_fit_the_runtime_contract() {
        let name = auto_workspace();
        let config = Config {
            workspace: name,
            label: None,
            collection: None,
            herdr: None,
            theme: appearance::default_theme(),
            host: None,
            ssh_config: None,
            directory: None,
            agent: None,
            executable: String::new(),
            project_host: true,
            desktop_helper: runtime::default_desktop_helper(),
        };
        config.validate().unwrap();
    }
}
