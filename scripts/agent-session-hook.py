#!/usr/bin/env python3
"""Register a provider session for a bound local STAR/FLUX pane and report its lifecycle."""
import argparse
import json
import os
import shlex
import subprocess
import sys
import uuid
from pathlib import Path


def process_identity(pid):
    try:
        _, separator, suffix = Path(f"/proc/{pid}/stat").read_bytes().rpartition(b")")
        fields = suffix.split()
        if not separator or len(fields) <= 19 or fields[0] in (b"Z", b"X", b"x"):
            return None
        return int(fields[1]), int(fields[19])
    except (OSError, ValueError):
        return None


def belongs_to_hook_process(pid, start):
    current = os.getpid()
    for _ in range(64):
        identity = process_identity(current)
        if identity is None:
            return False
        parent, ticks = identity
        if current == pid:
            return ticks == start
        if parent <= 1:
            break
        current = parent
    return False


def resume_template(argv):
    if argv.count("{session_id}") != 1:
        raise ValueError("resume argv needs exactly one {session_id} argument")
    if argv[0] == "{session_id}" or argv[0].startswith("-"):
        raise ValueError("resume argv must start with the provider executable")
    if len(argv) > 64 or any(
        not arg or len(arg.encode("utf-8")) > 4096
        or any(ord(char) < 32 or 127 <= ord(char) < 160 for char in arg)
        for arg in argv
    ):
        raise ValueError("resume argv needs at most 64 nonempty arguments of up to 4096 UTF-8 bytes without control characters")
    return argv


# Provider hook events after SessionStart and the lifecycle state each reports.
LIFECYCLE = (
    "UserPromptSubmit", "Notification", "PermissionRequest", "SubagentStart", "SubagentStop",
    "PreCompact", "PostCompact", "Stop", "SessionEnd",
)
# Tools that stop and wait for the user; only these run the hook around tool use.
ASKING_TOOLS = "AskUserQuestion|ExitPlanMode"


def lifecycle_state(event, subagents_running=False):
    name = event.get("hook_event_name")
    if name in ("UserPromptSubmit", "SubagentStart", "PreCompact"):
        return "working"
    if name == "PostCompact":
        # Automatic compaction happens inside a turn that goes on working; a
        # manual /compact is the whole request, and no Stop follows it.
        return "working" if event.get("trigger") == "auto" else "completed"
    if name == "PreToolUse":
        return "needs_input" if event.get("tool_name") in ASKING_TOOLS.split("|") else None
    if name == "PostToolUse":
        return "working" if event.get("tool_name") in ASKING_TOOLS.split("|") else None
    if name == "PermissionRequest":
        return "waiting_for_approval"
    if name == "Notification":
        kind = str(event.get("notification_type", "")).lower()
        if kind in ("idle_prompt", "auth_success"):
            # "Waiting for your input" after a finished turn is not a question.
            return None
        text = " ".join(str(event.get(key, "")) for key in ("notification_type", "message")).lower()
        return "waiting_for_approval" if "permission" in text or "approv" in text else "needs_input"
    if name == "Stop":
        # The turn ended, but sub-agents it started in the background still work.
        return "working" if subagents_running else "completed"
    if name == "SessionEnd":
        return "stopped"
    return None


def track_subagents(event):
    """Record sub-agents that started and have not stopped; return whether any remain."""
    name = event.get("hook_event_name")
    session = event.get("session_id")
    if name not in ("SubagentStart", "SubagentStop", "Stop", "SessionEnd") or not isinstance(session, str):
        return False
    import fcntl
    root = Path(os.environ.get("XDG_RUNTIME_DIR") or f"/tmp/starflux-{os.getuid()}") / "starflux" / "agent-subagents"
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    path = root / str(uuid.UUID(session))
    with open(path, "a+", encoding="utf-8") as file:
        fcntl.flock(file, fcntl.LOCK_EX)
        file.seek(0)
        running = set(file.read().split())
        agent = str(event.get("agent_id") or "")
        if name == "SubagentStart" and agent:
            running.add(agent)
        elif name == "SubagentStop":
            running.discard(agent)
        elif name == "SessionEnd":
            running.clear()
        file.seek(0)
        file.truncate()
        file.write("\n".join(sorted(running)))
    if not running:
        path.unlink(missing_ok=True)
    return bool(running)


def hook_config(args):
    command = shlex.join([
        args.starflux, "--agent-session-hook", "--provider", args.provider,
        "--", *resume_template(args.resume_argv),
    ])
    group = {"hooks": [{"type": "command", "command": command}]}
    hooks = {name: [group] for name in ("SessionStart", *LIFECYCLE)}
    for name in ("PreToolUse", "PostToolUse"):
        hooks[name] = [{"matcher": ASKING_TOOLS, **group}]
    return {"hooks": hooks}


def bound_status(args):
    command = [args.starflux, "--workspace", args.workspace]
    status = json.loads(subprocess.check_output(
        [*command, "--agent-status", "--agent-status-pane", str(args.pane)], timeout=5,
    ))
    pane = next((p for p in status["panes"] if p["pane"] == args.pane), None)
    if pane is None or pane["epoch"] != args.epoch:
        raise ValueError("the bound pane generation is no longer active")
    health = pane["status"]
    pid, ticks = health.get("agent_pid"), health.get("agent_start_ticks")
    if health.get("agent") != args.provider or not pid or not ticks or not belongs_to_hook_process(pid, ticks):
        raise ValueError(f"the hook is not a descendant of this pane's foreground {args.provider} process")
    return command, health


def report_lifecycle(args, event, state):
    """Advance the registered run; events for another process or conversation are ignored."""
    session = str(uuid.UUID(event.get("session_id", "")))
    command, health = bound_status(args)
    record = health.get("agent_record") or {}
    if not (
        record.get("conversation_id") == session
        and record.get("epoch") == args.epoch
        and record.get("agent_pid") == health["agent_pid"]
        and record.get("agent_start_ticks") == health["agent_start_ticks"]
        and record.get("boot_id") == health.get("boot_id")
        and record.get("source") == f"{args.provider}:session-start"
    ):
        return
    if record.get("state") == state:
        return
    report = {key: record[key] for key in (
        "run_id", "epoch", "agent_pid", "agent_start_ticks", "boot_id", "harness",
        "source", "conversation_id", "resume_argv",
    ) if key in record}
    report.update(sequence=int(record["sequence"]) + 1, state=state)
    subprocess.run(
        [*command, "--pane", str(args.pane), "--agent-report", json.dumps(report)],
        stdout=subprocess.DEVNULL, check=True, timeout=5,
    )


def register(args, event):
    if not isinstance(event, dict):
        raise ValueError("Provider hook payload must be an object")
    state = lifecycle_state(event, track_subagents(event))
    if state is not None:
        report_lifecycle(args, event, state)
        return
    if event.get("hook_event_name") != "SessionStart":
        return
    source = event.get("source")
    if source not in ("startup", "resume", "clear", "fork"):
        return
    session_id = event.get("session_id")
    if not isinstance(session_id, str):
        raise ValueError("Provider session ID must be a UUID string")
    session = str(uuid.UUID(session_id))
    template = resume_template(args.resume_argv)
    argv = [session if arg == "{session_id}" else arg for arg in template]
    command, health = bound_status(args)
    pid, ticks = health["agent_pid"], health["agent_start_ticks"]
    previous = health.get("agent_record") or {}
    same_process = (
        previous.get("epoch") == args.epoch
        and previous.get("agent_pid") == pid
        and previous.get("agent_start_ticks") == ticks
        and previous.get("boot_id") == health.get("boot_id")
    )
    if (
        same_process
        and previous.get("conversation_id")
        and previous["conversation_id"] != session
    ):
        if args.provider == "claude":
            # Claude can dispatch these callbacks without an ordering token.
            raise ValueError("Claude conversation changed; register it from a new bound process")
        if source == "startup":
            raise ValueError("Startup event cannot replace this process's registered conversation")
    if same_process and previous.get("conversation_id") == session and previous.get("resume_argv") == argv:
        return
    report = {
        "run_id": str(uuid.uuid4()),
        "sequence": 1,
        "epoch": args.epoch,
        "agent_pid": pid,
        "agent_start_ticks": ticks,
        "boot_id": health["boot_id"],
        "harness": args.provider,
        "source": f"{args.provider}:session-start",
        "state": "idle",
        "conversation_id": session,
        "resume_argv": argv,
        "replaces_run": previous.get("run_id"),
    }
    subprocess.run(
        [*command, "--pane", str(args.pane), "--agent-report", json.dumps(report)],
        stdout=subprocess.DEVNULL, check=True, timeout=5,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provider", choices=("codex", "claude"), default="codex")
    parser.add_argument("--starflux", default="starflux")
    parser.add_argument("--print-config", action="store_true", help="Print a provider hook JSON fragment for review")
    parser.add_argument("--workspace", default=os.environ.get("STARFLUX_WORKSPACE"))
    parser.add_argument("--pane", default=os.environ.get("STARFLUX_PANE"), type=int)
    parser.add_argument("--epoch", default=os.environ.get("STARFLUX_PANE_EPOCH"))
    parser.add_argument("resume_argv", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.resume_argv[:1] == ["--"]:
        args.resume_argv = args.resume_argv[1:]
    if args.print_config:
        try:
            print(json.dumps(hook_config(args), indent=2))
        except ValueError as error:
            parser.error(str(error))
        return 0
    if not args.workspace or not args.pane or not args.epoch:
        return 0
    if not 1 <= args.pane <= 32:
        parser.error("pane must be between 1 and 32")
    try:
        payload = sys.stdin.buffer.read(65537)
        if len(payload) > 65536:
            raise ValueError("Provider hook payload is too large")
        register(args, json.loads(payload))
    except (KeyError, OSError, ValueError, subprocess.SubprocessError) as error:
        # Providers interpret stdout as hook output. Keep diagnostics on stderr.
        print(f"STAR/FLUX {args.provider} registration: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
