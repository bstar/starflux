"""Private collector v1-to-v2 handoff, optionally with an actual previous ELF."""
disabled={**env,'STARFLUX_SNAPSHOT_SERVICE':'off'}
session=api('sessions.create',dict(name='Collector upgrade',directory=str(tmp)),scope=disabled,write=True)
project=session['project'];projects.append((env,project))
target=api('panes.get',dict(project=project,pane=1),scope=disabled)['target']
owner=target['pid'];owner_stamp=stamp(owner)
service_root=runtime/'starflux';prefix='snapshot-'+namespace(env)
request=service_root/(prefix+'.request.json');request.write_text(json.dumps(dict(version=1,stop=False)))
previous=None;legacy=None
if args.previous_executable:
    assert args.previous_executable.is_file(),'previous executable is unavailable'
    previous=tmp/'previous-executable'
    previous.write_text('#!/bin/bash\nsource '+shlex.quote(str(root/'target/run-env.sh'))+'\nexec '+shlex.quote(str(args.previous_executable.resolve()))+' "$@"\n');previous.chmod(0o700)
    info=service_root.stat()
    command=[str(previous),'--snapshot-worker',str(service_root),str(info.st_dev)+':'+str(info.st_ino)]
else:
    fake=tmp/'legacy-collector.py'
    fake.write_text('''import fcntl,json,os,sys,time
from pathlib import Path
root=Path(sys.argv[-2]);prefix=sys.argv[-1]
lock=(root/(prefix+'.lock')).open('a');fcntl.flock(lock,fcntl.LOCK_EX)
ticks=int(Path('/proc',str(os.getpid()),'stat').read_text().rsplit(')',1)[1].split()[19])
record=dict(version=1,pid=os.getpid(),start_ticks=ticks,boot_id=Path('/proc/sys/kernel/random/boot_id').read_text().strip())
(root/(prefix+'.status.json')).write_text(json.dumps(record))
end=time.monotonic()+25
while time.monotonic()<end:
 try:
  request=json.loads((root/(prefix+'.request.json')).read_text())
  if request.get('stop') or request.get('version',0)>1:break
 except (OSError,ValueError):pass
 time.sleep(.02)
else:raise SystemExit('legacy fixture did not receive a handoff')
''')
    command=['python3',str(fake),'--snapshot-worker',str(service_root),prefix]
try:
    legacy=subprocess.Popen(command,env=env,stdout=subprocess.DEVNULL,stderr=(tmp/'legacy-collector.log').open('w'),start_new_session=True)
    original=wait(lambda:(s if (s:=status(env)).get('pid')==legacy.pid and s.get('version')==1 else None),'legacy collector did not publish its identity')
    assert stamp(legacy.pid)==original['start_ticks']
    api('snapshots.status')
    wait(lambda:legacy.poll() is not None,'previous collector did not retire cooperatively')
    assert legacy.returncode==0,(tmp/'legacy-collector.log').read_text()
    replacement=wait(lambda:(s if (s:=status(env)).get('version')==2 and s.get('pid')!=legacy.pid and stamp(s.get('pid'))==s.get('start_ticks') else None),'replacement collector did not take over')
    collectors.append((replacement['pid'],replacement['start_ticks']))
    api('panes.send',dict(target=target,text='AFTER_COLLECTOR_HANDOFF_界',submit=True),write=True)
    wait(lambda:'SAVED_AFTER_COLLECTOR_HANDOFF_界' in screen(env,project).get('formatted',''),'replacement collector did not capture retained terminal')
    assert stamp(owner)==owner_stamp,'collector handoff replaced the terminal process'
    assert not list(service_root.glob('v-*.state.json')),'collector handoff opened a renderer'
    if previous:
        # A loaded older client accepts a newer collector without requesting a
        # downgrade or launching another worker.
        query=dict(version=1,id='older-client',method='snapshots.status',params={})
        result=json.loads(subprocess.check_output([str(previous),'--api',json.dumps(query)],env=env,timeout=20))
        assert result['ok'] and result['result']['status']['pid']==replacement['pid'],result
        assert json.loads(request.read_text())['version']==2,'old client downgraded the collector request'
        # Demonstrate why the old code needs replacement: the same read-only
        # archive is accepted by the new binary and rejected by the old one.
        boundary='upgrade-boundary';base=Path(env['XDG_DATA_HOME'])/'starflux/projects'
        config=json.loads((base/(project+'.json')).read_text());config['workspace']=boundary
        (base/(boundary+'.json')).write_text(json.dumps(config));(base/(boundary+'.model.json')).write_bytes((base/(project+'.model.json')).read_bytes())
        document=screen(env,project);document.pop('formatted',None)
        document.update(project=boundary,rows=8,columns=40,cursor=[0,40],formatted_base64=base64.b64encode(b'X'*40).decode())
        folder=base.parent/'screens'/boundary;folder.mkdir(parents=True,mode=0o700);(folder/'p0.screen.json').write_text(json.dumps(document))
        saved=api('panes.saved_screen',dict(project=boundary,pane=1),scope=disabled)
        assert saved['text']=='X'*40 and saved['screen']['cursor']==[0,40]
        query=dict(version=1,id='old-boundary',method='panes.saved_screen',params=dict(project=boundary,pane=1))
        rejected=json.loads(subprocess.check_output([str(previous),'--api',json.dumps(query)],env=disabled,timeout=20))
        assert not rejected['ok'] and 'cursor' in rejected['error']['message'],rejected
    assert stamp(replacement['pid'])==replacement['start_ticks'] and stamp(owner)==owner_stamp
    print(json.dumps(dict(result='PASS',ssh=args.ssh,previous_version=1,current_version=2,actual_previous_binary=previous is not None,cooperative_handoff=True,retained_terminal_preserved=True,new_capture=True,no_renderer=True,older_client_does_not_downgrade=previous is not None,wrap_boundary_compatibility=previous is not None)))
finally:
    if legacy and legacy.poll() is None:
        request.write_text(json.dumps(dict(version=2,stop=True)))
        try:legacy.wait(timeout=5)
        except subprocess.TimeoutExpired:legacy.terminate();legacy.wait(timeout=5)
