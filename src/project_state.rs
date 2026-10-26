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
    #[serde(default)]
    pub space: Space,
    #[serde(default)]
    pub terminal: usize,
    #[serde(default = "default_terminals")]
    pub terminals: Vec<usize>,
    #[serde(default)]
    pub split: bool,
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
            space: Space::Agent,
            terminal: 0,
            terminals: vec![0],
            split: false,
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
        let path = data_root()?.join(format!("{}.layout.json", config.workspace));
        let mut state: Self = if path.exists() {
            serde_json::from_slice(&fs::read(path)?)?
        } else {
            let mut s = Self::default();
            if !config.project_host
                && config
                    .call(0, &config.request(0, "inspect"))
                    .ok()
                    .is_some_and(|v| v["command"] == "codex")
            {
                s.agent = 0;
                s.terminal = 1;
                s.terminals = vec![1];
            }
            s
        };
        // Earlier releases saved the automatic 16px default as a numeric value.
        // Preserve non-default user zooms while returning untouched defaults to inheritance.
        if !state.terminal_font_explicit && state.terminal_font_size == 16 {
            state.terminal_font_size = 0;
        }
        state.validate()?;
        Ok(state)
    }
    pub fn agent_ids(&self) -> Vec<usize> {
        if self.agents.is_empty() {
            vec![self.agent]
        } else {
            self.agents.clone()
        }
    }
    pub fn add_agent(&mut self) -> Result<usize> {
        let mut agents = self.agent_ids();
        let id = (0..32)
            .find(|id| !agents.contains(id) && !self.terminals.contains(id))
            .ok_or_else(|| anyhow::anyhow!("Project session limit reached (32)"))?;
        agents.push(id);
        self.agents = agents;
        self.agent = id;
        self.space = Space::Agent;
        Ok(id)
    }
    pub fn validate(&self) -> Result<()> {
        let agents = self.agent_ids();
        let mut unique = agents.clone();
        unique.sort();
        unique.dedup();
        ensure!(
            !agents.is_empty()
                && agents.len() == unique.len()
                && agents.contains(&self.agent)
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
                self.terminals.contains(id),
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
                && self.terminals.len() <= 30
                && !self.terminals.is_empty()
                && self.terminals.iter().all(|i| *i < 32 && *i != self.agent),
            "Invalid project sessions"
        );
        ensure!(
            self.terminals.contains(&self.terminal),
            "Selected terminal is missing"
        );
        let mut ids = self.terminals.clone();
        ids.sort();
        ids.dedup();
        ensure!(ids.len() == self.terminals.len(), "Duplicate terminal IDs");
        Ok(())
    }
    pub fn save(&self, config: &Config) -> Result<()> {
        self.validate()?;
        let root = data_root()?;
        starkit::fs::write_private(
            &root.join(format!("{}.layout.json", config.workspace)),
            &serde_json::to_vec_pretty(self)?,
        )?;
        starkit::fs::write_private(
            &root.join(format!("{}.json", config.workspace)),
            &serde_json::to_vec_pretty(config)?,
        )?;
        Ok(())
    }
    pub fn add_terminal(&mut self) -> Result<usize> {
        ensure!(self.terminals.len() < 30, "Project terminal limit reached");
        let id = (0..32)
            .find(|i| !self.agent_ids().contains(i) && !self.terminals.contains(i))
            .ok_or_else(|| anyhow::anyhow!("Project terminal limit reached"))?;
        self.terminals.push(id);
        self.terminal = id;
        self.space = Space::Terminals;
        Ok(id)
    }
    pub fn selected(&self) -> usize {
        if self.space == Space::Agent {
            self.agent
        } else {
            self.terminal
        }
    }
    pub fn visible(&self) -> Vec<usize> {
        match self.space {
            Space::Agent => vec![self.agent],
            Space::Apps => vec![],
            Space::Terminals => {
                let mut ids = vec![self.terminal];
                if self.split
                    && let Some(other) = self.terminals.iter().find(|i| **i != self.terminal)
                {
                    ids.push(*other);
                    // Focusing the other half must not exchange pane geometry
                    // underneath a pointer drag.
                    ids.sort_unstable();
                }
                ids
            }
        }
    }
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
        let mut state = ProjectState::default();
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
    fn tabs_splits_and_agents_keep_stable_ids() {
        let mut s = ProjectState::default();
        assert_eq!(s.visible(), vec![1]);
        let id = s.add_terminal().unwrap();
        assert_eq!(id, 2);
        assert_eq!(s.visible(), vec![2]);
        s.split = true;
        assert_eq!(s.visible(), vec![0, 2]);
        s.terminal = 0;
        assert_eq!(s.visible(), vec![0, 2]);
        s.space = Space::Agent;
        assert_eq!(s.visible(), vec![1]);
        s.validate().unwrap();
        s.terminals.push(1);
        assert!(s.validate().is_err())
    }
}
