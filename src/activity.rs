//! Read-only agent activity summaries, independent of which project is selected.
use crate::{project_state::ProjectState, runtime::Config};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Seek},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, Serialize)]
pub struct Activity {
    pub running: usize,
    pub working: usize,
    pub waiting: usize,
    pub questions: usize,
    #[serde(skip)]
    pub signals: BTreeMap<String, String>,
    #[serde(skip)]
    pub source_sequence: u64,
}
impl Activity {
    pub fn observe(&mut self, state: &str) {
        if matches!(
            state,
            "needs_input" | "question_pending" | "waiting_for_answer"
        ) {
            self.questions += 1;
        }
        if !matches!(state, "stopped" | "missing" | "disconnected") {
            self.running += 1;
        }
        match state {
            "working" | "busy" | "thinking" | "executing" => self.working += 1,
            "needs_input"
            | "question_pending"
            | "waiting_for_answer"
            | "waiting_for_approval"
            | "needs_approval"
            | "needs_permission" => self.waiting += 1,
            _ => {}
        }
    }
}
fn read_output(mut command: Command) -> Option<Vec<u8>> {
    let mut output = tempfile::tempfile().ok()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(output.try_clone().ok()?)
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    let result = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(2) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !result.success() {
        return None;
    }
    output.rewind().ok()?;
    let mut bytes = Vec::new();
    output.take(1024 * 1024).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}
fn read(command: Command) -> Option<Value> {
    serde_json::from_slice(&read_output(command)?).ok()
}
pub fn screen_state(text: &str) -> &'static str {
    let footer = text
        .lines()
        .rev()
        .take(32)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    let choices: Vec<_> = footer
        .lines()
        .filter_map(|line| {
            let line =
                line.trim_start_matches(|c: char| c.is_whitespace() || "│┃|>›❯●*".contains(c));
            let tail = line.trim_start_matches(|c: char| c.is_ascii_digit());
            if tail.len() < line.len() && (tail.starts_with('.') || tail.starts_with(')')) {
                Some(tail[1..].trim_start())
            } else {
                line.strip_prefix("y)")
                    .or_else(|| line.strip_prefix("n)"))
                    .map(str::trim_start)
            }
        })
        .collect();
    let yes = choices
        .iter()
        .any(|c| c.starts_with("yes") || c.starts_with("allow once"));
    let no = choices
        .iter()
        .any(|c| c.starts_with("no") || c.starts_with("don't allow") || c.starts_with("deny this"));
    let question = footer.lines().any(|line| {
        let line = line.trim_start_matches(|c: char| c.is_whitespace() || "│┃|>›❯●*".contains(c));
        line.starts_with("question ")
            || line.starts_with("question:")
            || line.starts_with("user input requested")
    });
    if question
        && !choices.is_empty()
        && ["enter to submit", "enter to confirm", "enter to select"]
            .iter()
            .any(|s| footer.contains(s))
    {
        return "needs_input";
    }
    if yes && no && (footer.contains("esc") || footer.contains("enter")) {
        return "waiting_for_approval";
    }
    for line in footer.lines().rev() {
        let line = line.trim_start_matches(|c: char| c.is_whitespace() || "│┃|●*•".contains(c));
        if line.starts_with("worked for ") {
            return "idle";
        }
        let Some((label, tail)) = line.split_once('(') else {
            continue;
        };
        if !matches!(
            label.trim(),
            "working" | "thinking" | "running" | "executing"
        ) {
            continue;
        }
        if let Some((elapsed, after)) = tail.split_once("esc to interrupt")
            && elapsed.starts_with(|c: char| c.is_ascii_digit())
            && elapsed
                .chars()
                .all(|c| c.is_ascii_digit() || ".hms ·•".contains(c))
            && after.starts_with(')')
        {
            return "working";
        }
    }
    "idle"
}
pub fn screen_observation(command: Command) -> Option<(String, String)> {
    let bytes = read_output(command)?;
    Some((
        screen_state(&String::from_utf8_lossy(&bytes)).into(),
        format!("{:x}", Sha256::digest(&bytes)),
    ))
}
pub fn inspect(projects: &[Config]) -> BTreeMap<String, Activity> {
    let mut activity = BTreeMap::new();
    let mut sources = BTreeMap::new();
    // Imported agents are read in one batch for the whole Herdr session.
    for project in projects {
        activity.insert(project.workspace.clone(), Activity::default());
        if let Some(source) = &project.herdr {
            sources
                .entry((source.executable.clone(), source.session.clone()))
                .or_insert(source);
        }
    }
    for source in sources.values() {
        let mut sequences = BTreeMap::<String, u64>::new();
        if let Some(data) = read(source.command(&["agent", "list"]))
            && let Some(agents) = data["result"]["agents"].as_array()
        {
            for agent in agents {
                if let (Some(workspace), Some(sequence)) = (
                    agent["workspace_id"].as_str(),
                    agent["state_change_seq"].as_u64(),
                ) {
                    let value = sequences.entry(workspace.into()).or_default();
                    *value = (*value).max(sequence);
                }
            }
        }
        if let Some(data) = read(source.command(&["api", "snapshot"]))
            && let Some(agents) = data["result"]["snapshot"]["panes"].as_array()
        {
            for project in projects.iter().filter(|p| {
                p.herdr.as_ref().is_some_and(|h| {
                    h.session == source.session && h.executable == source.executable
                })
            }) {
                let state = activity.get_mut(&project.workspace).unwrap();
                state.source_sequence = project
                    .herdr
                    .as_ref()
                    .and_then(|h| sequences.get(&h.workspace_id))
                    .copied()
                    .unwrap_or(0);
                for agent in agents.iter().filter(|a| {
                    Some(a["workspace_id"].as_str().unwrap_or(""))
                        == project.herdr.as_ref().map(|h| h.workspace_id.as_str())
                }) {
                    let profile = agent["agent"].as_str();
                    let observed = agent["pane_id"]
                        .as_str()
                        .and_then(|id| source.observed_status(id, profile));
                    let status = observed.as_ref().map(|v| v.0.as_str()).unwrap_or_else(|| {
                        if matches!(profile, Some("codex" | "claude")) {
                            "unknown"
                        } else {
                            agent["agent_status"].as_str().unwrap_or("unknown")
                        }
                    });
                    if profile.is_some() {
                        state.observe(status);
                    }
                    let id = agent["terminal_id"]
                        .as_str()
                        .or_else(|| agent["pane_id"].as_str())
                        .unwrap_or("agent");
                    state.signals.insert(
                        format!("herdr:{id}"),
                        format!(
                            "{}:{}",
                            observed
                                .as_ref()
                                .map(|v| v.1.clone())
                                .unwrap_or_else(|| agent["revision"].to_string()),
                            status
                        ),
                    );
                }
            }
        }
    }
    for project in projects {
        let Ok(state) = ProjectState::load(project) else {
            continue;
        };
        for pane in state
            .agent_ids()
            .into_iter()
            .chain(state.terminals.iter().copied())
            .filter(|id| {
                !project
                    .herdr
                    .as_ref()
                    .is_some_and(|h| h.panes.contains_key(id))
            })
        {
            let value = if let Some(terminal) = project.adopted_terminal(pane) {
                terminal.observe(pane).ok()
            } else {
                read(project.command(pane, &json!({"workspace": project.workspace, "pane": format!("p{pane}"), "action": "inspect"}), false))
            };
            if let Some(value) = value {
                let activity = activity.get_mut(&project.workspace).unwrap();
                if value["alive"] == true && value["agent"].as_str().is_some() {
                    activity.observe(value["state"].as_str().unwrap_or("unknown"));
                }
                activity.signals.insert(
                    format!("native:{pane}"),
                    format!(
                        "{}:{}:{}:{}",
                        value["pid"], value["alive"], value["state"], value["output_fingerprint"]
                    ),
                );
            }
        }
    }
    activity
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_footer_distinguishes_work_requests_and_idle() {
        assert_eq!(
            screen_state("Working (56m 33s · esc to interrupt)"),
            "working"
        );
        assert_eq!(
            screen_state(
                "Working (1s · esc to interrupt)\nQuestion 1/1\n› 1. Yes\n  2. No\nEnter to submit"
            ),
            "needs_input"
        );
        assert_eq!(
            screen_state("1. Yes, proceed\n2. No, and tell Codex\nEsc to cancel"),
            "waiting_for_approval"
        );
        for idle in [
            "Done. › Type a request",
            "Press Enter to continue; Esc to cancel",
            "The previous tool was blocked",
            "Should we change this?",
            "Shell screen tail: '\\n• Working (4m 28s • esc to interrupt)\\n'",
            "The old status was: Working (1s · esc to interrupt)",
            "• Working (1s · esc to interrupt)\nWorked for 4m\n› Ask Codex to do anything",
        ] {
            assert_eq!(screen_state(idle), "idle", "{idle}");
        }
        assert_eq!(
            screen_state("Worked for 4m\n• Working (2s · esc to interrupt)"),
            "working"
        );
        let mut activity = Activity::default();
        activity.observe("needs_input");
        assert_eq!(
            (activity.working, activity.questions, activity.waiting),
            (0, 1, 1)
        );
    }

    #[test]
    fn living_but_idle_agents_do_not_animate() {
        let mut activity = Activity::default();
        for state in [
            "idle",
            "done",
            "unknown",
            "running",
            "stopped",
            "disconnected",
        ] {
            activity.observe(state);
        }
        assert_eq!(activity.working, 0);
        activity.observe("working");
        activity.observe("thinking");
        activity.observe("waiting_for_approval");
        activity.observe("blocked");
        assert_eq!(activity.working, 2);
        assert_eq!(activity.waiting, 1);
    }
}
