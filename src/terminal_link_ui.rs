use super::*;
#[derive(Clone)]
pub(super) struct Target {
    project: crate::plugins::Project,
    pane: usize,
    url: String,
}
impl Workspace {
    fn resolve_terminal_link(&mut self, target: Target) {
        self.plugin_request("links".into(), move || {
            let mut result = crate::plugin_links::resolve(&target.url, 0, 16)?;
            result["project"] = json!(target.project);
            Ok(result)
        });
    }
    pub(super) fn terminal_link_action(&mut self, action: &str) -> Result<bool> {
        if action == "terminal-links:list" {
            ensure!(
                self.state.space != Space::Apps && self.desktop.is_none(),
                "Switch to a native terminal to choose visible links"
            );
            let pane = self.config.canonical_pane(self.state.selected());
            let links = self
                .terminals
                .get(&(self.config.workspace.clone(), pane))
                .context("Terminal is not attached yet")?
                .visible_links();
            self.terminal_link_choices = links
                .into_iter()
                .map(|link| Target {
                    project: crate::plugins::project(&self.config),
                    pane,
                    url: link.url,
                })
                .collect();
            self.release();
            let mut items = self
                .terminal_link_choices
                .iter()
                .enumerate()
                .map(|(index, target)| crate::shelf::MenuItem {
                    label: target.url.clone(),
                    hint: String::new(),
                    action: format!("terminal-links:choose:{index}"),
                })
                .collect::<Vec<_>>();
            if items.is_empty() {
                items.push(crate::shelf::MenuItem {
                    label: "No supported URL text is visible".into(),
                    hint: String::new(),
                    action: "menu:panel".into(),
                });
            }
            self.context_menu = Some(crate::shelf::ContextMenu {
                position: [16, 16],
                title: "Terminal links".into(),
                subtitle: "Choose a URL / copy or review a handler".into(),
                items,
                selected: Some(0),
            });
            return Ok(true);
        }
        if let Some(index) = action.strip_prefix("terminal-links:choose:") {
            ensure!(
                self.context_menu
                    .as_ref()
                    .is_some_and(|m| m.title == "Terminal links"),
                "Choose visible links again"
            );
            let target = self
                .terminal_link_choices
                .get(index.parse::<usize>()?)
                .context("Choose visible links again")?
                .clone();
            self.show_terminal_link(target, [16, 16]);
            return Ok(true);
        }
        let Some(kind) = action.strip_prefix("terminal-link:") else {
            return Ok(false);
        };
        ensure!(
            self.context_menu
                .as_ref()
                .is_some_and(|m| m.title == "Terminal link"),
            "Choose a visible terminal link again"
        );
        let target = self
            .terminal_link
            .clone()
            .context("Choose a visible terminal link again")?;
        match kind {
            "open" => self.resolve_terminal_link(target),
            "copy" => {
                self.effects
                    .push(ServerMessage::Clipboard { text: target.url });
                self.context_menu = None;
            }
            "options" => {
                ensure!(
                    self.config.workspace == target.project.id
                        && self.state.terminal_ids().contains(&target.pane),
                    "The source terminal changed"
                );
                self.open_context_menu(Some(&format!("terminal:{}", target.pane)), [16, 16]);
            }
            _ => anyhow::bail!("Unknown terminal link action"),
        }
        Ok(true)
    }
    fn show_terminal_link(&mut self, target: Target, position: [u16; 2]) {
        self.release();
        let item = |label: &str, action: &str| crate::shelf::MenuItem {
            label: label.into(),
            hint: String::new(),
            action: action.into(),
        };
        self.context_menu = Some(crate::shelf::ContextMenu {
            position,
            title: "Terminal link".into(),
            subtitle: target.url.clone(),
            selected: Some(0),
            items: vec![
                item("Open with a plugin…", "terminal-link:open"),
                item("Copy URL", "terminal-link:copy"),
                item("Terminal options…", "terminal-link:options"),
            ],
        });
        self.terminal_link = Some(target);
    }
    pub(super) fn terminal_link_input(&mut self, input: &Input) -> bool {
        let pointer = match input {
            Input::Embedded { event, .. } => match &event.kind {
                EmbeddedKind::Pointer {
                    action,
                    button,
                    position,
                    modifiers,
                } => Some((action.as_str(), *button, *position, *modifiers, false)),
                _ => None,
            },
            Input::Pointer {
                action,
                button,
                pixel,
                x,
                y,
                modifiers,
            } => {
                let point = if self.cells {
                    [f64::from(*x), f64::from(*y)]
                } else {
                    let p = pixel.unwrap_or([
                        u32::from(*x) * self.viewport.width
                            / u32::from(self.viewport.columns.max(1)),
                        u32::from(*y) * self.viewport.height / u32::from(self.viewport.rows.max(1)),
                    ]);
                    let canvas = self.presentation_canvas();
                    [
                        f64::from(p[0]) - f64::from(canvas.x),
                        f64::from(p[1]) - f64::from(canvas.y),
                    ]
                };
                Some((action.as_str(), *button, point, *modifiers, self.cells))
            }
            Input::CancelPointer => {
                self.link_pointer_capture = false;
                None
            }
            _ => None,
        };
        let Some((action, button, point, modifiers, cells)) = pointer else {
            return false;
        };
        if self.link_pointer_capture && action == "up" {
            self.link_pointer_capture = false;
            return true;
        }
        if action == "down" {
            self.link_pointer_capture = false;
        }
        if action != "down"
            || !(button == 1 || (button == 0 && modifiers == 2))
            || self.modal.is_some()
            || self.context_menu.is_some()
            || self.git_open
            || self.state.space == Space::Apps
            || self.desktop.is_some()
        {
            return false;
        }
        let panes = if cells {
            self.cell_panes()
                .into_iter()
                .map(|(id, r)| (id, PixelRect::new(r.x, r.y, r.width, r.height)))
                .collect()
        } else {
            self.visible_panes()
        };
        let Some((pane, area)) = panes.into_iter().find(|(_, r)| {
            point[0] >= f64::from(r.x)
                && point[0] < f64::from(r.x) + f64::from(r.width)
                && point[1] >= f64::from(r.y)
                && point[1] < f64::from(r.y) + f64::from(r.height)
        }) else {
            return false;
        };
        let relative = if cells {
            let cell = [
                (self.viewport.width / u32::from(self.viewport.columns.max(1))).max(1),
                (self.viewport.height / u32::from(self.viewport.rows.max(1))).max(1),
            ];
            [
                (point[0] - f64::from(area.x)) * f64::from(cell[0]),
                (point[1] - f64::from(area.y)) * f64::from(cell[1]),
            ]
        } else {
            [point[0] - f64::from(area.x), point[1] - f64::from(area.y)]
        };
        let url = self
            .terminals
            .get(&(self.config.workspace.clone(), pane))
            .and_then(|t| t.link_at(relative));
        let Some(url) = url else {
            return false;
        };
        let target = Target {
            project: crate::plugins::project(&self.config),
            pane,
            url,
        };
        self.link_pointer_capture = true;
        if button == 0 {
            self.resolve_terminal_link(target);
        } else {
            let anchor = if cells {
                [point[0] as u16, point[1] as u16]
            } else {
                let canvas = self.presentation_canvas();
                [
                    (point[0] + f64::from(canvas.x)) as u16,
                    (point[1] + f64::from(canvas.y)) as u16,
                ]
            };
            self.show_terminal_link(target, anchor);
        }
        true
    }
    pub(super) fn terminal_link_regions(&self, scene: &mut Scene) {
        if self.popup.is_some()
            || self.state.space == Space::Apps
            || self.desktop.is_some()
            || self.modal.is_some()
            || self.context_menu.is_some()
            || self.git_open
            || !self.native_terminal_cells
        {
            return;
        }
        let panes = if self.cells {
            self.cell_panes()
        } else {
            let content = self.presentation_canvas();
            self.visible_panes()
                .into_iter()
                .map(|(id, r)| (id, native_terminal_rect(r, content, self.viewport).0))
                .collect()
        };
        for (pane, area) in panes {
            if let Some(terminal) = self.terminals.get(&(self.config.workspace.clone(), pane)) {
                scene
                    .pointer_regions
                    .extend(crate::terminal_links::pointer_regions(
                        area,
                        terminal.links(),
                    ));
            }
        }
    }
}
