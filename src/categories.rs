//! Session-scoped organization; category edits never operate on processes.
use crate::runtime::Config;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs};
pub const UNASSIGNED: &str = "uncategorized";
pub const HEADER_PADDING: u16 = 6;
pub const HEADER_HEIGHT: u16 = 32 + HEADER_PADDING * 2;
pub fn row_height(id: &str) -> u16 {
    if id.starts_with("category-end:") {
        12
    } else if id.starts_with("category:") {
        HEADER_HEIGHT
    } else {
        34
    }
}
pub fn sidebar_capacity(height: u16, offset: usize, ids: &[&str]) -> usize {
    let mut used = 0u16;
    let mut count = 0;
    for id in ids.iter().skip(offset) {
        let next = used.saturating_add(row_height(id));
        if count > 0 && next > height.saturating_sub(198) {
            break;
        }
        used = next;
        count += 1;
    }
    count.max(1)
}
/// Category headers and padding own their complete drop area.
pub fn drop_index_sized(offset: usize, y: u32, rows: &[(bool, u16)]) -> usize {
    let mut local = y.saturating_sub(122);
    for (i, (header, height)) in rows.iter().enumerate().skip(offset) {
        if local < u32::from(*height) {
            return if *header || local < u32::from(*height) / 2 {
                i
            } else {
                i + 1
            };
        }
        local = local.saturating_sub(u32::from(*height));
    }
    rows.len()
}
#[cfg(test)]
fn drop_index(offset: usize, y: u32, headers: &[bool]) -> usize {
    drop_index_sized(
        offset,
        y,
        &headers.iter().map(|b| (*b, 34)).collect::<Vec<_>>(),
    )
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub revision: u64,
    pub groups: Vec<Group>,
    pub assignments: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
}
impl Catalog {
    pub fn load(config: &Config) -> Result<Self> {
        let path = crate::session_catalog::order_path(config)?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let document: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        Ok(serde_json::from_value(
            document
                .get("categories")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        )?)
    }
    pub fn category<'a>(&'a self, project: &str) -> &'a str {
        self.assignments
            .get(project)
            .filter(|id| self.groups.iter().any(|g| g.id == **id))
            .map(String::as_str)
            .unwrap_or(UNASSIGNED)
    }
    pub fn all(&self) -> Vec<Group> {
        let mut groups = self.groups.clone();
        groups.push(Group {
            id: UNASSIGNED.into(),
            name: "Uncategorized".into(),
        });
        groups
    }
    pub fn edit(
        config: &Config,
        expected: u64,
        change: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<Self> {
        Self::update(config, expected, |c, _| change(c))
    }
    fn update(
        config: &Config,
        expected: u64,
        change: impl FnOnce(&mut Self, &mut serde_json::Value) -> Result<()>,
    ) -> Result<Self> {
        let path = crate::session_catalog::order_path(config)?;
        let _lock = crate::project_sync::Transaction::acquire(path.with_extension("lock"))?;
        let mut document: serde_json::Value = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            serde_json::json!({"manual_order":[],"recent":false,"high_first":true})
        };
        let mut state: Self = serde_json::from_value(
            document
                .get("categories")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        )?;
        ensure!(
            state.revision == expected,
            "Categories changed in another window; reopen the menu"
        );
        change(&mut state, &mut document)?;
        state.revision = state
            .revision
            .checked_add(1)
            .context("Category revision exhausted")?;
        document["categories"] = serde_json::to_value(&state)?;
        document["recent"] = false.into();
        starkit::fs::write_private(&path, &serde_json::to_vec_pretty(&document)?)?;
        crate::project_state::catalog_changed()?;
        Ok(state)
    }
    pub fn move_project(
        config: &Config,
        expected: u64,
        project: &str,
        category: &str,
        before: Option<&str>,
    ) -> Result<Self> {
        Self::update(config, expected, |c, document| {
            c.assign(project, category)?;
            let mut order: Vec<String> = serde_json::from_value(document["manual_order"].clone())?;
            // Newly discovered projects may not have participated in an order edit yet.
            for known in crate::project_state::catalog(config) {
                if !order.contains(&known.workspace) {
                    order.push(known.workspace);
                }
            }
            order.retain(|id| id != project);
            let index = before
                .and_then(|b| order.iter().position(|id| id == b))
                .unwrap_or(order.len());
            order.insert(index, project.into());
            document["manual_order"] = serde_json::to_value(order)?;
            Ok(())
        })
    }
    pub fn name(&mut self, id: Option<&str>, name: &str) -> Result<()> {
        let name = name.trim();
        ensure!(
            !name.is_empty() && name.chars().count() <= 80,
            "Choose a category name of 1–80 characters"
        );
        ensure!(
            !name.eq_ignore_ascii_case("Uncategorized")
                && !self
                    .groups
                    .iter()
                    .any(|g| Some(g.id.as_str()) != id
                        && g.name.to_lowercase() == name.to_lowercase()),
            "That category name already exists"
        );
        if let Some(id) = id {
            self.groups
                .iter_mut()
                .find(|g| g.id == id)
                .context("Category disappeared")?
                .name = name.into();
        } else {
            self.groups.push(Group {
                id: uuid::Uuid::new_v4().simple().to_string(),
                name: name.into(),
            });
        }
        Ok(())
    }
    pub fn assign(&mut self, project: &str, id: &str) -> Result<()> {
        ensure!(
            id == UNASSIGNED || self.groups.iter().any(|g| g.id == id),
            "Category disappeared"
        );
        if id == UNASSIGNED {
            self.assignments.remove(project);
        } else {
            self.assignments.insert(project.into(), id.into());
        }
        Ok(())
    }
    pub fn remove(&mut self, id: &str) -> Result<()> {
        ensure!(
            id != UNASSIGNED && self.groups.iter().any(|g| g.id == id),
            "Category cannot be removed"
        );
        self.groups.retain(|g| g.id != id);
        self.assignments.retain(|_, v| v != id);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn entire_category_header_accepts_a_drop_including_its_bottom_edge() {
        let rows = [(true, 34), (false, 34), (true, 12), (true, 34)];
        for y in 190..202 {
            assert_eq!(drop_index_sized(0, y, &rows), 2);
        }
        assert_eq!(drop_index_sized(0, 202, &rows), 3);
        assert_eq!(row_height("category-end:group"), 12);
        assert_eq!(row_height("category:group"), 44);
        for y in 122..166 {
            assert_eq!(
                drop_index_sized(0, y, &[(true, HEADER_HEIGHT), (false, 34)]),
                0
            );
        }

        for offset in [0, 2] {
            let headers = [false, false, true, false, true];
            for dy in 0..34 {
                assert_eq!(
                    drop_index(offset, 122 + (2 - offset) as u32 * 34 + dy, &headers),
                    2
                );
            }
        }
        assert_eq!(drop_index(0, 122 + 34 + 30, &[false, false, false]), 2);
        assert_eq!(drop_index(0, 122 + 2 * 34 + 33, &[false, false, true]), 2);
        assert_eq!(drop_index(0, 600, &[true, false, false]), 3);
    }
    #[test]
    fn assignment_and_delete_preserve_projects() {
        let mut c = Catalog::default();
        c.name(None, "Work").unwrap();
        let id = c.groups[0].id.clone();
        c.assign("project", &id).unwrap();
        assert_eq!(c.category("project"), id);
        c.remove(&id).unwrap();
        assert_eq!(c.category("project"), UNASSIGNED);
        assert!(c.remove(UNASSIGNED).is_err());
    }
    #[test]
    fn names_are_unique_and_migration_defaults_are_safe() {
        let mut c: Catalog = serde_json::from_str("{}").unwrap();
        assert_eq!(c.category("legacy"), UNASSIGNED);
        c.name(None, "Work").unwrap();
        assert!(c.name(None, " work ").is_err());
        assert!(c.name(None, "").is_err());
        assert!(c.name(None, "Uncategorized").is_err());
    }
}
