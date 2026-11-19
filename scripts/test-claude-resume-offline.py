#!/usr/bin/env python3
"""Verify Claude Code's SessionStart ID and exact resume in a disposable profile."""
import argparse
import json
import os
import shlex
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("claude", type=Path, help="Path to an installed Claude Code executable")
parser.add_argument("--starflux", type=Path, help="Verify the STAR/FLUX resume action with Claude print mode")
args = parser.parse_args()
claude = args.claude.resolve(strict=True)

with tempfile.TemporaryDirectory(prefix="starflux-claude-proof-") as directory:
    root = Path(directory)
    profile = root / "profile"
    profile.mkdir()
    workspace = root / "workspace"
    workspace.mkdir()
    event_path = root / "event.json"
    capture = root / "capture.py"
    capture.write_text(
        "import json, sys\n"
        "from pathlib import Path\n"
        "event = json.load(sys.stdin)\n"
        "fields = ('hook_event_name', 'session_id', 'source')\n"
        "destination = Path(sys.argv[1])\n"
        "temporary = destination.with_suffix('.tmp')\n"
        "temporary.write_text(json.dumps({key: event[key] for key in fields}))\n"
        "temporary.replace(destination)\n"
    )
    settings = {
        "hooks": {"SessionStart": [{"hooks": [{
            "type": "command",
            "command": shlex.join([sys.executable, str(capture), str(event_path)]),
        }]}]},
    }
    command = [
        "bwrap", "--die-with-parent", "--unshare-net", "--ro-bind", "/", "/",
        "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/etc",
        "--bind", directory, directory, "--clearenv",
        "--setenv", "HOME", str(Path.home()),
        "--setenv", "PATH", os.defpath,
        "--setenv", "TMPDIR", directory,
        "--setenv", "CLAUDE_CONFIG_DIR", str(profile),
        "--setenv", "ANTHROPIC_API_KEY", "offline-fixture",
        "--setenv", "ANTHROPIC_BASE_URL", "http://127.0.0.1:9",
        "--setenv", "DISABLE_NONESSENTIAL_MODEL_CALLS", "1",
        "--chdir", str(workspace), str(claude),
        "--settings", json.dumps(settings), "--setting-sources", "",
        "--max-turns", "1", "-p", "Offline fixture",
    ]
    session_id = str(uuid.uuid4())
    for flag, source in [("--session-id", "startup"), ("--resume", "resume")]:
        event_path.unlink(missing_ok=True)
        with subprocess.Popen([*command, flag, session_id], stdout=subprocess.DEVNULL) as child:
            try:
                deadline = time.monotonic() + 15
                while not event_path.exists() and child.poll() is None and time.monotonic() < deadline:
                    time.sleep(0.05)
                assert event_path.exists(), f"Claude did not emit {source} (exit {child.poll()})"
                assert json.loads(event_path.read_text()) == {
                    "hook_event_name": "SessionStart", "session_id": session_id, "source": source,
                }
                if source == "startup":
                    deadline = time.monotonic() + 4
                    while not list(profile.rglob(session_id + ".jsonl")) and time.monotonic() < deadline:
                        time.sleep(0.05)
                    assert list(profile.rglob(session_id + ".jsonl")), "Claude did not save the session"
            finally:
                if child.poll() is None:
                    child.terminate()
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()

    # The SSH runs keep the hook explicit: automatic hooks are added to local launches only.
    runs = [(["--cells"], "1"), (["--cells"], "0"), (["--ssh", "--cells"], "0")]
    for presentation, automatic in (runs if args.starflux else []):
        subprocess.run(
            [sys.executable, str(Path(__file__).with_name("test-session-manager.py")),
             "--agent-lifecycle", *presentation],
            env={
                **os.environ,
                "STARFLUX_TEST_APP": str(args.starflux.resolve(strict=True)),
                "STARFLUX_TEST_CLAUDE_RESUME": json.dumps([*command, "--resume", session_id]),
                "STARFLUX_TEST_CLAUDE_AUTOMATIC": automatic,
            },
            check=True,
        )

print(json.dumps({"startup_id_confirmed": True, "exact_resume_id_confirmed": True, "network_disabled": True}))
