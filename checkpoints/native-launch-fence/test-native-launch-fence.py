#!/usr/bin/env python3
"""Normal headless automation with native sources, isolated from user sessions."""
import argparse,hashlib,base64,getpass,json,os,shlex,shutil,signal,socket,subprocess,tempfile,time,uuid
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
 def command(*argv):return [*ssh,shlex.join(['env','SHELL='+env['SHELL'],str(wrapper),*argv])] if args.ssh else [str(wrapper),*argv]
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
  model=root/'data/starflux/projects'/(project+'.model.json')
  saved_launch=dict(id=str(uuid.uuid4()),epoch=str(uuid.uuid4()),argv=['/usr/bin/touch',str(root/'MUST_NOT_RUN')],directory=str(root),size=dict(rows=24,columns=80,cell=[8,16]),palette=dict(foreground=[220,215,186],background=[31,31,40],ansi=[[i*10]*3 for i in range(16)]))
  digest=hashlib.sha256(json.dumps(saved_launch,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()
  old_epoch=str(uuid.uuid4());pending=json.loads(json.dumps(target));pending['previous']=None
  pending['binding'].update(pane=1,terminal_epoch=old_epoch,source_id=saved_launch['id'],source_epoch=saved_launch['epoch'],launch_digest=digest,source=None)
  slot=Path(target['namespace'])/'native-sources/panes'/project/'p1.json';slot.write_text(json.dumps(pending['binding']));slot.chmod(0o600)
  state=json.loads(model.read_text());state['terminals'].append(1);state['terminal_epochs']['1']=old_epoch;state['native_sources']['1']=pending
  state.setdefault('terminal_commands',{})['1']='/usr/bin/touch '+str(root/'MUST_NOT_RUN')
  state.setdefault('terminal_launch_pending',[]).append(1);model.write_text(json.dumps(state))
  before=(root/'starts').read_text()
  info=native(dict(op='call',target=pending,action='inspect'))
  assert info['native_pending_recovery_available'] and info['launch_status']=='not_started',info
  receipt=Path(target['namespace'])/'native-sources/launches'/(saved_launch['id']+'.json')
  assert not receipt.exists() and (root/'starts').read_text()==before,'read-only health consumed a launch'
  operation=str(uuid.uuid4());params=dict(project=project,pane=2,epoch=old_epoch,confirm='RECOVER')
  result=api('panes.recover',params,True,operation=operation)
  fresh=lookup(project,1);sources.append(fresh['binding']['source'])
  wait(lambda:'NATIVE_READY' in screen(fresh)['text'],'fresh recovery shell did not start')
  assert fresh['binding']['terminal_epoch']!=old_epoch and fresh['binding']['source_id']!=saved_launch['id']
  assert len((root/'starts').read_text().splitlines())==len(before.splitlines())+1
  fence=json.loads(receipt.read_text());assert fence['sealed'] and fence['owner']==owner and fence['digest']==digest
  assert stamp(source['pid'])==source['start_ticks'],'pending recovery stopped sibling source'
  assert api('panes.recover',params,True,operation=operation)==result
  api('panes.recover',params,True,False)
  native(dict(op='open',target=pending,launch=saved_launch),ok=False)
  # Deliver an old already-authorized Open directly, bypassing slot discovery.
  broker=root/'data/starflux/native-sources';token=json.loads((broker/'token.json').read_text())
  with socket.socket(socket.AF_UNIX) as peer:
   peer.connect(str(broker/'broker.sock'));peer.sendall((json.dumps(dict(version=1,id=1,token=token,action=dict(op='open',launch=saved_launch)))+'\n').encode());reply=json.loads(peer.makefile('rb').readline())
  assert not reply['ok'] and 'consumed' in reply['error'],reply
  assert not (root/'MUST_NOT_RUN').exists()
  state=json.loads(model.read_text());assert '1' not in state.get('terminal_commands',{}) and 1 not in state.get('terminal_launch_pending',[])
  # A known source whose Open reply was lost still resolves by discovery.
  fresh_epoch=fresh['binding']['terminal_epoch'];fresh_pid=fresh['binding']['source']
  state=json.loads(model.read_text());state['native_sources']['1']['binding']['source']=None;model.write_text(json.dumps(state))
  api('panes.recover',dict(project=project,pane=2,epoch=fresh_epoch,confirm='RECOVER'),True)
  recovered=lookup(project,1);sources.append(recovered['binding']['source'])
  wait(lambda:'NATIVE_READY' in screen(recovered)['text'],'lost-reply recovery failed')
  assert stamp(fresh_pid['pid'])!=fresh_pid['start_ticks'] and stamp(source['pid'])==source['start_ticks']
  assert len((root/'starts').read_text().splitlines())==len(before.splitlines())+2
  print(json.dumps(dict(result='PASS',ssh=args.ssh,pending_never_delivered_recovery=True,health_read_only=True,durable_fence=True,fresh_shell_only=True,delayed_open_rejected=True,saved_command_not_replayed=True,operation_replay_safe=True,lost_reply_discovery_preserved=True,sibling_preserved=True)))

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
