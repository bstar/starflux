#!/usr/bin/env python3
"""Registered application catalog and launch checks using disposable local/SSH desktops."""
import getpass,json,os,pathlib,shlex,socket,subprocess,tempfile,time,uuid
repo=pathlib.Path(__file__).resolve().parents[1];app=repo/'scripts/run-local.sh';runtime=pathlib.Path(os.environ['XDG_RUNTIME_DIR'])/'starflux'
out=repo/'target/app-picker-proof';out.mkdir(parents=True,exist_ok=True)
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
        xdg=tmp/'data';entries=xdg/'applications';entries.mkdir(parents=True)
        fixture=entries/'starflux-picker-proof.desktop'
        fixture.write_text('[Desktop Entry]\nType=Application\nName=Registered Picker Proof\nComment=Disposable registered GTK test\nExec=/usr/bin/python3 "'+str(gui)+'" "Registered Picker Proof" "'+str(tmp/'registered-keys')+'" %U\nTerminal=false\nDBusActivatable=true\n')
        hidden=entries/'starflux-picker-hidden.desktop';hidden.write_text('[Desktop Entry]\nType=Application\nName=Hidden Picker Proof\nExec=/usr/bin/true\nNoDisplay=true\n')
        os.environ['XDG_DATA_HOME']=str(xdg)
        helper=tmp/'remote-helper';helper.write_text('#!/bin/bash\nexport XDG_DATA_HOME='+shlex.quote(str(xdg))+'\nexec '+shlex.quote(str(app))+' "$@"\n');helper.chmod(0o700)
        remote_config['desktop_helper']=str(helper);(runtime/(remote+'.json')).write_text(json.dumps(remote_config))
        cmd=['/usr/bin/kitty','--hold','--class','starflux-shelf-proof','--config','NONE','--listen-on',address,'--title','STAR-FLUX-SHELF-PROOF','-o','linux_display_server=wayland','-o','allow_remote_control=yes','-o','initial_window_width=1450','-o','initial_window_height=1000',str(app),'--workspace',local,'--agent','bash --noprofile --norc','--directory',str(tmp),'--app',shlex.join(['/usr/bin/python3',str(gui),'Project app',str(tmp/'local-keys')])]
        kitty=subprocess.Popen(cmd,stdout=subprocess.DEVNULL,stderr=(out/'kitty.log').open('w'));children.append(kitty)
        wait(lambda:inspect() if inspect().get('sessions') else None,'Workspace not ready')
        action('launch-app')
        wait(lambda:(inspect().get('application_picker') or {}).get('count',0)>0,'Application catalog did not load')
        picker=inspect()['application_picker']
        assert all(a['name']!='Hidden Picker Proof' for a in picker['matches'])
        rc('send-text','--stdin','--bracketed-paste','enable',data=b'Registered Picker Proof')
        wait(lambda:inspect()['application_picker']['query']=='Registered Picker Proof','Search input missing')
        wait(lambda:len(inspect()['application_picker']['matches'])==1,'Search did not filter')
        screenshot('picker.png')
        rc('send-key','enter')
        wait(lambda:inspect() if inspect().get('apps') and inspect()['apps'][0]['windows'] else None,'Registered app did not open in virtual desktop')
        assert inspect()['apps'][0]['windows'][0]['name']=='Registered Picker Proof'
        screenshot('registered-app.png')
        # Query and launch the same registered fixture through a real SSH helper.
        helper=tmp/'remote-helper';helper.write_text('#!/bin/bash\nexport XDG_DATA_HOME='+shlex.quote(str(xdg))+'\nexec '+shlex.quote(str(app))+' "$@"\n');helper.chmod(0o700)
        remote_config['desktop_helper']=str(helper);(runtime/(remote+'.json')).write_text(json.dumps(remote_config))
        action('project:'+remote)
        wait(lambda:inspect()['active_project']==remote,'SSH project did not switch')
        action('launch-app')
        wait(lambda:(inspect().get('application_picker') or {}).get('count',0)>0,'SSH catalog did not load')
        rc('send-text','--stdin','--bracketed-paste','enable',data=b'Registered Picker Proof')
        wait(lambda:len(inspect()['application_picker']['matches'])==1,'SSH catalog filter failed')
        rc('send-key','enter')
        wait(lambda:inspect() if inspect().get('apps') and inspect()['apps'][0]['windows'] else None,'SSH registered app did not open')
        (out/'result.json').write_text(json.dumps({'passed':True,'local_catalog_count':picker['count'],'local_and_ssh_registered_launch':True,'hidden_entries_filtered':True,'query_paste_and_enter':True,'exec_file_placeholder_expanded':True},indent=2))
        print('PASS: local and SSH app catalogs, search, registered launch and hidden entries')
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
