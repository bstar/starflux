#!/usr/bin/env python3
"""STAR/FLUX live PTY ownership receiver and tmux proxy (Herdr handoff v1).

No agent process is relaunched. Before commit, failure closes only duplicated
masters and staged proxies; Herdr still owns its original descriptors.
"""
import array
import base64
import collections
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import termios
import time
import tty

LIMIT=16*1024*1024
QUEUE=2*1024*1024

def atomic(path,data):
    path=Path(path);path.parent.mkdir(parents=True,exist_ok=True)
    data=data if isinstance(data,bytes) else json.dumps(data,ensure_ascii=False,indent=2).encode()
    with tempfile.NamedTemporaryFile(dir=path.parent,delete=False) as f:
        f.write(data);f.flush();os.fsync(f.fileno());temp=Path(f.name)
    temp.chmod(0o600);temp.replace(path)

def line(sock):
    data=bytearray()
    while len(data)<=LIMIT:
        byte=sock.recv(1)
        if not byte:raise EOFError('Handoff closed before commit')
        if byte==b'\n':return bytes(data)
        data+=byte
    raise ValueError('Handoff manifest exceeds limit')

def process_alive(pid):
    try:return Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()[0]!='Z'
    except OSError:return False

def resize(fd,rows,cols):
    if not 1<=rows<=512 or not 2<=cols<=1024:raise ValueError('Invalid terminal dimensions')
    fcntl.ioctl(fd,termios.TIOCSWINSZ,struct.pack('HHHH',rows,cols,0,0))

def mode_ansi(state):
    value=state.get('input_state') or {}
    result=b'\x1b[0m\x1b[?25h'
    for key,mode in [('alternate_screen',1049),('application_cursor',1),('bracketed_paste',2004),('focus_reporting',1004)]:
        result+=f'\x1b[?{mode}{"h" if value.get(key) else "l"}'.encode()
    mode=value.get('mouse_protocol_mode','None').lower()
    for key,code in [('press',1000),('button',1002),('any',1003)]:
        result+=f'\x1b[?{code}{"h" if key in mode else "l"}'.encode()
    if 'sgr' in value.get('mouse_protocol_encoding','').lower():result+=b'\x1b[?1006h'
    result+=(state.get('keyboard_protocol_ansi') or '').encode()
    return result

class Broker:
    def __init__(self,plan,manifest,fds):
        self.plan=plan;self.run=Path(plan['run']);self.sel=selectors.DefaultSelector();self.clients={};self.terms={};self.committed=False
        self.listener=socket.socket(socket.AF_UNIX);self.listener.bind(plan['broker']);os.chmod(plan['broker'],0o600)
        self.listener.listen(64);self.listener.setblocking(False);self.sel.register(self.listener,selectors.EVENT_READ,('listen',None))
        by_pid={p['pid']:p for p in plan['panes']}
        for state,fd in zip(manifest['panes'],fds):
            p=by_pid[state['child_pid']];os.set_inheritable(fd,False);os.set_blocking(fd,False)
            self.terms[p['key']]=dict(plan=p,state=state,fd=fd,peer=None,pending=bytearray(),eof=False,registered=False,
                bootstrap=mode_ansi(state)+(state.get('initial_history_ansi') or '').encode()+base64.b64decode(p['screen']),sequence=0,hash=hashlib.sha256(),inspect_at=0,health={})
    def watch(self,key):
        t=self.terms[key]
        if t['eof']:return
        # Stop reading the original PTY before its proxy queue fills. The
        # kernel then backpressures the application without dropping its bridge.
        peer=t['peer']
        readable=peer in self.clients and len(self.clients[peer]['out'])<QUEUE//2
        events=(selectors.EVENT_READ if self.committed and readable else 0)|(selectors.EVENT_WRITE if self.committed and t['pending'] else 0)
        if t['registered']:
            if events:self.sel.modify(t['fd'],events,('pty',key))
            else:self.sel.unregister(t['fd']);t['registered']=False
        elif events:self.sel.register(t['fd'],events,('pty',key));t['registered']=True
    def disconnect(self,peer):
        c=self.clients.pop(peer,None)
        try:self.sel.unregister(peer)
        except (KeyError,ValueError):pass
        peer.close()
        if c and c['key'] and self.terms[c['key']]['peer'] is peer:
            t=self.terms[c['key']];t['peer']=None;self.watch(c['key'])
    def output(self,peer,data,close=False):
        c=self.clients.get(peer)
        if c is None:return
        if len(c['out'])+len(data)>QUEUE:self.disconnect(peer);return
        c['out'].extend(data);c['close']|=close
        self.sel.modify(peer,selectors.EVENT_READ|selectors.EVENT_WRITE,('client',peer))
    def health(self,key):
        t=self.terms[key];now=time.monotonic()
        if now-t['inspect_at']<.5:return t['health']
        p=t['plan'];pid=p['pid'];agent=None;agent_pid=None;command='shell';alive=not t['eof'] and process_alive(pid)
        if alive:
            command=Path('/proc',str(pid),'comm').read_text().strip()
            try:
                foreground=int(Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()[5]);pending=[pid];seen=set()
                while pending and len(seen)<128:
                    child=pending.pop()
                    if child in seen:continue
                    seen.add(child);root=Path('/proc',str(child))
                    try:
                        st=(root/'stat').read_text().rsplit(')',1)[1].split()
                        if int(st[2])==foreground:
                            args=[s.decode(errors='replace') for s in (root/'cmdline').read_bytes().split(b'\0') if s]
                            families={'codex':'codex','claude':'claude','opencode':'opencode','aider':'aider','gemini':'gemini'}
                            for arg in args[:2]:
                                family=families.get(Path(arg).name.lstrip('.').removesuffix('-wrapped').removesuffix('.js'))
                                if family:agent=family;agent_pid=child;break
                        pending.extend(map(int,(root/'task'/str(child)/'children').read_text().split()))
                    except (OSError,ValueError,IndexError):continue
                    if agent:break
            except (OSError,ValueError,IndexError):pass
        status='unknown'
        if agent:
            status='idle'
            try:
                captured=subprocess.run(['tmux','-L',p['server'],'capture-pane','-p','-t',p['target']],capture_output=True,timeout=.5).stdout.decode(errors='replace')
                footer='\n'.join(captured.splitlines()[-32:]).lower()
                for line in reversed(footer.splitlines()):
                    line=line.lstrip(' \t│┃|●*•')
                    if line.startswith('worked for '):break
                    if re.match(r'^(?:working|thinking|running|executing)\s+\([0-9][hms0-9. ·•]*esc to interrupt\)',line):status='working';break
                choice=r'(?m)^\s*[│┃|>›❯●* ]*(?:\d+[.)]|[yn]\))\s*'
                if re.search(choice+r'(?:yes\b|allow once\b)',footer) and re.search(choice+r"(?:no\b|don't allow\b|deny this\b)",footer) and ('esc' in footer or 'enter' in footer):status='waiting_for_approval'
                if re.search(r'(?m)^\s*[│┃|>›❯●* ]*(?:question[ :]|user input requested)',footer) and re.search(choice,footer) and any(v in footer for v in ['enter to submit','enter to confirm','enter to select']):status='needs_input'
            except (OSError,subprocess.TimeoutExpired):pass
        t['health']=dict(alive=alive,exited=not alive,pid=pid,command=agent or command,agent=agent,agent_pid=agent_pid,state=status,source='starflux-adopted',output_fingerprint=t['hash'].hexdigest())
        t['inspect_at']=now;return t['health']
    def request(self,peer,request):
        c=self.clients[peer];key=request.get('key');op=request.get('op')
        if c['key']:
            t=self.terms[c['key']]
            if op=='input':
                data=base64.b64decode(request['data'],validate=True)
                if len(t['pending'])+len(data)>QUEUE:raise ValueError('PTY input queue full')
                t['pending'].extend(data);self.watch(c['key'])
            elif op=='resize':
                if self.committed:resize(t['fd'],int(request['rows']),int(request['cols']))
                else:t['size']=(int(request['rows']),int(request['cols']))
            else:raise ValueError('Unknown terminal operation')
        elif op=='attach' and key in self.terms:
            t=self.terms[key]
            if t['peer'] is not None or t['eof']:raise ValueError('Terminal proxy already attached or exited')
            c['key']=key;t['peer']=peer
            if self.committed:
                self.output(peer,t['bootstrap']);t['bootstrap']=b''
                resize(t['fd'],int(request['rows']),int(request['cols']));self.watch(key)
            else:t['size']=(int(request['rows']),int(request['cols']))
        elif op=='inspect' and key in self.terms:
            self.output(peer,json.dumps(self.health(key)).encode()+b'\n',close=True)
        elif op=='status':
            self.output(peer,json.dumps(dict(pid=os.getpid(),committed=self.committed,panes=len(self.terms),alive=sum(not t['eof'] for t in self.terms.values()))).encode()+b'\n',close=True)
        else:raise ValueError('Invalid broker request')
    def step(self,timeout=.25):
        for event,mask in self.sel.select(timeout):
            kind,key=event.data
            if kind=='listen':
                peer,_=self.listener.accept();peer.setblocking(False)
                self.clients[peer]=dict(key=None,incoming=bytearray(),out=bytearray(),close=False)
                self.sel.register(peer,selectors.EVENT_READ,('client',peer))
            elif kind=='client':
                peer=key;c=self.clients.get(peer)
                if not c:continue
                try:
                    if mask&selectors.EVENT_READ:
                        data=peer.recv(65536)
                        if not data:self.disconnect(peer);continue
                        c['incoming'].extend(data)
                        if len(c['incoming'])>QUEUE:raise ValueError('Proxy request too large')
                        while b'\n' in c['incoming']:
                            text,_,rest=c['incoming'].partition(b'\n');c['incoming']=bytearray(rest)
                            self.request(peer,json.loads(text))
                    if mask&selectors.EVENT_WRITE and c['out']:
                        count=peer.send(c['out']);del c['out'][:count]
                        if c['key']:self.watch(c['key'])
                    if not c['out']:
                        if c['close']:self.disconnect(peer)
                        elif peer in self.clients:self.sel.modify(peer,selectors.EVENT_READ,('client',peer))
                except (OSError,ValueError,KeyError):self.disconnect(peer)
            else:
                t=self.terms[key]
                if mask&selectors.EVENT_READ:
                    try:data=os.read(t['fd'],65536)
                    except BlockingIOError:continue
                    except OSError as exc:
                        if exc.errno!=5:raise
                        data=b''
                    if not data:
                        self.sel.unregister(t['fd']);t['registered']=False;t['eof']=True
                        os.close(t['fd'])
                        if t['peer']:
                            self.clients[t['peer']]['close']=True
                            if not self.clients[t['peer']]['out']:self.disconnect(t['peer'])
                    else:
                        t['sequence']+=1;t['hash'].update(data)
                        if t['peer']:self.output(t['peer'],data)
                        self.watch(key)
                if mask&selectors.EVENT_WRITE and not t['eof'] and t['pending']:
                    try:count=os.write(t['fd'],t['pending']);del t['pending'][:count]
                    except BlockingIOError:pass
                    self.watch(key)
    def own(self):
        self.committed=True
        for key,t in self.terms.items():
            if t['peer']:
                self.output(t['peer'],t['bootstrap']);t['bootstrap']=b''
                if 'size' in t:resize(t['fd'],*t.pop('size'))
            self.watch(key)

def receive(plan_path,socket_path,token):
    plan=json.loads(Path(plan_path).read_text());run=Path(plan['run']);log=(run/'broker.log').open('a',buffering=1)
    os.dup2(log.fileno(),2)
    staged=[];fds=[];saved_flags={};committed=False
    try:
        stream=socket.socket(socket.AF_UNIX);stream.settimeout(25);stream.connect(socket_path);stream.sendall(token.encode()+b'\n')
        manifest=json.loads(line(stream))
        if manifest['version']!=1 or manifest['source_protocol']!=22:raise ValueError('Unsupported handoff protocol')
        if {p['child_pid'] for p in manifest['panes']}!={p['pid'] for p in plan['panes']} or len(manifest['panes'])!=len(plan['panes']):raise ValueError('Live terminal inventory changed; source retains ownership')
        atomic(run/'handoff.json',manifest)
        stream.sendall(b'validated\n')
        while len(fds)<len(plan['panes']):
            data,ancillary,flags,_=stream.recvmsg(1,socket.CMSG_SPACE(64*array.array('i').itemsize))
            if data!=b'F' or flags&socket.MSG_CTRUNC:raise ValueError('Invalid descriptor batch')
            for level,kind,value in ancillary:
                if level==socket.SOL_SOCKET and kind==socket.SCM_RIGHTS:
                    values=array.array('i');values.frombytes(value[:len(value)-len(value)%values.itemsize]);fds.extend(values)
        if len(fds)!=len(plan['panes']):raise ValueError('Unexpected descriptor count')
        saved_flags={fd:fcntl.fcntl(fd,fcntl.F_GETFL) for fd in fds}
        broker=Broker(plan,manifest,fds)
        env={**os.environ,**plan['environment'],'SHELL':'/bin/sh'}
        for p in plan['panes']:
            state=next(v for v in manifest['panes'] if v['child_pid']==p['pid'])
            argv=[plan['proxy_executable'],plan['broker'],p['key']]
            result=subprocess.run(['tmux','-L',p['server'],'-f','/dev/null','new-session','-d','-s',p['session'],'-x',str(state['cols']),'-y',str(state['rows']),'-c',p['cwd'],' '.join(__import__('shlex').quote(s) for s in argv)],env=env,capture_output=True)
            if result.returncode:raise RuntimeError('Could not stage tmux proxy: '+result.stderr.decode(errors='replace'))
            staged.append(p)
            for option,value in [('status','off'),('mouse','on'),('history-limit','20000'),('remain-on-exit','on'),('set-clipboard','on'),('extended-keys','on'),('allow-passthrough','on')]:
                subprocess.run(['tmux','-L',p['server'],'set-option','-g',option,value],env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            subprocess.run(['tmux','-L',p['server'],'set-option','-w','-t',p['target'],'window-size','latest'],env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        deadline=time.monotonic()+15
        while any(t['peer'] is None for t in broker.terms.values()):
            if time.monotonic()>deadline:
                for p in staged:
                    diagnostic=subprocess.run(['tmux','-L',p['server'],'capture-pane','-p','-t',p['target']],capture_output=True)
                    print('Proxy startup:',p['key'],diagnostic.stdout.decode(errors='replace')[-1000:],file=log)
                raise RuntimeError('Staged terminal proxies did not attach')
            broker.step(.02)
        atomic(run/'status.json',dict(phase='prepared',pid=os.getpid(),panes=len(fds)))
        stream.sendall(b'restored\nready\n')
        if line(stream)!=b'committed':raise ValueError('Source did not commit')
        committed=True
        atomic(run/'status.json',dict(phase='committing',pid=os.getpid(),panes=len(fds)))
        # From here onward, any error must leave this process and the PTYs alive.
        while True:
            try:
                for document in plan['documents']:atomic(document['path'],document['value'])
                atomic(Path(plan['project_root'])/'catalog-version',os.urandom(16))
                break
            except OSError as error:
                print('Committed PTYs retained; retrying metadata publication:',error,file=log);time.sleep(.1)
        broker.own()
        try:stream.sendall(b'owned\n')
        except OSError:pass
        stream.close()
        atomic(run/'status.json',dict(phase='active',pid=os.getpid(),panes=len(fds),original_pids=[p['pid'] for p in plan['panes']]))
        for path in plan.get('gates',[]):Path(path).unlink(missing_ok=True)
        while True:
            try:broker.step()
            except Exception as error:
                # A malformed peer must never tear down live terminal ownership.
                print('Broker event error:',repr(error),file=log);time.sleep(.02)
    except BaseException as error:
        try:print('Handoff receiver failed:',repr(error),file=log)
        except OSError:pass
        if committed:
            # Preserve all masters for diagnosis rather than losing user processes.
            try:atomic(run/'status.json',dict(phase='retained-error',pid=os.getpid(),error=str(error)))
            except OSError:pass
            while True:time.sleep(1)
        for p in staged:
            subprocess.run(['tmux','-L',p['server'],'kill-session','-t',p['session']],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        for fd in fds:
            try:
                if fd in saved_flags:fcntl.fcntl(fd,fcntl.F_SETFL,saved_flags[fd])
                os.close(fd)
            except OSError:pass
        Path(plan['broker']).unlink(missing_ok=True)
        atomic(run/'status.json',dict(phase='failed',error=str(error)))
        raise

def proxy(path,key):
    fd=sys.stdin.fileno();saved=termios.tcgetattr(fd);tty.setraw(fd)
    peer=socket.socket(socket.AF_UNIX);peer.connect(path)
    rows,cols=struct.unpack('HHHH',fcntl.ioctl(fd,termios.TIOCGWINSZ,b'\0'*8))[:2]
    def send(value):peer.sendall(json.dumps(value).encode()+b'\n')
    send(dict(op='attach',key=key,rows=rows,cols=cols))
    changed=[False]
    signal.signal(signal.SIGWINCH,lambda *_:changed.__setitem__(0,True))
    sel=selectors.DefaultSelector();sel.register(fd,selectors.EVENT_READ);sel.register(peer,selectors.EVENT_READ)
    try:
        while True:
            if changed[0]:
                changed[0]=False;rows,cols=struct.unpack('HHHH',fcntl.ioctl(fd,termios.TIOCGWINSZ,b'\0'*8))[:2]
                send(dict(op='resize',rows=rows,cols=cols))
            for event,_ in sel.select(.2):
                if event.fileobj is peer:
                    data=peer.recv(65536)
                    if not data:return
                    view=memoryview(data)
                    while view:written=os.write(sys.stdout.fileno(),view);view=view[written:]
                else:
                    data=os.read(fd,65536)
                    if not data:return
                    send(dict(op='input',data=base64.b64encode(data).decode()))
    finally:termios.tcsetattr(fd,termios.TCSANOW,saved);peer.close()

if __name__=='__main__':
    if sys.argv[1]=='receive':receive(*sys.argv[2:5])
    elif sys.argv[1]=='proxy':proxy(*sys.argv[2:4])
    else:raise SystemExit('Unknown PTY operation')
