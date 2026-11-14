#!/usr/bin/env python3
"""Private retained native-PTY broker acceptance; never opens a real project/agent."""
import concurrent.futures,queue,threading
import argparse,base64,getpass,json,os,select,secrets,shlex,shutil,signal,socket,subprocess,tempfile,time,uuid
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--executable',type=Path,required=True)
parser.add_argument('--ssh',action='store_true')
parser.add_argument('--streams',action='store_true')
parser.add_argument('--service',action='store_true')
parser.add_argument('--worker-executable',type=Path)
args=parser.parse_args();exe=args.executable.resolve()
def wait(f,label,timeout=12):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        value=f()
        if value:return value
        time.sleep(.02)
    raise AssertionError(label)
with tempfile.TemporaryDirectory(prefix='sf-source-') as directory:
    root=Path(directory);sock=root/'broker.sock';receipts=root/'receipts';token=secrets.token_hex(32);key=root/'key';key.write_text(token);key.chmod(0o600)
    daemon=None;worker=None;worker_pid=None;worker_start=None;worker_fd=None;ssh=[];clients=[];references=[];extra_references=[]
    data=root/'managed-data'/('namespace-'+'x'*60)/('deep-'+'y'*60)
    worker_exe=args.worker_executable.resolve() if args.worker_executable else exe
    private_runtime=root/'runtime';private_runtime.mkdir(mode=0o700)
    private_env=['env','XDG_RUNTIME_DIR='+str(private_runtime),'XDG_DATA_HOME='+str(root/'private-data'),'XDG_CONFIG_HOME='+str(root/'private-config')]
    seeded=False;loaded_unlinked=False
    if args.service:
        bad_bin=root/'bundle-bin';bad_bin.mkdir();bad_python=bad_bin/'python3';bad_python.write_text('#!/bin/sh\ntouch '+shlex.quote(str(root/'MUST_NOT_USE_BUNDLED_PYTHON'))+'\nexec '+shlex.quote(shutil.which('python3'))+' "$@"\n');bad_python.chmod(0o700)
        private_env+=['STARFLUX_SYSTEM_PATH='+os.environ['PATH'],'PATH='+str(bad_bin)+':'+os.environ['PATH'],'GST_PLUGIN_SCANNER=/private/should-not-leak','ALSA_PLUGIN_DIR=/private/should-not-leak','KITTY_PID=12345','TERM_PROGRAM=kitty','SF_PRIVATE_SERVICE_ENV_CHECK=1']
    def connector():return private_env+([str(worker_exe),'--native-source-stdio',str(data)] if args.service else [str(exe),'--relay',str(sock)])
    def envelope(action,identity):
        request=dict(version=1,id=identity,action=action)
        if not args.service:request['token']=token
        return request
    def stamp(pid):
        try:
            fields=Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()
            return None if fields[0]=='Z' else int(fields[19])
        except OSError:return None
    palette=dict(foreground=[220,215,186],background=[31,31,40],ansi=[[i*10,i*10,i*10] for i in range(16)])
    size=dict(rows=24,columns=80,cell=[8,16])
    script=root/'source.py'
    script.write_text(r'''import base64,hashlib,os,select,sys,termios,tty,fcntl,struct,time
tty.setraw(0)
if 'SF_PRIVATE_SERVICE_ENV_CHECK' in os.environ:
 assert all(name not in os.environ for name in ['STARFLUX_SYSTEM_PATH','GST_PLUGIN_SCANNER','ALSA_PLUGIN_DIR','KITTY_PID','TERM_PROGRAM'])
with open(sys.argv[1],'a') as f:f.write('START\n')
assert sys.argv[2]=='literal ; $(touch NEVER_EXECUTE) café'
def receive(n):
 data=b'';end=time.monotonic()+8
 while len(data)<n:
  remaining=end-time.monotonic()
  if remaining<=0 or not select.select([0],[],[],remaining)[0]:raise SystemExit('source input timeout')
  data+=os.read(0,min(65536,n-len(data)))
 return data
os.write(1,b'\x1b]10;?\x1b\\\x1b[14t')
expected=b'\x1b]10;rgb:dcdc/d7d7/baba\x1b\\\x1b[4;384;640t'
assert receive(len(expected))==expected
os.write(1,b'BROKER_SOURCE_READY\r\n\x1b[31mANSI_COLOR\x1b[0m\r\n')
while True:
 line=b''
 while not line.endswith(b'\n'):
  if not line and not select.select([0],[],[],60)[0]:raise SystemExit('idle fixture timeout')
  line+=receive(1)
 if line.startswith(b'COPY '):
  os.write(1,b'\x1b]52;c;'+base64.b64encode(line[5:-1])+b'\x1b\\');continue
 if line==b'PING\n' or line.startswith(b'PING '):os.write(1,b'\r\n\r\n'+line.replace(b'PING',b'PONG',1)+b'\r\n')
 elif line in (b'IMAGE\n',b'DEFER\n'):
  if line==b'DEFER\n':time.sleep(.25)
  os.write(1,b'\x1b_Ga=T,f=32,s=1,v=2,c=2,r=2,C=1,i=51;'+base64.b64encode(bytes([255,0,0,255,0,0,255,255]))+b'\x1b\\\x1b[c')
  expected=b'\x1b_Gi=51;OK\x1b\\\x1b[?1;2c';assert receive(len(expected))==expected
  os.write(1,b'IMAGE_READY\r\n' if line==b'IMAGE\n' else b'\r\n\r\nDEFERRED_IMAGE_READY\r\n')
 elif line==b'RESET\n':os.write(1,b'\x1bcRESET_DONE\r\n')
 elif line==b'SIZE\n':
  rows,cols,_,_=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,bytes(8)));os.write(1,('SIZE='+str(rows)+'x'+str(cols)+'\r\n').encode())
 elif line.startswith(b'BIG '):
  n=int(line[4:]);data=receive(n);assert data==b'x'*n;os.write(1,('BIG_OK='+str(n)+'\r\n').encode())
 elif line==b'EXIT\n':break
 elif line==b'HANG\n':
  import signal
  signal.signal(signal.SIGHUP,signal.SIG_IGN);signal.signal(signal.SIGTERM,signal.SIG_IGN);os.write(1,b'HANGING\r\n')
 else:raise SystemExit('unexpected/duplicated source input')
''')
    def run(command,**kwargs):
        command=private_env+command
        return subprocess.run([*ssh,shlex.join(command)] if ssh else command,**kwargs)
    def start_worker():
        global worker,worker_pid,worker_start,worker_fd,sock,receipts,token,seeded,loaded_unlinked
        if args.service:
            flag='--native-source-discover' if args.worker_executable else '--discover'
            executable=worker_exe
            if not seeded:
                executable=root/'loaded-native-owner';shutil.copy2(worker_exe,executable);seeded=True
            with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
                results=list(pool.map(lambda _:run([str(executable),flag,str(data)],capture_output=True,text=True,check=True,timeout=15),range(3)))
            owner=json.loads(results[0].stdout);assert all(json.loads(r.stdout)==owner for r in results),'concurrent discovery created different owners'
            if executable!=worker_exe:executable.unlink();loaded_unlinked=True
            worker_pid=owner['pid'];sock=data/'native-sources/broker.sock';receipts=data/'native-sources/launches';token=json.loads((data/'native-sources/token.json').read_text())
        elif ssh:
            code="import subprocess,sys;f=open(sys.argv[5],'ab',buffering=0);p=subprocess.Popen([sys.argv[1],'--serve',*sys.argv[2:5]],stdin=subprocess.DEVNULL,stdout=f,stderr=f,start_new_session=True);print(p.pid)"
            result=run(['python3','-c',code,str(exe),str(sock),str(receipts),str(key),str(root/'worker.log')],capture_output=True,text=True,check=True)
            worker_pid=int(result.stdout)
        else:
            worker=subprocess.Popen([str(exe),'--serve',str(sock),str(receipts),str(key)],stdin=subprocess.DEVNULL,stdout=(root/'worker.log').open('ab'),stderr=subprocess.STDOUT,start_new_session=True)
            worker_pid=worker.pid
        worker_start=wait(lambda:stamp(worker_pid),'private broker did not start');worker_fd=os.pidfd_open(worker_pid)
        wait(sock.exists,'private broker did not bind')
    class Client:
        def __init__(self):
            self.sequence=0;self.process=None;self.socket=None
            if ssh or args.service:
                command=[*ssh,shlex.join(connector())] if ssh else connector()
                self.process=subprocess.Popen(command,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(root/'relay.log').open('ab'))
                self.reader=self.process.stdout;self.writer=self.process.stdin
            else:
                self.socket=socket.socket(socket.AF_UNIX);self.socket.connect(str(sock));self.socket.settimeout(None)
                self.reader=self.socket.makefile('rb');self.writer=self.socket.makefile('wb')
            self.messages=queue.Queue();self.events=[]
            def read():
                try:
                    for line in self.reader:self.messages.put(json.loads(line))
                finally:self.messages.put(None)
            self.thread=threading.Thread(target=read,daemon=True);self.thread.start()
            clients.append(self)
        def call(self,action,ok=True,auth=None):
            self.sequence+=1;request=envelope(action,self.sequence)
            if auth is not None:request['token']=auth
            self.writer.write((json.dumps(request)+'\n').encode());self.writer.flush()
            while True:
                reply=self.messages.get(timeout=12)
                assert reply is not None,'private broker disconnected'
                if 'event' in reply:self.events.append(reply);continue
                assert reply['id']==self.sequence and reply['ok']==ok,reply
                break
            return reply.get('result',reply)
        def event(self,timeout=12):
            if self.events:return self.events.pop(0)
            value=self.messages.get(timeout=timeout)
            assert value is not None and 'event' in value,value
            return value
        def no_event(self):
            assert not self.events,self.events
            try:value=self.messages.get(timeout=.2)
            except queue.Empty:return
            raise AssertionError(('unexpected pushed event',value))
        def close(self):
            if self.socket:
                self.socket.shutdown(socket.SHUT_RDWR);self.writer.close();self.thread.join(timeout=3);self.reader.close();self.socket.close();self.socket=None
            if self.process:
                self.process.stdin.close();self.process.wait(timeout=12);assert self.process.returncode==0;self.process=None
    def cells(client,reference,view=size):return client.call(dict(op='cells',source=reference,size=view,palette=palette))['frame']
    def text(frame):return '\n'.join(''.join(r['text'] for r in row['runs']) for row in frame['rows'])
    def info(client,reference):return client.call(dict(op='inspect',source=reference))
    def send(client,reference,data):return client.call(dict(op='input',source=reference,data=base64.b64encode(data).decode()))
    def stop_worker(client,instance):
        global worker_fd
        client.call(dict(op='shutdown',instance=instance));client.close()
        wait(lambda:stamp(worker_pid)!=worker_start,'private broker did not stop')
        os.close(worker_fd);worker_fd=None
        if worker:worker.wait(timeout=5)
    try:
        # Verify fixture syntax before starting any private worker/source.
        compile(script.read_text(),str(script),'exec')
        if args.ssh:
            for name in ['host','client']:subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(root/name)],check=True)
            (root/'authorized_keys').write_text((root/'client.pub').read_text())
            with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
            (root/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(root/'host.pub').read_text())
            (root/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {root}/host\nPidFile {root}/sshd.pid\nAuthorizedKeysFile {root}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
            daemon=subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd','-D','-e','-f',str(root/'sshd_config')],stderr=(root/'sshd.log').open('w'))
            def listening():
                try:
                    with socket.create_connection(('127.0.0.1',port),timeout=.1):return True
                except OSError:return False
            wait(listening,'private SSH daemon unavailable')
            ssh=['ssh','-T','-p',str(port),'-i',str(root/'client'),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(root/'known_hosts'),getpass.getuser()+'@127.0.0.1']
        start_worker();a=Client();hello=a.call(dict(op='hello'));instance=hello['instance']
        one_shot=run(connector(),input=(json.dumps(envelope(dict(op='hello'),99))+'\n').encode(),capture_output=True,timeout=12)
        assert one_shot.returncode==0 and json.loads(one_shot.stdout)['result']['instance']==instance,'half-closed client lost its response'
        if not args.service:assert not a.call(dict(op='hello'),ok=False,auth='0'*64)['ok']
        if args.service:
            invalid=run(connector(),input=(json.dumps(dict(version=1,id=404,token='0'*64,action=dict(op='hello')))+'\n').encode(),capture_output=True,timeout=15)
            assert invalid.returncode!=0 and not invalid.stdout,'connector accepted a caller-supplied authentication field'
            assert stamp(worker_pid)==worker_start
        launches=[]
        for i in range(2):
            launch=dict(id=str(uuid.uuid4()),epoch=str(uuid.uuid4()),argv=['python3','-u',str(script),str(root/('launch-'+str(i))), 'literal ; $(touch NEVER_EXECUTE) café'],directory=str(root),size=size,palette=palette)
            result=a.call(dict(op='open',launch=launch));reference=result['source'];references.append(reference);launches.append(launch)
            wait(lambda:'BROKER_SOURCE_READY' in text(cells(a,reference)),'native source did not become ready')
        first,second=references;assert first['pid']!=second['pid']
        assert 'terminal-metadata-v1' in hello['capabilities']
        if args.service:assert loaded_unlinked,'managed owner image was not unlinked before source launch'
        w=Client();v=Client();view=str(uuid.uuid4());other=str(uuid.uuid4())
        def watch(client,identity,epoch):
            client.call(dict(op='watch',source=first,view=identity,epoch=epoch,generation=epoch,focused=True,size=size,palette=palette))
            return client.event()
        def ack(client,event):
            assert client.call(dict(op='ack',view=event['view'],epoch=event['epoch'],sequence=event['sequence']))['accepted']
        initial=watch(w,view,1);assert initial['terminal_metadata']['clipboard_sequence']==0
        ack(w,initial)
        send(a,first,b'COPY first clipboard\n');event=w.event()
        assert not event['frame']['complete'] and not event['frame']['rows'],'copy-only output repainted text'
        assert event['terminal_metadata']['clipboard']=='first clipboard' and event['terminal_metadata']['clipboard_sequence']==1 and event['clipboard_floor']==0
        ack(w,event)
        for i in range(4):
            send(a,first,('PING '+str(i)+'\n').encode());event=w.event()
            assert event['terminal_metadata']['clipboard_sequence']==1 and event['terminal_metadata']['clipboard'] is None
            assert event['clipboard_floor']==1
            ack(w,event)
        resized=watch(w,view,2)
        assert resized['terminal_metadata']['clipboard'] is None and resized['clipboard_floor']==1
        assert not w.call(dict(op='ack',view=view,epoch=1,sequence=event['sequence']))['accepted']
        ack(w,resized)
        late=watch(v,other,1)
        assert late['terminal_metadata']['clipboard'] is None and late['clipboard_floor']==1
        ack(v,late)
        send(a,first,b'COPY second clipboard\n');fast=w.event();slow=v.event()
        assert fast['terminal_metadata']['clipboard']==slow['terminal_metadata']['clipboard']=='second clipboard'
        ack(w,fast)
        send(a,first,b'COPY newest clipboard\n');fast=w.event()
        assert fast['terminal_metadata']['clipboard']=='newest clipboard' and fast['clipboard_floor']==2
        ack(w,fast);v.no_event()
        ack(v,slow);latest=v.event()
        assert latest['terminal_metadata']['clipboard']=='newest clipboard' and latest['clipboard_floor']==2
        ack(v,latest)
        # Read-only snapshot RPCs never deliver retained copy effects.
        current=a.call(dict(op='cells',source=first,size=size,palette=palette))
        assert current['terminal_metadata']['clipboard'] is None and current['clipboard_floor']==3
        assert info(a,first)['terminal_metadata']['clipboard_sequence']==3
        assert info(a,first)['terminal_metadata']['clipboard_bytes']==len('newest clipboard')
        stop_worker(a,instance)
        print(json.dumps(dict(result='PASS',ssh=args.ssh,managed=args.service,unlinked_owner_can_launch=args.service,metadata_only_delta=True,clipboard_acknowledgement=True,no_payload_after_ack=True,watch_epoch_baseline=True,stale_ack_rejected=True,late_attach_suppressed=True,independent_slow_view=True,read_only_snapshot_has_no_copy=True)))

    finally:
        for client in clients:
            try:client.close()
            except (OSError,subprocess.TimeoutExpired):
                if client.process and client.process.poll() is None:client.process.kill();client.process.wait(timeout=3)
        if worker_fd is not None:
            if stamp(worker_pid)==worker_start:signal.pidfd_send_signal(worker_fd,signal.SIGTERM)
            os.close(worker_fd)
        for reference in references+extra_references:
            if stamp(reference['pid'])==reference['start_ticks']:
                try:
                    fd=os.pidfd_open(reference['pid']);signal.pidfd_send_signal(fd,signal.SIGTERM);os.close(fd)
                except ProcessLookupError:pass
        wait(lambda:all(stamp(ref['pid'])!=ref['start_ticks'] for ref in references+extra_references),'private supervised sources did not retire')
        if worker and worker.poll() is None:worker.wait(timeout=5)
        if daemon:daemon.terminate();daemon.wait(timeout=5)
