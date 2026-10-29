#!/usr/bin/env python3
"""Compare RPC key injection → fixture receipt → screen-captured update.

Uses disposable Kitty windows/sessions. Screen capture adds sampling overhead;
these are software-path comparisons, not hardware input-to-photon measurements.
"""
import json
import getpass
import shlex
import os
from pathlib import Path
import socket
import statistics
import subprocess
import tempfile
import time
import uuid

repo=Path(__file__).resolve().parents[1]
app=repo/'scripts/run-local.sh'
runtime=Path(os.environ['XDG_RUNTIME_DIR'])/'starflux'
phase=os.environ.get('STARFLUX_LATENCY_PHASE','before')
out=repo/'target/terminal-latency-proof';out.mkdir(parents=True,exist_ok=True)
base_name='latency-'+uuid.uuid4().hex[:8]
names=[]
original=json.loads(subprocess.check_output(['hyprctl','activewindow','-j'])).get('address')

def wait(fn,label,timeout=30,pause=.03):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
        time.sleep(pause)
    raise AssertionError(label)

def stats(values):
    values=sorted(values)
    return {'median':round(statistics.median(values),2),'p95':round(values[min(len(values)-1,int(len(values)*.95))],2),'min':round(values[0],2),'max':round(values[-1],2)}

with tempfile.TemporaryDirectory(prefix='starflux-latency-') as temp:
    temp=Path(temp);children=[];addresses=[];results={}
    fixture=temp/'codex'
    fixture.write_text('''import json,os,sys,time,tty,termios,subprocess
old=termios.tcgetattr(0);tty.setraw(0)
n=0
if os.environ.get('STARFLUX_BENCH_ACTIVITY')=='1' and os.environ.get('TMUX'):
 subprocess.run(['tmux','set-option','-p','@starflux_state','working'],check=True)
with open(sys.argv[1],'a') as log:
 def draw(color):
  patch='\\x1b[?25l\\x1b[48;2;%d;%d;%dm'%color
  patch+=''.join('\\x1b[%d;6H'%row+' '*12 for row in range(6,10))
  os.write(1,patch.encode())
 os.write(1,b'\\x1b[2;1HWorking (0s esc to interrupt)')
 draw((24,40,56))
 log.write(json.dumps({'ready':True,'pid':os.getpid()})+'\\n');log.flush()
 try:
  while True:
   chunk=os.read(0,1)
   if not chunk:break
   if chunk!=b'x':continue
   n+=1;received=time.monotonic_ns()
   color=(112,48,32) if n%2 else (32,48,112)
   draw(color);written=time.monotonic_ns()
   log.write(json.dumps({'n':n,'received':received,'written':written})+'\\n');log.flush()
 finally:termios.tcsetattr(0,termios.TCSANOW,old)
''')
    def records(path):return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
    def rc(address,*args):return subprocess.check_output(['kitten','@','--to',address,*args],stderr=subprocess.PIPE,timeout=10)
    def inject(address,text):
        packet={'cmd':'send-text','version':[0,48,2],'no_response':True,'payload':{'data':'text:'+text,'bracketed_paste':'disable'}}
        with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as sock:
            sock.connect(address.removeprefix('unix:'))
            sent=time.monotonic_ns()
            sock.sendall(b'\x1bP@kitty-cmd'+json.dumps(packet).encode()+b'\x1b\\')
        return sent
    width=int(os.environ.get('STARFLUX_LATENCY_WIDTH','1400'));height=int(os.environ.get('STARFLUX_LATENCY_HEIGHT','1000'))
    daemon=None
    ssh_config=None
    remote=None
    shell=temp/'quiet-shell';shell.write_text('#!/bin/sh\nif [ "$1" = "-c" ]; then exec /bin/sh "$@"; fi\nexec cat\n');shell.chmod(0o700)
    if any('ssh' in kind for kind in os.environ.get('STARFLUX_LATENCY_KINDS','').split(',')):
        for key in ['host','client']:
            subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(temp/key)],check=True)
        (temp/'authorized_keys').write_text((temp/'client.pub').read_text())
        with socket.socket() as probe:probe.bind(('127.0.0.1',0));port=probe.getsockname()[1]
        (temp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(temp/'host.pub').read_text())
        (temp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {temp}/host\nPidFile {temp}/sshd.pid\nAuthorizedKeysFile {temp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
        daemon=subprocess.Popen(['/usr/bin/sshd','-D','-e','-f',str(temp/'sshd_config')],stderr=(out/(phase+'-sshd.log')).open('w'),env={k:v for k,v in os.environ.items() if k!='NOTIFY_SOCKET'})
        ssh_config=temp/'ssh_config'
        ssh_config.write_text(f'Host bench\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {temp}/client\n IdentitiesOnly yes\n BatchMode yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {temp}/known_hosts\n')
        remote=temp/'remote-starflux'
        remote.write_text('#!/bin/sh\nexec env '+shlex.join(['SHELL='+str(shell),'STARFLUX_BENCH_ACTIVITY=1','PATH='+os.environ['PATH'],'XDG_RUNTIME_DIR='+os.environ['XDG_RUNTIME_DIR']])+' '+shlex.quote(str(app))+' "$@"\n')
        remote.chmod(0o700)
        def listening():
            try:
                with socket.create_connection(('127.0.0.1',port),timeout=.2):return True
            except OSError:return False
        wait(listening,'SSH test daemon did not start')
    try:
        for kind in os.environ.get('STARFLUX_LATENCY_KINDS','kitty,cells,graphical').split(','):
            name=base_name+'-'+kind;names.append(name)
            frontend='v-'+name
            state_path=runtime/(frontend+'.state.json')
            address='unix:'+str(temp/(kind+'.sock'));addresses.append(address)
            log=temp/(kind+'.jsonl')
            if kind=='kitty':command=['python3',str(fixture),str(log)]
            else:
                cfg={'workspace':name,'collection':name,'theme':'kanagawa','host':None,'directory':str(temp),'agent':f'python3 {fixture} {log}','executable':str(repo/'target/release/starflux'),'project_host':True,'desktop_helper':str(app)}
                path=runtime/(name+'.json');path.write_text(json.dumps(cfg));path.chmod(0o600)
                command=[str(app),'--'+('cells' if kind.startswith('cells') else 'graphical'),'--workspace',name,'--frontend',frontend]
                if 'ssh' in kind:command+=['--ssh','bench','--ssh-config',str(ssh_config),'--remote-executable',str(remote)]
            child=subprocess.Popen(['/usr/bin/kitty','--hold','--class','starflux-latency-proof','--listen-on',address,'-o','allow_remote_control=yes','-o','background_opacity=1',*command],env=dict(os.environ,STAR_GRAPHICS_RENDERER=os.environ.get('STARFLUX_BENCH_RENDERER','cpu'),STAR_TERMINAL_RENDERER='native',STARFLUX_BENCH_ACTIVITY='1',SHELL=str(shell)),stdout=subprocess.DEVNULL,stderr=(out/(phase+'-'+kind+'.log')).open('w'));children.append(child)
            wait(lambda:any(e.get('ready') for e in records(log)),'Fixture not ready')
            wait(lambda:rc(address,'ls'),'Remote control not ready')
            if kind!='kitty':
                wait(lambda:json.loads(state_path.read_text()).get('terminal_presentation',{}).get('native_cells'),'Native terminal transport absent')
            def client():return next((c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['pid']==child.pid),None)
            c=wait(client,'Kitty window absent')
            subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.window.float({{window="address:{c["address"]}"}}))'],stdout=subprocess.DEVNULL,check=True)
            subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.window.resize({{window="address:{c["address"]}",x={width},y={height},relative=false}}))'],stdout=subprocess.DEVNULL,check=True)
            subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{c["address"]}"}}))'],stdout=subprocess.DEVNULL,check=True)
            time.sleep(.6);c=client()
            x,y=c['at'];w,h=c['size']
            image=out/(phase+'-'+kind+'.png')
            def marker():
                subprocess.run(['grim','-s','1','-g',f'{x},{y} {w}x{h}',str(image)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
                iw,ih=map(int,subprocess.check_output(['magick','identify','-format','%w %h',str(image)]).split())
                raw=subprocess.check_output(['magick',str(image),'-depth','8','rgb:-'])
                at=next((i for i in range(0,len(raw)-24,3) if all(abs(raw[i+c]-v)<=3 for c,v in enumerate((24,40,56))) and raw[i:i+24]==raw[i:i+3]*8),-1)
                return (int((at//3%iw+3)*w/iw),int((at//3//iw+2)*h/ih)) if at>=0 else None
            mx,my=wait(marker,'Initial marker not visible')
            geometry=f'{x+mx},{y+my} 1x1'
            def pixel():
                began=time.monotonic_ns()
                raw=subprocess.check_output(['grim','-s','1','-t','ppm','-g',geometry,'-'],stderr=subprocess.PIPE)
                return tuple(raw[-3:]),time.monotonic_ns(),(time.monotonic_ns()-began)/1e6
            def matches(actual,expected):return all(abs(a-b)<=3 for a,b in zip(actual,expected))
            wait(lambda:matches(pixel()[0],(24,40,56)),'Initial color not visible')
            if kind!='kitty':
                # Activate the live terminal canvas through its pixel mouse route.
                v=json.loads(state_path.read_text())['viewport']
                inject(address,f'\x1b[<0;{v["width"]//2};{v["height"]//2}M\x1b[<0;{v["width"]//2};{v["height"]//2}m')
                time.sleep(.1)
            samples=[]
            for n in range(1,int(os.environ.get('STARFLUX_LATENCY_SAMPLES','27'))+1):
                c=client();x,y=c['at'];geometry=f'{x+mx},{y+my} 1x1'
                sent=inject(address,'x')
                event=wait(lambda:next((e for e in records(log) if e.get('n')==n),None),'Key not received',5,pause=.001)
                expected=(112,48,32) if n%2 else (32,48,112)
                deadline=time.monotonic()+3
                while True:
                    actual,visible,capture=pixel()
                    if matches(actual,expected):break
                    assert time.monotonic()<deadline,(kind,n,actual,expected)
                if n>3:
                    samples.append({'sent_ns':sent,'written_ns':event['written'],'visible_ns':visible,'input_ms':(event['received']-sent)/1e6,'output_ms':(visible-event['written'])/1e6,'total_ms':(visible-sent)/1e6,'capture_ms':capture})
                time.sleep(float(os.environ.get('STARFLUX_LATENCY_PAUSE','.18')))
            results[kind]={'samples':len(samples),'window':[w,h],**{key:stats([s[key] for s in samples]) for key in ['input_ms','output_ms','total_ms','capture_ms']},'raw':samples}
            if kind!='kitty':
                metadata=json.loads(state_path.read_text())
                results[kind]['presentation']=metadata.get('terminal_presentation')
                results[kind]['viewport']=metadata['viewport']
                results[kind]['graphical_render']=metadata.get('graphical_render')
            rc(address,'close-window');child.wait(timeout=15)
        result={'phase':phase,'measurement':'RPC key injection to fixture receipt and screen-capture-observed update; capture adds sampling delay','results':results}
        (out/(phase+'.json')).write_text(json.dumps(result,indent=2)+'\n')
        print(json.dumps({k:{key:value for key,value in v.items() if key!='raw'} for k,v in results.items()},indent=2))
    finally:
        for name in names:
            frontend='v-'+name
            if (runtime/(frontend+'.log')).exists():
                (out/(phase+'-'+name+'-controller.log')).write_bytes((runtime/(frontend+'.log')).read_bytes())
        for name in names:
            subprocess.run([str(app),'--workspace',name,'--stop'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=20)
        for address in addresses:
            try:rc(address,'close-window')
            except subprocess.SubprocessError:pass
        for child in children:
            if child.poll() is None:child.terminate();child.wait(timeout=10)
        if daemon:
            daemon.terminate();daemon.wait(timeout=5)
        if original:subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{original}"}}))'],stdout=subprocess.DEVNULL)
