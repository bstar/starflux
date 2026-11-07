"""Private plugin UI acceptance; run through test-session-manager.py --plugin-ui."""
import json, os, shlex, subprocess, time, uuid
from pathlib import Path

def api(method,params=None,write=False,ok=True):
    request=dict(version=1,id='plugin-ui',method=method,params=params or {})
    if write:request['operation_id']=str(uuid.uuid4())
    command=[str(app),'--api',json.dumps(request)]
    if args.ssh:command=ssh_args+[shlex.join(['env',*[key+'='+env[key] for key in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','PATH','SHELL','TMUX_TMPDIR','STARFLUX_HERDR') if key in env],*command])]
    response=json.loads(subprocess.check_output(command,env=env,timeout=15));assert response['ok']==ok,response
    return response.get('result',response.get('error'))

def menu(client=a):
    return wait(lambda:(m if (m:=client.snapshot().get('context_menu')) and not client.snapshot().get('plugin_workspace',{}).get('loading',True) and m['title']=='Plugins' and m['items'] and m['items'][0]['label']!='Loading workflows…' else None),'plugin menu did not load')

def opened(client=a):
    if client.snapshot().get('context_menu'):
        client.key('escape');wait(lambda:not client.snapshot().get('context_menu'),'previous menu did not close')
    generation=client.snapshot().get('plugin_workspace',{}).get('generation',0)
    client.shortcut('char:U',1)
    settled(client,generation)
    return menu(client)

def settled(client,generation):
    wait(lambda:(s:=client.snapshot().get('plugin_workspace',{})).get('generation',0)>generation and not s.get('loading',True),'plugin request did not settle')

def accept_plugin():
    generation=a.snapshot().get('plugin_workspace',{}).get('generation',0)
    a.accept();settled(a,generation)

def choose(action,client=a,keyboard=False):
    generation=client.snapshot().get('plugin_workspace',{}).get('generation',0)
    def applied():
        if action.startswith('plugins:') and action not in ('plugins:link','plugins:install','plugins:link-url') and not action.startswith(('plugins:update:','plugins:uninstall:')):settled(client,generation)
    m=wait(lambda:(m if (m:=client.snapshot().get('context_menu')) and any(i['action']==action for i in m['items']) else None),'plugin control missing: '+action)
    index=next(i for i,v in enumerate(m['items']) if v['action']==action)
    r=m['bounds'];capacity=max(1,r['height']-2 if args.cells else (r['height']-63)//34)
    offset=(m.get('selected') or 0)//capacity*capacity
    if keyboard or not offset<=index<offset+capacity:
        client.key('home')
        for _ in range(index):client.key('down')
        wait(lambda:(client.snapshot().get('context_menu')or{}).get('selected')==index,'plugin control did not receive keyboard focus')
        if keyboard:client.key('enter');applied();return
        m=menu(client);r=m['bounds'];offset=index//capacity*capacity
    x=r['x']+10;y=r['y']+(index-offset+1 if args.cells else 63+34*(index-offset)+16)
    label=m['items'][index]['label']
    def rendered():
        if args.cells:
            return label[:12].strip() in ''.join(span.get('text','') for span in client.scene.get('spans',[]) if span.get('y')==y)
        return any(node.get('text')==label and node.get('rect',{}).get('y')==y-9 for component in client.scene.get('components',[]) if component['kind']=='surface' for node in component['surface'].get('nodes',[]))
    wait(rendered,'plugin pointer target was not presented: '+label)
    pointer=dict(kind='pointer',action='down',button=0,modifiers=0,x=x if args.cells else x//10,y=y if args.cells else y//20)
    if not args.cells:pointer['pixel']=[x,y]
    client.input(pointer)
    applied()

def plugin():
    return next((p for p in api('plugins.list')['plugins'] if p['id']=='ui-fixture'),None)

def job(identity):return api('plugins.job',dict(job_id=identity))

def jobs():return api('plugins.jobs',dict(plugin='ui-fixture'))['jobs']

package=tmp/'UI plugin with spaces 界';package.mkdir()
script=package/'runner'
script.write_text('''#!/usr/bin/python3
import json,os,subprocess,sys,time
from pathlib import Path
context=json.load(sys.stdin)
assert context['project']['id']==PROJECT
with Path(CALLS).open('a') as f:f.write(json.dumps(context)+'\\n')
if sys.argv[1]=='slow':
 child=subprocess.Popen(['/bin/sleep','60'])
 Path(PIDS).write_text(json.dumps([os.getpid(),child.pid]))
 print('SLOW_RUNNING',flush=True);time.sleep(60)
else:
 for i in range(90):print('LOG_%03d café 界'%i)
 print('LONG_START_'+('x'*600)+'_LONG_END_界')
 print('\\x1b[31mCONTROL_LOG\\x1b[0m',file=sys.stderr)
'''.replace('PROJECT',repr(name)).replace('CALLS',repr(str(package/'calls'))).replace('PIDS',repr(str(package/'pids'))))
script.chmod(0o700)
long='argument with spaces 界 '+('review '*60)+'REVIEW_END_界 $(touch NEVER_UI)'
manifest=dict(version=1,id='ui-fixture',name='UI fixture 界',description='Native plugin review and cancellation fixture.',actions=[dict(id='echo',title='Echo context',argv=['package:runner','echo',long]),dict(id='slow',title='Slow cancellable command',argv=['package:runner','slow'])])
source=package/'starflux-plugin.json';source.write_text(json.dumps(manifest))
manifest['link_handlers']=[dict(id='issue',title='Show linked issue',action='echo',schemes=['https'],hosts=['example.invalid'])]
manifest['link_handlers'] += [dict(id='extra'+str(i),title='Additional handler '+str(i),action='echo',schemes=['https'],hosts=['example.invalid']) for i in range(16)]
source.write_text(json.dumps(manifest))
# Open entirely through the configured keyboard command, then link through the prompt.
opened()
choose('plugins:link',keyboard=True)
wait(lambda:a.snapshot().get('modal_open'),'plugin link dialog did not open')
a.paste(str(package));a.accept()
wait(lambda:plugin() is not None,'UI link did not register package')
assert not plugin()['enabled'] and not (package/'calls').exists()
choose('plugins:plugin:ui-fixture')
m=menu();wait(lambda:'REVIEW_END_界' in ''.join(i['label'] for i in menu()['items']),'review clipped the declared command')
assert not any(i['action'].startswith('plugins:run:') for i in menu()['items']),'disabled plugin offered execution'
choose('menu:panel',keyboard=True)
wait(lambda:(a.snapshot().get('context_menu')or{}).get('title')=='Plugins','informational row dismissed plugin review')
choose('plugins:toggle:ui-fixture',keyboard=True)
wait(lambda:plugin()['enabled'],'UI enable did not persist')
choose('plugins:run:ui-fixture:echo')
completed=wait(lambda:next((j for j in jobs() if j['action']['id']=='echo' and j['status']=='succeeded'),None),'UI command did not finish')
identity=completed['id'];assert not (package/'NEVER_UI').exists()
choose('plugins:job:'+identity+':0',keyboard=True)
wait(lambda:any(i['label'].startswith('LOG_') for i in menu()['items']),'job output did not render')
seen='';page=0
while True:
    m=menu();seen+=''.join(i['label'] for i in m['items'])
    next_action=next((i['action'] for i in m['items'] if i['label']=='Next output page'),None)
    if next_action is None:break
    choose(next_action,keyboard=True);page+=1
    wait(lambda:any(i['label']=='Refresh job' and i['action']=='plugins:job:'+identity+':'+str(page*40) for i in menu()['items']),'job log page did not change')
    assert page<20,'log pagination failed to make progress'
assert 'LOG_089' in seen and '_LONG_END_界' in seen and '␛[31mCONTROL_LOG␛[0m' in seen,'log rendering lost output or emitted active terminal controls'
assert '\x1b' not in seen,'plugin output injected terminal controls'
choose('plugins:list');choose('plugins:link-url',keyboard=True)
wait(lambda:a.snapshot().get('modal_open'),'link URL prompt missing')
url='https://example.invalid/issue/界?q=%24%28touch%20NEVER_UI_LINK%29#details'
a.paste(url);accept_plugin()
wait(lambda:any(i['action']=='plugins:open-link:ui-fixture:issue' for i in menu()['items']),'matching link handler missing')
choose('plugins:links-page:16',keyboard=True)
wait(lambda:any(i['action']=='plugins:open-link:ui-fixture:extra15' for i in menu()['items']),'last handler page missing')
choose('plugins:links-page:0',keyboard=True)
assert not any((j.get('event') or {}).get('kind')=='link.opened' for j in jobs()),'resolving a URL executed plugin code'
choose('plugins:open-link:ui-fixture:issue',keyboard=True)
link_job=wait(lambda:next((j for j in jobs() if (j.get('event')or{}).get('kind')=='link.opened' and j['status']=='succeeded'),None),'native link handler did not finish')
assert link_job['event']['url']==url and link_job['event']['handler']=='issue'
assert not (package/'NEVER_UI_LINK').exists()
# Visible terminal URLs route through the same explicit handler review. Hover
# regions come from the emitted grid, without registry work on pointer motion.
a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'plugin view did not close')
terminal_url='https://example.invalid/terminal-view?q=literal#details'
a.all_frames=[]
a.paste(terminal_url);a.key('enter')
def terminal_url_point():
    for frame in reversed(a.all_frames):
        for row in frame['rows']:
            text=''.join(run['text'] for run in row['runs'])
            column=text.find('https://example.invalid/terminal-view')
            if column>=0:
                return frame['rect']['x']+column+3,frame['rect']['y']+row['index']
point=wait(terminal_url_point,'visible terminal URL was not rendered')
wait(lambda:any(r['x']<=point[0]<r['x']+r['width'] and r['y']<=point[1]<r['y']+r['height'] for r in a.scene.get('pointer_regions',[])),'visible URL did not publish hand-pointer region')
def link_pointer(action,button,modifiers=0):
    if args.cells:a.input(dict(kind='pointer',action=action,button=button,modifiers=modifiers,x=point[0],y=point[1]))
    elif button==1:a.input(dict(kind='pointer',action=action,button=button,modifiers=modifiers,x=point[0],y=point[1],pixel=[point[0]*10+5,point[1]*20+10]))
    else:
        canvas=a.snapshot()['pane_layout']['canvas']
        a.input(dict(kind='embedded',canvas='terminals',event=dict(timestamp_ns=time.monotonic_ns(),sequence=a.serial+1,kind=dict(kind='pointer',action=action,button=button,modifiers=modifiers,position=[point[0]*10+5-canvas['x'],point[1]*20+10-canvas['y']]))))
link_pointer('down',1);link_pointer('up',1)
wait(lambda:(a.snapshot().get('context_menu')or{}).get('title')=='Terminal link','terminal URL context menu missing')
assert not (b.snapshot().get('context_menu')or{}).get('title')=='Terminal link'
a.key('down');a.key('enter')
wait(lambda:terminal_url in a.clipboards,'Copy URL did not preserve the visible URL')
a.shortcut('char:l')
wait(lambda:(a.snapshot().get('context_menu')or{}).get('title')=='Terminal links','keyboard URL picker missing')
link_items=a.snapshot()['context_menu']['items']
selected=next(i for i,item in enumerate(link_items) if item['label']==terminal_url)
a.key('home')
for _ in range(selected):a.key('down')
a.key('enter')
wait(lambda:(a.snapshot().get('context_menu')or{}).get('title')=='Terminal link','keyboard URL did not open link options')
a.key('enter')
wait(lambda:any(i['action']=='plugins:open-link:ui-fixture:issue' for i in menu()['items']),'keyboard URL did not review handlers')
a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'keyboard handler review did not close')
link_pointer('down',0,2);link_pointer('up',0,2)
wait(lambda:any(i['action']=='plugins:open-link:ui-fixture:issue' for i in menu()['items']),'Ctrl-click did not review visible terminal URL')
assert not any((j.get('event')or{}).get('url')==terminal_url for j in jobs()),'Ctrl-click executed a handler before selection'
choose('plugins:open-link:ui-fixture:issue',keyboard=True)
wait(lambda:any((j.get('event')or{}).get('url')==terminal_url and j['status']=='succeeded' for j in jobs()),'visible terminal URL handler did not finish')
# OSC 8 labels have independent destinations, including more semicolons than
# the underlying VTE callback parameter limit. Reattach without restarting the
# backend, then exercise hidden-target copy and explicit handler delivery.
a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'handler result did not close')
hidden='https://example.invalid/hidden'+(';part'*20)+'?q=destination#OSC8'
a.all_frames=[];a.action('reconnect-terminal:0')
def hidden_point():
    for frame in reversed(a.all_frames):
        for row in frame['rows']:
            text=''.join(run['text'] for run in row['runs']);column=text.find('OSC8_LABEL_')
            if column>=0:return frame['rect']['x']+column+2,frame['rect']['y']+row['index']
point=wait(hidden_point,'OSC 8 label did not survive reattachment')
wait(lambda:any(r['x']<=point[0]<r['x']+r['width'] and r['y']<=point[1]<r['y']+r['height'] for r in a.scene.get('pointer_regions',[])),'hidden destination did not publish a pointer region')
link_pointer('down',1);link_pointer('up',1)
wait(lambda:(a.snapshot().get('context_menu')or{}).get('title')=='Terminal link','OSC 8 context menu missing')
a.key('down');a.key('enter');wait(lambda:hidden in a.clipboards,'OSC 8 copy lost or substituted its destination')
assert a.snapshot()['panes'][0]['pid']==original_pid and alive(original_pid),'OSC 8 reattachment restarted the backend'
screen_path=tmp/'data/starflux/screens'/name/'p0.screen.json'
def saved_hidden():
    try:
        import base64
        return hidden in base64.b64decode(json.loads(screen_path.read_text())['formatted_base64']).decode()
    except (OSError,ValueError):return False
wait(saved_hidden,'cold screen preview discarded OSC 8 destination',timeout=25)
link_pointer('down',0,2);link_pointer('up',0,2)
wait(lambda:any(i['action']=='plugins:open-link:ui-fixture:issue' for i in menu()['items']),'OSC 8 Ctrl-click did not review handlers')
assert not any((j.get('event')or{}).get('url')==hidden for j in jobs()),'OSC 8 activation executed before review'
choose('plugins:open-link:ui-fixture:issue',keyboard=True)
wait(lambda:any((j.get('event')or{}).get('url')==hidden and j['status']=='succeeded' for j in jobs()),'OSC 8 handler did not receive its full destination')
choose('plugins:plugin:ui-fixture')
choose('plugins:run:ui-fixture:slow',keyboard=True)
running=wait(lambda:next((j for j in jobs() if j['action']['id']=='slow' and j['status']=='running'),None),'slow UI job did not start')
pids=wait(lambda:json.loads((package/'pids').read_text()) if (package/'pids').exists() else None,'job children absent')
choose('plugins:job:'+running['id']+':0')
choose('plugins:cancel:'+running['id'],keyboard=True)
wait(lambda:job(running['id'])['status']=='cancelled','UI cancel did not finish')
wait(lambda:all(not alive(pid) for pid in pids),'UI cancel left children running')
# A second frontend sees the shared registry, without moving the first frontend's menu.
opened(b);choose('plugins:plugin:ui-fixture',client=b)
wait(lambda:any(i['action']=='plugins:toggle:ui-fixture' for i in menu(b)['items']),'second view did not show enabled plugin')
choose('plugins:plugin:ui-fixture')
# Stale revision must fail visibly; it must not silently overwrite an external edit.
c=api('plugins.list');p=plugin()
api('plugins.configure',dict(plugin='ui-fixture',enabled=False,expected_digest=p['digest'],expected_revision=c['revision']),write=True)
choose('plugins:toggle:ui-fixture')
wait(lambda:any('registry changed' in i['label'] for i in menu()['items']),'stale UI revision was not reported')
assert not plugin()['enabled']
choose('plugins:list');choose('plugins:plugin:ui-fixture');choose('plugins:toggle:ui-fixture')
wait(lambda:plugin()['enabled'],'reloaded plugin did not enable')
# Narrow layouts must keep controls and complete argv review accessible.
a.resize(45,16,450,320);opened();choose('plugins:plugin:ui-fixture',keyboard=True)
wait(lambda:'REVIEW_END_界' in ''.join(i['label'] for i in menu()['items']),'narrow review lost command text')
choose('plugins:toggle:ui-fixture',keyboard=True);wait(lambda:not plugin()['enabled'],'narrow keyboard control did not work')
# Closing/replacing a loading view cannot resurrect its asynchronous callback.
a.action('plugins:list');a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'plugin menu did not close')
a.action('plugins:list');a.action('workspace:projects');wait(lambda:(a.snapshot().get('context_menu')or{}).get('title')=='Projects','replacement menu missing')
end=time.monotonic()+1
wait(lambda:time.monotonic()>end and (a.snapshot().get('context_menu')or{}).get('title')=='Projects','plugin callback replaced a different menu')
a.key('escape');opened();choose('plugins:plugin:ui-fixture');choose('plugins:unlink:ui-fixture',keyboard=True)
wait(lambda:plugin() is None,'UI unlink did not persist')
assert source.exists() and (package/'calls').exists() and job(identity)['status']=='succeeded'
assert alive(original_pid) and a.snapshot()['panes'][0]['pid']==original_pid,'plugin UI replaced a project terminal'
assert not b.snapshot().get('modal_open'),'plugin UI opened a dialog in another frontend'
# Run the shipped example against a private repository, never a user project.
repository=tmp/'example-repository';repository.mkdir()
subprocess.run(['git','init','-q',str(repository)],env=env,check=True,capture_output=True)
example_project=api('sessions.create',dict(name='Plugin example',directory=str(repository)),write=True)['project']
c=api('plugins.list')
example=api('plugins.link',dict(path=str(root/'examples/plugins/project-summary'),expected_revision=c['revision']),write=True)
p=next(p for p in example['plugins'] if p['id']=='org.starflux.project-summary')
api('plugins.configure',dict(plugin=p['id'],enabled=True,expected_digest=p['digest'],expected_revision=example['revision']),write=True)
example_job=api('plugins.run',dict(plugin=p['id'],action='summary',expected_digest=p['digest'],project=example_project),write=True)
wait(lambda:job(example_job['id'])['status']=='succeeded','shipped plugin example failed')
assert 'Project: '+example_project in job(example_job['id'])['output']['stdout']
c=api('plugins.list');api('plugins.unlink',dict(plugin=p['id'],expected_revision=c['revision']),write=True)
# Managed source install/update/offline rollback/uninstall through native controls.
managed=tmp/'managed UI repository 界';managed.mkdir()
def git(*args):return subprocess.check_output(['git','-c','core.hooksPath=/dev/null','-C',str(managed),*args],env=env,stderr=subprocess.PIPE,text=True).strip()
git('init','-q','-b','main');git('config','user.name','UI package fixture');git('config','user.email','ui@example.invalid')
managed_manifest=dict(version=1,id='managed-ui',name='Managed UI fixture',actions=[dict(id='echo',title='Show pinned version',argv=['package:runner'])])
(managed/'starflux-plugin.json').write_text(json.dumps(managed_manifest))
(managed/'runner').write_text('#!/usr/bin/python3\nimport json,sys\nfrom pathlib import Path\njson.load(sys.stdin)\nprint(Path(__file__).with_name("version").read_text())\n');(managed/'runner').chmod(0o700)
(managed/'version').write_text('UI_VERSION_ONE');git('add','.');git('commit','-qm','one');git('tag','v1')
(managed/'version').write_text('UI_VERSION_TWO');git('add','.');git('commit','-qm','two');git('tag','v2')
def managed_entry():return next((p for p in api('plugins.list')['plugins'] if p['id']=='managed-ui'),None)
opened();choose('plugins:install',keyboard=True)
wait(lambda:a.snapshot().get('modal_open'),'managed install dialog missing')
a.paste(str(managed));a.key('tab');a.paste('v1');accept_plugin()
wait(lambda:managed_entry() is not None,'UI installation failed')
first=managed_entry();assert Path(first['path']).is_relative_to(tmp/'data/starflux/plugin-packages'),'UI package escaped private fixture data'
assert not first['enabled'];original_release=first['managed']['release']
choose('plugins:plugin:managed-ui');choose('plugins:toggle:managed-ui',keyboard=True)
wait(lambda:managed_entry()['enabled'],'managed UI enable failed')
choose('plugins:run:managed-ui:echo')
wait(lambda:any(j['status']=='succeeded' and job(j['id'])['output']['stdout'].strip()=='UI_VERSION_ONE' for j in api('plugins.jobs',dict(plugin='managed-ui'))['jobs']),'managed UI version one did not execute')
# A damaged managed snapshot can still be disabled from the native UI.
asset=Path(first['path'])/'version';asset.write_text('CHANGED_AFTER_APPROVAL')
opened();choose('plugins:plugin:managed-ui');choose('plugins:disable:managed-ui',keyboard=True)
wait(lambda:not managed_entry().get('configured_enabled'),'damaged managed plugin could not be disabled')
asset.write_text('UI_VERSION_ONE')
opened();choose('plugins:plugin:managed-ui');choose('plugins:toggle:managed-ui',keyboard=True)
wait(lambda:managed_entry()['enabled'],'restored managed version did not enable')
choose('plugins:update:managed-ui',keyboard=True)
wait(lambda:a.snapshot().get('modal_open'),'update version dialog missing')
a.key('end');a.key('backspace');a.paste('2');accept_plugin()
wait(lambda:managed_entry()['managed']['reference']=='v2','UI update did not save selected version')
assert not managed_entry()['enabled'] and Path(first['path']).exists()
choose('plugins:versions:managed-ui')
choose('plugins:activate:managed-ui:'+original_release,keyboard=True)
wait(lambda:managed_entry()['managed']['release']==original_release,'UI rollback did not select retained version')
assert not managed_entry()['enabled']
choose('plugins:uninstall:managed-ui',keyboard=True)
wait(lambda:a.snapshot().get('modal_open'),'managed uninstall confirmation missing')
a.key('escape');wait(lambda:not a.snapshot().get('modal_open'),'uninstall confirmation did not cancel')
assert managed_entry() is not None and Path(first['path']).exists()
opened();choose('plugins:plugin:managed-ui');choose('plugins:uninstall:managed-ui')
wait(lambda:a.snapshot().get('modal_open'),'managed uninstall dialog did not reopen')
a.paste('UNINSTALL');accept_plugin();wait(lambda:managed_entry() is None,'managed UI uninstall failed')
assert managed.exists() and not Path(first['path']).parent.parent.exists()
assert alive(original_pid) and a.snapshot()['panes'][0]['pid']==original_pid
print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,keyboard_entry=True,link_opt_in=True,declared_link_handlers=True,handler_pagination=True,complete_command_review=True,execution=True,unicode=True,log_pagination=True,control_bytes_visible=True,cancel_child_cleanup=True,stale_revision=True,narrow_controls=True,closed_callbacks=True,independent_frontends=True,unlink_retains_files=True,shipped_example=True,managed_install=True,managed_update=True,version_rollback=True,damaged_package_disable=True,uninstall_cancel=True,owned_uninstall=True,terminal_preserved=True)))
