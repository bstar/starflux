#!/usr/bin/env python3
"""App Shelf acceptance: real Kitty, project switching, native GTK apps and loopback SSH.

Creates only disposable projects, application windows, keys and an SSH daemon.
No desktop configuration or user agent sessions are changed.
"""
import getpass,json,os,pathlib,shlex,socket,subprocess,tempfile,time,uuid
repo=pathlib.Path(__file__).resolve().parents[1];app=repo/'scripts/run-local.sh';runtime=pathlib.Path(os.environ['XDG_RUNTIME_DIR'])/'starflux'
out=repo/'target/app-shelf-proof';out.mkdir(parents=True,exist_ok=True)
local='shelf-'+uuid.uuid4().hex[:8];remote='shelf-ssh-'+uuid.uuid4().hex[:8];names=[local,remote];children=[]
original=json.loads(subprocess.check_output(['hyprctl','activewindow','-j'])).get('address')
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
def clients():return json.loads(subprocess.check_output(['hyprctl','clients','-j']))
def screenshot(name):
    c=wait(lambda:next((c for c in clients() if c['class']=='starflux-shelf-proof' or c['pid']==kitty.pid),None),'Outer Kitty missing')
    time.sleep(.6)
    x,y=c['at'];w,h=c['size'];subprocess.run(['grim','-g',f'{x},{y} {w}x{h}',str(out/name)],check=True)
with tempfile.TemporaryDirectory(prefix='starflux-shelf-') as tmp:
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
        gui=tmp/'app.py';gui.write_text('''import gi,sys,os,json
os.environ['GSK_RENDERER']='cairo'
gi.require_version('Gtk','4.0')
from gi.repository import Gtk,Gio
class App(Gtk.Application):
 def __init__(self):super().__init__(flags=Gio.ApplicationFlags.NON_UNIQUE)
 def do_activate(self):
  w=Gtk.ApplicationWindow(application=self,title=sys.argv[1]);w.set_default_size(850,500)
  box=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=20);box.set_margin_top(30);box.set_margin_start(30);box.set_margin_end(30)
  box.append(Gtk.Label(label=sys.argv[1]));box.append(Gtk.Label(label='Actual GTK application in the project virtual desktop'))
  entry=Gtk.Entry(placeholder_text='Type here to verify app input');box.append(entry)
  button=Gtk.Button(label='App action');box.append(button)
  def key(controller,keyval,keycode,state):
   with open(sys.argv[2],'a') as f:f.write(json.dumps({'pid':os.getpid(),'key':keyval})+'\\n')
   return False
  k=Gtk.EventControllerKey();k.set_propagation_phase(Gtk.PropagationPhase.CAPTURE);k.connect('key-pressed',key);w.add_controller(k)
  w.set_child(box);w.present();entry.grab_focus()
App().run([sys.argv[0]])
''')
        remote_config={'workspace':remote,'theme':'kanagawa','host':'shelf-host','ssh_config':str(sshconfig),'directory':str(tmp),'agent':'bash --noprofile --norc','executable':str(repo/'target/release/starflux'),'project_host':True,'desktop_helper':str(app)}
        (runtime/(remote+'.json')).write_text(json.dumps(remote_config));os.chmod(runtime/(remote+'.json'),0o600)
        address='unix:'+str(tmp/'kitty.sock')
        def rc(*args,data=None):return subprocess.check_output(['kitten','@','--to',address,*args],input=data,stderr=subprocess.PIPE,timeout=15)
        cmd=['/usr/bin/kitty','--hold','--class','starflux-shelf-proof','--config','NONE','--listen-on',address,'--title','STAR-FLUX-SHELF-PROOF','-o','linux_display_server=wayland','-o','allow_remote_control=yes','-o','initial_window_width=1450','-o','initial_window_height=1000',str(app),'--workspace',local,'--agent','bash --noprofile --norc','--directory',str(tmp),'--app',shlex.join(['/usr/bin/python3',str(gui),'Project app',str(tmp/'local-keys')])]
        kitty=subprocess.Popen(cmd,stdout=subprocess.DEVNULL,stderr=(out/'kitty.log').open('w'));children.append(kitty)
        state=wait(lambda:inspect() if inspect()['desktop']['windows']==2 and all(p.get('alive') for p in inspect()['panes']) else None,'Initial sessions did not map')
        pids=[p['pid']for p in state['panes']]
        c=wait(lambda:next((c for c in clients() if c['class']=='starflux-shelf-proof' or c['pid']==kitty.pid),None),'Outer window missing')
        for expression in [f'hl.dsp.window.float({{action="set",window="address:{c["address"]}"}})',f'hl.dsp.window.resize({{x=1450,y=1000,relative=false,window="address:{c["address"]}"}})']:
            subprocess.run(['hyprctl','eval','hl.dispatch('+expression+')'],check=True,stdout=subprocess.DEVNULL)
        outer=rc('get-text');(out/'outer-terminal.txt').write_bytes(outer);assert b'Error: Graphical session' not in outer,outer.decode();screenshot('agent.png')
        form_project='shelf-form-'+uuid.uuid4().hex[:8];names.append(form_project)
        action('project:new');wait(lambda:inspect().get('modal_open'),'New project dialog missing')
        screenshot('new-project-dialog.png')
        rc('send-text','--stdin','--bracketed-paste','enable',data=form_project.encode())
        rc('send-key','ctrl+enter')
        wait(lambda:inspect()['active_project']==form_project and len(inspect()['sessions'])==1 and all((p.get('health') or {}).get('alive') for p in inspect()['sessions']),'Project creation form failed')
        action('project:'+local);wait(lambda:inspect()['active_project']==local,'Project return after creation failed')
        action('terminal:new');wait(lambda:len(inspect()['sessions'])==3 and len(inspect()['desktop']['project_windows'])==3,'Extra terminal missing')
        action('terminal:split');wait(lambda:inspect()['project_state']['split'],'Explicit split missing');screenshot('terminals-split.png')
        action('terminal:split');action('view:agent');wait(lambda:inspect()['project_state']['space']=='terminals','Terminal space missing')
        assert [p['pid']for p in inspect()['panes']]==pids,'Space changes restarted the agent'
        action('run-app');state=wait(lambda:inspect() if inspect().get('apps') and inspect()['apps'][0]['windows'] else None,'Local GTK app did not map')
        local_worker=state['apps'][0]['worker_pid'];local_windows=state['apps'][0]['windows'];screenshot('local-app.png')
        action('run:'+shlex.join(['/usr/bin/python3',str(gui),'Supporting app',str(tmp/'support-keys')]))
        wait(lambda:len(inspect()['apps'][0]['windows'])==2,'Supporting GUI app missing');action('apps:tile');screenshot('apps-tiled.png')
        action('view:agent');wait(lambda:inspect()['project_state']['space']=='terminals','App shelf space missing');screenshot('app-shelf.png')
        action('project:'+remote);state=wait(lambda:inspect() if inspect()['active_project']==remote and all(p.get('alive') for p in inspect()['panes']) else None,'Remote project unavailable')
        remote_pids=[p['pid']for p in state['panes']]
        action('run:'+shlex.join(['/usr/bin/python3',str(gui),'SSH project app',str(tmp/'remote-keys')]))
        state=wait(lambda:inspect() if inspect().get('apps') and inspect()['apps'][0]['windows'] else None,'SSH GTK app did not map',50)
        remote_worker=state['apps'][0]['worker_pid'];window=state['apps'][0]['windows'][0];screenshot('ssh-app.png')
        action('apps:maximize');time.sleep(.4)
        # Real embedded pointer and key phases into the SSH compositor.
        rc('send-key','ctrl+escape');v=inspect()['viewport'];px=int(v['width']*.48);py=400
        rc('send-text','--stdin',data=f'\x1b[<0;{px};{py}M\x1b[<0;{px};{py}m'.encode());rc('send-key','x')
        wait(lambda:(tmp/'remote-keys').exists(),'SSH app did not receive an embedded key')
        action('apps:reconnect');state=wait(lambda:inspect() if inspect().get('apps') and inspect()['apps'][0]['connected'] and inspect()['apps'][0]['windows'] else None,'App reconnect failed')
        assert state['apps'][0]['worker_pid']==remote_worker,'Reconnect restarted remote compositor'
        assert state['apps'][0]['windows'][0]['id']==window['id'],'Reconnect restarted app window'
        action('project:'+local);wait(lambda:inspect()['active_project']==local,'Local project return failed')
        assert [p['pid']for p in inspect()['panes']]==pids
        action('project:'+remote);wait(lambda:inspect()['active_project']==remote,'Remote project return failed')
        assert [p['pid']for p in inspect()['panes']]==remote_pids
        invoke(local,'--close');time.sleep(.6)
        # Relaunch the same controller and return to the same project app desktop.
        rc('launch','--type=tab',str(app),'--workspace',local);wait(lambda:inspect()['controller_pid']!=state['controller_pid'],'Workspace did not restart')
        action('project:'+remote);state=wait(lambda:inspect() if inspect()['active_project']==remote and inspect().get('apps') and inspect()['apps'][0]['connected'] and inspect()['apps'][0]['windows'] else None,'Persistent remote app did not reattach')
        assert state['apps'][0]['worker_pid']==remote_worker
        assert state['apps'][0]['windows'][0]['id']==window['id']
        result={'passed':True,'renderer':os.environ.get('STAR_GRAPHICS_RENDERER','cpu'),'project_container':True,'new_project_dialog_keyboard_and_paste':True,'terminal_tabs_and_explicit_split':True,'agent_pids_preserved':pids,'remote_terminal_pids_preserved':remote_pids,'local_app_worker':local_worker,'ssh_app_worker':remote_worker,'ssh_app_window':window,'supporting_app':True,'app_input_over_ssh':True,'remote_compositor_and_app_survive_reconnect_and_workspace_close':True,'ssh_scope':'authenticated loopback SSH'}
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    except BaseException:
        try:(out/'failure-state.json').write_text(json.dumps(inspect(),indent=2))
        except Exception:pass
        if os.environ.get('STARFLUX_PROOF_DEBUG'):time.sleep(50)
        raise
    finally:
        for name in names:
            try:invoke(name,'--stop')
            except Exception as error:print('Cleanup',name,str(error))
        for child in reversed(children):
            if child.poll() is None:child.terminate();child.wait(timeout=8)
        if original:subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{original}"}}))'],stdout=subprocess.DEVNULL)
