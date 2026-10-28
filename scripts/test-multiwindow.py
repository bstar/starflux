#!/usr/bin/env python3
"""Two private protocol frontends sharing fake terminal processes; no GUI or real agents."""
import argparse
import base64
import concurrent.futures
import getpass
import math
import struct
import shlex
import shutil
import socket
import json
import os
from pathlib import Path
import queue
import secrets
import subprocess
import tempfile
import threading
import time

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--ssh',action='store_true',help='Use a private local OpenSSH server for the second frontend')
parser.add_argument('--scrollback',action='store_true',help='Read pre-attachment plan history while new output arrives')
parser.add_argument('--apps',action='store_true',help='Exercise a private GTK app and streamed App Space (requires graphics runtime)')
parser.add_argument('--video',action='store_true',help='Negotiate H.264 and measure an animated private app over SSH')
parser.add_argument('--audio',action='store_true',help='Measure continuous PCM delivery alongside animated video, without speaker output')
parser.add_argument('--slow-video',action='store_true',help='Simulate a frontend that presents 4:4:4 slowly but handles 4:2:0 promptly')
parser.add_argument('--legacy-audio',action='store_true',help='Check older frontend audio compatibility')
parser.add_argument('--pointer-latency',action='store_true',help='Measure input to decoded canvas cursor using a private app')
parser.add_argument('--presentation-delay-ms',type=float,default=0,help='Additional presentation delay for the private cursor consumer')
args=parser.parse_args()
assert not args.audio or args.video,'--audio requires --video'
assert not args.video or args.apps,'--video requires --apps'
assert not args.pointer_latency or args.video,'--pointer-latency requires --video'
root=Path(__file__).resolve().parents[1]
app=root/'scripts/run-local.sh'
with tempfile.TemporaryDirectory(prefix='starmux-multiwindow-') as tmp:
    tmp=Path(tmp); runtime=tmp/'runtime';runtime.mkdir(mode=0o700)
    state=runtime/'starmux';state.mkdir(mode=0o700)
    workspace='multi-'+secrets.token_hex(4)
    shell=tmp/'shell';shell.write_text("#!/bin/sh\nprintf 'SHARED_TERMINAL_READY\\r\\n'\nexec cat\n");shell.chmod(0o700)
    if args.scrollback:
        shell.write_text("#!/bin/sh\ni=0\nwhile [ $i -lt 200 ]; do printf 'PLAN_HISTORY_%04d a long plan section before approval\\r\\n' \"$i\"; i=$((i+1)); done\nprintf 'SHARED_TERMINAL_READY -- PLAN_APPROVAL_PENDING\\r\\n'\nexec cat\n")
    config=dict(workspace=workspace,host=None,directory=str(tmp),agent=None,executable=str(app),project_host=True)
    (state/(workspace+'.json')).write_text(json.dumps(config))
    other=workspace+'-other'
    (state/(other+'.json')).write_text(json.dumps({**config,'workspace':other}))
    env={**os.environ,'XDG_RUNTIME_DIR':str(runtime),'XDG_DATA_HOME':str(tmp/'data'),'SHELL':str(shell),'STAR_TERMINAL_RENDERER':'native'}
    processes=[]
    daemon=None
    ssh_args=None
    if args.ssh:
        for key in ('host','client'):
            subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(tmp/key)],check=True)
        (tmp/'authorized_keys').write_text((tmp/'client.pub').read_text())
        with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
        (tmp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
        (tmp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(tmp/'host.pub').read_text())
        daemon=subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd','-D','-e','-f',str(tmp/'sshd_config')],stderr=(tmp/'sshd.log').open('w'),env={k:v for k,v in os.environ.items() if k!='NOTIFY_SOCKET'})
        ssh_args=['ssh','-F','/dev/null','-T','-p',str(port),'-i',str(tmp/'client'),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(tmp/'known_hosts'),'127.0.0.1']
        deadline=time.monotonic()+3
        while True:
            try:
                with socket.create_connection(('127.0.0.1',port),timeout=.1):break
            except OSError:
                assert daemon.poll() is None and time.monotonic()<deadline,(tmp/'sshd.log').read_text()
                time.sleep(.02)
    class Client:
        def __init__(self,identity,cols,rows):
            self.identity=identity;self.cols=cols;self.rows=rows;self.messages=queue.Queue();self.types=set();self.fonts=[];self.scene=None;self.app_frames=0;self.video_frames=0;self.credit_due=[];self.video_is444=True;self.audio_times=[];self.audio_nonzero=False;self.audio_epoch=None;self.allow_refresh=False;self.refresh_requested=False
            self.log=(tmp/(identity+'.log')).open('a')
            self.decoded=queue.Queue();self.cursor_frames=[];self.decoder=None
            if args.pointer_latency and identity.startswith('v-b'):
                decoder_command='source target/run-env.sh\nexec /usr/bin/python3 scripts/app-pointer-decoder.py'
                self.decoder=subprocess.Popen(['bash','-c',decoder_command],cwd=root,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log,env=env)
                processes.append(self.decoder)
                def decoded():
                    for line in self.decoder.stdout:self.decoded.put(json.loads(line))
                threading.Thread(target=decoded,daemon=True).start()
            command=[str(app),'--graphical-relay',workspace+'@'+identity]
            # This loopback host shares one backend installation. Preserve its
            # tool PATH so SSH cannot select an incompatible system tmux.
            if ssh_args and identity.startswith('v-b'):
                command=ssh_args+[shlex.join(['env',*[k+'='+env[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','SHELL','STAR_TERMINAL_RENDERER','PATH')],*command])]
            self.process=subprocess.Popen(command,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log)
            processes.append(self.process)
            def receive():
                for line in self.process.stdout:
                    try:
                        message=json.loads(line);message['_received_ns']=time.monotonic_ns();self.messages.put(message)
                    except ValueError:self.messages.put({'type':'bad','line':repr(line)})
            threading.Thread(target=receive,daemon=True).start()
            self.send(dict(type='hello',version=1,client=identity,viewport=dict(columns=cols,rows=rows,width=cols*10,height=rows*20,generation=1),capabilities=dict(native_terminal_cells=True,native_font_millipoints=12500,image_transport='kitty',pixel_geometry='measured',pointer_precision='cells',keyboard=True,paste=True,presentation_ack=True,streamed_frames=args.apps,audio_relay=args.audio,audio_pcm_v2=args.audio and not args.legacy_audio,canvas_video=args.video,canvas_video_high_quality=args.video,canvas_video_pipelined=args.video,native_surfaces=True,physical_text=True,pixel_layout=True)))
        def send(self,message):
            self.process.stdin.write(json.dumps(message).encode()+b'\n');self.process.stdin.flush()
        def pump(self):
            now=time.monotonic()
            while not self.decoded.empty():
                frame=self.decoded.get_nowait();self.cursor_frames.append(frame)
                credit=dict(type='media',message=dict(kind='canvas_video_credit',session=frame['session'],generation=frame['generation']))
                self.credit_due.append((frame['decoded_ns']/1e9+args.presentation_delay_ms/1000,credit))
            pending=self.credit_due;self.credit_due=[]
            for due,credit in pending:
                if due<=now:self.send(credit)
                else:self.credit_due.append((due,credit))
            while True:
                try:m=self.messages.get_nowait()
                except queue.Empty:return
                self.types.add(m['type'])
                if m['type']=='scene':
                    self.scene=m['scene'];self.send(dict(type='presented',revision=self.scene['revision'],generation=self.scene['viewport']['generation']))
                if m['type']=='canvas_stream' and m['frame']['stream']=='apps':self.app_frames+=1
                if m['type']=='media':
                    media=m['message'];kind=media['kind']
                    if kind in ('audio_open','pcm_open'):
                        self.audio_epoch=(media['session'],media['epoch'])
                        self.send(dict(type='media',message=dict(kind='audio_credit',session=media['session'],epoch=media['epoch'],blocks=8)))
                    if kind in ('audio_chunk','pcm_chunk'):
                        self.audio_times.append(m['_received_ns'])
                        if kind=='pcm_chunk':
                            samples=struct.unpack('<1920f',base64.b64decode(media['data']))
                            assert all(math.isfinite(s) and abs(s)<=1 for s in samples)
                        else:samples=media['samples']
                        self.audio_nonzero|=any(samples)
                        self.send(dict(type='media',message=dict(kind='audio_credit',session=media['session'],epoch=media['epoch'],blocks=1)))
                    if kind in ('canvas_video_open','canvas_video_window','canvas_video_packet'):
                        if self.decoder and kind in ('canvas_video_open','canvas_video_packet'):
                            self.decoder.stdin.write(json.dumps(media).encode()+b'\n');self.decoder.stdin.flush()
                        if kind=='canvas_video_packet':
                            self.app_frames+=1;self.video_frames+=1
                            if args.slow_video:
                                encoded=base64.b64decode(media['data'])
                                for i in range(len(encoded)-5):
                                    if encoded[i:i+3]==b'\x00\x00\x01' and encoded[i+3]&31==7:
                                        self.video_is444=encoded[i+4]==244;break
                        count=media.get('frames',1)-(1 if kind=='canvas_video_window' else 0)
                        for _ in range(count):
                            if self.decoder and kind=='canvas_video_packet':continue
                            credit=dict(type='media',message=dict(kind='canvas_video_credit',session=media['session'],generation=media['generation']))
                            if kind=='canvas_video_packet' and args.slow_video and self.video_is444:self.credit_due.append((time.monotonic()+.075,credit))
                            else:self.send(credit)
                if m['type']=='terminal_font':self.fonts.append(m['size_millipoints'])
                if m['type']=='error':
                    if self.allow_refresh and m['message']=='STARMUX_RELOAD_UI':self.refresh_requested=True
                    else:raise AssertionError(m)
        def snapshot(self):
            p=state/(self.identity+'.state.json')
            try:return json.loads(p.read_text())
            except (OSError,ValueError):return {}
        def action(self,action):
            subprocess.run([str(app),'--workspace',workspace,'--frontend',self.identity,'--action',action],env=env,check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        def input(self,event):
            assert self.scene
            self.send(dict(type='input',id=time.monotonic_ns(),revision=self.scene['revision'],generation=self.scene['viewport']['generation'],input=event))
    clients=[]
    def wait(predicate,label,timeout=12):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            for c in clients:c.pump()
            result=predicate()
            if result:return result
            time.sleep(.02)
        for project in (workspace,other):
            debug=subprocess.run(['tmux','-L','starmux-'+project,'list-panes','-a','-F','#{pane_pid} #{pane_current_command} #{pane_start_command}'],env=env,capture_output=True,text=True)
            print('Private fixture:',project,debug.stdout,debug.stderr,flush=True)
        for c in clients:print('Private client:',c.identity,'project',c.snapshot().get('active_project'),'terminals',c.snapshot().get('direct_terminals'),'history',c.snapshot().get('history'),flush=True)
        if args.pointer_latency:print('Decoded cursor frames:',b.cursor_frames[-8:],flush=True)
        raise AssertionError(label+': '+repr([dict(frontend=c.identity,exit=c.process.poll(),active=c.snapshot().get('active'),tabs=tabs(c),status=c.snapshot().get('status'),stderr=(tmp/(c.identity+'.log')).read_text()) for c in clients])+(' sshd: '+(tmp/'sshd.log').read_text() if daemon else ''))
    def terminal_text(c,pane=0):return next((t['text'] for t in c.snapshot().get('direct_terminals',[]) if t['pane']==pane),'')
    def terminal_size(pane=0):
        return tuple(map(int, subprocess.check_output(['tmux','-L','starmux-'+workspace,'display-message','-p','-t',f'p{pane}:0.0','#{pane_width} #{pane_height}'],env=env,text=True).split()))
    def expected_size(c):
        return next((component['rect']['width'],component['rect']['height']) for component in c.scene['components'] if component['kind']=='terminal')
    def tabs(c):return c.snapshot().get('project_state',{}).get('terminals',[])
    def pid():return subprocess.check_output(['tmux','-L','starmux-'+workspace,'display-message','-p','-t','p0:0.0','#{pane_pid}'],env=env,text=True).strip()
    try:
        b_columns,b_rows=(230,72) if args.video else (130,44)
        a=Client('v-a'+secrets.token_hex(5),100,35);b=Client('v-b'+secrets.token_hex(5),b_columns,b_rows);clients=[a,b]
        wait(lambda:all('SHARED_TERMINAL_READY' in terminal_text(c) for c in clients),'both shared attachments')
        first_pid=pid()
        if args.apps:
            assert all(c.snapshot()['app_view_available'] for c in clients)
            assert not any(c.snapshot()['apps'] for c in clients),'App workers started before App Space was opened'
            gui=tmp/'app.py';keys=tmp/'app-keys'
            tone=tmp/'tone.raw'
            if args.audio:
                tone.write_bytes(b''.join(struct.pack('<hh',value,value) for i in range(48000*15) for value in [int(2000*math.sin(i*440*2*math.pi/48000))]))
            gui.write_text("""import gi,os,sys,subprocess,time
os.environ['GSK_RENDERER']='cairo'
gi.require_version('Gtk','4.0')
from gi.repository import Gtk,Gio,GLib
class App(Gtk.Application):
 def __init__(self):super().__init__(flags=Gio.ApplicationFlags.NON_UNIQUE)
 def do_activate(self):
  w=Gtk.ApplicationWindow(application=self,title='Private App Space proof');w.set_default_size(600,400)
  entry=Gtk.Entry(placeholder_text='SSH application input')
  box=Gtk.Box(orientation=Gtk.Orientation.VERTICAL);box.append(entry);w.set_child(box)
  if len(sys.argv)>2:
   area=Gtk.DrawingArea();area.set_vexpand(True);box.append(area);phase=[0];cursor=[-100]
   def draw(area,cr,width,height):
    cr.set_source_rgb(.1,.15,.2);cr.paint();cr.set_source_rgb(.2,.7,.9);cr.rectangle((phase[0]*7)%max(1,width),20,80,height*.7);cr.fill()
    cr.set_source_rgb(1,0,0);cr.rectangle(cursor[0],0,24,height);cr.fill()
   def motion(controller,x,y):
    cursor[0]=x;area.queue_draw()
    with open(sys.argv[1]+'.pointer','a') as f:f.write(str(x)+' '+str(time.monotonic_ns())+'\\n')
   mouse=Gtk.EventControllerMotion();mouse.connect('motion',motion);area.add_controller(mouse)
   def animate():phase[0]+=1;area.queue_draw();return True
   area.set_draw_func(draw);GLib.timeout_add(16,animate)
  def key(controller,keyval,keycode,state):
   with open(sys.argv[1],'a') as f:f.write(str(keyval)+'\\n')
   return False
  k=Gtk.EventControllerKey();k.set_propagation_phase(Gtk.PropagationPhase.CAPTURE);k.connect('key-pressed',key);w.add_controller(k)
  w.present();entry.grab_focus()
  if len(sys.argv)>3:subprocess.Popen(['paplay','--raw','--format=s16le','--rate=48000','--channels=2',sys.argv[3]])
App().run([sys.argv[0]])
""")
            b.action('run:'+shlex.join(['/usr/bin/python3',str(gui),str(keys)]+(['animate'] if args.video else [])+([str(tone)] if args.audio else [])))
            wait(lambda:b.snapshot().get('project_state',{}).get('space')=='apps' and any(h['windows'] for h in b.snapshot().get('apps',[])),'SSH App Space maps the private GTK app',timeout=40)
            wait(lambda:b.app_frames>0,'SSH App Space streams actual app pixels',timeout=20)
            worker=b.snapshot()['apps'][0]['worker_pid']
            b.action('apps:maximize')
            wait(lambda:b.snapshot()['apps'][0].get('selected') is not None,'app window selection')
            time.sleep(.2)
            for action in ('down','up'):
                b.input(dict(kind='embedded',canvas='apps',event=dict(timestamp_ns=time.monotonic_ns(),sequence=time.monotonic_ns(),kind=dict(kind='pointer',action=action,button=0,position=[100,100],modifiers=0))))
            time.sleep(.2)
            b.input(dict(kind='embedded',canvas='apps',event=dict(timestamp_ns=time.monotonic_ns(),sequence=1,kind=dict(kind='key',code='char:z',phase='press',modifiers=0))))
            wait(lambda:keys.exists() and '122' in keys.read_text(),'SSH app receives keyboard input')
            if args.video:
                began=time.monotonic();initial=b.video_frames
                while time.monotonic()-began<(8 if args.audio or args.slow_video else 3):
                    for c in clients:c.pump()
                    time.sleep(.002)
                rate=(b.video_frames-initial)/(time.monotonic()-began)
                assert b.video_frames-initial>=20,'H.264 app output did not keep streaming'
                stats=b.snapshot()
                canvas=next(c for c in b.scene['components'] if c['kind']=='live_canvas' and c['id']=='apps')['view']
                assert canvas['source']==[canvas['target']['width'],canvas['target']['height']],canvas
                assert stats['app_video_render']['encoded_size']==stats['app_video_render']['source_size']==canvas['source'],stats['app_video_render']
                assert canvas['source'][0]<=1920 and canvas['source'][1]<=1080,canvas
                if args.slow_video:assert stats['app_video_render']['responsive_stage']>=1 and stats['app_video_render']['chroma']=='I420',stats['app_video_render']
                report={'transport':'authenticated loopback SSH','delivered_video_fps':rate,'video_frames':b.video_frames,'graphical_render':stats['graphical_render'],'encoder':stats['app_video_render']}
                if args.pointer_latency:
                    samples=[];input_samples=[]
                    source_width=stats['app_video_render']['source_size'][0]
                    def matches_cursor(frame,x):
                        return frame['cursor_x'] is not None and abs(frame['cursor_x']*source_width/frame['width']-x)<10
                    for index in range(14):
                        x=150 if index%2 else 650
                        began_input=time.monotonic_ns()
                        b.input(dict(kind='embedded',canvas='apps',event=dict(timestamp_ns=began_input,sequence=began_input,kind=dict(kind='pointer',action='move',button=0,position=[x,400],modifiers=0))))
                        wait(lambda:any(f['decoded_ns']>=began_input and matches_cursor(f,x) for f in b.cursor_frames),'input appears in decoded canvas cursor',timeout=5)
                        received=min(f['decoded_ns'] for f in b.cursor_frames if f['decoded_ns']>=began_input and matches_cursor(f,x))
                        delivered=[int(line.split()[1]) for line in Path(str(keys)+'.pointer').read_text().splitlines() if abs(float(line.split()[0])-x)<10 and int(line.split()[1])>=began_input]
                        assert delivered,'Canvas cursor moved without a matching app input event'
                        assert min(delivered)<=received,'An old cursor frame matched before this input reached the app'
                        if index>=2:
                            samples.append((received-began_input)/1e6);input_samples.append((min(delivered)-began_input)/1e6)
                    ordered=sorted(samples)
                    report['canvas_cursor']={'samples_ms':samples,'median_ms':ordered[len(ordered)//2],'p95_ms':ordered[len(ordered)*95//100],'app_input_ms':input_samples,'presentation_delay_ms':args.presentation_delay_ms,'scope':'input through SSH, private app redraw, capture, encode, SSH and actual H.264 decode; excludes hardware display'}
                if args.audio:
                    assert len(b.audio_times)>=50 and b.audio_nonzero,'Live app tone did not arrive'
                    steady=[t for t in b.audio_times if t>=began*1e9]
                    gaps=sorted((right-left)/1e6 for left,right in zip(steady,steady[1:]))
                    report['audio']={'protocol':stats['audio']['playback_protocol'],'blocks':len(steady),'nonzero':b.audio_nonzero,'max_arrival_gap_ms':max(gaps),'p95_arrival_gap_ms':gaps[len(gaps)*95//100],'gap_samples_ms':[(right-left)/1e6 for left,right in zip(steady,steady[1:])]}
                    assert max(gaps)<80,report['audio']
                    assert stats['audio']['playback_protocol']==('legacy-stereo' if args.legacy_audio else 'pcm-v2')


                out=root/'target/app-video-proof';out.mkdir(parents=True,exist_ok=True);(out/'ssh.json').write_text(json.dumps(report,indent=2)+'\n')
                print(json.dumps({k:({x:y for x,y in v.items() if x!='gap_samples_ms'} if isinstance(v,dict) else v) for k,v in report.items()}),flush=True)
                a.action('view:apps')
                wait(lambda:a.snapshot().get('app_video_render',{}).get('packets_sent',0)>3,'smaller frontend receives its native cropped app pixels')
                small=next(c for c in a.scene['components'] if c['kind']=='live_canvas' and c['id']=='apps')['view']
                assert small['source']==[small['target']['width'],small['target']['height']],small
                assert all(small['source'][i]<canvas['source'][i] for i in (0,1)),(small,canvas)
                assert a.snapshot()['apps'][0]['worker_pid']==worker,'A smaller frontend replaced the app desktop'
                a.action('view:terminals')
                wait(lambda:'SHARED_TERMINAL_READY' in terminal_text(a),'smaller frontend returns to terminals')
            b.action('view:terminals')
            wait(lambda:'SHARED_TERMINAL_READY' in terminal_text(b),'return to terminals from App Space')
            assert pid()==first_pid
            b.action('view:apps')
            wait(lambda:b.snapshot()['project_state']['space']=='apps','reopen App Space')
            assert b.snapshot()['apps'][0]['worker_pid']==worker
            b.action('apps:close');b.action('view:terminals')
            wait(lambda:b.snapshot()['project_state']['space']=='terminals','finish private app proof')
        if args.scrollback:
            assert 'PLAN_HISTORY_0000' not in terminal_text(a)
            def wheel(client,action='scroll_up'):
                client.input(dict(kind='embedded',canvas='terminals',event=dict(timestamp_ns=time.monotonic_ns(),sequence=time.monotonic_ns(),kind=dict(kind='pointer',action=action,button=0,position=[80,80],modifiers=0))))
            for reader in clients:
                wheel(reader)
                def in_copy_mode():
                    return subprocess.check_output(['tmux','-L','starmux-'+workspace,'display-message','-p','-t','p0:0.0','#{pane_in_mode}'],env=env,text=True).strip()=='1'
                wait(in_copy_mode,'wheel must enter backend history after attaching to an existing plan')
                for _ in range(75):
                    wheel(reader)
                    for c in clients:c.pump()
                    if 'PLAN_HISTORY_0000' in terminal_text(reader):break
                    time.sleep(.025)
                wait(lambda:'PLAN_HISTORY_0000' in terminal_text(reader),'scroll to plan beginning that predates attachment')
                # Write only to the private fake process's PTY, simulating continued
                # generation without delivering any input to the approval prompt.
                with open('/proc/'+first_pid+'/fd/1','wb',buffering=0) as output:
                    output.write(b'BACKGROUND_OUTPUT_WHILE_READING\r\n'*5)
                stable_until=time.monotonic()+.5
                while time.monotonic()<stable_until:
                    for c in clients:c.pump()
                    assert 'PLAN_HISTORY_0000' in terminal_text(reader),'new output displaced the plan being read'
                    time.sleep(.03)
                assert pid()==first_pid
                for _ in range(80):wheel(reader,'scroll_down')
                wait(lambda:not in_copy_mode(),'wheel down returns to the live approval prompt')
                wait(lambda:all('SHARED_TERMINAL_READY' in terminal_text(c) for c in clients),'return to untouched approval prompt')
        assert a.scene['viewport']['columns']==100 and b.scene['viewport']['columns']==b_columns
        a.input(dict(kind='paste',text='SHARED_FROM_FIRST_WINDOW'))
        wait(lambda:all('SHARED_FROM_FIRST_WINDOW' in terminal_text(c) for c in clients),'shared terminal input')
        wait(lambda:terminal_size()==expected_size(a),'active smaller frontend owns terminal size')
        b.input(dict(kind='paste',text='SHARED_FROM_LARGER_WINDOW'))
        wait(lambda:terminal_size()==expected_size(b),'inactive smaller frontend must not constrain larger terminal')
        assert pid()==first_pid
        a.input(dict(kind='paste',text='ACTIVE_SMALLER_AGAIN'))
        wait(lambda:terminal_size()==expected_size(a),'terminal follows active frontend when returning')
        row=next(h['rect'] for c in a.scene['components'] if c['kind']=='surface' for h in c['surface'].get('hits',[]) if h['action']=='project:'+other)
        # The right/bottom padding belongs to the row's base selection target.
        point=[row['x']+row['width']-1,row['y']+row['height']-1]
        for action in ('down','up'):
            a.input(dict(kind='pointer',action=action,button=0,x=point[0]//10,y=point[1]//20,pixel=point,modifiers=0))
        wait(lambda:a.snapshot().get('active_project')==other,'clicking project-row padding selects the project')
        a.action('project:'+workspace)
        wait(lambda:a.snapshot().get('active_project')==workspace and 'SHARED_TERMINAL_READY' in terminal_text(a),'return after full-row project selection')
        with concurrent.futures.ThreadPoolExecutor() as pool:list(pool.map(lambda c:c.action('terminal:new'),clients))
        wait(lambda:all(len(tabs(c))==3 for c in clients),'concurrent tab allocation')
        assert set(a.snapshot()['project_state']['terminals'])=={0,1,2}
        assert {a.snapshot()['active'],b.snapshot()['active']}=={1,2},'selected tabs leaked across views'
        a.action('terminal:split')
        wait(lambda:a.snapshot().get('project_state',{}).get('split'),'first split')
        assert not b.snapshot()['project_state']['split'],'split leaked across views'
        # A new pane must fill a tall split even when another frontend learns
        # about the new tab while keeping it hidden. Hidden 80x24 attachments
        # previously became tmux's latest client and produced dotted margins.
        a.input(dict(kind='resize',viewport=dict(columns=250,rows=100,width=2500,height=2000,generation=2)))
        wait(lambda:a.scene['viewport']['columns']==250,'large split viewport')
        def visible_sizes(c):
            snapshot=c.snapshot();model=snapshot.get('project_state',{})
            ids=model.get('split_panes') if model.get('split') else [snapshot.get('active')]
            rects=[component['rect'] for component in c.scene['components'] if component['kind']=='terminal']
            return dict(zip(ids or [],[(r['width'],r['height']) for r in rects]))
        def panes_fill_views():
            return all(visible_sizes(c) and
                {t['pane'] for t in c.snapshot().get('direct_terminals',[])}==set(visible_sizes(c)) and
                all(terminal_size(pane)==size for pane,size in visible_sizes(c).items()) for c in clients)
        wait(panes_fill_views,'split terminal sizes must match visible panes, without hidden clients')
        if args.scrollback:
            active=a.snapshot()['active']
            left=a.snapshot()['project_state']['split_panes'][0]
            assert left!=active
            wheel(a)
            wait(lambda:subprocess.check_output(['tmux','-L','starmux-'+workspace,'display-message','-p','-t',f'p{left}:0.0','#{pane_in_mode}'],env=env,text=True).strip()=='1','wheel targets the hovered split, not the keyboard-focused pane')
            assert a.snapshot()['active']==active,'scrolling moved keyboard focus'
            for _ in range(80):wheel(a,'scroll_down')
            wait(lambda:subprocess.check_output(['tmux','-L','starmux-'+workspace,'display-message','-p','-t',f'p{left}:0.0','#{pane_in_mode}'],env=env,text=True).strip()=='0','split wheel returns to live output')
        stable_until=time.monotonic()+1.5
        while time.monotonic()<stable_until:
            for c in clients:c.pump()
            assert panes_fill_views(),'a late hidden attachment shrank a visible pane'
            time.sleep(.05)
        a.input(dict(kind='resize',viewport=dict(columns=100,rows=35,width=1000,height=700,generation=3)))
        wait(lambda:a.scene['viewport']['columns']==100 and panes_fill_views(),'split resize follows viewport')
        a.action('terminal:font:increase')
        wait(lambda:a.fonts==[13500],'window-local font request')
        assert b.fonts==[]
        previous_revision=a.scene['revision']
        a.action('project:'+other)
        wait(lambda:a.snapshot().get('active_project')==other and a.scene['revision']>previous_revision and 'SHARED_TERMINAL_READY' in terminal_text(a),'independent project navigation and terminal readiness')
        assert b.snapshot()['active_project']==workspace
        a.input(dict(kind='paste',text='ONLY_IN_SECOND_PROJECT'))
        wait(lambda:'ONLY_IN_SECOND_PROJECT' in terminal_text(a),'project-local input')
        assert 'ONLY_IN_SECOND_PROJECT' not in terminal_text(b)
        a.action('close-project:'+workspace)
        wait(lambda:workspace not in a.snapshot().get('project_order',[]),'closing an inactive project removes only this window row')
        assert b.snapshot()['active_project']==workspace and workspace in b.snapshot()['project_order']
        assert pid()==first_pid
        b.input(dict(kind='pointer',action='down',button=1,x=2,y=6,pixel=[20,130],modifiers=0))
        wait(lambda:b.snapshot().get('context_menu') is not None,'independent context menu')
        assert a.snapshot().get('context_menu') is None
        assert any(c['kind']=='terminal' for c in b.scene['components'])
        b.action('menu:dismiss')
        b.input(dict(kind='resize',viewport=dict(columns=140,rows=45,width=1400,height=900,generation=2)))
        wait(lambda:b.scene['viewport']['columns']==140,'independent viewport resize')
        assert a.scene['viewport']['columns']==100
        a.action('project:new')
        wait(lambda:a.snapshot().get('modal_open'),'reopen project dialog')
        a.input(dict(kind='paste',text=workspace))
        for code in ('tab','tab','enter'):a.input(dict(kind='key',code=code,modifiers=0))
        wait(lambda:a.snapshot().get('active_project')==workspace and not a.snapshot().get('modal_open'),'reopen original project without restarting')
        assert a.snapshot()['project_state']['split'] and not b.snapshot()['project_state']['split']
        assert pid()==first_pid,'second window restarted the terminal'
        # Exit only our private fake cat process. Both views must remove that shared tab.
        subprocess.run(['tmux','-L','starmux-'+workspace,'send-keys','-t','p0:0.0','C-d','C-d'],env=env,check=True)
        wait(lambda:all(0 not in tabs(c) for c in clients),'shared exit removal')
        a.action('terminal:new')
        wait(lambda:all(0 in tabs(c) for c in clients),'safe slot reuse')
        b.action('terminal:0')
        wait(lambda:all('SHARED_TERMINAL_READY' in terminal_text(c,0) for c in clients),'replacement attachments on selected tabs')
        new_pid=pid();assert new_pid!=first_pid
        old_controller=a.snapshot()['controller_pid'];identity=a.identity
        a.allow_refresh=True;a.action('ui:refresh')
        wait(lambda:a.refresh_requested,'window-scoped refresh')
        a.process.wait(timeout=5);clients.remove(a)
        subprocess.run([str(app),'--wait-ui-refresh',identity],env=env,check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        a=Client(identity,100,35);clients.insert(0,a)
        wait(lambda:a.snapshot().get('controller_pid')!=old_controller and a.scene is not None and 'SHARED_TERMINAL_READY' in terminal_text(a,0),'refresh preserves view and terminal')
        assert a.snapshot()['project_state']['split'] and not b.snapshot()['project_state']['split']
        assert pid()==new_pid and b.process.poll() is None
        a.process.stdin.close();a.process.wait(timeout=5);clients.remove(a)
        wait(lambda:b.process.poll() is None and b.snapshot().get('frontend')==b.identity,'other frontend retained')
        b.action('terminal:0');wait(lambda:b.snapshot().get('active')==0,'second window selecting terminal')
        b.input(dict(kind='paste',text='SURVIVING_WINDOW_INPUT'))
        wait(lambda:'SURVIVING_WINDOW_INPUT' in terminal_text(b),'surviving input')
        assert pid()==new_pid
        subprocess.run([str(app),'--workspace',workspace,'--frontend',b.identity,'--close'],env=env,check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        old_controller=b.snapshot().get('controller_pid')
        b.process.wait(timeout=5);clients.remove(b)
        b=Client(b.identity,120,40);clients=[b]
        wait(lambda:b.scene is not None and b.snapshot().get('controller_pid')!=old_controller and 'SURVIVING_WINDOW_INPUT' in terminal_text(b),'reattach preserves process and screen')
        assert pid()==new_pid and not b.snapshot()['project_state']['split']
        inspection=json.loads(subprocess.check_output([str(app),'--workspace',workspace,'--inspect'],env=env))
        assert len(inspection['frontends'])==1,[(v.get('frontend'),v.get('controller_pid')) for v in inspection['frontends']]
        print(json.dumps(dict(result='PASS',ssh=args.ssh,simultaneous_frontends=2,active_terminal_sizing=True,app_space_streaming=args.apps,h264_video=args.video,audio_video=args.audio,plan_history_scrolling=args.scrollback,split_panes_fill=True,hidden_tabs_do_not_resize=True,project_close_is_window_local=True,shared_input=True,independent_projects=True,independent_menus=True,independent_viewports=True,independent_selection=True,independent_splits=True,independent_fonts=True,concurrent_tab_allocation=True,shared_exit=True,safe_slot_reuse=True,window_scoped_refresh=True,detach_preserves_other_window=True,reattach_preserves_process=True)))
    finally:
        for c in clients:
            try:c.process.stdin.close()
            except OSError:pass
        for p in processes:
            if p.poll() is None:
                try:p.wait(timeout=3)
                except subprocess.TimeoutExpired:p.terminate();p.wait(timeout=3)
        if args.apps:
            subprocess.run([str(app),'--workspace',workspace,'--stop'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=20)
        if daemon:
            daemon.terminate();daemon.wait(timeout=3)
        # Controllers are private to this temporary runtime; leave actual workspaces alone.
        for descriptor in state.glob('*.frontend.json'):
            identity=json.loads(descriptor.read_text())['id'];(state/(identity+'.close')).touch()
        for name in (workspace,other):
            subprocess.run(['tmux','-L','starmux-'+name,'kill-server'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        # GTK portals can mount FUSE inside this disposable runtime. Detach only
        # this test's mount before TemporaryDirectory attempts to traverse it.
        for portal in ('doc','gvfs'):
            portal_mount=runtime/portal
            if args.apps and os.path.ismount(portal_mount):
                subprocess.run([shutil.which('fusermount3') or 'fusermount','-u','-z',str(portal_mount)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
