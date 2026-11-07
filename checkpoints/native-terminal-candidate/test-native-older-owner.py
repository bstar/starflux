#!/usr/bin/env python3
"""Normal headless automation with native sources, isolated from user sessions."""
import argparse,base64,getpass,json,os,shlex,shutil,signal,socket,subprocess,tempfile,time,uuid
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--executable',type=Path,required=True);p.add_argument('--ssh',action='store_true');args=p.parse_args();exe=args.executable.resolve()
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
 root=Path(temp);runtime=root/'runtime';runtime.mkdir(mode=0o700);sources=[];worker=None;old_owner=None
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
  old_exe=Path.cwd()/'target/release/starflux-previous-native';assert old_exe.is_file()
  old_wrapper=root/'run-old';old_wrapper.write_text('#!/bin/bash\nsource '+shlex.quote(str(Path.cwd()/'target/run-env.sh'))+'\nexec '+shlex.quote(str(old_exe))+' "$@"\n');old_wrapper.chmod(0o700)
  argv=[str(old_wrapper),'--native-source-discover',str(root/'data/starflux')]
  argv=[*ssh,shlex.join(argv)] if args.ssh else argv
  discovered=subprocess.run(argv,env=env,capture_output=True,timeout=20);assert discovered.returncode==0,discovered.stderr.decode()
  old_owner=json.loads(discovered.stdout);worker=old_owner
  operation=str(uuid.uuid4());created=api('sessions.create',dict(name='Native real API',directory=str(root),native_terminals=True),True,operation=operation);project=created['project']
  target=lookup(project);source=target['binding']['source'];sources.append(source)
  owner=target['binding']['owner'];worker=owner
  wait(lambda:'NATIVE_READY' in screen(target)['text'],'native source output missing')
  assert api('sessions.create',dict(name='Native real API',directory=str(root),native_terminals=True),True,operation=operation)==created
  info=api('panes.get',dict(project=project,pane=1))
  assert info['status']['agent']=='codex' and info['status']['pid']==source['pid'],info
  assert info['target']['agent_pid']==source['pid']
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
  # New lifecycle operations must not trust an older owner's Close semantics.
  assert owner==old_owner
  before=(root/'starts').read_text();saved=lookup(project)
  current=api('panes.get',dict(project=project,pane=1))
  assert current['status']['native_retirement_available'] is False
  assert current['capabilities']['input'] is True
  assert current['capabilities']['terminate'] is False and current['capabilities']['recover'] is False
  api('panes.recover',dict(project=project,pane=1,epoch=saved['binding']['terminal_epoch'],confirm='RECOVER'),True,False)
  api('panes.terminate',dict(target=current['target'],confirm='TERMINATE'),True,False)
  assert lookup(project)==saved and (root/'starts').read_text()==before
  assert all(stamp(s['pid'])==s['start_ticks'] for s in sources)
  assert stamp(old_owner['pid'])==old_owner['start_ticks']
  api('panes.send',dict(target=current['target'],text='AFTER_REFUSED_RETIREMENT',submit=True),True)
  wait(lambda:'AFTER_REFUSED_RETIREMENT' in screen(target)['text'],'old source input stopped after refused retirement')
  print(json.dumps(dict(result='PASS',ssh=args.ssh,older_owner_preserved=True,compatible_read_and_input=True,unsafe_retirement_rejected=True,unsupported_lifecycle_capabilities_false=True,no_command_replay=True,original_sources_preserved=True)))

 finally:
  if old_owner and old_owner!=worker and stamp(old_owner['pid'])==old_owner['start_ticks']:
   fd=os.pidfd_open(old_owner['pid'])
   if stamp(old_owner['pid'])==old_owner['start_ticks']:signal.pidfd_send_signal(fd,signal.SIGTERM)
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
