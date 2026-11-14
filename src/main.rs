mod activity;
mod adoption;
mod agent_state;
mod app_audio;
mod app_desktop;
mod app_view;
mod appearance;
mod applications;
mod automation;
mod backend_snapshot;
#[cfg(test)]
mod blueprint;
mod bridge_handoff;
mod categories;
mod category_agent;
mod category_agent_files;
mod category_agent_runtime;
mod cold_start;
mod compositor;
mod features;
mod fixture;
mod font_frontend;
mod frontend;
mod frontend_commands;
mod git;
mod graphical;
mod graphics_decode;
mod graphics_owner;
mod graphics_paint;
mod graphics_reply;
mod graphics_requests;
mod graphics_resources;
mod graphics_store;
mod graphics_stream;
mod graphics_transfer;
mod herdr;
#[cfg(test)]
mod inline_graphics_tests;
mod keyboard;
mod kitty_selection;
mod machines;
mod navigation;
mod notifications;
#[cfg(test)]
mod osc8_tests;
mod panes;
mod plugin_links;
mod plugin_packages;
mod plugin_panes;
mod plugin_popup;
mod plugin_runner;
mod plugin_store;
mod plugins;
mod presentation;
mod project;
mod project_state;
mod project_sync;
mod read_query;
mod refresh;
mod remote_video;
mod responsive;
mod runtime;
mod selection;
mod session_catalog;
mod session_import;
mod settings;
mod shelf;
mod snapshot_service;
#[cfg(target_os = "linux")]
mod source_broker;
mod terminal;
mod terminal_history;
mod terminal_links;
mod tmux_source;
mod ui;
mod worktrees;
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
    os::unix::net::UnixStream,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};
#[derive(Parser)]
#[command(
    version,
    about = "S T A R / F L U X — persistent local/SSH terminals in cells or graphics"
)]
struct Args {
    /// Render text cells directly in the launching terminal
    #[arg(long, conflicts_with = "graphical")]
    cells: bool,
    /// Require the graphical presentation
    #[arg(long, conflicts_with = "cells")]
    graphical: bool,
    /// Named workspace to create or reopen; unnamed launches are independent
    #[arg(long, default_value = "demo")]
    workspace: String,
    /// Identity of this presentation; retained across UI refreshes
    #[arg(long, hide = true)]
    frontend: Option<String>,
    /// List saved workspace names for reattachment
    #[arg(long)]
    sessions: bool,
    #[arg(long, hide = true)]
    default_workspace: bool,
    /// Import the default Herdr session as an isolated interactive collection
    #[arg(long)]
    import_herdr: bool,
    /// Transfer live default Herdr terminals into STAR/FLUX without restarting them
    #[arg(long)]
    migrate_herdr: bool,
    /// Prepare and back up a live Herdr migration without applying it
    #[arg(long)]
    plan_herdr_migration: bool,
    /// Application-wide Kitty/chrome theme (see --themes); defaults to system
    #[arg(long)]
    theme: Option<String>,
    /// List available shared palettes
    #[arg(long)]
    themes: bool,
    /// SSH destination for this project (legacy workspaces retain their sessions)
    #[arg(long)]
    host: Option<String>,
    /// Render locally while attaching to a saved STAR/FLUX workspace over SSH
    #[arg(long, conflicts_with = "host")]
    ssh: Option<String>,
    /// Connect to a saved machine by identity or label
    #[arg(long, conflicts_with_all = ["host", "ssh", "ssh_config"])]
    machine: Option<String>,
    /// STAR/FLUX executable on the workspace host
    #[arg(long, default_value = "starflux")]
    remote_executable: String,
    /// OpenSSH configuration file for the remote host
    #[arg(long)]
    ssh_config: Option<PathBuf>,
    /// Project working directory on its host
    #[arg(long)]
    directory: Option<String>,
    /// Optional command to start in an initial terminal (compatibility shortcut)
    #[arg(long)]
    agent: Option<String>,
    /// Report live session state as JSON
    #[arg(long)]
    inspect: bool,
    /// Report structured lifecycle/identity JSON for --pane (1-based)
    #[arg(long)]
    agent_report: Option<String>,
    /// Inspect layered agent health and saved conversation references as JSON
    #[arg(long)]
    agent_status: bool,
    /// Execute one versioned JSON automation request
    #[arg(long, conflicts_with = "api_stdio")]
    api: Option<String>,
    /// Serve versioned JSON Lines automation requests on stdin/stdout
    #[arg(long)]
    api_stdio: bool,
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
    /// S T A R / F L U X executable on the app host (default: starflux)
    #[arg(long)]
    desktop_helper: Option<String>,
    /// Send a workspace action (e.g. terminal:new, terminal:split, git:details)
    #[arg(long)]
    action: Option<String>,
    /// Reload the UI from the latest build while keeping sessions and apps running
    #[arg(long)]
    refresh: bool,
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
    presentation::event(event)
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
fn auto_workspace() -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("session-{}-{stamp:x}", std::process::id())
}
fn relay(route: &str) -> Result<()> {
    let view = frontend::View::parse(route)?;
    let path = view.ensure()?;
    if std::env::var_os("SSH_CONNECTION").is_none() && std::env::var_os("KITTY_WINDOW_ID").is_some()
    {
        font_frontend::relay(&path)
    } else {
        session::relay(&path)
    }
}
fn main() -> Result<()> {
    refresh::finish(run())
}
fn run() -> Result<()> {
    runtime::initialize_environment()?;
    let raw: Vec<String> = std::env::args().collect();
    if raw.get(1).is_some_and(|s| s == "--category-agent-handoffs") {
        return category_agent_runtime::handoffs(
            raw.get(2).context("Missing project")?,
            raw.get(3).context("Missing portfolio")?,
        );
    }
    if raw.get(1).is_some_and(|s| s == "--category-agent-worker") {
        return category_agent_runtime::worker(
            raw.get(2).context("Missing portfolio project")?,
            raw.get(3).context("Missing portfolio")?,
            raw.get(4).context("Missing review identity")?,
        );
    }
    if raw
        .get(1)
        .is_some_and(|s| s == "--category-repository-query")
    {
        let value: serde_json::Value =
            serde_json::from_str(raw.get(2).context("Missing repository query")?)?;
        let result = category_agent_files::local(
            value["directory"].as_str().context("Missing repository")?,
            value["operation"].as_str().context("Missing operation")?,
            value["path"].as_str().unwrap_or(""),
            value["pattern"].as_str().unwrap_or(""),
        )?;
        println!("{}", result);
        return Ok(());
    }
    if raw.get(1).is_some_and(|s| s == "--plugin-popup-worker") {
        return plugin_popup::worker(
            raw.get(2).context("Missing popup store")?,
            raw.get(3).context("Missing popup id")?,
        );
    }
    if raw.get(1).is_some_and(|s| s == "--plugin-pane") {
        return plugin_panes::worker(
            raw.get(2).context("Missing plugin pane store")?,
            raw.get(3).context("Missing plugin pane identity")?,
        );
    }
    if raw.get(1).is_some_and(|s| s == "--plugin-service") {
        return plugin_runner::service(raw.get(2).context("Missing plugin store")?);
    }
    if raw.get(1).is_some_and(|s| s == "--plugin-job") {
        return plugin_runner::worker(
            raw.get(2).context("Missing plugin store")?,
            raw.get(3).context("Missing plugin job")?,
        );
    }
    #[cfg(target_os = "linux")]
    if raw.get(1).is_some_and(|s| s == "--native-source-service") {
        anyhow::ensure!(raw.len() == 3, "Expected native source data namespace");
        return source_broker::service::run(std::path::Path::new(&raw[2]));
    }
    #[cfg(target_os = "linux")]
    if raw
        .get(1)
        .is_some_and(|s| s == "--native-source-stdio" || s == "--native-source-attach")
    {
        anyhow::ensure!(
            raw.len() == 3 || raw.len() == 4,
            "Expected native source data namespace and optional owner"
        );
        let owner = if raw[1] == "--native-source-attach" {
            Some(serde_json::from_str::<source_broker::service::Identity>(
                raw.get(3).context("Missing native source owner")?,
            )?)
        } else {
            anyhow::ensure!(raw.len() == 3, "Unexpected native source arguments");
            None
        };
        return source_broker::service::stdio(
            std::path::Path::new(&raw[2]),
            std::path::Path::new("/proc/self/exe"),
            owner.as_ref(),
        );
    }
    #[cfg(target_os = "linux")]
    if raw.get(1).is_some_and(|s| s == "--native-source-discover") {
        anyhow::ensure!(raw.len() == 3, "Expected native source data namespace");
        let handle = source_broker::service::ensure(
            std::path::Path::new(&raw[2]),
            std::path::Path::new("/proc/self/exe"),
        )?;
        println!("{}", serde_json::to_string(&handle.owner)?);
        return Ok(());
    }
    if raw.get(1).is_some_and(|s| s == "--read-query-worker") {
        return read_query::run();
    }
    if raw.get(1).is_some_and(|s| s == "--snapshot-worker") {
        return snapshot_service::run(
            raw.get(2).context("Missing snapshot runtime")?,
            raw.get(3).context("Missing snapshot namespace")?,
        );
    }
    if raw.get(1).is_some_and(|s| s == "--validate-pty-plan") {
        return adoption::validate_plan(raw.get(2).context("Missing migration plan")?);
    }
    if raw.get(1).is_some_and(|s| s == "--native-pty-import") {
        return adoption::receive(
            raw.get(2).context("Missing migration plan")?,
            raw.get(3).context("Missing handoff socket")?,
            raw.get(4).context("Missing handoff token")?,
        );
    }
    if raw
        .get(1)
        .is_some_and(|s| s == "--graphical-terminal-client")
    {
        let _font_output = font_frontend::LocalOutput::start()?;
        let proxy = font_frontend::Proxy::new(std::path::Path::new(
            raw.get(2).context("Missing terminal socket")?,
        ))?;
        return client::run_terminal_socket_with_events(&proxy.path, terminal_event);
    }
    if raw.get(1).is_some_and(|s| s == "--herdr-view") {
        return herdr::viewer(
            raw.get(2).context("Missing Herdr session")?,
            raw.get(3).context("Missing Herdr pane")?,
        );
    }
    if raw.get(1).is_some_and(|s| s == "--wait-ui-refresh") {
        return refresh::wait(raw.get(2).context("Missing refresh workspace")?);
    }
    if raw.get(1).is_some_and(|s| s == "--system-applications") {
        println!("{}", serde_json::to_string(&applications::installed()?)?);
        return Ok(());
    }
    if raw
        .get(1)
        .is_some_and(|s| s == "--launch-system-application")
    {
        return applications::launch(raw.get(2).context("Missing application ID")?);
    }
    if let Some(mode) = raw.get(1).filter(|s| {
        matches!(
            s.as_str(),
            "--desktop-audio-worker" | "--desktop-audio-relay"
        )
    }) {
        app_audio::environment();
        return app_audio::internal(
            mode,
            raw.get(2).context("Missing desktop session")?,
            raw.get(3).context("Missing desktop owner")?.parse()?,
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
    if raw.get(1).is_some_and(|s| s == "--serve-view") {
        let workspace = raw.get(2).context("Missing workspace")?;
        let id = raw.get(3).context("Missing frontend identity")?;
        return session::serve_exclusive(
            &root()?,
            id,
            ui::Workspace::new_view(saved(workspace)?, id.clone())?,
        );
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
    if raw.get(1).is_some_and(|s| s == "--automation-worker") {
        return automation::worker();
    }
    let mut a = Args::parse();
    let mut machine_workspace = false;
    if let Some(id) = &a.machine {
        let machine = machines::get(id)?;
        ensure!(
            machine.enabled,
            "Machine is disabled; enable it in the machine manager before connecting"
        );
        ensure!(
            !raw.iter()
                .any(|v| v == "--remote-executable" || v.starts_with("--remote-executable=")),
            "--machine uses its saved remote executable"
        );
        a.ssh = Some(machine.target);
        a.ssh_config = machine.ssh_config;
        a.remote_executable = machine.remote_executable;
        if !raw
            .iter()
            .any(|v| v == "--workspace" || v.starts_with("--workspace="))
            && let Some(workspace) = machine.workspace
        {
            a.workspace = workspace;
            machine_workspace = true;
        }
    }
    if a.api.is_some() || a.api_stdio {
        return if let Some(host) = &a.ssh {
            automation::remote(
                host,
                &a.remote_executable,
                a.ssh_config.as_deref(),
                a.api.as_deref(),
            )
        } else {
            automation::run(a.api.as_deref())
        };
    }
    let mode = if a.cells {
        presentation::Mode::Cells
    } else if a.graphical {
        presentation::Mode::Graphical
    } else {
        presentation::Mode::Auto
    };
    if a.migrate_herdr || a.plan_herdr_migration {
        return adoption::migrate(a.plan_herdr_migration);
    }
    if a.default_workspace {
        println!(
            "{}",
            session_catalog::default_project()?.unwrap_or_else(|| "demo".into())
        );
        return Ok(());
    }
    if let Some(host) = &a.ssh {
        if !a.refresh
            && !machine_workspace
            && !raw
                .iter()
                .any(|s| s == "--workspace" || s.starts_with("--workspace="))
        {
            let mut command = Command::new("ssh");
            if let Some(path) = &a.ssh_config {
                command.arg("-F").arg(path);
            }
            let output = command
                .args([
                    "-T",
                    "-o",
                    "ConnectTimeout=10",
                    "-o",
                    "ServerAliveInterval=15",
                    "-o",
                    "ServerAliveCountMax=3",
                    "--",
                    host,
                ])
                .arg(shell_words::join([
                    a.remote_executable.as_str(),
                    "--default-workspace",
                ]))
                .output()?;
            ensure!(
                output.status.success(),
                "Read default session on SSH host: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            let workspace = String::from_utf8(output.stdout)?.trim().to_owned();
            ensure!(
                !workspace.is_empty()
                    && workspace.len() <= 32
                    && workspace
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
                "SSH host returned an invalid default workspace"
            );
            a.workspace = workspace;
        }
        if a.refresh {
            return refresh::request_remote(
                host,
                &a.workspace,
                &a.remote_executable,
                a.ssh_config.as_deref(),
            );
        }
        let view = frontend::View::new(&a.workspace, a.frontend.as_deref())?;
        refresh::register_remote(
            host,
            &a.workspace,
            &a.remote_executable,
            a.ssh_config.as_deref(),
            &view.id,
            mode,
        )?;
        return presentation::run(
            Launch {
                executable: a.remote_executable.clone(),
                host: Some(host.clone()),
                ssh_config: a.ssh_config.clone(),
                session: view.route(),
                directory: None,
                attach_only: true,
                play: None,
            },
            mode,
        );
    }
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
    if let Some(theme) = &a.theme {
        settings::Settings::set_theme(theme)?;
        println!(
            "Saved application theme '{}'. All controllers apply it without restarting agents.",
            theme
        );
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
    let mut explicit_workspace = raw
        .iter()
        .any(|s| s == "--workspace" || s.starts_with("--workspace="));
    let management = a.inspect
        || a.agent_status
        || a.agent_report.is_some()
        || a.close
        || a.stop
        || a.refresh
        || a.capture
        || a.deliver.is_some()
        || a.action.is_some();
    if !explicit_workspace && !management {
        let default =
            if a.directory.is_none() && a.host.is_none() && a.agent.is_none() && a.app.is_none() {
                session_catalog::default_project()?
            } else {
                None
            };
        if let Some(project) = default {
            a.workspace = project;
            explicit_workspace = true;
        } else {
            a.workspace = auto_workspace();
        }
    }
    if !management {
        adoption::wait(&a.workspace)?;
        if a.frontend.is_none() {
            a.frontend = adoption::resumed_frontend(&a.workspace);
        }
    }
    let mut config = Config {
        workspace: a.workspace.clone(),
        label: None,
        collection: None,
        herdr: None,
        tmux: None,
        adopted: Default::default(),
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
    if let Some(report) = a.agent_report {
        ensure!(report.len() <= 65536, "Agent report exceeds 64 KiB");
        let record = agent_state::publish(
            &config,
            usize::from(a.pane - 1),
            serde_json::from_str(&report)?,
        )?;
        println!("{}", serde_json::to_string(&record)?);
        return Ok(());
    }
    if a.agent_status {
        let state = project_state::ProjectState::load(&config)?;
        let panes=state.terminal_ids().into_iter().map(|pane| {
            let mut health=config.call(pane,&config.request(pane,"inspect")).unwrap_or_else(|e|serde_json::json!({"state":"disconnected","error":format!("{e:#}")}));
            agent_state::enrich(&config,pane,&mut health);
            serde_json::json!({"pane":pane+1,"epoch":state.terminal_epochs.get(&pane),"status":health})
        }).collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"workspace":config.workspace,"panes":panes,"records":agent_state::records(&config)?})
            )?
        );
        return Ok(());
    }
    if let Some(action) = a
        .action
        .as_deref()
        .or(if a.refresh { Some("ui:refresh") } else { None })
    {
        let views = frontend::active(&a.workspace)?;
        ensure!(
            !views.is_empty(),
            "Workspace is not running; open it before sending actions"
        );
        for view in views
            .iter()
            .filter(|v| a.frontend.as_ref().is_none_or(|id| id == &v.id))
        {
            frontend::action(view, action)?;
            // Model-changing commands execute once; other views observe shared state.
            if !matches!(action, "ui:refresh" | "view:apps" | "view:terminals") {
                break;
            }
        }
        return Ok(());
    }
    if a.inspect {
        let views = frontend::active(&a.workspace)?;
        let snapshot = root()?.join(format!(
            "{}.state.json",
            a.frontend
                .as_deref()
                .or_else(|| views.first().map(|v| v.id.as_str()))
                .unwrap_or(&a.workspace)
        ));
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
        data["controller_running"] = serde_json::json!(!views.is_empty());
        data["frontends"] = serde_json::json!(
            views
                .iter()
                .map(|v| frontend::snapshot(v)
                    .unwrap_or_else(|_| serde_json::json!({"frontend":v.id})))
                .collect::<Vec<_>>()
        );
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
        for view in frontend::active(&a.workspace)? {
            if a.frontend.as_ref().is_some_and(|id| id != &view.id) {
                continue;
            }
            let socket = session::socket_path(&root()?, &view.id)?;
            fs::write(root()?.join(format!("{}.close", view.id)), b"close")?;
            let start = Instant::now();
            while socket_active(&socket)? {
                ensure!(
                    start.elapsed() < Duration::from_secs(10),
                    "Frontend did not close within ten seconds"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }
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
            let _ = fs::remove_file(data.join(format!("{}.model.json", config.workspace)));
            project_state::catalog_changed()?;
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
    if let Some(path) = a.png {
        let mut ui = ui::Workspace::new(config)?;
        let scene = session::Controller::scene(&mut ui, Default::default());
        starkit::terminal_graphics::renderer::render_reference(&scene)?.save(path)?;
        return Ok(());
    }
    if mode != presentation::Mode::Cells {
        let mut handoff = bridge_handoff::Handoff::acquire()?;
        if handoff.is_some() {
            font_frontend::select_terminal_client()?;
        }
        if let Some(bridge) = starkit::terminal_graphics::terminal_bridge::probe()? {
            if let Some(handoff) = &mut handoff {
                handoff.attached();
            }
            let view = frontend::View::new(&config.workspace, a.frontend.as_deref())?;
            refresh::register_local(&config.workspace, &view.id, mode)?;
            let socket = view.ensure()?;
            if mode == presentation::Mode::Graphical {
                frontend::action(&view, "presentation:require-graphical")?;
            }
            return bridge.relay_with_presentation(&socket, None, mode.cells());
        }
    }
    if unsafe { libc::isatty(0) } == 0 {
        ensure!(
            mode != presentation::Mode::Cells,
            "Cells mode requires an interactive terminal"
        );
        let mut c =
            Command::new(std::env::var("STARFLUX_KITTY").unwrap_or_else(|_| "kitty".into()));
        c.args([
            "--title",
            "S T A R / F L U X",
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
    let view = frontend::View::new(&config.workspace, a.frontend.as_deref())?;
    refresh::register_local(&config.workspace, &view.id, mode)?;
    presentation::run(
        Launch {
            executable: std::env::current_exe()?.to_string_lossy().into_owned(),
            host: None,
            ssh_config: None,
            session: view.route(),
            directory: None,
            attach_only: false,
            play: None,
        },
        mode,
    )
}

#[cfg(test)]
mod workspace_tests {
    use super::*;
    #[test]
    fn frontend_ids_are_independent_of_the_shared_workspace() {
        let first = frontend::View::new("shared", None).unwrap();
        let second = frontend::View::new("shared", None).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(first.workspace, second.workspace);
        assert_eq!(
            frontend::View::new("shared", Some(&first.id)).unwrap().id,
            first.id
        );
        assert!(frontend::View::new("shared", Some("../outside")).is_err());
    }
    #[test]
    fn generated_workspace_names_fit_the_runtime_contract() {
        let name = auto_workspace();
        let config = Config {
            workspace: name,
            label: None,
            collection: None,
            herdr: None,
            tmux: None,
            adopted: Default::default(),
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
