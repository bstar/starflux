use super::*;
#[derive(Clone)]
pub(super) struct Coordinator {
    category: String,
    tab: String,
    data: Value,
    error: Option<String>,
    pending: Option<Instant>,
    updated: Instant,
    cursor: usize,
    offset: usize,
    proposal: Option<(String, String)>,
    targets: Value,
    target: Option<Value>,
}
fn item(label: impl Into<String>, action: impl Into<String>) -> crate::shelf::MenuItem {
    crate::shelf::MenuItem {
        label: label.into(),
        hint: String::new(),
        action: action.into(),
    }
}
fn lines(width: usize, items: &mut Vec<crate::shelf::MenuItem>, text: &str) {
    for line in text.lines() {
        let chars = line.chars().collect::<Vec<_>>();
        if chars.is_empty() {
            items.push(item(" ", "category-agent:noop"));
        }
        for part in chars.chunks(width) {
            items.push(item(part.iter().collect::<String>(), "category-agent:noop"));
        }
    }
}
const FLEET: &str = "Keep the member projects aligned as STAR/FLEET. Map shared STAR/KIT primitives and reference implementations. Compare equivalent capabilities across members; separate intentional product differences from parity gaps. Evaluate reported progress against source evidence and acceptance checks. Track dependencies and cross-project regressions. Suggest shared toolkit changes before duplicated application work. Propose scoped tasks with evidence and explicit verification; never change code or dispatch work without user approval.";
impl Workspace {
    pub(super) fn coordinator_request(
        &mut self,
        work: impl FnOnce(&Config, &str) -> Result<()> + Send + 'static,
    ) {
        let Some(ui) = self.coordinator.as_mut() else {
            return;
        };
        let token = Instant::now();
        ui.pending = Some(token);
        let config = self.config.clone();
        let category = ui.category.clone();
        let tx = self.tx.clone();
        let offset = ui.offset;
        std::thread::spawn(move || {
            let result = (|| -> Result<Value> {
                work(&config, &category)?;
                let mut data = crate::category_agent::read(&config, &category)?;
                if offset > 0 {
                    data["runtime"] =
                        crate::category_agent_runtime::snapshot_page(&config, &category, offset)?;
                }
                Ok(data)
            })();
            let _ = tx.send(Event::CategoryAgent(
                token,
                category,
                result.map_err(|e| format!("{e:#}")),
            ));
        });
    }
    pub(super) fn coordinator_result(
        &mut self,
        token: Instant,
        category: &str,
        result: std::result::Result<Value, String>,
    ) {
        let Some(ui) = self
            .coordinator
            .as_mut()
            .filter(|u| u.category == category && u.pending == Some(token))
        else {
            return;
        };
        ui.pending = None;
        ui.updated = Instant::now();
        match result {
            Ok(data) => {
                if data.get("target_choices").is_some() {
                    ui.targets = data["target_choices"].clone();
                } else {
                    ui.data = data;
                }
                // A periodic read must not erase a failed selection before the user can read it.
            }
            Err(error) => ui.error = Some(error),
        }
        if self
            .coordinator
            .as_ref()
            .is_some_and(|u| !u.category.is_empty())
            && self.modal.is_none()
            && self
                .context_menu
                .as_ref()
                .is_some_and(|m| m.title == "Category agent")
        {
            self.coordinator_render();
        }
    }
    pub(super) fn coordinator_tick(&mut self) {
        if !self.coordinator_badges_pending
            && self.coordinator_badges_at.elapsed() > Duration::from_secs(2)
        {
            self.coordinator_badges_pending = true;
            self.coordinator_badges_at = Instant::now();
            let config = self.config.clone();
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let badges = crate::category_agent_runtime::badges(&config).unwrap_or_default();
                let _ = tx.send(Event::CategoryAgentBadges(badges));
            });
        }

        if self.modal.is_none()
            && self
                .context_menu
                .as_ref()
                .is_some_and(|m| m.title == "Category agent")
            && self.coordinator.as_ref().is_some_and(|u| {
                u.pending.is_none() && u.updated.elapsed() > Duration::from_millis(600)
            })
        {
            if let Some(ui) = &mut self.coordinator {
                ui.cursor = self
                    .context_menu
                    .as_ref()
                    .and_then(|m| m.selected)
                    .unwrap_or(ui.cursor);
            }
            self.coordinator_request(|_, _| Ok(()));
        }
    }
    pub(super) fn coordinator_render(&mut self) {
        let Some(ui) = self.coordinator.as_ref() else {
            return;
        };
        let width = if self.cells {
            usize::from(self.viewport.columns.saturating_sub(12)).clamp(12, 86)
        } else {
            86
        };
        let state = &ui.data["state"];
        let rt = &ui.data["runtime"];
        let profile = &state["profile"];
        let name = self
            .category_catalog
            .all()
            .into_iter()
            .find(|g| g.id == ui.category)
            .map(|g| g.name)
            .or_else(|| state["label"].as_str().map(String::from))
            .unwrap_or_else(|| {
                if ui.category.is_empty() {
                    "Archived category agents".into()
                } else {
                    ui.category.clone()
                }
            });
        let enabled = profile["enabled"] == true;
        let archived = state["archived"] == true;
        let mut items = vec![
            item("Overview", "category-agent:tab:overview"),
            item("Conversation", "category-agent:tab:chat"),
            item("Findings", "category-agent:tab:findings"),
            item("Proposed work / approvals", "category-agent:tab:proposals"),
        ];
        if ui.tab == "archives" {
            items.clear();
        }
        let history = rt["history"].as_array().cloned().unwrap_or_default();
        let active = history.first();
        let status = active
            .and_then(|r| r["status"].as_str())
            .unwrap_or("not connected");
        if let Some(error) = &ui.error {
            lines(width, &mut items, &format!("Error: {error}"));
        }
        if matches!(ui.tab.as_str(), "chat" | "findings" | "proposals") {
            if ui.offset > 0 {
                items.push(item("Newer reviews", "category-agent:newer"));
            }
            if ui.data["runtime"]["total"].as_u64().unwrap_or(0) > ui.offset as u64 + 6 {
                items.push(item("Older reviews", "category-agent:older"));
            }
        }
        match ui.tab.as_str() {
            "setup" => {
                items.push(item(
                    format!(
                        "Provider: {} · choose…",
                        profile["provider"].as_str().unwrap_or("codex")
                    ),
                    "category-agent:tab:providers",
                ));
                items.push(item(
                    format!(
                        "Authentication: {} · choose…",
                        profile["authentication"].as_str().unwrap_or("subscription")
                    ),
                    "category-agent:tab:authentication",
                ));
                items.push(item(
                    "Edit executable and category charter…",
                    "category-agent:settings",
                ));
                items.push(item(
                    if enabled {
                        "Disable category agent"
                    } else {
                        "Enable category agent for this category"
                    },
                    if enabled {
                        "category-agent:disable"
                    } else {
                        "category-agent:enable"
                    },
                ));
                lines(
                    width,
                    &mut items,
                    "Enabling is opt in. Connect discovers available models; reviews run only when requested.",
                );
                if enabled {
                    items.push(item(
                        "Connect / refresh available models",
                        "category-agent:connect",
                    ));
                }
            }
            "providers" => {
                for (id, label) in [
                    ("codex", "Codex"),
                    ("claude", "Claude Code"),
                    ("custom", "Custom adapter"),
                ] {
                    items.push(item(
                        format!(
                            "{}{}",
                            if profile["provider"] == id {
                                "✓ "
                            } else {
                                ""
                            },
                            label
                        ),
                        format!("category-agent:provider:{id}"),
                    ));
                }
                lines(
                    width,
                    &mut items,
                    "Changing providers disables coordination until you explicitly enable it again. Existing conversation history is retained.",
                );
            }
            "authentication" => {
                let choices: &[(&str, &str)] = if profile["provider"] == "custom" {
                    &[("custom", "Custom adapter authentication")]
                } else {
                    &[
                        ("subscription", "Existing CLI subscription login"),
                        ("api", "API credentials (API billing)"),
                    ]
                };
                for (id, label) in choices {
                    items.push(item(
                        format!(
                            "{}{}",
                            if profile["authentication"] == *id {
                                "✓ "
                            } else {
                                ""
                            },
                            label
                        ),
                        format!("category-agent:authentication:{id}"),
                    ));
                }
            }
            "archives" => {
                for c in ui.data["archived_categories"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    items.push(item(
                        c["label"]
                            .as_str()
                            .or(c["id"].as_str())
                            .unwrap_or("Archived category"),
                        format!(
                            "category-agent:archive-open:{}",
                            c["id"].as_str().unwrap_or("")
                        ),
                    ));
                }
            }
            "overview" => {
                if archived {
                    lines(
                        width,
                        &mut items,
                        "Archived category agent. Conversation and assessment history remain available. Coordination and repository access have been revoked.",
                    );
                } else {
                    items.push(item(
                        if enabled {
                            "Configure provider, authentication and charter…"
                        } else {
                            "Enable category agent…"
                        },
                        "category-agent:setup",
                    ));
                }
                if enabled {
                    items.push(item(
                        "Connect / refresh available models",
                        "category-agent:connect",
                    ));
                    items.push(item(
                        format!(
                            "Model: {} · choose…",
                            profile["model"].as_str().unwrap_or("not selected")
                        ),
                        "category-agent:tab:models",
                    ));
                    items.push(item(
                        format!(
                            "Effort: {} · choose…",
                            profile["effort"].as_str().unwrap_or("provider default")
                        ),
                        "category-agent:tab:effort",
                    ));
                    items.push(item("Review category now", "category-agent:review"));
                    items.push(item("Message category agent…", "category-agent:compose"));
                    items.push(item(
                        "Use STAR/FLEET alignment charter",
                        "category-agent:fleet",
                    ));
                    items.push(item("Cancel active review", "category-agent:cancel"));
                    items.push(item(
                        "Start a new category conversation / retain history",
                        "category-agent:new-conversation",
                    ));
                    items.push(item(
                        "Resume approved task queue",
                        "category-agent:resume-queue",
                    ));
                    items.push(item(
                        "Disable category agent / cancel future work",
                        "category-agent:disable",
                    ));
                }
                lines(
                    width,
                    &mut items,
                    &format!(
                        "Provider: {} · authentication: {}",
                        profile["provider"].as_str().unwrap_or("codex"),
                        profile["authentication"].as_str().unwrap_or("subscription")
                    ),
                );
                if let Some(plan) = rt["capabilities"]["plan"].as_str() {
                    lines(width, &mut items, &format!("Account plan: {plan}"));
                }
                lines(
                    width,
                    &mut items,
                    "Reviews run only when requested. Project agents and editable drafts are retained. Runtime status below is an observation; findings are AI assessments.",
                );
                for p in &self.category_projects {
                    if self.category_catalog.category(&p.workspace) == ui.category {
                        let a = self.project_activity(p);
                        items.push(item(
                            format!(
                                "{} · {} working · {} attention · {} unknown",
                                p.label.as_deref().unwrap_or(&p.workspace),
                                a.working,
                                a.waiting + a.failed,
                                a.unknown
                            ),
                            format!("categories:reveal:{}", p.workspace),
                        ));
                    }
                }
                for r in history.iter().take(1) {
                    for p in r["result"]["projects"].as_array().into_iter().flatten() {
                        lines(
                            width,
                            &mut items,
                            &format!(
                                "AI assessment · {} · {} · progress {} · confidence {}",
                                p["project"].as_str().unwrap_or(""),
                                p["status"].as_str().unwrap_or("unknown"),
                                p["progress"],
                                p["confidence"].as_str().unwrap_or("unknown")
                            ),
                        );
                        lines(width, &mut items, p["reason"].as_str().unwrap_or(""));
                    }
                }
                if let Some(r) = active
                    && let Some(error) = r["error"].as_str()
                {
                    lines(width, &mut items, error);
                }
                lines(width, &mut items, profile["charter"].as_str().unwrap_or(""));
            }
            "models" => {
                items.push(item(
                    "Enter an exact model ID…",
                    "category-agent:model-entry",
                ));
                if let Some(models) = rt["capabilities"]["models"].as_array() {
                    for (i, m) in models.iter().enumerate() {
                        items.push(item(
                            format!(
                                "{}{}",
                                if m["model"] == profile["model"]
                                    || m["id"].as_str().is_some_and(|id| profile["model"] == id)
                                {
                                    "✓ "
                                } else {
                                    ""
                                },
                                m["displayName"]
                                    .as_str()
                                    .or(m["model"].as_str())
                                    .or(m["id"].as_str())
                                    .unwrap_or("Model")
                            ),
                            format!("category-agent:model:{i}"),
                        ));
                    }
                }
                if rt["capabilities"].is_null() {
                    lines(
                        width,
                        &mut items,
                        "Enable and connect the provider first. Connecting discovers account/model metadata; it does not start a review.",
                    );
                }
            }
            "effort" => {
                items.push(item(
                    "Use this model's provider default",
                    "category-agent:effort:default",
                ));
                if let Some(models) = rt["capabilities"]["models"].as_array()
                    && let Some(model) = models
                        .iter()
                        .find(|m| m["model"] == profile["model"] || m["id"] == profile["model"])
                    && let Some(efforts) = model["supportedReasoningEfforts"].as_array()
                {
                    for (i, e) in efforts.iter().enumerate() {
                        items.push(item(
                            format!(
                                "{}{}",
                                if e["reasoningEffort"] == profile["effort"] {
                                    "✓ "
                                } else {
                                    ""
                                },
                                e["reasoningEffort"].as_str().unwrap_or("default")
                            ),
                            format!("category-agent:effort:{i}"),
                        ));
                    }
                }
                if profile["provider"] != "codex" {
                    items.push(item(
                        "Enter provider effort…",
                        "category-agent:effort-entry",
                    ));
                }
            }
            "chat" => {
                if enabled {
                    items.push(item("Message category agent…", "category-agent:compose"));
                    items.push(item("Review category now", "category-agent:review"));
                    items.push(item("Cancel active review", "category-agent:cancel"));
                    items.push(item(
                        "Start a new category conversation / retain history",
                        "category-agent:new-conversation",
                    ));
                }
                for r in history.iter().filter(|r| r["mode"] != "connect").rev() {
                    lines(
                        width,
                        &mut items,
                        &format!(
                            "You: {}",
                            r["prompt"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .unwrap_or("Review the category")
                        ),
                    );
                    lines(
                        width,
                        &mut items,
                        &format!(
                            "Agent · {}{}",
                            r["status"].as_str().unwrap_or("unknown"),
                            if r["stale"] == true {
                                " · stale assessment"
                            } else {
                                ""
                            }
                        ),
                    );
                    lines(
                        width,
                        &mut items,
                        r["result"]["summary"]
                            .as_str()
                            .or(r["text"].as_str())
                            .unwrap_or("Waiting for provider…"),
                    );
                    if let Some(error) = r["error"].as_str() {
                        lines(width, &mut items, error);
                    }
                }
                if history.iter().all(|r| r["mode"] == "connect") {
                    lines(
                        width,
                        &mut items,
                        "This is a dedicated category conversation. Start a review or send a message; no existing project conversation is reused.",
                    );
                }
            }
            "findings" => {
                lines(
                    width,
                    &mut items,
                    "AI assessments require source verification. Runtime working/attention indicators remain separate.",
                );
                for r in &history {
                    for f in r["result"]["findings"].as_array().into_iter().flatten() {
                        lines(
                            width,
                            &mut items,
                            &format!(
                                "{} · {} · {}{}",
                                f["project"].as_str().unwrap_or(""),
                                f["severity"].as_str().unwrap_or("info"),
                                f["title"].as_str().unwrap_or("Finding"),
                                if r["stale"] == true { " · STALE" } else { "" }
                            ),
                        );
                        lines(width, &mut items, f["detail"].as_str().unwrap_or(""));
                        for e in f["evidence"].as_array().into_iter().flatten() {
                            lines(width, &mut items, e.as_str().unwrap_or(""));
                        }
                    }
                }
            }
            "proposals" => {
                for r in &history {
                    for p in r["result"]["proposals"].as_array().into_iter().flatten() {
                        items.push(item(
                            format!(
                                "{} · {} · {}{}",
                                p["status"].as_str().unwrap_or("proposed"),
                                p["project"].as_str().unwrap_or(""),
                                p["title"].as_str().unwrap_or("Task"),
                                if r["stale"] == true { " · STALE" } else { "" }
                            ),
                            format!(
                                "category-agent:proposal:{}:{}",
                                r["id"].as_str().unwrap_or(""),
                                p["id"].as_str().unwrap_or("")
                            ),
                        ));
                    }
                }
                lines(
                    width,
                    &mut items,
                    "Nothing is sent until you approve an exact task and a current project conversation. Busy Codex agents wait in the queue; native delivery leaves terminal drafts untouched. Other harnesses retain an approved manual task.",
                );
            }
            "task" | "targets" => {
                if let Some((review, proposal)) = &ui.proposal
                    && let Some(p) = history
                        .iter()
                        .find(|r| r["id"] == *review)
                        .and_then(|r| r["result"]["proposals"].as_array())
                        .and_then(|ps| ps.iter().find(|p| p["id"] == *proposal))
                {
                    lines(
                        width,
                        &mut items,
                        &format!(
                            "{} · {}",
                            p["project"].as_str().unwrap_or(""),
                            p["status"].as_str().unwrap_or("")
                        ),
                    );
                    if ui.tab == "targets" {
                        for (i, t) in ui.targets["targets"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .enumerate()
                        {
                            items.push(item(
                                format!(
                                    "Pane {} · {} · {} · {}",
                                    t["pane"],
                                    t["provider"].as_str().unwrap_or("unknown"),
                                    t["conversation"]
                                        .as_str()
                                        .unwrap_or("no conversation identity"),
                                    t["state"]
                                ),
                                format!("category-agent:target:{i}"),
                            ));
                        }
                    }
                    if p["depends_on"].as_array().is_some_and(|d| !d.is_empty()) {
                        lines(
                            width,
                            &mut items,
                            &format!(
                                "Dependencies (await user-verified completion): {}",
                                p["depends_on"]
                            ),
                        );
                    }
                    if !archived && p["status"] == "proposed" {
                        items.push(item(
                            "Choose the project agent conversation…",
                            "category-agent:targets",
                        ));
                        if let Some(target) = &ui.target {
                            lines(
                                width,
                                &mut items,
                                &format!(
                                    "Selected conversation: {}",
                                    target["conversation"].as_str().unwrap_or("")
                                ),
                            );
                            items.push(item(
                                "Approve this exact task for this conversation…",
                                "category-agent:approve",
                            ));
                        }
                    }
                    if !archived
                        && matches!(
                            p["status"].as_str(),
                            Some(
                                "proposed"
                                    | "queued"
                                    | "approved_manual"
                                    | "needs_review"
                                    | "needs_reapproval"
                            )
                        )
                    {
                        items.push(item("Reject / cancel this task", "category-agent:reject"));
                    }
                    if matches!(p["status"].as_str(), Some("delivered" | "approved_manual")) {
                        items.push(item(
                            "Mark complete after verifying acceptance…",
                            "category-agent:verify",
                        ));
                    }
                    items.push(item("Copy exact task to clipboard", "category-agent:copy"));
                    lines(width, &mut items, p["text"].as_str().unwrap_or(""));
                    if let Some(error) = p["error"].as_str() {
                        lines(width, &mut items, error);
                    }
                }
            }
            _ => {}
        }
        items.push(item("Back to category projects", "category-agent:close"));
        let cursor = ui.cursor.min(items.len().saturating_sub(1));
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [12, 12],
            title: "Category agent".into(),
            subtitle: format!(
                "{name} · {} · {status}",
                if archived {
                    "archived / read only"
                } else if enabled {
                    "enabled"
                } else {
                    "disabled / opt in"
                }
            ),
            items,
            selected: Some(cursor),
        });
    }
    pub(super) fn coordinator_action(&mut self, action: &str) -> Result<bool> {
        if let Some(index) = action.strip_prefix("menu:item:")
            && self
                .context_menu
                .as_ref()
                .is_some_and(|m| m.title == "Category agent")
            && let Some(ui) = &mut self.coordinator
        {
            ui.cursor = index.parse().unwrap_or(0);
        }
        let Some(command) = action.strip_prefix("category-agent:") else {
            return Ok(false);
        };
        if command == "archives" {
            self.release();
            let token = Instant::now();
            self.coordinator = Some(Coordinator {
                category: String::new(),
                tab: "archives".into(),
                data: json!({}),
                error: None,
                pending: Some(token),
                updated: Instant::now(),
                cursor: 0,
                offset: 0,
                proposal: None,
                targets: json!({}),
                target: None,
            });
            self.coordinator_render();
            let config = self.config.clone();
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let result = crate::category_agent::archives(&config).map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::CategoryAgent(token, String::new(), result));
            });
            return Ok(true);
        }
        if let Some(category) = command
            .strip_prefix("open:")
            .or_else(|| command.strip_prefix("archive-open:"))
        {
            ensure!(
                command.starts_with("archive-open:")
                    || self.category_catalog.all().iter().any(|g| g.id == category),
                "Category is unavailable"
            );
            self.release();
            self.coordinator = Some(Coordinator {
                category: category.into(),
                tab: "overview".into(),
                data: if command.starts_with("archive-open:") {
                    json!({"state":{"archived":true}})
                } else {
                    json!({})
                },
                error: None,
                pending: None,
                updated: Instant::now(),
                cursor: 0,
                offset: 0,
                proposal: None,
                targets: json!({}),
                target: None,
            });
            self.coordinator_render();
            self.coordinator_request(|_, _| Ok(()));
            return Ok(true);
        }
        if command != "noop"
            && let Some(ui) = &mut self.coordinator
        {
            ui.error = None;
        }
        let ui = self
            .coordinator
            .as_ref()
            .context("Open a category agent first")?
            .clone();
        let profile: crate::category_agent::Profile =
            serde_json::from_value(ui.data["state"]["profile"].clone()).unwrap_or_default();
        let revision = ui.data["state"]["revision"].as_u64().unwrap_or(0);
        ensure!(
            ui.data["state"]["archived"] != true
                || matches!(command, "noop" | "close" | "copy" | "older" | "newer")
                || command.starts_with("tab:")
                || command.starts_with("proposal:"),
            "Archived category agents are read only"
        );
        match command {
            "noop" => self.coordinator_render(),
            "older" | "newer" => {
                if let Some(u) = &mut self.coordinator {
                    u.offset = if command == "older" {
                        u.offset + 6
                    } else {
                        u.offset.saturating_sub(6)
                    };
                    u.cursor = 0;
                }
                self.coordinator_render();
                self.coordinator_request(|_, _| Ok(()));
            }
            "close" => {
                self.coordinator = None;
                self.context_menu = None;
                if ui.category.is_empty() {
                    self.category_action("categories:global")?;
                } else if ui.data["state"]["archived"] == true {
                    self.coordinator_action("category-agent:archives")?;
                } else {
                    self.category_action(&format!("categories:details:{}", ui.category))?;
                }
            }
            "setup" => {
                if let Some(u) = &mut self.coordinator {
                    u.tab = "setup".into();
                    u.cursor = 0;
                }
                self.coordinator_render();
            }
            "settings" => {
                self.open_prompt(Prompt::CategoryAgentSetup(ui.category, revision, profile))
            }
            "compose" => self.open_prompt(Prompt::CategoryAgentMessage(ui.category)),
            "model-entry" => self.open_prompt(Prompt::CategoryAgentSetting(
                ui.category,
                revision,
                profile,
                false,
            )),
            "effort-entry" => self.open_prompt(Prompt::CategoryAgentSetting(
                ui.category,
                revision,
                profile,
                true,
            )),
            "connect" | "review" => {
                let mode = command.to_owned();
                self.coordinator_render();
                self.coordinator_request(move |config, category| {
                    crate::category_agent_runtime::start(config, category, &mode, "").map(|_| ())
                });
            }
            "new-conversation" => {
                self.coordinator_render();
                self.coordinator_request(|c, g| {
                    crate::category_agent_runtime::reset_conversation(c, g).map(|_| ())
                });
            }
            "cancel" => {
                self.coordinator_render();
                self.coordinator_request(|c, g| {
                    crate::category_agent_runtime::cancel(c, g).map(|_| ())
                });
            }
            "resume-queue" => {
                self.coordinator_render();
                self.coordinator_request(|c, g| {
                    crate::category_agent_runtime::resume_queue(c, g).map(|_| ())
                });
            }
            "enable" | "disable" | "fleet" => {
                let mut profile = profile;
                if command == "enable" {
                    profile.enabled = true;
                } else if command == "disable" {
                    profile.enabled = false;
                } else {
                    profile.charter = FLEET.into();
                }
                self.coordinator_render();
                self.coordinator_request(move |c, g| {
                    crate::category_agent::configure(c, g, revision, profile).map(|_| ())
                });
            }
            "targets" => {
                let (review, proposal) = ui.proposal.context("Choose a proposal")?;
                let p = ui.data["runtime"]["history"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|r| r["id"] == review)
                    .and_then(|r| r["result"]["proposals"].as_array())
                    .and_then(|ps| ps.iter().find(|p| p["id"] == proposal))
                    .context("Proposal disappeared")?;
                let project = p["project"].as_str().context("Missing project")?.to_owned();
                // Agent identity inspection is finite and occurs outside the rendering thread.
                let config = self.config.clone();
                let category = ui.category.clone();
                let tx = self.tx.clone();
                let token = Instant::now();
                if let Some(u) = &mut self.coordinator {
                    u.tab = "targets".into();
                    u.pending = Some(token);
                    u.cursor = 0;
                }
                self.coordinator_render();
                std::thread::spawn(move || {
                    let result =
                        crate::category_agent_runtime::targets(&config, &category, &project)
                            .map(|targets| json!({"target_choices":targets}))
                            .map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Event::CategoryAgent(token, category, result));
                });
            }
            "approve" | "verify" => {
                let (review, proposal) = ui.proposal.context("Choose a proposal")?;
                if command == "approve" {
                    ensure!(
                        ui.target.is_some(),
                        "Choose a current project conversation first"
                    );
                }
                self.open_prompt(Prompt::CategoryAgentApprove(
                    ui.category,
                    review,
                    proposal,
                    if command == "approve" {
                        ui.target
                    } else {
                        None
                    },
                ));
            }
            "reject" => {
                let (r, p) = ui.proposal.context("Choose a proposal")?;
                self.coordinator_render();
                self.coordinator_request(move |c, g| {
                    crate::category_agent_runtime::proposal_update(c, g, &r, &p, "reject")
                        .map(|_| ())
                });
            }
            "copy" => {
                let (r, p) = ui.proposal.context("Choose a proposal")?;
                let text = ui.data["runtime"]["history"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|v| v["id"] == r)
                    .and_then(|r| r["result"]["proposals"].as_array())
                    .and_then(|ps| ps.iter().find(|v| v["id"] == p))
                    .and_then(|p| p["text"].as_str())
                    .context("Task text is unavailable")?;
                self.effects
                    .push(ServerMessage::Clipboard { text: text.into() });
                self.coordinator_render();
            }
            _ if command.starts_with("tab:") => {
                let tab = command.trim_start_matches("tab:");
                ensure!(
                    [
                        "overview",
                        "setup",
                        "providers",
                        "authentication",
                        "chat",
                        "findings",
                        "proposals",
                        "models",
                        "effort"
                    ]
                    .contains(&tab),
                    "Unknown category page"
                );
                if let Some(ui) = &mut self.coordinator {
                    ui.tab = tab.into();
                    ui.cursor = 0;
                }
                self.coordinator_render();
            }
            _ if command.starts_with("proposal:") => {
                let (r, p) = command
                    .trim_start_matches("proposal:")
                    .split_once(':')
                    .context("Invalid proposal")?;
                if let Some(ui) = &mut self.coordinator {
                    ui.proposal = Some((r.into(), p.into()));
                    ui.target = None;
                    ui.tab = "task".into();
                    ui.cursor = 0;
                }
                self.coordinator_render();
            }
            _ if command.starts_with("target:") => {
                let index = command.trim_start_matches("target:").parse::<usize>()?;
                let target = ui.targets["targets"]
                    .as_array()
                    .and_then(|v| v.get(index))
                    .context("Agent list changed")?
                    .clone();
                ensure!(
                    target["conversation"].is_string(),
                    "This agent has not reported a conversation identity"
                );
                if let Some(ui) = &mut self.coordinator {
                    ui.target = Some(target);
                    ui.tab = "task".into();
                    ui.cursor = 0;
                }
                self.coordinator_render();
            }
            _ if command.starts_with("provider:") || command.starts_with("authentication:") => {
                let mut p = profile;
                if let Some(provider) = command.strip_prefix("provider:") {
                    let provider = match provider {
                        "codex" => crate::category_agent::Provider::Codex,
                        "claude" => crate::category_agent::Provider::Claude,
                        "custom" => crate::category_agent::Provider::Custom,
                        _ => anyhow::bail!("Unknown provider"),
                    };
                    if p.provider != provider {
                        p.enabled = false;
                        p.authentication = if provider == crate::category_agent::Provider::Custom {
                            "custom"
                        } else {
                            "subscription"
                        }
                        .into();
                        p.provider = provider;
                        p.executable = None;
                        p.model = None;
                        p.effort = None;
                    }
                } else {
                    p.authentication = command.trim_start_matches("authentication:").into();
                }
                if let Some(u) = &mut self.coordinator {
                    u.tab = "setup".into();
                    u.cursor = 0;
                }
                self.coordinator_render();
                self.coordinator_request(move |c, g| {
                    crate::category_agent::configure(c, g, revision, p).map(|_| ())
                });
            }
            _ if command.starts_with("model:") => {
                let index = command.trim_start_matches("model:").parse::<usize>()?;
                let model = ui.data["runtime"]["capabilities"]["models"]
                    .as_array()
                    .and_then(|v| v.get(index))
                    .context("Model list changed")?;
                let mut p = profile;
                p.model = model["model"]
                    .as_str()
                    .or(model["id"].as_str())
                    .map(String::from);
                p.effort = None;
                if let Some(ui) = &mut self.coordinator {
                    ui.tab = "overview".into();
                    ui.cursor = 0;
                }
                self.coordinator_render();
                self.coordinator_request(move |c, g| {
                    crate::category_agent::configure(c, g, revision, p).map(|_| ())
                });
            }
            _ if command.starts_with("effort:") => {
                let mut p = profile;
                let choice = command.trim_start_matches("effort:");
                p.effort = if choice == "default" {
                    None
                } else {
                    let index = choice.parse::<usize>()?;
                    ui.data["runtime"]["capabilities"]["models"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|m| {
                            m["model"] == ui.data["state"]["profile"]["model"]
                                || m["id"] == ui.data["state"]["profile"]["model"]
                        })
                        .and_then(|m| m["supportedReasoningEfforts"].as_array())
                        .and_then(|v| v.get(index))
                        .and_then(|v| v["reasoningEffort"].as_str())
                        .map(String::from)
                };
                if let Some(ui) = &mut self.coordinator {
                    ui.tab = "overview".into();
                    ui.cursor = 0;
                }
                self.coordinator_render();
                self.coordinator_request(move |c, g| {
                    crate::category_agent::configure(c, g, revision, p).map(|_| ())
                });
            }
            _ => anyhow::bail!("Unknown category agent action"),
        }
        Ok(true)
    }
}
