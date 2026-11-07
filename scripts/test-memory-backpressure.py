#!/usr/bin/env python3
"""Bound a slow view's terminal queue using only a disposable fake session."""
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='starflux-memory-proof-') as temporary:
    temporary = Path(temporary)
    runtime = temporary / 'runtime'
    state = runtime / 'starflux'
    state.mkdir(parents=True, mode=0o700)
    runtime.chmod(0o700)
    name = 'memory-' + secrets.token_hex(6)
    source = temporary / 'source.py'
    source.write_text('''import os,sys,time
from pathlib import Path
Path(os.environ['PROOF_PID']).write_text(str(os.getpid()))
n=0
while not Path(os.environ['PROOF_STOP']).exists():
 sys.stdout.write('\\033[H'+(('NATIVE_MEMORY_PROOF '+chr(65+n%26)*180+'\\r\\n')*68));sys.stdout.flush()
 n+=1;time.sleep(.02)
print('\\r\\nBACKPRESSURE_RECOVERED',flush=True)
os.execvp('cat',['cat'])
''')
    shell = temporary / 'shell'
    shell.write_text('#!/bin/sh\nexec /usr/bin/python3 -u "' + str(source) + '"\n')
    shell.chmod(0o700)
    executable = str(root / 'scripts/run-local.sh')
    config = dict(workspace=name, host=None, directory=str(temporary), agent=None,
                  executable=executable, project_host=True)
    (state / (name + '.json')).write_text(json.dumps(config))
    stop_output = temporary / 'stop-output'
    source_pid = temporary / 'source-pid'
    env = {**os.environ, 'XDG_RUNTIME_DIR': str(runtime),
           'XDG_DATA_HOME': str(temporary / 'data'),
           'XDG_CONFIG_HOME': str(temporary / 'config'), 'SHELL': str(shell),
           'STAR_TERMINAL_RENDERER': 'native', 'PROOF_STOP': str(stop_output),
           'PROOF_PID': str(source_pid)}
    peer = None
    with (temporary / 'controller.log').open('w+') as log:
        process = subprocess.Popen([executable, '--serve', name], env=env,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=log)
        try:
            deadline = time.monotonic() + 20
            path = state / (name + '.sock')
            while not path.exists():
                assert process.poll() is None and time.monotonic() < deadline
                time.sleep(.02)
            peer = socket.socket(socket.AF_UNIX)
            peer.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4096)
            peer.connect(str(path))
            peer.settimeout(.1)
            def send(message):
                peer.sendall(json.dumps(message).encode() + b'\n')
            send(dict(type='hello', version=1, client='memory-proof',
                      viewport=dict(columns=210, rows=75, width=2100, height=1500, generation=1),
                      capabilities=dict(native_terminal_cells=True, image_transport='kitty',
                                        pixel_geometry='measured', pointer_precision='cells',
                                        keyboard=True, paste=True, presentation_ack=True,
                                        native_surfaces=True, physical_text=True, pixel_layout=True)))
            snapshot = state / (name + '.state.json')
            while not source_pid.exists() or not snapshot.exists():
                assert time.monotonic() < deadline
                peer.recv(65536)
            pid = int(source_pid.read_text())
            start = Path('/proc', str(pid), 'stat').read_text().rsplit(')', 1)[1].split()[19]
            peak = 0
            measured = False
            began = time.monotonic()
            # Keep consuming slowly enough to back up reliable output, while
            # making progress often enough to avoid the socket write timeout.
            while time.monotonic() - began < 15:
                try:
                    peer.recv(1024)
                except TimeoutError:
                    pass
                try:
                    data = json.loads(snapshot.read_text())
                    queued = data['terminal_presentation'].get('queued_cell_frames')
                    if queued is not None:
                        measured = True
                        peak = max(peak, queued)
                except (FileNotFoundError, json.JSONDecodeError):
                    pass
                time.sleep(.03)
            assert measured, 'Controller must expose its queued frame count'
            assert peak <= 1, f'One pane retained {peak} unsent frames'
            stop_output.touch()
            pending = b''
            recovered = False
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline and not recovered:
                try:
                    pending += peer.recv(65536)
                except TimeoutError:
                    continue
                while b'\n' in pending:
                    line, pending = pending.split(b'\n', 1)
                    # The slow-consumer phase ended mid-message.
                    try:
                        message = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    if message['type'] == 'scene':
                        scene = message['scene']
                        send(dict(type='presented', revision=scene['revision'], generation=1))
                    if message['type'] == 'terminal_cells':
                        recovered |= any('BACKPRESSURE_RECOVERED' in run['text']
                                         for row in message['frame']['rows'] for run in row['runs'])
            assert recovered, 'Terminal did not catch up after the frontend resumed'
            assert Path('/proc', str(pid), 'stat').read_text().rsplit(')', 1)[1].split()[19] == start
            print(json.dumps(dict(result='PASS', peak_queued_frames=peak, source_preserved=True)))
        finally:
            if peer:
                peer.close()
            (state / (name + '.close')).touch()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                # This Popen owns only the disposable controller above.
                process.terminate()
                process.wait(timeout=5)
            subprocess.run(['tmux', '-L', 'starflux-' + name, 'kill-server'], env=env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
