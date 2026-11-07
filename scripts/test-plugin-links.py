"""Declared plugin link handlers; run with test-plugins.py --links [--ssh]."""
import copy, json, shutil, uuid

manifest['link_handlers']=[dict(id='issues',title='Show issue',action='echo',schemes=['https'],hosts=['example.invalid']),
    dict(id='custom',title='Show custom link',action='echo',schemes=['sf-issue'])]
manifest['actions'].append(dict(id='project',title='Project action',argv=['package:runner','echo'],cwd='project'))
manifest['link_handlers'].append(dict(id='project',title='Project issue',action='project',schemes=['project-issue']))
source.write_text(json.dumps(manifest))
linked=api('plugins.link',dict(path=str(package),expected_revision='initial'),write=True)
url='https://EXAMPLE.invalid:8443/issue/界?q=%24%28touch%20NEVER_LINK%29#details'
def resolve(value=url):return api('plugins.resolve_link',dict(url=value))
assert not resolve()['handlers'] and not calls(),'disabled plugin handled URL'
configure(True)
offered=resolve();assert len(offered['handlers'])==1 and not calls(),'resolution executed a handler'
digest=offered['handlers'][0]['digest']
def open_link(value=url,handler='issues',plugin='fixture',expected=digest,operation=None,ok=True,scope=None,project=None):
    return api('plugins.open_link',dict(plugin=plugin,handler=handler,url=value,expected_digest=expected,project=project),
        write=True,operation=operation,ok=ok,scope=scope)
for value in ['https://example.invalid.evil.test/', 'https://evil.test/example.invalid','http://example.invalid/']:
    assert not resolve(value)['handlers'];open_link(value,ok=False)
for value in ['https://user:password@example.invalid/','https://example.invalid/a b','/relative','x:'+'a'*8192,'https://example.invalid/\x1b']:
    api('plugins.resolve_link',dict(url=value),ok=False);open_link(value,ok=False)
open_link(handler='missing',ok=False)
open_link(value='project-issue:123',handler='project',ok=False)
operation=str(uuid.uuid4());record=open_link(operation=operation)
finished(record['id'],'succeeded')
out=json.loads(job(record['id'])['output']['stdout'])
assert out['context']['event']==dict(kind='link.opened',handler='issues',url=url)
assert out['argv']==['echo','literal space 界 $(touch NEVER)']
assert not (package/'NEVER_LINK').exists() and not (package/'NEVER').exists()
assert open_link(operation=operation)['id']==record['id']
assert sum(c['context']['job_id']==record['id'] for c in calls())==1
open_link(value=url+'other',operation=operation,ok=False)
custom=open_link(value='sf-issue:project/123',handler='custom');finished(custom['id'],'succeeded')
project_directory=tmp/'link-project';project_directory.mkdir()
project_id=api('sessions.create',dict(name='Link handler project',directory=str(project_directory)),write=True)['project']
project_job=open_link(value='project-issue:123',handler='project',project=project_id)
finished(project_job['id'],'succeeded')
assert json.loads(job(project_job['id'])['output']['stdout'])['context']['project']['id']==project_id
project_file=tmp/'data/starflux/projects'/(project_id+'.json')
project_config=json.loads(project_file.read_text());project_config['host']='must-not-contact.invalid'
project_config['project_host']=True;project_file.write_text(json.dumps(project_config))
(runtime/'starflux'/(project_id+'.json')).write_text(json.dumps(project_config))
open_link(value='project-issue:123',handler='project',project=project_id,ok=False)
# Multiple matching packages require an explicit choice; a broken source must
# not suppress an independent matching plugin or execute it during discovery.
other=tmp/'second-link-plugin';shutil.copytree(package,other)
second=copy.deepcopy(manifest);second['id']='second';second['name']='Second handler'
second['link_handlers']=second['link_handlers'][:1]
(other/'starflux-plugin.json').write_text(json.dumps(second))
c=catalogue();c=api('plugins.link',dict(path=str(other),expected_revision=c['revision']),write=True)
p=next(p for p in c['plugins'] if p['id']=='second')
api('plugins.configure',dict(plugin='second',enabled=True,expected_digest=p['digest'],expected_revision=c['revision']),write=True)
assert len(resolve()['handlers'])==2
page=api('plugins.resolve_link',dict(url=url,limit=1))
assert page['total']==2 and len(page['handlers'])==1 and page['next_offset']==1
tail=api('plugins.resolve_link',dict(url=url,offset=page['next_offset'],limit=1))
assert tail['total']==2 and tail['next_offset'] is None and tail['handlers'][0]['plugin']!=page['handlers'][0]['plugin']
assert not api('plugins.resolve_link',dict(url=url,offset=100,limit=1))['handlers']
api('plugins.resolve_link',dict(url=url,limit=0),ok=False)
before=len(calls());source.write_text('{broken')
r=resolve();assert [h['plugin'] for h in r['handlers']]==['second'] and len(r['warnings'])==1
assert len(calls())==before
open_link(ok=False)
source.write_text(json.dumps(manifest))
c=catalogue();api('plugins.unlink',dict(plugin='second',expected_revision=c['revision']),write=True)
open_link(plugin='second',expected=p['digest'],ok=False)
# A handler chosen earlier cannot bypass disablement while queued.
stopped={**env,'STARFLUX_PLUGIN_SERVICE':'off'}
configure(False)
open_link(ok=False)
configure(True,scope=stopped)
service=api('plugins.service')['service'];service_pid=service['pid']
assert stamp(service_pid)==service['start_ticks']
service_args=Path('/proc',str(service_pid),'cmdline').read_bytes().split(b'\0')
assert b'--plugin-service' in service_args and str(data).encode() in service_args
os.kill(service_pid,signal.SIGSTOP)
try:
    queued=open_link(scope=stopped)
    configure(False,scope=stopped)
finally:
    os.kill(service_pid,signal.SIGCONT)
api('plugins.service');finished(queued['id'],'rejected')
assert not any(c['context']['job_id']==queued['id'] for c in calls())
# Invalid declarations never become linked, including unknown actions/hosts.
for change in [dict(action='missing'),dict(schemes=[]),dict(hosts=['*.example.invalid']),dict(title='bad\x1btitle')]:
    bad=copy.deepcopy(second);bad['id']='invalid';bad['link_handlers'][0].update(change)
    (other/'starflux-plugin.json').write_text(json.dumps(bad))
    c=catalogue();api('plugins.link',dict(path=str(other),expected_revision=c['revision']),write=True,ok=False)
# A version-one service may still be alive when a new binary adds manifest
# fields. Simulate its documented lock/request protocol in a second namespace.
# A replacement must wait for cooperative lock release and drain the queue,
# without stopping the independent job already running in that namespace.
upgrade={**env,'XDG_DATA_HOME':str(tmp/'upgrade-data'),'XDG_CONFIG_HOME':str(tmp/'upgrade-config'),'STARFLUX_PLUGIN_SERVICE':'off'}
upgrade_data=tmp/'upgrade-data/starflux/plugins';legacy=None;worker=None
def upgraded_job(identity):return api('plugins.job',dict(job_id=identity),scope=upgrade)
try:
    c=api('plugins.link',dict(path=str(package),expected_revision='initial'),write=True,scope=upgrade)
    d=c['plugins'][0]['digest']
    api('plugins.configure',dict(plugin='fixture',enabled=True,expected_digest=d,expected_revision=c['revision']),write=True,scope=upgrade)
    slow=api('plugins.run',dict(plugin='fixture',action='slow',expected_digest=d),write=True,scope=upgrade)
    # Retained version-one receipts remain readable by the replacement.
    old_receipt=upgrade_data/('job-'+slow['id']+'.json')
    legacy_job=json.loads(old_receipt.read_text());legacy_job['version']=1
    old_receipt.write_text(json.dumps(legacy_job))
    worker=subprocess.Popen([str(app),'--plugin-job',str(upgrade_data),slow['id']],env=upgrade,
        start_new_session=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    running=wait(lambda:(j if (j:=upgraded_job(slow['id']))['status']=='running' and (package/('live-'+slow['id'])).exists() else None),'upgrade fixture worker did not start')
    retained_pids=json.loads((package/('live-'+slow['id'])).read_text())
    retained_stamps={pid:stamp(pid) for pid in retained_pids}
    legacy_code='''import fcntl,json,os,sys,time
from pathlib import Path
data=Path(sys.argv[-1]);lock=(data/'service.lock').open('a');fcntl.flock(lock,fcntl.LOCK_EX)
ticks=int(Path('/proc',str(os.getpid()),'stat').read_text().rsplit(')',1)[1].split()[19])
(data/'service.json').write_text(json.dumps(dict(version=3,pid=os.getpid(),start_ticks=ticks,boot=Path('/proc/sys/kernel/random/boot_id').read_text().strip(),instance='legacy-fixture',scans=0,errors=0,warning=None)))
(data/'legacy-ready').touch()
while True:
 for path in data.glob('job-*.json'):
  try:
   job=json.loads(path.read_text())
   if job['version']<=3 and job['status']=='queued':(data/'legacy-would-claim').touch()
  except (OSError,ValueError):pass
 try:
  if json.loads((data/'service-request.json').read_text())['version']>3:break
 except (OSError,ValueError):pass
 time.sleep(.02)
time.sleep(.25)
'''
    legacy=subprocess.Popen(['python3','-c',legacy_code,'--plugin-service',str(upgrade_data)],env=upgrade,
        start_new_session=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    wait(lambda:(upgrade_data/'legacy-ready').exists(),'legacy service fixture did not lock namespace')
    pending=api('plugins.open_link',dict(plugin='fixture',handler='issues',url=url,expected_digest=d),write=True,scope=upgrade)
    assert pending['version']>=4,'new handler receipt could be claimed by an incompatible service'
    live_scope={**upgrade,'STARFLUX_PLUGIN_SERVICE':'on'}
    api('plugins.service',scope=live_scope)
    wait(lambda:legacy.poll()==0,'legacy service did not retire cooperatively')
    wait(lambda:upgraded_job(pending['id'])['status']=='succeeded','replacement service abandoned queued link handler')
    current=json.loads((upgrade_data/'service.json').read_text());assert current['version']>=4 and current['pid']!=legacy.pid
    assert not (upgrade_data/'legacy-would-claim').exists(),'legacy service could claim the new handler before handoff'
    assert all(stamp(pid)==ticks for pid,ticks in retained_stamps.items()),'service handoff stopped a running job'
    api('plugins.cancel',dict(job_id=slow['id']),write=True,scope=upgrade)
    wait(lambda:upgraded_job(slow['id'])['status']=='cancelled','upgraded worker cancellation failed')
    wait(lambda:all(stamp(pid) is None for pid in retained_pids),'upgraded cancellation left children running')
finally:
    if legacy and legacy.poll() is None:legacy.kill();legacy.wait(timeout=5)
    for process in Path('/proc').iterdir():
        if not process.name.isdigit():continue
        try:
            argv=(process/'cmdline').read_bytes().split(b'\0')
            if str(upgrade_data).encode() in argv and any(role in argv for role in (b'--plugin-service',b'--plugin-job')):
                pid=int(process.name)
                if os.getpgid(pid)==pid:os.killpg(pid,signal.SIGKILL)
        except (OSError,ValueError):pass
    if worker:worker.wait(timeout=5)
print(json.dumps(dict(result='PASS',ssh=args.ssh,link_resolution_read_only=True,explicit_handler_choice=True,
    exact_host_matching=True,literal_url_context=True,no_terminal_typing=True,credentials_rejected=True,
    multiple_plugins=True,corrupt_source_isolation=True,approval_and_activation_guard=True,idempotency=True,
    service_upgrade_drains_queue=True,running_job_preserved_across_handoff=True)))
