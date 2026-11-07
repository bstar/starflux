"""Run inside test-multiwindow.py's private GTK/SSH/App Space fixture."""
package=tmp/'app-popup';package.mkdir();popup_input=tmp/'app-popup-input';popup_ready=tmp/'app-popup-ready'
program=package/'popup'
program.write_text('''#!/usr/bin/python3
import os,tty
from pathlib import Path
tty.setraw(0)
Path('''+repr(str(popup_ready))+''').touch()
print('APP_POPUP_READY\\r\\n',flush=True)
while True:
 data=os.read(0,4096)
 if not data:break
 with Path('''+repr(str(popup_input))+''').open('ab') as f:f.write(data)
''');program.chmod(0o700)
(package/'starflux-plugin.json').write_text(json.dumps(dict(version=1,id='app-popup',name='App coexistence',actions=[],panes=[dict(id='popup',title='App popup',argv=['package:popup'],placement='popup')])))
api('plugins.link',dict(path=str(package),expected_revision=api('plugins.list')['revision']),secrets.token_hex(16))
entry=api('plugins.list')['plugins'][0]
api('plugins.configure',dict(plugin='app-popup',expected_digest=entry['digest'],expected_revision=api('plugins.list')['revision'],enabled=True),secrets.token_hex(16))
before=b.snapshot()['project_state'];before_app_keys=keys.read_text();before_frames=b.video_frames if args.video else b.app_frames;before_audio=len(b.audio_times)
response=api('plugins.panes.open',dict(plugin='app-popup',entrypoint='popup',expected_digest=entry['digest'],project=workspace,frontend=lease),secrets.token_hex(16))
assert response['presentation']['status']=='applied',response
identifier=response['popup']['id']
wait(lambda:b.snapshot().get('plugin_popup'),'App Space popup did not open')
wait(lambda:any(c['kind']=='live_canvas' and c['id']=='plugin-popup' and c['interactive'] for c in b.scene['components']),'popup canvas was not interactive')
wait(lambda:popup_ready.exists(),'popup program was not ready for keyboard input')
assert any(c['kind']=='live_canvas' and c['id']=='apps' and not c['interactive'] for c in b.scene['components']),'App Space retained input through popup'
# The protocol rejects keys addressed to the now-inactive app canvas. The host
# sends new captured input to the popup's explicitly interactive canvas.
b.input(dict(kind='embedded',canvas='plugin-popup',event=dict(timestamp_ns=time.monotonic_ns(),sequence=time.monotonic_ns(),kind=dict(kind='key',code='char:p',phase='press',modifiers=0))))
wait(lambda:popup_input.exists() and b'p' in popup_input.read_bytes(),'popup canvas key did not reach its PTY')
assert keys.read_text()==before_app_keys,'popup input leaked into App Space'
if args.video:wait(lambda:b.video_frames>before_frames+2,'App Space video stopped while popup was open')
else:wait(lambda:b.app_frames>before_frames,'App Space streaming stopped while popup was open')
if args.audio:wait(lambda:len(b.audio_times)>before_audio+2,'App Space audio stopped while popup was open')
assert b.snapshot()['apps'][0]['worker_pid']==worker,'popup replaced the App Space worker'
if getattr(args,'plugin_popup_keyboard',False):
    before_popup_keys=popup_input.read_bytes()
    # Send the shortcut and Enter without waiting for another scene. Both keys
    # stay in the same input scope despite the round-trip presentation delay.
    for code,modifiers in [('f:6',1),('enter',0)]:
        b.input(dict(kind='embedded',canvas='plugin-popup',event=dict(timestamp_ns=time.monotonic_ns(),sequence=time.monotonic_ns(),kind=dict(kind='key',code=code,phase='press',modifiers=modifiers))))
else:
    response=api('plugins.popups.close',dict(id=identifier,frontend=lease),secrets.token_hex(16));assert response['status']=='applied',response
wait(lambda:not b.snapshot().get('plugin_popup') and any(c['kind']=='live_canvas' and c['id']=='apps' and c['interactive'] for c in b.scene['components']),'App Space input was not restored')
if getattr(args,'plugin_popup_keyboard',False):
    assert popup_input.read_bytes()==before_popup_keys and keys.read_text()==before_app_keys,'popup close keys leaked into a program'
for key in ('terminals','terminal','pane_tree','space'):
    assert b.snapshot()['project_state'].get(key)==before.get(key),'popup changed the underlying App Space layout'
panel_key('char:y');wait(lambda:'121' in keys.read_text(),'app did not regain keyboard input after popup close')
assert b.snapshot()['apps'][0]['worker_pid']==worker,'popup close replaced App Space worker'
print(json.dumps(dict(result='PASS',ssh=args.ssh,plugin_popup_with_app_space=True,worker_preserved=True,input_capture_and_restore=True,streaming_preserved=True,audio_preserved=args.audio,keyboard_close=getattr(args,'plugin_popup_keyboard',False))))
