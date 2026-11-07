use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Panel {
    Projects,
    Toolbar,
    Tabs,
    Terminal(usize),
    Apps,
}
impl Workspace {
    fn focus_panels(&self) -> Vec<Panel> {
        let mut panels = vec![Panel::Projects, Panel::Toolbar];
        if self.state.space == Space::Apps {
            panels.push(Panel::Apps);
        } else {
            panels.push(Panel::Tabs);
            panels.extend(
                self.visible_panes()
                    .into_iter()
                    .map(|(id, _)| Panel::Terminal(id)),
            );
        }
        panels
    }
    pub(super) fn focus_step(&mut self, backwards: bool) {
        self.release();
        let panels = self.focus_panels();
        let current = self.panel_focus.filter(|p| panels.contains(p)).or_else(|| {
            Some(if self.state.space == Space::Apps {
                Panel::Apps
            } else {
                Panel::Terminal(self.state.selected())
            })
        });
        let index = current
            .and_then(|p| panels.iter().position(|x| *x == p))
            .unwrap_or(0);
        let index = if backwards {
            (index + panels.len() - 1) % panels.len()
        } else {
            (index + 1) % panels.len()
        };
        self.panel_focus = Some(panels[index]);
        self.cell_ui.terminal_focus = false;
        if let Panel::Terminal(id) = panels[index] {
            self.state.select_terminal(id);
            self.layout();
            self.save();
        }
        self.focus_note();
    }
    fn focus_note(&mut self) {
        if let Some(hint) = self.panel_hint() {
            self.note(hint);
        }
    }
    pub(super) fn panel_hint(&self) -> Option<String> {
        let name = match self.panel_focus {
            Some(Panel::Projects) => "Projects",
            Some(Panel::Toolbar) => "Workspace controls",
            Some(Panel::Tabs) => "Terminal tabs",
            Some(Panel::Terminal(_)) => "Terminal pane",
            Some(Panel::Apps) => "App Space",
            None => return None,
        };
        Some(format!(
            "{name} focus · Tab/Shift+Tab: panels · arrows: navigate · Enter: activate"
        ))
    }
    pub(super) fn panel_input(&mut self, input: &Input) -> bool {
        if self.modal.is_some() || self.context_menu.is_some() || self.git_open {
            return false;
        }
        if matches!(input, Input::Pointer{action,..} if action=="down")
            || matches!(input,Input::Embedded{event,..} if matches!(&event.kind,EmbeddedKind::Pointer{action,..} if action=="down"))
        {
            self.panel_focus = None;
            return false;
        }
        let (code, modifiers, release) = match input {
            Input::Key { code, modifiers } => (code.as_str(), *modifiers, false),
            Input::Embedded { event, .. } => match &event.kind {
                EmbeddedKind::Key {
                    code,
                    modifiers,
                    phase,
                } => (code.as_str(), *modifiers, *phase == KeyPhase::Release),
                _ => return self.panel_focus.is_some(),
            },
            Input::Paste { .. } => return self.panel_focus.is_some(),
            _ => return false,
        };
        let physical = matches!(input, Input::Embedded { .. });
        if physical {
            if release && self.panel_keys.remove(code) {
                return true;
            }
            if !release {
                self.panel_keys.remove(code);
            }
        }
        let tab = matches!(code, "tab" | "backtab") && modifiers & !1 == 0;
        let workspace_key =
            matches!(input, Input::Key { .. }) && (!self.cells || !self.cell_ui.terminal_focus);
        let cycle = tab
            && (self.panel_focus.is_some() || workspace_key || self.keyboard.tab_switches_panels());
        if cycle {
            if physical && !release {
                self.panel_keys.insert(code.into());
            }
            if !release {
                self.focus_step(code == "backtab" || modifiers & 1 != 0);
            }
            return true;
        }
        if matches!(code, "f6" | "f:6") && modifiers & !1 == 0 {
            if self.keyboard.direct_disabled(code, modifiers)
                || (self.state.space == Space::Apps && self.panel_focus.is_none() && physical)
            {
                return false;
            }
            if physical && !release {
                self.panel_keys.insert(code.into());
            }
            if !release {
                if modifiers & 1 != 0 {
                    self.focus_step(true);
                } else if self.panel_focus.is_some() || (self.cells && !self.cell_ui.terminal_focus)
                {
                    self.panel_focus = None;
                    self.cell_ui.terminal_focus = true;
                    self.note("Terminal/app input · Ctrl+Esc: workspace controls".into());
                } else {
                    self.release();
                    self.panel_focus = Some(Panel::Toolbar);
                    self.cell_ui.terminal_focus = false;
                    self.focus_note();
                }
            }
            return true;
        }
        if code == "escape" && modifiers & 2 != 0 {
            if physical && !release {
                self.panel_keys.insert(code.into());
            }
            if !release {
                self.release();
                self.panel_focus = Some(if self.state.space == Space::Apps {
                    Panel::Apps
                } else {
                    Panel::Terminal(self.state.selected())
                });
                self.cell_ui.terminal_focus = false;
                self.focus_note();
            }
            return true;
        }
        let Some(panel) = self.panel_focus else {
            return false;
        };
        if release {
            return true;
        }
        if self.cells && !matches!(code, "enter" | "escape" | "up" | "down" | "left" | "right") {
            return false;
        }
        if physical {
            self.panel_keys.insert(code.into());
        }
        let result = (|| -> Result<()> {
            match code {
                "enter" | "escape" => match panel {
                    Panel::Projects if code == "enter" => {
                        if let Some(row) = self.category_rows().get(self.category_cursor) {
                            if row.id.starts_with("category-end:") {
                                return Ok(());
                            }
                            let action = if let Some(id) = row.id.strip_prefix("category:") {
                                format!("categories:toggle:{id}")
                            } else {
                                format!("project:{}", row.id)
                            };
                            self.action(&action)?;
                        }
                    }
                    Panel::Toolbar if code == "enter" => self.action("workspace:menu")?,
                    _ => {
                        self.panel_focus = None;
                        self.cell_ui.terminal_focus = true;
                        self.note("Terminal/app input · Ctrl+Esc: workspace controls".into());
                    }
                },
                "up" | "down" | "left" | "right" => match panel {
                    Panel::Projects => {
                        let rows = self.category_rows();
                        if !rows.is_empty() {
                            let index = self.category_cursor.min(rows.len() - 1);
                            if matches!(code, "left" | "right") {
                                let id = rows[index]
                                    .id
                                    .strip_prefix("category:")
                                    .unwrap_or_else(|| {
                                        self.category_catalog.category(&rows[index].id)
                                    })
                                    .to_owned();
                                if code == "left" {
                                    self.navigation.collapsed_categories.insert(id.clone());
                                } else {
                                    self.navigation.collapsed_categories.remove(&id);
                                }
                                self.navigation.dirty = true;
                                self.category_cursor = self
                                    .category_rows()
                                    .iter()
                                    .position(|r| r.id == format!("category:{id}"))
                                    .unwrap_or(0);
                            } else {
                                self.category_cursor = if code == "up" {
                                    index.saturating_sub(1)
                                } else {
                                    (index + 1).min(rows.len() - 1)
                                };
                                while rows[self.category_cursor].id.starts_with("category-end:") {
                                    let next = if code == "up" {
                                        self.category_cursor.saturating_sub(1)
                                    } else {
                                        (self.category_cursor + 1).min(rows.len() - 1)
                                    };
                                    if next == self.category_cursor {
                                        self.category_cursor = index;
                                        break;
                                    }
                                    self.category_cursor = next;
                                }
                            }
                            if self.category_cursor < self.project_offset {
                                self.project_offset = self.category_cursor;
                            } else if self.category_cursor
                                >= self.project_offset + self.project_page_size.max(1)
                            {
                                self.project_offset =
                                    self.category_cursor + 1 - self.project_page_size.max(1);
                            }
                        }
                    }
                    Panel::Tabs => {
                        let ids = self.state.terminal_ids();
                        let n = ids.len();
                        if n > 0 {
                            let i = ids
                                .iter()
                                .position(|id| *id == self.state.selected())
                                .unwrap_or(0);
                            let next = if matches!(code, "up" | "left") {
                                (i + n - 1) % n
                            } else {
                                (i + 1) % n
                            };
                            self.action(&format!("terminal:{}", ids[next]))?;
                        }
                    }
                    Panel::Terminal(_) => {
                        self.pane_action(&format!("terminal:focus-{code}"))?;
                        self.panel_focus = Some(Panel::Terminal(self.state.selected()));
                    }
                    _ => (),
                },
                _ => (),
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.note(format!("Panel navigation: {e:#}"));
        }
        true
    }
    pub(super) fn focus_rect(&self) -> Option<PixelRect> {
        if self.cells_overlay() {
            return None;
        }
        let panel = self.panel_focus?;
        if !self.focus_panels().contains(&panel) {
            return None;
        }
        let layout = crate::shelf::Layout::new(
            self.viewport.width as u16,
            self.viewport.height as u16,
            self.state.space,
        );
        let content = self.presentation_canvas();
        match panel {
            Panel::Projects => Some(if layout.sidebar.width > 0 {
                layout.sidebar
            } else {
                PixelRect::new(0, 0, content.width, 38)
            }),
            Panel::Toolbar => Some(PixelRect::new(
                layout.frame.x,
                0,
                layout.frame.width,
                layout.tabs.y.max(30),
            )),
            Panel::Tabs => Some(layout.tabs),
            Panel::Apps => Some(content),
            Panel::Terminal(id) => self
                .visible_panes()
                .into_iter()
                .find(|(pane, _)| *pane == id)
                .map(|(_, r)| PixelRect::new(content.x + r.x, content.y + r.y, r.width, r.height)),
        }
    }
}
