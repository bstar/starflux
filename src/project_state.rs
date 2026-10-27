//! Durable project metadata. tmux session IDs remain stable across UI migrations.
use crate::runtime::Config;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Space {
    #[default]
    Agent,
    Terminals,
    Apps,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectState {
    #[serde(skip)]
    pub baseline: std::cell::RefCell<serde_json::Value>,
    /// Legacy agent slots are launch hints only; all sessions are terminal tabs.
    #[serde(default)]
    pub terminal_first: bool,
    #[serde(default)]
    pub reserved_slots: Vec<usize>,
    #[serde(default)]
    pub space: Space,
    #[serde(default)]
    pub terminal: usize,
    #[serde(default = "default_terminals")]
    pub terminals: Vec<usize>,
    #[serde(default)]
    pub terminal_epochs: std::collections::BTreeMap<usize, String>,
    #[serde(default)]
    pub split: bool,
    #[serde(default)]
    pub split_panes: Option<[usize; 2]>,
    #[serde(default)]
    pub terminal_labels: std::collections::BTreeMap<usize, String>,
    #[serde(default = "default_ratio")]
    pub split_ratio: f64,
    #[serde(default = "default_agent")]
    pub agent: usize,
    #[serde(default)]
    pub agents: Vec<usize>,
    #[serde(default)]
    pub agent_labels: std::collections::BTreeMap<usize, String>,
    #[serde(default)]
    pub app_command: String,
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: u16,
    #[serde(default)]
    pub terminal_font_explicit: bool,
    #[serde(default)]
    pub app_targets: Vec<AppTarget>,
    #[serde(default)]
    pub app_layout: Vec<crate::project::WindowLayout>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppTarget {
    pub host: Option<String>,
    pub directory: String,
    pub helper: String,
}
pub fn validate_label(label: &str) -> Result<()> {
    ensure!(!label.trim().is_empty(), "Enter a name");
    ensure!(
        label.chars().count() <= 64 && !label.chars().any(char::is_control),
        "Use up to 64 characters without control characters"
    );
    Ok(())
}
pub fn default_terminal_font_size() -> u16 {
    0
}
fn default_terminals() -> Vec<usize> {
    vec![0]
}
fn default_ratio() -> f64 {
    0.5
}
fn default_agent() -> usize {
    1
}
impl Default for ProjectState {
    fn default() -> Self {
        Self {
            baseline: Default::default(),
            terminal_first: true,
            reserved_slots: vec![],
            space: Space::Terminals,
            terminal: 0,
            terminals: vec![0],
            terminal_epochs: Default::default(),
            split: false,
            split_panes: None,
            terminal_labels: Default::default(),
            split_ratio: 0.5,
            agent: 1,
            agents: vec![],
            agent_labels: Default::default(),
            app_command: String::new(),
            terminal_font_size: default_terminal_font_size(),
            terminal_font_explicit: false,
            app_targets: vec![],
            app_layout: vec![],
        }
    }
}
pub fn data_root() -> Result<PathBuf> {
    let path = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or(
            std::env::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".local/share"),
        )
        .join("starmux/projects");
    starkit::terminal_graphics::session::private_root(&path)?;
    Ok(path)
}
impl ProjectState {
    pub fn load(config: &Config) -> Result<Self> {
        let model = data_root()?.join(format!("{}.model.json", config.workspace));
        let path = if model.exists() {
            model
        } else {
            data_root()?.join(format!("{}.layout.json", config.workspace))
        };
        let mut state: Self = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            let mut s = Self::default();
            if config.agent.is_some() {
                s.agents = vec![s.agent];
                s.terminal = s.agent;
            }
            if !config.project_host
                && config
                    .call(0, &config.request(0, "inspect"))
                    .ok()
                    .is_some_and(|v| v["command"] == "codex")
            {
                s.agent = 0;
                s.agents = vec![0];
                s.terminal = 1;
                s.terminals = vec![1];
            }
            s
        };
        let baseline = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            serde_json::Value::Null
        };
        state.migrate_terminals(config);
        // Earlier releases saved the automatic 16px default as a numeric value.
        // Preserve non-default user zooms while returning untouched defaults to inheritance.
        if !state.terminal_font_explicit && state.terminal_font_size == 16 {
            state.terminal_font_size = 0;
        }
        state.validate()?;
        let mut baseline = baseline;
        if baseline.is_object() {
            baseline["__config"] = serde_json::to_value(config)?;
        }
        *state.baseline.borrow_mut() = baseline;
        Ok(state)
    }
    pub fn agent_ids(&self) -> Vec<usize> {
        if self.agents.is_empty() && !self.terminal_first {
            vec![self.agent]
        } else {
            self.agents.clone()
        }
    }
    pub fn terminal_ids(&self) -> Vec<usize> {
        let mut ids = self.terminals.clone();
        for id in self.agent_ids() {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }
    fn migrate_terminals(&mut self, config: &Config) {
        if let Some(source) = &config.herdr {
            self.reserved_slots = source
                .panes
                .keys()
                .copied()
                .filter(|id| config.canonical_pane(*id) != *id)
                .collect();
        }
        if !self.terminal_first {
            let hints = self.agent_ids();
            if self.space == Space::Agent {
                self.terminal = self.agent;
            }
            self.agents = hints;
            self.terminal_first = true;
        }
        if self.space == Space::Agent {
            self.space = Space::Terminals;
        }
        // Herdr's legacy agent shortcut may alias an existing terminal slot.
        // Normalize only our metadata; preserve its original session/process.
        let imported: Vec<_> = self
            .agents
            .iter()
            .copied()
            .filter(|id| {
                config
                    .herdr
                    .as_ref()
                    .is_some_and(|h| h.panes.contains_key(id))
            })
            .collect();
        for id in imported {
            let canonical = config.canonical_pane(id);
            if !self.terminals.contains(&canonical) {
                self.terminals.push(canonical);
            }
            self.agents.retain(|agent| *agent != id);
        }
        self.terminal = config.canonical_pane(self.terminal);
        self.split_panes = self
            .split_panes
            .map(|pair| pair.map(|id| config.canonical_pane(id)));
        if self.split_panes.is_some_and(|pair| pair[0] == pair[1]) {
            self.split_panes = None;
        }
        for (id, label) in self.agent_labels.clone() {
            self.terminal_labels
                .entry(config.canonical_pane(id))
                .or_insert(label);
        }
        self.agent_labels.retain(|id, _| self.agents.contains(id));
    }
    pub fn add_agent(&mut self) -> Result<usize> {
        let mut agents = self.agent_ids();
        let id = (0..32)
            .find(|id| {
                !agents.contains(id)
                    && !self.terminals.contains(id)
                    && !self.reserved_slots.contains(id)
            })
            .ok_or_else(|| anyhow::anyhow!("Project session limit reached (32)"))?;
        self.terminal_epochs
            .insert(id, uuid::Uuid::new_v4().to_string());
        agents.push(id);
        self.agents = agents;
        self.agent = id;
        self.terminal = id;
        self.space = if self.terminal_first {
            Space::Terminals
        } else {
            Space::Agent
        };
        Ok(id)
    }
    pub fn validate(&self) -> Result<()> {
        let agents = self.agent_ids();
        let mut unique = agents.clone();
        unique.sort();
        unique.dedup();
        ensure!(
            (self.terminal_first || !agents.is_empty())
                && agents.len() == unique.len()
                && (agents.is_empty() || agents.contains(&self.agent))
                && agents
                    .iter()
                    .all(|id| *id < 32 && !self.terminals.contains(id)),
            "Invalid agent sessions"
        );
        for (id, label) in &self.agent_labels {
            ensure!(agents.contains(id), "Label belongs to a missing agent");
            validate_label(label)?;
        }
        for (id, label) in &self.terminal_labels {
            ensure!(
                self.terminal_ids().contains(id),
                "Label belongs to a missing terminal"
            );
            validate_label(label)?;
        }
        ensure!(
            self.terminal_font_size == 0 || (10..=32).contains(&self.terminal_font_size),
            "Terminal font size must be 10–32 pixels"
        );
        ensure!(
            self.split_ratio.is_finite() && (0.2..=0.8).contains(&self.split_ratio),
            "Invalid split ratio"
        );
        ensure!(
            self.agent < 32
                && self.terminal_ids().len() <= 32
                && self.terminals.iter().all(|i| *i < 32),
            "Invalid project sessions"
        );
        ensure!(
            self.terminal_ids().is_empty() || self.terminal_ids().contains(&self.terminal),
            "Selected terminal is missing"
        );
        if let Some(pair) = self.split_panes {
            ensure!(
                pair[0] != pair[1] && pair.iter().all(|id| self.terminal_ids().contains(id)),
                "Invalid split terminals"
            );
        }
        let mut ids = self.terminals.clone();
        ids.sort();
        ids.dedup();
        ensure!(ids.len() == self.terminals.len(), "Duplicate terminal IDs");
        Ok(())
    }
    pub fn save(&self, config: &Config) -> Result<()> {
        self.validate()?;
        let _transaction = crate::project_sync::transaction(config)?;
        let root = data_root()?;
        let path = root.join(format!("{}.model.json", config.workspace));
        let mut local = serde_json::to_value(self)?;
        let latest = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| local.clone());
        let merged = crate::project_sync::merge(&self.baseline.borrow(), &local, &latest);
        let mut durable: Self = serde_json::from_value(merged)?;
        durable.repair_view();
        durable.validate()?;
        starkit::fs::write_private(&path, &serde_json::to_vec_pretty(&durable)?)?;
        let local_config = serde_json::to_value(config)?;
        let config_path = root.join(format!("{}.json", config.workspace));
        let mut merged_config: serde_json::Value = fs::read(&config_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| local_config.clone());
        let previous_config = merged_config.clone();
        let baseline_config = self.baseline.borrow()["__config"].clone();
        for (key, value) in local_config.as_object().unwrap() {
            if baseline_config.is_null() || baseline_config[key] != *value {
                merged_config[key] = value.clone();
            }
        }
        let config_changed = !config_path.exists() || previous_config != merged_config;
        if config_changed {
            starkit::fs::write_private(&config_path, &serde_json::to_vec_pretty(&merged_config)?)?;
        }
        let runtime_path = crate::runtime::root()?.join(format!("{}.json", config.workspace));
        if config_changed && runtime_path.exists() {
            starkit::fs::write_private(&runtime_path, &serde_json::to_vec_pretty(&merged_config)?)?;
        }
        if config_changed {
            catalog_changed()?;
        }
        local["__config"] = local_config;
        *self.baseline.borrow_mut() = local;
        Ok(())
    }
    pub fn save_view(&self, config: &Config, frontend: &str) -> Result<()> {
        let directory = data_root()?.join("views").join(frontend);
        fs::create_dir_all(&directory)?;
        starkit::fs::write_private(
            &directory.join(format!("{}.json", config.workspace)),
            &serde_json::to_vec(self)?,
        )
    }
    pub fn load_view(config: &Config, frontend: &str) -> Result<Self> {
        let mut state = Self::load(config)?;
        if let Ok(bytes) = fs::read(
            data_root()?
                .join("views")
                .join(frontend)
                .join(format!("{}.json", config.workspace)),
        ) {
            let view: serde_json::Value = serde_json::from_slice(&bytes)?;
            let mut local = serde_json::to_value(&state)?;
            for key in crate::project_sync::VIEW {
                if let Some(value) = view.get(*key) {
                    local[*key] = value.clone();
                }
            }
            let baseline = state.baseline.clone();
            state = serde_json::from_value(local)?;
            state.baseline = baseline;
        }
        state.repair_view();
        Ok(state)
    }
    pub fn sync_shared(&mut self, config: &Config) -> Result<bool> {
        let path = data_root()?.join(format!("{}.model.json", config.workspace));
        let Ok(bytes) = fs::read(path) else {
            return Ok(false);
        };
        let latest: serde_json::Value = serde_json::from_slice(&bytes)?;
        let local = serde_json::to_value(&self)?;
        let mut result = local.clone();
        for key in crate::project_sync::SHARED {
            if let Some(value) = latest.get(*key) {
                result[*key] = value.clone();
            }
        }
        if local == result {
            return Ok(false);
        }
        let config_baseline = self.baseline.borrow()["__config"].clone();
        *self = serde_json::from_value(result)?;
        let mut latest = latest;
        latest["__config"] = config_baseline;
        *self.baseline.borrow_mut() = latest;
        self.repair_view();
        Ok(true)
    }
    fn repair_view(&mut self) {
        let ids = self.terminal_ids();
        if !ids.contains(&self.terminal) {
            self.terminal = ids.first().copied().unwrap_or(0)
        }
        if self.split
            && (ids.len() < 2
                || self
                    .split_panes
                    .is_some_and(|p| p.iter().any(|id| !ids.contains(id))))
        {
            self.split = false;
            self.split_panes = None;
        }
    }
    pub fn add_terminal(&mut self) -> Result<usize> {
        ensure!(
            self.terminal_ids().len() < 32,
            "Project terminal limit reached"
        );
        let id = (0..32)
            .find(|i| {
                !self.agent_ids().contains(i)
                    && !self.terminals.contains(i)
                    && !self.reserved_slots.contains(i)
            })
            .ok_or_else(|| anyhow::anyhow!("Project terminal limit reached"))?;
        self.terminal_epochs
            .insert(id, uuid::Uuid::new_v4().to_string());
        self.terminals.push(id);
        self.select_terminal(id);
        self.space = Space::Terminals;
        Ok(id)
    }
    pub fn remove_terminal(&mut self, pane: usize) -> bool {
        let ids = self.terminal_ids();
        let Some(index) = ids.iter().position(|id| *id == pane) else {
            return false;
        };
        let neighbor = self.visible().into_iter().find(|id| *id != pane);
        self.terminal_first = true;
        self.terminal_epochs.remove(&pane);
        self.terminals.retain(|id| *id != pane);
        self.agents.retain(|id| *id != pane);
        self.terminal_labels.remove(&pane);
        self.agent_labels.remove(&pane);
        if self.agent == pane {
            self.agent = self.agents.first().copied().unwrap_or(1);
        }
        let remaining = self.terminal_ids();
        if self.split
            && (self.split_panes.is_some_and(|pair| pair.contains(&pane))
                || remaining.len() < 2
                || (self.split_panes.is_none() && ids.contains(&pane)))
        {
            self.split = false;
            self.split_panes = None;
        }
        if self.terminal == pane {
            self.terminal = neighbor
                .filter(|id| remaining.contains(id))
                .or_else(|| {
                    remaining
                        .get(index.min(remaining.len().saturating_sub(1)))
                        .copied()
                })
                .unwrap_or(0);
        }
        true
    }
    pub fn select_terminal(&mut self, id: usize) {
        if let Some(pair) = &mut self.split_panes
            && !pair.contains(&id)
        {
            let half = usize::from(pair[1] == self.terminal);
            pair[half] = id;
        }
        self.terminal = id;
    }
    pub fn toggle_split(&mut self) -> Result<()> {
        if self.terminal_ids().is_empty() {
            self.add_terminal()?;
            return Ok(());
        }
        if self.split {
            self.split = false;
            self.split_panes = None;
        } else {
            let previous = self.terminal;
            let fresh = self.add_terminal()?;
            self.split_panes = Some([previous, fresh]);
            self.split = true;
        }
        self.space = Space::Terminals;
        Ok(())
    }
    pub fn selected(&self) -> usize {
        if self.space == Space::Agent {
            self.agent
        } else {
            self.terminal
        }
    }
    pub fn visible(&self) -> Vec<usize> {
        if self.terminal_ids().is_empty() {
            return vec![];
        }
        match self.space {
            Space::Agent => vec![self.agent],
            Space::Apps => vec![],
            Space::Terminals => {
                if self.terminal_ids().is_empty() {
                    return vec![];
                }
                if self.split
                    && let Some(pair) = self.split_panes
                    && pair.contains(&self.terminal)
                    && pair.iter().all(|id| self.terminal_ids().contains(id))
                {
                    return pair.to_vec();
                }
                let mut ids = vec![self.terminal];
                if self.split
                    && let Some(other) = self
                        .terminal_ids()
                        .into_iter()
                        .find(|i| *i != self.terminal)
                {
                    ids.push(other);
                    // Focusing the other half must not exchange pane geometry
                    // underneath a pointer drag.
                    ids.sort_unstable();
                }
                ids
            }
        }
    }
}
pub fn catalog_changed() -> Result<()> {
    starkit::fs::write_private(
        &data_root()?.join("catalog-version"),
        uuid::Uuid::new_v4().as_bytes(),
    )
}
pub fn catalog(current: &Config) -> Vec<Config> {
    let mut configs = vec![current.clone()];
    let roots = [crate::runtime::root(), data_root()];
    for root in roots.into_iter().flatten() {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                if let Ok(bytes) = fs::read(entry.path())
                    && let Ok(config) = serde_json::from_slice::<Config>(&bytes)
                    && config.validate().is_ok()
                    && config.collection == current.collection
                    && !configs.iter().any(|c| c.workspace == config.workspace)
                {
                    configs.push(config)
                }
            }
        }
    }
    configs.sort_by(|a, b| {
        a.herdr
            .as_ref()
            .map(|h| h.order)
            .unwrap_or(usize::MAX)
            .cmp(&b.herdr.as_ref().map(|h| h.order).unwrap_or(usize::MAX))
            .then_with(|| a.workspace.cmp(&b.workspace))
    });
    configs
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exiting_a_split_terminal_focuses_its_neighbor_and_removes_its_metadata() {
        let mut state = ProjectState::default();
        state.add_terminal().unwrap();
        state.select_terminal(0);
        state.toggle_split().unwrap();
        assert_eq!(state.visible(), vec![0, 2]);
        state.terminal_labels.insert(2, "Exited".into());
        assert!(state.remove_terminal(2));
        assert_eq!(state.terminal, 0);
        assert!(!state.split);
        assert_eq!(state.visible(), vec![0]);
        assert_eq!(state.terminal_ids(), vec![0, 1]);
        assert!(!state.terminal_labels.contains_key(&2));
        state.validate().unwrap();
    }
    #[test]
    fn exiting_last_terminal_keeps_an_empty_project_and_reuses_the_slot_on_request() {
        let mut state = ProjectState::default();
        assert!(state.remove_terminal(0));
        assert!(state.terminal_ids().is_empty());
        assert!(state.visible().is_empty());
        state.validate().unwrap();
        let restored: ProjectState =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        restored.validate().unwrap();
        assert!(restored.terminal_ids().is_empty());
        assert!(!state.remove_terminal(0));
        assert_eq!(state.add_terminal().unwrap(), 0);
        state.validate().unwrap();
    }
    #[test]
    fn exiting_a_hidden_terminal_preserves_the_current_split() {
        let mut state = ProjectState::default();
        state.add_terminal().unwrap();
        state.select_terminal(0);
        state.toggle_split().unwrap();
        assert!(state.remove_terminal(1));
        assert_eq!(state.visible(), vec![0, 2]);
        assert!(state.split);
        state.validate().unwrap();
    }
    #[test]
    fn split_opens_a_fresh_shell_and_keeps_the_other_tabs_out() {
        let mut state = ProjectState::default();
        state.add_terminal().unwrap();
        state.add_terminal().unwrap();
        state.select_terminal(1);
        state.toggle_split().unwrap();
        assert_eq!(state.terminal, 3);
        assert_eq!(state.visible(), vec![1, 3]);
        state.select_terminal(1);
        assert_eq!(state.visible(), vec![1, 3]);
        state.select_terminal(2);
        assert_eq!(state.visible(), vec![2, 3]);
        let loaded: ProjectState =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert_eq!(loaded.visible(), vec![2, 3]);
        loaded.validate().unwrap();
        state.toggle_split().unwrap();
        assert_eq!(state.visible(), vec![2]);
        state.toggle_split().unwrap();
        assert_eq!(state.visible(), vec![2, 4]);
    }
    #[test]
    fn split_at_capacity_preserves_the_current_view() {
        let mut state = ProjectState::default();
        for _ in 1..32 {
            state.add_terminal().unwrap();
        }
        assert!(state.toggle_split().is_err());
        assert!(!state.split);
        assert_eq!(state.visible(), vec![31]);
        assert_eq!(state.split_panes, None);
    }
    #[test]
    fn legacy_and_multiple_agents_keep_distinct_durable_sessions() {
        for original in [0, 1, 31] {
            let terminal = if original == 0 { 1 } else { 0 };
            let mut state: ProjectState = serde_json::from_value(serde_json::json!({
                "agent": original, "terminal": terminal, "terminals": [terminal]
            }))
            .unwrap();
            assert_eq!(state.agent_ids(), vec![original]);
            let next = state.add_agent().unwrap();
            assert_ne!(next, original);
            assert_ne!(next, terminal);
            let shell = state.add_terminal().unwrap();
            assert!(!state.agent_ids().contains(&shell));
            let restored: ProjectState =
                serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
            assert_eq!(restored.agent_ids(), vec![original, next]);
            assert_eq!(restored.agent, next);
            restored.validate().unwrap();
        }
        let mut state = ProjectState {
            terminal_first: false,
            ..Default::default()
        };
        for _ in 0..30 {
            state.add_agent().unwrap();
        }
        assert_eq!(state.agent_ids().len(), 31);
        assert!(state.add_agent().is_err());
        assert!(state.add_terminal().is_err());
        state.validate().unwrap();
    }
    #[test]
    fn renamed_tabs_round_trip_without_changing_session_identity() {
        let mut state = ProjectState::default();
        state.terminal_labels.insert(0, "Build / café".into());
        let loaded: ProjectState =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        loaded.validate().unwrap();
        assert_eq!(loaded.selected(), state.selected());
        assert_eq!(loaded.terminals, state.terminals);
        assert_eq!(loaded.terminal_labels[&0], "Build / café");
        let mut invalid = loaded;
        invalid.terminal_labels.insert(31, "Unknown".into());
        assert!(invalid.validate().is_err());
        for label in ["", "  ", "hello\nworld", "hello\0world"] {
            assert!(validate_label(label).is_err());
        }
        assert!(validate_label(&"a".repeat(65)).is_err());
    }
    #[test]
    fn terminals_are_primary_and_legacy_sessions_migrate_without_new_ids() {
        let mut state = ProjectState::default();
        assert!(state.agent_ids().is_empty());
        assert_eq!(state.visible(), vec![0]);
        let second = state.add_terminal().unwrap();
        assert_eq!(second, 1);
        state.split = true;
        assert_eq!(state.visible(), vec![0, 1]);
        state.terminal = 0;
        assert_eq!(state.visible(), vec![0, 1]);
        let config: Config = serde_json::from_value(serde_json::json!({"workspace":"migration","host":null,"directory":null,"agent":"codex","executable":"starmux"})).unwrap();
        let mut legacy: ProjectState = serde_json::from_value(serde_json::json!({"space":"agent","agent":1,"agents":[1,3],"terminal":0,"terminals":[0,2],"agent_labels":{"3":"Review"}})).unwrap();
        legacy.migrate_terminals(&config);
        assert_eq!(legacy.terminal_ids(), vec![0, 2, 1, 3]);
        assert_eq!(legacy.terminal, 1);
        assert_eq!(legacy.space, Space::Terminals);
        assert_eq!(legacy.terminal_labels[&3], "Review");
        legacy.validate().unwrap();
        legacy.migrate_terminals(&config);
        assert_eq!(legacy.terminal_ids(), vec![0, 2, 1, 3]);
        legacy.terminal = 3;
        legacy.validate().unwrap();
        let mut restored: ProjectState =
            serde_json::from_slice(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.add_terminal().unwrap(), 4);
    }
}
