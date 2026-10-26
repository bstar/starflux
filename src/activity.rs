//! Read-only agent activity summaries, independent of which project is selected.
use crate::{project_state::ProjectState, runtime::Config};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Seek},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, Serialize)]
pub struct Activity {
    pub working: usize,
    pub waiting: usize,
}
impl Activity {
    pub fn observe(&mut self, state: &str) {
        match state {
            "working" | "busy" | "thinking" | "executing" => self.working += 1,
            "waiting"
            | "needs_input"
            | "waiting_for_input"
            | "waiting_for_approval"
            | "needs_permission" => self.waiting += 1,
            _ => {}
        }
    }
}
fn read(mut command: Command) -> Option<Value> {
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
    serde_json::from_slice(&bytes).ok()
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
        if let Some(data) = read(source.command(&["agent", "list"]))
            && let Some(agents) = data["result"]["agents"].as_array()
        {
            for project in projects.iter().filter(|p| {
                p.herdr.as_ref().is_some_and(|h| {
                    h.session == source.session && h.executable == source.executable
                })
            }) {
                let state = activity.get_mut(&project.workspace).unwrap();
                for agent in agents.iter().filter(|a| {
                    Some(a["workspace_id"].as_str().unwrap_or(""))
                        == project.herdr.as_ref().map(|h| h.workspace_id.as_str())
                }) {
                    state.observe(agent["agent_status"].as_str().unwrap_or("unknown"));
                }
            }
        }
    }
    for project in projects {
        let Ok(state) = ProjectState::load(project) else {
            continue;
        };
        for pane in state.agent_ids().into_iter().filter(|id| {
            !project
                .herdr
                .as_ref()
                .is_some_and(|h| h.panes.contains_key(id))
        }) {
            if let Some(value) = read(project.command(pane, &json!({"workspace": project.workspace, "pane": format!("p{pane}"), "action": "inspect"}), false))
                && value["alive"] == true {
                activity.get_mut(&project.workspace).unwrap().observe(value["state"].as_str().unwrap_or("unknown"));
            }
        }
    }
    activity
}
#[cfg(test)]
mod tests {
    use super::*;
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
        activity.observe("waiting_for_input");
        assert_eq!(activity.working, 2);
        assert_eq!(activity.waiting, 1);
    }
}
