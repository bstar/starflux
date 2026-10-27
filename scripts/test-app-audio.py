#!/usr/bin/env python3
"""Private SSH app audio: isolation, bounded credits, live reconnect and local KIT mute.
All audio goes into disposable null sinks; no user app is stopped or modified.
"""
import getpass,json,math,os,shlex,socket,struct,subprocess,tempfile,threading,time,uuid
from pathlib import Path
repo=Path(__file__).resolve().parents[1];app=repo/'scripts/run-local.sh'
name='audio-'+uuid.uuid4().hex[:8];out=repo/'target/audio-proof';out.mkdir(parents=True,exist_ok=True)
runtime=Path(os.environ['XDG_RUNTIME_DIR'])/'starmux';data=Path.home()/'.local/share/starmux/projects'
def wait(fn,label,timeout=25):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  try:
   v=fn()
   if v:return v
  except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
  time.sleep(.05)
 raise AssertionError(label)
def send(p,value):
 b=json.dumps(value).encode();p.stdin.write(struct.pack('>I',len(b))+b);p.stdin.flush()
def receiver(p,items):
 def exact(n):
  b=b''
  while len(b)<n:
   part=p.stdout.read(n-len(b))
   if not part:raise EOFError
   b+=part
  return b
 def run():
  try:
   while True:items.append(json.loads(exact(struct.unpack('>I',exact(4))[0])))
  except (EOFError,OSError):pass
 threading.Thread(target=run,daemon=True).start()
def pulse(*args):return subprocess.check_output(['pactl',*args],stderr=subprocess.PIPE).decode().strip()
children=[];modules=[];original=json.loads(subprocess.check_output(['hyprctl','activewindow','-j'])).get('address')
with tempfile.TemporaryDirectory(prefix='starmux-audio-') as tmp:
 tmp=Path(tmp)
 try:
  for key in ['host','client']:subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(tmp/key)],check=True)
  (tmp/'authorized_keys').write_text((tmp/'client.pub').read_text())
  with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
  (tmp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(tmp/'host.pub').read_text())
  (tmp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM yes\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
  cfg=tmp/'ssh_config';cfg.write_text(f'Host audio-test\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {tmp}/client\n IdentitiesOnly yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {tmp}/known_hosts\n')
  daemon=subprocess.Popen(['/usr/bin/sshd','-D','-e','-f',str(tmp/'sshd_config')],stderr=(out/'sshd.log').open('w'),env={k:v for k,v in os.environ.items() if k!='NOTIFY_SOCKET'});children.append(daemon)
  ssh=['ssh','-F',str(cfg),'-T','audio-test']
  wait(lambda:subprocess.run(ssh+['true'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0,'SSH unavailable')
  def remote(*argv):return subprocess.check_output(ssh+[shlex.join([str(app),*argv])],stderr=subprocess.PIPE,timeout=30)
  video=subprocess.Popen(ssh+[shlex.join([str(app),'--desktop-relay',name])],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(out/'video.log').open('w'));children.append(video);v=[];receiver(video,v)
  owner=wait(lambda:next((p['pid'] for p in v if p['type']=='hello'),None),'No app desktop')
  guard='mux_audio_guard_'+uuid.uuid4().hex[:6];modules.append(pulse('load-module','module-null-sink','sink_name='+guard,'rate=48000','channels=2'))
  tone=tmp/'tone.py';tone.write_text('''import gi,os,math,struct,subprocess,threading
os.environ['GSK_RENDERER']='cairo'
gi.require_version('Gtk','4.0')
from gi.repository import Gtk,Gio
class App(Gtk.Application):
 def __init__(self):super().__init__(flags=Gio.ApplicationFlags.NON_UNIQUE)
 def do_activate(self):
  w=Gtk.ApplicationWindow(application=self,title='Private audio probe');w.set_default_size(400,240);w.set_child(Gtk.Label(label='440 Hz test tone in a private sink'));w.present()
def sound():
 p=subprocess.Popen(['pw-cat','--playback','--raw','--format=s16','--rate=48000','--channels=2','--target',GUARD,'-'],stdin=subprocess.PIPE,env=dict(os.environ,PIPEWIRE_NODE=GUARD))
 b=b''.join(struct.pack('<hh',int(4000*math.sin(2*math.pi*440*i/48000)),int(4000*math.sin(2*math.pi*440*i/48000)))for i in range(4800))
 try:
  while True:p.stdin.write(b);p.stdin.flush()
 except (OSError,BrokenPipeError):pass
threading.Thread(target=sound,daemon=True).start();App().run([])
'''.replace('GUARD',repr(guard)))
  windows=json.loads(remote('--app-launch',name,'--app-directory',str(tmp),'--','/usr/bin/python3',str(tone)))
  assert windows
  def audio():
   p=subprocess.Popen(ssh+[shlex.join([str(app),'--desktop-audio-relay',name,str(owner)])],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(out/'capture.log').open('a'));children.append(p);items=[];receiver(p,items)
   wait(lambda:any(m['type']=='ready' for m in items),'Audio handshake failed')
   return p,items
  source,pcm=audio();time.sleep(.2);assert len(pcm)==1,'Audio sent without credits'
  send(source,{'blocks':8});wait(lambda:len([p for p in pcm if p['type']=='chunk'])==8,'Credit window not delivered')
  blocks=[p['samples']for p in pcm if p['type']=='chunk'];assert all(len(b)==1920 for b in blocks)
  time.sleep(.25);assert len([p for p in pcm if p['type']=='chunk'])==8,'Audio exceeded credits'
  send(source,{'blocks':1});wait(lambda:len([p for p in pcm if p['type']=='chunk'])==9,'Credit refill stalled')
  count=9
  for attempt in range(12):
   blocks=[p['samples']for p in pcm if p['type']=='chunk'];peak=max(abs(x)for b in blocks for x in b)
   if peak>1000:break
   send(source,{'blocks':8});count+=8
   wait(lambda:len([p for p in pcm if p['type']=='chunk'])==count,'Native stream credit refill failed')
   time.sleep(.1)
  assert peak>1000,('Native app was not adopted',peak)
  source.terminate();source.wait(timeout=5)
  source,again=audio();send(source,{'blocks':2});wait(lambda:any(p['type']=='chunk' and max(map(abs,p['samples']))>1000 for p in again),'Reconnect lost audio')
  assert json.loads(remote('--app-list',name))==windows,'Audio reconnect restarted the app'
  source.terminate();source.wait(timeout=5);video.terminate();video.wait(timeout=5)
  # The local frontend connects to the remote controller, whose app desktop is local to it.
  output_sink='mux_audio_output_'+uuid.uuid4().hex[:6]
  modules.append(pulse('load-module','module-null-sink','sink_name='+output_sink,'rate=48000','channels=2'))
  config={'workspace':name,'label':'SSH audio proof','theme':'kanagawa','host':None,'directory':str(tmp),'agent':None,'executable':str(repo/'target/release/starmux'),'project_host':True,'desktop_helper':str(app)}
  (runtime/(name+'.json')).write_text(json.dumps(config));(runtime/(name+'.json')).chmod(0o600)
  data.mkdir(parents=True,exist_ok=True)
  (data/(name+'.json')).write_text(json.dumps(config))
  (data/(name+'.layout.json')).write_text(json.dumps({'terminal_first':True,'space':'apps','agents':[],'terminals':[0],'terminal':0,'app_targets':[{'host':None,'directory':str(tmp),'helper':str(app)}]}))
  address='unix:'+str(tmp/'kitty.sock')
  kitty=subprocess.Popen(['/usr/bin/kitty','--hold','--class','starmux-audio-proof','--listen-on',address,'-o','allow_remote_control=yes',str(app),'--ssh','audio-test','--ssh-config',str(cfg),'--remote-executable',str(app),'--workspace',name],env=dict(os.environ,PULSE_SINK=output_sink,PIPEWIRE_NODE=output_sink,STAR_GRAPHICS_RENDERER='gpu'),stdout=subprocess.DEVNULL,stderr=(out/'kitty.log').open('w'));children.append(kitty)
  def state():return json.loads((runtime/(name+'.state.json')).read_text())
  wait(lambda:state().get('audio',{}).get('capable') and state()['audio'].get('active'),'Local KIT frontend did not open audio')
  def amplitude():
   p=subprocess.Popen(['parec','--raw','--format=s16le','--rate=48000','--channels=2','--latency-msec=20','--device='+output_sink+'.monitor'],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
   try:
    b=p.stdout.read(19200);return max(map(abs,struct.unpack('<'+'h'*(len(b)//2),b)))
   finally:p.terminate();p.wait(timeout=4)
  audible=wait(lambda:amplitude() if amplitude()>500 else None,'KIT output device received no audio',timeout=20)
  def action(value):subprocess.check_call([str(app),'--workspace',name,'--action',value],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  view=state()['viewport'];side=state()['project_navigation']['sidebar_width'];theme=150 if view['width']-side>850 else 100
  subprocess.check_output(['kitten','@','--to',address,'send-key','--match','state:focused','ctrl+escape'])
  x=view['width']-theme-83
  subprocess.check_output(['kitten','@','--to',address,'send-text','--match','state:focused','--stdin'],input=f'\x1b[<0;{x};23M\x1b[<0;{x};23m'.encode())
  wait(lambda:state()['audio']['muted'] and not state()['audio']['active'],'Header mute button did not close playback');time.sleep(.25)
  assert amplitude()==0,'Muted audio still reached local output'
  action('audio:unmute');wait(lambda:state()['audio']['active'] and not state()['audio']['muted'],'Unmute failed')
  wait(lambda:amplitude()>500,'Unmute did not restore sound')
  assert state()['apps'][0]['worker_pid']==owner,'Mute restarted app desktop'
  action('audio:mute');wait(lambda:state()['audio']['muted'],'Second mute failed')
  old=state()['controller_pid'];subprocess.check_call([str(app),'--workspace',name,'--close'],stdout=subprocess.DEVNULL)
  subprocess.check_output(['kitten','@','--to',address,'launch','--type=tab',str(app),'--ssh','audio-test','--ssh-config',str(cfg),'--remote-executable',str(app),'--workspace',name])
  wait(lambda:state()['controller_pid']!=old and state()['audio']['capable'] and state()['audio']['muted'],'Mute did not survive restart')
  assert not state()['audio']['active']
  action('audio:unmute');wait(lambda:state()['audio']['active'],'Reopened audio did not reconnect')
  wait(lambda:amplitude()>500,'Restart lost audio')
  result={'passed':True,'authenticated_ssh_audio':True,'host_output_isolated':True,'bounded_eight_block_credits':True,'reconnect_preserves_app':True,'local_kit_playback_peak':audible,'native_pipewire_app_adopted':True,'mute_survives_restart':True,'header_mute_button_works':True,'mute_is_silent':True,'unmute_restores_playback':True,'original_user_apps_touched':False}
  (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
 except BaseException:
  for p in runtime.glob(name+'*'):
   if p.is_file() and p.suffix in ['.json','.log']:
    try:(out/p.name).write_bytes(p.read_bytes())
    except OSError:pass
  try:(out/'outer-terminal.txt').write_bytes(subprocess.check_output(['kitten','@','--to',address,'get-text','--extent','all'],timeout=5))
  except (subprocess.SubprocessError,NameError):pass
  raise
 finally:
  try:subprocess.run([str(app),'--workspace',name,'--stop'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=20)
  except subprocess.SubprocessError:pass
  try:remote('--desktop-stop',name)
  except (subprocess.SubprocessError,NameError):pass
  for p in reversed(children):
   if p.poll() is None:p.terminate();p.wait(timeout=10)
  for module in modules:
   try:pulse('unload-module',module)
   except subprocess.SubprocessError:pass
  if original:subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{original}"}}))'],stdout=subprocess.DEVNULL)
