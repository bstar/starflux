"""Private native popup acceptance, loaded by test-session-manager.py."""
import copy,json,os,shlex,subprocess,uuid

def api(method,params=None,write=False,ok=True,operation=None):
    request=dict(version=1,id='plugin-popup-ui',method=method,params=params or {})
    if write:request['operation_id']=operation or str(uuid.uuid4())
    command=[str(app),'--api',json.dumps(request)]
    if args.ssh:command=ssh_args+[shlex.join(['env',*[k+'='+env[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','PATH','STARFLUX_HERDR') if k in env],*command])]
    result=json.loads(subprocess.check_output(command,env=env,timeout=20));assert result['ok']==ok,result
    return result.get('result',result.get('error'))
def menu(client=None):return (client or a).snapshot().get('context_menu') or {}
def popup(client=None):return (client or a).snapshot().get('plugin_popup')
def frontend(client):return next(v for v in api('frontends.list')['frontends'] if v['frontend']==client.identity)
def plugin():return api('plugins.list')['plugins'][0]
def calls():return [json.loads(line) for line in counter.read_text().splitlines()] if counter.exists() else []
def received():return inputs.read_bytes() if inputs.exists() else b''
def page():
    a.action('plugins:plugin:popup-ui')
    wait(lambda:any(i['action']=='plugins:open-pane:popup-ui:popup' for i in menu().get('items',[])),'popup entrypoint was not offered')
def check_membership():
    for client,before in [(a,saved_a),(b,saved_b)]:
        current=client.snapshot()['project_state']
        for key in ('terminals','terminal','terminal_epochs','terminal_plugins','pane_tree','pane_zoom','space'):
            assert current.get(key)==before.get(key),(key,current.get(key),before.get(key))
    assert len(api('panes.list',dict(project=name))['panes'])==initial_panes,'popup entered pane membership'
    assert not api('plugins.panes.list')['panes'],'popup entered persistent plugin pane ownership'
def request(client=None,**options):
    return dict(plugin='popup-ui',entrypoint='popup',expected_digest=plugin()['digest'],project=name,frontend=frontend(client or a)['target'],**options)
def opening(client=None,**options):
    response=api('plugins.panes.open',request(client,**options),write=True)
    assert response['kind']=='popup' and response['presentation']['status']=='applied',response
    info=response['popup'];assert info['acknowledgement']=='pty_started' and 'pane' not in info,info
    wait(lambda:(p:=popup(client)) and p['id']==info['id'],'acknowledged popup did not appear')
    return info['id']
def close(identifier,client=None):
    response=api('plugins.popups.close',dict(id=identifier,frontend=frontend(client or a)['target']),write=True)
    assert response['status']=='applied',response
    wait(lambda:not popup(client),'popup did not close')
def group_stopped(call):
    return not alive(call['pid']) and not alive(call['child'])
def pending_frame(identifier):
    return next((f for f in reversed(a.all_frames) if f['stream']=='plugin-popup-'+identifier),None)
def raw(code,modifiers=0):a.input(dict(kind='key',code=code,modifiers=modifiers))
def mark_ready(identifier,count):
    wait(lambda:(p:=popup()) and p['id']==identifier,'popup snapshot did not catch up with its acknowledgement')
    wait(lambda:len(calls())==count,'popup ran zero or multiple commands')
    wait(lambda:any(f['stream']=='plugin-popup-'+identifier and 'café' in ''.join(r['text'] for row in f['rows'] for r in row['runs']) for f in a.all_frames),'popup did not render Unicode output in native terminal cells')
    call=calls()[-1]
    assert call['context']['plugin_popup_id']==identifier and call['context']['focused_pane']['pane']==saved_a['terminal']+1
    assert 'pane' not in call['context'] and call['no_pane_environment'],'popup inherited ordinary pane identity'
    assert call['tty'] and call['context']['project']['id']==name,call
    return call

package=tmp/'popup-ui-package';package.mkdir();counter=tmp/'popup-starts';inputs=tmp/'popup-input';sizes=tmp/'popup-sizes'
program=package/'popup'
program.write_text('''#!/usr/bin/python3
import fcntl,json,os,signal,struct,subprocess,sys,termios,tty
from pathlib import Path
c=json.loads(os.environ['STARFLUX_PLUGIN_CONTEXT_JSON'])
child=subprocess.Popen(['/bin/sleep','600'])
tty.setraw(0)
with Path('''+repr(str(counter))+''').open('a') as f:f.write(json.dumps(dict(context=c,pid=os.getpid(),child=child.pid,tty=os.isatty(0) and os.isatty(1),no_pane_environment=all(k not in os.environ for k in ['STARFLUX_PANE','STARFLUX_PLUGIN_PANE_ID','STARFLUX_PLUGIN_JOB_ID'])))+'\\n')
def report(*args):
 rows,cols,_,_=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,b'\\0'*8))
 with Path('''+repr(str(sizes))+''').open('a') as f:f.write(str(cols)+'x'+str(rows)+'\\n')
 print('\\r\\nPOPUP_SIZE='+str(cols)+'x'+str(rows)+' café 界\\r\\n',flush=True)
signal.signal(signal.SIGWINCH,report)
print('\\x1b[?1000h\\x1b[?1006h',end='',flush=True)
report()
while True:
 data=os.read(0,4096)
 if not data:break
 with Path('''+repr(str(inputs))+''').open('ab') as f:f.write(data)
 if b'!' in data:raise SystemExit(0)
 if data==b'~':raise SystemExit(7)
 print('\\r\\nPOPUP_INPUT='+data.hex()+'\\r\\n',flush=True)
''');program.chmod(0o700)
manifest=dict(version=1,id='popup-ui',name='Popup UI proof',actions=[],panes=[dict(id='popup',title='Popup proof',argv=['package:popup'],placement='popup',width='80%',height=12)])
(package/'starflux-plugin.json').write_text(json.dumps(manifest))
def git(*arguments):
    return subprocess.check_output(['git','-C',str(package),*arguments],env={**env,'GIT_CONFIG_GLOBAL':'/dev/null','GIT_CONFIG_SYSTEM':'/dev/null'},stderr=subprocess.PIPE,text=True).strip()
if args.plugin_popups_managed:
    git('init','-q');git('config','user.name','Fixture');git('config','user.email','fixture@example.invalid');git('add','.');git('commit','-qm','v1')
    api('plugins.install',dict(repository=str(package),expected_revision=api('plugins.list')['revision']),write=True)
else:api('plugins.link',dict(path=str(package),expected_revision=api('plugins.list')['revision']),write=True)
a.action('plugins:plugin:popup-ui')
wait(lambda:'package:popup' in ''.join(i['label'] for i in menu().get('items',[])),'popup argv absent from command review')
assert not any(i['action'].startswith('plugins:open-pane:') for i in menu().get('items',[])),'disabled popup could launch'
p=plugin();api('plugins.configure',dict(plugin='popup-ui',expected_revision=api('plugins.list')['revision'],expected_digest=p['digest'],enabled=True),write=True)
page();assert not calls(),'review launched popup without selection'
saved_a=copy.deepcopy(a.snapshot()['project_state']);saved_b=copy.deepcopy(b.snapshot()['project_state'])
initial_panes=len(api('panes.list',dict(project=name))['panes'])
a.action('plugins:open-pane:popup-ui:popup')
identifier=wait(lambda:(p:=popup()) and p['id'],'popup menu selection did not open a transient terminal')
call=mark_ready(identifier,1);check_membership()
assert not popup(b),'popup appeared in another window'
status=api('plugins.popups.get',dict(id=identifier));assert status['alive'] and status['status']=='running' and 'pane' not in status,status
if args.plugin_popups_managed:
    retained=Path(plugin()['path'])
    rejected=api('plugins.uninstall',dict(plugin='popup-ui',expected_revision=api('plugins.list')['revision']),write=True,ok=False)
    assert 'active' in rejected['message'] and retained.is_dir() and alive(call['pid']),'managed uninstall removed a running popup version'
    program.write_text(program.read_text()+'\n# managed popup version two\n');git('add','.');git('commit','-qm','v2')
    api('plugins.update',dict(plugin='popup-ui',expected_revision=api('plugins.list')['revision']),write=True)
    current=plugin();assert not current['enabled'] and Path(current['path'])!=retained and retained.is_dir() and alive(call['pid']),'managed update replaced running popup files'
    api('plugins.configure',dict(plugin='popup-ui',expected_revision=api('plugins.list')['revision'],expected_digest=current['digest'],enabled=True),write=True)
replay=subprocess.run([str(app),'--plugin-popup-worker',str(tmp/'data/starflux/plugins/popups'),identifier],env=env,start_new_session=True,capture_output=True,timeout=10)
assert replay.returncode!=0 and len(calls())==1 and alive(call['pid']),'raw popup worker replay executed or stopped a program'
for code,modifiers,expected in [('escape',0,b'\x1b'),('char:b',2,b'\x02'),('f:1',0,b'\x1bOP')]:
    position=len(received());raw(code,modifiers)
    wait(lambda:expected in received()[position:],'popup key was intercepted by global navigation')
    assert not menu(),'popup key opened global menu'
a.paste('héllo 界');wait(lambda:'héllo 界'.encode() in received(),'popup paste did not reach PTY')
if args.cells:
    rect=popup()['terminal_rect'];a.input(dict(kind='pointer',action='down',button=0,modifiers=0,x=rect['x']+1,y=rect['y']+1));a.input(dict(kind='pointer',action='up',button=0,modifiers=0,x=rect['x']+1,y=rect['y']+1))
else:
    for action in ['down','up']:
        a.input(dict(kind='embedded',canvas='plugin-popup',event=dict(timestamp_ns=time.monotonic_ns(),sequence=a.serial+1,kind=dict(kind='pointer',action=action,button=0,position=[15,25],modifiers=0))))
wait(lambda:b'\x1b[<' in received(),'popup mouse input did not reach PTY')
# Both the originating controller's modal guard and the session-wide lease
# reject a second command without allocating a saved pane.
response=api('plugins.panes.open',request(),write=True)
assert response['presentation']['status']=='rejected' and response['presentation']['error']['code']=='frontend_busy',response
response=api('plugins.panes.open',request(b),write=True)
assert response['presentation']['status']=='rejected' and response['presentation']['error']['code']=='popup_busy',response
assert len(calls())==1;check_membership()
stale=api('plugins.popups.close',dict(id=str(uuid.uuid4()),frontend=frontend(a)['target']),write=True)
assert stale['status']=='rejected' and stale['error']['code']=='stale_popup' and popup()['id']==identifier,stale
for cols,rows in [(70,24),(40,12),(12,6),(100,35)]:
    a.resize(cols,rows,cols*10,rows*20)
    rect=wait(lambda:(p:=popup()) and p['terminal_rect'],'popup disappeared on resize')
    assert rect['x']+rect['width']<=cols and rect['y']+rect['height']<=rows
    wait(lambda:(f:=pending_frame(identifier)) and f['rect']==popup()['terminal_rect'],'popup frame did not track resized viewport')
    # SIGWINCH output uses the actual PTY size; frame metadata independently
    # checks that the host places those cells inside the current popup.
    wait(lambda:sizes.exists() and sizes.read_text().splitlines()[-1]==str(rect['width'])+'x'+str(rect['height']),'popup PTY did not receive its resized geometry')
close(identifier);wait(lambda:group_stopped(call),'closing popup left its program or child alive')
check_membership()
assert alive(original_pid) and alive(source_pid),'popup cleanup stopped an unrelated terminal'
a.key('f:1');wait(lambda:menu().get('title')=='Keyboard shortcuts','underlying UI focus was not restored')
response=api('plugins.panes.open',request(),write=True)
assert response['presentation']['status']=='rejected' and response['presentation']['error']['code']=='frontend_busy',response
a.key('escape');wait(lambda:not menu(),'help menu did not close')
without_target=request();without_target.pop('frontend')
assert api('plugins.panes.open',without_target,write=True,ok=False)['code']=='frontend_required'
for value in ['101%','abc','0%']:
    error=api('plugins.panes.open',request(width=value),write=True,ok=False)
    assert 'percent' in error['message'].lower(),error
assert len(calls())==1 and not popup(),'invalid popup dimensions launched a command'
# API operation replay returns the same receipt and never spawns a second PTY.
operation=str(uuid.uuid4());parameters=request(width=30,height='60%')
response=api('plugins.panes.open',parameters,write=True,operation=operation)
identifier=response['popup']['id'];call=mark_ready(identifier,2)
assert api('plugins.panes.open',parameters,write=True,operation=operation)['popup']['id']==identifier and len(calls())==2
assert popup()['rect']['width']==30 and popup()['terminal_rect']['width']==28
raw('char:!');wait(lambda:not popup(),'normal popup exit did not dismiss')
wait(lambda:group_stopped(call),'natural exit left a background child alive')
assert api('plugins.popups.get',dict(id=identifier))['exit_code']==0
# Nonzero application exit is visible and keeps its true status despite the
# supervisor's unconditional process-group cleanup.
identifier=opening(width=35,height=10);call=mark_ready(identifier,3)
raw('char:~');wait(lambda:not popup(),'failed popup did not dismiss')
wait(lambda:group_stopped(call),'failed popup retained a background child')
status=api('plugins.popups.get',dict(id=identifier));assert status['status']=='failed' and status['exit_code']==7 and status['error'],status
wait(lambda:'Popup command exited' in a.snapshot().get('status',''),'popup failure was not visible')
# The header close control works with pointer coordinates in either presentation.
identifier=opening();call=mark_ready(identifier,4);rect=popup()['rect']
a.input(dict(kind='pointer',action='down',button=0,modifiers=0,x=rect['x']+rect['width']-2,y=rect['y']))
wait(lambda:not popup(),'popup header close was not clickable')
wait(lambda:group_stopped(call),'header close retained popup processes')
# A controller refresh cannot restore or rerun a transient terminal. Only this
# fixture's view exits; its ordinary pane and the sibling controller survive.
identifier=opening();call=mark_ready(identifier,5);a=restart(a)
wait(lambda:group_stopped(call),'view close retained popup processes')
assert not popup() and len(calls())==5,'transient popup was restored or replayed'
check_membership()
# If the supervisor itself is killed, the controller cleans its still-reserved
# process group before reaping it. The unrelated pane must remain untouched.
identifier=opening();call=mark_ready(identifier,6);supervisor=popup()['process']
assert api('plugins.popups.get',dict(id=identifier))['alive']
assert b'--plugin-popup-worker' in Path('/proc',str(supervisor),'cmdline').read_bytes().split(b'\0')
os.kill(supervisor,signal.SIGKILL)
wait(lambda:not popup() and group_stopped(call),'supervisor death left popup descendants alive')
assert alive(original_pid) and alive(source_pid),'supervisor cleanup stopped an unrelated terminal'
# Test abrupt owner death independently of graceful teardown. Its exact private
# PID is verified before signalling; no live user controller is involved.
identifier=opening();call=mark_ready(identifier,7);controller=a.snapshot()['controller_pid']
argv=Path('/proc',str(controller),'cmdline').read_bytes().split(b'\0')
assert b'--serve-view' in argv and a.identity.encode() in argv
os.kill(controller,signal.SIGKILL)
wait(lambda:group_stopped(call),'owner death left popup or ordinary background child alive')
a.process.wait(timeout=8)
(state/(a.identity+'.state.json')).unlink(missing_ok=True)
a=Client(a.identity,remote=args.ssh)
wait(lambda:a.scene and a.snapshot().get('panes') and a.snapshot()['panes'][0].get('pid'),'view did not reopen after abrupt death')
assert not popup() and len(calls())==7,'owner death replayed transient popup'
check_membership();assert alive(original_pid) and alive(source_pid),'owner death killed another terminal'
identifier=opening();call=mark_ready(identifier,8);close(identifier)
wait(lambda:group_stopped(call),'session lease could not be reused after owner death')
assert not api('plugins.panes.list')['panes']
if getattr(args,'plugin_popup_keyboard',False):
    exec(compile((root/'scripts/test-plugin-popup-keyboard.py').read_text(),str(root/'scripts/test-plugin-popup-keyboard.py'),'exec'),globals())
if args.plugin_popups_managed:
    api('plugins.uninstall',dict(plugin='popup-ui',expected_revision=api('plugins.list')['revision']),write=True)
    assert not api('plugins.list')['plugins'] and package.is_dir(),'managed uninstall removed the source repository or retained registry entry'
print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,full_command_review=True,explicit_opt_in=True,transient_no_pane_membership=True,underlying_focus_context=True,raw_keyboard_and_paste=True,pty_mouse=True,bounded_resize=True,modal_and_session_singleton=True,stale_close_guard=True,operation_replay=True,natural_and_failed_exit=True,owned_group_cleanup=True,view_restart_no_restore=True,supervisor_death_cleanup=True,owner_death_cleanup=True,independent_views=True,source_processes_preserved=True,managed_update_and_uninstall=args.plugin_popups_managed)))
