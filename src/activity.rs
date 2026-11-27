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
    pub available: bool,
    pub unknown: usize,
    pub failed: usize,
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
        self.available = true;
        self.unknown += usize::from(state == "unknown");
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
            "failed" | "backend_unavailable" => self.failed += 1,
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
    if choices.len() >= 2 && choice_dialog(&footer) {
        return "needs_input";
    }
    if background_work(&footer) {
        return "working";
    }
    for line in footer.lines().rev() {
        if claude_working(line, &footer) {
            return "working";
        }
        let line = line.trim_start_matches(|c: char| c.is_whitespace() || "│┃|●*•■".contains(c));
        if line.starts_with("automatic reconnect could not restore this session")
            || line.starts_with("server connection could not be restored")
        {
            return "backend_unavailable";
        }
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
/// A selection dialog waiting on the user: the cursor sits on a numbered
/// choice (`❯ 1. Yes, and auto-accept edits`) under a question or with the
/// dialog's key hints, as in Claude Code's plan approval and its questions.
fn choice_dialog(footer: &str) -> bool {
    let cursor = footer.lines().any(|line| {
        let Some(rest) = line.trim_start().strip_prefix('❯') else {
            return false;
        };
        let rest = rest.trim_start();
        let tail = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        tail.len() < rest.len() && tail.starts_with(['.', ')'])
    });
    cursor
        && (footer.lines().any(|line| line.trim_end().ends_with('?'))
            || [
                "enter to select",
                "esc to cancel",
                "to navigate",
                "tell claude what to change",
            ]
            .iter()
            .any(|hint| footer.contains(hint)))
}
/// Agent work that outlives Claude Code's turn: sub-agents, tasks and
/// teammates, listed on the status line (`⏵⏵ … · 1 shell, 2 agents · ↓ to
/// manage`) and on the finished line (`✻ Cooked for 3m · done 9:33 AM · 1 agent
/// still running`). A background shell is not: it may be a server, a game or a
/// stuck command that runs for days after the agent has finished, and a monitor
/// only waits on events.
fn background_work(footer: &str) -> bool {
    footer
        .lines()
        .filter(|line| line.contains("↓ to manage") || line.contains("still running"))
        .flat_map(|line| line.split(['·', ',']))
        .any(|segment| {
            let segment = segment.trim();
            let segment = segment
                .strip_suffix("still running")
                .map_or(segment, str::trim_end);
            let Some((count, kind)) = segment.split_once(' ') else {
                return false;
            };
            count.parse::<u32>().is_ok_and(|n| n > 0)
                && matches!(
                    kind.trim(),
                    "agent"
                        | "agents"
                        | "local agent"
                        | "local agents"
                        | "background task"
                        | "background tasks"
                        | "task"
                        | "tasks"
                        | "teammate"
                        | "teammates"
                )
        })
}
/// Claude Code's busy line: a spinner glyph and one changing verb ending in an
/// ellipsis, optionally followed by a parenthesis, e.g. `· misting…` (2.1.329,
/// with "esc to interrupt" on the hint line below the input box) or
/// `✻ cogitating… (12s · ↑ 1.2k tokens · esc to interrupt)`. "esc to interrupt"
/// must appear somewhere in the footer.
fn claude_working(line: &str, footer: &str) -> bool {
    const GLYPHS: &str = "✻✶✳✢✽·*●•+";
    let line = line.trim_start();
    if !line.starts_with(|c: char| GLYPHS.contains(c)) {
        return false;
    }
    let line = line.trim_start_matches(|c: char| c.is_whitespace() || GLYPHS.contains(c));
    let (label, tail) = line
        .split_once('(')
        .map_or((line, None), |(label, tail)| (label, Some(tail)));
    let label = label.trim();
    let Some(word) = label
        .strip_suffix('…')
        .or_else(|| label.strip_suffix("..."))
    else {
        return false;
    };
    // A verb (`Cogitating…`) or a short phrase: Claude shows compaction as
    // `Compacting conversation…` and its hooks as `Running PreCompact hooks…`.
    if word.is_empty()
        || word.len() > 40
        || word.split(' ').count() > 4
        || word.contains("  ")
        || !word
            .chars()
            .all(|c| c.is_alphabetic() || c == '-' || c == ' ')
    {
        return false;
    }
    match tail {
        // A running timer is the busy line itself; newer releases show a tip
        // beneath it instead of "esc to interrupt".
        Some(tail) => {
            tail.contains(')') && (elapsed_timer(tail) || footer.contains("esc to interrupt"))
        }
        None => footer.contains("esc to interrupt"),
    }
}
/// `5m 1s · ↓ 11.4k tokens)` or `12s)`: the parenthesis opens with a duration.
fn elapsed_timer(tail: &str) -> bool {
    let head = tail.split(['·', '•', ')']).next().unwrap_or("").trim();
    !head.is_empty()
        && head.split_whitespace().all(|part| {
            part.len() >= 2
                && part[..part.len() - 1].chars().all(|c| c.is_ascii_digit())
                && part.ends_with(['h', 'm', 's'])
        })
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
    // Imported agents are read in one batch for the whole multiplexer session.
    for project in projects {
        activity.insert(project.workspace.clone(), Activity::default());
        if let Some(source) = &project.agent_mux {
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
                p.agent_mux.as_ref().is_some_and(|h| {
                    h.session == source.session && h.executable == source.executable
                })
            }) {
                let state = activity.get_mut(&project.workspace).unwrap();
                state.source_sequence = project
                    .agent_mux
                    .as_ref()
                    .and_then(|h| sequences.get(&h.workspace_id))
                    .copied()
                    .unwrap_or(0);
                state.available = true;
                for agent in agents.iter().filter(|a| {
                    Some(a["workspace_id"].as_str().unwrap_or(""))
                        == project.agent_mux.as_ref().map(|h| h.workspace_id.as_str())
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
                        format!("{}:{id}", crate::agent_mux::SOURCE),
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
        if state.terminal_ids().is_empty() {
            activity.get_mut(&project.workspace).unwrap().available = true;
        }
        for pane in state
            .agent_ids()
            .into_iter()
            .chain(state.terminals.iter().copied())
            .filter(|id| {
                !project
                    .agent_mux
                    .as_ref()
                    .is_some_and(|h| h.panes.contains_key(id))
            })
        {
            let value = if let Some(terminal) = project.adopted_terminal(pane) {
                terminal.observe(pane).ok()
            } else if matches!(crate::native_backend::reference(project, pane), Ok(Some(_))) {
                // Native terminals answer over the backend's socket; spawning a
                // helper process per pane every refresh dominated idle CPU.
                project.call(pane, &project.request(pane, "inspect")).ok()
            } else {
                read(project.command(pane, &json!({"workspace": project.workspace, "pane": format!("p{pane}"), "action": "inspect"}), false))
            };
            if let Some(mut value) = value {
                crate::agent_state::enrich(project, pane, &mut value);
                let _ = crate::agent_state::observe_attention(
                    project,
                    pane,
                    state.terminal_epochs.get(&pane).map(String::as_str),
                    &value,
                );
                let activity = activity.get_mut(&project.workspace).unwrap();
                activity.available = true;
                if (value["alive"] == true && value["agent"].as_str().is_some())
                    || matches!(
                        value["state"].as_str(),
                        Some("failed" | "backend_unavailable")
                    )
                {
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
    fn claude_code_busy_lines_are_working() {
        for line in [
            "✻ Cogitating… (12s · ↑ 1.2k tokens · esc to interrupt)",
            "✶ Pondering… (esc to interrupt · ctrl+t to show todos)",
            "· Brewing... (3s · esc to interrupt)",
        ] {
            assert_eq!(
                screen_state(&format!("output\n{line}\n> ")),
                "working",
                "{line}"
            );
        }
        // Interruption offered on the hint line below the input box.
        assert_eq!(
            screen_state(
                "✻ Clauding… (8s · ↓ 300 tokens)\n╭───╮\n│ > │\n╰───╯\n  ⏵⏵ auto mode · esc to interrupt"
            ),
            "working"
        );
        // Screens captured from Claude Code 2.1.329 in a tmux pane.
        assert_eq!(
            screen_state(include_str!("../testdata/claude-code-2.1.329-busy.txt")),
            "working"
        );
        assert_eq!(
            screen_state(include_str!("../testdata/claude-code-2.1.329-idle.txt")),
            "idle"
        );
        // Without a spinner glyph, an ellipsis line is ordinary output.
        assert_eq!(screen_state("Loading… (step 2)\nesc to interrupt"), "idle");
        // A finished turn, prose that mentions interruption and a bare ellipsis are idle.
        for screen in [
            "✻ Worked for 2m 10s",
            "Loading… the docs say (esc to interrupt) works",
            "> Waiting…",
        ] {
            assert_eq!(screen_state(screen), "idle", "{screen}");
        }
    }
    #[test]
    fn claude_spinner_with_a_timer_and_a_tip_is_working() {
        let screen = "● Checking the build\n\n✽ Jitterbugging… (5m 1s · ↓ 11.4k tokens)\n  ⎿  Tip: Use /btw to ask a side question without interrupting Claude\n\n──────\n❯ f\n──────\n  ⏵⏵ auto mode on (shift+tab to cycle)";
        assert_eq!(screen_state(screen), "working");
        assert_eq!(screen_state("✻ Cogitating… (12s)\n❯ "), "working");
        // Compaction runs on the same busy line, under a phrase.
        assert_eq!(
            screen_state("✻ Compacting conversation… (34s · esc to interrupt)\n❯ "),
            "working"
        );
        assert_eq!(
            screen_state("✢ Running PreCompact hooks… (2s)\n❯ "),
            "working"
        );
        assert_eq!(screen_state("● Read the notes for this…\n❯ "), "idle");
        for idle in [
            "✻ Worked for 5m 1s\n❯ ",
            "✽ Jitterbugging… (soon · tip)\n❯ ",
            "✽ Jitterbugging…\n❯ ",
        ] {
            assert_eq!(screen_state(idle), "idle", "{idle}");
        }
    }
    #[test]
    fn claude_plan_approval_and_questions_need_input() {
        // Claude Code 2.1.332's plan approval, as captured from a live pane.
        let plan = "   4. Trainers & chemistry\n\n  ────────\n   Claude has written up a plan and is ready to execute. Would you like to proceed?\n\n   ❯ 1. Yes, and switch to BYPASS PERMISSIONS (no further prompts) for this session\n     2. Yes, manually approve edits\n     3. Tell Claude what to change\n        shift+tab to approve with this feedback\n\n   ctrl+g to edit in Omarchy-launch-editor · ~/.claude/plans/plan.md";
        assert_eq!(screen_state(plan), "needs_input");
        let question = "☐ Layout\n\nWhich layout should the panel use?\n\n❯ 1. Grouped by machine\n     Projects under each host\n  2. Flat list\n  3. Type something.\n\nEnter to select · ↑/↓ to navigate · Esc to cancel";
        assert_eq!(screen_state(question), "needs_input");
        for not_a_dialog in [
            // A numbered list in the transcript, with no cursor on it.
            "Steps?\n  1. Build\n  2. Test\n──────\n❯ \n──────\n  ⏵⏵ bypass permissions on (shift+tab to cycle)",
            // The cursor on one numbered line is a typed prompt, not choices.
            "──────\n❯ 1. do it?\n──────",
        ] {
            assert_eq!(screen_state(not_a_dialog), "idle", "{not_a_dialog}");
        }
    }
    #[test]
    fn claude_background_agents_are_working_and_shells_are_not() {
        let agent = "✻ Cooked for 1h 38m 8s · done 12:14 AM · 1 agent still running\n\n──────\n❯ \n──────\n  ⏵⏵ bypass permissions on · 1 agent · ↓ to manage";
        assert_eq!(screen_state(agent), "working");
        for status in [
            "  ⏵⏵ bypass permissions on · 2 agents · ↓ to manage",
            "  ⏵⏵ accept edits on · 1 shell, 1 local agent · ↓ to manage",
            "  1 local agent · ↓ to manage",
        ] {
            assert_eq!(
                screen_state(&format!("✻ Worked for 2m\n❯ \n{status}")),
                "working",
                "{status}"
            );
        }
        for idle in [
            // A monitor waits on events; on its own it is not work.
            "✻ Worked for 2m\n❯ \n  ⏵⏵ bypass permissions on · 1 monitor · ↓ to manage",
            // A finished turn whose background shell (a server, a stuck
            // command) keeps running is idle; the agent is not working.
            "✻ Cooked for 1h 38m 8s · done 12:14 AM · 1 shell still running\n\n──────\n❯ \n──────\n  ⏵⏵ bypass permissions on · 1 shell · ↓ to manage",
            "✻ Baked for 2m 10s · done 10:07 PM · 1 shell still running\n──────\n❯ please run the game so I can see the progress\n──────\n  ⏵⏵ bypass permissions on · 1 shell · ↓ to manage",
            "✻ Worked for 2m\n❯ \n  ⏵⏵ accept edits on · 2 shells, 1 monitor · ↓ to manage",
            "✻ Brewed for 36m 13s · done 12:03 AM\n❯ \n  ⏵⏵ bypass permissions on (shift+tab to cycle)",
            "  Ran 2 shell commands\n❯ ",
        ] {
            assert_eq!(screen_state(idle), "idle", "{idle}");
        }
    }
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
    #[test]
    fn reconnect_failure_is_not_idle_or_old_work() {
        assert_eq!(
            screen_state(
                "Working (1s · esc to interrupt)\n■ Automatic reconnect could not restore this session. Your draft is still editable.\n■ Server connection could not be restored"
            ),
            "backend_unavailable"
        );
        assert_eq!(
            screen_state("The error was: Server connection could not be restored"),
            "idle"
        );
        assert_eq!(
            screen_state(
                "■ Server connection could not be restored\nWorking (1s · esc to interrupt)"
            ),
            "working"
        );
        let mut activity = Activity::default();
        activity.observe("backend_unavailable");
        assert_eq!(
            (activity.failed, activity.running, activity.working),
            (1, 1, 0)
        );
    }
}
