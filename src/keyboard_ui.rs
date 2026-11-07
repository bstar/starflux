use super::*;
impl Workspace {
    pub(super) fn terminal_review_hint(&self) -> Option<String> {
        if self.state.space == Space::Apps {
            return None;
        }
        self.terminals
            .get(&(
                self.config.workspace.clone(),
                self.config.canonical_pane(self.state.selected()),
            ))
            .and_then(|t| t.review_hint())
    }
    pub(super) fn terminal_review_input(&mut self, input: &Input) -> bool {
        if self.modal.is_some()
            || self.context_menu.is_some()
            || self.git_open
            || self.state.space == Space::Apps
            || (self.cells && !self.cell_ui.terminal_focus)
        {
            return false;
        }
        let key = (
            self.config.workspace.clone(),
            self.config.canonical_pane(self.state.selected()),
        );
        let Some(terminal) = self
            .terminals
            .get_mut(&key)
            .filter(|t| t.review_status().is_some())
        else {
            return false;
        };
        match input {
            Input::Key { code, modifiers }
                if !matches!(code.as_str(), "f6" | "f:6")
                    && !(code == "escape" && modifiers & 2 != 0) =>
            {
                terminal.review_key(code, *modifiers)
            }
            Input::Paste { text } => terminal.review_text(text),
            Input::Embedded { event, .. } => match &event.kind {
                EmbeddedKind::Key {
                    code,
                    modifiers,
                    phase,
                } => {
                    if *phase != KeyPhase::Release {
                        terminal.review_key(code, *modifiers);
                    }
                    true
                }
                EmbeddedKind::Text { text } => terminal.review_text(text),
                _ => false,
            },
            _ => false,
        }
    }
    pub(super) fn load_keyboard(&mut self) {
        match crate::keyboard::Keyboard::load() {
            Ok(keyboard) => self.keyboard.replace(keyboard),
            Err(e) => self.note(format!("Keyboard settings: {e:#}")),
        }
    }
    fn send_prefix(&mut self, code: String, modifiers: u8) {
        for phase in [KeyPhase::Press, KeyPhase::Release] {
            let kind = EmbeddedKind::Key {
                code: code.clone(),
                modifiers: if phase == KeyPhase::Release {
                    0
                } else {
                    modifiers
                },
                phase,
            };
            if self.cells {
                self.direct_input(kind);
            } else {
                self.embedded(kind);
            }
        }
    }
    pub(super) fn keyboard_input(&mut self, input: &Input) -> bool {
        let cancel = match input {
            Input::Paste { .. } | Input::CancelPointer | Input::Resize { .. } => true,
            Input::Pointer { action, .. } => action == "down" || action.starts_with("scroll_"),
            Input::Embedded { event, .. } => match &event.kind {
                EmbeddedKind::Text { .. } | EmbeddedKind::ReleaseAll => true,
                EmbeddedKind::Pointer { action, .. } => {
                    action == "down" || action.starts_with("scroll_")
                }
                _ => false,
            },
            _ => false,
        };
        if cancel {
            self.keyboard.cancel();
        }
        let (code, modifiers, release, repeat) = match input {
            Input::Key { code, modifiers } => (code, *modifiers, false, false),
            Input::Embedded { event, .. } => match &event.kind {
                EmbeddedKind::Key {
                    code,
                    modifiers,
                    phase,
                } => (
                    code,
                    *modifiers,
                    *phase == KeyPhase::Release,
                    *phase == KeyPhase::Repeat,
                ),
                _ => return false,
            },
            _ => return false,
        };
        let blocked = self.modal.is_some() || self.context_menu.is_some() || self.git_open;
        match self
            .keyboard
            .route(code, modifiers, release, repeat, blocked)
        {
            crate::keyboard::Route::Pass => false,
            crate::keyboard::Route::Consume => true,
            crate::keyboard::Route::ReplayPrefix(code, m) => {
                self.send_prefix(code, m);
                // Route the original key to the program as well, even when
                // workspace controls currently have focus.
                match input {
                    Input::Key { code, modifiers } => self.send_prefix(code.clone(), *modifiers),
                    Input::Embedded { event, .. } => self.embedded(event.kind.clone()),
                    _ => {}
                }
                true
            }
            crate::keyboard::Route::Action(action) => {
                if let Err(e) = self.keyboard_action(&action) {
                    self.note(format!("Keyboard: {e:#}"));
                }
                true
            }
        }
    }
    pub(super) fn keyboard_action(&mut self, action: &str) -> Result<()> {
        match action {
            "next-panel" => self.focus_step(false),
            "previous-panel" => self.focus_step(true),
            "send-tab" => {
                ensure!(
                    !self.cells_overlay(),
                    "Close the menu or dialog before sending Tab"
                );
                self.panel_focus = None;
                self.cell_ui.terminal_focus = true;
                for phase in [KeyPhase::Press, KeyPhase::Release] {
                    let kind = EmbeddedKind::Key {
                        code: "tab".into(),
                        modifiers: 0,
                        phase,
                    };
                    if self.cells {
                        self.direct_input(kind);
                    } else {
                        self.embedded(kind);
                    }
                }
            }
            "workspace-menu" => self.action("workspace:menu")?,
            "project-picker" => self.action("workspace:projects")?,
            "tab-picker" => self.action("workspace:tabs")?,
            "machines" => self.action("machines:list")?,
            "plugins" => self.action("plugins:list")?,
            "terminal-links" => self.action("terminal-links:list")?,
            "worktrees" => self.action("worktrees:list")?,
            "copy-mode" | "search-history" => self.action(&format!("terminal:{action}"))?,
            "attention" => self.action("attention:inbox")?,
            "literal-prefix" => {
                if let Some((code, m)) = self.keyboard.literal_prefix() {
                    self.send_prefix(code, m);
                }
            }
            "help" => {
                self.release();
                self.context_menu = Some(crate::shelf::ContextMenu {
                    position: [16, 16],
                    title: "Keyboard shortcuts".into(),
                    subtitle: "↑ ↓ / PgUp PgDn · Enter · Esc".into(),
                    items: self
                        .keyboard
                        .help()
                        .into_iter()
                        .filter(|(_, _, a)| self.app_view_available || a != "apps")
                        .filter(|(_, _, a)| !self.cells || !a.starts_with("font-"))
                        .filter(|(_, _, a)| {
                            self.desktop.is_none()
                                || !matches!(a.as_str(), "copy-mode" | "search-history")
                        })
                        .map(|(label, hint, action)| crate::shelf::MenuItem {
                            label,
                            hint,
                            action: format!("keyboard:{action}"),
                        })
                        .collect(),
                    selected: Some(0),
                });
            }
            "reload-keys" => {
                self.load_keyboard();
            }
            "history-up" | "history-down" | "history-end" => {
                ensure!(
                    self.state.space != Space::Apps,
                    "Switch to a terminal to scroll history"
                );
                let key = (
                    self.config.workspace.clone(),
                    self.config.canonical_pane(self.state.selected()),
                );
                if action == "history-up"
                    && self
                        .terminals
                        .get(&key)
                        .is_none_or(|t| t.review_status().is_none())
                {
                    self.action("terminal:copy-mode")?;
                }
                if let Some(terminal) = self.terminals.get_mut(&key) {
                    terminal.scroll_page(match action {
                        "history-up" => -1,
                        "history-down" => 1,
                        _ => 0,
                    });
                }
            }
            "new-tab" => self.action("terminal:new")?,
            "split-right" | "split-down" | "resize-less" | "resize-more" | "focus-left"
            | "focus-right" | "focus-up" | "focus-down" | "move-left" | "move-right"
            | "move-up" | "move-down" => self.action(&format!("terminal:{action}"))?,
            "zoom-pane" => self.action("terminal:zoom")?,
            "swap-pane" => self.action("terminal:swap-next")?,
            "close-pane" => self.action("terminal:close-pane")?,
            "next-tab" => self.action("terminal:next")?,
            "previous-tab" => self.action("terminal:previous")?,
            "next-project" | "previous-project" => {
                if let Some(n) = self
                    .projects
                    .iter()
                    .position(|p| p.workspace == self.config.workspace)
                {
                    let next = if action == "next-project" {
                        (n + 1) % self.projects.len()
                    } else {
                        (n + self.projects.len() - 1) % self.projects.len()
                    };
                    self.switch_project(&self.projects[next].workspace.clone())?;
                }
            }
            "next-pane" => {
                let ids = self
                    .state
                    .unzoomed_tree()
                    .map(|tree| tree.ids())
                    .unwrap_or_default();
                if !ids.is_empty() {
                    let n = ids
                        .iter()
                        .position(|id| *id == self.state.selected())
                        .unwrap_or(0);
                    let other = ids[(n + 1) % ids.len()];
                    self.release();
                    self.state.select_terminal(other);
                    self.layout();
                    self.save();
                }
            }
            "project-menu" => {
                self.open_context_menu(
                    Some(&format!("project:{}", self.config.workspace)),
                    [16, 16],
                );
                if let Some(m) = &mut self.context_menu {
                    m.selected = Some(0);
                }
            }
            "pane-menu" => {
                self.open_context_menu(
                    Some(&format!("terminal:{}", self.state.selected())),
                    [16, 16],
                );
                if let Some(m) = &mut self.context_menu {
                    m.selected = Some(0);
                }
            }
            "new-project" => self.action("project:new")?,
            "sessions" => self.action("sessions:open")?,
            "split" => self.action("terminal:split")?,
            "rename-tab" => self.action(&format!("rename-tab:{}", self.state.selected()))?,
            "git" => self.action("git:details")?,
            "apps" => {
                ensure!(
                    self.app_view_available,
                    "App Space is unavailable in this presentation"
                );
                self.set_space(Space::Apps);
            }
            "terminal" => self.set_space(Space::Terminals),
            "font-increase" => self.action("terminal:font:increase")?,
            "font-decrease" => self.action("terminal:font:decrease")?,
            "font-reset" => self.action("terminal:font:reset")?,
            "refresh" => self.refresh_ui()?,
            "detach" => self.quit = true,
            a if a.starts_with("tab-") => {
                let n = a.trim_start_matches("tab-").parse::<usize>()?;
                if let Some(id) = self.state.terminal_ids().get(n.saturating_sub(1)).copied() {
                    self.action(&format!("terminal:{id}"))?;
                }
            }
            _ => anyhow::bail!("Unknown keyboard action {action}"),
        }
        if self.cells
            && (matches!(
                action,
                "new-tab"
                    | "next-tab"
                    | "previous-tab"
                    | "next-project"
                    | "previous-project"
                    | "next-pane"
                    | "split"
                    | "terminal"
            ) || action.starts_with("tab-"))
        {
            self.cell_ui.terminal_focus = true;
        }
        Ok(())
    }
}
