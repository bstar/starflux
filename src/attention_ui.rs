use super::*;
impl Workspace {
    pub(super) fn attention_action(&mut self, action: &str) -> Result<bool> {
        let Some(command) = action.strip_prefix("attention:") else {
            return Ok(false);
        };
        let item = |label: String, action: String| crate::shelf::MenuItem {
            label,
            hint: String::new(),
            action,
        };
        if matches!(command, "read" | "source" | "scope" | "state" | "reset") {
            self.attention_query.offset = 0;
        }
        match command {
            "next" => {
                self.attention_query.offset = self
                    .attention_query
                    .offset
                    .saturating_add(self.attention_query.limit)
                    .min(10000000);
                self.attention_load(false)?;
                return Ok(true);
            }
            "previous" => {
                self.attention_query.offset = self
                    .attention_query
                    .offset
                    .saturating_sub(self.attention_query.limit);
                self.attention_load(false)?;
                return Ok(true);
            }
            "inbox" => {
                self.attention_load(false)?;
                return Ok(true);
            }
            "search" => {
                self.open_prompt(Prompt::AttentionSearch);
                return Ok(true);
            }
            "read" => {
                use crate::notifications::ReadState::*;
                self.attention_query.read = match self.attention_query.read {
                    Unread => All,
                    All => Read,
                    Read => Unread,
                };
                self.attention_load(false)?;
                return Ok(true);
            }
            "source" => {
                use crate::notifications::Source::*;
                self.attention_query.source = match self.attention_query.source {
                    All => Reported,
                    Reported => Screen,
                    Screen => All,
                };
                self.attention_load(false)?;
                return Ok(true);
            }
            "scope" => {
                if self.attention_query.project.is_some() {
                    self.attention_query.project = None;
                    self.attention_query.session =
                        Some(crate::session_catalog::group(&self.config));
                } else if self.attention_query.session.is_some() {
                    self.attention_query.session = None;
                } else {
                    self.attention_query.project = Some(self.config.workspace.clone());
                }
                self.attention_load(false)?;
                return Ok(true);
            }
            "state" => {
                use crate::agent_state::Lifecycle::*;
                self.attention_query.state = match self.attention_query.state {
                    None => Some(NeedsInput),
                    Some(NeedsInput) => Some(WaitingForApproval),
                    Some(WaitingForApproval) => Some(Completed),
                    Some(Completed) => Some(Failed),
                    Some(Failed) => Some(BackendUnavailable),
                    _ => None,
                };
                self.attention_load(false)?;
                return Ok(true);
            }
            "reset" => {
                self.attention_query = Default::default();
                self.attention_load(false)?;
                return Ok(true);
            }
            "settings" => {
                self.attention_settings()?;
                return Ok(true);
            }
            "retention" => {
                self.open_prompt(Prompt::NotificationRetention(
                    self.notification_settings
                        .clone()
                        .unwrap_or(crate::notifications::settings()?),
                ));
                return Ok(true);
            }
            "prune" => {
                let tx = self.tx.clone();
                self.context_menu = None;
                self.note("Applying notification retention…".into());
                std::thread::spawn(move || {
                    let result =
                        crate::notifications::apply_retention().map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Event::AttentionUpdated(result));
                });
                return Ok(true);
            }
            "mark" => {
                let items = self
                    .attention_items
                    .iter()
                    .filter(|i| !i.event.dismissed)
                    .map(|i| crate::notifications::Reference {
                        project: i.project.clone(),
                        event: i.event.id.clone(),
                    })
                    .collect::<Vec<_>>();
                let tx = self.tx.clone();
                self.context_menu = None;
                self.note("Marking matching notifications read…".into());
                std::thread::spawn(move || {
                    let result =
                        crate::notifications::mark_read(&items).map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Event::AttentionUpdated(result));
                });
                return Ok(true);
            }
            "diagnostics" => {
                let status = crate::notifications::delivery_status()?;
                let mut rows = status["attempts"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(id, v)| {
                        (
                            v["at_ms"].as_u64().unwrap_or(0),
                            item(
                                format!(
                                    "{id} · {}{}",
                                    v["outcome"].as_str().unwrap_or("unknown"),
                                    v["error"]
                                        .as_str()
                                        .map(|e| format!(" · {e}"))
                                        .unwrap_or_default()
                                ),
                                "menu:panel".into(),
                            ),
                        )
                    })
                    .collect::<Vec<_>>();
                rows.sort_by_key(|v| std::cmp::Reverse(v.0));
                let mut items = rows.into_iter().take(20).map(|v| v.1).collect::<Vec<_>>();
                if items.is_empty() {
                    items.push(item(
                        "No desktop delivery attempts".into(),
                        "menu:panel".into(),
                    ));
                }
                items.push(item(
                    "Notification settings".into(),
                    "attention:settings".into(),
                ));
                self.context_menu = Some(crate::shelf::ContextMenu {
                    position: [16, 16],
                    title: "Notification delivery".into(),
                    subtitle: status["capability"]["reason"].as_str().unwrap_or("").into(),
                    items,
                    selected: Some(0),
                });
                return Ok(true);
            }
            _ => {}
        }
        if let Some(option) = command.strip_prefix("option:") {
            let mut settings = self
                .notification_settings
                .clone()
                .context("Open notification settings before editing")?;
            let revision = settings.revision.clone();
            match option {
                "delivery" => {
                    settings.delivery = match settings.delivery {
                        crate::notifications::Delivery::InApp
                            if crate::notifications::system_capability()["available"] == true =>
                        {
                            crate::notifications::Delivery::System
                        }
                        crate::notifications::Delivery::Off => {
                            crate::notifications::Delivery::InApp
                        }
                        _ => crate::notifications::Delivery::Off,
                    };
                }
                "active" => settings.suppress_active = !settings.suppress_active,
                _ => {
                    use crate::agent_state::Lifecycle::*;
                    let state = match option {
                        "questions" => NeedsInput,
                        "approvals" => WaitingForApproval,
                        "completed" => Completed,
                        "failed" => Failed,
                        "backend" => BackendUnavailable,
                        _ => anyhow::bail!("Unknown notification option"),
                    };
                    if settings.states.contains(&state) {
                        settings.states.retain(|s| *s != state);
                    } else {
                        settings.states.push(state);
                    }
                }
            }
            crate::notifications::save(settings, &revision)?;
            self.attention_settings()?;
            return Ok(true);
        }
        let (operation, target) = command
            .split_once(':')
            .context("Invalid attention action")?;
        let (workspace, id) = target.split_once(':').context("Invalid attention item")?;
        let config = crate::session_catalog::projects()
            .into_iter()
            .find(|c| c.workspace == workspace)
            .context("Project no longer exists")?;
        let event = crate::agent_state::attention(&config)?
            .into_iter()
            .find(|e| e.id == id)
            .context("Attention item no longer exists")?;
        match operation {
            "view" => {
                self.release();
                self.context_menu = Some(crate::shelf::ContextMenu {
                    position: [16, 16],
                    title: "Attention item".into(),
                    subtitle: format!(
                        "{} · {}",
                        config.label.as_deref().unwrap_or(workspace),
                        event.label()
                    ),
                    items: vec![
                        item(
                            format!(
                                "Source: {} · {} · {}",
                                event.source,
                                crate::session_catalog::group(&config),
                                if event.dismissed { "read" } else { "unread" }
                            ),
                            "menu:panel".into(),
                        ),
                        item(
                            format!("Open pane {}", event.pane + 1),
                            format!("attention:open:{workspace}:{id}"),
                        ),
                        item(
                            "Dismiss notification".into(),
                            format!("attention:dismiss:{workspace}:{id}"),
                        ),
                        item("Back to inbox".into(), "attention:inbox".into()),
                    ],
                    selected: Some(0),
                });
            }
            "dismiss" => {
                crate::agent_state::dismiss(&config, id)?;
                self.attention_action("attention:inbox")?;
            }
            "open" => {
                let state = ProjectState::load(&config)?;
                ensure!(
                    state.terminal_ids().contains(&event.pane)
                        && state.terminal_epochs.get(&event.pane) == event.epoch.as_ref(),
                    "Original pane no longer exists; notification retained"
                );
                if self.projects.iter().any(|c| c.workspace == workspace) {
                    self.switch_project(workspace)?;
                } else {
                    self.open_session(config.clone())?;
                }
                self.context_menu = None;
                ensure!(
                    self.state.terminal_ids().contains(&event.pane)
                        && self.state.terminal_epochs.get(&event.pane) == event.epoch.as_ref(),
                    "Pane changed while opening its session; notification retained"
                );
                self.action(&format!("terminal:{}", event.pane))?;
                crate::agent_state::dismiss(&config, id)?;
                self.cell_ui.terminal_focus = true;
            }
            _ => anyhow::bail!("Unknown attention action"),
        }
        Ok(true)
    }
    pub(super) fn attention_load(&mut self, quiet: bool) -> Result<()> {
        if !quiet {
            crate::notifications::save_query(&self.controller_id, &self.attention_query)?;
        }
        let token = Instant::now();
        self.attention_token = Some(token);
        self.attention_loading = true;
        if !quiet {
            self.release();
            self.context_menu = Some(crate::shelf::ContextMenu {
                position: [16, 16],
                title: "Loading attention…".into(),
                subtitle: String::new(),
                items: vec![crate::shelf::MenuItem {
                    label: "Loading…".into(),
                    hint: String::new(),
                    action: "menu:panel".into(),
                }],
                selected: Some(0),
            });
        }
        let tx = self.tx.clone();
        let query = self.attention_query.clone();
        std::thread::spawn(move || {
            let result = crate::notifications::query(&query).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::AttentionInbox(token, result));
        });
        Ok(())
    }
    pub(super) fn attention_render(&mut self, result: crate::notifications::Inbox) {
        let item = |label: String, action: &str| crate::shelf::MenuItem {
            label,
            hint: String::new(),
            action: action.into(),
        };
        self.attention_query.offset = result.offset;
        self.attention_total = result.total;
        let selected = self
            .context_menu
            .as_ref()
            .and_then(|m| m.selected.and_then(|i| m.items.get(i)))
            .map(|i| i.action.clone());
        let mut items = result
            .items
            .iter()
            .map(|i| {
                item(
                    format!(
                        "{} · {} · pane {} · {}{}",
                        i.label,
                        i.session,
                        i.event.pane + 1,
                        i.event.label(),
                        if i.event.dismissed { " · read" } else { "" }
                    ),
                    &format!("attention:view:{}:{}", i.project, i.event.id),
                )
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            items.push(item(
                if self.attention_query.search.is_empty()
                    && self.attention_query.read == crate::notifications::ReadState::Unread
                {
                    "No unread attention items"
                } else {
                    "No matching attention items"
                }
                .into(),
                "menu:panel",
            ));
        }
        items.extend(result.warnings.iter().map(|w| {
            item(
                format!(
                    "{} · attention data unavailable",
                    w["project"].as_str().unwrap_or("?")
                ),
                "menu:panel",
            )
        }));
        let scope = self
            .attention_query
            .project
            .as_deref()
            .or(self.attention_query.session.as_deref())
            .unwrap_or("All saved sessions");
        items.extend([
            item(
                format!(
                    "Search: {}",
                    if self.attention_query.search.is_empty() {
                        "all"
                    } else {
                        &self.attention_query.search
                    }
                ),
                "attention:search",
            ),
            item(
                format!(
                    "Read filter: {}",
                    match self.attention_query.read {
                        crate::notifications::ReadState::Unread => "Unread",
                        crate::notifications::ReadState::All => "All items",
                        crate::notifications::ReadState::Read => "Read",
                    }
                ),
                "attention:read",
            ),
            item(
                format!(
                    "State: {}",
                    match &self.attention_query.state {
                        None => "All events",
                        Some(crate::agent_state::Lifecycle::NeedsInput) => "Questions",
                        Some(crate::agent_state::Lifecycle::WaitingForApproval) => "Approvals",
                        Some(crate::agent_state::Lifecycle::Completed) => "Completed",
                        Some(crate::agent_state::Lifecycle::Failed) => "Failures",
                        Some(crate::agent_state::Lifecycle::BackendUnavailable) =>
                            "Backend unavailable",
                        _ => "All events",
                    }
                ),
                "attention:state",
            ),
            item(format!("Scope: {scope}"), "attention:scope"),
            item(
                format!(
                    "Source: {}",
                    match self.attention_query.source {
                        crate::notifications::Source::All => "All sources",
                        crate::notifications::Source::Reported => "Reported events",
                        crate::notifications::Source::Screen => "Screen observations",
                    }
                ),
                "attention:source",
            ),
            item(
                format!(
                    "Mark unread items on this page read ({})",
                    result.items.iter().filter(|i| !i.event.dismissed).count()
                ),
                "attention:mark",
            ),
            item("Reset filters".into(), "attention:reset"),
            item("Notification settings…".into(), "attention:settings"),
            item("Refresh inbox".into(), "attention:inbox"),
        ]);
        if result.offset > 0 {
            items.push(item("Previous page".into(), "attention:previous"));
        }
        if result.offset + result.items.len() < result.total {
            items.push(item("Next page".into(), "attention:next"));
        }
        let selected = selected
            .and_then(|action| items.iter().position(|i| i.action == action))
            .unwrap_or(0);
        self.attention_items = result.items;
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [16, 16],
            title: "Attention inbox".into(),
            subtitle: format!(
                "{} matching · page {} · / search",
                self.attention_total,
                1 + self.attention_query.offset / self.attention_query.limit
            ),
            items,
            selected: Some(selected),
        });
    }
    fn attention_settings(&mut self) -> Result<()> {
        let s = crate::notifications::settings()?;
        self.notification_settings = Some(s.clone());
        let capability = crate::notifications::system_capability();
        let item = |label: String, action: &str| crate::shelf::MenuItem {
            label,
            hint: String::new(),
            action: action.into(),
        };
        use crate::agent_state::Lifecycle::*;
        let mut items = vec![
            item(
                format!(
                    "Popup delivery: {}",
                    match s.delivery {
                        crate::notifications::Delivery::InApp => "In-app status",
                        crate::notifications::Delivery::System => "Desktop + in-app status",
                        crate::notifications::Delivery::Off => "Off (inbox retained)",
                    }
                ),
                "attention:option:delivery",
            ),
            item(
                format!("Suppress active panes: {}", s.suppress_active),
                "attention:option:active",
            ),
        ];
        for (name, label, state) in [
            ("questions", "Questions", NeedsInput),
            ("approvals", "Approvals", WaitingForApproval),
            ("completed", "Completed work", Completed),
            ("failed", "Failures", Failed),
            ("backend", "Backend unavailable", BackendUnavailable),
        ] {
            items.push(item(
                format!(
                    "{} {label}",
                    if s.states.contains(&state) {
                        "✓"
                    } else {
                        "·"
                    }
                ),
                &format!("attention:option:{name}"),
            ));
        }
        items.extend([
            item(
                format!(
                    "Retention: {} days / {} items per project…",
                    s.retention_days, s.max_items_per_project
                ),
                "attention:retention",
            ),
            item("Apply retention now".into(), "attention:prune"),
            item(
                "Desktop delivery diagnostics".into(),
                "attention:diagnostics",
            ),
            item("Back to inbox".into(), "attention:inbox"),
        ]);
        self.release();
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [16, 16],
            title: "Notification settings".into(),
            subtitle: capability["reason"].as_str().unwrap_or("").into(),
            items,
            selected: Some(0),
        });
        Ok(())
    }
}
