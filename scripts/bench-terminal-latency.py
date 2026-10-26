#!/usr/bin/env python3
"""Compare RPC key injection → fixture receipt → screen-captured update.

Uses disposable Kitty windows/sessions. Screen capture adds sampling overhead;
these are software-path comparisons, not hardware input-to-photon measurements.
"""
import json
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
runtime=Path(os.environ['XDG_RUNTIME_DIR'])/'starmux'
phase=os.environ.get('STARMUX_LATENCY_PHASE','before')
out=repo/'target/terminal-latency-proof';out.mkdir(parents=True,exist_ok=True)
name='latency-'+uuid.uuid4().hex[:8]
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

with tempfile.TemporaryDirectory(prefix='starmux-latency-') as temp:
    temp=Path(temp);children=[];addresses=[];results={}
    fixture=temp/'fixture.py'
    fixture.write_text('''import json,os,sys,time,tty,termios,subprocess
old=termios.tcgetattr(0);tty.setraw(0)
n=0
if os.environ.get('STARMUX_BENCH_ACTIVITY')=='1' and os.environ.get('TMUX'):
 subprocess.run(['tmux','set-option','-p','@starmux_state','working'],check=True)
with open(sys.argv[1],'a') as log:
 def draw(color):
  patch='\\x1b[?25l\\x1b[48;2;%d;%d;%dm'%color
  patch+=''.join('\\x1b[%d;6H'%row+' '*12 for row in range(6,10))
  os.write(1,patch.encode())
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
    width=int(os.environ.get('STARMUX_LATENCY_WIDTH','1400'));height=int(os.environ.get('STARMUX_LATENCY_HEIGHT','1000'))
    try:
        for kind in os.environ.get('STARMUX_LATENCY_KINDS','kitty,starmux').split(','):
            address='unix:'+str(temp/(kind+'.sock'));addresses.append(address)
            log=temp/(kind+'.jsonl')
            if kind=='kitty':command=['python3',str(fixture),str(log)]
            else:
                cfg={'workspace':name,'collection':name,'theme':'kanagawa','host':None,'directory':str(temp),'agent':f'python3 {fixture} {log}','executable':str(repo/'target/release/starmux'),'project_host':True,'desktop_helper':str(app)}
                path=runtime/(name+'.json');path.write_text(json.dumps(cfg));path.chmod(0o600)
                command=[str(app),'--workspace',name]
            child=subprocess.Popen(['/usr/bin/kitty','--hold','--class','starmux-latency-proof','--listen-on',address,'-o','allow_remote_control=yes','-o','background_opacity=1',*command],env=dict(os.environ,STAR_GRAPHICS_RENDERER='gpu',STAR_TERMINAL_RENDERER='kitty'),stdout=subprocess.DEVNULL,stderr=(out/(phase+'-'+kind+'.log')).open('w'));children.append(child)
            wait(lambda:any(e.get('ready') for e in records(log)),'Fixture not ready')
            wait(lambda:rc(address,'ls'),'Remote control not ready')
            if kind=='starmux':
                state_path=runtime/(name+'.state.json')
                wait(lambda:json.loads(state_path.read_text()).get('terminal_presentation',{}).get('frame'),'Terminal frame absent')
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
                at=raw.find(bytes([24,40,56])*8)
                return (int((at//3%iw+3)*w/iw),int((at//3//iw+2)*h/ih)) if at>=0 else None
            mx,my=wait(marker,'Initial marker not visible')
            geometry=f'{x+mx},{y+my} 1x1'
            def pixel():
                began=time.monotonic_ns()
                raw=subprocess.check_output(['grim','-s','1','-t','ppm','-g',geometry,'-'],stderr=subprocess.PIPE)
                return tuple(raw[-3:]),time.monotonic_ns(),(time.monotonic_ns()-began)/1e6
            def matches(actual,expected):return all(abs(a-b)<=3 for a,b in zip(actual,expected))
            wait(lambda:matches(pixel()[0],(24,40,56)),'Initial color not visible')
            if kind=='starmux':
                # Activate the live terminal canvas through its pixel mouse route.
                v=json.loads(state_path.read_text())['viewport']
                inject(address,f'\x1b[<0;{v["width"]//2};{v["height"]//2}M\x1b[<0;{v["width"]//2};{v["height"]//2}m')
                time.sleep(.1)
            samples=[]
            for n in range(1,int(os.environ.get('STARMUX_LATENCY_SAMPLES','27'))+1):
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
                time.sleep(.055)
            results[kind]={'samples':len(samples),'window':[w,h],**{key:stats([s[key] for s in samples]) for key in ['input_ms','output_ms','total_ms','capture_ms']},'raw':samples}
            if kind=='starmux':results[kind]['presentation']=json.loads(state_path.read_text()).get('terminal_presentation')
            rc(address,'close-window');child.wait(timeout=15)
        result={'phase':phase,'measurement':'RPC key injection to fixture receipt and screen-capture-observed update; capture adds sampling delay','results':results}
        (out/(phase+'.json')).write_text(json.dumps(result,indent=2)+'\n')
        print(json.dumps({k:{key:value for key,value in v.items() if key!='raw'} for k,v in results.items()},indent=2))
    finally:
        if (runtime/(name+'.log')).exists():
            (out/(phase+'-controller.log')).write_bytes((runtime/(name+'.log')).read_bytes())
        subprocess.run([str(app),'--workspace',name,'--stop'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=20)
        for address in addresses:
            try:rc(address,'close-window')
            except subprocess.SubprocessError:pass
        for child in children:
            if child.poll() is None:child.terminate();child.wait(timeout=10)
        if original:subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{original}"}}))'],stdout=subprocess.DEVNULL)
