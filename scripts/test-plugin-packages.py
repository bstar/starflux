"""Managed package lifecycle; invoked by test-plugins.py --packages [--ssh]."""
import getpass,json,os,shlex,subprocess,time,uuid
from pathlib import Path
repo=tmp/'Git source with spaces 界';repo.mkdir()
package=repo/'plugins/workflow';package.mkdir(parents=True)
calls=tmp/'managed-calls';gate=tmp/'finish-managed-job';pids=tmp/'managed-pids'
env.update(GIT_CONFIG_GLOBAL='/dev/null',GIT_CONFIG_SYSTEM='/dev/null')
def git(*args):
    return subprocess.check_output(['git','-c','core.hooksPath=/dev/null','-C',str(repo),*args],env=env,stderr=subprocess.PIPE,text=True).strip()
git('init','-q','-b','main');git('config','user.name','Managed fixture');git('config','user.email','fixture@example.invalid')
manifest=dict(version=1,id='managed-fixture',name='Managed fixture 界',actions=[dict(id='echo',title='Echo installed version',argv=['package:runner','echo']),dict(id='slow',title='Read old version after update',argv=['package:runner','slow'],timeout_ms=30000)])
(package/'starflux-plugin.json').write_text(json.dumps(manifest))
def code(version):
    return '''#!/usr/bin/python3
import json,os,subprocess,sys,time
from pathlib import Path
context=json.load(sys.stdin)
with Path(CALLS).open('a') as f:f.write(context['job_id']+'\\n')
if sys.argv[1]=='slow':
 child=subprocess.Popen(['/bin/sleep','60'])
 Path(PIDS).write_text(json.dumps([os.getpid(),child.pid]))
 while not Path(GATE).exists():time.sleep(.05)
 child.terminate();child.wait()
print(Path(__file__).with_name('version').read_text().strip(),flush=True)
'''.replace('CALLS',repr(str(calls))).replace('PIDS',repr(str(pids))).replace('GATE',repr(str(gate)))
(package/'runner').write_text(code('v1'));(package/'runner').chmod(0o700)
(package/'version').write_text('MANAGED_V1')
(package/'install.sh').write_text('#!/bin/sh\ntouch '+shlex.quote(str(tmp/'MUST_NOT_BUILD'))+'\n');(package/'install.sh').chmod(0o700)
(package/'runner-link').symlink_to('runner')
git('add','.');git('commit','-qm','version one');git('tag','v1');v1=git('rev-parse','HEAD')
repo_url=str(repo)
# The SSH case also fetches Git itself through the authenticated private server.
# Git URLs encode the spaces in the remote path.
if args.ssh:
    from urllib.parse import quote
    repo_url='ssh://'+getpass.getuser()+'@127.0.0.1:'+str(port)+quote(str(repo),safe='/')
def listed():return api('plugins.list')
def current():return next((p for p in listed()['plugins'] if p['id']=='managed-fixture'),None)
def install(reference='v1',operation=None,expected=None,ok=True):
    return api('plugins.install',dict(repository=repo_url,reference=reference,subdirectory='plugins/workflow',expected_revision=expected or listed()['revision']),write=True,operation=operation,ok=ok)
def enable():
    c=listed();p=current();api('plugins.configure',dict(plugin=p['id'],enabled=True,expected_digest=p['digest'],expected_revision=c['revision']),write=True)
def run(action='echo',scope=None):
    p=current();return api('plugins.run',dict(plugin=p['id'],action=action,expected_digest=p['digest']),write=True,scope=scope)
def job(identity):return api('plugins.job',dict(job_id=identity))
def done(identity,status='succeeded'):
    def result():
        j=job(identity)
        if j['status'] in ('failed','rejected','indeterminate') and j['status']!=status:raise AssertionError(j)
        return j if j['status']==status else None
    return wait(result,'managed job did not finish: '+status)
def update(reference,ok=True):return api('plugins.update',dict(plugin='managed-fixture',reference=reference,expected_revision=listed()['revision']),write=True,ok=ok)
operation=str(uuid.uuid4());initial_revision=listed()['revision'];first=install(operation=operation,expected=initial_revision)
p=current();assert not p['enabled'] and p['managed']['commit']==v1 and not calls.exists()
assert install(operation=operation,expected=initial_revision)==first
assert len(api('plugins.versions',dict(plugin='managed-fixture'))['versions'])==1
old_path=Path(p['path']);assert old_path.is_relative_to(tmp/'data/starflux/plugin-packages'),'managed package escaped private fixture data'
old_release=p['managed']['release'];old_digest=p['digest']
assert old_path.is_dir() and not (old_path/'.git').exists() and (old_path/'runner-link').is_symlink()
assert (old_path/'runner-link').resolve()==old_path/'runner'
assert not (tmp/'MUST_NOT_BUILD').exists()
enable();assert done(run()['id'])['output']['stdout'].strip()=='MANAGED_V1'
# An installed snapshot does not follow uncommitted or committed source changes.
(package/'version').write_text('MANAGED_V2')
assert done(run()['id'])['output']['stdout'].strip()=='MANAGED_V1'
git('add','.');git('commit','-qm','version two');git('tag','v2');v2=git('rev-parse','HEAD')
slow=run('slow');wait(lambda:job(slow['id'])['status']=='running' and pids.exists(),'old-version job did not run')
protected=json.loads(pids.read_text());update('v2');p=current()
assert not p['enabled'] and not p['approved'] and p['managed']['commit']==v2 and p['digest']!=old_digest
assert old_path.exists() and all(stamp(pid) is not None for pid in protected),'update removed an in-use version'
api('plugins.uninstall',dict(plugin='managed-fixture',expected_revision=listed()['revision']),write=True,ok=False)
assert current() is not None and old_path.exists(),'busy uninstall removed plugin files'
gate.touch();assert done(slow['id'])['output']['stdout'].strip()=='MANAGED_V1'
wait(lambda:all(stamp(pid) is None for pid in protected),'old-version job leaked children')
enable();assert done(run()['id'])['output']['stdout'].strip()=='MANAGED_V2'
versions=api('plugins.versions',dict(plugin='managed-fixture'))
assert len(versions['versions'])==2 and sum(v['current'] for v in versions['versions'])==1
# Retained versions roll back offline and always require explicit reapproval.
backup=repo.with_name('repository temporarily offline');repo.rename(backup)
api('plugins.activate',dict(plugin='managed-fixture',release=old_release,expected_revision=listed()['revision']),write=True)
p=current();assert not p['enabled'] and p['path']==str(old_path) and p['managed']['commit']==v1
backup.rename(repo);enable();assert done(run()['id'])['output']['stdout'].strip()=='MANAGED_V1'
unchanged=current();update('missing-ref',ok=False);assert current()['path']==unchanged['path'] and current()['enabled']
# Manifest identity changes and unsafe symlinks must not replace the active version.
manifest['id']='wrong-identity';(package/'starflux-plugin.json').write_text(json.dumps(manifest));git('add','.');git('commit','-qm','wrong identity');git('tag','wrong')
update('wrong',ok=False);assert current()['path']==str(old_path)
manifest['id']='managed-fixture';(package/'starflux-plugin.json').write_text(json.dumps(manifest))
outside=tmp/'DO_NOT_CHANGE';outside.write_text('preserved');(package/'escape').symlink_to(outside)
git('add','.');git('commit','-qm','unsafe link');git('tag','unsafe')
update('unsafe',ok=False);assert current()['path']==str(old_path) and outside.read_text()=='preserved'
(package/'escape').unlink();git('add','.');git('commit','-qm','clean again');git('tag','clean')
# Managed snapshots pin scripts/assets too, not just unchanged manifest bytes.
status=api('plugins.service');service=status.get('service')
if service:
    (data/'service-request.json').write_text(json.dumps(dict(version=service['version']+1)))
    wait(lambda:stamp(service['pid'])!=service['start_ticks'],'service did not stop for queued integrity test')
stopped={**env,'STARFLUX_PLUGIN_SERVICE':'off'}
p=current();queued=api('plugins.run',dict(plugin='managed-fixture',action='echo',expected_digest=p['digest']),write=True,scope=stopped)
previous_calls=calls.read_text()
(old_path/'version').write_text('TAMPERED')
assert not current()['enabled'],'modified managed assets stayed enabled'
api('plugins.configure',dict(plugin='managed-fixture',enabled=True,expected_digest=p['digest'],expected_revision=listed()['revision']),write=True,ok=False)
api('plugins.service');done(queued['id'],'rejected');assert calls.read_text()==previous_calls,'modified package code ran'
update('clean');enable();assert done(run()['id'])['output']['stdout'].strip()=='MANAGED_V2'
# Invalid/stale requests cannot replace the registry or contact arbitrary Git transports.
revision=listed()['revision']
api('plugins.install',dict(repository='ext::touch NEVER',reference='HEAD',subdirectory='',expected_revision=revision),write=True,ok=False)
api('plugins.install',dict(repository='https://user:password@github.com/example/repo.git',reference='HEAD',subdirectory='',expected_revision=revision),write=True,ok=False)
api('plugins.install',dict(repository=repo_url,reference='clean',subdirectory='../escape',expected_revision=revision),write=True,ok=False)
api('plugins.update',dict(plugin='managed-fixture',reference='v1',expected_revision='stale'),write=True,ok=False)
assert listed()['revision']==revision and outside.read_text()=='preserved'
# Uninstall removes only owned versions. Job logs and external source remain.
all_paths=[Path(v['source']['release']) for v in api('plugins.versions',dict(plugin='managed-fixture'))['versions']]
root=old_path.parent.parent
api('plugins.uninstall',dict(plugin='managed-fixture',expected_revision=revision),write=True)
assert current() is None and not root.exists() and (package/'runner').exists() and calls.exists() and job(slow['id'])['status']=='succeeded'
assert outside.read_text()=='preserved' and not (tmp/'MUST_NOT_BUILD').exists()
# Parent death stops the Git wrapper and its child; a later operation cleans
# only abandoned quarantine directories, preserving a verified live owner.
import shutil,signal
package_root=root.parent
wrapper_dir=tmp/'git-wrapper';wrapper_dir.mkdir();real_git=shutil.which('git');control=tmp/'fetch-control'
wrapper=wrapper_dir/'git';wrapper.write_text('#!/usr/bin/python3\nimport json,os,subprocess,sys\nfrom pathlib import Path\nif "fetch" in sys.argv[1:]:\n child=subprocess.Popen(["/bin/sleep","60"])\n Path('+repr(str(control))+').write_text(json.dumps(dict(pid=os.getpid(),child=child.pid,group=os.getpgrp())))\n child.wait()\nos.execv('+repr(real_git)+',['+repr(real_git)+',*sys.argv[1:]])\n');wrapper.chmod(0o700)
fetch_scope={**env,'PATH':str(wrapper_dir)+os.pathsep+env['PATH']}
def begin_fetch():
    control.unlink(missing_ok=True)
    operation=str(uuid.uuid4());parameters=dict(repository=repo_url,reference='clean',subdirectory='plugins/workflow',expected_revision=listed()['revision'])
    request=dict(version=1,id='interrupted-fetch',method='plugins.install',params=parameters,operation_id=operation,deadline_ms=90000)
    command=[str(app),'--api',json.dumps(request)]
    if args.ssh:command=ssh+[shlex.join(['env',*[k+'='+fetch_scope[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','PATH','STARFLUX_SNAPSHOT_SERVICE','STARFLUX_PLUGIN_SERVICE','STARFLUX_HERDR','GIT_SSH_COMMAND','GIT_CONFIG_GLOBAL','GIT_CONFIG_SYSTEM') if k in fetch_scope],*command])]
    previous=set(package_root.glob('.stage-*'))
    proc=subprocess.Popen(command,env=fetch_scope,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    record=wait(lambda:json.loads(control.read_text()) if control.exists() else None,'Git wrapper did not start')
    stage=next(p for p in package_root.glob('.stage-*') if p not in previous and (p/'owner.json').exists())
    owner=json.loads((stage/'owner.json').read_text())
    assert stamp(owner['pid'])==owner['start_ticks'] and b'--automation-worker' in Path('/proc',str(owner['pid']),'cmdline').read_bytes().split(b'\0')
    assert b'--read-query-worker' in Path('/proc',str(record['group']),'cmdline').read_bytes().split(b'\0')
    return proc,record,stage,owner,operation,parameters
proc,record,stage,owner,operation,parameters=begin_fetch()
second=begin_fetch()
full=install('clean',ok=False);assert full['code']=='package_busy',full
assert os.getpgid(owner['pid'])==owner['pid'];os.killpg(owner['pid'],signal.SIGKILL)
stdout,stderr=proc.communicate(timeout=15);assert not json.loads(stdout)['ok']
wait(lambda:all(stamp(pid) is None for pid in record.values()),'cancelled fetch left a Git/SSH helper running')
api('plugins.install',parameters,write=True,operation=operation,ok=False)
assert current() is None,'interrupted fetch registered a plugin'
os.utime(stage,(time.time()-30,time.time()-30))
live=package_root/'.stage-live-fixture';live.mkdir(mode=0o700);(live/'keep').write_text('live owner')
(live/'owner.json').write_text(json.dumps(dict(version=1,pid=os.getpid(),start_ticks=stamp(os.getpid()),boot=Path('/proc/sys/kernel/random/boot_id').read_text().strip())))
os.utime(live,(time.time()-30,time.time()-30))
retained=package_root/'.stage-retained-fixture';(retained/('release-'+str(uuid.uuid4()))/'package').mkdir(parents=True,mode=0o700)
(retained/'keep').write_text('retained version namespace');os.utime(retained,(time.time()-7200,time.time()-7200))
unknown=package_root/'.stage-unowned-fixture';unknown.mkdir(mode=0o700);(unknown/'keep').write_text('unowned source');os.utime(unknown,(time.time()-7200,time.time()-7200))
install('clean');assert not stage.exists() and (live/'keep').read_text()=='live owner'
assert (retained/'keep').read_text()=='retained version namespace' and (unknown/'keep').read_text()=='unowned source'
api('plugins.uninstall',dict(plugin='managed-fixture',expected_revision=listed()['revision']),write=True)
shutil.rmtree(live)
# Removing an in-flight package namespace must not recreate it or commit a
# partially installed registry entry. The old directory incarnation is revoked.
proc,record,stage,owner,operation,parameters=second
shutil.rmtree(package_root)
stdout,stderr=proc.communicate(timeout=15);assert not json.loads(stdout)['ok']
wait(lambda:all(stamp(pid) is None for pid in record.values()),'namespace removal left fetch helpers running')
assert not package_root.exists() and current() is None,'fetch resurrected its removed namespace'
print(json.dumps(dict(result='PASS',ssh=args.ssh,git_over_ssh=args.ssh,subdirectory=True,pinned_commit=True,initially_disabled=True,idempotent_install=True,no_package_execution=True,internal_symlinks=True,update_reapproval=True,in_use_version_retained=True,busy_uninstall_guard=True,offline_rollback=True,failed_update_retains_version=True,escape_rejected=True,script_and_asset_integrity=True,stale_revision=True,owned_uninstall=True,logs_retained=True,fetch_limit=2,parent_death_cleanup=True,orphan_cleanup=True,live_staging_preserved=True,retained_and_unowned_paths_preserved=True,no_namespace_resurrection=True)))
