use super::*;
use crate::categories::{Catalog, UNASSIGNED};
#[derive(Clone)]
pub(super) struct CategoryRow {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub attention: usize,
    pub working: usize,
    pub project: Option<Config>,
}
fn item(label: impl Into<String>, action: impl Into<String>) -> crate::shelf::MenuItem {
    crate::shelf::MenuItem {
        label: label.into(),
        hint: String::new(),
        action: action.into(),
    }
}
impl Workspace {
    pub(super) fn category_drop(&mut self, source: &str, gap: usize) -> Result<()> {
        let rows = self.category_rows();
        let target = rows.get(gap);
        if target.is_some_and(|r| r.id == source) {
            return Ok(());
        }
        let target_category = target
            .or_else(|| rows.last())
            .map(|r| {
                r.id.strip_prefix("category:")
                    .or_else(|| r.id.strip_prefix("category-end:"))
                    .unwrap_or_else(|| self.category_catalog.category(&r.id))
            })
            .unwrap_or(UNASSIGNED)
            .to_owned();
        if let Some(id) = source.strip_prefix("category:") {
            if id == target_category {
                return Ok(());
            }
            self.category_catalog =
                Catalog::edit(&self.config, self.category_catalog.revision, |c| {
                    let from = c
                        .groups
                        .iter()
                        .position(|g| g.id == id)
                        .context("Category disappeared")?;
                    let group = c.groups.remove(from);
                    let to = c
                        .groups
                        .iter()
                        .position(|g| g.id == target_category)
                        .unwrap_or(c.groups.len());
                    c.groups.insert(to, group);
                    Ok(())
                })?;
        } else {
            self.category_catalog = Catalog::move_project(
                &self.config,
                self.category_catalog.revision,
                source,
                &target_category,
                target
                    .filter(|r| r.project.is_some())
                    .map(|r| r.id.as_str()),
            )?;
        }
        self.reload_categories();
        self.navigation.load_session_order(&self.config)?;
        self.navigation.sort(&mut self.projects);
        Ok(())
    }
    pub(super) fn reload_categories(&mut self) {
        if let Ok(c) = Catalog::load(&self.config) {
            self.category_catalog = c;
        }
        self.category_projects = crate::project_state::catalog(&self.config);
    }
    pub(super) fn category_rows(&self) -> Vec<CategoryRow> {
        let mut rows = Vec::new();
        for group in self.category_catalog.all() {
            let projects = self
                .category_projects
                .iter()
                .filter(|p| self.category_catalog.category(&p.workspace) == group.id)
                .collect::<Vec<_>>();
            let hidden = projects
                .iter()
                .filter(|p| self.navigation.closed_projects.contains(&p.workspace))
                .count();
            let mut working = 0;
            let mut attention = 0;
            let mut unknown = 0;
            for p in &projects {
                let a = self.project_activity(p);
                working += usize::from(a.working > 0);
                attention += usize::from(a.waiting > 0 || a.failed > 0);
                unknown += usize::from(
                    !a.available
                        || a.unknown > 0
                        || self.activity_at.elapsed() > Duration::from_secs(8),
                );
            }
            let detail = if projects.is_empty() {
                "No projects".into()
            } else {
                if working == 0 && attention == 0 && unknown == 0 {
                    "Idle".into()
                } else {
                    format!("{working} working · {attention} attention · {unknown} unknown")
                }
            };
            let detail = if let Some(badge) = self.coordinator_badges.get(&group.id) {
                format!("{badge} · {detail}")
            } else {
                detail
            };
            let collapsed = self.navigation.collapsed_categories.contains(&group.id);
            rows.push(CategoryRow {
                id: format!("category:{}", group.id),
                name: format!(
                    "{} {} ({}{}){}",
                    if collapsed { "▸" } else { "▾" },
                    group.name,
                    projects.len(),
                    if hidden > 0 {
                        format!(" · {hidden} hidden")
                    } else {
                        String::new()
                    },
                    if self.category_catalog.category(&self.config.workspace) == group.id {
                        " ●"
                    } else {
                        ""
                    }
                ),
                detail,
                attention,
                working,
                project: None,
            });
            if self.cells {
                rows.push(CategoryRow {
                    id: format!("category:{}", group.id),
                    name: format!("  W{working} !{attention} ?{unknown}"),
                    detail: String::new(),
                    attention,
                    working,
                    project: None,
                });
            }
            if !collapsed {
                for p in &self.projects {
                    if self.category_catalog.category(&p.workspace) == group.id {
                        rows.push(CategoryRow {
                            id: p.workspace.clone(),
                            name: p.label.clone().unwrap_or(p.workspace.clone()),
                            detail: p.host.clone().unwrap_or("local".into()),
                            attention: 0,
                            working: 0,
                            project: Some(p.clone()),
                        });
                    }
                }
            }
            rows.push(CategoryRow {
                id: format!("category-end:{}", group.id),
                name: String::new(),
                detail: String::new(),
                attention: 0,
                working: 0,
                project: None,
            });
        }
        rows
    }
    fn category_menu(&mut self, title: &str, items: Vec<crate::shelf::MenuItem>) {
        self.release();
        self.category_menu_revision = self.category_catalog.revision;
        self.context_menu = Some(crate::shelf::ContextMenu {
            position: [12, 12],
            title: title.into(),
            subtitle: "Session categories · manual order".into(),
            items,
            selected: Some(0),
        });
    }
    pub(super) fn category_action(&mut self, action: &str) -> Result<bool> {
        let Some(command) = action.strip_prefix("categories:") else {
            return Ok(false);
        };
        let expected = if self.context_menu.as_ref().is_some_and(|m| {
            matches!(
                m.title.as_str(),
                "Categories" | "Category projects" | "Move project to category"
            )
        }) {
            self.category_menu_revision
        } else {
            self.category_catalog.revision
        };
        if command == "global" {
            self.category_menu(
                "Sidebar options",
                vec![
                    item("Create category…", "categories:new"),
                    item("Add project…", "project:new"),
                    item("Manage categories…", "categories:manage"),
                    item("Archived category agents…", "category-agent:archives"),
                    item("Expand all categories", "categories:expand-all"),
                    item("Collapse all categories", "categories:collapse-all"),
                ],
            );
            return Ok(true);
        }
        if matches!(command, "expand-all" | "collapse-all") {
            if command == "expand-all" {
                self.navigation.collapsed_categories.clear();
            } else {
                self.navigation
                    .collapsed_categories
                    .extend(self.category_catalog.all().into_iter().map(|g| g.id));
            }
            self.navigation.dirty = true;
            self.navigation.save(&self.navigation_scope)?;
            self.context_menu = None;
            self.project_offset = 0;
            return Ok(true);
        }
        if command == "background" {
            return Ok(true);
        }
        if command.starts_with("append:") {
            return Ok(true);
        }
        if command == "new" {
            self.open_prompt(Prompt::Category(None, expected, None));
            return Ok(true);
        }
        if command == "manage" {
            let mut items = vec![item("Add category…", "categories:new")];
            for g in self.category_catalog.all() {
                items.push(item(g.name, format!("categories:details:{}", g.id)));
            }
            self.category_menu("Categories", items);
            return Ok(true);
        }
        if let Some(id) = command.strip_prefix("toggle:") {
            if !self.navigation.collapsed_categories.remove(id) {
                self.navigation.collapsed_categories.insert(id.into());
            }
            self.navigation.dirty = true;
            self.navigation.save(&self.navigation_scope)?;
            self.project_offset = self.project_offset.min(
                self.category_rows()
                    .len()
                    .saturating_sub(self.project_page_size.max(1)),
            );
            return Ok(true);
        }
        if let Some(project) = command.strip_prefix("new-for:") {
            self.open_prompt(Prompt::Category(None, expected, Some(project.into())));
            return Ok(true);
        }
        if let Some(project) = command.strip_prefix("move:") {
            let mut items = self
                .category_catalog
                .all()
                .iter()
                .map(|g| item(&g.name, format!("categories:assign:{project}:{}", g.id)))
                .collect::<Vec<_>>();
            items.push(item(
                "Create category and move project…",
                format!("categories:new-for:{project}"),
            ));
            self.category_menu("Move project to category", items);
            return Ok(true);
        }
        if let Some(id) = command.strip_prefix("details:") {
            let mut items = vec![
                item("Category agent…", format!("category-agent:open:{id}")),
                item("Expand / collapse", format!("categories:toggle:{id}")),
            ];
            if id != UNASSIGNED {
                items.extend([
                    item("Rename…", format!("categories:rename:{id}")),
                    item("Move up", format!("categories:up:{id}")),
                    item("Move down", format!("categories:down:{id}")),
                    item(
                        "Delete category / move projects to Uncategorized",
                        format!("categories:delete:{id}"),
                    ),
                ]);
            }
            for p in &self.category_projects {
                if self.category_catalog.category(&p.workspace) == id {
                    let a = self.project_activity(p);
                    items.push(item(
                        format!(
                            "{} · {} working · {} waiting · {} failed{}",
                            p.label.as_deref().unwrap_or(&p.workspace),
                            a.working,
                            a.waiting,
                            a.failed,
                            if self.navigation.closed_projects.contains(&p.workspace) {
                                " · hidden"
                            } else {
                                ""
                            }
                        ),
                        format!("categories:reveal:{}", p.workspace),
                    ));
                }
            }
            self.category_menu("Category projects", items);
            return Ok(true);
        }
        if let Some(id) = command.strip_prefix("rename:") {
            self.open_prompt(Prompt::Category(Some(id.into()), expected, None));
            return Ok(true);
        }
        if let Some(project) = command.strip_prefix("reveal:") {
            let config = crate::saved(project)?;
            self.navigation.open_project(project);
            self.navigation
                .collapsed_categories
                .remove(self.category_catalog.category(project));
            self.context_menu = None;
            self.reopen_project(config)?;
            return Ok(true);
        }
        if let Some(tail) = command.strip_prefix("assign:") {
            let (project, category) = tail.split_once(':').context("Missing category")?;
            self.category_catalog =
                Catalog::move_project(&self.config, expected, project, category, None)?;
            self.navigation.load_session_order(&self.config)?;
            self.navigation.sort(&mut self.projects);
            self.reload_categories();
            self.context_menu = None;
            return Ok(true);
        }
        let (kind, id) = command.split_once(':').context("Unknown category action")?;
        if kind == "delete" {
            crate::category_agent::archive(&self.config, id)?;
        }
        self.category_catalog = Catalog::edit(&self.config, expected, |c| {
            match kind {
                "delete" => {
                    crate::category_agent::archive(&self.config, id)?;
                    c.remove(id)?;
                }
                "assign" => {
                    let (project, category) = id.split_once(':').context("Missing category")?;
                    c.assign(project, category)?;
                }
                "up" | "down" => {
                    let i = c
                        .groups
                        .iter()
                        .position(|g| g.id == id)
                        .context("Category disappeared")?;
                    let next = if kind == "up" {
                        i.saturating_sub(1)
                    } else {
                        (i + 1).min(c.groups.len() - 1)
                    };
                    c.groups.swap(i, next);
                }
                _ => anyhow::bail!("Unknown category action"),
            }
            Ok(())
        })?;
        self.reload_categories();
        self.context_menu = None;
        Ok(true)
    }
}
