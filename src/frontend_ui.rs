use super::*;
use crate::{
    automation::fault,
    frontend_commands::{Command, Direction, Space as CommandSpace},
};
impl Workspace {
    pub(super) fn frontend_command_busy(&self) -> bool {
        self.popup.is_some()
            || self.modal.is_some()
            || self.context_menu.is_some()
            || self.git_open
            || self.panel_focus.is_some()
            || self
                .terminals
                .get(&(
                    self.config.workspace.clone(),
                    self.config.canonical_pane(self.state.selected()),
                ))
                .is_some_and(|t| t.review_status().is_some())
    }
    fn frontend_command_state(&self) -> Value {
        json!({"frontend":self.controller_id,"project":self.config.workspace,"session":crate::session_catalog::group(&self.config),"pane":self.state.selected()+1,"space":if self.state.space==Space::Apps{"apps"}else{"terminals"},"zoomed":self.state.pane_zoom,"presentation":if self.cells{"cells"}else{"graphical"},"acknowledgement":"controller_applied"})
    }
    pub(super) fn frontend_command(&mut self, command: &Command) -> Result<Value> {
        if let Command::ClosePluginPopup { id } = command {
            self.close_popup(id)?;
            let mut result = self.frontend_command_state();
            result["popup_closed"] = json!(id);
            return Ok(result);
        }

        if self.frontend_command_busy() {
            return Err(fault(
                "frontend_busy",
                "A dialog, menu, panel navigation or history review is active",
            ));
        }
        let project = match command {
            Command::OpenPluginPopup { project, .. }
            | Command::PresentPluginPane { project, .. }
            | Command::SelectPane { project, .. }
            | Command::SelectSpace { project, .. }
            | Command::FocusPane { project, .. }
            | Command::SetZoom { project, .. } => Some(project),
            _ => None,
        };
        if project.is_some_and(|p| p != &self.config.workspace) {
            return Err(fault(
                "frontend_project_changed",
                "Select the intended project before issuing a pane or space command",
            ));
        }
        let mut plugin_result = None;
        match command {
            Command::OpenPluginPopup {
                plugin,
                entrypoint,
                digest,
                options,
                ..
            } => {
                let popup = self.open_popup(plugin, entrypoint, digest, options.clone())?;
                let mut result = self.frontend_command_state();
                result["popup"] = popup;
                return Ok(result);
            }
            Command::ClosePluginPopup { .. } => unreachable!("handled before modal refusal"),
            Command::PresentPluginPane {
                project,
                id,
                epoch,
                placement,
            } => {
                let config = self.config.clone();
                let _guard = crate::project_sync::transaction(&config)?;
                self.sync_project()?;
                let record = crate::plugin_panes::get(id)?;
                if record.project.id != *project
                    || record.epoch != *epoch
                    || self.state.terminal_epochs.get(&record.pane) != Some(epoch)
                    || crate::plugin_panes::owner(&config, record.pane)?.is_none_or(|r| r.id != *id)
                {
                    return Err(fault(
                        "stale_target",
                        "Plugin pane ownership or epoch changed",
                    ));
                }
                let placement = placement.unwrap_or(record.entrypoint.placement);
                self.state.present_plugin_pane(record.pane, placement)?;
                self.release();
                self.cell_ui.terminal_focus = true;
                self.ensure_sessions();
                self.layout();
                self.save();
                plugin_result = Some((id, placement));
            }
            Command::SelectProject { project } => {
                let config = crate::saved(project)
                    .map_err(|_| fault("project_unavailable", "Project is not registered"))?;
                if config.workspace != *project {
                    return Err(fault(
                        "project_unavailable",
                        "Project routing alias no longer names this project",
                    ));
                }
                if crate::session_catalog::group(&config)
                    != crate::session_catalog::group(&self.config)
                {
                    return Err(fault(
                        "wrong_session",
                        "Use open_session before selecting a project in another session",
                    ));
                }
                self.reopen_project(config)?;
            }
            Command::SelectPane { pane, epoch, .. } => {
                let id = usize::from(
                    pane.checked_sub(1)
                        .filter(|id| *id < 32)
                        .ok_or_else(|| fault("invalid_params", "Pane must be 1..32"))?,
                );
                let config = self.config.clone();
                let _guard = crate::project_sync::transaction(&config)?;
                self.sync_project()?;
                if !self.state.terminal_ids().contains(&id)
                    || self.config.canonical_pane(id) != id
                    || self.state.terminal_epochs.get(&id) != epoch.as_ref()
                {
                    return Err(fault(
                        "stale_target",
                        "Pane membership or terminal epoch changed",
                    ));
                }
                self.action(&format!("terminal:{id}"))?;
            }
            Command::SelectSpace { space, .. } => {
                if *space == CommandSpace::Apps && !self.app_view_available {
                    return Err(fault(
                        "unsupported_capability",
                        "App Space is unavailable in this presentation",
                    ));
                }
                self.set_space(if *space == CommandSpace::Apps {
                    Space::Apps
                } else {
                    Space::Terminals
                });
            }
            Command::OpenSession { session } => {
                let selected = crate::session_catalog::list()?
                    .into_iter()
                    .find(|s| s.id == *session)
                    .and_then(|s| s.project)
                    .ok_or_else(|| fault("session_unavailable", "Session no longer exists"))?;
                let config = crate::saved(&selected)?;
                if crate::session_catalog::group(&config) != *session {
                    return Err(fault("session_unavailable", "Session membership changed"));
                }
                self.open_session(config)?;
            }
            Command::FocusPane { direction, .. } => {
                if self.state.space != Space::Terminals {
                    return Err(fault("wrong_space", "Pane focus requires terminal space"));
                }
                let direction = match direction {
                    Direction::Left => "left",
                    Direction::Right => "right",
                    Direction::Up => "up",
                    Direction::Down => "down",
                };
                self.action(&format!("terminal:focus-{direction}"))?;
            }
            Command::SetZoom { enabled, .. } => {
                if self.state.space != Space::Terminals {
                    return Err(fault("wrong_space", "Pane zoom requires terminal space"));
                }
                if self.state.pane_zoom != *enabled {
                    self.action("terminal:zoom")?;
                }
            }
        }
        let mut state = self.frontend_command_state();
        if let Some((id, placement)) = plugin_result {
            state["plugin_pane"] = json!(id);
            state["placement"] = json!(placement);
        }
        Ok(state)
    }
}
