#!/usr/bin/env python3
"""A real private SSH shell must never execute or record late Kitty replies."""
import argparse
import base64
import getpass
import json
import os
from pathlib import Path
import pty
import re
import select
import shlex
import shutil
import socket
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--app', type=Path)
parser.add_argument('--startup-error', action='store_true')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
app = args.app or root / 'scripts/run-local.sh'
with tempfile.TemporaryDirectory(prefix='starflux-bridge-history-') as directory:
    tmp = Path(directory)
    runtime = tmp / 'runtime'
    runtime.mkdir(mode=0o700)
    state = runtime / 'starflux'
    state.mkdir(mode=0o700)
    name = 'bridge-' + os.urandom(4).hex()
    env = {**os.environ, 'XDG_RUNTIME_DIR': str(runtime), 'XDG_DATA_HOME': str(tmp / 'data'), 'SHELL': '/bin/bash'}
    (state / (name + '.json')).write_text(json.dumps(dict(workspace=name, host=None, directory=str(tmp), agent=None, executable=str(app), project_host=True)))
    for key in ('host', 'client'):
        subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(tmp / key)], check=True)
    (tmp / 'authorized_keys').write_text((tmp / 'client.pub').read_text())
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    (tmp / 'known_hosts').write_text(f'[127.0.0.1]:{port} ' + (tmp / 'host.pub').read_text())
    (tmp / 'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
    log = (tmp / 'sshd.log').open('w')
    daemon = subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd', '-D', '-e', '-f', str(tmp / 'sshd_config')], stderr=log)
    master = None
    ssh = None
    try:
        deadline = time.monotonic() + 5
        while True:
            try:
                with socket.create_connection(('127.0.0.1', port), timeout=.1): break
            except OSError:
                assert daemon.poll() is None and time.monotonic() < deadline
                time.sleep(.02)
        master, slave = pty.openpty()
        history = tmp / 'history'
        remote = shlex.join(['env', 'TERM=xterm-kitty', 'PS1=HISTORY_READY> ', 'HISTCONTROL=', 'HISTFILE=' + str(history), 'XDG_RUNTIME_DIR=' + str(runtime), 'XDG_DATA_HOME=' + str(tmp / 'data'), '/bin/bash', '--noprofile', '--norc', '-i'])
        ssh = subprocess.Popen(['ssh', '-tt', '-F', '/dev/null', '-p', str(port), '-i', str(tmp / 'client'), '-o', 'IdentitiesOnly=yes', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes', '-o', 'UserKnownHostsFile=' + str(tmp / 'known_hosts'), '127.0.0.1', remote], stdin=slave, stdout=slave, stderr=slave)
        os.close(slave)
        output = bytearray()
        pending = bytearray()
        nonce = None
        started = False
        closing = False
        stopped = False
        reply_at = None
        replied_at = None
        submitted = False
        close_at = None
        pattern = re.compile(rb'\x1b\]1337;SetUserVar=star_kit=([A-Za-z0-9+/=]+)\x1b\\')
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            if select.select([master], [], [], .02)[0]:
                try: chunk = os.read(master, 65536)
                except OSError: break
                if not chunk: break
                output.extend(chunk)
                pending.extend(chunk)
            if not started and b'HISTORY_READY> ' in output:
                launch = [str(app), '--workspace', name]
                if args.startup_error: launch += ['--frontend', 'invalid']
                os.write(master, (shlex.join(launch) + '\n').encode())
                started = True
            while match := pattern.search(pending):
                message = json.loads(base64.b64decode(match[1]))
                del pending[:match.end()]
                nonce = message['nonce']
                if message['op'] == 'probe':
                    os.write(master, f'STAR_KIT_BRIDGE {nonce} ready\n'.encode())
                elif message['op'] == 'start':
                    hello = dict(type='hello', version=1, viewport=dict(columns=80, rows=24, width=800, height=480, generation=1), client='history-proof', capabilities=dict(image_transport='kitty', pixel_geometry='measured', pointer_precision='cells', native_terminal_cells=True, keyboard=True, paste=True, presentation_ack=True, native_surfaces=True, physical_text=True, pixel_layout=True))
                    os.write(master, f'STAR_KIT_BRIDGE {nonce} {json.dumps(hello)}\n'.encode())
                    close_at = time.monotonic() + .3
                elif message['op'] == 'stop':
                    # Reproduce a real SSH/Kitty teardown reply arriving after
                    # the SDK relay has returned and restored termios.
                    stopped = True
                    queued = json.dumps(dict(type='input', id=1, revision=1, generation=1, input=dict(kind='key', code='up', modifiers=0)))
                    os.write(master, f'STAR_KIT_BRIDGE {nonce} {queued}\n'.encode())
                    reply_at = time.monotonic() + .25
            if close_at and not closing and time.monotonic() >= close_at:
                for path in state.glob('v-*.state.json'):
                    path.with_name(path.name.removesuffix('.state.json') + '.close').touch()
                closing = True
            if reply_at and time.monotonic() >= reply_at:
                os.write(master, f'STAR_KIT_BRIDGE {nonce} gone\n'.encode())
                replied_at = time.monotonic()
                reply_at = None
            if replied_at and not submitted and time.monotonic() - replied_at > .5:
                os.write(master, b'history -w\nexit\n')
                submitted = True
            if ssh.poll() is not None: break
        ssh.wait(timeout=3)
        assert stopped and submitted, 'Private bridge did not complete teardown'
        recorded = history.read_text()
        assert 'STAR_KIT_BRIDGE' not in recorded, 'Internal reply leaked into SSH shell history'
        assert b'STAR_KIT_BRIDGE' not in output, 'Internal reply leaked into terminal scrollback'
        assert 'history -w' in recorded, 'Shell did not restore normal input after disconnect'
        assert b'HISTORY_READY> history -w' in output, 'Shell echo was not restored'
        print(json.dumps(dict(result='PASS', ssh=True, startup_error=args.startup_error, delayed_close_reply=True, shell_history_clean=True, terminal_scrollback_clean=True, shell_input_restored=True)))
    finally:
        if ssh and ssh.poll() is None:
            ssh.terminate()
            ssh.wait(timeout=3)
        if master is not None: os.close(master)
        for path in state.glob('v-*.state.json'):
            path.with_name(path.name.removesuffix('.state.json') + '.close').touch()
        subprocess.run(['tmux', '-L', 'starflux-' + name, 'kill-server'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
        daemon.terminate()
        daemon.wait(timeout=3)
        log.close()
