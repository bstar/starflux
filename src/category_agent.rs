//! Opt-in category coordinator identity and configuration. No implicit inference.
use crate::runtime::Config;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Codex,
    Claude,
    Custom,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub enabled: bool,
    pub provider: Provider,
    pub executable: Option<PathBuf>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub charter: String,
    #[serde(default = "subscription")]
    pub authentication: String,
}
fn subscription() -> String {
    "subscription".into()
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: Provider::Codex,
            executable: None,
            model: None,
            effort: None,
            charter: String::new(),
            authentication: subscription(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct State {
    pub version: u32,
    pub revision: u64,
    pub archived: bool,
    pub label: Option<String>,
    pub profile: Profile,
    pub conversation: Option<String>,
    pub archived_conversations: Vec<Value>,
}
pub fn root_path(config: &Config, category: &str) -> Result<PathBuf> {
    ensure!(
        crate::categories::Catalog::load(config)?
            .all()
            .iter()
            .any(|c| c.id == category),
        "Category is unavailable"
    );
    storage_path(config, category)
}
pub fn storage_path(config: &Config, category: &str) -> Result<PathBuf> {
    ensure!(
        !category.is_empty()
            && category
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "Invalid category identity"
    );
    Ok(crate::project_state::data_path()
        .parent()
        .context("Missing data namespace")?
        .join("category-agents")
        .join(crate::session_catalog::group(config))
        .join(category))
}
pub(crate) fn load_path(path: &std::path::Path) -> Result<State> {
    let file = path.join("coordinator.json");
    if !file.exists() {
        return Ok(State {
            version: 1,
            ..Default::default()
        });
    }
    let state: State = crate::plugin_store::read(&file, 1024 * 1024)?;
    ensure!(
        state.version == 1,
        "Unsupported category coordinator version"
    );
    Ok(state)
}
pub fn read(config: &Config, category: &str) -> Result<Value> {
    let state = load_path(&read_root(config, category)?)?;
    Ok(
        json!({"category":category,"session":crate::session_catalog::group(config),"state":state,"runtime":crate::category_agent_runtime::snapshot(config,category)?,"policy":{"review_timing":"on_demand","evidence":"repositories_and_structured_status","terminal_history":false,"repository_writes":false,"handoff_approval_required":true,"busy_agent":"queue"}}),
    )
}
pub fn configure(
    config: &Config,
    category: &str,
    expected: u64,
    profile: Profile,
) -> Result<Value> {
    ensure!(
        profile.charter.len() <= 32768,
        "Category objectives exceed 32 KiB"
    );
    for text in [&profile.model, &profile.effort].into_iter().flatten() {
        ensure!(
            !text.contains('\0') && text.len() <= 256,
            "Invalid provider setting"
        );
    }
    ensure!(
        if profile.provider == Provider::Custom {
            profile.authentication == "custom"
        } else {
            matches!(profile.authentication.as_str(), "subscription" | "api")
        },
        "Choose subscription or api authentication for Codex/Claude, or custom for a custom adapter"
    );
    if profile.provider == Provider::Custom && profile.enabled {
        ensure!(
            profile.executable.is_some(),
            "Custom providers require an executable"
        );
    }
    let root = root_path(config, category)?;
    starkit::terminal_graphics::session::private_root(&root)?;
    let _lock = crate::project_sync::Transaction::acquire(root.join("coordinator.lock"))?;
    let mut state = load_path(&root)?;
    ensure!(
        state.revision == expected,
        "Category agent configuration changed; reload it"
    );
    let provider_changed = state.profile.provider != profile.provider
        || state.profile.executable != profile.executable
        || state.profile.authentication != profile.authentication;
    if provider_changed && let Some(conversation) = state.conversation.take() {
        state
            .archived_conversations
            .push(json!({"provider":state.profile.provider,"conversation":conversation}));
    }
    ensure!(
        ["subscription", "api", "custom"].contains(&profile.authentication.as_str()),
        "Choose subscription, api or custom authentication"
    );
    if !profile.enabled || serde_json::to_value(&state.profile)? != serde_json::to_value(&profile)?
    {
        crate::category_agent_runtime::revoke(&root)?;
    }
    state.archived = false;
    state.label = crate::categories::Catalog::load(config)?
        .all()
        .into_iter()
        .find(|g| g.id == category)
        .map(|g| g.name);
    state.profile = profile;
    state.revision = state
        .revision
        .checked_add(1)
        .context("Coordinator revision exhausted")?;
    crate::automation::persist(&root.join("coordinator.json"), &serde_json::to_vec(&state)?)?;
    if provider_changed {
        crate::category_agent_runtime::clear_capabilities(&root)?;
    }
    drop(_lock);
    read(config, category)
}
pub fn members(config: &Config, category: &str) -> Result<Vec<Config>> {
    root_path(config, category)?;
    let catalog = crate::categories::Catalog::load(config)?;
    Ok(crate::project_state::catalog(config)
        .into_iter()
        .filter(|p| catalog.category(&p.workspace) == category)
        .collect())
}
pub fn evidence(config: &Config, category: &str) -> Result<Value> {
    let state = load_path(&root_path(config, category)?)?;
    ensure!(
        state.profile.enabled,
        "Enable the category agent before requesting its evidence"
    );
    let catalog = crate::categories::Catalog::load(config)?;
    let mut projects = Vec::new();
    let began = std::time::Instant::now();
    let root = root_path(config, category)?;
    for member in members(config, category)? {
        let current = load_path(&root)?;
        ensure!(
            current.profile.enabled && current.revision == state.revision,
            "Category evidence access was revoked"
        );
        ensure!(
            crate::categories::Catalog::load(config)?.category(&member.workspace) == category,
            "Category membership changed"
        );
        let mut project = json!({"id":member.workspace,"label":member.label,"host":member.host,"directory":member.directory,"reported_agents":crate::agent_state::records(&member).map(|records|records.iter().map(|r|json!({"pane":r.pane,"observed_ms":r.observed_ms,"run_id":r.report.run_id,"state":r.report.state,"harness":r.report.harness,"conversation_id":r.report.conversation_id})).collect::<Vec<_>>()).unwrap_or_default()});
        project["coverage"] = if began.elapsed() > std::time::Duration::from_secs(10) {
            json!({"available":false,"reason":"Snapshot budget reached; inspect this project through the scoped repository tools"})
        } else {
            crate::category_agent_files::query(&member, "git", "", "")
                .unwrap_or_else(|e| json!({"available":false,"reason":format!("{e:#}")}))
        };
        projects.push(project);
    }
    Ok(
        json!({"version":1,"category":category,"membership_revision":catalog.revision,"configuration_revision":state.revision,"charter":state.profile.charter,"projects":projects,"terminal_history_included":false}),
    )
}

pub fn read_root(config: &Config, category: &str) -> Result<PathBuf> {
    if let Ok(root) = root_path(config, category) {
        return Ok(root);
    }
    let root = storage_path(config, category)?;
    ensure!(load_path(&root)?.archived, "Category is unavailable");
    Ok(root)
}
pub fn archive(config: &Config, category: &str) -> Result<()> {
    let root = root_path(config, category)?;
    if !root.exists() {
        return Ok(());
    }
    let _guard = crate::project_sync::Transaction::acquire(root.join("coordinator.lock"))?;
    let mut state = load_path(&root)?;
    crate::category_agent_runtime::revoke(&root)?;
    state.profile.enabled = false;
    state.archived = true;
    state.label = crate::categories::Catalog::load(config)?
        .all()
        .into_iter()
        .find(|g| g.id == category)
        .map(|g| g.name);
    if let Some(conversation) = state.conversation.take() {
        state.archived_conversations.push(json!({"provider":state.profile.provider,"conversation":conversation,"reason":"category_deleted"}));
    }
    state.revision = state
        .revision
        .checked_add(1)
        .context("Coordinator revision exhausted")?;
    crate::automation::persist(&root.join("coordinator.json"), &serde_json::to_vec(&state)?)
}
pub fn archives(config: &Config) -> Result<Value> {
    let parent = storage_path(config, "placeholder")?
        .parent()
        .context("Missing category namespace")?
        .to_owned();
    let mut entries = Vec::new();
    if parent.exists() {
        for child in std::fs::read_dir(parent)?.flatten() {
            let id = child.file_name().to_string_lossy().into_owned();
            if let Ok(root) = storage_path(config, &id)
                && let Ok(state) = load_path(&root)
                && state.archived
            {
                entries.push(json!({"id":id,"label":state.label}));
            }
        }
    }
    Ok(json!({"archived_categories":entries}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_is_disabled_and_read_only_by_default() {
        let p: Profile = serde_json::from_str("{}").unwrap();
        assert!(!p.enabled);
        assert_eq!(p.provider, Provider::Codex);
        assert!(p.model.is_none());
        assert!(serde_json::from_str::<Profile>(r#"{"terminal_history":true}"#).is_err());
    }
}
