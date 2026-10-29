#!/usr/bin/env python3
"""Headless native-PTY proof using a private fake shell and isolated tmux server.

No Kitty windows, display server, real agents, or existing workspace are used.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--exit-lifecycle',action='store_true')
parser.add_argument('--context-menu',action='store_true')
parser.add_argument('--font-controls',action='store_true')
parser.add_argument('--render-cache',action='store_true')
parser.add_argument('--app-capabilities',action='store_true')
args=parser.parse_args()
root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='starflux-native-proof-') as temp:
    temp = Path(temp)
    runtime = temp / 'runtime'
    runtime.mkdir(mode=0o700)
    state = runtime / 'starflux'
    state.mkdir(mode=0o700)
    name = 'npty-' + secrets.token_hex(5)
    shell = temp / 'fake-shell'
    shell.write_text("#!/bin/sh\nprintf '\\033[1;31mNATIVE_PTY_PROOF\\033[0m\\r\\n'\nexec cat\n")
    if args.exit_lifecycle:
        shell.write_text("#!/bin/sh\nprintf '\\033[1;31mNATIVE_PTY_PROOF\\033[0m\\r\\n'\nif [ ! -f \"$NATIVE_PROOF_MARKER\" ]; then touch \"$NATIVE_PROOF_MARKER\"; sleep 1; exit 0; fi\nexec cat\n")
    shell.chmod(0o700)
    config = dict(workspace=name, host=None, directory=str(temp), agent=None,
                  executable=str(root / 'scripts/run-local.sh'), project_host=True)
    (state / (name + '.json')).write_text(json.dumps(config))
    env = {**os.environ, 'XDG_RUNTIME_DIR': str(runtime), 'XDG_DATA_HOME': str(temp / 'data'),
           'STAR_TERMINAL_RENDERER': 'native', 'SHELL': str(shell), 'NATIVE_PROOF_MARKER': str(temp/'shell-started')}
    log = (temp / 'controller.log').open('w+')
    process = subprocess.Popen([str(root / 'scripts/run-local.sh'), '--serve', name],
                               env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=log)
    peer = None
    try:
        deadline = time.monotonic() + 20
        path = state / (name + '.sock')
        while not path.exists():
            if process.poll() is not None or time.monotonic() > deadline:
                log.seek(0)
                raise AssertionError('Private native controller failed: ' + log.read())
            time.sleep(.02)
        peer = socket.socket(socket.AF_UNIX)
        peer.connect(str(path))
        peer.settimeout(.25)
        viewport = dict(columns=100, rows=35, width=1000, height=700, generation=1)
        caps = dict(native_terminal_cells=True, image_transport='kitty', pixel_geometry='measured',
                    pointer_precision='cells', keyboard=True, paste=True, presentation_ack=True,
                    native_surfaces=True, physical_text=True, pixel_layout=True)
        if args.font_controls:
            caps['native_font_millipoints']=12500
        if args.app_capabilities:
            caps['streamed_frames']=True
        def send(message):
            peer.sendall(json.dumps(message).encode() + b'\n')
        send(dict(type='hello', version=1, viewport=viewport, client='native-proof', capabilities=caps))
        pending = b''
        types = set()
        found = None
        acks = []
        latest_scene = None
        menu_requested = False
        menu_kept_terminal = False
        exit_removed_tab = False
        reopened = False
        reopened_at = None
        font_requests=[]
        font_actions=['increase','decrease','reset']
        font_sent=0
        cache_baseline=None
        cache_verified=False
        while time.monotonic() < deadline:
            try:
                data = peer.recv(65536)
            except TimeoutError:
                data = None
            if data == b'':
                break
            if data:
                pending += data
            while b'\n' in pending:
                line, pending = pending.split(b'\n', 1)
                message = json.loads(line)
                types.add(message['type'])
                if message['type']=='terminal_font': font_requests.append(message['size_millipoints'])
                if message['type']=='ack': acks.append(message)
                if message['type'] == 'scene':
                    scene = message['scene']
                    latest_scene = scene
                    send(dict(type='presented', revision=scene['revision'], generation=scene['viewport']['generation']))
                if message['type'] == 'error':
                    raise AssertionError(message['message'])
                if message['type'] == 'terminal_cells':
                    text = ''.join(run['text'] for row in message['frame']['rows'] for run in row['runs'])
                    if 'NATIVE_PTY_PROOF' in text:
                        found = message
            snapshot = state / (name + '.state.json')
            if found and snapshot.exists():
                metadata = json.loads(snapshot.read_text())
                assert metadata['terminal_renderer'] == 'native-kitty', metadata['terminal_renderer']
                assert metadata['terminal_presentation']['transport'] == 'native-kitty-cells'
                assert metadata['desktop']['windows'] == 0
                if args.app_capabilities:
                    assert metadata['app_view_available']
                    if latest_scene is None:continue
                    actions=[hit['action'] for component in latest_scene['components'] if component['kind']=='surface' for hit in component['surface'].get('hits',[])]
                    assert 'view:apps' in actions,actions
                if args.render_cache:
                    stats=metadata['graphical_render']
                    if cache_baseline is None and latest_scene is not None:
                        cache_baseline=dict(stats)
                        send(dict(type='input',id=1,revision=latest_scene['revision'],generation=1,input=dict(kind='paste',text='UNCHANGED_CHROME_INPUT')))
                    if cache_baseline is not None and 'UNCHANGED_CHROME_INPUT' in metadata['direct_terminals'][0]['text'] and stats['scene_cache_hits']>cache_baseline['scene_cache_hits']:
                        assert stats['scene_builds']<=cache_baseline['scene_builds']+1, (cache_baseline,stats)
                        cache_verified=True
                        break
                    continue
                if args.font_controls:
                    if len(font_requests)==3:
                        assert font_requests==[13500,12500,0],font_requests
                        assert metadata['project_state']['terminals']==[0]
                        break
                    if font_sent==len(font_requests):
                        subprocess.run([str(root/'scripts/run-local.sh'),'--workspace',name,'--action','terminal:font:'+font_actions[font_sent]],env=env,check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
                        font_sent+=1
                    continue
                if args.context_menu:
                    if not menu_requested and latest_scene is not None:
                        send(dict(type='input',id=1,revision=latest_scene['revision'],generation=1,
                                  input=dict(kind='pointer',action='down',button=1,x=2,y=6,pixel=[20,130],modifiers=0)))
                        menu_requested=True
                    if metadata.get('context_menu'):
                        assert any(c['kind']=='terminal' for c in latest_scene['components']), 'Sidebar menu hid the terminal region'
                        assert metadata['project_state']['terminals']==[0]
                        assert 'NATIVE_PTY_PROOF' in metadata['direct_terminals'][0]['text']
                        menu_kept_terminal=True
                        break
                    continue
                if not args.exit_lifecycle:
                    break
                terminals=metadata['project_state']['terminals']
                if not terminals and not reopened:
                    exit_removed_tab=True
                    subprocess.run([str(root/'scripts/run-local.sh'),'--workspace',name,'--action','terminal:new'],env=env,check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
                    reopened=True;reopened_at=time.monotonic()
                if reopened and time.monotonic()-reopened_at>3:
                    assert terminals==[0],metadata['project_state']
                    assert metadata['sessions'][0]['health']['alive'] is True,metadata['sessions']
                    break
        assert found, 'PTY did not deliver native styled terminal rows'
        assert not types.intersection({'canvas_frame', 'canvas_pixels', 'canvas_stream'}), types
        if args.render_cache:
            assert cache_verified, 'Terminal input did not reuse unchanged graphical chrome'
        if args.context_menu:
            assert menu_kept_terminal, f'Sidebar menu failed to preserve terminal: acks={acks}, menu={metadata.get("context_menu")}, status={metadata.get("status")}'
        if args.font_controls:
            assert font_requests==[13500,12500,0],font_requests
        if args.exit_lifecycle:
            assert exit_removed_tab and reopened, 'Exited terminal was not removed and reopened'
        print(json.dumps(dict(app_view_available=metadata['app_view_available'], render_cache_verified=cache_verified, font_requests=font_requests, sidebar_menu_kept_terminal=menu_kept_terminal, exit_removed_tab=exit_removed_tab, reused_slot=0 if reopened else None, result='PASS', renderer='host Kitty text cells',
                              frame_bytes=len(json.dumps(found).encode()), terminal_pixel_frames=0,
                              messages=sorted(types))))
    finally:
        if peer:
            peer.close()
        (state / (name + '.close')).touch()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.terminate()
            process.wait(timeout=3)
        subprocess.run(['tmux', '-L', 'starflux-' + name, 'kill-server'],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
