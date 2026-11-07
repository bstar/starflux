#!/usr/bin/env python3
"""Private persistent-terminal acceptance, invoked by test-plugins.py --panes."""
import json,os,subprocess,uuid
from pathlib import Path

def git(*args):
    return subprocess.check_output(['git','-C',str(package),*args],env=env,text=True,stderr=subprocess.PIPE).strip()

def plugin():return api('plugins.list')['plugins'][0]
def enable():
    p=plugin();api('plugins.configure',dict(plugin=p['id'],enabled=True,expected_digest=p['digest'],expected_revision=api('plugins.list')['revision']),write=True)
def opened(operation=None,ok=True):
    return api('plugins.panes.open',dict(plugin='pane-fixture',entrypoint='board',expected_digest=plugin()['digest'],project=project),write=True,operation=operation,ok=ok)
def pane_record(identity):
    return next(p for p in api('plugins.panes.list')['panes'] if p['record']['id']==identity)
def alive(identity):return pane_record(identity)['alive']
def close(record,ok=True,epoch=None):return api('plugins.panes.close',dict(id=record['id'],expected_epoch=epoch or record['epoch']),write=True,ok=ok)
def calls():return [json.loads(line) for line in counter.read_text().splitlines()] if counter.exists() else []

env['SHELL']='/bin/sh'
env['GIT_CONFIG_GLOBAL']='/dev/null'
env['GIT_CONFIG_SYSTEM']='/dev/null'
counter=tmp/'pane-calls';manifest=dict(version=1,id='pane-fixture',name='Persistent fixture',actions=[],panes=[dict(id='board',title='Board 界',argv=['package:board','literal space $(touch NEVER_PANE)'],placement='tab')])
(package/'starflux-plugin.json').write_text(json.dumps(manifest))
script=package/'board'
script.write_text('''#!/usr/bin/python3
import json,os,sys
from pathlib import Path
assert os.isatty(0) and os.isatty(1)
c=json.loads(os.environ['STARFLUX_PLUGIN_CONTEXT_JSON'])
assert os.environ['STARFLUX_PLUGIN_ENTRYPOINT_ID']=='board'
assert c['plugin_pane_id']==os.environ['STARFLUX_PLUGIN_PANE_ID']
with Path('''+repr(str(counter))+''').open('a') as out:out.write(json.dumps(dict(context=c,argv=sys.argv[1:],pid=os.getpid(),version=1))+'\\n')
print('PLUGIN_PANE_READY café 界',flush=True)
for line in sys.stdin:print('PANE_REPLY='+line.strip(),flush=True)
''');script.chmod(0o700)
git('init','-q');git('config','user.name','Fixture');git('config','user.email','fixture@example.invalid');git('add','.');git('commit','-qm','v1')
project=None;records=[]
try:
    created=api('sessions.create',dict(name='Plugin pane proof',directory=str(tmp)),write=True);project=created['project']
    initial=api('panes.get',dict(project=project,pane=1));protected=initial['target']['pid'];protected_stamp=stamp(protected)
    api('plugins.install',dict(repository=str(package),expected_revision=api('plugins.list')['revision']),write=True)
    opened(ok=False);assert not calls(),'disabled pane command ran'
    enable();operation=str(uuid.uuid4());record=opened(operation)['record'];records.append(record)
    wait(lambda:alive(record['id']) and calls(),'pane did not start with a real PTY')
    assert opened(operation)['record']['id']==record['id'],'operation replay created another terminal'
    assert len(calls())==1 and calls()[0]['argv']==['literal space $(touch NEVER_PANE)']
    assert not (Path(record['source'])/'NEVER_PANE').exists()
    target=api('panes.get',dict(project=project,pane=record['pane']+1))['target']
    api('panes.send',dict(target=target,text='hello 界',submit=True),write=True)
    wait(lambda:'PANE_REPLY=hello 界' in api('panes.capture',dict(project=project,pane=record['pane']+1))['text'],'interactive plugin input failed')
    close(record,ok=False,epoch=str(uuid.uuid4()));assert alive(record['id']),'stale close stopped the plugin'
    metadata=data/'panes'/('pane-'+record['id'].replace('-','')+'.json')
    saved=metadata.read_bytes();poison=json.loads(saved)
    poison.update(status='launching',boot_id='088f251c-a7de-43cd-a07c-3032d2c12f71',pid=None,start_ticks=None)
    metadata.write_text(json.dumps(poison));close(record,ok=False)
    assert stamp(calls()[0]['pid']) is not None and stamp(protected)==protected_stamp,'old-boot launch receipt authorized a close'
    metadata.write_bytes(saved)
    # A consumed terminal launcher cannot be replayed by another attachment.
    consumed=subprocess.run([str(app),'--plugin-pane',str(data/'panes'),record['id']],env=env,capture_output=True)
    assert consumed.returncode!=0 and len(calls())==1,'replayed launcher executed plugin code'
    old_source=Path(record['source']);old_pid=calls()[0]['pid'];old_stamp=stamp(old_pid)
    script.write_text(script.read_text().replace('version=1','version=2'));git('add','.');git('commit','-qm','v2')
    api('plugins.update',dict(plugin='pane-fixture',expected_revision=api('plugins.list')['revision']),write=True)
    assert not plugin()['enabled'] and old_source.is_dir() and stamp(old_pid)==old_stamp,'update changed the running pane'
    denied=api('plugins.uninstall',dict(plugin='pane-fixture',expected_revision=api('plugins.list')['revision']),write=True,ok=False)
    assert 'terminal' in denied['message'].lower(),denied
    enable();second=opened()['record'];records.append(second)
    wait(lambda:len(calls())==2 and alive(second['id']),'updated pane did not start')
    assert calls()[1]['version']==2 and Path(second['source'])!=old_source
    close(record);wait(lambda:stamp(old_pid)!=old_stamp,'owned plugin process did not end')
    assert alive(second['id']) and stamp(protected)==protected_stamp,'closing one plugin stopped another terminal'
    close(second);wait(lambda:not alive(second['id']),'second plugin did not close')
    close(second) # explicit repeated close remains harmless
    api('plugins.uninstall',dict(plugin='pane-fixture',expected_revision=api('plugins.list')['revision']),write=True)
    assert stamp(protected)==protected_stamp,'uninstall stopped the project shell'
    print(json.dumps(dict(result='PASS',ssh=args.ssh,persistent_pty=True,literal_argv=True,interactive_input=True,one_shot_launch=True,idempotency=True,stale_close_guard=True,reboot_close_guard=True,managed_update_retains_live_source=True,busy_uninstall_guard=True,independent_terminals=True,closed_uninstall=True,project_process_preserved=True)))
finally:
    if project:
        subprocess.run(['tmux','-L','starflux-'+project,'kill-server'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
