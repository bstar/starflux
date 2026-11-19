use super::*;
use crate::panes::{Area, Axis, Layout};
impl Workspace {
    pub(super) fn close_split_pane_view(&mut self, pane: usize) -> bool {
        if !self.state.close_pane(pane) {
            return false;
        }
        self.release();
        self.cell_ui.terminal_focus = true;
        self.layout();
        self.save();
        true
    }

    pub(super) fn native_pane_geometry(&self) -> Layout {
        let minimum = [
            (self.viewport.width / self.viewport.columns.max(1) as u32)
                .saturating_mul(8)
                .clamp(64, u32::from(u16::MAX)) as u16,
            (self.viewport.height / self.viewport.rows.max(1) as u32)
                .saturating_mul(3)
                .clamp(32, u32::from(u16::MAX)) as u16,
        ];
        self.state
            .layout_tree()
            .map(|tree| {
                tree.layout(
                    Area {
                        x: 0,
                        y: 0,
                        width: self.settings.width as u16,
                        height: self.settings.height as u16,
                    },
                    12,
                    minimum,
                    self.state.selected(),
                )
            })
            .unwrap_or_default()
    }
    pub(super) fn pane_divider_input(
        &mut self,
        action: &str,
        button: u8,
        position: [f64; 2],
        geometry: Layout,
    ) -> bool {
        if self.state.space == Space::Apps {
            return false;
        }
        if action == "down"
            && button == 0
            && let Some(divider) = geometry.dividers.iter().find(|d| d.rect.contains(position))
        {
            self.pane_drag = Some(divider.clone());
            self.text_gesture.cancel();
            return true;
        }
        let Some(divider) = self.pane_drag.clone() else {
            return false;
        };
        if self
            .state
            .layout_tree()
            .map(|t| t.ids())
            .unwrap_or_default()
            != divider.leaves
        {
            self.pane_drag = None;
            return true;
        }
        if action == "up" {
            self.pane_drag = None;
            self.save();
            return true;
        }
        if matches!(action, "move" | "drag") {
            let (coordinate, start, length, gap) = match divider.axis {
                Axis::Right => (
                    position[0],
                    divider.parent.x,
                    divider.parent.width,
                    divider.rect.width,
                ),
                Axis::Down => (
                    position[1],
                    divider.parent.y,
                    divider.parent.height,
                    divider.rect.height,
                ),
            };
            let ratio = ((coordinate - f64::from(start))
                / f64::from(length.saturating_sub(gap).max(1))
                * 10000.)
                .clamp(1000., 9000.) as u16;
            self.state.resize_pane(&divider.path, ratio);
            self.layout();
        }
        true
    }
    pub(super) fn pane_action(&mut self, action: &str) -> Result<bool> {
        match action {
            "terminal:copy-mode"
            | "terminal:search-history"
            | "terminal:saved-current"
            | "terminal:saved-previous" => {
                ensure!(
                    self.desktop.is_none(),
                    "History review requires the native terminal renderer"
                );
                ensure!(
                    self.state.space != Space::Apps,
                    "Switch to a terminal to review history"
                );
                self.release();
                let key = (
                    self.config.workspace.clone(),
                    self.config.canonical_pane(self.state.selected()),
                );
                if !self.terminals.contains_key(&key) {
                    self.launch(self.config.clone(), key.1);
                }
                if self.native_terminal_cells
                    && self.desktop.is_none()
                    && let Some((_, area)) = self
                        .visible_panes()
                        .into_iter()
                        .find(|(id, _)| *id == key.1)
                {
                    let (rect, padding) = if self.cells {
                        (
                            self.cell_panes()
                                .into_iter()
                                .find(|(id, _)| *id == key.1)
                                .context("Terminal pane is hidden")?
                                .1,
                            [0, 0],
                        )
                    } else {
                        native_terminal_rect(area, self.presentation_canvas(), self.viewport)
                    };
                    if let Some(terminal) = self.terminals.get_mut(&key) {
                        terminal.set_metrics(self.typography);
                        terminal.resize_native(
                            rect,
                            padding,
                            [
                                (self.viewport.width / u32::from(self.viewport.columns.max(1)))
                                    .max(1) as u16,
                                (self.viewport.height / u32::from(self.viewport.rows.max(1))).max(1)
                                    as u16,
                            ],
                        )?;
                    }
                }
                let token = self
                    .terminals
                    .get_mut(&key)
                    .context(
                        "Terminal history is not attached yet; focus the terminal and try again",
                    )?
                    .review(action.ends_with("search-history"));
                let config = self.config.clone();
                let pane = key.1;
                let epoch = self.state.terminal_epochs.get(&pane).cloned();
                let tx = self.tx.clone();
                let saved = action
                    .strip_prefix("terminal:saved-")
                    .map(|s| s == "previous");
                std::thread::spawn(move || {
                    if let Some(previous) = saved {
                        let result = crate::cold_start::load(&config.workspace, pane, previous)
                            .map_err(|e| format!("{e:#}"));
                        let _ = tx.send(Event::SavedScreen(
                            config.workspace,
                            pane,
                            epoch,
                            token,
                            result,
                        ));
                        return;
                    }
                    let result = (|| {
                        let before = ProjectState::load(&config)?;
                        ensure!(
                            before.terminal_epochs.get(&pane) == epoch.as_ref(),
                            "Terminal was replaced before history capture"
                        );
                        let output = config.terminal_history(pane)?;
                        let after = ProjectState::load(&config)?;
                        ensure!(
                            after.terminal_epochs.get(&pane) == epoch.as_ref(),
                            "Terminal was replaced during history capture"
                        );
                        Ok(output)
                    })()
                    .map_err(|e: anyhow::Error| format!("{e:#}"));
                    let _ = tx.send(Event::TerminalHistory(
                        config.workspace,
                        pane,
                        epoch,
                        token,
                        result,
                    ));
                });
            }
            "terminal:split-right" | "terminal:split-down" => {
                let _lock = crate::project_sync::transaction(&self.config)?;
                self.sync_project()?;
                self.release();
                self.state.split_pane(if action.ends_with("down") {
                    Axis::Down
                } else {
                    Axis::Right
                })?;
                self.ensure_sessions();
                self.layout();
                self.save();
            }
            "terminal:zoom" => {
                self.release();
                self.state.pane_zoom = !self.state.pane_zoom;
                self.layout();
                self.save();
            }
            "terminal:swap-next" => {
                self.release();
                let ids = self
                    .state
                    .unzoomed_tree()
                    .map(|t| t.ids())
                    .unwrap_or_default();
                if ids.len() > 1 {
                    let n = ids
                        .iter()
                        .position(|id| *id == self.state.terminal)
                        .unwrap_or(0);
                    self.state.swap_panes(ids[n], ids[(n + 1) % ids.len()]);
                    self.layout();
                    self.save();
                }
            }
            "terminal:resize-less" | "terminal:resize-more" => {
                if let Some(tree) = self.state.unzoomed_tree()
                    && let Some(mut path) = tree.path_to(self.state.terminal)
                {
                    let second = path.pop().unwrap_or(false);
                    if let Some(ratio) = tree.ratio_at(&path) {
                        let ratio = if action.ends_with("less") != second {
                            ratio.saturating_sub(500)
                        } else {
                            ratio.saturating_add(500)
                        };
                        self.state.resize_pane(&path, ratio);
                        self.layout();
                        self.save();
                    }
                }
            }
            a if a.starts_with("terminal:focus-") || a.starts_with("terminal:move-") => {
                let moving = a.starts_with("terminal:move-");
                let direction = a
                    .strip_prefix("terminal:focus-")
                    .or_else(|| a.strip_prefix("terminal:move-"))
                    .unwrap();
                ensure!(
                    matches!(direction, "left" | "right" | "up" | "down"),
                    "Invalid focus direction"
                );
                let panes = self.visible_panes();
                let selected = self.config.canonical_pane(self.state.selected());
                if let Some((_, origin)) = panes.iter().find(|(id, _)| *id == selected) {
                    let center = |r: &PixelRect| {
                        [
                            f64::from(r.x) + f64::from(r.width) / 2.,
                            f64::from(r.y) + f64::from(r.height) / 2.,
                        ]
                    };
                    let from = center(origin);
                    let axis = usize::from(matches!(direction, "up" | "down"));
                    let sign = if matches!(direction, "left" | "up") {
                        -1.
                    } else {
                        1.
                    };
                    if let Some((id, _)) = panes
                        .iter()
                        .filter(|(id, r)| {
                            *id != selected && (center(r)[axis] - from[axis]) * sign > 0.
                        })
                        .min_by(|(_, a), (_, b)| {
                            let score = |r: &PixelRect| {
                                let to = center(r);
                                (to[axis] - from[axis]).abs()
                                    + (to[1 - axis] - from[1 - axis]).abs() * 2.
                            };
                            score(a).total_cmp(&score(b))
                        })
                    {
                        let id = *id;
                        self.release();
                        if moving {
                            self.state.swap_panes(selected, id);
                        } else {
                            self.state.select_terminal(id);
                        }
                        self.layout();
                        self.save();
                    }
                }
            }
            a if a.starts_with("split-existing:") => {
                let (anchor, target) = a["split-existing:".len()..]
                    .split_once(':')
                    .context("Invalid existing-tab split")?;
                let anchor = anchor.parse()?;
                let _lock = crate::project_sync::transaction(&self.config)?;
                self.sync_project()?;
                if target == "menu" {
                    let visible = self
                        .state
                        .unzoomed_tree()
                        .context("No terminal layout")?
                        .ids();
                    ensure!(
                        visible.contains(&anchor),
                        "Target pane is outside the current split layout"
                    );
                    let anchor_epoch = self
                        .state
                        .terminal_epochs
                        .get(&anchor)
                        .map_or("", String::as_str);
                    let mut items = Vec::new();
                    for tab in self
                        .state
                        .terminal_ids()
                        .into_iter()
                        .filter(|tab| !visible.contains(tab))
                    {
                        let tab_epoch = self
                            .state
                            .terminal_epochs
                            .get(&tab)
                            .map_or("", String::as_str);
                        for side in ["right", "down"] {
                            items.push(crate::shelf::MenuItem {
                                label: format!("{} · {side}", self.terminal_label(tab)),
                                hint: String::new(),
                                action: format!("split-existing:{anchor}:{tab}:{side}:{anchor_epoch}:{tab_epoch}"),
                            });
                        }
                    }
                    ensure!(!items.is_empty(), "No hidden terminal tabs to split with");
                    self.release();
                    self.context_menu = Some(crate::shelf::ContextMenu {
                        position: [16, 16],
                        title: "Split with existing tab".into(),
                        subtitle: String::new(),
                        items,
                        selected: Some(0),
                    });
                    return Ok(true);
                }
                let [tab, side, anchor_epoch, tab_epoch] = target
                    .split(':')
                    .collect::<Vec<_>>()
                    .try_into()
                    .ok()
                    .context("Invalid split target")?;
                let tab = tab.parse()?;
                let matches_epoch = |pane, epoch: &str| {
                    self.state
                        .terminal_epochs
                        .get(&pane)
                        .map_or("", String::as_str)
                        == epoch
                };
                ensure!(
                    matches_epoch(anchor, anchor_epoch) && matches_epoch(tab, tab_epoch),
                    "A terminal changed while the split picker was open; reopen the picker"
                );
                let axis = match side {
                    "right" => Axis::Right,
                    "down" => Axis::Down,
                    _ => anyhow::bail!("Invalid split direction"),
                };
                self.state.split_with_tab(anchor, tab, axis)?;
                self.release();
                self.context_menu = None;
                self.layout();
                self.save();
            }
            a if a.starts_with("pane-edge:") => {
                let (id, direction) = a["pane-edge:".len()..]
                    .split_once(':')
                    .context("Invalid pane placement")?;
                let id = id.parse()?;
                let _lock = crate::project_sync::transaction(&self.config)?;
                self.sync_project()?;
                if direction == "menu" {
                    ensure!(
                        self.state.terminal_ids().contains(&id),
                        "Terminal no longer exists"
                    );
                    self.release();
                    let project = &self.config.workspace;
                    let epoch = self
                        .state
                        .terminal_epochs
                        .get(&id)
                        .map_or("", String::as_str);
                    self.context_menu = Some(crate::shelf::ContextMenu {
                        position: [16, 16],
                        title: "Move pane to edge".into(),
                        subtitle: String::new(),
                        items: [
                            ("Left", "left"),
                            ("Right", "right"),
                            ("Top", "up"),
                            ("Bottom", "down"),
                        ]
                        .into_iter()
                        .map(|(label, edge)| crate::shelf::MenuItem {
                            label: label.into(),
                            hint: String::new(),
                            action: format!("pane-edge:{id}:{edge}:{project}:{epoch}"),
                        })
                        .collect(),
                        selected: Some(0),
                    });
                    return Ok(true);
                }
                let direction = if let Some((direction, identity)) = direction.split_once(':') {
                    let (project, epoch) =
                        identity.split_once(':').context("Invalid pane identity")?;
                    ensure!(
                        project == self.config.workspace
                            && self.state.terminal_ids().contains(&id)
                            && self
                                .state
                                .terminal_epochs
                                .get(&id)
                                .map_or("", String::as_str)
                                == epoch,
                        "Terminal changed while the edge picker was open; reopen the picker"
                    );
                    direction
                } else {
                    direction
                };
                let (axis, leading) = match direction {
                    "left" => (Axis::Right, true),
                    "right" => (Axis::Right, false),
                    "up" => (Axis::Down, true),
                    "down" => (Axis::Down, false),
                    _ => anyhow::bail!("Invalid pane edge"),
                };
                self.state.move_pane_to_edge(id, axis, leading)?;
                self.release();
                self.context_menu = None;
                self.layout();
                self.save();
            }
            a if a.starts_with("pane-target:") => {
                let _lock = crate::project_sync::transaction(&self.config)?;
                self.sync_project()?;
                let (id, next) = a
                    .trim_start_matches("pane-target:")
                    .split_once(':')
                    .context("Invalid pane action")?;
                self.action(&format!("terminal:{id}"))?;
                self.action(&format!("terminal:{next}"))?;
            }
            _ => return Ok(false),
        }
        self.cell_ui.terminal_focus = true;
        Ok(true)
    }
}
