use super::*;
use crate::machines::{Machine, Save};
const AGENT_MACHINES_PER_PAGE: usize = 4;
const AGENT_PANES_PER_PAGE: usize = 8;
pub(super) const SWITCH_MENU: &str = "Connect to saved machine";
#[derive(Debug)]
pub(super) struct AgentScope {
    machine: Machine,
    origin: Option<(usize, usize)>,
}
const AGENT_MENU: &str = "Agents on saved machines";
impl Workspace {
    pub(super) fn machine_menu(&mut self) -> Result<()> {
        self.release();
        self.machine_profile = None;
        let mut items = vec![
            machine_item("Add SSH machine…", "", "machines:add"),
            machine_item("Local projects", "", "machines:local"),
            machine_item("Agents on saved machines", "", "machines:agents:0:0"),
        ];
        if self.local_return_available {
            items.insert(
                0,
                machine_item(
                    "Return to local view",
                    "Keep this session running",
                    "machines:return-local",
                ),
            );
        }
        items.extend(crate::machines::list()?.into_iter().map(|m| {
            machine_item(
                &format!(
                    "{} · {}",
                    m.label,
                    if m.enabled { &m.target } else { "disabled" }
                ),
                "",
                &format!("machines:entry:{}", m.id),
            )
        }));
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [16, 16],
            title: "Machines".into(),
            subtitle: "OpenSSH connections · independent sessions".into(),
            items,
            selected: Some(0),
        });
        Ok(())
    }
    fn show_machine_agents(
        &mut self,
        machine_offset: usize,
        pane_offset: usize,
        scope: Option<AgentScope>,
    ) -> Result<()> {
        let (ids, more) = if let Some(scope) = &scope {
            (vec![scope.machine.id.clone()], false)
        } else {
            let profiles = crate::machines::list()?;
            let ids = profiles
                .iter()
                .skip(machine_offset)
                .take(AGENT_MACHINES_PER_PAGE)
                .map(|m| m.id.clone())
                .collect::<Vec<_>>();
            (
                ids,
                machine_offset.saturating_add(AGENT_MACHINES_PER_PAGE) < profiles.len(),
            )
        };
        let token = Instant::now();
        self.machine_agents_token = Some(token);
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [16, 16],
            title: AGENT_MENU.into(),
            subtitle: "Reading foreground agents over SSH".into(),
            items: vec![machine_item("All machines", "", "machines:list")],
            selected: Some(0),
        });
        if ids.is_empty() {
            self.machine_agents_result(
                token,
                machine_offset,
                pane_offset,
                false,
                scope,
                Ok(json!({"machines":[]})),
            );
            return Ok(());
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = crate::machines::agent_overview(crate::machines::AgentOverview {
                machines: ids,
                offset: pane_offset,
                limit: Some(AGENT_PANES_PER_PAGE),
                project: None,
                session: None,
            })
            .map_err(|error| format!("{error:#}"));
            let _ = tx.send(Event::MachineAgents(
                token,
                machine_offset,
                pane_offset,
                more,
                scope,
                result,
            ));
        });
        Ok(())
    }
    pub(super) fn machine_agents_result(
        &mut self,
        token: Instant,
        machine_offset: usize,
        pane_offset: usize,
        more: bool,
        scope: Option<AgentScope>,
        result: std::result::Result<Value, String>,
    ) {
        if self.machine_agents_token != Some(token)
            || !self
                .context_menu
                .as_ref()
                .is_some_and(|m| m.title == AGENT_MENU)
        {
            return;
        }
        self.machine_agents_token = None;
        let machine = scope.as_ref().map(|scope| &scope.machine);
        let mut items = if let Some(machine) = machine {
            vec![machine_item(
                "Machine details",
                "",
                &format!("machines:entry:{}", machine.id),
            )]
        } else {
            vec![machine_item("All machines", "", "machines:list")]
        };
        if let Some(machine) = machine
            && !crate::machines::get(&machine.id)
                .is_ok_and(|current| current.revision == machine.revision)
        {
            items.clear();
            items.push(machine_item("All machines", "", "machines:list"));
            if let Some(menu) = self.context_menu.as_mut() {
                menu.subtitle = "Machine changed. Reopen its details.".into();
                menu.items = items;
            }
            return;
        }
        let origin = scope.as_ref().and_then(|scope| scope.origin);
        if let Some((machines, panes)) = origin {
            items.push(machine_item(
                "Back to combined agents",
                "",
                &format!("machines:agents:{machines}:{panes}"),
            ));
        }
        let add_page = |items: &mut Vec<_>, label, machines, panes| {
            let mut action = if let Some(machine) = machine {
                format!(
                    "machines:agents-on:{}:{}:{panes}",
                    machine.id, machine.revision
                )
            } else {
                format!("machines:agents:{machines}:{panes}")
            };
            if let Some((machines, panes)) = origin {
                action.push_str(&format!(":{machines}:{panes}"));
            }
            items.push(machine_item(label, "", &action));
        };
        add_page(
            &mut items,
            "Refresh agent status",
            machine_offset,
            pane_offset,
        );
        if machine_offset > 0 {
            add_page(
                &mut items,
                "Previous machines",
                machine_offset.saturating_sub(AGENT_MACHINES_PER_PAGE),
                0,
            );
        }
        if more {
            add_page(
                &mut items,
                "Next machines",
                machine_offset.saturating_add(AGENT_MACHINES_PER_PAGE),
                0,
            );
        }
        if pane_offset > 0 {
            add_page(
                &mut items,
                "Previous pane page",
                machine_offset,
                pane_offset.saturating_sub(AGENT_PANES_PER_PAGE),
            );
        }
        let can_switch = crate::refresh::can_switch(&self.controller_id);
        let combined = machine.is_none();
        match result {
            Ok(snapshot) => {
                if let Some(hosts) = snapshot["machines"].as_array() {
                    if hosts
                        .iter()
                        .any(|host| host["page"]["next_offset"].is_number())
                    {
                        add_page(
                            &mut items,
                            "Next pane page",
                            machine_offset,
                            pane_offset.saturating_add(AGENT_PANES_PER_PAGE),
                        );
                    }
                    if hosts.is_empty() {
                        items.push(machine_item(
                            "No saved machines on this page",
                            "",
                            "menu:panel",
                        ));
                    }
                    for host in hosts {
                        let Some(id) = host["machine"].as_str() else {
                            continue;
                        };
                        let Ok(machine) = crate::machines::get(id) else {
                            if let Some(label) = host["label"].as_str() {
                                items.push(machine_item(
                                    label,
                                    "Profile removed · refresh to update",
                                    "menu:panel",
                                ));
                            }
                            continue;
                        };
                        if host["revision"] != machine.revision {
                            items.push(machine_item(
                                &machine.label,
                                "Profile changed · refresh to update",
                                &format!("machines:entry:{id}"),
                            ));
                            continue;
                        }
                        items.push(machine_item(
                            &machine.label,
                            "Machine details",
                            &format!("machines:entry:{id}"),
                        ));
                        if let Some(error) = host["error"].as_str() {
                            items.push(machine_item(
                                &error.chars().take(220).collect::<String>(),
                                "",
                                "menu:panel",
                            ));
                        } else if let Some(agents) = host["page"]["agents"].as_array() {
                            if agents.is_empty() {
                                items.push(machine_item(
                                    "No foreground agents on this page",
                                    "",
                                    "menu:panel",
                                ));
                            }
                            for agent in agents {
                                if let (Some(project), Some(pane), Some(harness), Some(state)) = (
                                    agent["project"].as_str(),
                                    agent["pane"].as_u64(),
                                    agent["harness"].as_str(),
                                    agent["state"].as_str(),
                                ) {
                                    let label = agent["project_label"].as_str().unwrap_or(project);
                                    let epoch = agent["epoch"].as_str().unwrap_or("");
                                    let action = format!(
                                        "machines:agent-workspace:{id}:{}:{project}:{pane}:{epoch}",
                                        machine.revision
                                    );
                                    items.push(machine_item(
                                        &format!("{label} · pane {pane} · {harness}"),
                                        state,
                                        &action,
                                    ));
                                    if can_switch {
                                        items.push(machine_item(
                                            &format!("Open {label} pane {pane} here"),
                                            "Keep the current session running",
                                            &format!(
                                                "machines:switch-agent:{id}:{}:{project}:{pane}:{epoch}",
                                                machine.revision
                                            ),
                                        ));
                                    }
                                }
                            }
                            if let Some(warnings) = host["page"]["warnings"].as_array()
                                && !warnings.is_empty()
                            {
                                items.push(machine_item(
                                    &format!("{} pane or project read errors", warnings.len()),
                                    "",
                                    "menu:panel",
                                ));
                            }
                            if combined && let Some(next) = host["page"]["next_offset"].as_u64() {
                                items.push(machine_item(
                                    &format!("More agents on {}", machine.label),
                                    "Open this machine’s next pane page",
                                    &format!("machines:agents-on:{id}:{}:{next}:{machine_offset}:{pane_offset}", machine.revision),
                                ));
                            }
                        }
                    }
                }
            }
            Err(error) => items.push(machine_item(&error, "", "menu:panel")),
        }
        if let Some(menu) = &mut self.context_menu {
            let pane_page = pane_offset / AGENT_PANES_PER_PAGE + 1;
            menu.subtitle = if let Some(machine) = machine {
                format!("{} · pane page {pane_page}", machine.label)
            } else {
                format!(
                    "Machine page {} · pane page {pane_page}",
                    machine_offset / AGENT_MACHINES_PER_PAGE + 1
                )
            };
            menu.items = items;
        }
    }
    pub(super) fn machine_details(&mut self, m: Machine) {
        let result = self
            .machine_checks
            .get(&m.id)
            .filter(|v| v["revision"] == m.revision);
        let mut items = vec![
            machine_item("Test connection / retry", "Read-only", "machines:test"),
            machine_item(
                "Copy SSH setup command",
                "Authentication / host keys",
                "machines:setup",
            ),
            machine_item("Projects on this machine", "", "machines:projects"),
        ];
        if m.enabled {
            if crate::refresh::can_switch(&self.controller_id) {
                items.push(machine_item(
                    "Open workspace here",
                    "",
                    "machines:switch-workspace",
                ));
            }
            items.push(machine_item(
                "Agents on this machine",
                "",
                &format!("machines:agents-on:{}:{}:0", m.id, m.revision),
            ));
            items.push(machine_item(
                "Add project on this machine…",
                "",
                "machines:project",
            ));
            items.push(machine_item(
                "Copy remote workspace launch command",
                "",
                "machines:command",
            ));
        }
        items.extend([
            machine_item("Edit connection…", "", "machines:edit"),
            machine_item(
                if m.enabled {
                    "Disable new connections"
                } else {
                    "Enable new connections"
                },
                "Keep running sessions",
                "machines:toggle",
            ),
            machine_item(
                "Remove saved profile",
                "Keep running sessions",
                "machines:remove",
            ),
            machine_item("All machines", "", "machines:list"),
        ]);
        if let Some(r) = result {
            if let Some(e) = r["error"].as_str() {
                items.push(machine_item(e, "", "menu:panel"));
            } else {
                items.push(machine_item(
                    if r["project_terminals_ready"] == true {
                        "Python / tmux ready"
                    } else {
                        "Project terminals need Linux, Python 3 and tmux"
                    },
                    "",
                    "menu:panel",
                ));
                items.push(machine_item(
                    if r["workspace_api_ready"] == true {
                        "Remote STAR/FLUX workspace API ready"
                    } else {
                        "Remote workspace helper unavailable / incompatible"
                    },
                    "",
                    "menu:panel",
                ));
                if let Some(e) = r["workspace_error"].as_str() {
                    items.push(machine_item(e, "", "menu:panel"));
                }
            }
        }
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [16, 16],
            title: "SSH machine".into(),
            subtitle: format!("{} · {}", m.label, m.target),
            items,
            selected: Some(0),
        });
        self.machine_profile = Some(m);
    }
    pub(super) fn machine_action(&mut self, action: &str) -> Result<bool> {
        let Some(action) = action.strip_prefix("machines:") else {
            return Ok(false);
        };
        match action {
            "cancel-switch" => {
                self.machine_switch_token = None;
                self.context_menu = None;
                self.note("Machine switch canceled".into());
            }
            "return-local" => {
                ensure!(
                    self.local_return_available,
                    "This window has no local view to return to"
                );
                ensure!(
                    self.transfer.is_none() && self.receiving.is_none(),
                    "Finish the active file transfer first"
                );
                self.quit = true;
            }
            "list" => self.machine_menu()?,
            "add" => self.open_prompt(Prompt::Machine(None)),
            "local" => self.machine_projects(None),
            _ if action.starts_with("agent-workspace:") || action.starts_with("switch-agent:") => {
                let switching = action.starts_with("switch-agent:");
                let payload = action
                    .strip_prefix("agent-workspace:")
                    .or_else(|| action.strip_prefix("switch-agent:"))
                    .unwrap();
                let (id, target) = payload.split_once(':').context("Invalid agent workspace")?;
                let (revision, project) =
                    target.split_once(':').context("Invalid agent workspace")?;
                let (project, pane) = project.split_once(':').context("Missing agent pane")?;
                let (pane, epoch) = pane.split_once(':').unwrap_or((pane, ""));
                let pane = pane.parse::<u8>()?;
                ensure!((1..=32).contains(&pane), "Invalid agent pane");
                let machine = crate::machines::get(id)?;
                ensure!(
                    machine.revision == revision,
                    "Machine changed; refresh the agent overview"
                );
                ensure!(machine.enabled, "Machine is disabled");
                ensure!(
                    !project.is_empty()
                        && project.len() <= 32
                        && project
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
                    "Remote controller returned an invalid workspace"
                );
                if switching {
                    self.start_machine_switch(
                        machine,
                        Some((
                            project.into(),
                            pane,
                            (!epoch.is_empty()).then(|| epoch.to_owned()),
                        )),
                    )?;
                    return Ok(true);
                }
                let mode = if self.cells { "--cells" } else { "--graphical" };
                let pane = pane.to_string();
                let mut args = vec![
                    "starflux",
                    "--machine",
                    &machine.id,
                    "--workspace",
                    project,
                    "--focus-pane",
                    &pane,
                ];
                if !epoch.is_empty() {
                    args.extend(["--focus-pane-epoch", epoch]);
                }
                args.push(mode);
                self.effects.push(ServerMessage::Clipboard {
                    text: shell_words::join(args),
                });
                self.note("Remote pane launch command copied".into());
            }
            _ if action.starts_with("agents-on:") => {
                let (id, target) = action[10..]
                    .split_once(':')
                    .context("Invalid machine agent page")?;
                let (revision, offset) = target
                    .split_once(':')
                    .context("Invalid machine agent page")?;
                let machine = crate::machines::get(id)?;
                ensure!(
                    machine.revision == revision,
                    "Machine changed; reopen its menu"
                );
                ensure!(machine.enabled, "Machine is disabled");
                let (offset, origin) = if let Some((offset, origin)) = offset.split_once(':') {
                    let (machines, panes) = origin
                        .split_once(':')
                        .context("Invalid combined agent page")?;
                    (offset, Some((machines.parse()?, panes.parse()?)))
                } else {
                    (offset, None)
                };
                self.show_machine_agents(0, offset.parse()?, Some(AgentScope { machine, origin }))?;
            }
            _ if action.starts_with("agents:") => {
                let (machines, panes) =
                    action[7..].split_once(':').context("Invalid agent page")?;
                self.show_machine_agents(machines.parse()?, panes.parse()?, None)?;
            }
            _ if action.starts_with("entry:") => {
                self.machine_details(crate::machines::get(&action[6..])?)
            }
            _ if action.starts_with("open:") => {
                let config = crate::saved(&action[5..])?;
                self.context_menu = None;
                if config.collection == self.config.collection {
                    self.reopen_project(config)?;
                } else {
                    self.open_session(config)?;
                }
            }
            _ => {
                let machine = self
                    .machine_profile
                    .clone()
                    .context("Choose a machine first")?;
                ensure!(
                    crate::machines::get(&machine.id)?.revision == machine.revision,
                    "Machine changed; reopen its menu"
                );
                match action {
                    "edit" => self.open_prompt(Prompt::Machine(Some(machine))),
                    "project" => {
                        ensure!(machine.enabled, "Enable the machine first");
                        self.open_prompt(Prompt::MachineProject(machine));
                    }
                    "toggle" => {
                        let mut input: Save = machine.into();
                        input.enabled = !input.enabled;
                        self.machine_details(crate::machines::save(input)?);
                    }
                    "remove" => {
                        crate::machines::remove(&machine.id, &machine.revision)?;
                        self.machine_checks.remove(&machine.id);
                        self.machine_pending.remove(&machine.id);
                        self.machine_menu()?;
                        self.note("Machine profile removed / running sessions retained".into());
                    }
                    "projects" => self.machine_projects(Some(&machine)),
                    "switch-workspace" => self.start_machine_switch(machine, None)?,
                    "setup" => {
                        self.effects.push(ServerMessage::Clipboard {
                            text: machine.setup_command(),
                        });
                        self.note(
                            "SSH setup command copied / run in an interactive terminal".into(),
                        );
                    }
                    "command" => {
                        let command = shell_words::join(["starflux", "--machine", &machine.id]);
                        self.effects
                            .push(ServerMessage::Clipboard { text: command });
                        self.note("Saved-machine launch command copied / run on this controller's machine".into());
                    }
                    "test" => {
                        let token = Instant::now();
                        ensure!(
                            !self.machine_pending.contains_key(&machine.id),
                            "Connection check is already running"
                        );
                        self.machine_pending.insert(machine.id.clone(), token);
                        self.note(format!("Checking SSH connection to {}…", machine.label));
                        let tx = self.tx.clone();
                        std::thread::spawn(move || {
                            let result =
                                crate::machines::test(&machine).map_err(|e| format!("{e:#}"));
                            let _ = tx.send(Event::MachineChecked(machine, token, result));
                        });
                    }
                    _ => anyhow::bail!("Unknown machine action"),
                }
            }
        }
        Ok(true)
    }
    fn start_machine_switch(
        &mut self,
        machine: Machine,
        target: Option<(String, u8, Option<String>)>,
    ) -> Result<()> {
        ensure!(machine.enabled, "Machine is disabled");
        ensure!(
            crate::refresh::can_switch(&self.controller_id),
            "This presenter cannot switch controllers"
        );
        ensure!(
            self.transfer.is_none() && self.receiving.is_none(),
            "Finish the active file transfer first"
        );
        let token = Instant::now();
        self.machine_switch_token = Some(token);
        self.release();
        let location = target
            .as_ref()
            .map_or_else(|| "workspace".into(), |(_, pane, _)| format!("pane {pane}"));
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [16, 16],
            title: SWITCH_MENU.into(),
            subtitle: format!("Checking {} · {location}", machine.label),
            items: vec![machine_item("Cancel", "", "machines:cancel-switch")],
            selected: Some(0),
        });
        self.note(format!("Checking the remote {location}…"));
        let graphical = !self.cells;
        let origin = self.config.workspace.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let (project, pane, pane_epoch, result) = if let Some((project, pane, epoch)) = target {
                let result =
                    crate::machines::check_pane(&machine, &project, pane, epoch.as_deref());
                (project, Some(pane), epoch, result)
            } else {
                match crate::machines::check_workspace(&machine) {
                    Ok(project) => (project, None, None, Ok(())),
                    Err(error) => (String::new(), None, None, Err(error)),
                }
            };
            let destination = crate::refresh::MachineSwitch {
                machine: machine.id,
                revision: machine.revision,
                project,
                pane,
                pane_epoch,
                graphical,
            };
            let _ = tx.send(Event::MachineSwitch(
                token,
                origin,
                destination,
                result.map_err(|error| format!("{error:#}")),
            ));
        });
        Ok(())
    }

    fn machine_projects(&mut self, machine: Option<&Machine>) {
        let mut items = crate::session_catalog::projects()
            .into_iter()
            .filter(|c| machine.map_or(c.host.is_none(), |m| m.matches(c)))
            .map(|c| {
                machine_item(
                    c.label.as_deref().unwrap_or(&c.workspace),
                    &crate::session_catalog::group(&c),
                    &format!("machines:open:{}", c.workspace),
                )
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            items.push(machine_item(
                "No projects registered on this controller",
                "",
                "menu:panel",
            ));
        }
        items.push(machine_item("All machines", "", "machines:list"));
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [16, 16],
            title: "Machine projects".into(),
            subtitle: machine.map_or("Local".into(), |m| m.label.clone()),
            items,
            selected: Some(0),
        });
    }
}
fn machine_item(label: &str, hint: &str, action: &str) -> crate::shelf::MenuItem {
    crate::shelf::MenuItem {
        label: label.into(),
        hint: hint.into(),
        action: action.into(),
    }
}
