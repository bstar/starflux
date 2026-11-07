"""Popup transport through the actual cells executable in a private host PTY."""
import uuid

def api(method,params=None,write_request=False):
    request=dict(version=1,id='popup-real-cells',method=method,params=params or {})
    if write_request:request['operation_id']=str(uuid.uuid4())
    response=json.loads(subprocess.check_output([*cli,'--api',json.dumps(request)],env=env,timeout=20))
    assert response['ok'],response
    return response['result']
package=tmp/'real-cells-popup';package.mkdir();recorded=tmp/'popup-input';started=tmp/'popup-pid'
program=package/'popup'
program.write_text('''#!/usr/bin/python3
import os,tty
from pathlib import Path
tty.setraw(0)
Path('''+repr(str(started))+''').write_text(str(os.getpid()))
print('REAL_POPUP_READY café 界\\r\\n',flush=True)
while True:
 data=os.read(0,4096)
 if not data:break
 with Path('''+repr(str(recorded))+''').open('ab') as f:f.write(data)
 print('POPUP_HEX='+data.hex()+'\\r\\n',flush=True)
''');program.chmod(0o700)
(package/'starflux-plugin.json').write_text(json.dumps(dict(version=1,id='real-popup',name='Actual cells popup',actions=[],panes=[dict(id='popup',title='Actual popup',argv=['package:popup'],placement='popup',width='80%',height='70%')])))
api('plugins.link',dict(path=str(package),expected_revision=api('plugins.list')['revision']),True)
entry=api('plugins.list')['plugins'][0]
api('plugins.configure',dict(plugin='real-popup',expected_revision=api('plugins.list')['revision'],expected_digest=entry['digest'],enabled=True),True)
def target():return next(v['target'] for v in api('frontends.list')['frontends'] if v['frontend']=='v-cells-proof')
underlying=inputs();project_before=snapshot()['project_state'];offset=len(tty_bytes)
response=api('plugins.panes.open',dict(plugin='real-popup',entrypoint='popup',expected_digest=entry['digest'],project=name,frontend=target()),True)
assert response['presentation']['status']=='applied',response
identifier=response['popup']['id']
wait(lambda:snapshot().get('plugin_popup') and b'REAL_POPUP_READY' in tty_bytes[offset:],'actual host did not render popup terminal cells')
assert 'café 界'.encode() in tty_bytes[offset:],'actual host lost popup Unicode'
write(b'\x1b\x02\x1bOP')
wait(lambda:recorded.exists() and b'\x1b\x02\x1bOP' in recorded.read_bytes(),'actual host intercepted popup Escape, Ctrl+B or F1')
assert not snapshot().get('context_menu') and inputs()==underlying
offset=len(tty_bytes);write(b'\x1b[200~POPUP_PASTE_\xe7\x95\x8c\x1b[201~')
wait(lambda:b'POPUP_PASTE_\xe7\x95\x8c' in recorded.read_bytes(),'actual host popup paste failed')
resize(45,14)
wait(lambda:snapshot().get('viewport',{}).get('columns')==45,'actual popup host resize failed')
rect=wait(lambda:(p:=snapshot().get('plugin_popup')) and p['terminal_rect'],'popup vanished during host resize')
assert rect['x']+rect['width']<=45 and rect['y']+rect['height']<=14
if getattr(args,'plugin_popup_keyboard',False):
    before=recorded.read_bytes();write(b'\x0c')
    wait(lambda:b'\x0c' in recorded.read_bytes()[len(before):],'actual host repaint key bypassed popup')
    before=recorded.read_bytes();write(b'\x1b[17;2~')
    wait(lambda:(snapshot().get('plugin_popup') or {}).get('controls_focused'),'actual host Shift+F6 did not focus popup controls')
    write(b'\x1b[200~IGNORED_CONTROL_PASTE\x1b[201~')
    resize(70,24)
    wait(lambda:snapshot().get('viewport',{}).get('columns')==70 and snapshot()['plugin_popup']['controls_focused'],'actual host resize lost popup control focus')
    write(b'\x1b')
    wait(lambda:snapshot().get('plugin_popup') and not snapshot()['plugin_popup']['controls_focused'],'actual host Escape did not resume popup input')
    assert recorded.read_bytes()==before,'actual host header navigation leaked into popup'
    write(b'\x1b[17;2~')
    wait(lambda:snapshot()['plugin_popup']['controls_focused'],'actual host close controls did not refocus')
    write(b'\r')
else:
    response=api('plugins.popups.close',dict(id=identifier,frontend=target()),True)
    assert response['status']=='applied',response
wait(lambda:not snapshot().get('plugin_popup'),'actual host popup did not close')
popup_pid=int(started.read_text())
def dead():
    try:return Path('/proc',str(popup_pid),'stat').read_text().rsplit(')',1)[1].split()[0]=='Z'
    except OSError:return True
wait(dead,'actual host popup process survived close')
for key in ('terminals','terminal','terminal_epochs','terminal_plugins','pane_tree','space'):
    assert snapshot()['project_state'].get(key)==project_before.get(key),'actual popup changed project state'
write(b'AFTER_POPUP')
wait(lambda:b'AFTER_POPUP' in inputs(),'actual host did not return input to the underlying terminal')
os.kill(backend_pid,0)
print(json.dumps(dict(result='PASS',ssh=args.ssh,actual_cells_host=True,popup_text_unicode=True,raw_keys=True,paste=True,resize=True,cleanup=True,no_pane_membership=True,underlying_input_restored=True,keyboard_popup_close=getattr(args,'plugin_popup_keyboard',False))))
