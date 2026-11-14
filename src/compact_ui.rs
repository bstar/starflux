use super::*;
impl Workspace {
    pub(super) fn compact_action(&mut self, action: &str) -> Result<bool> {
        let Some(command) = action.strip_prefix("workspace:") else {
            return Ok(false);
        };
        let item = |label: &str, action: &str| crate::shelf::MenuItem {
            label: label.into(),
            hint: String::new(),
            action: action.into(),
        };
        self.release();
        let (title, items) = match command {
            "projects" => {
                let mut items = self
                    .category_rows()
                    .iter()
                    .filter(|r| !r.id.starts_with("category-end:"))
                    .map(|r| {
                        item(
                            &r.name,
                            &if let Some(id) = r.id.strip_prefix("category:") {
                                format!("categories:details:{id}")
                            } else {
                                format!("project:{}", r.id)
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                items.push(item("Portfolios…", "categories:manage"));
                items.push(item("+ Add / reopen project…", "project:new"));
                ("Projects", items)
            }
            "apps" => {
                ensure!(
                    self.app_view_available,
                    "App Space is unavailable in this presentation"
                );
                let mut items = self
                    .app_hosts
                    .iter()
                    .filter(|(_, h)| h.config.workspace == self.config.workspace)
                    .flat_map(|(key, h)| {
                        h.windows
                            .iter()
                            .map(move |w| item(&w.name, &format!("app:{key}:{}", w.id)))
                    })
                    .collect::<Vec<_>>();
                items.push(item("Launch app…", "launch-app"));
                ("App windows", items)
            }
            "tabs" => {
                let mut items = self
                    .state
                    .terminal_ids()
                    .into_iter()
                    .map(|id| {
                        item(
                            &format!(
                                "{}{}",
                                if id == self.state.selected() {
                                    "● "
                                } else {
                                    ""
                                },
                                self.terminal_label(id)
                            ),
                            &format!("terminal:{id}"),
                        )
                    })
                    .collect::<Vec<_>>();
                items.push(item("+ Terminal tab", "terminal:new"));
                ("Terminal tabs", items)
            }
            "project-menu" => {
                self.open_context_menu(Some(&format!("project:{}", self.config.workspace)), [8, 8]);
                return Ok(true);
            }
            "terminal-menu" => {
                self.open_context_menu(
                    Some(&format!("terminal:{}", self.state.selected())),
                    [8, 8],
                );
                return Ok(true);
            }
            "menu" => {
                let mut items = vec![
                    item("Projects…", "workspace:projects"),
                    item("Sessions…", "sessions:open"),
                    item("Attention inbox…", "attention:inbox"),
                    item("Machines…", "machines:list"),
                    item("Plugins…", "plugins:list"),
                    item("Current project…", "workspace:project-menu"),
                    item("Terminal tabs…", "workspace:tabs"),
                    item("Current terminal / recovery…", "workspace:terminal-menu"),
                    item("New terminal tab", "terminal:new"),
                    item("Split right", "terminal:split-right"),
                    item("Split down", "terminal:split-down"),
                    item("Zoom / restore pane", "terminal:zoom"),
                    item("Close split pane / keep terminal", "terminal:close-pane"),
                    item("Git status…", "git:details"),
                    item("Git worktrees…", "worktrees:list"),
                    item("Deliver a file…", "file:deliver"),
                    item("Portfolios…", "categories:manage"),
                    item("Reverse project order", "projects:sort:direction"),
                    item("Next theme", "theme:next"),
                ];
                if !self.cells {
                    items.extend([
                        item("Increase terminal font", "terminal:font:increase"),
                        item("Decrease terminal font", "terminal:font:decrease"),
                        item("Reset terminal font", "terminal:font:reset"),
                    ]);
                }
                if self.app_view_available {
                    items.extend([
                        item("App Space", "view:apps"),
                        item("App windows…", "workspace:apps"),
                        item("Terminal Space", "view:terminals"),
                        item("Launch app…", "launch-app"),
                        item("Audio / mute", "audio:toggle"),
                    ]);
                    if self.state.space == Space::Apps {
                        items.extend([
                            item("Tile apps", "apps:tile"),
                            item("Maximize app", "apps:maximize"),
                            item("Close app", "apps:close"),
                            item("Reconnect apps", "apps:reconnect"),
                        ]);
                    }
                }
                items.extend([
                    item("Keyboard shortcuts…", "keyboard:help"),
                    item("Refresh this view", "ui:refresh"),
                    item("Detach / keep sessions", "keyboard:detach"),
                ]);
                ("Workspace menu", items)
            }
            _ => anyhow::bail!("Unknown workspace menu"),
        };
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [8, 8],
            title: title.into(),
            subtitle: "↑ ↓ / PgUp PgDn · Enter · Esc".into(),
            items,
            selected: Some(0),
        });
        Ok(true)
    }
}
