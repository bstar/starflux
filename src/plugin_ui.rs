use super::*;
fn item(label: impl Into<String>, action: impl Into<String>) -> crate::shelf::MenuItem {
    crate::shelf::MenuItem {
        label: label.into(),
        hint: String::new(),
        action: action.into(),
    }
}
fn display_lines(text: &str, width: usize) -> Vec<String> {
    let mut result = Vec::new();
    for source in text.split('\n') {
        let mut line = String::new();
        let mut columns = 0;
        let mut append = |c: char| {
            let mut bytes = [0; 4];
            let cells = if c.is_ascii() {
                1
            } else {
                starkit::ratatui::text::Line::from(c.encode_utf8(&mut bytes) as &str).width()
            };
            if columns + cells > width.max(1) && !line.is_empty() {
                result.push(std::mem::take(&mut line));
                columns = 0;
            }
            line.push(c);
            columns += cells;
        };
        for c in source.chars() {
            match c {
                '\t' => {
                    for _ in 0..4 {
                        append(' ');
                    }
                }
                c if (c as u32) < 32 => append(char::from_u32(0x2400 + c as u32).unwrap()),
                '\u{7f}' => append('␡'),
                c if c.is_control() => append('�'),
                c => append(c),
            }
        }
        result.push(if line.is_empty() { " ".into() } else { line });
    }
    result
}
fn detail(items: &mut Vec<crate::shelf::MenuItem>, text: &str, width: usize) {
    items.extend(
        display_lines(text, width)
            .into_iter()
            .map(|line| item(line, "menu:panel")),
    );
}
impl Workspace {
    pub(super) fn plugin_request(
        &mut self,
        target: String,
        work: impl FnOnce() -> Result<Value> + Send + 'static,
    ) {
        self.release();
        self.plugin_generation = self.plugin_generation.wrapping_add(1);
        self.plugin_view = target.clone();
        let token = Instant::now();
        self.plugin_token = Some(token);
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [16, 16],
            title: "Plugins".into(),
            subtitle: "Loading…".into(),
            items: vec![item("Loading workflows…", "menu:panel")],
            selected: Some(0),
        });
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Value> {
                let output = work()?;
                let registry = crate::plugins::list()?;
                let jobs = if let Some(id) = target.strip_prefix("plugin:") {
                    crate::plugin_runner::list(Some(id))?
                } else if let Some(id) = target.strip_prefix("job:") {
                    crate::plugin_runner::get(id.split(':').next().unwrap_or(""), true)?
                } else if let Some(id) = target.strip_prefix("panes:") {
                    crate::plugin_panes::list(Some(id))?
                } else if let Some(id) = target.strip_prefix("versions:") {
                    crate::plugin_packages::versions(id)?
                } else {
                    output
                };
                Ok(json!({"registry":registry,"jobs":jobs}))
            })();
            let _ = tx.send(Event::Plugins(
                token,
                target,
                result.map_err(|e| format!("{e:#}")),
            ));
        });
    }
    pub(super) fn plugin_loading(&mut self, label: &str) {
        if let Some(menu) = &mut self.context_menu
            && menu.title == "Plugins"
        {
            menu.subtitle = "Escape hides this view; the operation continues".into();
            menu.items = vec![item(label, "menu:panel")];
        }
    }
    pub(super) fn plugin_result(
        &mut self,
        token: Instant,
        target: &str,
        result: std::result::Result<Value, String>,
    ) {
        if self.plugin_token != Some(token) {
            return;
        }
        self.plugin_token = None;
        if self
            .context_menu
            .as_ref()
            .is_none_or(|m| m.title != "Plugins")
        {
            return;
        }
        let width = if self.cells {
            usize::from(self.viewport.columns.saturating_sub(8)).clamp(8, 80)
        } else {
            ((self.viewport.width.saturating_sub(100) / 8).min(80) as usize).max(8)
        };
        let mut items = Vec::new();
        let mut subtitle = "Trusted commands · execute on this controller".to_owned();
        match result {
            Err(error) => {
                detail(&mut items, &error, width);
                items.push(item("Reload plugins", "plugins:list"));
            }
            Ok(data) => {
                self.plugin_data = data;
                let registry = &self.plugin_data["registry"];
                if target == "list" {
                    items.push(item(
                        "Links in the visible terminal…",
                        "terminal-links:list",
                    ));
                    items.push(item("Open link with a plugin…", "plugins:link-url"));
                    items.push(item(
                        "Install from GitHub / Git repository…",
                        "plugins:install",
                    ));
                    items.push(item("Link local plugin directory…", "plugins:link"));
                    for p in registry["plugins"].as_array().into_iter().flatten() {
                        if let Some(id) = p["id"].as_str() {
                            items.push(item(
                                format!(
                                    "{} · {}",
                                    p["manifest"]["name"].as_str().unwrap_or(id),
                                    if p["enabled"] == true {
                                        "enabled"
                                    } else {
                                        "disabled / review"
                                    }
                                ),
                                format!("plugins:plugin:{id}"),
                            ));
                        }
                    }
                } else if target == "links" {
                    let links = &self.plugin_data["jobs"];
                    subtitle = "Choose a handler / nothing runs until selected".into();
                    detail(&mut items, links["url"].as_str().unwrap_or(""), width);
                    for h in links["handlers"].as_array().into_iter().flatten() {
                        items.push(item(
                            format!(
                                "{} / {} · {}",
                                h["plugin"].as_str().unwrap_or(""),
                                h["handler"]["id"].as_str().unwrap_or(""),
                                h["handler"]["title"].as_str().unwrap_or("")
                            ),
                            format!(
                                "plugins:open-link:{}:{}",
                                h["plugin"].as_str().unwrap_or(""),
                                h["handler"]["id"].as_str().unwrap_or("")
                            ),
                        ));
                    }
                    if links["handlers"].as_array().is_some_and(Vec::is_empty) {
                        detail(&mut items, "No enabled plugin handles this URL.", width);
                    }
                    for warning in links["warnings"].as_array().into_iter().flatten() {
                        detail(
                            &mut items,
                            &format!(
                                "{}: {}",
                                warning["plugin"].as_str().unwrap_or(""),
                                warning["warning"].as_str().unwrap_or("")
                            ),
                            width,
                        );
                    }
                    let offset = links["offset"].as_u64().unwrap_or(0);
                    let limit = links["limit"].as_u64().unwrap_or(16);
                    if offset > 0 {
                        items.push(item(
                            "Previous handlers",
                            format!("plugins:links-page:{}", offset.saturating_sub(limit)),
                        ));
                    }
                    if let Some(next) = links["next_offset"].as_u64() {
                        items.push(item("Next handlers", format!("plugins:links-page:{next}")));
                    }
                    items.push(item("Choose another URL…", "plugins:link-url"));
                } else if let Some(id) = target.strip_prefix("plugin:") {
                    if let Some(p) = registry["plugins"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|p| p["id"] == id)
                    {
                        subtitle = format!(
                            "{} · commands run as your user",
                            p["path"].as_str().unwrap_or(id)
                        );
                        if let Some(warning) = p["warning"].as_str() {
                            detail(&mut items, warning, width);
                        }
                        detail(&mut items, p["path"].as_str().unwrap_or(id), width);
                        detail(
                            &mut items,
                            p["manifest"]["description"].as_str().unwrap_or(""),
                            width,
                        );
                        items.push(item(
                            "Plugin terminals / focus and close",
                            format!("plugins:panes:{id}"),
                        ));
                        for a in p["manifest"]["actions"].as_array().into_iter().flatten() {
                            let action = a["id"].as_str().unwrap_or("");
                            items.push(item(
                                format!(
                                    "{} · cwd {} · {}ms",
                                    a["title"].as_str().unwrap_or(action),
                                    a["cwd"].as_str().unwrap_or("package"),
                                    a["timeout_ms"]
                                ),
                                if p["enabled"] == true {
                                    format!("plugins:run:{id}:{action}")
                                } else {
                                    "menu:panel".into()
                                },
                            ));
                            detail(&mut items, &a["argv"].to_string(), width);
                        }
                        for pane in p["manifest"]["panes"].as_array().into_iter().flatten() {
                            let entrypoint = pane["id"].as_str().unwrap_or("");
                            items.push(item(
                                format!(
                                    "Open {} · {} · cwd {}",
                                    pane["title"].as_str().unwrap_or(entrypoint),
                                    pane["placement"].as_str().unwrap_or("overlay"),
                                    pane["cwd"].as_str().unwrap_or("package")
                                ),
                                if p["enabled"] == true {
                                    format!("plugins:open-pane:{id}:{entrypoint}")
                                } else {
                                    "menu:panel".into()
                                },
                            ));
                            detail(&mut items, &pane["argv"].to_string(), width);
                        }
                        for hook in p["manifest"]["hooks"].as_array().into_iter().flatten() {
                            detail(
                                &mut items,
                                &format!(
                                    "Hook {} → {} · states {}",
                                    hook["on"], hook["action"], hook["states"]
                                ),
                                width,
                            );
                        }
                        for handler in p["manifest"]["link_handlers"]
                            .as_array()
                            .into_iter()
                            .flatten()
                        {
                            detail(
                                &mut items,
                                &format!(
                                    "Link handler {} · {} → {} · schemes {} · exact hosts {}",
                                    handler["id"],
                                    handler["title"],
                                    handler["action"],
                                    handler["schemes"],
                                    handler["hosts"]
                                ),
                                width,
                            );
                        }
                        if p["enabled"] != true && p["configured_enabled"] == true {
                            items.push(item(
                                "Disable future actions / hooks",
                                format!("plugins:disable:{id}"),
                            ));
                        }
                        if p["digest"].is_string() {
                            items.push(item(
                                if p["enabled"] == true {
                                    "Disable future actions / hooks"
                                } else {
                                    "Trust these commands and enable"
                                },
                                format!("plugins:toggle:{id}"),
                            ));
                        }
                        if p["managed"].is_object() {
                            detail(
                                &mut items,
                                &format!(
                                    "Repository: {} · version: {} · commit: {}",
                                    p["managed"]["repository"].as_str().unwrap_or(""),
                                    p["managed"]["reference"].as_str().unwrap_or(""),
                                    p["managed"]["commit"].as_str().unwrap_or("")
                                ),
                                width,
                            );
                            items.push(item(
                                "Fetch an updated / different version…",
                                format!("plugins:update:{id}"),
                            ));
                            items.push(item(
                                "Installed versions / roll back",
                                format!("plugins:versions:{id}"),
                            ));
                            items.push(item(
                                "Uninstall managed files / keep logs and settings…",
                                format!("plugins:uninstall:{id}"),
                            ));
                        } else {
                            items.push(item(
                                "Unlink / keep package and logs",
                                format!("plugins:unlink:{id}"),
                            ));
                        }
                        for j in self.plugin_data["jobs"]["jobs"]
                            .as_array()
                            .into_iter()
                            .flatten()
                        {
                            if let Some(job) = j["id"].as_str() {
                                items.push(item(
                                    format!(
                                        "{} · {}",
                                        j["action"]["title"].as_str().unwrap_or("Action"),
                                        j["status"].as_str().unwrap_or("unknown")
                                    ),
                                    format!("plugins:job:{job}:0"),
                                ));
                            }
                        }
                        items.push(item(
                            "Refresh actions and jobs",
                            format!("plugins:plugin:{id}"),
                        ));
                    }
                } else if let Some(id) = target.strip_prefix("panes:") {
                    subtitle = "Persistent plugin terminals in this project".into();
                    for pane in self.plugin_data["jobs"]["panes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        let r = &pane["record"];
                        if r["project"]["id"] != self.config.workspace {
                            continue;
                        }
                        let record_id = r["id"].as_str().unwrap_or("");
                        detail(
                            &mut items,
                            &format!(
                                "{} · {}",
                                r["entrypoint"]["title"].as_str().unwrap_or("Terminal"),
                                pane["status"].as_str().unwrap_or("unknown")
                            ),
                            width,
                        );
                        if pane["alive"] == true {
                            items.push(item(
                                "Focus terminal",
                                format!("plugins:focus-pane:{record_id}"),
                            ));
                            items.push(item(
                                "Terminate plugin terminal…",
                                format!("plugins:terminate-pane:{record_id}"),
                            ));
                        }
                    }
                    items.push(item("Refresh terminals", format!("plugins:panes:{id}")));
                    items.push(item(
                        "Back to plugin commands",
                        format!("plugins:plugin:{id}"),
                    ));
                } else if let Some(id) = target.strip_prefix("versions:") {
                    subtitle = "Retained Git versions · switching requires review/enable".into();
                    for version in self.plugin_data["jobs"]["versions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        let source = &version["source"];
                        items.push(item(
                            format!(
                                "{}{} · {}",
                                if version["current"] == true {
                                    "Current · "
                                } else {
                                    "Use · "
                                },
                                source["reference"].as_str().unwrap_or(""),
                                source["commit"]
                                    .as_str()
                                    .unwrap_or("")
                                    .chars()
                                    .take(12)
                                    .collect::<String>()
                            ),
                            format!(
                                "plugins:activate:{id}:{}",
                                source["release"].as_str().unwrap_or("")
                            ),
                        ));
                    }
                    items.push(item("Plugin actions", format!("plugins:plugin:{id}")));
                } else if let Some(spec) = target.strip_prefix("job:") {
                    let mut parts = spec.split(':');
                    let id = parts.next().unwrap_or("");
                    let offset = parts
                        .next()
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0);
                    let job = &self.plugin_data["jobs"];
                    subtitle = format!(
                        "Job {} · {}",
                        id,
                        job["status"].as_str().unwrap_or("unknown")
                    );
                    if let Some(error) = job["error"].as_str() {
                        detail(&mut items, error, width);
                    }
                    if matches!(job["status"].as_str(), Some("queued" | "running")) {
                        items.push(item("Cancel this job", format!("plugins:cancel:{id}")));
                    }
                    items.push(item("Refresh job", format!("plugins:job:{id}:{offset}")));
                    let output = format!(
                        "stdout:\n{}\nstderr:\n{}",
                        job["output"]["stdout"].as_str().unwrap_or(""),
                        job["output"]["stderr"].as_str().unwrap_or("")
                    );
                    let lines = display_lines(&output, width);
                    let offset = offset.min(lines.len().saturating_sub(1));
                    for line in lines.iter().skip(offset).take(40) {
                        items.push(item(line.clone(), "menu:panel"));
                    }
                    if offset > 0 {
                        items.push(item(
                            "Previous output page",
                            format!("plugins:job:{id}:{}", offset.saturating_sub(40)),
                        ));
                    }
                    if offset + 40 < lines.len() {
                        items.push(item(
                            "Next output page",
                            format!("plugins:job:{id}:{}", offset + 40),
                        ));
                    }
                    if let Some(plugin) = job["plugin"].as_str() {
                        items.push(item("Plugin actions", format!("plugins:plugin:{plugin}")));
                    }
                }
                if target != "list" {
                    items.push(item("All plugins", "plugins:list"));
                }
            }
        }
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [16, 16],
            title: "Plugins".into(),
            subtitle,
            items,
            selected: Some(0),
        });
    }
    pub(super) fn plugin_action(&mut self, action: &str) -> Result<bool> {
        let Some(action) = action.strip_prefix("plugins:") else {
            return Ok(false);
        };
        let revision = self.plugin_data["registry"]["revision"]
            .as_str()
            .unwrap_or("initial")
            .to_owned();
        if let Some(tail) = action.strip_prefix("open-pane:") {
            let (plugin, entrypoint) = tail.split_once(':').context("Invalid plugin pane")?;
            let reviewed = self.plugin_data["registry"]["plugins"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|p| p["id"] == plugin && p["enabled"] == true)
                .context("Review and enable the plugin first")?;
            let digest = reviewed["digest"]
                .as_str()
                .context("Missing plugin approval")?
                .to_owned();
            let pane: crate::plugin_panes::Entrypoint = serde_json::from_value(
                reviewed["manifest"]["panes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|p| p["id"] == entrypoint)
                    .context("Pane entrypoint disappeared")?
                    .clone(),
            )?;
            if pane.placement == crate::plugin_panes::Placement::Popup {
                if self
                    .context_menu
                    .as_ref()
                    .is_some_and(|m| m.title == "Plugins")
                {
                    self.context_menu = None;
                }
                self.open_popup(
                    plugin,
                    entrypoint,
                    &digest,
                    crate::plugin_popup::Options {
                        width: pane.width,
                        height: pane.height,
                    },
                )?;
                return Ok(true);
            }
            let plugin = plugin.to_owned();
            let entrypoint = entrypoint.to_owned();
            let project = self.config.workspace.clone();
            self.plugin_generation = self.plugin_generation.wrapping_add(1);
            let generation = self.plugin_generation;
            self.plugin_token = Some(Instant::now());
            self.release();
            self.context_menu = Some(crate::shelf::ContextMenu {
                position: [16, 16],
                title: "Plugins".into(),
                subtitle: "Escape leaves the new terminal available in its tab".into(),
                items: vec![item("Opening plugin terminal…", "menu:panel")],
                selected: Some(0),
            });
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let result =
                    crate::plugin_panes::open(&plugin, &entrypoint, &digest, &project, None)
                        .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::PluginPaneOpened(generation, Box::new(result)));
            });
            return Ok(true);
        }
        if let Some(id) = action
            .strip_prefix("focus-pane:")
            .or_else(|| action.strip_prefix("terminate-pane:"))
        {
            let record = crate::plugin_panes::get(id)?;
            ensure!(
                record.project.id == self.config.workspace,
                "Switch to this plugin terminal's project first"
            );
            self.sync_project()?;
            ensure!(
                self.state.terminal_epochs.get(&record.pane) == Some(&record.epoch),
                "Plugin terminal was replaced"
            );
            self.context_menu = None;
            if action.starts_with("terminate-pane:") {
                self.action(&format!("terminate-terminal:{}", record.pane))?;
            } else {
                self.state
                    .present_plugin_pane(record.pane, record.entrypoint.placement)?;
                self.release();
                self.layout();
                self.save();
            }
            return Ok(true);
        }
        if action == "install" {
            self.open_prompt(Prompt::InstallPlugin(revision));
            return Ok(true);
        }
        if action == "link" {
            self.open_prompt(Prompt::LinkPlugin(revision));
            return Ok(true);
        }
        if action == "link-url" {
            self.open_prompt(Prompt::PluginLink);
            return Ok(true);
        }
        if let Some(offset) = action.strip_prefix("links-page:") {
            let offset = offset.parse::<usize>()?;
            let links = &self.plugin_data["jobs"];
            let url = links["url"]
                .as_str()
                .context("Resolve a URL first")?
                .to_owned();
            let project = links["project"].clone();
            self.plugin_request("links".into(), move || {
                let mut result = crate::plugin_links::resolve(&url, offset, 16)?;
                result["project"] = project;
                Ok(result)
            });
            return Ok(true);
        }
        if let Some(tail) = action.strip_prefix("open-link:") {
            let (plugin, handler) = tail.split_once(':').context("Invalid link handler")?;
            let links = &self.plugin_data["jobs"];
            let chosen = links["handlers"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|h| h["plugin"] == plugin && h["handler"]["id"] == handler)
                .context("Resolve the URL again before choosing a handler")?;
            let digest = chosen["digest"]
                .as_str()
                .context("Missing handler digest")?
                .to_owned();
            let url = links["url"]
                .as_str()
                .context("Missing link URL")?
                .to_owned();
            let project: crate::plugins::Project =
                serde_json::from_value(links["project"].clone())?;
            let plugin = plugin.to_owned();
            let handler = handler.to_owned();
            self.plugin_request(format!("plugin:{plugin}"), move || {
                crate::plugin_runner::open_link(
                    &plugin,
                    &handler,
                    &digest,
                    &url,
                    Some(project),
                    &uuid::Uuid::new_v4().to_string(),
                )
            });
            return Ok(true);
        }
        if action == "list"
            || action.starts_with("plugin:")
            || action.starts_with("job:")
            || action.starts_with("versions:")
            || action.starts_with("panes:")
        {
            self.plugin_request(action.into(), || Ok(json!({})));
            return Ok(true);
        }
        if let Some(id) = action.strip_prefix("cancel:") {
            let id = id.to_owned();
            let target = format!("job:{id}:0");
            self.plugin_request(target, move || crate::plugin_runner::cancel(&id));
            return Ok(true);
        }
        let (kind, tail) = action.split_once(':').context("Unknown plugin action")?;
        let (id, command) = tail.split_once(':').unwrap_or((tail, ""));
        let p = self.plugin_data["registry"]["plugins"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["id"] == id)
            .context("Reload the plugin list")?;
        let enabled = p["enabled"] == true;
        let digest = p["digest"].as_str().unwrap_or("").to_owned();
        let id = id.to_owned();
        let target = format!("plugin:{id}");
        match kind {
            "update" => self.open_prompt(Prompt::UpdatePlugin(
                id,
                revision,
                p["managed"]["reference"].as_str().unwrap_or("HEAD").into(),
            )),
            "uninstall" => self.open_prompt(Prompt::UninstallPlugin(id, revision)),
            "activate" => {
                let release = command.to_owned();
                self.plugin_request(target, move || {
                    crate::plugin_packages::activate(&id, &release, &revision)
                });
            }
            "disable" => self.plugin_request(target, move || {
                crate::plugins::configure(&id, false, "", &revision)
            }),
            "toggle" => self.plugin_request(target, move || {
                crate::plugins::configure(&id, !enabled, &digest, &revision)
            }),
            "unlink" => self.plugin_request("list".into(), move || {
                crate::plugins::unlink(&id, &revision)
            }),
            "run" => {
                let command = command.to_owned();
                let project = crate::plugins::project(&self.config);
                self.plugin_request(target, move || {
                    crate::plugin_runner::enqueue(
                        &id,
                        &command,
                        &digest,
                        Some(project),
                        &uuid::Uuid::new_v4().to_string(),
                    )
                });
            }
            _ => anyhow::bail!("Unknown plugin action"),
        }
        Ok(true)
    }
}

impl Workspace {
    pub(super) fn plugin_pane_opened(
        &mut self,
        generation: u64,
        result: std::result::Result<crate::plugin_panes::Record, String>,
    ) {
        if self.plugin_generation == generation {
            self.plugin_token = None;
        }
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                self.note(format!("Plugin pane: {e}"));
                if self.plugin_generation == generation
                    && self
                        .context_menu
                        .as_ref()
                        .is_some_and(|m| m.title == "Plugins")
                {
                    self.context_menu = None;
                }
                return;
            }
        };
        if self.plugin_generation != generation
            || self.config.workspace != record.project.id
            || self
                .context_menu
                .as_ref()
                .is_none_or(|m| m.title != "Plugins")
        {
            self.note("Plugin terminal opened / available in its tab".into());
            return;
        }
        self.context_menu = None;
        let result = (|| -> Result<()> {
            self.sync_project()?;
            ensure!(
                self.state.terminal_epochs.get(&record.pane) == Some(&record.epoch),
                "Plugin terminal changed before presentation"
            );
            self.state
                .present_plugin_pane(record.pane, record.entrypoint.placement)?;
            self.ensure_sessions();
            self.layout();
            self.save();
            Ok(())
        })();
        if let Err(error) = result {
            self.note(format!("Plugin pane: {error:#}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::display_lines;
    #[test]
    fn complete_logs_fit_cells_without_running_control_sequences() {
        let text = format!("{}\x1b[31mvisible\0", "界e\u{301}".repeat(120));
        let lines = display_lines(&text, 15);
        assert!(
            lines
                .iter()
                .all(|s| starkit::ratatui::text::Line::from(s.as_str()).width() <= 15)
        );
        assert_eq!(lines.concat(), text.replace('\x1b', "␛").replace('\0', "␀"));
    }
}
