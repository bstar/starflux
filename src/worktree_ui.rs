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
            let token = Instant::now();
            self.worktree_token = Some(token);
            self.release();
            self.context_menu = Some(crate::shelf::ContextMenu {
                position: [16, 16],
                title: "Git worktrees".into(),
                subtitle: "Loading from project host…".into(),
                items: vec![crate::shelf::MenuItem {
                    label: "Loading…".into(),
                    hint: String::new(),
                    action: "menu:panel".into(),
                }],
                selected: Some(0),
            });
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let result = crate::worktrees::list(&config).map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Worktrees(Box::new(config), token, result));
            });
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
                if let Some(reason) = target["locked"].as_str() {
                    items.push(crate::shelf::MenuItem {
                        label: format!("Locked: {reason}"),
                        hint: String::new(),
                        action: "menu:panel".into(),
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
            "open" => self.start_worktree_open(config, target),
            "remove" => self.open_prompt(Prompt::RemoveWorktree(config, target)),
            _ => bail!("Unknown worktree action"),
        }
        Ok(true)
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
    pub(super) fn worktree_menu(&mut self, config: Config, catalog: crate::worktrees::Catalog) {
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
            selected: Some(0),
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
