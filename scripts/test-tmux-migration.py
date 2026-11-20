#!/usr/bin/env python3
"""Headless proof that terminals on the retired tmux backend migrate to native terminals.

A private tmux server holds an idle shell with scrollback and an idle fake Claude
with a recorded conversation. `--migrate-terminals` must reopen the shell natively
with its scrollback and resume the conversation natively, then retire the tmux
session. A busy fake Claude must stay on tmux. Everything runs in private
directories; no display, real agent or existing project is used.
"""
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time

app = Path(os.environ.get('STARFLUX_TEST_APP', str(Path(__file__).resolve().parents[1] / 'scripts/run-local.sh'))).resolve()
FAKE_CLAUDE = r'''#!/usr/bin/env python3
import json, os, pathlib, signal, sys, time, uuid
home = pathlib.Path.home()
args = sys.argv[1:]
with open(os.environ['MIGRATION_LOG'], 'a') as log:
    log.write(json.dumps(args) + '\n')
session = args[args.index('--resume') + 1] if '--resume' in args else str(uuid.uuid4())
ticks = pathlib.Path('/proc/self/stat').read_text().rsplit(')', 1)[1].split()[19]
status = 'busy' if os.environ.get('FAKE_BUSY') else 'idle'
folder = home / '.claude/sessions'
folder.mkdir(parents=True, exist_ok=True)
(folder / (str(os.getpid()) + '.json')).write_text(json.dumps(dict(pid=os.getpid(), sessionId=session, procStart=ticks, status=status)))
print('FAKE_CLAUDE ' + session, flush=True)
signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
while True:
    time.sleep(1)
'''

with tempfile.TemporaryDirectory(prefix='sf-mig-') as directory:
    tmp = Path(directory)
    runtime = tmp / 'runtime'; runtime.mkdir(mode=0o700)
    home = tmp / 'home'; home.mkdir()
    sockets = tmp / 'tmux'; sockets.mkdir(mode=0o700)
    bin_dir = tmp / 'bin'; bin_dir.mkdir()
    claude = bin_dir / 'claude'; claude.write_text(FAKE_CLAUDE); claude.chmod(0o700)
    log = tmp / 'claude-runs.log'
    env = {k: v for k, v in os.environ.items() if k not in ('TMUX', 'STARFLUX_SNAPSHOT_SERVICE')}
    env.update(HOME=str(home), XDG_RUNTIME_DIR=str(runtime), XDG_DATA_HOME=str(tmp / 'data'), XDG_CONFIG_HOME=str(tmp / 'config'),
               TMUX_TMPDIR=str(sockets), SHELL='/bin/bash', PATH=str(bin_dir) + ':' + env['PATH'], MIGRATION_LOG=str(log),
               STARFLUX_SNAPSHOT_SERVICE='off', STARFLUX_AGENT_MUX=str(tmp / 'mux-not-installed'))
    projects = tmp / 'data/starflux/projects'
    for folder in (tmp / 'data', tmp / 'data/starflux', projects):folder.mkdir(mode=0o700)
    # Older builds left the data directory world-readable; migration must cope.
    (tmp / 'data/starflux').chmod(0o755)
    names = dict(idle='mig-idle', busy='mig-busy', plan='mig-plan')
    for name in names.values():
        (projects / (name + '.json')).write_text(json.dumps(dict(workspace=name, host=None, directory=str(tmp), agent=None,
                                                                 executable=str(app), project_host=True)))
    (projects / (names['idle'] + '.model.json')).write_text(json.dumps(dict(terminal_first=True, terminals=[0, 1])))
    (projects / (names['busy'] + '.model.json')).write_text(json.dumps(dict(terminal_first=True, terminals=[0])))
    (projects / (names['plan'] + '.model.json')).write_text(json.dumps(dict(terminal_first=True, terminals=[0])))
    # A migration interrupted after its tmux session ended: only the plan remains.
    plans = runtime / ('starflux-host-' + str(os.getuid())); plans.mkdir(mode=0o700)
    (plans / ('migrated-' + names['plan'] + '-p0.ansi')).write_text('INTERRUPTED_SCROLLBACK\n')
    (plans / ('migrated-' + names['plan'] + '-p0.json')).write_text(json.dumps(dict(migrated=True, cwd=str(tmp), shell=['/bin/bash', '--norc', '-i'], claude=None,
                                                                                  history=str(plans / ('migrated-' + names['plan'] + '-p0.ansi')))))

    def tmux(name, *arguments, check=True):
        return subprocess.run(['tmux', '-L', 'starflux-' + name, '-f', '/dev/null', *arguments], env=env, check=check,
                              stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True).stdout

    def wait(condition, message, timeout=15):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if (value := condition()):
                return value
            time.sleep(.1)
        raise AssertionError(message)

    def api(method, params):
        request = json.dumps(dict(version=1, id='1', method=method, params=params))
        reply = json.loads(subprocess.run([str(app), '--api', request], env=env, check=True, stdout=subprocess.PIPE, text=True).stdout)
        assert reply.get('ok'), reply
        return reply['result']

    try:
        tmux(names['idle'], 'new-session', '-d', '-s', 'p0', '-c', str(tmp), '-x', '100', '-y', '30', '/bin/bash', '--norc', '-i')
        tmux(names['idle'], 'send-keys', '-t', 'p0', 'echo LEGACY_SCROLLBACK_$((6*7))', 'Enter')
        tmux(names['idle'], 'new-session', '-d', '-s', 'p1', '-c', str(tmp), '/bin/bash', '--norc', '-i')
        tmux(names['idle'], 'send-keys', '-t', 'p1', 'claude --dangerously-skip-permissions "do not resend this prompt"', 'Enter')
        tmux(names['busy'], 'new-session', '-d', '-s', 'p0', '-c', str(tmp), '-e', 'FAKE_BUSY=1', '/bin/bash', '--norc', '-i')
        tmux(names['busy'], 'send-keys', '-t', 'p0', 'claude', 'Enter')
        wait(lambda: 'LEGACY_SCROLLBACK_42' in tmux(names['idle'], 'capture-pane', '-p', '-t', 'p0'), 'legacy shell did not start')
        wait(lambda: log.exists() and len(log.read_text().splitlines()) == 2, 'fake agents did not start')
        original = json.loads(log.read_text().splitlines()[0])
        first_session = next(json.loads(p.read_text())['sessionId'] for p in (home / '.claude/sessions').iterdir()
                             if json.loads(p.read_text())['status'] == 'idle')

        dry = json.loads(subprocess.run([str(app), '--migrate-terminals', '--dry-run'], env=env, check=True, stdout=subprocess.PIPE, text=True).stdout)
        results = {(t['project'], t['pane']): t['result'] for t in dry['terminals']}
        assert results == {(names['idle'], 1): 'would migrate', (names['idle'], 2): 'would migrate',
                           (names['busy'], 1): 'stays on tmux: agent busy', (names['plan'], 1): 'would migrate'}, results
        assert tmux(names['idle'], 'list-sessions', '-F', '#{session_name}').split() == ['p0', 'p1'], 'dry run changed tmux'

        done = json.loads(subprocess.run([str(app), '--migrate-terminals'], env=env, check=True, stdout=subprocess.PIPE, text=True).stdout)
        results = {(t['project'], t['pane']): t['result'] for t in done['terminals']}
        assert results == {(names['idle'], 1): 'migrated', (names['idle'], 2): 'migrated',
                           (names['busy'], 1): 'stays on tmux: agent busy', (names['plan'], 1): 'migrated'}, results
        wait(lambda: 'INTERRUPTED_SCROLLBACK' in api('panes.capture', dict(project=names['plan'], pane=1))['text'], 'interrupted migration did not resume')
        assert not list(plans.glob('*.json')), 'migration plans were not cleared'
        assert subprocess.run(['tmux', '-L', 'starflux-' + names['idle'], 'has-session'], env=env, stderr=subprocess.DEVNULL).returncode != 0, 'tmux server kept the idle project'
        assert tmux(names['busy'], 'list-sessions', '-F', '#{session_name}').split() == ['p0'], 'busy agent was not kept'

        model = json.loads((projects / (names['idle'] + '.model.json')).read_text())
        assert sorted(model['native_sources']) == ['0', '1'], model.get('native_sources')
        wait(lambda: 'LEGACY_SCROLLBACK_42' in api('panes.capture', dict(project=names['idle'], pane=1))['text'], 'native shell lost its scrollback')
        resumed = wait(lambda: len(log.read_text().splitlines()) == 3 and json.loads(log.read_text().splitlines()[2]), 'conversation did not resume')
        assert resumed[resumed.index('--resume') + 1] == first_session, resumed
        assert '--dangerously-skip-permissions' in resumed and 'do not resend this prompt' not in resumed, (original, resumed)
        wait(lambda: 'FAKE_CLAUDE ' + first_session in api('panes.capture', dict(project=names['idle'], pane=2))['text'], 'resumed agent is not on the native terminal')
        again = json.loads(subprocess.run([str(app), '--migrate-terminals'], env=env, check=True, stdout=subprocess.PIPE, text=True).stdout)
        again = {(t['project'], t['pane']): t['result'] for t in again['terminals']}
        assert again.pop((names['busy'], 1)) == 'stays on tmux: agent busy' and set(again.values()) <= {'not on tmux'}, again
        print(json.dumps(dict(result='PASS', idle_shell_migrated=True, scrollback_kept=True, conversation_resumed=True,
                              prompt_not_resent=True, interrupted_plan_resumed=True, public_data_directory=True, busy_agent_kept=True, dry_run_read_only=True, repeat_is_noop=True)))
    finally:
        for name in names.values():
            subprocess.run(['tmux', '-L', 'starflux-' + name, 'kill-server'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        owner_path = tmp / 'data/starflux/native-sources/owner.json'
        if owner_path.exists():
            try:
                record = json.loads(owner_path.read_text())['owner']
                token = json.loads(owner_path.with_name('token.json').read_text())
                with socket.socket(socket.AF_UNIX) as peer:
                    peer.settimeout(5); peer.connect(str(owner_path.with_name('broker.sock')))
                    peer.sendall((json.dumps(dict(version=1, id=1, token=token, action=dict(op='shutdown', instance=record['instance']))) + '\n').encode())
                    peer.makefile('rb').readline(65536)
                deadline = time.monotonic() + 10
                while Path('/proc', str(record['pid'])).exists() and time.monotonic() < deadline:
                    time.sleep(.05)
            except (OSError, ValueError, KeyError) as error:
                print('native owner cleanup failed:', error)
