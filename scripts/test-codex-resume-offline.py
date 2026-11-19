#!/usr/bin/env python3
"""Check the real Codex app-server exact-ID resume contract in a disposable profile."""
import argparse
import json
import os
import pty
import selectors
import shlex
import subprocess
import tempfile
import sys
import termios
import time
import uuid
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("codex", type=Path, help="Path to the installed Codex executable")
parser.add_argument("--starflux", type=Path, help="Also verify STAR/FLUX launches the registered Codex command")
parser.add_argument("--cells", action="store_true", help="Use the STAR/FLUX cells frontend")
parser.add_argument("--ssh", action="store_true", help="Run the STAR/FLUX fixture over loopback SSH")
args = parser.parse_args()
if (args.cells or args.ssh) and not args.starflux:
    parser.error("--cells and --ssh require --starflux")
codex = args.codex.resolve(strict=True)
profile_mount = Path.home() / ".codex"
if not profile_mount.is_dir() or profile_mount.is_symlink():
    parser.error("A real ~/.codex directory is required as a private mount point")

# Pass only the variables needed to find executables and resolve the default
# profile mount. In particular, no authentication or alternate profile is used.
environment = {key: os.environ[key] for key in ("HOME", "PATH", "LANG") if key in os.environ}

with tempfile.TemporaryDirectory(prefix='starflux-provider-') as directory:
    root = Path(directory)
    profile = root / 'profile'
    profile.mkdir()
    (profile / "config.toml").write_text(
        'model_provider = "offline-fixture"\n'
        '[model_providers.offline-fixture]\n'
        'name = "Offline fixture"\n'
        'base_url = "http://127.0.0.1:9/v1"\n'
        'wire_api = "responses"\n'
    )
    workspace = root / 'workspace'
    workspace.mkdir()
    command = ['bwrap', '--die-with-parent', '--unshare-net', '--ro-bind', '/', '/', '--dev', '/dev',
                '--proc', '/proc', '--clearenv',
               '--setenv', 'HOME', str(Path.home()),
               '--setenv', 'PATH', os.defpath,
               '--setenv', 'TERM', 'xterm-256color',
               '--bind', directory, directory,
               '--bind', str(profile), str(profile_mount),
               '--chdir', str(workspace), str(codex)]

    def server(action, launch=None):
        with subprocess.Popen([*(launch or command), "app-server", "--listen", "stdio://"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              stderr=subprocess.DEVNULL, bufsize=0, env=environment) as child:
            with selectors.DefaultSelector() as selector:
                selector.register(child.stdout, selectors.EVENT_READ)
                def send(message):
                    child.stdin.write((json.dumps(message)+'\n').encode())
                def call(method, params, *, expect_error=False):
                    call.serial += 1
                    send(dict(id=call.serial, method=method, params=params))
                    for _ in range(100):
                        if not selector.select(10):
                            raise RuntimeError(f'{method} timed out')
                        line = child.stdout.readline()
                        if not line:
                            raise RuntimeError(f'{method} connection ended')
                        response = json.loads(line)
                        if response.get('id') == call.serial:
                            if expect_error:
                                if "error" not in response:
                                    raise AssertionError(f"{method} accepted an unknown thread ID")
                                return response["error"]
                            if 'error' in response:
                                raise RuntimeError(f"{method}: {response['error']}")
                            return response['result']
                    raise RuntimeError(f'{method} response missing')
                call.serial = 0
                try:
                    call('initialize', {'clientInfo': {'name':'starflux-fixture','version':'0.1'},
                                        'capabilities': {'experimentalApi':True}})
                    send({'method':'initialized'})
                    return action(call)
                finally:
                    child.stdin.close()
                    try:
                        child.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait()
    def start(call):
        result = call('thread/start', {'cwd':str(workspace), 'ephemeral':False})
        thread = result['thread']['id']
        call('thread/inject_items', {'threadId':thread, 'items':[
            {'type':'message', 'role':'user','content':[{'type':'input_text','text':'STARFLUX_OFFLINE_FIXTURE'}]}
        ]})
        return thread
    thread_id = server(start)
    def resume(call):
        result = call('thread/resume', {'threadId':thread_id})
        assert result['thread']['id'] == thread_id
        call('thread/resume', {'threadId':str(uuid.uuid4())}, expect_error=True)
    server(resume)

    system_hooks = root / "system-codex"
    system_hooks.mkdir()
    captured_hook = root / "session-start.json"
    capture = root / "capture-hook.py"
    capture.write_text(
        "import json, sys\n"
        "from pathlib import Path\n"
        "event = json.load(sys.stdin)\n"
        "fields = ('hook_event_name', 'source', 'session_id')\n"
        f"destination = Path({str(captured_hook)!r})\n"
        "temporary = destination.with_suffix('.tmp')\n"
        "temporary.write_text(json.dumps({key: event[key] for key in fields}))\n"
        "temporary.replace(destination)\n"
    )
    hook_command = shlex.join([sys.executable, str(capture)])
    (system_hooks / "requirements.toml").write_text(
        '[features]\nhooks = true\n'
        '[[hooks.SessionStart]]\nmatcher = "^resume$"\n'
        '[[hooks.SessionStart.hooks]]\ntype = "command"\n'
        f'command = {json.dumps(hook_command)}\n'
    )
    resume_command = command[:-1] + ["--tmpfs", "/etc", "--ro-bind", str(system_hooks), "/etc/codex", command[-1]]
    def capture_session_start(call):
        call("thread/resume", {"threadId": thread_id})
        # SessionStart runs before processing the first turn. The fixture
        # provider's endpoint is unreachable in this network namespace.
        call("turn/start", {
            "threadId": thread_id,
            "input": [{"type": "text", "text": "Offline lifecycle fixture", "text_elements": []}],
        })
        deadline = time.monotonic() + 5
        while not captured_hook.exists() and time.monotonic() < deadline:
            time.sleep(0.02)
        assert captured_hook.exists(), "Codex did not emit the managed SessionStart hook"

    server(capture_session_start, resume_command)

    master, slave = pty.openpty()
    try:
        termios.tcsetwinsize(slave, (40, 120))
        with subprocess.Popen(
            [*command, "resume", "--no-daemon", "--no-alt-screen", thread_id],
            stdin=slave, stdout=slave, stderr=slave,
            env={**environment, "TERM": "xterm-256color"},
        ) as child:
            try:
                with selectors.DefaultSelector() as selector:
                    selector.register(master, selectors.EVENT_READ)
                    output = bytearray()
                    deadline = time.monotonic() + 20
                    marker = thread_id.encode()
                    status_requested = False
                    status_submitted = False
                    while marker not in output and len(output) < 2 * 1024 * 1024:
                        remaining = deadline - time.monotonic()
                        if remaining <= 0 or not selector.select(remaining):
                            break
                        output.extend(os.read(master, 65536))
                        if not status_requested and b"state=idle:app=codex" in output:
                            os.write(master, b"/status")
                            status_requested = True
                        elif status_requested and not status_submitted and output.count(b"/status") >= 2:
                            os.write(master, b"\r")
                            status_submitted = True
                    if marker not in output:
                        raise AssertionError(
                            f"Interactive resume did not confirm the saved thread ID: {output[-1800:]!r}"
                        )
            finally:
                child.terminate()
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
    finally:
        os.close(slave)
        os.close(master)
    event = json.loads(captured_hook.read_text())
    assert event == {
        "hook_event_name": "SessionStart", "source": "resume", "session_id": thread_id,
    }, event
    print(json.dumps({
        "codex_session_start_confirmed": True,
        "same_thread_after_restart": True,
        "unknown_thread_rejected": True,
        "interactive_thread_id_confirmed": True,
        "network_disabled": True,
    }))

    if args.starflux:
        fixture = Path(__file__).with_name("test-session-manager.py")
        launch = [*command, "resume", "--no-daemon", "--no-alt-screen", thread_id]
        subprocess.run(
            [sys.executable, str(fixture), "--agent-lifecycle",
             *(["--cells"] if args.cells else []), *(["--ssh"] if args.ssh else [])],
            check=True,
            env={
                **os.environ,
                "STARFLUX_TEST_APP": str(args.starflux.resolve(strict=True)),
                "STARFLUX_TEST_CODEX_RESUME": json.dumps(launch),
                "STARFLUX_TEST_CODEX_EVENT": json.dumps(event),
            },
        )
