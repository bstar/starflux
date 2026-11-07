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
        if args.service:assert not (root/'MUST_NOT_USE_BUNDLED_PYTHON').exists(),'native source used bundled Python instead of host PATH'
        if args.service:
            def slot(command,project='Fleet',pane=0,epoch=None,payload=None,ok=True):
                argv=[str(exe),command,str(data),project,str(pane)]
                if epoch is not None:argv.append(epoch)
                result=run(argv,input=json.dumps(payload).encode() if payload is not None else None,capture_output=True,timeout=15)
                assert (result.returncode==0)==ok,result.stderr.decode(errors='replace')
                return json.loads(result.stdout) if ok else result
            assert slot('--slot-read') is None
            epoch=str(uuid.uuid4());launch={**launches[0],'id':str(uuid.uuid4()),'epoch':str(uuid.uuid4()),'argv':['python3','-u',str(script),str(root/'slot-launches'),'literal ; $(touch NEVER_EXECUTE) café']}
            def create(_):return slot('--slot-open',epoch=epoch,payload=dict(launch=launch,expected=None))
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:records=list(pool.map(create,range(2)))
            record=records[0];assert records[1]==record,'concurrent slot open created different sources'
            reference=record['source'];extra_references.append(reference)
            wait(lambda:'BROKER_SOURCE_READY' in text(cells(a,reference)),'slot source failed to start')
            assert (root/'slot-launches').read_text().count('START')==1
            assert slot('--slot-open',epoch=epoch,payload=dict(launch=launch,expected=None))==record
            attached=run([str(worker_exe),'--native-source-attach',str(data),json.dumps(record['owner'])],input=(json.dumps(envelope(dict(op='inspect',source=reference),707))+'\n').encode(),capture_output=True,timeout=15)
            assert attached.returncode==0 and json.loads(attached.stdout)['result']['source']==reference,'existing-owner stdio connector failed'
            slot('--slot-open',epoch=epoch,payload=dict(launch={**launch,'argv':['/must-not-replay-a-different-slot-command']},expected=record),ok=False)
            slot('--slot-reattach',epoch=str(uuid.uuid4()),ok=False)
            record_path=data/'native-sources/panes/Fleet/p0.json'
            wrong_intent={**record,'source':None,'launch_digest':'0'*64};record_path.write_text(json.dumps(wrong_intent))
            slot('--slot-reattach',epoch=epoch,ok=False)
            pending={**record,'source':None};record_path.write_text(json.dumps(pending))
            assert slot('--slot-reattach',epoch=epoch)==record,'uncertain source was not recovered read-only'
            assert (root/'slot-launches').read_text().count('START')==1
            # A runtime wipe leaves the same durable owner/socket/source intact.
            shutil.rmtree(private_runtime);private_runtime.mkdir(mode=0o700)
            assert slot('--slot-reattach',epoch=epoch)==record
            assert stamp(reference['pid'])==reference['start_ticks'] and stamp(worker_pid)==worker_start
            old_boot={**record,'owner':{**record['owner'],'boot':str(uuid.uuid4())}}
            record_path.write_text(json.dumps(old_boot))
            slot('--slot-reattach',epoch=epoch,ok=False)
            record_path.write_text(json.dumps(record))
            assert stamp(worker_pid)==worker_start,'stale owner caused a service restart'
            failed_epoch=str(uuid.uuid4());failed_launch={**launch,'id':str(uuid.uuid4()),'epoch':str(uuid.uuid4()),'argv':['/does-not-exist-private-slot-proof']}
            slot('--slot-open',pane=1,epoch=failed_epoch,payload=dict(launch=failed_launch,expected=None),ok=False)
            failed_record=slot('--slot-read',pane=1);assert failed_record['source'] is None
            slot('--slot-reattach',pane=1,epoch=failed_epoch,ok=False)
            next_epoch=str(uuid.uuid4());fresh_slot={**launch,'id':str(uuid.uuid4()),'epoch':str(uuid.uuid4())}
            recovered=slot('--slot-open',pane=1,epoch=next_epoch,payload=dict(launch=fresh_slot,expected=failed_record));extra_references.append(recovered['source'])
            wait(lambda:'BROKER_SOURCE_READY' in text(cells(a,recovered['source'])),'explicit fresh slot recovery failed')
            assert (root/'slot-launches').read_text().count('START')==2
            slot('--slot-open',pane=1,epoch=str(uuid.uuid4()),payload=dict(launch={**fresh_slot,'id':str(uuid.uuid4())},expected=failed_record),ok=False)
            assert slot('--slot-read',pane=1)==recovered,'stale slot writer replaced a recovered source'
            # Future descriptors are preserved; an older client cannot downgrade.
            owner_path=data/'native-sources/owner.json';owner_record=json.loads(owner_path.read_text())
            try:
                owner_path.write_text(json.dumps({**owner_record,'version':2}))
                result=run([str(exe),'--discover',str(data)],capture_output=True,timeout=15);assert result.returncode!=0
                assert json.loads(owner_path.read_text())['version']==2 and stamp(worker_pid)==worker_start
            finally:owner_path.write_text(json.dumps(owner_record))
            assert loaded_unlinked,'loaded executable upgrade was not exercised'
            # Existing-only attachment never starts a replacement after loss.
            retired_record=record
        custom={**palette,'ansi':[[1,2,3] for _ in range(16)]}
        custom_frame=a.call(dict(op='cells',source=first,size=size,palette=custom))['frame']
        assert any('ANSI_COLOR' in r['text'] and r['foreground']==[1,2,3] for row in custom_frame['rows'] for r in row['runs']),'view palette lost indexed color'
        if args.streams:
            w=Client();v=Client();view=str(uuid.uuid4());other=str(uuid.uuid4())
            def watch(client,identity,epoch,**kw):
                return client.call(dict(op='watch',source=first,view=identity,epoch=epoch,generation=7,focused=False,size=size,palette=palette,**kw))
            watch(w,view,1);watch(v,other,1)
            initial=w.event();second_view=v.event()
            assert initial['event']=='cells' and initial['epoch']==1 and initial['sequence']==1
            assert initial['frame']['stream']==view and initial['frame']['generation']==7
            # Leave both initial frames unacknowledged while ordinary input,
            # source-owned graphics queries and independent RPC renders continue.
            for i in range(6):send(a,first,('PING STREAM'+str(i)+'\n').encode())
            wait(lambda:'PONG STREAM5' in text(cells(a,first)),'slow view blocked terminal input')
            w.no_event();v.no_event()
            assert not w.call(dict(op='ack',view=view,epoch=1,sequence=999))['accepted']
            assert not w.call(dict(op='ack',view=view,epoch=2,sequence=1))['accepted']
            assert w.call(dict(op='ack',view=view,epoch=1,sequence=1))['accepted']
            latest=w.event();assert latest['sequence']==2 and latest['base_sequence']==1 and not latest['frame']['complete']
            merged={row['index']:row for row in initial['frame']['rows']};merged.update({row['index']:row for row in latest['frame']['rows']})
            assert 'PONG STREAM5' in text(dict(rows=[merged[i] for i in sorted(merged)]))
            assert [merged[i] for i in sorted(merged)]==cells(a,first)['rows'],'row delta did not reconstruct current frame'
            assert not w.call(dict(op='ack',view=view,epoch=1,sequence=1))['accepted']
            send(a,first,b'DEFER\n');wait(lambda:'DEFERRED_IMAGE_READY' in text(cells(a,first)),'slow view blocked source query replies')
            w.no_event();v.no_event()
            w.call(dict(op='watch',source=first,view=view,epoch=2,generation=8,focused=True,size=dict(rows=20,columns=60,cell=[12,24]),palette=custom))
            updated=w.event();assert updated['epoch']==2 and updated['sequence']==1 and updated['frame']['generation']==8
            assert not w.call(dict(op='unwatch',view=view,epoch=1))['accepted']
            assert not w.call(dict(op='ack',view=view,epoch=1,sequence=2))['accepted']
            assert info(a,first)['size']==size,'watch resized the retained PTY'
            w.call(dict(op='watch',source=first,view=view,epoch=1,generation=7,focused=False,size=size,palette=palette),ok=False)
            assert w.call(dict(op='unwatch',view=view,epoch=2))['accepted']
            w.call(dict(op='watch',source=first,view=view,epoch=2,generation=7,focused=False,size=size,palette=palette),ok=False)
            w.call(dict(op='watch',source=first,view=view,epoch=3,generation=9,focused=True,size=size,palette=palette))
            w.call(dict(op='watch',source=second,view=view,epoch=4,generation=10,focused=True,size=size,palette=palette))
            while True:
                switched=w.event()
                if switched['epoch']==4:break
                assert switched['epoch']==3
            assert switched['source']==second and switched['frame']['complete'],'source switch reused an old frame'
            assert not w.call(dict(op='ack',view=view,epoch=3,sequence=1))['accepted']
            assert w.call(dict(op='unwatch',view=view,epoch=4))['accepted']
            w.no_event()
            assert v.call(dict(op='unwatch',view=other,epoch=1))['accepted']
            w.close();v.close()
            half=run(connector(),input=(json.dumps(envelope(dict(op='cells',source=first,size=size,palette=palette),101))+'\n').encode(),capture_output=True,timeout=12)
            assert half.returncode==0 and json.loads(half.stdout)['result']['source']==first,'half-closed async cells lost its response'
            assert a.call(dict(op='hello'))['watches']==0
            wait(lambda:a.call(dict(op='hello'))['presentation_usage']==0,'render snapshots leaked')
            send(a,first,b'RESET\n');wait(lambda:info(a,first)['graphics_usage']==[0,0,0] and 'RESET_DONE' in text(cells(a,first)),'reset between independent scenarios failed')
        a.call(dict(op='open',launch={**launches[0],'argv':['/not-the-original-command']}),ok=False)
        send(a,first,b'IMAGE\n');wait(lambda:info(a,first)['graphics_usage'][1:]==[1,1],'source did not retain its image')
        send(a,first,b'DEFER\n');a.close();time.sleep(.4)
        b=Client();assert b.call(dict(op='open',launch=launches[0]))['source']==first
        wait(lambda:'DEFERRED_IMAGE_READY' in text(cells(b,first)),'source output/graphics stopped without a view')
        assert (root/'launch-0').read_text().count('START')==1 and not (root/'NEVER_EXECUTE').exists()
        for reference in references:assert stamp(reference['pid'])==reference['start_ticks'],(info(b,reference),text(cells(b,reference)))
        assert info(b,first)['graphics_usage'][0]==8
        frame=cells(b,first);assert any(r['background'] in ([255,0,0],[0,0,255]) for row in frame['rows'] for r in row['runs']),'retained image not represented in cells'
        c=Client();before=info(c,first)
        cells(c,first,dict(rows=20,columns=60,cell=[12,24]));assert info(c,first)['size']==before['size'],'read-only view resized the source'
        stale={**first,'epoch':str(uuid.uuid4())};b.call(dict(op='close',source=stale),ok=False)
        send_stale={**first,'boot':str(uuid.uuid4())};b.call(dict(op='input',source=send_stale,data=base64.b64encode(b'MUST_NOT_REACH_SOURCE\n').decode()),ok=False)
        assert stamp(first['pid'])==first['start_ticks']
        b.call(dict(op='resize',source=second,size=dict(rows=30,columns=100,cell=[8,16])))
        send(c,second,b'SIZE\n');wait(lambda:'SIZE=30x100' in text(cells(c,second)),'kernel PTY resize missing')
        n=1024*1024-64;send(b,second,('BIG '+str(n)+'\n').encode()+b'x'*n)
        wait(lambda:'BIG_OK='+str(n) in text(cells(c,second)),'bounded large input did not reach source',timeout=20)
        send(b,first,b'PING\n');wait(lambda:'PONG' in text(cells(b,first)),'source lost input after reattachment')
        assert stamp(worker_pid)==worker_start,'broker restarted when views disconnected'
        if args.streams:
            closing_view=Client();closing_id=str(uuid.uuid4());closing_view.call(dict(op='watch',source=second,view=closing_id,epoch=1,generation=1,focused=True,size=size,palette=palette));closing_initial=closing_view.event()
            b.call(dict(op='resize',source=second,size=dict(rows=10,columns=40,cell=[8,16])))
            closing_view.call(dict(op='ack',view=closing_id,epoch=1,sequence=closing_initial['sequence']))
            error=closing_view.event();assert error['event']=='view_error' and error['epoch']==1
            closing_view.no_event()
            closing_view.call(dict(op='watch',source=second,view=closing_id,epoch=2,generation=2,focused=True,size=dict(rows=10,columns=40,cell=[8,16]),palette=palette))
            resized=closing_view.event();assert resized['frame']['complete'] and resized['frame']['rect']['width']==40
            b.call(dict(op='resize',source=second,size=dict(rows=30,columns=100,cell=[8,16])))
        send(c,second,b'HANG\n');wait(lambda:'HANGING' in text(cells(c,second)),'close escalation fixture missing')
        c.close();assert b.call(dict(op='close',source=second))['closing']
        if args.streams:
            closed=closing_view.event();assert closed['event']=='source_closed' and closed['source']==second and closed['epoch']==2
            closing_view.close()
        send(b,first,b'PING CLOSE\n');wait(lambda:'PONG CLOSE' in text(cells(b,first)),'closing a hung source blocked another source')
        wait(lambda:stamp(second['pid'])!=second['start_ticks'],'explicit source close failed')
        assert stamp(first['pid'])==first['start_ticks'],'closing another source killed this one'
        stop_worker(b,instance);wait(lambda:stamp(first['pid'])!=first['start_ticks'],'private source survived explicit broker shutdown')
        if args.service:
            slot('--slot-reattach',epoch=retired_record['terminal_epoch'],ok=False)
            # There must still be no live owner before explicit new discovery.
            assert stamp(worker_pid)!=worker_start
        start_worker();d=Client();hello=d.call(dict(op='hello'));assert hello['instance']!=instance
        if args.service:slot('--slot-reattach',epoch=retired_record['terminal_epoch'],ok=False)
        d.call(dict(op='open',launch=launches[0]),ok=False);assert d.call(dict(op='list'))==[]
        assert (root/'launch-0').read_text().count('START')==1,'broker restart replayed consumed launch'
        failed={**launches[0],'id':str(uuid.uuid4()),'argv':['/does-not-exist-private-broker-proof']};d.call(dict(op='open',launch=failed),ok=False)
        d.call(dict(op='open',launch={**failed,'argv':launches[0]['argv']}),ok=False)
        fresh={**launches[0],'id':str(uuid.uuid4()),'epoch':str(uuid.uuid4())}
        launched=run(connector(),input=(json.dumps(envelope(dict(op='open',launch=fresh),100))+'\n').encode(),capture_output=True,timeout=12)
        assert launched.returncode==0 and json.loads(launched.stdout)['ok'],'half-closed async source launch lost its response'
        new=json.loads(launched.stdout)['result']['source'];references.append(new)
        assert d.call(dict(op='open',launch=fresh))['source']==new
        wait(lambda:'BROKER_SOURCE_READY' in text(cells(d,new)),'explicit fresh source failed');assert (root/'launch-0').read_text().count('START')==2
        if args.streams:
            exit_view=Client();exit_id=str(uuid.uuid4());exit_view.call(dict(op='watch',source=new,view=exit_id,epoch=1,generation=1,focused=True,size=size,palette=palette));ready=exit_view.event();exit_view.call(dict(op='ack',view=exit_id,epoch=1,sequence=ready['sequence']))
        send(d,new,b'EXIT\n');wait(lambda:info(d,new)['exited'],'natural source exit missing')
        if args.streams:
            for _ in range(4):
                ended=exit_view.event()
                if ended['exited']:break
                exit_view.call(dict(op='ack',view=exit_id,epoch=1,sequence=ended['sequence']))
            assert ended['exited'] and not ended['input_available'],'stream did not publish natural exit'
            exit_view.close()
        assert d.call(dict(op='open',launch=fresh))['exited'] and (root/'launch-0').read_text().count('START')==2,'exited source restarted implicitly'
        stop_worker(d,hello['instance'])
        print(json.dumps(dict(result='PASS',ssh=args.ssh,retained_native_ptys=True,detached_launcher=args.ssh,view_detach_preserves_sources=True,reattach_same_pid=True,no_command_replay=True,literal_argv=True,independent_sources=True,independent_cells_views=True,source_geometry_preserved=True,kernel_resize=True,large_input=True,image_retained_without_views=True,cells_fallback=True,source_queries_owned=True,stale_close_rejected=True,receipt_survives_broker_restart=True,half_closed_launch=True,view_palettes=True,nonblocking_close=True,natural_exit_no_replay=True,failed_launch_not_replayed=True,explicit_fresh_launch=True,no_real_agents=True,streamed_frames=args.streams,slow_view_credit=args.streams,stale_view_epochs=args.streams,half_closed_cells=args.streams,row_deltas=args.streams,source_switch_fences=args.streams,source_closed_events=args.streams,natural_exit_frames=args.streams,resize_error_recovery=args.streams,managed_service=args.service,deep_durable_socket=args.service,concurrent_owner_discovery=args.service,concurrent_slot_open=args.service,durable_pane_references=args.service,uncertain_open_recovery=args.service,runtime_loss_preserves_sources=args.service,loaded_owner_upgrade=args.service,no_service_downgrade=args.service,explicit_slot_recovery=args.service,packaged_host_path=args.service)))
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
                    fd=os.pidfd_open(reference['pid']);signal.pidfd_send_signal(fd,signal.SIGKILL);os.close(fd)
                except ProcessLookupError:pass
        if worker and worker.poll() is None:worker.wait(timeout=5)
        if daemon:daemon.terminate();daemon.wait(timeout=5)
