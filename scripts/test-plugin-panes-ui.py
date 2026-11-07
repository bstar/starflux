"""Private native UI acceptance; test-session-manager.py --plugin-panes-ui."""
import json,os,shlex,subprocess,uuid

def api(method,params=None,write=False,ok=True,operation=None):
    request=dict(version=1,id='plugin-pane-ui',method=method,params=params or {})
    if write:request['operation_id']=operation or str(uuid.uuid4())
    command=[str(app),'--api',json.dumps(request)]
    if args.ssh:command=ssh_args+[shlex.join(['env',*[k+'='+env[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','PATH','STARFLUX_HERDR') if k in env],*command])]
    result=json.loads(subprocess.check_output(command,env=env,timeout=20));assert result['ok']==ok,result
    return result.get('result',result.get('error'))
def plugin():return api('plugins.list')['plugins'][0]
def menu():return a.snapshot().get('context_menu') or {}
def page():
    a.action('plugins:plugin:pane-ui')
    wait(lambda:any(i['action']=='plugins:open-pane:pane-ui:overlay' for i in menu().get('items',[])),'pane entrypoints were not offered')
def records():return api('plugins.panes.list')['panes']
def calls():return [json.loads(line) for line in counter.read_text().splitlines()] if counter.exists() else []

package=tmp/'pane-ui-package';package.mkdir();counter=tmp/'pane-ui-starts'
program=package/'pane'
program.write_text('''#!/usr/bin/python3
import json,os,sys
from pathlib import Path
assert os.isatty(0)
c=json.loads(os.environ['STARFLUX_PLUGIN_CONTEXT_JSON'])
with Path('''+repr(str(counter))+''').open('a') as f:f.write(json.dumps(dict(context=c,pid=os.getpid()))+'\\n')
print('PLUGIN_UI_READY café 界',flush=True)
for line in sys.stdin:print('PLUGIN_UI_REPLY='+line.strip(),flush=True)
''');program.chmod(0o700)
placements=['overlay','split','zoomed','tab','popup']
manifest=dict(version=1,id='pane-ui',name='Pane UI proof',actions=[],panes=[dict(id=p,title='Pane '+p,argv=['package:pane'],placement=p) for p in placements])
(package/'starflux-plugin.json').write_text(json.dumps(manifest))
api('plugins.link',dict(path=str(package),expected_revision=api('plugins.list')['revision']),write=True)
a.shortcut('char:U',1)
wait(lambda:menu().get('title')=='Plugins' and any(i['action']=='plugins:plugin:pane-ui' for i in menu().get('items',[])),'keyboard plugin menu did not open')
a.action('plugins:plugin:pane-ui')
wait(lambda:'package:pane' in ''.join(i['label'] for i in menu().get('items',[])),'pane argv was absent from command review')
assert not any(i['action'].startswith('plugins:open-pane:') for i in menu().get('items',[])),'disabled pane could launch'
p=plugin();api('plugins.configure',dict(plugin='pane-ui',expected_revision=api('plugins.list')['revision'],expected_digest=p['digest'],enabled=True),write=True)
original_state=b.snapshot()['project_state'];original_b_focus=original_state['terminal'];original_b_tree=original_state['pane_tree']
for index,placement in enumerate(placements[:-1]):
    page();before=a.snapshot()['project_state'];previous_focus=before['terminal'];previous_tree=before['pane_tree'];previous_zoom=before['pane_zoom']
    a.action('plugins:open-pane:pane-ui:'+placement)
    wait(lambda:(s:=a.snapshot()['project_state']).get('terminal_plugins') and s['terminal']!=previous_focus and not menu(),'pane did not present in the requesting window')
    try:
        record=wait(lambda:next((p['record'] for p in records() if p['alive'] and p['record']['entrypoint']['id']==placement),None),'pane process did not start')
    except AssertionError:
        print('PANE_DIAGNOSTICS', [{k:p['record'].get(k) for k in ('status','error','pid','pane')} for p in records()],flush=True)
        raise

    pane=record['pane'];wait(lambda:len(calls())==index+1 and any(t['pane']==pane and 'PLUGIN_UI_READY' in t['text'] for t in a.snapshot().get('direct_terminals',[])),'plugin terminal was not rendered')
    assert b.snapshot()['project_state']['terminal']==original_b_focus and b.snapshot()['project_state']['pane_tree']==original_b_tree,'pane placement changed another window'
    if placement=='overlay':
        assert a.snapshot()['project_state']['plugin_overlay'] and a.snapshot()['project_state']['pane_zoom']
        a=restart(a)
        wait(lambda:a.snapshot()['project_state'].get('plugin_overlay') and a.snapshot()['project_state']['terminal']==pane,'overlay did not survive a view restart')
        assert len(calls())==index+1,'view restart replayed a plugin command'
        a.action('terminal:close-pane')
        wait(lambda:not a.snapshot()['project_state'].get('plugin_overlay') and a.snapshot()['project_state']['terminal']==previous_focus,'overlay close did not restore focus')
        assert a.snapshot()['project_state']['pane_tree']==previous_tree and a.snapshot()['project_state']['pane_zoom']==previous_zoom,'overlay close changed the previous layout'
        page();a.action('plugins:panes:pane-ui')
        wait(lambda:any(i['action']=='plugins:focus-pane:'+record['id'] for i in menu().get('items',[])),'owned pane list did not offer focus')
        a.action('plugins:focus-pane:'+record['id']);wait(lambda:a.snapshot()['project_state']['terminal']==pane and not menu(),'owned terminal could not be focused')
    elif placement=='split':assert a.snapshot()['project_state']['split'] and not a.snapshot()['project_state']['pane_zoom']
    elif placement=='zoomed':assert a.snapshot()['project_state']['split'] and a.snapshot()['project_state']['pane_zoom']
    a.paste('input '+placement);a.key('enter')
    wait(lambda:any(t['pane']==pane and 'PLUGIN_UI_REPLY=input '+placement in t['text'] for t in a.snapshot().get('direct_terminals',[])),'keyboard input did not reach plugin PTY')
    api('plugins.panes.close',dict(id=record['id'],expected_epoch=record['epoch']),write=True)
    wait(lambda: str(pane) not in a.snapshot()['project_state'].get('terminal_plugins',{}),'closed plugin membership did not disappear')
    assert alive(original_pid),'plugin close stopped the original terminal'
# Hold the approval lock so Escape is observed before the background open can
# finish. Its late callback must leave focus and an unrelated menu alone.
import fcntl
page();focus=a.snapshot()['project_state']['terminal']
with (tmp/'config/starflux/plugins.lock').open('a') as lock:
    fcntl.flock(lock,fcntl.LOCK_EX)
    a.action('plugins:open-pane:pane-ui:tab')
    wait(lambda:menu().get('items') and menu()['items'][0]['label']=='Opening plugin terminal…','pending pane did not show progress')
    a.key('escape');wait(lambda:not menu(),'pending open did not dismiss')
    a.key('f:1');wait(lambda:menu().get('title')=='Keyboard shortcuts','unrelated menu did not open')
    fcntl.flock(lock,fcntl.LOCK_UN)
record=wait(lambda:next((p['record'] for p in records() if p['alive']),None),'dismissed open did not retain its terminal')
wait(lambda:len(calls())==5,'dismissed open ran zero or multiple commands')
assert a.snapshot()['project_state']['terminal']==focus and menu().get('title')=='Keyboard shortcuts','late callback stole focus or replaced another menu'
a.key('escape');wait(lambda:not menu(),'unrelated menu did not close')
api('plugins.panes.close',dict(id=record['id'],expected_epoch=record['epoch']),write=True)
wait(lambda:str(record['pane']) not in a.snapshot()['project_state'].get('terminal_plugins',{}),'dismissed pane did not close')
page();count=len(calls());panes=len(a.snapshot()['project_state']['terminals'])
assert any(i['action']=='plugins:open-pane:pane-ui:popup' for i in menu().get('items',[]))
error=api('plugins.panes.open',dict(plugin='pane-ui',entrypoint='popup',expected_digest=plugin()['digest'],project=name),write=True,ok=False)
assert error['code']=='frontend_required' and len(calls())==count and len(a.snapshot()['project_state']['terminals'])==panes,'untargeted popup allocated a normal project pane'
# Creation and presentation have distinct receipts: explicit targets are checked
# before launch, and placement is acknowledged by only the named controller.
a.key('escape');wait(lambda:not menu(),'pane page did not close before API placement')
def frontend(client):return next(v for v in api('frontends.list')['frontends'] if v['frontend']==client.identity)
lease=frontend(a)['target'];other_lease=frontend(b)['target']
assert frontend(a)['plugin_pane_commands'],'frontend did not advertise pane commands'
baseline=len(calls());base_panes=len(a.snapshot()['project_state']['terminals'])
request=dict(plugin='pane-ui',entrypoint='tab',expected_digest=plugin()['digest'],project=name,frontend=lease,placement='split')
poison={**lease,'start_ticks':lease['start_ticks']+1}
assert api('plugins.panes.open',{**request,'frontend':poison},write=True,ok=False)['code']=='stale_frontend'
assert api('plugins.panes.open',{**request,'project':'other-project'},write=True,ok=False)['code']=='frontend_project_changed'
a.key('f:1');wait(lambda:menu().get('title')=='Keyboard shortcuts' and frontend(a)['busy'],'busy preflight did not observe help menu')
assert api('plugins.panes.open',request,write=True,ok=False)['code']=='frontend_busy'
assert len(calls())==baseline and len(a.snapshot()['project_state']['terminals'])==base_panes,'rejected target launched a command'
a.key('escape');wait(lambda:not menu() and not frontend(a)['busy'],'help menu did not close')
operation=str(uuid.uuid4());response=api('plugins.panes.open',request,write=True,operation=operation)
record=response['record'];receipt=response['presentation']
assert receipt['status']=='applied' and receipt['result']['plugin_pane']==record['id'] and receipt['result']['placement']=='split',receipt
wait(lambda:a.snapshot()['project_state']['terminal']==record['pane'] and a.snapshot()['project_state']['split'],'targeted split did not appear')
wait(lambda:len(calls())==baseline+1,'targeted open ran zero or multiple commands')
assert api('plugins.panes.open',request,write=True,operation=operation)['record']['id']==record['id'] and len(calls())==baseline+1,'open replay created a second pane'
assert b.snapshot()['project_state']['terminal']==original_b_focus and b.snapshot()['project_state']['pane_tree']==original_b_tree,'targeted open changed a sibling window'
focus=dict(id=record['id'],expected_epoch=record['epoch'],frontend=other_lease,placement='zoomed')
ack=api('plugins.panes.focus',focus,write=True)
assert ack['status']=='applied' and ack['result']['placement']=='zoomed',ack
wait(lambda:b.snapshot()['project_state']['terminal']==record['pane'] and b.snapshot()['project_state']['pane_zoom'],'targeted focus did not zoom the named window')
assert not a.snapshot()['project_state']['pane_zoom'] and len(calls())==baseline+1,'focus changed another window or replayed a command'
assert api('plugins.panes.focus',{**focus,'expected_epoch':str(uuid.uuid4())},write=True,ok=False)['code']=='stale_target'
a.key('f:1');wait(lambda:menu().get('title')=='Keyboard shortcuts','help menu did not open for controller recheck')
ack=api('plugins.panes.focus',{**focus,'frontend':lease,'placement':'overlay'},write=True)
assert ack['status']=='rejected' and ack['error']['code']=='frontend_busy' and menu().get('title')=='Keyboard shortcuts',ack
a.key('escape');wait(lambda:not menu(),'controller recheck help did not close')
# Placement overrides apply before allocation, including a normal tab override
# for an entrypoint whose default is the transient popup surface.
override=api('plugins.panes.open',dict(plugin='pane-ui',entrypoint='popup',expected_digest=plugin()['digest'],project=name,placement='tab'),write=True)
assert override['presentation']=='not_requested' and override['record']['entrypoint']['placement']=='tab'
wait(lambda:len(calls())==baseline+2,'normal placement override did not launch exactly once')
for current in [record,override['record']]:api('plugins.panes.close',dict(id=current['id'],expected_epoch=current['epoch']),write=True)
wait(lambda:len(a.snapshot()['project_state']['terminals'])==base_panes,'targeted panes did not close cleanly')
assert alive(original_pid),'targeted placement stopped the original terminal'
if args.plugin_pane_upgrade:
    # Use a disposable ELF, never rename or unlink the installed application.
    import shutil
    controller=a.snapshot()['controller_pid'];original_inode=copied.stat().st_ino
    assert Path('/proc',str(controller),'exe').stat().st_ino==original_inode
    replacement=tmp/'replacement-build';shutil.copy2(root/'target/release/starflux',replacement);replacement.replace(copied)
    assert str(Path('/proc',str(controller),'exe').readlink()).endswith(' (deleted)')
    page();before=len(calls());a.action('plugins:open-pane:pane-ui:tab')
    current=wait(lambda:next((p['record'] for p in records() if p['alive']),None),'replacement launcher did not start the terminal')
    wait(lambda:len(calls())==before+1,'replacement launcher duplicated or lost the command')
    assert a.snapshot()['controller_pid']==controller,'launch replaced the controller'
    api('plugins.panes.close',dict(id=current['id'],expected_epoch=current['epoch']),write=True)
    wait(lambda:not a.snapshot()['project_state'].get('terminal_plugins'),'replacement pane did not close')
    copied.unlink();page();before=len(calls());membership=a.snapshot()['project_state']['terminals']
    a.action('plugins:open-pane:pane-ui:tab')
    wait(lambda:'launcher is unavailable' in a.snapshot().get('status',''),'missing launcher did not report pre-launch failure')
    assert len(calls())==before and a.snapshot()['project_state']['terminals']==membership,'missing launcher allocated a dead terminal'
    assert a.snapshot()['controller_pid']==controller and alive(original_pid),'launcher failure disturbed the controller or original process'
# Force a target to disappear after preflight but before creation finishes.
# The response must retain the created backend and report placement separately.
import concurrent.futures
if menu():a.key('escape');wait(lambda:not menu(),'final pane page did not dismiss')
lease=frontend(a)['target'];baseline=len(calls())
pending=dict(plugin='pane-ui',entrypoint='tab',expected_digest=plugin()['digest'],project=name,frontend=lease)
with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
    with (tmp/'config/starflux/plugins.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        future=pool.submit(api,'plugins.panes.open',pending,True)
        inode_suffix=':'+str(os.fstat(lock.fileno()).st_ino)
        wait(lambda:any('->' in line and inode_suffix in line for line in Path('/proc/locks').read_text().splitlines()),'API creation did not wait after target preflight')
        subprocess.run([str(app),'--workspace',name,'--frontend',a.identity,'--close'],env=env,check=True,capture_output=True)
        a.process.wait(timeout=5)
        fcntl.flock(lock,fcntl.LOCK_UN)
    partial=future.result(timeout=20)
assert partial['presentation']['status']=='unconfirmed' and partial['presentation']['error']['code']=='stale_frontend',partial
wait(lambda:len(calls())==baseline+1,'lost presentation target lost or duplicated backend creation')
retained=partial['record'];assert any(p['record']['id']==retained['id'] and p['alive'] for p in records()),'created terminal was hidden by a presentation error'
api('plugins.panes.close',dict(id=retained['id'],expected_epoch=retained['epoch']),write=True)
assert alive(original_pid) and b.snapshot()['controller_pid']==other_lease['pid'],'target loss stopped an unrelated process or view'
print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,full_command_review=True,explicit_opt_in=True,overlay_restore=True,overlay_view_restart=True,split=True,zoomed=True,tab=True,pty_keyboard=True,owned_terminal_focus=True,no_command_replay=True,dismissed_callback_preserves_focus=True,independent_views=True,source_process_preserved=True,targeted_api_placement=True,targeted_api_focus=True,target_preflight_no_launch=True,targeted_operation_replay=True,creation_survives_target_loss=True,launcher_upgrade=args.plugin_pane_upgrade,popup_requires_frontend=True)))
