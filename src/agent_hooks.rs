//! Automatic conversation registration for Claude Code agents STAR/FLUX launches.
//!
//! Claude accepts per-launch hooks through `--settings`, so a local agent launch
//! gets session and lifecycle hooks without any change to the user's own settings.
//! The hook (`starflux --agent-session-hook`) records the exact session ID for
//! the bound pane and verifies process ancestry, so a stray or delayed callback
//! cannot register. Codex hooks need persisted trust, so Codex registration stays
//! an explicit, reviewed opt-in (see `docs/agent-lifecycle.md`).
//! `STARFLUX_AGENT_HOOKS=0` launches agents unchanged.
use std::path::Path;

/// Session start registers the conversation; the others report its lifecycle
/// (working, needs input or approval, completed, stopped) to agent status and
/// the attention inbox. Sub-agent events keep a turn that ends with sub-agents
/// still running reported as working.
const EVENTS: [&str; 8] = [
    "SessionStart",
    "UserPromptSubmit",
    "Notification",
    "PermissionRequest",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "SessionEnd",
];

/// Tools that stop and wait for the user: a question, or a plan to approve.
/// Only these run the hook around tool use, so other tool calls pay nothing.
const ASKING_TOOLS: &str = "AskUserQuestion|ExitPlanMode";

/// [`with_session_hook`] with this executable's stable path. A rebuilt binary
/// reports `/proc/self/exe` with a " (deleted)" suffix that would name a file
/// that no longer exists, so every hook would fail; without a usable path the
/// agent launches unchanged and its status comes from its screen.
pub fn with_local_session_hook(argv: Vec<String>) -> Vec<String> {
    match crate::plugin_panes::launcher() {
        Ok(path) => with_session_hook(argv, &path.to_string_lossy()),
        Err(_) => argv,
    }
}

/// Options whose value is a launch choice a resumed conversation should keep.
const KEPT_OPTIONS: [&str; 4] = [
    "--model",
    "--permission-mode",
    "--agent",
    "--fallback-model",
];

pub fn with_session_hook(argv: Vec<String>, starflux: &str) -> Vec<String> {
    if std::env::var("STARFLUX_AGENT_HOOKS").is_ok_and(|value| value == "0") {
        return argv;
    }
    let Some(index) = argv.iter().position(|arg| {
        Path::new(arg)
            .file_name()
            .is_some_and(|name| name == "claude")
    }) else {
        return argv;
    };
    if argv[index + 1..]
        .iter()
        .any(|arg| arg == "--settings" || arg.starts_with("--settings="))
    {
        // The user chose their own per-launch settings; do not override them.
        return argv;
    }
    let mut resume: Vec<String> = argv[..=index].to_vec();
    let mut options = argv[index + 1..].iter();
    while let Some(arg) = options.next() {
        if KEPT_OPTIONS.contains(&arg.as_str()) {
            if let Some(value) = options.next() {
                resume.extend([arg.clone(), value.clone()]);
            }
        } else if KEPT_OPTIONS
            .iter()
            .any(|option| arg.starts_with(&format!("{option}=")))
        {
            resume.push(arg.clone());
        }
    }
    resume.extend(["--resume".into(), "{session_id}".into()]);
    let hook = shell_words::join(
        [
            starflux,
            "--agent-session-hook",
            "--provider",
            "claude",
            "--",
        ]
        .into_iter()
        .map(String::from)
        .chain(resume),
    );
    let group = serde_json::json!([{"hooks": [{"type": "command", "command": hook}]}]);
    let mut hooks: serde_json::Map<String, serde_json::Value> = EVENTS
        .iter()
        .map(|event| ((*event).to_string(), group.clone()))
        .collect();
    let asking = serde_json::json!([{"matcher": ASKING_TOOLS, "hooks": group[0]["hooks"].clone()}]);
    hooks.insert("PreToolUse".into(), asking.clone());
    hooks.insert("PostToolUse".into(), asking);
    let settings = serde_json::json!({ "hooks": hooks }).to_string();
    let mut launched = argv;
    launched.splice(index + 1..index + 1, ["--settings".to_string(), settings]);
    launched
}

#[cfg(test)]
mod tests {
    use super::with_session_hook;

    fn argv(text: &str) -> Vec<String> {
        shell_words::split(text).unwrap()
    }

    #[test]
    fn claude_launches_get_a_session_start_hook_that_keeps_model_choices() {
        let launched = with_session_hook(
            argv("env A=1 /usr/bin/claude --model opus --permission-mode=plan hello"),
            "/opt/starflux",
        );
        assert_eq!(
            &launched[..4],
            &argv("env A=1 /usr/bin/claude --settings")[..]
        );
        assert_eq!(
            &launched[5..],
            &argv("--model opus --permission-mode=plan hello")[..]
        );
        let settings: serde_json::Value = serde_json::from_str(&launched[4]).unwrap();
        for event in super::EVENTS {
            assert_eq!(settings["hooks"][event], settings["hooks"]["SessionStart"]);
        }
        for event in ["PreToolUse", "PostToolUse"] {
            let group = &settings["hooks"][event][0];
            assert_eq!(group["matcher"], super::ASKING_TOOLS);
            assert_eq!(
                group["hooks"],
                settings["hooks"]["SessionStart"][0]["hooks"]
            );
        }
        let hook = settings["hooks"]["SessionStart"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert_eq!(
            shell_words::split(hook).unwrap(),
            argv(
                "/opt/starflux --agent-session-hook --provider claude -- env A=1 /usr/bin/claude \
                 --model opus --permission-mode=plan --resume {session_id}"
            )
        );
    }

    #[test]
    fn other_programs_and_explicit_settings_are_unchanged() {
        for command in [
            "codex resume",
            "bash -l",
            "claude --settings {}",
            "claude --settings={}",
        ] {
            assert_eq!(with_session_hook(argv(command), "starflux"), argv(command));
        }
    }
}
