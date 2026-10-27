#!/usr/bin/env python3
"""Headless SSH checks for staged files, raw image MIME data and browser image drops."""
import getpass,json,os,pathlib,shlex,socket,subprocess,tempfile,time,uuid
repo=pathlib.Path(__file__).resolve().parents[1];app=repo/'scripts/run-local.sh';runtime=pathlib.Path(os.environ['XDG_RUNTIME_DIR'])/'starmux'
out=repo/'target/image-drop-proof';out.mkdir(parents=True,exist_ok=True)
local='shelf-'+uuid.uuid4().hex[:8];remote='shelf-ssh-'+uuid.uuid4().hex[:8];names=[local,remote];children=[]
def wait(fn,label,timeout=35):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        try:
            result=fn()
            if result:return result
        except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
        time.sleep(.12)
    raise AssertionError(label)
def invoke(name,*args):return subprocess.check_output([str(app),'--workspace',name,*args],stderr=subprocess.PIPE,timeout=25)
def inspect():return json.loads((runtime/(local+'.state.json')).read_text())
def action(value):invoke(local,'--action',value)
with tempfile.TemporaryDirectory(prefix='starmux-shelf-') as tmp:
    tmp=pathlib.Path(tmp)
    try:
        for key in ['host','client']:subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(tmp/key)],check=True)
        (tmp/'authorized_keys').write_text((tmp/'client.pub').read_text())
        with socket.socket() as probe:probe.bind(('127.0.0.1',0));port=probe.getsockname()[1]
        (tmp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(tmp/'host.pub').read_text())
        (tmp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM yes\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
        sshconfig=tmp/'ssh_config';sshconfig.write_text(f'Host shelf-host\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {tmp}/client\n IdentitiesOnly yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {tmp}/known_hosts\n')
        daemon=subprocess.Popen(['/usr/bin/sshd','-D','-e','-f',str(tmp/'sshd_config')],stdout=subprocess.DEVNULL,stderr=(out/'sshd.log').open('w'),env={k:v for k,v in os.environ.items() if k!='NOTIFY_SOCKET'});children.append(daemon)
        wait(lambda:subprocess.run(['ssh','-F',str(sshconfig),'shelf-host','true'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0,'Private SSH unavailable')
        names=[local]
        import threading,base64,hashlib
        config={'workspace':local,'theme':'kanagawa','host':None,'directory':str(tmp),'agent':None,'executable':str(repo/'target/release/starmux'),'project_host':True,'desktop_helper':str(app)}
        p=runtime/(local+'.json');p.write_text(json.dumps(config));p.chmod(0o600)
        server=subprocess.Popen([str(app),'--serve',local],env={**os.environ,'STAR_TERMINAL_RENDERER':'direct'},stdout=subprocess.DEVNULL,stderr=(out/'server.log').open('w'));children.append(server)
        wait(lambda:(runtime/(local+'.sock')).exists(),'Workspace server missing')
        process=subprocess.Popen(['ssh','-F',str(sshconfig),'-T','shelf-host',shlex.join([str(app),'--graphical-relay',local])],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(out/'relay.log').open('w'));children.append(process)
        lock=threading.Lock();scene={};effects=[];acks={}
        def send(message):
            with lock:process.stdin.write((json.dumps(message)+'\n').encode());process.stdin.flush()
        def reader():
            for line in process.stdout:
                m=json.loads(line)
                if m['type']=='scene':
                    scene.update(m['scene']);send({'type':'presented','revision':scene['revision'],'generation':1})
                if m['type']=='osc72':
                    effects.append(m['meta']);send({'type':'effect_ack','id':m['id']})
                if m['type']=='ack':acks[m['id']]=m['accepted']
        threading.Thread(target=reader,daemon=True).start()
        send({'type':'hello','version':1,'client':'image-drop-proof','viewport':{'columns':100,'rows':40,'width':1200,'height':800,'generation':1}})
        wait(lambda:scene.get('revision'),'No scene received')
        serial=0
        def wire(text):
            global serial
            serial+=1
            send({'type':'input','id':serial,'revision':scene['revision'],'generation':1,'input':{'kind':'osc72','text':text}})
            wait(lambda:serial in acks,'Drop input was not acknowledged');assert acks[serial]
        wire('t=q')
        wait(lambda:any(x.startswith('t=a:x=1') for x in effects),'Host machine identity not advertised')
        import struct,zlib
        def chunk(t,v):return struct.pack('>I',len(v))+t+v+struct.pack('>I',zlib.crc32(t+v))
        image=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',32,32,8,6,0,0,0))+chunk(b'IDAT',zlib.compress((b'\0'+b'\x33\x99\xdd\xff'*32)*32))+chunk(b'IEND',b'')
        def delivered():return inspect() if inspect().get('status','').startswith('Copied to ') and inspect().get('status')!=old_status else None
        for kind in ['remote-file','image-mime','browser-image','remote-multi']:
            old_status=inspect().get('status')
            offered='text/uri-list' if kind in ['remote-file','remote-multi'] else ('text/uri-list image/png' if kind=='browser-image' else 'image/png')
            wire('t=m:x=40:y=20:o=1:i=1;'+offered);wire('t=M:x=40:y=20:o=1:i=1;'+offered)
            if kind in ['remote-file','remote-multi']:
                uri=base64.b64encode(b'file://mac/Users/proof/image%20over%20ssh.png\r\n'+(b'file://mac/Users/proof/second.png\r\n' if kind=='remote-multi' else b'')).decode()
                wire('t=r:x=1:X=1:m=1:i=1;'+uri);wire('t=r:x=1:X=1:m=0:i=1;')
                wait(lambda:'t=r:x=1:y=1:i=1' in effects,'Remote file contents were never requested')
                wire('t=r:x=1:y=1:X=0:m=0;'+base64.b64encode(image).decode())
                wire('t=r:x=1:y=1:m=0;')
                if kind=='remote-multi':
                    wait(lambda:'t=r:x=1:y=2:i=1' in effects,'Second image contents were never requested')
                    wire('t=r:x=1:y=2:X=0:m=1;'+base64.b64encode(image).decode())
                    wire('t=r:x=1:y=2:m=0;')
            elif kind=='browser-image':
                wire('t=r:x=1:m=1;'+base64.b64encode(b'https://example.invalid/image.png\r\n').decode());wire('t=r:x=1:m=0;')
                wait(lambda:'t=r:x=2:i=1' in effects,'Browser image fallback was not requested')
                wire('t=r:x=2:m=1;'+base64.b64encode(image).decode());wire('t=r:x=2:m=0;')
            else:
                wire('t=r:x=1:m=1;'+base64.b64encode(image).decode());wire('t=r:x=1:m=0;')
            state=wait(delivered,'Image was not delivered to terminal')
            path=state['status'].split(': ',1)[1].rsplit(' / press Enter',1)[0]
            paths=path.split(', ')
            assert len(paths)==(2 if kind=='remote-multi' else 1)
            assert all(pathlib.Path(p).read_bytes()==image for p in paths),'Staged image bytes changed'
            assert '/Users/proof/' not in path,'Mac-only path was pasted into remote terminal'
            print(kind,hashlib.sha256(pathlib.Path(paths[0]).read_bytes()).hexdigest(),flush=True)
            time.sleep(.7)
        (out/'result.json').write_text(json.dumps({'passed':True,'remote_uri_and_direct_image_mime':True,'browser_image_fallback':True,'multiple_files':True,'authenticated_ssh_transport':True,'bytes_preserved':True,'no_enter_sent':True},indent=2))
    except BaseException:
        try:(out/'failure-state.json').write_text(json.dumps(inspect(),indent=2))
        except Exception:pass

        raise
    finally:
        for name in names:
            try:invoke(name,'--stop')
            except Exception as error:print('Cleanup',name,str(error))
        for child in reversed(children):
            if child.poll() is None:child.terminate();child.wait(timeout=8)
