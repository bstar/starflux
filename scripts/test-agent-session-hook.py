#!/usr/bin/env python3
"""Exercise the provider session hook's registration and lifecycle reports.

A fake `starflux` serves `--agent-status` with this test process as the pane's
foreground agent (so the hook's ancestry check holds) and records every
`--agent-report`. No provider, terminal or network is used.
"""
import json
import os
import subprocess
import sys
import tempfile
import uuid
from pathlib import Path

root = Path(__file__).resolve().parents[1]
hook = root / "scripts" / "agent-session-hook.py"


def ticks(pid):
    return int(Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[19])


with tempfile.TemporaryDirectory(prefix="starflux-hook-proof-") as directory:
    tmp = Path(directory)
    state = tmp / "state.json"
    epoch = str(uuid.uuid4())
    boot = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
    state.write_text(json.dumps({
        "agent_pid": os.getpid(), "agent_start_ticks": ticks(os.getpid()),
        "record": None, "reports": [],
    }))
    fake = tmp / "starflux"
    fake.write_text(f"""#!{sys.executable}
import json, sys
from pathlib import Path
state_path = Path({str(state)!r})
data = json.loads(state_path.read_text())
argv = sys.argv[1:]
if "--agent-status" in argv:
    print(json.dumps({{"panes": [{{"pane": 1, "epoch": {epoch!r}, "status": {{
        "agent": "claude", "agent_pid": data["agent_pid"],
        "agent_start_ticks": data["agent_start_ticks"], "boot_id": {boot!r},
        "agent_record": data["record"]}}}}]}}))
elif "--agent-report" in argv:
    report = json.loads(argv[argv.index("--agent-report") + 1])
    data["reports"].append(report)
    data["record"] = report
    state_path.write_text(json.dumps(data))
""")
    fake.chmod(0o700)

    def run(event, expect_ok=True):
        result = subprocess.run(
            [sys.executable, str(hook), "--provider", "claude", "--starflux", str(fake),
             "--workspace", "proof", "--pane", "1", "--epoch", epoch,
             "--", "/usr/bin/claude", "--resume", "{session_id}"],
            input=json.dumps(event), text=True, capture_output=True, timeout=20,
            env={**os.environ, "XDG_RUNTIME_DIR": str(tmp / "run")},
        )
        assert (result.returncode == 0) == expect_ok, result.stderr
        return json.loads(state.read_text())["reports"]

    session = str(uuid.uuid4())
    reports = run({"hook_event_name": "SessionStart", "source": "startup", "session_id": session})
    assert len(reports) == 1 and reports[0]["conversation_id"] == session and reports[0]["sequence"] == 1
    assert reports[0]["resume_argv"] == ["/usr/bin/claude", "--resume", session]
    # None: the event reports nothing new (the state is unchanged or not lifecycle).
    expected = [
        ({"hook_event_name": "UserPromptSubmit"}, "working"),
        ({"hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion"}, "needs_input"),
        ({"hook_event_name": "PostToolUse", "tool_name": "AskUserQuestion"}, "working"),
        ({"hook_event_name": "PreToolUse", "tool_name": "ExitPlanMode"}, "needs_input"),
        ({"hook_event_name": "PostToolUse", "tool_name": "Bash"}, None),
        ({"hook_event_name": "PermissionRequest", "tool_name": "Bash"}, "waiting_for_approval"),
        ({"hook_event_name": "SubagentStart", "agent_id": "a1"}, "working"),
        ({"hook_event_name": "SubagentStart", "agent_id": "a2"}, None),
        ({"hook_event_name": "SubagentStop", "agent_id": "a1"}, None),
        # The turn ends while a background sub-agent still runs: still working.
        ({"hook_event_name": "Stop"}, None),
        ({"hook_event_name": "Notification", "message": "Claude needs your permission to use Bash"},
         "waiting_for_approval"),
        ({"hook_event_name": "Notification", "message": "Claude has a question"}, "needs_input"),
        ({"hook_event_name": "SubagentStop", "agent_id": "a2"}, None),
        ({"hook_event_name": "Stop"}, "completed"),
        ({"hook_event_name": "Notification", "notification_type": "idle_prompt",
          "message": "Claude is waiting for your input"}, None),
        ({"hook_event_name": "SessionEnd", "reason": "exit"}, "stopped"),
    ]
    sequence = 1
    for event, wanted in expected:
        before = len(reports)
        reports = run({**event, "session_id": session})
        if wanted is None:
            assert len(reports) == before, (event, reports[-1])
            continue
        sequence += 1
        last = reports[-1]
        assert last["state"] == wanted and last["sequence"] == sequence, (event, last)
        assert last["run_id"] == reports[0]["run_id"] and last["conversation_id"] == session
    assert not list((tmp / "run" / "starflux" / "agent-subagents").iterdir()), "finished sub-agents leave no record"
    count = len(reports)
    # Another conversation's events, and a repeated state, change nothing.
    assert len(run({"hook_event_name": "Stop", "session_id": str(uuid.uuid4())})) == count
    assert len(run({"hook_event_name": "SessionEnd", "session_id": session})) == count
    # A hook outside the bound agent's process tree is refused.
    data = json.loads(state.read_text())
    data["agent_pid"] = 1
    data["agent_start_ticks"] = ticks(1)
    state.write_text(json.dumps(data))
    run({"hook_event_name": "UserPromptSubmit", "session_id": session}, expect_ok=False)

print(json.dumps({"result": "PASS", "registration": True, "lifecycle_states": [w for _, w in expected if w],
                  "foreign_conversation_ignored": True, "non_descendant_rejected": True}))
