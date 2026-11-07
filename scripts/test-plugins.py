#!/usr/bin/env python3
"""Private plugin commands, hooks and worker failure tests, optionally over SSH."""
import argparse,concurrent.futures,getpass,json,os,shlex,shutil,signal,socket,subprocess,tempfile,time,uuid
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--ssh',action='store_true');parser.add_argument('--capacity',action='store_true');parser.add_argument('--packages',action='store_true');parser.add_argument('--links',action='store_true');parser.add_argument('--panes',action='store_true');args=parser.parse_args()
app=Path(__file__).resolve().parents[1]/'scripts/run-local.sh'
with tempfile.TemporaryDirectory(prefix='sf-plugins-') as temporary:
    tmp=Path(temporary);runtime=tmp/'runtime';runtime.mkdir(mode=0o700);(tmp/'tmux').mkdir(mode=0o700)
    env={k:v for k,v in os.environ.items() if not k.startswith(('HERDR_','BASH_FUNC_','STARFLUX_')) and k not in ('TMUX','SSH_CONNECTION','SSH_TTY')}
    env.update(TMUX_TMPDIR=str(tmp/'tmux'),XDG_RUNTIME_DIR=str(runtime),XDG_DATA_HOME=str(tmp/'data'),XDG_CONFIG_HOME=str(tmp/'config'),STARFLUX_SNAPSHOT_SERVICE='off',STARFLUX_HERDR=str(tmp/'no-herdr'))
    data=tmp/'data/starflux/plugins';package=tmp/'package with spaces 界';package.mkdir();daemon=None;ssh=[]
    def wait(predicate,label,timeout=20):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            value=predicate()
            if value:return value
            time.sleep(.05)
        raise AssertionError(label)
    def stamp(pid):
        try:
            parts=Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()
            return None if parts[0]=='Z' else int(parts[19])
        except OSError:return None
    def api(method,params=None,write=False,operation=None,ok=True,scope=None):
        scope=scope or env;request=dict(version=1,id='plugin-proof',method=method,params=params or {})
        if method in ('plugins.install','plugins.update'):request['deadline_ms']=90000
        if write:request['operation_id']=operation or str(uuid.uuid4())
        command=[str(app),'--api',json.dumps(request)]
        if args.ssh:command=ssh+[shlex.join(['env',*[key+'='+value for key,value in scope.items() if key in ('SHELL','TMUX_TMPDIR','XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','PATH','STARFLUX_SNAPSHOT_SERVICE','STARFLUX_PLUGIN_SERVICE','STARFLUX_HERDR','GIT_SSH_COMMAND','GIT_CONFIG_GLOBAL','GIT_CONFIG_SYSTEM')],*command])]
        response=json.loads(subprocess.check_output(command,env=scope,timeout=100));assert response['ok']==ok,response
        return response['result'] if ok else response['error']
    def catalogue(scope=None):return api('plugins.list',scope=scope)
    def configure(enabled,scope=None):
        c=catalogue(scope);p=c['plugins'][0]
        return api('plugins.configure',dict(plugin='fixture',enabled=enabled,expected_digest=p.get('digest',''),expected_revision=c['revision']),write=True,scope=scope)
    def run(action,scope=None,operation=None,ok=True):
        c=catalogue(scope);return api('plugins.run',dict(plugin='fixture',action=action,expected_digest=c['plugins'][0]['digest']),write=True,operation=operation,scope=scope,ok=ok)
    def job(identity):return api('plugins.job',dict(job_id=identity))
    def finished(identity,status):return wait(lambda:(j if (j:=job(identity))['status']==status else None),'job did not reach '+status)
    def calls():return [json.loads(line) for line in (package/'calls').read_text().splitlines()] if (package/'calls').exists() else []
    try:
        if args.ssh:
            for key in ['host','client']:subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(tmp/key)],check=True)
            (tmp/'authorized_keys').write_text((tmp/'client.pub').read_text())
            with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
            (tmp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(tmp/'host.pub').read_text())
            (tmp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
            daemon=subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd','-D','-e','-f',str(tmp/'sshd_config')],stderr=(tmp/'sshd.log').open('w'))
            def listening():
                try:
                    with socket.create_connection(('127.0.0.1',port),timeout=.1):return True
                except OSError:return False
            wait(listening,'private sshd unavailable')
            ssh=['ssh','-T','-p',str(port),'-i',str(tmp/'client'),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(tmp/'known_hosts'),getpass.getuser()+'@127.0.0.1']
        if args.panes:
            import runpy
            runpy.run_path(str(app.parents[1]/'scripts/test-plugin-panes.py'),init_globals=dict(api=api,tmp=tmp,env=env,args=args,wait=wait,stamp=stamp,data=data,app=app,package=package))
            raise SystemExit(0)
        if args.packages:
            if args.ssh:env['GIT_SSH_COMMAND']=shlex.join(['ssh','-p',str(port),'-i',str(tmp/'client'),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(tmp/'known_hosts')])
            import runpy
            runpy.run_path(str(app.parents[1]/'scripts/test-plugin-packages.py'),init_globals=dict(api=api,tmp=tmp,env=env,args=args,wait=wait,stamp=stamp,data=data,port=port if args.ssh else None,app=app,ssh=ssh))
            raise SystemExit(0)
        script=package/'runner';script.write_text('''#!/usr/bin/python3
import json,os,subprocess,sys,time
from pathlib import Path
c=json.load(sys.stdin)
assert json.loads(os.environ['STARFLUX_PLUGIN_CONTEXT_JSON'])==c
assert Path(os.environ['STARFLUX_PLUGIN_CONFIG_DIR']).is_dir()
assert Path(os.environ['STARFLUX_PLUGIN_STATE_DIR']).is_dir()
assert Path(os.environ['STARFLUX_BIN_PATH']).exists()
with Path('''+repr(str(package/'calls'))+''').open('a') as f:f.write(json.dumps(dict(context=c,argv=sys.argv[1:]))+'\\n')
mode=sys.argv[1]
if mode=='gate':
 while not Path('CAPACITY_GATE').exists():time.sleep(.05)
if mode=='fail':print('failure details',file=sys.stderr);sys.exit(7)
if mode=='overflow':os.write(1,b'x'*(1024*1024));time.sleep(5)
if mode=='escaped':os.write(1,b'\\0'*(512*1024))
elif mode=='escaped-both':os.write(1,b'\\0'*(512*1024));os.write(2,b'\\0'*(512*1024))
elif mode=='slow':
 child=subprocess.Popen(['/bin/sleep','30'])
 Path('''+repr(str(package))+'''+'/live-'+c['job_id']).write_text(json.dumps([os.getpid(),child.pid]))
 time.sleep(30)
else:print(json.dumps(dict(context=c,argv=sys.argv[1:]),ensure_ascii=False))
''');script.chmod(0o700)
        actions=[dict(id=a,title=a,argv=['package:runner',mode,'literal space 界 $(touch NEVER)'],timeout_ms=timeout) for a,mode,timeout in [('echo','echo',30000),('fail','fail',30000),('overflow','overflow',30000),('escaped','escaped',30000),('escaped-both','escaped-both',30000),('timeout','slow',300),('slow','slow',30000),('hook','hook',30000)]]
        manifest=dict(version=1,id='fixture',name='Fixture workflow',actions=actions)
        source=package/'starflux-plugin.json';source.write_text(json.dumps(manifest))
        if args.links:
            import runpy
            runpy.run_path(str(app.parents[1]/'scripts/test-plugin-links.py'),init_globals=globals())
            raise SystemExit(0)
        if args.capacity:
            manifest['actions'].append(dict(id='gate',title='Gate command',argv=['package:runner','gate'],timeout_ms=30000))
            manifest['hooks']=[dict(id='startup',on='startup',action='hook')]
            source.write_text(json.dumps(manifest))
            stopped={**env,'STARFLUX_PLUGIN_SERVICE':'off'}
            c=api('plugins.link',dict(path=str(package),expected_revision='initial'),write=True,scope=stopped)
            configure(True,scope=stopped)
            with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
                queued=list(executor.map(lambda _:run('gate',scope=stopped),range(32)))
            identities={j['id'] for j in queued};assert len(identities)==32
            rejected=run('gate',scope=stopped,ok=False)
            assert 'queue is full' in rejected['message'],rejected
            assert len(api('plugins.jobs',scope=stopped)['jobs'])==32
            api('plugins.service')
            wait(lambda:len([c for c in calls() if c['context']['job_id'] in identities])==2,'two workers did not start when hook queue was full')
            # Keep both actions blocked: another scan must not exceed the limit.
            time.sleep(1.2)
            assert len([c for c in calls() if c['context']['job_id'] in identities])==2,'plugin worker capacity was exceeded'
            (package/'CAPACITY_GATE').touch()
            wait(lambda:all(j['status']=='succeeded' for j in api('plugins.jobs')['jobs'] if j['id'] in identities),'queued plugin jobs did not drain under hook backpressure',timeout=30)
            wait(lambda:len([c for c in calls() if (c['context']['event'] or {}).get('kind')=='startup'])==1,'startup hook was lost or duplicated under backpressure')
            assert all(sum(c['context']['job_id']==identity for c in calls())==1 for identity in identities),'capacity recovery duplicated commands'
            configure(False)
            print(json.dumps(dict(result='PASS',ssh=args.ssh,concurrent_publishers=True,queue_limit=32,worker_limit=2,hook_backpressure_drains=True,all_jobs_once=True)))
            raise SystemExit(0)
        assert catalogue()==dict(revision='initial',plugins=[],execution_host='workspace_controller')
        assert not list((tmp/'data/starflux/projects').glob('*.json'))
        c=api('plugins.link',dict(path=str(package),expected_revision='initial'),write=True);assert not c['plugins'][0]['enabled']
        run('echo',ok=False);configure(True)
        operation=str(uuid.uuid4());queued=run('echo',operation=operation);result=finished(queued['id'],'succeeded')
        output=json.loads(result['output']['stdout']);assert output['argv'][1]=='literal space 界 $(touch NEVER)';assert output['context']['project'] is None
        assert run('echo',operation=operation)==queued
        assert len([c for c in calls() if c['context']['job_id']==queued['id']])==1
        result=finished(run('fail')['id'],'failed');assert result['exit_code']==7 and 'failure details' in result['output']['stderr']
        result=finished(run('timeout')['id'],'timed_out');pids=json.loads((package/('live-'+result['id'])).read_text());wait(lambda:all(stamp(p) is None for p in pids),'timeout left plugin children alive')
        finished(run('overflow')['id'],'output_limit')
        result=finished(run('escaped')['id'],'succeeded');assert result['output']['stdout']=='\0'*(512*1024)
        result=finished(run('escaped-both')['id'],'succeeded');assert all(result['output'][s]=='\0'*(512*1024) for s in ('stdout','stderr'))
        result=run('slow');wait(lambda:job(result['id'])['status']=='running','slow action did not start')
        pids=wait(lambda:json.loads((package/('live-'+result['id'])).read_text()) if (package/('live-'+result['id'])).exists() else None,'slow action did not create child')
        api('plugins.cancel',dict(job_id=result['id']),write=True);finished(result['id'],'cancelled');wait(lambda:all(stamp(p) is None for p in pids),'cancel left plugin children alive')
        # Queue with no service; an activation change invalidates every old job.
        status=api('plugins.service');old_service=status['service'];(data/'service-request.json').write_text(json.dumps(dict(version=old_service['version']+1)))
        wait(lambda:stamp(old_service['pid'])!=old_service['start_ticks'],'service did not retire')
        stopped={**env,'STARFLUX_PLUGIN_SERVICE':'off'}
        queued=run('echo',scope=stopped);configure(False,scope=stopped);configure(True,scope=stopped)
        api('plugins.service');finished(queued['id'],'rejected')
        source.write_text(source.read_text()+'\n');assert not catalogue()['plugins'][0]['enabled'];run('echo',ok=False);configure(True)
        api('plugins.unlink',dict(plugin='fixture',expected_revision='stale'),write=True,ok=False)
        # Hooks start at enable-time and work with every UI and API process gone.
        manifest['hooks']=[dict(id='boot',on='startup',action='hook'),dict(id='created',on='project.created',action='hook')]
        source.write_text(json.dumps(manifest));configure(True)
        wait(lambda:any(c['context']['event'] and c['context']['event']['kind']=='startup' for c in calls()),'startup hook did not run')
        before=len(calls());time.sleep(1.2);assert len(calls())==before,'startup hook duplicated each scan'
        project=api('sessions.create',dict(name='Plugin event fixture',directory=str(package)),write=True)['project']
        wait(lambda:any(c['context']['event'] and c['context']['event'].get('resource')=='projects/'+project for c in calls()),'headless project hook did not run')
        configure(False);before=len(calls());api('sessions.create',dict(name='Disabled hook fixture',directory=str(package)),write=True);time.sleep(1.2);assert len(calls())==before,'disabled hook executed'
        # Lost worker outcome is explicit and commands are never rerun.
        configure(True);running=run('slow');record=wait(lambda:(j if (j:=job(running['id']))['status']=='running' else None),'worker not running')
        pids=wait(lambda:json.loads((package/('live-'+record['id'])).read_text()) if (package/('live-'+record['id'])).exists() else None,'worker child absent')
        assert stamp(record['pid'])==record['start_ticks'];assert b'--plugin-job' in Path('/proc',str(record['pid']),'cmdline').read_bytes().split(b'\0')
        os.killpg(record['pid'],signal.SIGKILL);finished(record['id'],'indeterminate');time.sleep(.7);assert sum(c['context']['job_id']==record['id'] for c in calls())==1
        configure(False);assert all(stamp(p) is None for p in pids)
        status=api('plugins.service');pid=status['service']['pid'];ticks=status['service']['start_ticks'];shutil.rmtree(data)
        wait(lambda:stamp(pid)!=ticks,'removed namespace left service alive');time.sleep(.6);assert not data.exists(),'service resurrected deleted namespace'
        print(json.dumps(dict(result='PASS',ssh=args.ssh,literal_argv=True,unicode=True,manifest_approval=True,revision_guard=True,headless_jobs=True,idempotency=True,exit_logs=True,timeout_cleanup=True,cancel_cleanup=True,output_bounds=True,both_escaped_streams=True,activation_guard=True,startup_once=True,event_hooks=True,worker_loss_indeterminate=True,no_namespace_resurrection=True)))
    finally:
        # Only this fixture's role and data path authorize cleanup.
        for process in Path('/proc').iterdir():
            if not process.name.isdigit():continue
            try:
                argv=(process/'cmdline').read_bytes().split(b'\0')
                if str(data).encode() in argv and any(role in argv for role in (b'--plugin-service',b'--plugin-job')):
                    pid=int(process.name)
                    if os.getpgid(pid)==pid:os.killpg(pid,signal.SIGKILL)
            except (OSError,ValueError):pass
        if daemon:daemon.terminate();daemon.wait(timeout=5)
