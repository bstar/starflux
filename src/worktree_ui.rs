use super::*;
use anyhow::bail;
impl Workspace {
    pub(super) fn worktree_action(&mut self, action: &str) -> Result<bool> {
        let Some(command) = action.strip_prefix("worktrees:") else {
            return Ok(false);
        };
        if command == "list" || command.starts_with("project:") {
            let config = if let Some(id) = command.strip_prefix("project:") {
                crate::saved(id)?
            } else {
                self.config.clone()
            };
            self.load_worktree_catalog(config, None);
            return Ok(true);
        }
        let (config, catalog) = self
            .worktree_catalog
            .clone()
            .context("Open the worktree list first")?;
        if command == "create" {
            self.open_prompt(Prompt::CreateWorktree(config));
            return Ok(true);
        }
        let (operation, index) = command
            .split_once(':')
            .context("Invalid worktree command")?;
        let target = catalog
            .worktrees
            .get(index.parse::<usize>()?)
            .context("Worktree list changed")?
            .clone();
        match operation {
            "entry" => {
                let branch = target["branch"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches("refs/heads/");
                let mut items = vec![crate::shelf::MenuItem {
                    label: format!(
                        "Branch: {}",
                        if branch.is_empty() {
                            "detached"
                        } else {
                            branch
                        }
                    ),
                    hint: String::new(),
                    action: "menu:panel".into(),
                }];
                if target["available"] == true {
                    items.push(crate::shelf::MenuItem {
                        label: "Open worktree project".into(),
                        hint: String::new(),
                        action: format!("worktrees:open:{index}"),
                    });
                    if target["main"] != true && target["locked"].is_null() {
                        items.push(crate::shelf::MenuItem {
                            label: "Remove worktree…".into(),
                            hint: "Clean / no running programs".into(),
                            action: format!("worktrees:remove:{index}"),
                        });
                    }
                }
                if target["main"] == true {
                    items.push(crate::shelf::MenuItem {
                        label: "Primary worktree / retained".into(),
                        hint: String::new(),
                        action: "menu:panel".into(),
                    });
                }
                if target["main"] != true && target["bare"] != true {
                    let (label, action) = if target["locked"].is_null() {
                        ("Lock worktree", "lock")
                    } else {
                        ("Unlock worktree", "unlock")
                    };
                    items.push(crate::shelf::MenuItem {
                        label: label.into(),
                        hint: String::new(),
                        action: format!("worktrees:{action}:{index}"),
                    });
                }
                if let Some(reason) = target["locked"]
                    .as_str()
                    .filter(|reason| !reason.is_empty())
                {
                    items.push(crate::shelf::MenuItem {
                        label: format!("Locked: {reason}"),
                        hint: String::new(),
                        action: "menu:panel".into(),
                    });
                }
                if target["path"].is_string() {
                    items.push(crate::shelf::MenuItem {
                        label: "Copy worktree path".into(),
                        hint: config.host.as_deref().unwrap_or("Local checkout").into(),
                        action: format!("worktrees:copy-path:{index}"),
                    });
                }
                if let Some(head) = target["head"].as_str().filter(|head| !head.is_empty()) {
                    items.push(crate::shelf::MenuItem {
                        label: format!("Copy HEAD · {}", head.chars().take(12).collect::<String>()),
                        hint: String::new(),
                        action: format!("worktrees:copy-head:{index}"),
                    });
                }
                if target["available"] != true {
                    items.push(crate::shelf::MenuItem {
                        label: "Directory unavailable / explicit Git repair required".into(),
                        hint: String::new(),
                        action: "menu:panel".into(),
                    });
                }
                items.push(crate::shelf::MenuItem {
                    label: "Back to repository worktrees".into(),
                    hint: String::new(),
                    action: format!("worktrees:project:{}", config.workspace),
                });
                self.context_menu = Some(crate::shelf::ContextMenu {
                    position: [16, 16],
                    title: "Git worktree".into(),
                    subtitle: target["path"].as_str().unwrap_or("").into(),
                    items,
                    selected: Some(1),
                });
            }
            "lock" | "unlock" => {
                self.load_worktree_catalog(config, Some((target, operation == "lock")))
            }
            "copy-path" | "copy-head" => {
                let (field, label) = if operation == "copy-path" {
                    ("path", "Worktree path copied")
                } else {
                    ("head", "Worktree HEAD copied")
                };
                let text = target[field]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .context("Worktree detail is unavailable")?;
                self.effects
                    .push(ServerMessage::Clipboard { text: text.into() });
                self.note(label.into());
            }
            "open" => self.start_worktree_open(config, target),
            "remove" => self.open_prompt(Prompt::RemoveWorktree(config, target)),
            _ => bail!("Unknown worktree action"),
        }
        Ok(true)
    }
    fn load_worktree_catalog(&mut self, config: Config, change: Option<(Value, bool)>) {
        let token = Instant::now();
        self.worktree_token = Some(token);
        self.release();
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [16, 16],
            title: "Git worktrees".into(),
            subtitle: if change.is_some() {
                "Updating lock on project host…".into()
            } else {
                "Loading from project host…".into()
            },
            items: vec![crate::shelf::MenuItem {
                label: "Loading…".into(),
                hint: String::new(),
                action: "menu:panel".into(),
            }],
            selected: Some(0),
        });
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let refresh = || crate::worktrees::list(&config).map_err(|error| format!("{error:#}"));
            let (result, lock_error) = match &change {
                Some((target, locked)) => {
                    match crate::worktrees::set_lock(&config, target, *locked, None) {
                        Ok(result) => (Ok(result.catalog), None),
                        Err(error) => (refresh(), Some(format!("{error:#}"))),
                    }
                }
                None => (refresh(), None),
            };
            let _ = tx.send(Event::Worktrees(
                Box::new(config),
                token,
                result,
                lock_error,
                change.map(|(target, _)| target),
            ));
        });
    }
    fn start_worktree_open(&mut self, config: Config, target: Value) {
        let token = Instant::now();
        self.worktree_token = Some(token);
        let origin = self.config.workspace.clone();
        self.context_menu = None;
        self.release();
        self.note("Opening worktree project…".into());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = crate::worktrees::open(&config, &target)
                .map(Box::new)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::WorktreeOpened(origin, token, result));
        });
    }
    pub(super) fn create_worktree(
        &mut self,
        config: Config,
        branch: String,
        path: String,
        base: String,
    ) {
        let token = Instant::now();
        self.worktree_token = Some(token);
        let origin = self.config.workspace.clone();
        self.note("Creating and opening worktree…".into());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let target = crate::worktrees::create(
                    &config,
                    &branch,
                    (!path.is_empty()).then_some(path.as_str()),
                    (!base.is_empty()).then_some(base.as_str()),
                )?;
                crate::worktrees::open(&config, &target)
            })()
            .map(Box::new)
            .map_err(|e: anyhow::Error| format!("{e:#}"));
            let _ = tx.send(Event::WorktreeOpened(origin, token, result));
        });
    }
    pub(super) fn remove_worktree(&mut self, config: Config, target: Value) {
        let token = Instant::now();
        self.worktree_token = Some(token);
        self.note("Checking worktree before removal…".into());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = crate::worktrees::remove(&config, &target).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::WorktreeRemoved(token, result));
        });
    }
    pub(super) fn worktree_menu(
        &mut self,
        config: Config,
        catalog: crate::worktrees::Catalog,
        previous_target: Option<&Value>,
    ) {
        let selected = previous_target
            .and_then(|target| {
                catalog
                    .worktrees
                    .iter()
                    .position(|row| crate::worktrees::same_directory(row, target))
            })
            .map_or(0, |index| index + 1);
        let mut items = vec![crate::shelf::MenuItem {
            label: "Create & open worktree…".into(),
            hint: String::new(),
            action: "worktrees:create".into(),
        }];
        items.extend(catalog.worktrees.iter().enumerate().map(|(i, target)| {
            let branch = target["branch"]
                .as_str()
                .unwrap_or("")
                .trim_start_matches("refs/heads/");
            crate::shelf::MenuItem {
                label: format!(
                    "{} · {}",
                    if branch.is_empty() {
                        "detached"
                    } else {
                        branch
                    },
                    target["path"].as_str().unwrap_or("")
                ),
                hint: if target["main"] == true {
                    "primary".into()
                } else if !target["locked"].is_null() {
                    "locked".into()
                } else {
                    String::new()
                },
                action: format!("worktrees:entry:{i}"),
            }
        }));
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [16, 16],
            title: "Git worktrees".into(),
            subtitle: catalog.repository["common_dir"]
                .as_str()
                .unwrap_or("")
                .into(),
            items,
            selected: Some(selected),
        });
        self.worktree_catalog = Some((config, catalog));
    }
    pub(super) fn leave_removed_worktree(&mut self) -> Result<()> {
        if let Some(origin) = crate::worktrees::removed_origin(&self.config)? {
            let old = self.config.workspace.clone();
            self.reopen_project(crate::saved(&origin)?)?;
            self.close_project(&old)?;
            self.note("Worktree removed / returned to its repository project".into());
        }
        Ok(())
    }
}
