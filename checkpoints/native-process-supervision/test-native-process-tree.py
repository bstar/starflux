#!/usr/bin/env python3
"""Normal headless automation with native sources, isolated from user sessions."""
import argparse,base64,getpass,json,os,shlex,shutil,signal,socket,subprocess,tempfile,time,uuid
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--executable',type=Path,required=True);p.add_argument('--ssh',action='store_true');p.add_argument('--guardian-loss',action='store_true');args=p.parse_args();exe=args.executable.resolve()
def stamp(pid):
 try:return int(Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()[19])
 except OSError:return None
def wait(f,label):
 end=time.monotonic()+15
 while time.monotonic()<end:
  value=f()
  if value:return value
  time.sleep(.05)
 raise AssertionError(label)
with tempfile.TemporaryDirectory(prefix='sf-normal-native-') as temp:
 root=Path(temp);runtime=root/'runtime';runtime.mkdir(mode=0o700);sources=[];worker=None;extra_children=[]
 shell=root/'fake-agent';shell.write_text(r'''#!/usr/bin/python3
import ctypes,os,sys,tty
if len(sys.argv)>1 and sys.argv[1]=='-c':os.execv('/bin/sh',['sh',*sys.argv[1:]])
ctypes.CDLL(None).prctl(15,ctypes.c_char_p(b'codex'),0,0,0)
tty.setraw(0)
with open(os.environ['NATIVE_PRIVATE_STARTS'],'a') as f:f.write(str(os.getpid())+'\n')
os.write(1,b'\x1b[?2004h\x1b[31mNATIVE_READY caf\xc3\xa9 \xe7\x95\x8c\x1b[0m\r\n')
data=b'';draft=b'';count=0
while True:
 data+=os.read(0,65536)
 while data:
  if data.startswith(b'\x1b[200~'):
   end=data.find(b'\x1b[201~',6)
   if end<0:break
   draft+=data[6:end];data=data[end+6:]
  elif data[:1] in (b'\r',b'\n'):
   data=data[1:];count+=1;os.write(1,('SUBMIT '+str(count)+' '+draft.decode()+'\r\n').encode())
   if draft==b'EXIT':os._exit(0)
   if draft==b'HOLD_CHILD':
    child=os.fork()
    if child==0:
     import signal,time
     signal.signal(signal.SIGHUP,signal.SIG_IGN);signal.signal(signal.SIGTERM,signal.SIG_IGN)
     with open(os.environ['NATIVE_PRIVATE_STARTS']+'.child','w') as f:f.write(str(os.getpid()))
     while True:time.sleep(1)

   if draft in (b'DETACHED_CHILD',b'NESTED_CHILD',b'FORK_RACE'):
    mode=draft.decode();child=os.fork()
    if child==0:
     import signal,time
     os.setsid()
     null=os.open('/dev/null',os.O_RDWR)
     for fd in (0,1,2):os.dup2(null,fd)
     if null>2:os.close(null)
     signal.signal(signal.SIGHUP,signal.SIG_IGN);signal.signal(signal.SIGTERM,signal.SIG_IGN)
     if mode=='NESTED_CHILD':
      ctypes.CDLL(None).prctl(36,1,0,0,0)
      grandchild=os.fork()
      if grandchild==0:
       os.setsid()
       with open(os.environ['NATIVE_PRIVATE_STARTS']+'.nested','w') as f:f.write(str(os.getpid()))
       while True:time.sleep(.05)
     if mode=='FORK_RACE':
      def on_hup(*args):
       grandchild=os.fork()
       if grandchild==0:
        signal.signal(signal.SIGHUP,signal.SIG_IGN)
        with open(os.environ['NATIVE_PRIVATE_STARTS']+'.race','w') as f:f.write(str(os.getpid()))
        while True:time.sleep(.05)
       os._exit(0)
      signal.signal(signal.SIGHUP,on_hup)
     with open(os.environ['NATIVE_PRIVATE_STARTS']+'.detached','w') as f:f.write(str(os.getpid()))
     while True:time.sleep(.05)

   if draft==b'HISTORY':
    for i in range(120):os.write(1,('ROW '+str(i)+' NEEDLE caf\u00e9\r\n').encode())
   draft=b''
  elif data.startswith(b'\x1b') and len(data)<6:break
  else:draft+=data[:1];data=data[1:]
''');shell.chmod(0o700)
 env={**os.environ,'XDG_RUNTIME_DIR':str(runtime),'XDG_DATA_HOME':str(root/'data'),'XDG_CONFIG_HOME':str(root/'config'),'TMUX_TMPDIR':str(root/'tmux'),'SHELL':str(shell),'NATIVE_PRIVATE_STARTS':str(root/'starts'),'STARFLUX_SNAPSHOT_SERVICE':'off','STARFLUX_HERDR':str(root/'not-herdr')}
 wrapper=root/'run';wrapper.write_text('#!/bin/bash\nsource '+shlex.quote(str(Path.cwd()/'target/run-env.sh'))+'\nexec '+shlex.quote(str(exe))+' "$@"\n');wrapper.chmod(0o700)
 daemon=None;ssh=[];host_config=None
 if args.ssh:
  for key in ['host-key','client-key']:subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(root/key)],check=True)
  (root/'authorized_keys').write_text((root/'client-key.pub').read_text())
  with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
  (root/'known_hosts').write_text('[127.0.0.1]:'+str(port)+' '+(root/'host-key.pub').read_text())
  forced=root/'private-command';forced.write_text('#!/bin/bash\n'+''.join('export '+key+'='+shlex.quote(env[key])+'\n' for key in ['XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','NATIVE_PRIVATE_STARTS','STARFLUX_SNAPSHOT_SERVICE','STARFLUX_HERDR'])+'exec /bin/sh -c "$SSH_ORIGINAL_COMMAND"\n');forced.chmod(0o700)
  (root/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {root}/host-key\nPidFile {root}/sshd.pid\nAuthorizedKeysFile {root}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {getpass.getuser()}\nForceCommand {forced}\nPerSourcePenalties no\nLogLevel ERROR\n')
  daemon=subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd','-D','-e','-f',str(root/'sshd_config')],stderr=(root/'sshd.log').open('w'))
  def listening():
   try:
    with socket.create_connection(('127.0.0.1',port),timeout=.1):return True
   except OSError:return False
  wait(listening,'private SSH daemon unavailable')
  host_config=root/'ssh_config';host_config.write_text(f'Host proof\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {root}/client-key\n IdentitiesOnly yes\n BatchMode yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {root}/known_hosts\n')
  ssh=['ssh','-F',str(host_config),'-T','proof']
 def command(*argv):return [*ssh,shlex.join([str(wrapper),*argv])] if args.ssh else [str(wrapper),*argv]
 def api(method,params=None,write=False,ok=True,operation=None):
  request=dict(version=1,id=1,method=method,params=params or {},operation_id=(operation or str(uuid.uuid4())) if write else None)
  r=subprocess.run(command('--api-stdio'),env=env,input=(json.dumps(request)+'\n').encode(),capture_output=True,timeout=30)
  assert r.returncode==0,(method,r.stderr.decode())
  value=json.loads(r.stdout.splitlines()[0]);assert value['ok']==ok,(method,value)
  return value.get('result',value.get('error'))
 def native(request,ok=True):
  r=subprocess.run(command('--native-pane-rpc'),env=env,input=json.dumps(request).encode(),capture_output=True,timeout=20)
  assert (r.returncode==0)==ok,r.stderr.decode();return json.loads(r.stdout) if ok else r
 def lookup(project,pane=0):
  state=json.loads((root/'data/starflux/projects'/(project+'.model.json')).read_text());return state['native_sources'][str(pane)]
 def screen(target):return native(dict(op='call',target=target,action='screen'))
 try:
  operation=str(uuid.uuid4());created=api('sessions.create',dict(name='Native real API',directory=str(root),native_terminals=True),True,operation=operation);project=created['project']
  target=lookup(project);source=target['binding']['source'];sources.append(source)
  owner=target['binding']['owner'];worker=owner
  wait(lambda:'NATIVE_READY' in screen(target)['text'],'native source output missing')
  assert api('sessions.create',dict(name='Native real API',directory=str(root),native_terminals=True),True,operation=operation)==created
  info=api('panes.get',dict(project=project,pane=1))
  assert info['status']['agent']=='codex' and info['status']['pid']==source['pid'],info
  assert info['target']['agent_pid'] in [int(p) for p in (root/'starts').read_text().splitlines()]
  assert info['target']['agent_pid']!=source['pid'], 'guardian and foreground agent identities were conflated'
  native(dict(op='call',target=target,action='guarded-input',data=base64.b64encode(b'MUST_NOT_REACH').decode(),agent=None),ok=False)
  api('panes.send',dict(target=info['target'],text='hello café',submit=True),True)
  wait(lambda:'SUBMIT 1 hello café' in screen(target)['text'],'native API input missing')
  wrong={**info['target'],'agent_start_ticks':info['target']['agent_start_ticks']+1}
  api('panes.send',dict(target=wrong,text='MUST_NOT_REACH',submit=True),True,False)
  if args.ssh:
   path=root/'data/starflux/projects'/(project+'.json');config=json.loads(path.read_text());config.update(host='proof',ssh_config=str(host_config),desktop_helper=str(wrapper),project_host=True);path.write_text(json.dumps(config))
  second=api('panes.create',dict(project=project,label='Second native'),True);sources.append(lookup(project,1)['binding']['source'])
  assert sources[1]['pid']!=sources[0]['pid']
  assert len((root/'starts').read_text().splitlines())==2
  # Native history reaches the standard history adapter, not a tmux capture.
  api('panes.send',dict(target=info['target'],text='HISTORY',submit=True),True)
  wait(lambda:'ROW 119' in screen(target)['text'],'native history output missing')
  history=native(dict(op='call',target=target,action='history'))
  assert b'ROW 0' in base64.b64decode(history['history_base64'])
  before=(root/'starts').read_text()
  # Runtime loss must not rerun the registered native source.
  import shutil
  shutil.rmtree(runtime);runtime.mkdir(mode=0o700)
  info2=api('panes.get',dict(project=project,pane=1));assert info2['target']==info['target']
  assert (root/'starts').read_text()==before
  # Snapshot helper reads the real source with no frontend/query replay.
  captured=subprocess.run(command('--workspace',project,'--pane','1','--capture'),env=env,capture_output=True,timeout=15)
  assert captured.returncode==0 and b'ROW 119' in captured.stdout,(captured.returncode,captured.stdout.decode(errors='replace'),captured.stderr.decode())
  # Explicit recovery replaces only the confirmed source, clears launch
  # permissions, and has a durable operation receipt. It never resumes an agent.
  prior=lookup(project);old_source=prior['binding']['source'];old_epoch=prior['binding']['terminal_epoch']
  model=root/'data/starflux/projects'/(project+'.model.json')
  state=json.loads(model.read_text());state.setdefault('terminal_commands',{})['0']='/usr/bin/touch '+str(root/'MUST_NOT_RUN')
  state.setdefault('terminal_launch_pending',[]).append(0);model.write_text(json.dumps(state))
  recovery_id=str(uuid.uuid4());params=dict(project=project,pane=1,epoch=old_epoch,confirm='RECOVER')
  starts=len((root/'starts').read_text().splitlines())
  recovered=api('panes.recover',params,True,operation=recovery_id)
  fresh=lookup(project);fresh_source=fresh['binding']['source'];sources.append(fresh_source)
  wait(lambda:stamp(old_source['pid'])!=old_source['start_ticks'],'old source not retired')
  wait(lambda:'NATIVE_READY' in screen(fresh)['text'],'fresh shell did not attach')
  assert fresh_source!=old_source and fresh['binding']['terminal_epoch']!=old_epoch
  assert stamp(sources[1]['pid'])==sources[1]['start_ticks'],'recovery affected another terminal'
  assert len((root/'starts').read_text().splitlines())==starts+1
  assert api('panes.recover',params,True,operation=recovery_id)==recovered
  api('panes.recover',params,True,False)
  api('panes.send',dict(target=info['target'],text='MUST_NOT_REACH',submit=True),True,False)
  state=json.loads(model.read_text());assert '0' not in state.get('terminal_commands',{}) and 0 not in state.get('terminal_launch_pending',[])
  assert not (root/'MUST_NOT_RUN').exists()
  # A lost Open reply is resolved by discovery before another explicit
  # recovery; discovery itself must not launch or replay anything.
  state=json.loads(model.read_text());state['native_sources']['0']['binding']['source']=None;model.write_text(json.dumps(state))
  before_resolve=len((root/'starts').read_text().splitlines())
  api('panes.recover',dict(project=project,pane=1,epoch=fresh['binding']['terminal_epoch'],confirm='RECOVER'),True)
  fresh=lookup(project);fresh_source=fresh['binding']['source'];sources.append(fresh_source)
  wait(lambda:'NATIVE_READY' in screen(fresh)['text'],'resolved recovery shell did not attach')
  assert len((root/'starts').read_text().splitlines())==before_resolve+1
  # Explicit termination waits for retirement before removing slot membership.
  second_info=api('panes.get',dict(project=project,pane=2))
  terminate_id=str(uuid.uuid4());terminate=dict(target=second_info['target'],confirm='TERMINATE')
  terminated=api('panes.terminate',terminate,True,operation=terminate_id)
  assert stamp(sources[1]['pid'])!=sources[1]['start_ticks']
  replacement=api('panes.create',dict(project=project,label='Reused terminated slot'),True)
  replaced=lookup(project,1);sources.append(replaced['binding']['source'])
  assert replaced['binding']['source']!=sources[1]
  assert api('panes.terminate',terminate,True,operation=terminate_id)==terminated
  api('panes.terminate',terminate,True,False)
  assert stamp(replaced['binding']['source']['pid'])==replaced['binding']['source']['start_ticks']
  # A live source cannot be reaped. A naturally exited one is reclaimed and
  # can be replaced in the same slot without authorizing its old epoch again.
  third=api('panes.create',dict(project=project,label='Natural exit'),True)
  natural=lookup(project,2);sources.append(natural['binding']['source'])
  reap=dict(project=project,pane=3,epoch=natural['binding']['terminal_epoch'])
  assert api('panes.reap',reap,True)['reaped'] is False
  natural_info=api('panes.get',dict(project=project,pane=3))
  api('panes.send',dict(target=natural_info['target'],text='EXIT',submit=True),True)
  wait(lambda:native(dict(op='call',target=natural,action='inspect'))['exited'],'source did not exit')
  wait(lambda:api('panes.reap',reap,True)['reaped'],'exited pane did not reap')
  new=api('panes.create',dict(project=project,label='Reused natural slot'),True)
  new_target=lookup(project,2);sources.append(new_target['binding']['source'])
  assert new_target['binding']['terminal_epoch']!=reap['epoch']
  api('panes.reap',reap,True,False)
  assert stamp(new_target['binding']['source']['pid'])==new_target['binding']['source']['start_ticks']
  assert stamp(fresh_source['pid'])==fresh_source['start_ticks']
  assert stamp(owner['pid'])==owner['start_ticks']
  # Every source has a dedicated guardian; a stubborn PTY holder is retired
  # before a fresh shell can appear. No test-side kill is used for recovery.
  held=api('panes.create',dict(project=project,label='Held PTY'),True)
  held_target=lookup(project,3);sources.append(held_target['binding']['source'])
  held_info=api('panes.get',dict(project=project,pane=4))
  api('panes.send',dict(target=held_info['target'],text='HOLD_CHILD',submit=True),True)
  child_file=Path(str(root/'starts')+'.child');wait(child_file.exists,'private PTY child did not start')
  child_pid=int(child_file.read_text());child_start=stamp(child_pid);child_fd=os.pidfd_open(child_pid);extra_children.append(child_fd)
  before_hold=len((root/'starts').read_text().splitlines())
  recover_held=dict(project=project,pane=4,epoch=held_target['binding']['terminal_epoch'],confirm='RECOVER')
  api('panes.recover',recover_held,True)
  released=lookup(project,3);sources.append(released['binding']['source'])
  assert released['binding']['terminal_epoch']!=recover_held['epoch']
  wait(lambda:stamp(child_pid)!=child_start,'stubborn PTY descendant survived recovery')
  wait(lambda:len((root/'starts').read_text().splitlines())==before_hold+1,'recovery did not create exactly one shell')
  assert stamp(fresh_source['pid'])==fresh_source['start_ticks'],'unrelated terminal was stopped'
  for mode,natural in [('DETACHED_CHILD',False),('NESTED_CHILD',False),('FORK_RACE',False),('DETACHED_CHILD',True)]:
   for suffix in ['detached','nested','race']:
    Path(str(root/'starts')+'.'+suffix).unlink(missing_ok=True)
   slot=api('panes.create',dict(project=project,label=mode),True)['pane']
   target=lookup(project,slot-1);sources.append(target['binding']['source'])
   ready=api('panes.get',dict(project=project,pane=slot))
   api('panes.send',dict(target=ready['target'],text=mode,submit=True),True)
   detached=Path(str(root/'starts')+'.detached');wait(detached.exists,'detached child did not start')
   identities=[]
   for path in [detached,*([Path(str(root/'starts')+'.nested')] if mode=='NESTED_CHILD' else [])]:
    wait(path.exists,'nested child did not start');pid=int(path.read_text());identities.append((pid,stamp(pid)));extra_children.append(os.pidfd_open(pid))
   if natural:
    api('panes.send',dict(target=ready['target'],text='EXIT',submit=True),True)
    wait(lambda:native(dict(op='call',target=target,action='inspect'))['exited'],'natural exit did not retire descendants')
    assert api('panes.reap',dict(project=project,pane=slot,epoch=target['binding']['terminal_epoch']),True)['reaped']
   else:api('panes.terminate',dict(target=ready['target'],confirm='TERMINATE'),True)
   for pid,start in identities:wait(lambda:stamp(pid)!=start,'detached/nested descendant survived retirement')
   race=Path(str(root/'starts')+'.race')
   if mode=='FORK_RACE':
    assert race.exists(),'race handler was not exercised';pid=int(race.read_text());assert stamp(pid) is None,'child forked during retirement survived'
   assert stamp(fresh_source['pid'])==fresh_source['start_ticks'] and stamp(owner['pid'])==owner['start_ticks']
  if args.guardian_loss:
   Path(str(root/'starts')+'.detached').unlink(missing_ok=True)
   slot=api('panes.create',dict(project=project,label='Guardian loss'),True)['pane']
   lost=lookup(project,slot-1);sources.append(lost['binding']['source'])
   status=api('panes.get',dict(project=project,pane=slot))
   api('panes.send',dict(target=status['target'],text='DETACHED_CHILD',submit=True),True)
   child_file=Path(str(root/'starts')+'.detached');wait(child_file.exists,'private detached child missing')
   child_pid=int(child_file.read_text());child_start=stamp(child_pid);extra_children.append(os.pidfd_open(child_pid))
   fd=os.pidfd_open(lost['binding']['source']['pid']);signal.pidfd_send_signal(fd,signal.SIGKILL);os.close(fd)
   observed=wait(lambda:(value if (value:=native(dict(op='call',target=lost,action='inspect')))['exited'] and value['eof'] else None),'lost guardian did not expose final EOF')
   assert observed['retirement_verified'] is False and observed['native_retirement_available'] is False
   assert stamp(child_pid)==child_start,'adversarial detached child did not survive guardian loss'
   before=(root/'starts').read_text()
   api('panes.recover',dict(project=project,pane=slot,epoch=lost['binding']['terminal_epoch'],confirm='RECOVER'),True,False)
   assert lookup(project,slot-1)==lost and (root/'starts').read_text()==before
   assert stamp(fresh_source['pid'])==fresh_source['start_ticks']
   print(json.dumps(dict(result='PASS',ssh=args.ssh,guardian_loss_blocks_replacement=True,final_eof_is_not_tree_proof=True,source_binding_preserved=True,no_duplicate_launch=True)))
  print(json.dumps(dict(result='PASS',ssh=args.ssh,stubborn_pty_child=True,detached_closed_stdio_child=True,nested_subreapers=True,fork_during_retirement=True,natural_exit_tree_cleanup=True,guardian_identity_separate_from_agent=True,no_test_side_kills=not args.guardian_loss,unrelated_sources_preserved=True,recovery=True,termination=True,natural_exit_reap=True,safe_slot_reuse=True,operation_replay_safe=True)))

 finally:
  for fd in extra_children:
   try:signal.pidfd_send_signal(fd,signal.SIGKILL)
   except ProcessLookupError:pass
   os.close(fd)
  if worker is None:
   try:worker=json.loads((root/'data/starflux/native-sources/owner.json').read_text())['owner']
   except (OSError,ValueError,KeyError):pass
  if worker and stamp(worker['pid'])==worker['start_ticks']:
   try:
    data=root/'data/starflux/native-sources';token=json.loads((data/'token.json').read_text())
    with socket.socket(socket.AF_UNIX) as s:
     s.connect(str(data/'broker.sock'));s.sendall((json.dumps(dict(version=1,id=1,token=token,action=dict(op='shutdown',instance=worker['instance'])))+'\n').encode());s.makefile('rb').readline()
   except OSError:pass
   deadline=time.monotonic()+3
   while time.monotonic()<deadline and stamp(worker['pid'])==worker['start_ticks']:time.sleep(.05)
   if stamp(worker['pid'])==worker['start_ticks']:
    fd=os.pidfd_open(worker['pid']);signal.pidfd_send_signal(fd,signal.SIGKILL);os.close(fd)
  if daemon:daemon.terminate();daemon.wait(timeout=5)
  for source in sources:
   if stamp(source['pid'])==source['start_ticks']:
    try:
     fd=os.pidfd_open(source['pid']);signal.pidfd_send_signal(fd,signal.SIGKILL);os.close(fd)
    except ProcessLookupError:pass
