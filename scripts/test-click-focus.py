#!/usr/bin/env python3
"""Opening a window or clicking the sidebar leaves typing for the terminals; no GUI.

Private protocol frontends attach to fake terminals. Opening a window and
clicking another project ask the frontend to give the terminal canvas the keyboard; a
click that opens a dialog does not, and a frontend that cannot take the
request never receives it.
"""
import json
import os
from pathlib import Path
import queue
import secrets
import subprocess
import threading
import time

root = Path(__file__).resolve().parents[1]
app = Path(os.environ.get('STARFLUX_TEST_APP', str(root / 'scripts/run-local.sh'))).resolve()
COLS, ROWS = 160, 48

from proof_directory import ProofDirectory
with ProofDirectory(prefix='starflux-click-focus-') as tmp:
    tmp = Path(tmp)
    runtime = tmp / 'runtime'
    runtime.mkdir(mode=0o700)
    state = runtime / 'starflux'
    state.mkdir(mode=0o700)
    workspace = 'focus-' + secrets.token_hex(4)
    other = workspace + '-other'
    shell = tmp / 'shell'
    shell.write_text("#!/bin/sh\nprintf 'FOCUS_TERMINAL_READY\\r\\n'\nexec cat\n")
    shell.chmod(0o700)
    config = dict(workspace=workspace, host=None, directory=str(tmp), agent=None,
                  executable=str(app), project_host=True)
    for name in (workspace, other):
        (state / (name + '.json')).write_text(json.dumps({**config, 'workspace': name}))
    env = {**os.environ, 'XDG_RUNTIME_DIR': str(runtime), 'XDG_DATA_HOME': str(tmp / 'data'),
           'XDG_CONFIG_HOME': str(tmp / 'config'), 'SHELL': str(shell),
           'STAR_TERMINAL_RENDERER': 'native'}
    processes = []

    class Client:
        def __init__(self, identity, focus):
            self.identity = identity
            self.messages = queue.Queue()
            self.scene = None
            self.focus = []
            self.log = (tmp / (identity + '.log')).open('a')
            self.process = subprocess.Popen(
                [str(app), '--graphical-relay', workspace + '@' + identity], env=env,
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log)
            processes.append(self.process)

            def receive():
                for line in self.process.stdout:
                    try:
                        self.messages.put(json.loads(line))
                    except ValueError:
                        self.messages.put({'type': 'bad', 'line': repr(line)})
            threading.Thread(target=receive, daemon=True).start()
            self.send(dict(type='hello', version=1, client=identity,
                           viewport=dict(columns=COLS, rows=ROWS, width=COLS * 10,
                                         height=ROWS * 20, generation=1),
                           capabilities=dict(native_terminal_cells=True,
                                             native_font_millipoints=12500,
                                             image_transport='kitty', pixel_geometry='measured',
                                             pointer_precision='cells', keyboard=True, paste=True,
                                             presentation_ack=True, native_surfaces=True,
                                             physical_text=True, pixel_layout=True,
                                             canvas_focus_v1=focus)))

        def send(self, message):
            self.process.stdin.write(json.dumps(message).encode() + b'\n')
            self.process.stdin.flush()

        def pump(self):
            while True:
                try:
                    m = self.messages.get_nowait()
                except queue.Empty:
                    return
                if m['type'] == 'scene':
                    self.scene = m['scene']
                    self.send(dict(type='presented', revision=self.scene['revision'],
                                   generation=self.scene['viewport']['generation']))
                elif m['type'] == 'focus_canvas':
                    self.focus.append(m['canvas'])
                elif m['type'] in ('error', 'bad'):
                    raise AssertionError(m)

        def input(self, event):
            self.send(dict(type='input', id=time.monotonic_ns(), revision=self.scene['revision'],
                           generation=self.scene['viewport']['generation'], input=event))

        def hit(self, action):
            for c in (self.scene or {}).get('components', []):
                if c.get('kind') != 'surface':
                    continue
                for h in c['surface'].get('hits', []):
                    if h['action'] == action:
                        r = h['rect']
                        return r['x'] + r['width'] // 2, r['y'] + r['height'] // 2

        def canvas(self):
            return any(c.get('kind') == 'live_canvas' and c.get('id') == 'terminals'
                       for c in (self.scene or {}).get('components', []))

        def click(self, point):
            x, y = point
            for action in ('down', 'up'):
                self.input(dict(kind='pointer', action=action, button=0, x=x // 10, y=y // 20,
                                pixel=[x, y], modifiers=0))

        def snapshot(self):
            try:
                return json.loads((state / (self.identity + '.state.json')).read_text())
            except (OSError, ValueError):
                return {}

    clients = []

    def wait(predicate, label, timeout=15):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for c in clients:
                c.pump()
            result = predicate()
            if result:
                return result
            time.sleep(.02)
        raise AssertionError(label + ': ' + repr([
            dict(frontend=c.identity, exit=c.process.poll(), focus=c.focus,
                 stderr=(tmp / (c.identity + '.log')).read_text()[-2000:]) for c in clients]))

    def settle(seconds=1):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            for c in clients:
                c.pump()
            time.sleep(.02)

    try:
        a = Client('v-a-' + secrets.token_hex(3), True)
        clients.append(a)
        wait(lambda: a.canvas() and a.hit('project:' + other), 'Terminal canvas and project row')
        # A window opens already focused, so it gets the request right away.
        wait(lambda: 'terminals' in a.focus, 'Focus request when the window opens')
        settle(.5)
        a.focus.clear()

        # A project row click leaves the keyboard with the terminals.
        a.click(a.hit('project:' + other))
        wait(lambda: 'terminals' in a.focus, 'Project click focus request')
        wait(lambda: a.snapshot().get('active_project') == other, 'Project switch')

        # A click that opens a dialog keeps the keyboard for the dialog.
        settle(.5)
        a.focus.clear()
        wait(lambda: a.hit('project:new'), 'Add-project button')
        a.click(a.hit('project:new'))
        settle()
        assert not a.focus, 'A dialog click gave the terminals the keyboard'
        a.input(dict(kind='key', code='escape', modifiers=0))
        settle(.5)

        # A frontend without the capability never receives the request.
        b = Client('v-b-' + secrets.token_hex(3), False)
        clients.append(b)
        wait(lambda: b.canvas() and b.hit('project:' + workspace), 'Second frontend scene')
        b.click(b.hit('project:' + workspace))
        wait(lambda: b.snapshot().get('active_project') == workspace, 'Second frontend switch')
        settle()
        assert not b.focus, 'A frontend without canvas_focus_v1 received a focus request'
        print('PASS: opening a window and sidebar clicks keep typing in the terminals; dialogs and older frontends are left alone')
    finally:
        for p in processes:
            if p.poll() is None:
                try:
                    p.stdin.close()
                except OSError:
                    pass
        for p in processes:
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                p.kill()
        # Stop only this test's controllers, native owner and supervisors.
        for descriptor in state.glob('*.frontend.json'):
            try:
                identity = json.loads(descriptor.read_text())['id']
                (state / (identity + '.close')).touch()
            except (OSError, ValueError, KeyError):
                pass
        doomed = []
        try:
            owner = tmp / 'data/starflux/native-sources/owner.json'
            doomed.append(json.loads(owner.read_text())['owner']['pid'])
        except (OSError, ValueError, KeyError, TypeError):
            pass
        for proc in Path('/proc').iterdir():
            try:
                if (proc.name.isdigit()
                        and b'--native-source-supervisor' in (proc / 'cmdline').read_bytes()
                        and Path(os.readlink(proc / 'cwd')) == tmp):
                    doomed.append(int(proc.name))
            except OSError:
                pass
        for pid in doomed:
            try:
                os.kill(pid, 9)
            except OSError:
                pass
