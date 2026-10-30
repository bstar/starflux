#!/usr/bin/env python3
"""Exercise the real cells frontend in a private PTY; no desktop or live agents."""
import argparse
import fcntl
import getpass
import json
import os
from pathlib import Path
import pty
import queue
import secrets
import shlex
import shutil
import signal
import socket
import struct
import subprocess
import tempfile
import termios
import threading
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--ssh', action='store_true')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
app = root / 'scripts/run-local.sh'
report = root / 'target/cells-proof'
report.mkdir(parents=True, exist_ok=True)
(report / ('ssh.log' if args.ssh else 'local.log')).write_bytes(b'')
with tempfile.TemporaryDirectory(prefix='starflux-cells-') as directory:
    tmp = Path(directory)
    runtime = tmp / 'runtime'
    runtime.mkdir(mode=0o700)
    state = runtime / 'starflux'
    state.mkdir(mode=0o700)
    name = 'cells-' + secrets.token_hex(4)
    fixture = tmp / 'fixture.py'
    fixture.write_text("""import os,sys,tty
from pathlib import Path
tty.setraw(0)
os.write(1, '\\x1b[?2004hCELLS_READY café 世界\\r\\n'.encode())
with open(Path(sys.argv[1])/('input-'+str(os.getpid())), 'ab', buffering=0) as out:
    while True:
        data=os.read(0,65536)
        if not data: break
        out.write(data)
        os.write(1,data)
""")
    shell = tmp / 'shell'
    shell.write_text('#!/bin/sh\nexec python3 ' + shlex.quote(str(fixture)) + ' ' + shlex.quote(str(tmp)) + '\n')
    shell.chmod(0o700)
    env = {k: v for k, v in os.environ.items() if k not in (
        'KITTY_WINDOW_ID', 'KITTY_PID', 'KITTY_LISTEN_ON', 'WAYLAND_DISPLAY', 'DISPLAY',
        'SSH_CONNECTION', 'SSH_TTY', 'TMUX', 'STY', 'STAR_KIT_TERMINAL_SOCKET',
    )}
    env.update(XDG_RUNTIME_DIR=str(runtime), XDG_DATA_HOME=str(tmp / 'data'),
               SHELL=str(shell), TERM='xterm-256color' if args.ssh else 'xterm', STAR_TERMINAL_RENDERER='native' if args.ssh else 'kitty-pixels',
               STARFLUX_KITTY=str(tmp / 'kitty-must-not-run'))
    (state / (name + '.json')).write_text(json.dumps(dict(
        workspace=name, host=None, directory=str(tmp), agent=None,
        executable=str(app), project_host=True,
    )))
    # Legacy app metadata must survive presentation, without worker startup.
    data_root = tmp / 'data/starflux/projects'
    data_root.mkdir(parents=True, mode=0o700)
    legacy = dict(space='apps', terminal_first=True, terminal=0, terminals=[0],
                  app_command='DO_NOT_START_APP', app_targets=[dict(host=None, directory=str(tmp), helper='DO_NOT_START_HELPER')])
    (data_root / (name + '.layout.json')).write_text(json.dumps(legacy))
    processes = []
    created_projects = []
    logs = []
    tty_bytes = bytearray()
    master = None
    sshd = None
    ssh_config = None
    remote = None
    cli = [str(app)]
    if args.ssh:
        for key in ('host', 'client'):
            subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(tmp / key)], check=True)
        (tmp / 'authorized_keys').write_text((tmp / 'client.pub').read_text())
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        (tmp / 'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
        (tmp / 'known_hosts').write_text(f'[127.0.0.1]:{port} ' + (tmp / 'host.pub').read_text())
        ssh_log = (tmp / 'sshd.log').open('w')
        logs.append(ssh_log)
        sshd = subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd', '-D', '-e', '-f', str(tmp / 'sshd_config')], stderr=ssh_log)
        deadline = time.monotonic() + 5
        while True:
            try:
                with socket.create_connection(('127.0.0.1', port), timeout=.1):
                    break
            except OSError:
                assert sshd.poll() is None and time.monotonic() < deadline
                time.sleep(.02)
        ssh_config = tmp / 'ssh_config'
        ssh_config.write_text(f'Host proof\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {tmp}/client\n IdentitiesOnly yes\n BatchMode yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {tmp}/known_hosts\n')
        remote = tmp / 'remote-starflux'
        remote.write_text('#!/bin/sh\nexec env ' + shlex.join([k + '=' + env[k] for k in ('PATH', 'XDG_RUNTIME_DIR', 'XDG_DATA_HOME', 'SHELL', 'STAR_TERMINAL_RENDERER')]) + ' ' + shlex.quote(str(app)) + ' "$@"\n')
        remote.chmod(0o700)
        cli += ['--ssh', 'proof', '--ssh-config', str(ssh_config), '--remote-executable', str(remote)]

    def command(*extra):
        return subprocess.run([str(app), '--workspace', name, '--frontend', 'v-cells-proof', *extra], env=env, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def snapshot(identity='v-cells-proof'):
        try:
            return json.loads((state / (identity + '.state.json')).read_text())
        except (ValueError, OSError):
            return {}

    def inputs():
        return b''.join(p.read_bytes() for p in sorted(tmp.glob('input-*')))

    def wait(predicate, label, timeout=15):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            value = predicate()
            if value:
                return value
            time.sleep(.025)
        raise AssertionError(label + '\n' + json.dumps(snapshot(), indent=2) + '\n' + repr(bytes(tty_bytes[-5000:])))

    def write(data):
        os.write(master, data)

    def resize(cols, rows):
        fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack('HHHH', rows, cols, 0, 0))
        os.kill(frontend.pid, signal.SIGWINCH)

    def start_frontend(mode='--cells', identity='v-cells-proof'):
        global master, frontend
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
        log = (report / ('ssh.log' if args.ssh else 'local.log')).open('ab')
        logs.append(log)
        frontend = subprocess.Popen([*cli, *([mode] if mode else []), '--workspace', name, '--frontend', identity], stdin=slave, stdout=slave, stderr=log, env=env, start_new_session=True)
        processes.append(frontend)
        os.close(slave)
        fd = master
        def read():
            try:
                while True:
                    chunk = os.read(fd, 65536)
                    if not chunk:
                        break
                    tty_bytes.extend(chunk)
            except OSError:
                pass
        threading.Thread(target=read, daemon=True).start()
        return frontend

    try:
        frontend = start_frontend()
        wait(lambda: snapshot().get('presentation') == 'cells' and b'CELLS_READY' in tty_bytes, 'cells frontend did not display native terminal text')
        initial = snapshot()
        assert initial['project_state']['space'] == 'terminals'
        assert initial['project_state']['app_command'] == legacy['app_command']
        assert initial['project_state']['app_targets'] == legacy['app_targets']
        assert not initial['apps'] and not initial['app_view_available']
        assert initial['desktop']['windows'] == 0
        backend_pid = initial['panes'][0]['pid']
        assert 'café 世界'.encode() in tty_bytes
        write(b'qnap[]\x03\x0c')
        wait(lambda: b'qnap[]\x03\x0c' in inputs(), 'workspace shortcut or Ctrl+L stole focused terminal input')
        write(b'\x1b[200~PASTED_BYTES\x1b[201~')
        wait(lambda: b'PASTED_BYTES' in inputs(), 'bracketed paste did not reach terminal')
        assert frontend.poll() is None and snapshot()['project_state']['terminals'] == [0]
        # F6 is available even without enhanced keyboard / Ctrl+Escape support.
        write(b'\x1b[17~')
        wait(lambda: snapshot().get('cell_terminal_focus') is False, 'F6 did not release terminal focus')
        before = inputs()
        write(b'p')
        wait(lambda: snapshot().get('modal_open'), 'keyboard project dialog did not open')
        write(b'UNSUBMITTED_DIALOG_TEXT')
        time.sleep(.2)
        assert inputs() == before, 'dialog input leaked into the terminal'
        write(b'\x1b')
        wait(lambda: not snapshot().get('modal_open'), 'Escape did not dismiss dialog')
        # Pointer feedback is emitted locally for cell controls, then reset over text.
        pointer_offset = len(tty_bytes)
        write(b'\x1b[<35;2;2M')
        wait(lambda: b'\x1b]22;pointer\x1b\\' in tty_bytes[pointer_offset:], 'hand pointer was not emitted over a control')
        pointer_offset = len(tty_bytes)
        write(b'\x1b[<35;40;6M')
        wait(lambda: b'\x1b]22;\x1b\\' in tty_bytes[pointer_offset:], 'hand pointer did not reset over terminal text')
        # Visible project controls, close without killing, and reopen saved sessions.
        assert b'Close' in tty_bytes, 'project close control was not rendered'
        write(b'\x1b[<0;2;2M\x1b[<0;2;2m')
        wait(lambda: snapshot().get('project_navigation', {}).get('sidebar_width', 0) > 0, 'project sidebar did not open')
        menu_offset = len(tty_bytes)
        write(b'\x1b[<2;2;6M\x1b[<2;2;6m')
        wait(lambda: snapshot().get('context_menu') is not None, 'right-click project menu did not open')
        bounds = snapshot()['context_menu']['bounds']
        assert bounds['width'] < 80 and bounds['height'] < 24, bounds
        wait(lambda: b'CELLS_READY' in tty_bytes[menu_offset:], 'compact menu erased its terminal backdrop')
        write(f"\x1b[<35;{bounds['x']+3};{bounds['y']+2}M".encode())
        wait(lambda: snapshot().get('context_menu', {}).get('selected') == 0, 'menu hover did not highlight an item')
        write(b'\x1b[<0;13;2M\x1b[<0;13;2m')
        wait(lambda: snapshot().get('context_menu') is None, 'outside click did not dismiss the menu')
        assert frontend.poll() is None and snapshot()['active_project'] == name, 'outside click activated Close project behind the menu'
        # Popup positioning stays inside the terminal even at its lower-right edge.
        write(b'\x1b[<2;79;21M\x1b[<2;79;21m')
        wait(lambda: snapshot().get('context_menu') is not None, 'terminal context menu did not open')
        actions={item['action'] for item in snapshot()['context_menu']['items']}
        assert 'reconnect-terminal:0' in actions and 'recover-terminal:0' in actions, actions
        bounds = snapshot()['context_menu']['bounds']
        assert bounds['x'] + bounds['width'] <= 80 and bounds['y'] + bounds['height'] <= 24, bounds
        write(b'\x1b')
        wait(lambda: snapshot().get('context_menu') is None, 'Escape did not dismiss the popup')
        # Recovery is reachable for live/hung programs, and cancellation never
        # sends dialog input to the retained program, locally or over SSH.
        before=inputs()
        command('--action','recover-terminal:0')
        wait(lambda: snapshot().get('modal_open'), 'recovery confirmation did not open')
        write(b'\x1b')
        wait(lambda: not snapshot().get('modal_open'), 'recovery did not cancel')
        assert inputs()==before and snapshot()['panes'][0]['pid']==backend_pid
        attachment=snapshot()['direct_terminals'][0]['attachment_pid']
        command('--action','reconnect-terminal:0')
        wait(lambda: snapshot()['direct_terminals'] and snapshot()['direct_terminals'][0]['attachment_pid']!=attachment, 'pane did not reconnect')
        assert snapshot()['panes'][0]['pid']==backend_pid, 'reconnect replaced the running program'
        write(b'\x1b[<0;2;22M\x1b[<0;2;22m')
        wait(lambda: snapshot().get('modal_open'), 'bottom Add project button did not open the dialog')
        write(b'\x1b')
        wait(lambda: not snapshot().get('modal_open'), 'Add project dialog did not cancel')
        assert b'[Previous]' not in tty_bytes and b'[Next]' not in tty_bytes, 'obsolete project paging buttons remain'
        write(b'\x1b[<0;2;2M\x1b[<0;2;2m')
        wait(lambda: snapshot().get('project_navigation', {}).get('sidebar_width') == 0, 'project sidebar did not collapse')
        added_project = 'added-' + secrets.token_hex(4)
        created_projects.append(added_project)
        def add_project():
            write(b'+')
            wait(lambda: snapshot().get('modal_open'), 'Add project did not open')
            write(b'\x1b[200~' + added_project.encode() + b'\x1b[201~\t\t\r')
            wait(lambda: snapshot().get('active_project') == added_project and not snapshot().get('modal_open'), 'project did not open')
        add_project()
        wait(lambda: snapshot().get('panes', [{}])[0].get('pid'), 'new project shell did not start')
        added_pid = snapshot()['panes'][0]['pid']
        saved_config = (state / (added_project + '.json')).read_bytes()
        write(b'x')
        wait(lambda: snapshot().get('active_project') == name and added_project not in snapshot().get('project_order', []), 'Close project did not remove the project')
        os.kill(added_pid, 0)
        (data_root / 'catalog-version').write_text(secrets.token_hex(8))
        time.sleep(1)
        assert added_project not in snapshot()['project_order'], 'catalog reload reopened a closed project'
        assert (state / (added_project + '.json')).read_bytes() == saved_config
        add_project()
        wait(lambda: snapshot()['panes'][0].get('pid') == added_pid, 'reopening a saved project restarted its shell')
        write(b'x')
        wait(lambda: snapshot().get('active_project') == name and added_project not in snapshot().get('project_order', []), 'reopened project did not close')
        os.kill(added_pid, 0)
        # Keyboard creation, splitting and resizing preserve session identity.
        # The file dialog uses the same transfer and non-submitting paste path.
        delivery = tmp / 'delivered file.txt'
        delivery.write_text('CELLS_FILE_DELIVERY')
        file_input_offset = len(inputs())
        write(b'f')
        wait(lambda: snapshot().get('modal_open'), 'file dialog did not open')
        write(b'\x1b[200~' + str(delivery).encode() + b'\x1b[201~')
        write(b'\r')
        wait(lambda: b'delivered' in inputs(), 'file reference was not delivered')
        assert b'\r' not in inputs()[file_input_offset:] and b'\n' not in inputs()[file_input_offset:], 'file delivery submitted terminal input'
        wait(lambda: not snapshot().get('modal_open'), 'delivery dialog did not close')
        write(b'g')
        wait(lambda: snapshot().get('git_open'), 'Git overlay did not open')
        write(b'\x1b')
        wait(lambda: not snapshot().get('git_open'), 'Git overlay did not close')
        write(b'n')
        wait(lambda: len(snapshot().get('project_state', {}).get('terminals', [])) == 2, 'new tab failed')
        command('--action', 'terminal:split')
        wait(lambda: snapshot().get('project_state', {}).get('split') and len(snapshot()['project_state']['terminals']) == 3, 'split failed')
        terminal_count = 3
        resize(120, 35)
        wait(lambda: snapshot().get('viewport', {}).get('columns') == 120, 'resize was not negotiated')
        resize(30, 10)
        wait(lambda: b'Resize to at least' in tty_bytes, 'tiny-terminal fallback did not render')
        resize(80, 24)
        wait(lambda: snapshot().get('viewport', {}).get('columns') == 80, 'restore from tiny terminal failed')
        assert len(snapshot()['project_state']['terminals']) == terminal_count
        write(b'\x1b[<0;40;6M\x1b[<32;50;6M\x1b[<0;50;6m')
        wait(lambda: snapshot().get('project_state', {}).get('split_ratio', .5) > .55, 'mouse split resizing failed')
        command('--action', 'terminal:close-pane')
        wait(lambda: not snapshot().get('project_state', {}).get('split'), 'close pane failed')
        assert len(snapshot()['project_state']['terminals']) == terminal_count
        write(b'\x1b[<0;1;5M\x1b[<32;7;5M\x1b[<0;7;5m')
        wait(lambda: b'\x1b]52;c;' in tty_bytes, 'selection did not emit parent-terminal clipboard data')
        wait(lambda: snapshot().get('cell_terminal_focus') is True, 'mouse did not restore terminal focus')
        # OSC 72 hit testing must use the cell canvas, not the graphical layout.
        write(b'\x1b]72;t=q\x1b\\')
        write(b'\x1b]72;t=m:x=2:y=5:o=1:i=1;text/uri-list\x1b\\')
        wait(lambda: b'\x1b]72;t=m:o=1:i=1' in tty_bytes, 'cell terminal did not accept the file-drop hover')
        # Hidden actions must be guarded, including actions from stale clients.
        for action in ('view:apps', 'launch-app', 'run-app', 'audio:toggle', 'project-space:apps:' + name):
            command('--action', action)
            wait(lambda: 'App view is unavailable' in snapshot().get('status', ''), 'hidden action was not rejected')
            assert snapshot()['project_state']['space'] == 'terminals'
        # A real second frontend protocol shares terminals with independent presentation.
        graphical = subprocess.Popen([str(app), '--graphical-relay', name + '@v-pixels-proof'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env={**env, 'STAR_TERMINAL_RENDERER': 'native'})
        processes.append(graphical)
        messages = queue.Queue()
        def receive():
            for line in graphical.stdout:
                messages.put(json.loads(line))
        threading.Thread(target=receive, daemon=True).start()
        graphical.stdin.write(json.dumps(dict(type='hello', version=1, client='pixels-proof', viewport=dict(columns=100, rows=35, width=1000, height=700, generation=1), capabilities=dict(native_terminal_cells=True, cell_presentation=False, image_transport='kitty', pixel_geometry='measured', pointer_precision='cells', keyboard=True, paste=True, presentation_ack=True, native_surfaces=True, physical_text=True, pixel_layout=True))).encode() + b'\n')
        graphical.stdin.flush()
        def graphical_ready():
            while not messages.empty():
                message = messages.get_nowait()
                if message['type'] == 'scene':
                    scene = message['scene']
                    graphical.stdin.write(json.dumps(dict(type='presented', revision=scene['revision'], generation=scene['viewport']['generation'])).encode() + b'\n')
                    graphical.stdin.flush()
                    for component in scene['components']:
                        if component['kind'] == 'surface':
                            actions = [hit['action'] for hit in component['surface'].get('hits', [])]
                            assert not any(a in ('view:apps', 'launch-app', 'run-app') or a.startswith(('app:', 'apps:', 'audio:', 'project-space:apps:')) for a in actions), actions
            return snapshot('v-pixels-proof').get('presentation') == 'graphical'
        wait(graphical_ready, 'mixed presentation frontend did not attach')
        assert snapshot()['project_state']['terminals'] == snapshot('v-pixels-proof')['project_state']['terminals']
        # Refresh the real presenting process; its mode must survive re-exec.
        old_controller = snapshot()['controller_pid']
        write(b'\x1b[15~')  # F5
        wait(lambda: snapshot().get('controller_pid') != old_controller and snapshot().get('presentation') == 'cells', 'refresh lost cells presentation', timeout=25)
        assert frontend.poll() is None
        assert len(snapshot()['project_state']['terminals']) == terminal_count
        assert snapshot()['panes'][0]['pid'] == backend_pid
        assert added_project not in snapshot()['project_order'], 'refresh reopened a closed project'
        assert snapshot()['project_state']['app_command'] == legacy['app_command']
        # Close, reconnect automatically in an ordinary terminal, and keep sessions.
        command('--close')
        frontend.wait(timeout=10)
        os.close(master)
        master = None
        tty_bytes.clear()
        frontend = start_frontend(mode='--cells' if args.ssh else '')
        wait(lambda: snapshot().get('presentation') == 'cells' and b'CELLS_READY' in tty_bytes, 'reattachment did not redraw retained terminal')
        assert len(snapshot()['project_state']['terminals']) == terminal_count
        command('--action', 'project:close')
        frontend.wait(timeout=10)
        os.kill(backend_pid, 0)
        os.kill(added_pid, 0)
        command('--close')
        frontend.wait(timeout=10)
        os.close(master)
        master = None
        unsupported = start_frontend(mode='--graphical', identity='v-unsupported-proof')
        wait(lambda: unsupported.poll() is not None, 'explicit graphical mode did not fail in an ordinary terminal')
        assert unsupported.returncode != 0
        for log in logs:
            log.flush()
        assert b'Graphical presentation is unavailable' in (report / ('ssh.log' if args.ssh else 'local.log')).read_bytes()
        conflict = subprocess.run([str(app), '--cells', '--graphical'], env=env, capture_output=True)
        assert conflict.returncode == 2
        result = dict(result='PASS', transport='ssh' if args.ssh else 'local', unicode=True, parent_terminal=env['TERM'], cells=True, project_add_close_reopen=True, pointer_feedback=True, compact_context_menus=True, mixed_frontends=True, refresh=True, reattach=True, app_view_hidden=True)
        (report / ('ssh.json' if args.ssh else 'local.json')).write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result))
    finally:
        for path in state.glob('*.log'):
            (report / (('ssh-' if args.ssh else 'local-') + path.name)).write_bytes(path.read_bytes())
        for identity in ('v-cells-proof', 'v-pixels-proof', 'v-unsupported-proof'):
            (state / (identity + '.close')).touch()
        for process in processes:
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=5)
        if master is not None:
            os.close(master)
        for project in [name, *created_projects]:
            subprocess.run(['tmux', '-L', 'starflux-' + project, 'kill-server'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if sshd:
            sshd.terminate()
            sshd.wait(timeout=5)
        for log in logs:
            log.close()
