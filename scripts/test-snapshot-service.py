#!/usr/bin/env python3
"""Read-only collection with no controller, isolated metadata and cold namespaces."""
import argparse,base64,concurrent.futures,fcntl,getpass,hashlib,json,os,shlex,shutil,socket,subprocess,tempfile,time,uuid
from pathlib import Path

parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--ssh',action='store_true');parser.add_argument('--upgrade',action='store_true');parser.add_argument('--previous-executable',type=Path);args=parser.parse_args()
root=Path(__file__).resolve().parents[1];app=root/'scripts/run-local.sh'
with tempfile.TemporaryDirectory(prefix='sf-snap-service-') as directory:
    tmp=Path(directory);runtime=tmp/'runtime';runtime.mkdir(mode=0o700);(tmp/'tmux').mkdir(mode=0o700)
    shell=tmp/'fake-shell';shell.write_text("#!/usr/bin/python3\nimport os,sys\nprint('SNAPSHOT_READY',flush=True)\nfor line in sys.stdin:print('\\x1b[1;31mSAVED_'+line.strip()+'\\x1b[0m',flush=True)\n");shell.chmod(0o700)
    env={k:v for k,v in os.environ.items() if not k.startswith(('HERDR_','BASH_FUNC_')) and k not in ('TMUX','SSH_CONNECTION','SSH_TTY')}
    env.update(XDG_RUNTIME_DIR=str(runtime),XDG_DATA_HOME=str(tmp/'data'),XDG_CONFIG_HOME=str(tmp/'config'),TMUX_TMPDIR=str(tmp/'tmux'),SHELL=str(shell));env.pop('STARFLUX_SNAPSHOT_SERVICE',None)
    env2={**env,'XDG_DATA_HOME':str(tmp/'second-data')};daemon=None;ssh=[];projects=[];collectors=[]
    def stamp(pid):
        try:
            fields=Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()
            return None if fields[0]=='Z' else int(fields[19])
        except (OSError,ValueError):return None
    def wait(predicate,label,timeout=25):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            value=predicate()
            if value:return value
            time.sleep(.05)
        raise AssertionError(label)
    def api(method,params=None,scope=env,write=False):
        request=dict(version=1,id='snapshot-proof',method=method,params=params or {})
        if write:request['operation_id']=str(uuid.uuid4())
        command=[str(app),'--api',json.dumps(request)]
        if args.ssh:
            keys=('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','PATH','STARFLUX_SNAPSHOT_SERVICE')
            command=ssh+[shlex.join(['env',*[key+'='+scope[key] for key in keys if key in scope],*command])]
        result=json.loads(subprocess.check_output(command,env=scope,timeout=20));assert result['ok'],result
        return result['result']
    def namespace(scope):return hashlib.sha256(os.fsencode(str(Path(scope['XDG_DATA_HOME'])/'starflux/projects'))).hexdigest()[:16]
    def status(scope):
        try:return json.loads((runtime/'starflux'/('snapshot-'+namespace(scope)+'.status.json')).read_text())
        except (OSError,ValueError):return {}
    def screen(scope,project):
        try:
            data=json.loads((Path(scope['XDG_DATA_HOME'])/'starflux/screens'/project/'p0.screen.json').read_text());data['formatted']=base64.b64decode(data['formatted_base64']).decode();return data
        except (OSError,ValueError):return {}
    def stop(scope):
        metadata=status(scope);pid=metadata.get('pid');ticks=metadata.get('start_ticks')
        request=runtime/'starflux'/('snapshot-'+namespace(scope)+'.request.json')
        if not pid or stamp(pid)!=ticks:return
        argv=Path('/proc',str(pid),'cmdline').read_bytes().split(b'\0')
        assert b'--snapshot-worker' in argv and str(runtime/'starflux').encode() in argv,'refusing to stop an unrelated process'
        request.write_text(json.dumps(dict(version=1,stop=True)))
        wait(lambda:stamp(pid)!=ticks,'collector did not retire',timeout=20)
    def policy(scope,project,enabled):
        base=Path(scope['XDG_DATA_HOME'])/'starflux/projects'
        with (base/(project+'.model.lock')).open('a') as lock:
            fcntl.flock(lock,fcntl.LOCK_EX);path=base/(project+'.model.json');model=json.loads(path.read_text());model['save_terminal_screens']=enabled;path.write_text(json.dumps(model))
            if not enabled:
                for p in (base.parent/'screens'/project).glob('*.screen.json'):p.unlink()
    try:
        if args.ssh:
            for key in ['host','client']:subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(tmp/key)],check=True)
            (tmp/'authorized_keys').write_text((tmp/'client.pub').read_text())
            with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
            (tmp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(tmp/'host.pub').read_text())
            (tmp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
            daemon=subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd','-D','-e','-f',str(tmp/'sshd_config')],stderr=(tmp/'sshd.log').open('w'))
            def listening():
                try:
                    with socket.create_connection(('127.0.0.1',port),timeout=.1):return True
                except OSError:return False
            wait(listening,'private SSH server did not start')
            ssh=['ssh','-T','-p',str(port),'-i',str(tmp/'client'),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(tmp/'known_hosts'),getpass.getuser()+'@127.0.0.1']
        if args.upgrade:
            exec(compile((root/'scripts/test-snapshot-upgrade.py').read_text(),str(root/'scripts/test-snapshot-upgrade.py'),'exec'),globals())
            raise SystemExit(0)
        session=api('sessions.create',dict(name='Headless snapshots',directory=str(tmp)),write=True);project=session['project'];projects.append((env,project))
        target=api('panes.get',dict(project=project,pane=1))['target'];owner=target['pid']
        meta=wait(lambda:(m if (m:=status(env)).get('pid') and stamp(m['pid'])==m['start_ticks'] else None),'collector did not start');collectors.append((meta['pid'],meta['start_ticks']))
        assert not list((runtime/'starflux').glob('v-*.state.json')),'headless collection created a frontend'
        api('panes.send',dict(target=target,text='HEADLESS_界',submit=True),write=True)
        wait(lambda:'SAVED_HEADLESS_界' in screen(env,project).get('formatted',''),'no-controller snapshot was not captured')
        assert screen(env,project)['pid']==owner and stamp(owner)==target['start_ticks']
        assert (Path(env['XDG_DATA_HOME'])/'starflux/screens'/project/'p0.screen.json').stat().st_mode&0o777==0o600
        with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
            peers=list(pool.map(lambda _:api('snapshots.status'),range(6)))
        assert all(p['running'] and p['status']['pid']==meta['pid'] for p in peers),'multiple collectors claimed one namespace'
        model=Path(env['XDG_DATA_HOME'])/'starflux/projects'/(project+'.model.json');saved=json.loads(model.read_text());marker=tmp/'MUST_NOT_LAUNCH'
        saved['terminal_commands']={'0':'touch '+shlex.quote(str(marker))};saved['terminal_launch_pending']=[0];model.write_text(json.dumps(saved))
        api('panes.send',dict(target=target,text='AFTER_API_EXIT',submit=True),write=True)
        wait(lambda:'SAVED_AFTER_API_EXIT' in screen(env,project).get('formatted',''),'collector stopped with its API parent')
        assert not marker.exists() and stamp(owner)==target['start_ticks'],'collector launched or replaced an agent'
        # Register a read-only import of the same private source, with no UI or
        # renderer. A collector must inspect its original socket/session.
        base=model.parent;imported='collector-import'
        config=json.loads((base/(project+'.json')).read_text())
        socket_path,session_id=subprocess.check_output(['tmux','-L','starflux-'+project,'display-message','-p','-t','p0','#{socket_path} #{session_id}'],env=env,text=True).strip().split()
        config.update(workspace=imported,tmux=dict(socket=socket_path,session=session_id))
        (base/(imported+'.json')).write_text(json.dumps(config))
        (base/(imported+'.model.json')).write_text(model.read_text())
        api('panes.send',dict(target=target,text='IMPORTED_WITH_NO_CONTROLLER',submit=True),write=True)
        wait(lambda:'SAVED_IMPORTED_WITH_NO_CONTROLLER' in screen(env,imported).get('formatted',''),'headless imported tmux source was not captured')
        assert screen(env,imported)['pid']==owner and stamp(owner)==target['start_ticks']
        policy(env,project,False);api('panes.send',dict(target=target,text='WHILE_PAUSED',submit=True),write=True);time.sleep(12)
        assert not screen(env,project),'paused collector wrote a snapshot'
        policy(env,project,True);api('snapshots.status')
        wait(lambda:'SAVED_WHILE_PAUSED' in screen(env,project).get('formatted',''),'collector did not resume after shared policy changed')
        second=api('sessions.create',dict(name='Isolated metadata',directory=str(tmp)),scope=env2,write=True)['project'];projects.append((env2,second))
        other=api('panes.get',dict(project=second,pane=1),scope=env2)['target']
        second_meta=wait(lambda:(m if (m:=status(env2)).get('pid') and stamp(m['pid'])==m['start_ticks'] else None),'second namespace did not start');collectors.append((second_meta['pid'],second_meta['start_ticks']))
        assert second_meta['pid']!=meta['pid']
        assert not (Path(env2['XDG_DATA_HOME'])/'starflux/screens'/project).exists(),'collector mixed data namespaces'
        stop(env);assert stamp(second_meta['pid'])==second_meta['start_ticks'] and stamp(owner)==target['start_ticks']
        shutil.rmtree(runtime);runtime.mkdir(mode=0o700)
        wait(lambda:stamp(second_meta['pid'])!=second_meta['start_ticks'],'old collector survived runtime replacement',timeout=20)
        assert not (runtime/'starflux').exists(),'old collector recreated its removed runtime'
        api('snapshots.status',scope=env2)
        restarted=wait(lambda:(m if (m:=status(env2)).get('pid') and m['pid']!=second_meta['pid'] and stamp(m['pid'])==m['start_ticks'] else None),'new namespace did not get a collector');collectors.append((restarted['pid'],restarted['start_ticks']))
        api('panes.send',dict(target=other,text='AFTER_RUNTIME_REPLACEMENT',submit=True),scope=env2,write=True)
        wait(lambda:'SAVED_AFTER_RUNTIME_REPLACEMENT' in screen(env2,second).get('formatted',''),'new collector did not attach to retained terminal')
        assert stamp(other['pid'])==other['start_ticks'] and not marker.exists()
        print(json.dumps(dict(result='PASS',ssh=args.ssh,no_controller=True,parent_exit_survival=True,single_collector=True,headless_tmux_import=True,metadata_namespaces_isolated=True,unicode_screen=True,private_files=True,saved_commands_not_launched=True,shared_pause_resume=True,cold_runtime_retirement=True,retained_processes_preserved=True)))
    finally:
        for scope in [env,env2]:
            try:stop(scope)
            except (AssertionError,OSError,ValueError):pass
        for scope,project in projects:subprocess.run(['tmux','-L','starflux-'+project,'kill-server'],env=scope,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        if daemon:
            daemon.terminate();daemon.wait(timeout=5)
