use super::*;
use crate::machines::{Machine, Save};
impl Workspace {
    pub(super) fn machine_menu(&mut self) -> Result<()> {
        self.release();
        self.machine_profile = None;
        let mut items = vec![
            machine_item("Add SSH machine…", "", "machines:add"),
            machine_item("Local projects", "", "machines:local"),
        ];
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
            "list" => self.machine_menu()?,
            "add" => self.open_prompt(Prompt::Machine(None)),
            "local" => self.machine_projects(None),
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
