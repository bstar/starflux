//! Agent context efficiency: how much of each Codex agent's context window is in
//! use, how well its prompt cache is reused, how large its fixed instructions
//! are (global and project AGENTS.md files plus PORTFOLIO.md), and how large
//! its session log has grown. Measurement only; the coordinator proposes fixes.
//!
//! Codex records `token_count` events in its rollout file
//! (`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*-<conversation>.jsonl`). Only the
//! tail of that file is read.
use crate::runtime::Config;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
};

const TAIL: u64 = 1024 * 1024;
/// Codex's default `project_doc_max_bytes`.
const PROJECT_DOC_LIMIT: u64 = 32 * 1024;
const NEAR_LIMIT: f64 = 0.70;
const IDLE_LARGE: f64 = 0.50;
const LARGE_INSTRUCTIONS: u64 = 24 * 1024;
const LARGE_SESSION_LOG: u64 = 200 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentUsage {
    pub project: String,
    pub label: String,
    pub pane: usize,
    pub harness: String,
    pub state: String,
    pub conversation: Option<String>,
    /// Tokens in context after the most recent turn.
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    /// Fixed instruction bytes: global + project AGENTS.md chain and PORTFOLIO.md.
    pub instruction_bytes: u64,
    pub instruction_files: Vec<String>,
    pub session_log_bytes: Option<u64>,
    pub note: Option<String>,
}

impl AgentUsage {
    pub fn window_fraction(&self) -> Option<f64> {
        Some(self.context_tokens? as f64 / self.context_window.filter(|w| *w > 0)? as f64)
    }
    pub fn cache_ratio(&self) -> Option<f64> {
        let input = self.input_tokens.filter(|i| *i >= 20_000)?;
        Some(self.cached_input_tokens? as f64 / input as f64)
    }
    pub fn alerts(&self) -> Vec<crate::portfolio_health::Alert> {
        let mut out = Vec::new();
        let who = format!("{} agent {}", self.label, self.pane);
        let mut push = |severity: &str, kind: &str, message: String| {
            out.push(crate::portfolio_health::Alert {
                severity: severity.into(),
                kind: kind.into(),
                project: Some(self.project.clone()),
                message,
            })
        };
        if let Some(fraction) = self.window_fraction() {
            let idle = matches!(self.state.as_str(), "idle" | "completed");
            if fraction >= NEAR_LIMIT {
                push(
                    "warning",
                    "context_near_limit",
                    format!(
                        "{who} uses {:.0}% of its context window; compact it or start a fresh conversation at a natural break",
                        fraction * 100.0
                    ),
                );
            } else if idle && fraction >= IDLE_LARGE {
                push(
                    "warning",
                    "context_idle_large",
                    format!(
                        "{who} is idle with {:.0}% of its window in use; its next task will resend that context",
                        fraction * 100.0
                    ),
                );
            }
        }
        if let Some(ratio) = self.cache_ratio()
            && ratio < 0.5
        {
            push(
                "warning",
                "context_low_cache",
                format!(
                    "{who} reused only {:.0}% of its input from cache on its last turn",
                    ratio * 100.0
                ),
            );
        }
        if self.instruction_bytes > LARGE_INSTRUCTIONS {
            push(
                "warning",
                "context_large_instructions",
                format!(
                    "{who} carries about {} tokens of fixed instructions every turn ({})",
                    self.instruction_bytes / 4,
                    self.instruction_files.join(", ")
                ),
            );
        }
        if let Some(bytes) = self.session_log_bytes
            && bytes > LARGE_SESSION_LOG
        {
            push(
                "warning",
                "context_large_session_log",
                format!(
                    "{who}'s session log is {}",
                    crate::portfolio_health::human(bytes)
                ),
            );
        }
        out
    }
}

pub fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))
}

static ROLLOUTS: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Locate a conversation's rollout file, newest dated directories first.
pub fn rollout(conversation: &str) -> Option<PathBuf> {
    if !valid_id(conversation) {
        return None;
    }
    if let Some(found) = ROLLOUTS
        .lock()
        .ok()?
        .as_ref()
        .and_then(|m| m.get(conversation).cloned())
        .filter(|p| p.exists())
    {
        return Some(found);
    }
    let suffix = format!("-{conversation}.jsonl");
    let sessions = codex_home()?.join("sessions");
    let sorted = |dir: &Path| -> Vec<PathBuf> {
        let mut v = std::fs::read_dir(dir)
            .map(|r| r.flatten().map(|e| e.path()).collect::<Vec<_>>())
            .unwrap_or_default();
        v.sort();
        v.reverse();
        v
    };
    for year in sorted(&sessions) {
        for month in sorted(&year) {
            for day in sorted(&month) {
                for file in sorted(&day) {
                    if file
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().ends_with(&suffix))
                    {
                        if let Ok(mut cache) = ROLLOUTS.lock() {
                            cache
                                .get_or_insert_with(HashMap::new)
                                .insert(conversation.into(), file.clone());
                        }
                        return Some(file);
                    }
                }
            }
        }
    }
    None
}

/// The most recent `token_count` information in a rollout file's tail.
pub fn last_token_count(path: &Path) -> Option<Value> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL))).ok()?;
    let mut bytes = Vec::new();
    file.take(TAIL).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    text.lines().rev().find_map(|line| {
        if !line.contains("\"token_count\"") {
            return None;
        }
        let value: Value = serde_json::from_str(line).ok()?;
        let payload = value.get("payload").unwrap_or(&value);
        let info = payload.get("info").filter(|i| !i.is_null())?;
        Some(info.clone())
    })
}

/// Codex's instruction files for a working directory: global, then the
/// project chain from the repository root down to the directory.
pub fn instruction_files(directory: &Path) -> (u64, Vec<String>) {
    let size = |p: &Path| {
        std::fs::metadata(p)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len())
    };
    let pick = |dir: &Path| {
        ["AGENTS.override.md", "AGENTS.md"]
            .iter()
            .map(|n| dir.join(n))
            .find_map(|p| size(&p).map(|s| (p, s)))
    };
    let mut total = 0;
    let mut files = Vec::new();
    if let Some((path, bytes)) = codex_home().as_deref().and_then(pick) {
        total += bytes;
        files.push(path.to_string_lossy().into_owned());
    }
    let root = directory
        .ancestors()
        .find(|d| d.join(".git").exists())
        .unwrap_or(directory);
    let mut chain = directory
        .ancestors()
        .take_while(|d| d.starts_with(root))
        .collect::<Vec<_>>();
    chain.reverse();
    let mut project = 0;
    for dir in chain {
        if let Some((path, bytes)) = pick(dir) {
            project += bytes;
            files.push(path.to_string_lossy().into_owned());
        }
    }
    (total + project.min(PROJECT_DOC_LIMIT), files)
}

/// Measure every running agent in a portfolio's projects.
pub fn measure(config: &Config, category: &str) -> Vec<AgentUsage> {
    let mut out = Vec::new();
    let context = crate::portfolio_context::load(config, category).ok();
    for member in crate::category_agent::members(config, category).unwrap_or_default() {
        let Ok(records) = crate::agent_state::records(&member) else {
            continue;
        };
        for record in records {
            let state = serde_json::to_value(&record.report.state)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| "unknown".into());
            if state == "stopped" {
                continue;
            }
            let mut usage = AgentUsage {
                project: member.workspace.clone(),
                label: member.label.clone().unwrap_or(member.workspace.clone()),
                pane: record.pane + 1,
                harness: record.report.harness.clone(),
                state,
                conversation: record.report.conversation_id.clone(),
                ..Default::default()
            };
            if member.host.is_some() {
                usage.note = Some("Remote agent; context measurement is not available yet".into());
            } else if usage.harness != "codex" {
                usage.note = Some("Context is measured for Codex agents only".into());
            } else {
                if let Some(directory) = &member.directory {
                    let (bytes, files) = instruction_files(Path::new(directory));
                    usage.instruction_bytes = bytes;
                    usage.instruction_files = files;
                }
                if let Some(c) = &context
                    && c.ready().is_some()
                {
                    usage.instruction_bytes += c.bytes as u64;
                    if let Some(p) = &c.path {
                        usage
                            .instruction_files
                            .push(p.to_string_lossy().into_owned());
                    }
                }
                match usage.conversation.as_deref().and_then(rollout) {
                    None => usage.note = Some("No Codex session log found yet".into()),
                    Some(path) => {
                        usage.session_log_bytes = std::fs::metadata(&path).ok().map(|m| m.len());
                        if let Some(info) = last_token_count(&path) {
                            let last = &info["last_token_usage"];
                            usage.context_tokens = last["total_tokens"].as_u64();
                            usage.input_tokens = last["input_tokens"].as_u64();
                            usage.cached_input_tokens = last["cached_input_tokens"].as_u64();
                            usage.context_window = info["model_context_window"].as_u64();
                        }
                    }
                }
            }
            out.push(usage);
        }
    }
    out
}

/// Total size of Codex session logs.
pub fn session_storage() -> Option<crate::portfolio_health::FileUsage> {
    let sessions = codex_home()?.join("sessions");
    let bytes = crate::portfolio_health::directory_bytes(&sessions)?;
    Some(crate::portfolio_health::FileUsage {
        path: sessions.to_string_lossy().into_owned(),
        bytes,
        category: "agent_sessions".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_the_last_token_count_from_a_rollout_tail() {
        let dir = std::env::temp_dir().join(format!("ac-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"total_tokens":10},"model_context_window":100}}}"#,
                "\n",
                r#"{"type":"event_msg","payload":{"type":"token_count","info":null}}"#,
                "\n",
                r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"total_tokens":80,"input_tokens":79000,"cached_input_tokens":10000},"model_context_window":100}}}"#,
                "\n",
                r#"{"type":"response_item","payload":{}}"#,
                "\n"
            ),
        )
        .unwrap();
        let info = last_token_count(&path).unwrap();
        assert_eq!(info["last_token_usage"]["total_tokens"], 80);
        let usage = AgentUsage {
            label: "p".into(),
            pane: 1,
            state: "working".into(),
            context_tokens: Some(80),
            context_window: Some(100),
            input_tokens: Some(79000),
            cached_input_tokens: Some(10000),
            ..Default::default()
        };
        let kinds = usage
            .alerts()
            .into_iter()
            .map(|a| a.kind)
            .collect::<Vec<_>>();
        assert!(kinds.contains(&"context_near_limit".to_string()));
        assert!(kinds.contains(&"context_low_cache".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn idle_large_contexts_are_flagged_and_small_ones_are_not() {
        let mut usage = AgentUsage {
            state: "idle".into(),
            context_tokens: Some(55),
            context_window: Some(100),
            ..Default::default()
        };
        assert_eq!(usage.alerts()[0].kind, "context_idle_large");
        usage.context_tokens = Some(20);
        assert!(usage.alerts().is_empty());
    }
    #[test]
    fn instruction_chain_runs_from_the_repository_root() {
        let dir = std::env::temp_dir().join(format!("ac-chain-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
        std::fs::create_dir_all(dir.join("repo/sub")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "outside the repository").unwrap();
        std::fs::write(dir.join("repo/AGENTS.md"), "root").unwrap();
        std::fs::write(dir.join("repo/sub/AGENTS.md"), "sub").unwrap();
        std::fs::write(dir.join("repo/sub/AGENTS.override.md"), "override").unwrap();
        let (_, files) = instruction_files(&dir.join("repo/sub"));
        let project = files
            .iter()
            .filter(|f| f.starts_with(&*dir.to_string_lossy()))
            .collect::<Vec<_>>();
        assert_eq!(project.len(), 2);
        assert!(project[0].ends_with("repo/AGENTS.md"));
        assert!(project[1].ends_with("repo/sub/AGENTS.override.md"));
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn conversation_ids_cannot_escape_the_sessions_directory() {
        assert!(!valid_id("../x"));
        assert!(!valid_id(""));
        assert!(valid_id("01a1bded-5169-7a42-b41d-ae77e75ebb2e"));
    }
}
