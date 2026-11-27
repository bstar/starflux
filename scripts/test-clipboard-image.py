#!/usr/bin/env python3
"""Ctrl+V with a screenshot on the clipboard delivers the image to the pane.

Runs the real cells frontend in a private PTY against a private, headless
Wayland compositor, so it never touches the desktop or the user's clipboard.
The pane's program has no desktop session, like a terminal opened by an owner
that started from a console, or any pane on an SSH host: it cannot read the
clipboard itself, so STAR/FLUX must stage the image and paste its path.

Needs `wl-copy`/`wl-paste` and a headless compositor: set STARFLUX_TEST_WAYLAND
to a running one's socket, or have `sway` on PATH (STARFLUX_TEST_SWAY overrides).
"""
import fcntl, json, os, pty, secrets, shlex, shutil, signal, struct, subprocess
import tempfile, termios, threading, time, zlib
from pathlib import Path

root = Path(__file__).resolve().parents[1]
app = Path(os.environ.get('STARFLUX_TEST_APP', str(root / 'scripts/run-local.sh'))).resolve()
report = root / 'target/clipboard-image-proof'
report.mkdir(parents=True, exist_ok=True)


def png(width, height):
    def chunk(kind, value):
        return struct.pack('>I', len(value)) + kind + value + struct.pack('>I', zlib.crc32(kind + value))
    rows = (b'\0' + b'\x33\x99\xdd\xff' * width) * height
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


with tempfile.TemporaryDirectory(prefix='starflux-clip-') as directory:
    tmp = Path(directory)
    processes = []
    logs = []
    try:
        wayland = os.environ.get('STARFLUX_TEST_WAYLAND')
        if not wayland:
            sway = os.environ.get('STARFLUX_TEST_SWAY') or shutil.which('sway')
            assert sway, 'Need a headless compositor: set STARFLUX_TEST_WAYLAND or put sway on PATH'
            # The socket path must fit a sockaddr_un, so it lives in a short directory.
            sockets = Path(tempfile.mkdtemp(prefix='sfclip-', dir='/tmp'))
            (tmp / 'sway.conf').write_text('output HEADLESS-1 resolution 800x600\n')
            log = (report / 'sway.log').open('w')
            logs.append(log)
            compositor_env = {k: v for k, v in os.environ.items() if k not in (
                'WAYLAND_DISPLAY', 'DISPLAY', 'SWAYSOCK', 'HYPRLAND_INSTANCE_SIGNATURE')}
            compositor_env.update(XDG_RUNTIME_DIR=str(sockets), WLR_BACKENDS='headless',
                                  WLR_LIBINPUT_NO_DEVICES='1', WLR_RENDERER='pixman')
            processes.append(subprocess.Popen([sway, '-c', str(tmp / 'sway.conf')], env=compositor_env,
                                              stdout=log, stderr=log, start_new_session=True))
            deadline = time.monotonic() + 10
            while not any(sockets.glob('wayland-*[0-9]')):
                assert processes[-1].poll() is None and time.monotonic() < deadline, 'headless sway did not start'
                time.sleep(.05)
            wayland = str(next(p for p in sockets.glob('wayland-*') if not p.name.endswith('.lock')))
        clip_env = {**os.environ, 'WAYLAND_DISPLAY': wayland}

        def copy(data, mime):
            # wl-copy serves the selection from a background process until replaced.
            subprocess.run(['wl-copy', '--type', mime], input=data, env=clip_env, check=True, timeout=10)
            deadline = time.monotonic() + 5
            while mime not in subprocess.run(['wl-paste', '--list-types'], env=clip_env,
                                             capture_output=True, text=True, timeout=5).stdout:
                assert time.monotonic() < deadline, 'private clipboard did not take ' + mime
                time.sleep(.05)

        runtime = tmp / 'runtime'
        runtime.mkdir(mode=0o700)
        state = runtime / 'starflux'
        state.mkdir(mode=0o700)
        name = 'clip-' + secrets.token_hex(4)
        fixture = tmp / 'fixture.py'
        fixture.write_text("""import os,select,sys,tty
from pathlib import Path
tty.setraw(0)
os.write(1, b'\\x1b[?2004hCLIP_READY\\r\\n')
with open(Path(sys.argv[1])/'input', 'ab', buffering=0) as out:
    while True:
        if not select.select([0],[],[],.1)[0]:continue
        data=os.read(0,65536)
        if not data: break
        out.write(data)
""")
        shell = tmp / 'shell'
        # The pane's program sees no desktop session, as in a pane whose owner
        # started from a console: it could not read the clipboard by itself.
        shell.write_text('#!/bin/sh\nexec env -u WAYLAND_DISPLAY -u DISPLAY python3 '
                         + shlex.quote(str(fixture)) + ' ' + shlex.quote(str(tmp)) + '\n')
        shell.chmod(0o700)
        env = {k: v for k, v in os.environ.items() if k not in (
            'KITTY_WINDOW_ID', 'KITTY_PID', 'KITTY_LISTEN_ON', 'WAYLAND_DISPLAY', 'DISPLAY',
            'SSH_CONNECTION', 'SSH_TTY', 'TMUX', 'STY', 'STAR_CORE_TERMINAL_SOCKET')}
        env.update(XDG_RUNTIME_DIR=str(runtime), XDG_DATA_HOME=str(tmp / 'data'),
                   XDG_CONFIG_HOME=str(tmp / 'config'), SHELL=str(shell), TERM='xterm',
                   STAR_TERMINAL_RENDERER='kitty-pixels', STARFLUX_KITTY=str(tmp / 'kitty-must-not-run'),
                   WAYLAND_DISPLAY=wayland)
        (state / (name + '.json')).write_text(json.dumps(dict(
            workspace=name, host=None, directory=str(tmp), agent=None,
            executable=str(app), project_host=True)))
        identity = 'v-clip-proof'
        tty_bytes = bytearray()

        def snapshot():
            try:
                return json.loads((state / (identity + '.state.json')).read_text())
            except (ValueError, OSError):
                return {}

        def received():
            path = tmp / 'input'
            return path.read_bytes() if path.exists() else b''

        def wait(predicate, label, timeout=15):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                value = predicate()
                if value:
                    return value
                time.sleep(.025)
            raise AssertionError(label + '\npane received: ' + repr(received())
                                 + '\nstatus: ' + repr(snapshot().get('status')))

        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
        log = (report / 'frontend.log').open('w')
        logs.append(log)
        frontend = subprocess.Popen([str(app), '--cells', '--workspace', name, '--frontend', identity],
                                    stdin=slave, stdout=slave, stderr=log, env=env, start_new_session=True)
        processes.append(frontend)
        os.close(slave)

        def read():
            try:
                while chunk := os.read(master, 65536):
                    tty_bytes.extend(chunk)
            except OSError:
                pass
        threading.Thread(target=read, daemon=True).start()
        wait(lambda: snapshot().get('presentation') == 'cells' and b'CLIP_READY' in tty_bytes,
             'cells frontend did not show the pane', timeout=30)
        wait(lambda: (snapshot().get('panes') or [{}])[0].get('pid'), 'pane pid was not reported')

        # A screenshot: only an image type on the clipboard.
        shot = png(64, 48)
        copy(shot, 'image/png')
        os.write(master, b'\x16')
        delivered = wait(lambda: b'\x1b[201~' in received() and received(),
                         'Ctrl+V with a screenshot on the clipboard did not paste the image')
        pasted = delivered.split(b'\x1b[200~', 1)[1].split(b'\x1b[201~', 1)[0].decode()
        paths = shlex.split(pasted)
        assert len(paths) == 1 and paths[0].endswith('.png'), pasted
        assert Path(paths[0]).read_bytes() == shot, 'staged image differs from the clipboard'
        assert b'\x16' not in delivered, 'the raw Ctrl+V also reached the pane'
        print('image', paths[0], flush=True)

        # Text on the clipboard keeps Ctrl+V's ordinary meaning for the program.
        before = len(received())
        copy(b'plain words', 'text/plain;charset=utf-8')
        os.write(master, b'\x16')
        wait(lambda: received()[before:] == b'\x16', 'Ctrl+V with text on the clipboard did not reach the pane')
        print('text passthrough ok', flush=True)
    finally:
        if 'state' in dir():
            (state / 'v-clip-proof.close').touch()
        for process in reversed(processes):
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
        # Terminal owners started for this private runtime end with it.
        for proc in Path('/proc').iterdir():
            if proc.name.isdigit():
                try:
                    if ('XDG_DATA_HOME=' + str(tmp / 'data')).encode() in (proc / 'environ').read_bytes().split(b'\0'):
                        os.kill(int(proc.name), signal.SIGTERM)
                except (OSError, ProcessLookupError):
                    pass
        for log in logs:
            log.close()
        if 'sockets' in dir():
            shutil.rmtree(sockets, ignore_errors=True)
