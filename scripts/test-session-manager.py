#!/usr/bin/env python3
"""Private sessions, independent views, live tmux/Herdr imports and scoped deletion."""
import argparse
import base64
import fcntl
import getpass
import hashlib
import json
import os
from pathlib import Path
import queue
import pty
import secrets
import signal
import shlex
import shutil
import socket
import subprocess
import struct
import termios
import tempfile
import threading
import time

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--ssh',action='store_true')
parser.add_argument('--theme-settings',action='store_true')
parser.add_argument('--cells',action='store_true')
parser.add_argument('--herdr',action='store_true')
parser.add_argument('--keyboard-only',action='store_true')
parser.add_argument('--agent-lifecycle',action='store_true')
parser.add_argument('--pane-layouts',action='store_true')
parser.add_argument('--terminal-history',action='store_true')
parser.add_argument('--automation',action='store_true')
parser.add_argument('--worktrees',action='store_true')
parser.add_argument('--machines',action='store_true')
parser.add_argument('--notifications',action='store_true')
parser.add_argument('--narrow',action='store_true')
parser.add_argument('--cold-start',action='store_true')
parser.add_argument('--reboot-adopted',action='store_true')
parser.add_argument('--startup-retry',action='store_true')
parser.add_argument('--panel-focus',action='store_true')
parser.add_argument('--categories',action='store_true')
parser.add_argument('--category-agent',action='store_true')
parser.add_argument('--category-agent-workflow',action='store_true')
parser.add_argument('--category-agent-selection',action='store_true')
parser.add_argument('--plugin-ui',action='store_true')
parser.add_argument('--plugin-panes-ui',action='store_true')
parser.add_argument('--plugin-pane-upgrade',action='store_true')
parser.add_argument('--plugin-popups-ui',action='store_true')
parser.add_argument('--plugin-popups-managed',action='store_true')
parser.add_argument('--plugin-popup-keyboard',action='store_true')
parser.add_argument('--background-screens',action='store_true')
parser.add_argument('--snapshot-service',action='store_true')
parser.add_argument('--query-upgrade',action='store_true')
parser.add_argument('--frontend-commands',action='store_true')
parser.add_argument('--notification-service',action='store_true')
args=parser.parse_args()
if args.plugin_pane_upgrade:args.plugin_panes_ui=True
if args.plugin_popups_managed:args.plugin_popups_ui=True
if args.plugin_popup_keyboard:args.plugin_popups_ui=True
if args.reboot_adopted:args.cold_start=True
if args.category_agent_selection:args.category_agent_workflow=True
if args.category_agent_workflow:args.category_agent=True;args.agent_lifecycle=True
if args.notification_service and not args.notifications:parser.error("--notification-service requires --notifications")
root=Path(__file__).resolve().parents[1];app=root/'scripts/run-local.sh'
with tempfile.TemporaryDirectory(prefix='starflux-session-proof-') as directory:
    tmp=Path(directory);runtime=tmp/'runtime';runtime.mkdir(mode=0o700);state=runtime/'starflux';state.mkdir(mode=0o700)
    name='sessions-'+secrets.token_hex(4)
    shell=tmp/'fake-shell'
    shell.write_text("#!/usr/bin/python3\nimport os,sys\nprint('SESSION_PROOF_PID='+str(os.getpid()),flush=True)\nfor line in sys.stdin:print('REPLY='+line.strip(),flush=True)\n")
    if args.plugin_panes_ui:
        # tmux launches explicit argv through its default shell with -c. The
        # fake shell must execute that mode instead of swallowing the command.
        shell.write_text("#!/usr/bin/python3\nimport os,sys\nif len(sys.argv)>1 and sys.argv[1]=='-c':os.execv('/bin/sh',['sh',*sys.argv[1:]])\nprint('SESSION_PROOF_PID='+str(os.getpid()),flush=True)\nfor line in sys.stdin:print('REPLY='+line.strip(),flush=True)\n")
    if args.cold_start:
        shell.write_text("#!/usr/bin/python3\nimport ctypes,os,sys\nif len(sys.argv)>1 and sys.argv[1]=='-c':os.execv('/bin/sh',['sh',*sys.argv[1:]])\nctypes.CDLL(None).prctl(15,ctypes.c_char_p(b'codex'),0,0,0)\nprint('\\x1b[1;34mCOLD_SAVED café 界\\x1b[0m',flush=True)\nif 'cold-exact-thread' in sys.argv:print('RESUMED_COLD_EXACT_THREAD',flush=True)\nfor line in sys.stdin:print('REPLY='+line.strip(),flush=True)\n")
    if args.panel_focus:
        shell.write_text('#!/usr/bin/python3\nimport os,tty\nfrom pathlib import Path\ntty.setraw(0)\nprint("PANEL_READY",flush=True)\nwhile True:\n data=os.read(0,4096)\n if not data:break\n with Path('+repr(str(tmp/'panel-input'))+').open("ab") as out:out.write(data)\n')
    if args.agent_lifecycle or args.notifications:
        shell.write_text("""#!/usr/bin/python3
import ctypes,os,sys
if len(sys.argv)>1 and sys.argv[1]=='-c':os.execv('/bin/sh',['sh',*sys.argv[1:]])
ctypes.CDLL(None).prctl(15,ctypes.c_char_p(b'codex'),0,0,0)
print('SESSION_PROOF_PID='+str(os.getpid()),flush=True)
if 'exact-thread' in sys.argv:print('RESUMED_EXACT_THREAD',flush=True)
for line in sys.stdin:
 if line.strip()=='FAIL':print('\\x1b[2J\\x1b[H■ Server connection could not be restored',flush=True)
 else:print('REPLY='+line.strip(),flush=True)
""")
    if args.terminal_history:
        shell.write_text("""#!/usr/bin/python3
import os,sys
print('SESSION_PROOF_PID='+str(os.getpid()),flush=True)
for line in sys.stdin:
 if line.strip()=='HISTORY':
  for i in range(120):print(f'ROW {i:03} '+('NEEDLE 界é' if i in (7,75) else 'history text'),flush=True)
 else:print('REPLY='+line.strip(),flush=True)
""")
    shell.chmod(0o700)
    if args.plugin_ui:
        hidden='https://example.invalid/hidden'+(';part'*20)+'?q=destination#OSC8'
        text=shell.read_text();text=text.replace("for line in sys.stdin:","print("+repr('\x1b]8;id=fixture;'+hidden+'\x1b\\OSC8_LABEL_界é\x1b]8;;\x1b\\')+",flush=True)\nfor line in sys.stdin:")
        shell.write_text(text)
    env={k:v for k,v in os.environ.items() if not k.startswith('HERDR_') and k not in ('TMUX','SSH_CONNECTION','SSH_TTY')}
    env.update(XDG_RUNTIME_DIR=str(runtime),XDG_DATA_HOME=str(tmp/'data'),XDG_CONFIG_HOME=str(tmp/'config'),TMUX_TMPDIR=str(tmp/'tmux'),SHELL=str(shell),STAR_TERMINAL_RENDERER='native',STARFLUX_HERDR='herdr' if args.herdr else str(tmp/'herdr-not-installed'))
    if args.plugin_popups_ui:
        env.update(STARFLUX_PANE='32',STARFLUX_PLUGIN_PANE_ID='inherited-pane',STARFLUX_PLUGIN_JOB_ID='inherited-job')
    if args.notifications:
        bindir=tmp/'bin';bindir.mkdir()
        helper=bindir/'notify-send'
        helper.write_text('#!/usr/bin/python3\nimport json,sys\nfrom pathlib import Path\nwith Path('+repr(str(tmp/'desktop-notifications'))+').open("a") as f:f.write(json.dumps(sys.argv[1:])+"\\n")\nraise SystemExit(1 if Path('+repr(str(tmp/'desktop-fail'))+').exists() else 0)\n');helper.chmod(0o700)
        if args.notification_service:
            real_notify=shutil.which('notify-send');assert real_notify,'notify-send is required for the native service check'
            helper.write_text('#!/usr/bin/python3\nimport json,sys,os,subprocess\nfrom pathlib import Path\nwith Path('+repr(str(tmp/'desktop-notifications'))+').open("a") as f:f.write(json.dumps(sys.argv[1:])+"\\n")\nif Path('+repr(str(tmp/'desktop-fail'))+').exists():raise SystemExit(1)\nenv=dict(os.environ)\nraise SystemExit(subprocess.call(['+repr(real_notify)+',*sys.argv[1:]],env=env))\n')
        env.update(PATH=str(bindir)+':'+env['PATH'],DBUS_SESSION_BUS_ADDRESS='unix:path='+str(tmp/('bus' if args.notification_service else 'fake-bus')),WAYLAND_DISPLAY='fixture-display')
    project_directory=tmp
    if args.worktrees or args.narrow:
        project_directory=tmp/'repository';project_directory.mkdir()
        env.update(GIT_CONFIG_GLOBAL='/dev/null',GIT_CONFIG_SYSTEM='/dev/null')
        for command in [('init','-b','main'),('config','user.name','Private fixture'),('config','user.email','fixture@example.invalid')]:subprocess.run(['git','-C',str(project_directory),*command],env=env,check=True,capture_output=True)
        (project_directory/'source').write_text('source unchanged\n')
        subprocess.run(['git','-C',str(project_directory),'add','source'],env=env,check=True)
        subprocess.run(['git','-C',str(project_directory),'commit','-m','initial'],env=env,check=True,capture_output=True)
        if args.narrow:
            for i in range(16):(project_directory/f'narrow-file-{i:02}.txt').write_text('private fixture\n')
    if args.startup_retry:
        retry_bin=tmp/'retry-bin';retry_bin.mkdir();real_tmux=shutil.which('tmux')
        retry_ready=tmp/'backend-ready'
        retry_wrapper=retry_bin/'tmux'
        retry_wrapper.write_text('#!/usr/bin/python3\nimport os,sys,pathlib\nargs=sys.argv[1:]\nif "-L" in args and args[args.index("-L")+1]=='+repr('starflux-'+name)+' and "new-session" in args and not pathlib.Path('+repr(str(retry_ready))+').exists():\n print("fixture backend not ready",file=sys.stderr);sys.exit(75)\nos.execv('+repr(real_tmux)+',['+repr(real_tmux)+',*args])\n')
        retry_wrapper.chmod(0o700);env['PATH']=str(retry_bin)+os.pathsep+env['PATH']
    (state/(name+'.json')).write_text(json.dumps(dict(workspace=name,label='Original',collection='base',host=None,directory=str(project_directory),agent=None,executable=str(app),project_host=True)))
    for extra in ['extra-a','extra-b']:
        (state/(extra+'.json')).write_text(json.dumps(dict(workspace=extra,label=extra,collection='base',host=None,directory=str(tmp),agent=None,executable=str(app),project_host=True)))
    processes=[];clients=[];daemon=None;herdr=None;ssh_args=[]
    socket_root=tmp/'tmux'/('tmux-'+str(os.getuid()));socket_root.mkdir(parents=True,mode=0o700);source_socket=socket_root/'fixture'
    def tmux(*arguments):
        return subprocess.check_output(['tmux','-S',str(source_socket),*arguments],env=env,text=True).strip()
    def alive(pid):
        try:return Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()[0]!='Z'
        except OSError:return False
    class Client:
        def __init__(self,identity,remote=False,executable=None):
            self.executable=executable;self.identity=identity;self.scene=None;self.messages=queue.Queue();self.serial=0;self.frames={};self.clipboards=[];self.all_frames=[]
            command=[str(executable or app),'--graphical-relay',name+'@'+identity]
            if remote:command=ssh_args+[shlex.join(['env',*[k+'='+v for k,v in env.items() if k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','STAR_TERMINAL_RENDERER','STARFLUX_HERDR','HERDR_CONFIG_PATH','PATH')],*command])]
            self.log=(tmp/(identity+'.log')).open('a')
            self.process=subprocess.Popen(command,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log);processes.append(self.process);clients.append(self)
            def read():
                for line in self.process.stdout:
                    try:self.messages.put(json.loads(line))
                    except ValueError:self.messages.put(dict(type='invalid',line=repr(line)))
            threading.Thread(target=read,daemon=True).start()
            self.send(dict(type='hello',version=1,client=identity,viewport=dict(columns=100,rows=35,width=1000,height=700,generation=1),capabilities=dict(image_transport="kitty",pixel_geometry="measured",pointer_precision="cells",native_terminal_cells=True,cell_presentation=args.cells,native_surfaces=True,pixel_layout=not args.cells,physical_text=True,keyboard=True,paste=True,presentation_ack=True)))
        def send(self,message):
            self.process.stdin.write(json.dumps(message).encode()+b'\n');self.process.stdin.flush()
        def pump(self):
            while not self.messages.empty():
                message=self.messages.get_nowait()
                assert message['type'] not in ('error','invalid'),message
                if message['type']=='scene':
                    self.scene=message['scene'];self.send(dict(type='presented',revision=self.scene['revision'],generation=self.scene['viewport']['generation']))
                if message['type']=='terminal_cells':
                    stream=message['frame']['stream']
                    if '-p' in stream and stream.rsplit('-p',1)[1].isdigit():self.frames[int(stream.rsplit('-p',1)[1])]=message['frame']
                    if args.cold_start or args.plugin_ui or args.plugin_panes_ui or args.plugin_popups_ui:self.all_frames=(self.all_frames+[message['frame']])[-100:]
                if message['type']=='clipboard':self.clipboards.append(message['text'])
        def snapshot(self):
            try:return json.loads((state/(self.identity+'.state.json')).read_text())
            except (OSError,ValueError):return {}
        def input(self,value):
            self.serial+=1;self.send(dict(type='input',id=self.serial,revision=self.scene['revision'],generation=self.scene['viewport']['generation'],input=value))
        def resize(self,columns,rows,width,height):
            generation=self.scene['viewport']['generation']+1
            self.input(dict(kind='resize',viewport=dict(columns=columns,rows=rows,width=width,height=height,generation=generation)))
            wait(lambda:self.scene['viewport']['generation']==generation and self.snapshot().get('viewport',{}).get('generation')==generation,'viewport did not resize')
        def action(self,value):
            subprocess.run([str(app),'--workspace',name,'--frontend',self.identity,'--action',value],env=env,check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        def paste(self,text):self.input(dict(kind='paste',text=text))
        def accept(self):self.input(dict(kind='key',code='enter',modifiers=2))
        def key(self,code,modifiers=0):
            if args.cells or any(self.snapshot().get(k) for k in ('modal_open','context_menu','git_open')):self.input(dict(kind='key',code=code,modifiers=modifiers))
            else:
                for phase in ['press','release']:
                    self.input(dict(kind='embedded',canvas='terminals',event=dict(timestamp_ns=time.monotonic_ns(),sequence=self.serial+1,kind=dict(kind='key',code=code,modifiers=modifiers,phase=phase))))
        def shortcut(self,code,modifiers=0):
            self.key('char:b',2);self.key(code,modifiers)
        def pointer(self,action,x,y,button=0):
            if args.cells:self.input(dict(kind='pointer',action=action,button=button,modifiers=0,x=int(x),y=int(y)))
            else:self.input(dict(kind='embedded',canvas='terminals',event=dict(timestamp_ns=time.monotonic_ns(),sequence=self.serial+1,kind=dict(kind='pointer',action=action,button=button,position=[x,y],modifiers=0))))
    def wait(predicate,label,timeout=15):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            for client in clients:
                if client.process.poll() is None:client.pump()
            value=predicate()
            if value:return value
            time.sleep(.02)
        logs='\n'.join(p.read_text()[-1200:] for p in [*tmp.glob('v-*.log'),*state.glob('v-*.log')])
        logs+=repr([(c.identity,c.process.poll(),{k:c.snapshot().get(k) for k in ('active_project','session_id','status','session_manager','terminal_review')}) for c in clients])
        raise AssertionError(label+' '+logs)
    def picker(client,external=False):
        if client.snapshot().get('modal_open'):
            client.action('modal:cancel')
            wait(lambda:not client.snapshot().get('modal_open'),'previous dialog did not close')
        client.action('sessions:import' if external else 'sessions:open')
        return wait(lambda:(p if (p:=client.snapshot().get('session_manager')) and p['external']==external and not p['status'].startswith('Loading') else None),'session list did not load')
    def row(client,label):
        manager=client.snapshot()['session_manager']
        matches=[r for r in manager['rows'] if r[1].removeprefix('● ')==label]
        assert matches,(label,manager)
        found=matches[0]
        client.action('sessions:row:'+str(found[0]))
        position=manager['rows'].index(found)
        wait(lambda:client.snapshot()['session_manager']['selected']==position,'session row did not select')
        return found
    def choose(client,label):
        row(client,label);client.action('sessions:choose')
        wait(lambda:not client.snapshot().get('modal_open'),'session did not open')
    def delete(client,label):
        picker(client);row(client,label);client.action('sessions:delete')
        wait(lambda:client.snapshot().get('modal_open') and client.snapshot().get('session_manager') is None,'delete confirmation missing')
        client.paste('DELETE');client.accept()
        wait(lambda:(m:=client.snapshot().get('session_manager')) and not m['status'].startswith('Loading') and not any(r[1].removeprefix('● ')==label for r in m['rows']),'deleted session remained in list')
    def restart(client):
        subprocess.run([str(app),'--workspace',name,'--frontend',client.identity,'--close'],env=env,check=True,capture_output=True)
        client.process.wait(timeout=5)
        (state/(client.identity+'.state.json')).unlink(missing_ok=True)
        resumed=Client(client.identity,remote=args.ssh,executable=client.executable)
        wait(lambda:resumed.scene and resumed.snapshot().get('panes') and resumed.snapshot()['panes'][0].get('pid'),'view did not resume')
        return resumed
    def check_startup(expected,expected_pid,explicit=False,machine=None):
        identity='v-startup-explicit' if explicit else 'v-startup-default'
        command=[str(app),'--cells','--frontend',identity]
        if explicit:command+=['--workspace',name]
        if args.ssh and not machine:
            wrapper=tmp/'remote-starflux'
            wrapper.write_text('#!/bin/sh\nexec '+shlex.join(['env',*[k+'='+v for k,v in env.items() if k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','STAR_TERMINAL_RENDERER','STARFLUX_HERDR','HERDR_CONFIG_PATH','PATH')],str(app)])+' "$@"\n');wrapper.chmod(0o700)
            config=tmp/'startup-ssh-config'
            config.write_text(f'Host proof\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {tmp}/client\n IdentitiesOnly yes\n BatchMode yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {tmp}/known_hosts\n')
            command+=['--ssh','proof','--ssh-config',str(config),'--remote-executable',str(wrapper)]
        if machine:command += ["--machine",machine]
        (state/(identity+'.state.json')).unlink(missing_ok=True)
        master,slave=pty.openpty()
        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',35,100,0,0))
        process=subprocess.Popen(command,stdin=slave,stdout=slave,stderr=(tmp/(identity+'.log')).open('w'),env={**env,'TERM':'xterm-256color'},start_new_session=True)
        processes.append(process);os.close(slave)
        def drain():
            try:
                while os.read(master,65536):pass
            except OSError:pass
        threading.Thread(target=drain,daemon=True).start()
        try:
            def loaded():
                try:
                    snapshot=json.loads((state/(identity+'.state.json')).read_text())
                    return snapshot if snapshot.get('panes') and snapshot['panes'][0].get('pid') else None
                except (OSError,ValueError):return None
            snapshot=wait(loaded,'default startup failed')
            assert snapshot['active_project']==expected,(snapshot['active_project'],expected,command,subprocess.check_output([str(app),'--default-workspace'],env=env,text=True).strip(),(state/(identity+'.frontend.json')).read_text(),snapshot['workspace'])
            assert snapshot['panes'][0]['pid']==expected_pid,'Startup replaced the existing terminal process'
            subprocess.run([str(app),'--workspace',expected,'--frontend',identity,'--close'],env=env,check=True,capture_output=True)
            process.wait(timeout=5)
        finally:
            if process.poll() is None:process.terminate();process.wait(timeout=3)
            os.close(master)
    try:
        if args.notification_service:
            daemon_env=dict(env);daemon_env.pop('LD_LIBRARY_PATH',None);daemon_env.pop('GI_TYPELIB_PATH',None)
            bus_config=tmp/'dbus-fixture.conf'
            bus_config.write_text('<busconfig><type>session</type><listen>'+env['DBUS_SESSION_BUS_ADDRESS']+'</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
            bus=subprocess.Popen(['dbus-daemon','--nofork','--config-file='+str(bus_config)],env=daemon_env,stdout=subprocess.DEVNULL,stderr=(tmp/'dbus.log').open('w'));processes.append(bus)
            wait(lambda:(tmp/'bus').exists(),'private D-Bus did not start')
            service=tmp/'notification-service.py'
            service.write_text("""import json
from pathlib import Path
from gi.repository import Gio,GLib
root=Path(__file__).parent
xml='<node><interface name="org.freedesktop.Notifications"><method name="Notify"><arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/><arg type="a{sv}" direction="in"/><arg type="i" direction="in"/><arg type="u" direction="out"/></method><method name="GetCapabilities"><arg type="as" direction="out"/></method><method name="GetServerInformation"><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/></method></interface></node>'
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
info=Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0]
counter=0
def call(connection,sender,path,interface,method,params,invocation):
 global counter
 if method=='Notify':
  values=params.unpack();counter+=1
  with (root/'native-notifications').open('a') as f:f.write(json.dumps(dict(app=values[0],title=values[3],body=values[4]))+'\\n')
  invocation.return_value(GLib.Variant('(u)',(counter,)))
 elif method=='GetCapabilities':invocation.return_value(GLib.Variant('(as)',(['body'],)))
 elif method=='GetServerInformation':invocation.return_value(GLib.Variant('(ssss)',('Private notification fixture','STAR/FLUX','1','1.2')))
bus.register_object('/org/freedesktop/Notifications',info,call,None,None)
bus.call_sync('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','RequestName',GLib.Variant('(su)',('org.freedesktop.Notifications',0)),GLib.VariantType.new('(u)'),Gio.DBusCallFlags.NONE,3000,None)
(root/'notification-service-ready').touch()
GLib.MainLoop().run()
""")
            worker=subprocess.Popen(['/usr/bin/python3',str(service)],env=daemon_env,stderr=(tmp/'notification-service.log').open('w'));processes.append(worker)
            wait(lambda:(tmp/'notification-service-ready').exists() or worker.poll() is not None,'notification service did not register')
            assert worker.poll() is None,(tmp/'notification-service.log').read_text()
        if args.ssh or args.machines:
            for key in ('host','client'):subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(tmp/key)],check=True)
            (tmp/'authorized_keys').write_text((tmp/'client.pub').read_text())
            with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
            (tmp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(tmp/'host.pub').read_text())
            (tmp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAcceptEnv XDG_RUNTIME_DIR XDG_DATA_HOME XDG_CONFIG_HOME TMUX_TMPDIR SHELL GIT_CONFIG_GLOBAL GIT_CONFIG_SYSTEM\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
            daemon=subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd','-D','-e','-f',str(tmp/'sshd_config')],stderr=(tmp/'sshd.log').open('w'))
            deadline=time.monotonic()+5
            while True:
                try:
                    with socket.create_connection(('127.0.0.1',port),timeout=.1):break
                except OSError:
                    assert daemon.poll() is None and time.monotonic()<deadline;time.sleep(.02)
            ssh_args=['ssh','-F','/dev/null','-T','-p',str(port),'-i',str(tmp/'client'),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(tmp/'known_hosts'),'127.0.0.1']
            if args.worktrees or args.machines:
                host_config=tmp/'worktree-ssh-config'
                host_config.write_text(f'Host proof\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {tmp}/client\n IdentitiesOnly yes\n BatchMode yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {tmp}/known_hosts\n SetEnv '+ ' '.join(k+'='+env[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','GIT_CONFIG_GLOBAL','GIT_CONFIG_SYSTEM') if k in env)+'\n')
                if args.worktrees:
                    config=json.loads((state/(name+'.json')).read_text());config.update(host='proof',ssh_config=str(host_config));(state/(name+'.json')).write_text(json.dumps(config))
        tmux('new-session','-d','-s','Fixture','-c',str(tmp),str(shell))
        tmux('new-window','-d','-t','Fixture','-c',str(tmp),str(shell))
        source_pid=int(tmux('display-message','-p','-t','Fixture','#{pane_pid}'))
        source_topology=tmux('list-panes','-s','-t','Fixture','-F','#{pane_id} #{pane_pid}')
        if args.herdr:
            config=tmp/'config/herdr';config.mkdir(parents=True);(config/'config.toml').write_text('onboarding = false\n');env['HERDR_CONFIG_PATH']=str(config/'config.toml')
            herdr=subprocess.Popen(['herdr','server'],env=env,stdout=(tmp/'herdr.log').open('w'),stderr=subprocess.STDOUT)
            wait(lambda:(config/'herdr.sock').exists(),'private Herdr server missing')
            def herdr_cli(*arguments):return json.loads(subprocess.check_output(['herdr',*arguments],env=env))
            herdr_cli('workspace','create','--cwd',str(tmp),'--label','Herdr Proof One','--no-focus')
            herdr_cli('workspace','create','--cwd',str(tmp),'--label','Herdr Proof Two','--no-focus')
            snapshot=herdr_cli('api','snapshot')['result']['snapshot']
            herdr_pids=[herdr_cli('pane','process-info','--pane',p['pane_id'])['result']['process_info']['shell_pid'] for p in snapshot['panes']]
            wait(lambda:(config/'session.json').exists(),'private Herdr layout missing')
        copied=None;entrypoint=None
        if args.query_upgrade or args.plugin_pane_upgrade:
            copied=tmp/'loaded-build';shutil.copy2(root/'target/release/starflux',copied)
            entrypoint=tmp/'run-loaded';entrypoint.write_text('#!/bin/bash\nsource '+shlex.quote(str(root/'target/run-env.sh'))+'\nexec '+shlex.quote(str(copied))+' "$@"\n');entrypoint.chmod(0o700)
        a=Client('v-session-a',remote=args.ssh,executable=entrypoint);b=Client('v-session-b')
        if args.startup_retry:
            wait(lambda:'reconnecting' in a.snapshot().get('status','') and 'reconnecting' in b.snapshot().get('status',''),'startup failure did not expose reconnecting state')
            retry_ready.touch()
        wait(lambda:a.scene and b.scene and a.snapshot().get('panes') and a.snapshot()['panes'][0].get('pid'),'initial views missing')
        original_pid=a.snapshot()['panes'][0]['pid']
        if args.theme_settings:
            def appearance_is(theme):
                return all(c.snapshot().get('appearance',{}).get('id')==theme for c in (a,b))
            wait(lambda:appearance_is('system'),'system must be the application default')
            # Reproduce a stale per-project theme left by older versions.
            for directory in (state,tmp/'data/starflux/projects'):
                path=directory/(name+'.json')
                if path.exists():
                    value=json.loads(path.read_text());value['theme']='kanagawa';path.write_text(json.dumps(value))
            a.action('rename-project:'+name)
            wait(lambda:a.snapshot().get('modal_open'),'rename prompt missing')
            a.key('char:e',2);a.key('char:u',2);a.paste('Renamed theme fixture');a.accept()
            wait(lambda:a.snapshot().get('project_label')=='Renamed theme fixture','rename did not save')
            time.sleep(.7);a.pump();b.pump()
            assert appearance_is('system'),'rename restored stale project palette'
            # Explicit theme choice is global, durable, and reaches both views.
            for theme in ('kanagawa','system'):
                subprocess.run([str(app),'--workspace',name,'--theme',theme],env=env,check=True,capture_output=True)
                wait(lambda:appearance_is(theme),'global theme did not reach both views')
                assert json.loads((tmp/'config/starflux/config.json').read_text())['theme']==theme
            a.action('sessions:new');wait(lambda:a.snapshot().get('modal_open'),'session prompt missing')
            a.paste('Theme session');a.accept()
            wait(lambda:a.snapshot().get('session_id')!='base' and not a.snapshot().get('modal_open'),'new session missing')
            assert appearance_is('system'),'new session changed application theme'
            picker(a);choose(a,'base')
            wait(lambda:a.snapshot().get('active_project')==name,'return to original session failed')
            assert appearance_is('system') and alive(original_pid),'session switch changed theme or stopped source'
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,global_theme=True,rename_preserves_theme=True,session_switch_preserves_theme=True,live_theme_changes=True,processes_preserved=True)))
            raise SystemExit(0)
        if args.startup_retry:
            wait(lambda:a.snapshot().get('direct_terminals'),'retried terminal did not attach')
            a.paste('AFTER_STARTUP_RETRY');a.key('enter')
            wait(lambda:'REPLY=AFTER_STARTUP_RETRY' in a.snapshot()['direct_terminals'][0]['text'],'retried terminal input did not work')
            assert alive(source_pid),'retry changed an unrelated terminal'
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,startup_failure_visible=True,automatic_retry=True,input_restored=True,source_preserved=True)))
            raise SystemExit(0)
        if args.plugin_popups_ui:
            exec(compile((root/'scripts/test-plugin-popups-ui.py').read_text(),str(root/'scripts/test-plugin-popups-ui.py'),'exec'),globals())
            raise SystemExit(0)
        if args.plugin_panes_ui:
            exec(compile((root/'scripts/test-plugin-panes-ui.py').read_text(),str(root/'scripts/test-plugin-panes-ui.py'),'exec'),globals())
            raise SystemExit(0)
        if args.plugin_ui:
            import runpy
            runpy.run_path(str(root/'scripts/test-plugin-ui.py'),init_globals=dict(args=args,app=app,tmp=tmp,root=root,name=name,a=a,b=b,wait=wait,env=env,alive=alive,original_pid=original_pid,ssh_args=ssh_args))
            raise SystemExit(0)
        if args.categories or args.category_agent:
            def cat(client=None):return (client or a).snapshot().get('categories',{})
            def groups(client=None):return cat(client).get('catalog',{}).get('groups',[])
            # Right-click blank sidebar space; Create category is not a top link.
            if args.cells:a.input(dict(kind='pointer',action='down',button=1,modifiers=0,x=5,y=a.snapshot()['viewport']['rows']-4))
            else:a.input(dict(kind='pointer',action='down',button=1,modifiers=0,x=9,y=4,pixel=[90,96]))
            wait(lambda:(a.snapshot().get('context_menu') or {}).get('title')=='Sidebar options','blank sidebar did not open global options')
            actions=[i['action'] for i in a.snapshot()['context_menu']['items']]
            assert all(k in actions for k in ['categories:new','project:new','categories:manage','categories:expand-all','categories:collapse-all'])
            a.key('enter');wait(lambda:a.snapshot().get('modal_open'),'category prompt missing');a.paste('Work 界');a.accept()
            group=wait(lambda:next((g for g in groups() if g['name']=='Work 界'),None),'category did not save');gid=group['id']
            wait(lambda:any(g['id']==gid for g in groups(b)),'category did not sync to other window')
            a.action('categories:assign:'+name+':'+gid);wait(lambda:cat().get('catalog',{}).get('assignments',{}).get(name)==gid,'project assignment failed')
            if args.category_agent:
                def api(method,params,write=False,ok=True):
                    request=dict(version=1,id='category-agent-proof',method=method,params=params)
                    if write:request['operation_id']=secrets.token_hex(16)
                    command=[str(app),'--api',json.dumps(request)]
                    if args.ssh:command=ssh_args+[shlex.join(['env',*[k+'='+env[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','PATH','STAR_TERMINAL_RENDERER','STARFLUX_HERDR') if k in env],*command])]
                    response=json.loads(subprocess.check_output(command,env=env,timeout=15));assert response['ok']==ok,response
                    return response.get('result',response.get('error'))
                base=dict(project=name,category=gid)
                before=api('category_agent.get',base);assert before['state']['profile']['enabled']==False
                api('category_agent.evidence',base,ok=False)
                profile=dict(enabled=True,provider='codex',charter='Compare shared interfaces and report evidence-backed parity gaps.')
                configured=api('category_agent.configure',dict(**base,expected_revision=0,profile=profile),write=True)
                assert configured['state']['revision']==1
                api('category_agent.configure',dict(**base,expected_revision=0,profile=profile),write=True,ok=False)
                evidence=api('category_agent.evidence',base);assert [p['id'] for p in evidence['projects']]==[name]
                (tmp/'review-evidence.txt').write_text('café 界')
                read=dict(**base,member=name,path='review-evidence.txt')
                assert api('category_agent.read_file',read)['text']=='café 界'
                for bad in ['../outside','.git/config','.env','/etc/passwd']:
                    api('category_agent.read_file',dict(read,path=bad),ok=False)
                (tmp/'review-link').symlink_to('review-evidence.txt');api('category_agent.read_file',dict(read,path='review-link'),ok=False)
                api('category_agent.read_file',dict(read,member='extra-a'),ok=False)
                assert 'resume_argv' not in json.dumps(evidence) and evidence['terminal_history_included']==False
                if args.category_agent_workflow:
                    import runpy
                    runpy.run_path(str(root/'scripts/test-category-workflow.py'),init_globals=dict(api=api,base=base,tmp=tmp,root=root,name=name,a=a,b=b,wait=wait,env=env,alive=alive,original_pid=original_pid,gid=gid,groups=groups,cat=cat,args=args))
                current=api('category_agent.get',base)['state']['revision']
                api('category_agent.configure',dict(**base,expected_revision=current,profile=dict(profile,enabled=False)),write=True)
                api('category_agent.read_file',read,ok=False)
                assert alive(original_pid)
                print(json.dumps(dict(result='PASS',ssh=args.ssh,disabled_default=True,revision_guard=True,category_scope=True,unicode=True,no_transcripts=True,symlink_and_secret_exclusion=True,disable_revokes_evidence=True,no_inference_started=not args.category_agent_workflow,no_real_inference_started=True,processes_preserved=True)))
                raise SystemExit(0)
            # Pointer drop onto a category header changes membership without touching the PTY.
            a.action('categories:toggle:uncategorized');wait(lambda:'uncategorized' in cat().get('collapsed',[]),'target category did not collapse')
            rows=cat()['rows'];source_index=next(i for i,r in enumerate(rows) if r['id']==name);target_index=next(i for i,r in enumerate(rows) if r['id']=='category:uncategorized')
            def sidebar_pointer(action,index):
                if args.cells:a.input(dict(kind='pointer',action=action,button=0,modifiers=0,x=5,y=5+index))
                else:
                    current=cat()['rows'];height=lambda r:12 if r['id'].startswith('category-end:') else 44 if r['id'].startswith('category:') else 34
                    y=122+sum(height(r) for r in current[:index])+height(current[index])-3;a.input(dict(kind='pointer',action=action,button=0,modifiers=0,x=10,y=y//20,pixel=[100,y]))
            sidebar_pointer('down',source_index);sidebar_pointer('drag',target_index);sidebar_pointer('up',target_index)
            wait(lambda:name not in cat().get('catalog',{}).get('assignments',{}),'header drop did not move project')
            assert alive(original_pid),'drag stopped terminal'
            a.action('categories:toggle:uncategorized');wait(lambda:any(r['id']==name for r in cat()['rows']),'dropped project did not appear on expansion')
            rows=cat()['rows'];source_index=next(i for i,r in enumerate(rows) if r['id']==name);target_index=next(i for i,r in enumerate(rows) if r['id']=='category:'+gid)
            sidebar_pointer('down',source_index);sidebar_pointer('drag',target_index);sidebar_pointer('up',target_index)
            wait(lambda:cat()['catalog']['assignments'].get(name)==gid,'lower edge of category did not accept project')
            a.key('escape',2);a.key('tab');wait(lambda:a.snapshot().get('panel_focus')=='projects','keyboard did not focus sidebar')
            a.key('enter');wait(lambda:gid in cat().get('collapsed',[]),'keyboard header toggle failed')
            a.key('right');wait(lambda:gid not in cat().get('collapsed',[]),'keyboard expand failed');a.key('f:6')
            # A hidden project remains assigned and counted; revealing it keeps its PTY.
            project_dir=tmp/'data/starflux/projects';original=json.loads((project_dir/(name+'.json')).read_text());extra=dict(original,workspace='category-extra',label='Extra project')
            (project_dir/'category-extra.json').write_text(json.dumps(extra));(project_dir/'catalog-version').write_bytes(os.urandom(16))
            wait(lambda:'category-extra' in a.snapshot().get('project_order',[]),'extra fixture did not appear')
            # The spacer belongs to the preceding category, not the next header.
            for source in ['category-extra',name]:
                rows=cat()['rows'];source_index=next(i for i,r in enumerate(rows) if r['id']==source);target_index=next(i for i,r in enumerate(rows) if r['id']=='category-end:'+gid)
                time.sleep(.3) # The catalogue snapshot precedes the matching presented hit map.
                sidebar_pointer('down',source_index);sidebar_pointer('drag',target_index)
                if not args.cells:wait(lambda:a.snapshot().get('project_navigation',{}).get('dragging')==source,'category padding drag did not start')
                sidebar_pointer('up',target_index)
                try:wait(lambda:cat()['catalog']['assignments'].get(source)==gid and a.snapshot()['project_order'][-1]==source,'category padding did not append project to its own category')
                except AssertionError:
                    print(json.dumps(dict(source=source,categories=cat(),order=a.snapshot()['project_order'],navigation=a.snapshot().get('project_navigation'))),flush=True);raise
            order=a.snapshot()['project_order'];assert order.index('category-extra')<order.index(name),'same-category append did not move project below last child'
            assert alive(original_pid),'category padding drop restarted terminal'
            a.action('categories:assign:category-extra:uncategorized');wait(lambda:'category-extra' not in cat()['catalog']['assignments'],'fixture category restore failed')
            # Drop deep into the empty list area, immediately above Add project.
            rows=cat()['rows'];source_index=next(i for i,r in enumerate(rows) if r['id']==name)
            sidebar_pointer('down',source_index)
            for action in ['drag','up']:
                if args.cells:a.input(dict(kind='pointer',action=action,button=0,modifiers=0,x=5,y=a.snapshot()['viewport']['rows']-4))
                else:
                    y=a.snapshot()['viewport']['height']-68;a.input(dict(kind='pointer',action=action,button=0,modifiers=0,x=10,y=y//20,pixel=[100,y]))
            wait(lambda:cat()['catalog']['assignments'].get(name) is None and a.snapshot()['project_order'][-1]==name,'blank space drop did not append below the last project')
            assert alive(original_pid),'append restarted terminal'
            a.action('categories:assign:'+name+':'+gid);wait(lambda:cat()['catalog']['assignments'].get(name)==gid,'restore category failed')
            a.action('close-project:'+name);wait(lambda:a.snapshot().get('active_project') not in (None,name),'hidden project did not close locally')
            wait(lambda:any(r['id']=='category:'+gid and '1 hidden' in r['name'] for r in cat().get('rows',[])),'hidden project missing from category count')
            assert alive(original_pid),'hiding project killed source'
            a.action('categories:reveal:'+name);wait(lambda:a.snapshot().get('active_project')==name,'reveal did not select hidden project')
            assert any(p.get('pid')==original_pid for p in a.snapshot()['panes']),'reveal restarted source'
            a.action('categories:toggle:'+gid);wait(lambda:gid in cat().get('collapsed',[]),'collapse failed')
            assert gid not in cat(b).get('collapsed',[]),'collapse affected another window'
            assert all(r['id']!=name for r in cat().get('rows',[])),'collapsed project still visible'
            assert any(r['id']=='category:'+gid and '(1)' in r['name'] for r in cat().get('rows',[])),'collapsed count missing'
            assert alive(original_pid),'collapse killed terminal'
            a.action('categories:details:'+gid);wait(lambda:(a.snapshot().get('context_menu') or {}).get('title')=='Portfolio projects','category menu missing')
            assert any(i['action']=='categories:reveal:'+name for i in a.snapshot()['context_menu']['items'])
            a.key('escape');a.action('project:'+name);wait(lambda:gid not in cat().get('collapsed',[]),'project navigation did not expand category')
            a.action('categories:rename:'+gid);wait(lambda:a.snapshot().get('modal_open'),'rename missing');a.key('end');[a.key('backspace') for _ in range(6)];a.paste('Renamed');a.accept()
            wait(lambda:any(g['name']=='Renamed' for g in groups()),'rename failed')
            # An outstanding rename cannot overwrite a category change from another window.
            a.action('categories:rename:'+gid);wait(lambda:a.snapshot().get('modal_open'),'stale rename missing')
            b.action('categories:new');wait(lambda:b.snapshot().get('modal_open'),'other category prompt missing');b.paste('Other');b.accept()
            wait(lambda:any(g['name']=='Other' for g in groups(b)),'second category missing');a.accept()
            wait(lambda:'Portfolios changed' in (a.snapshot().get('modal_error') or ''),'stale category edit was not rejected');a.key('escape')
            a.action('categories:delete:'+gid);wait(lambda:all(g['id']!=gid for g in groups()),'delete failed');assert cat()['catalog']['assignments'].get(name) is None
            # Empty categories and collapse survive replacement of only this private controller.
            other=next(g['id'] for g in groups() if g['name']=='Other');a.action('categories:toggle:'+other)
            wait(lambda:other in cat().get('collapsed',[]),'empty category collapse failed')
            controller=a.snapshot()['controller_pid'];os.kill(controller,signal.SIGKILL);wait(lambda:a.process.poll() is not None,'private controller did not detach');a=Client('v-session-a',remote=args.ssh)
            wait(lambda:a.scene and a.snapshot().get('controller_pid')!=controller and any(g['id']==other for g in groups(a)),'categories did not survive controller replacement')
            wait(lambda:other in cat(a).get('collapsed',[]),'collapse did not persist')
            a.action('categories:collapse-all');wait(lambda:all(g['id'] in cat().get('collapsed',[]) for g in groups()),'collapse all failed')
            a.action('categories:expand-all');wait(lambda:not cat().get('collapsed',[]),'expand all failed')
            assert alive(original_pid),'category removal killed source'
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,create=True,assign=True,drag_to_header=True,append_in_blank_space=True,append_within_category=True,category_spacing=True,sidebar_context_options=True,keyboard_expand=True,collapse=True,independent_windows=True,rename=True,hidden_projects_counted=True,reveal_preserves_pid=True,stale_edit_rejected=True,restart_persistence=True,delete_preserves_projects=True,processes_preserved=True)))
            raise SystemExit(0)
        if args.frontend_commands:
            def api(method,params=None,operation=None,ok=True):
                request=dict(version=1,id='frontend-proof',method=method,params=params or {})
                if operation:request['operation_id']=operation
                command=[str(app),'--api',json.dumps(request)]
                if args.ssh:command=ssh_args+[shlex.join(['env',*[k+'='+env[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','PATH','STAR_TERMINAL_RENDERER','STARFLUX_HERDR') if k in env],*command])]
                process=subprocess.Popen(command,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
                wait(lambda:process.poll() is not None,'frontend API request did not finish',timeout=20)
                output,error=process.communicate();result=json.loads(output);assert result['ok']==ok,(result,error.decode())
                return result['result'] if ok else result['error']
            def write(method,params,operation=None):return api(method,params,operation or secrets.token_hex(16))
            def target(client):return next(view['target'] for view in api('frontends.list')['frontends'] if view['frontend']==client.identity)
            def command(value,operation=None,**options):return write('frontends.command',dict(target=lease,command=value,**options),operation)
            wait(lambda:len(api('frontends.list')['frontends'])==2,'frontends were not discoverable')
            lease=target(a);other=b.snapshot()['controller_pid']
            assert lease['pid']==a.snapshot()['controller_pid'] and lease['namespace']
            poisoned={**lease,'start_ticks':lease['start_ticks']+1}
            assert api('frontends.command',dict(target=poisoned,command=dict(kind='select_project',project='extra-a')),secrets.token_hex(16),ok=False)['code']=='stale_frontend'
            assert api('frontends.command',dict(target=lease,command=dict(kind='run',script='NEVER_RUN')),secrets.token_hex(16),ok=False)['code']=='invalid_params'
            assert api('frontends.command',dict(target=lease,command=dict(kind='select_project',project='a'*20000)),secrets.token_hex(16),ok=False)['code']=='invalid_params'
            operation=secrets.token_hex(16);payload=dict(kind='select_project',project='extra-a')
            applied=command(payload,operation);assert applied['status']=='applied' and applied['result']['project']=='extra-a',applied
            wait(lambda:a.snapshot()['active_project']=='extra-a','acknowledged project was not selected')
            assert b.snapshot()['active_project']==name and b.snapshot()['controller_pid']==other
            command(dict(kind='select_project',project=name))
            assert command(payload,operation)==applied,'replay changed the receipt'
            time.sleep(.6);a.pump();assert a.snapshot()['active_project']==name,'replay executed a presentation command twice'
            created=write('panes.create',dict(project=name,label='API navigation'))['target']
            wait(lambda:len(a.snapshot()['project_state']['terminals'])==2,'new pane did not reach frontend')
            selected=command(dict(kind='select_pane',project=name,pane=created['pane'],epoch=created['epoch']))
            assert selected['status']=='applied' and selected['result']['pane']==created['pane'],selected
            wait(lambda:a.snapshot()['active']==created['pane']-1,'acknowledged pane not selected')
            assert b.snapshot()['active']==0
            stale=command(dict(kind='select_pane',project=name,pane=created['pane'],epoch='wrong-generation'))
            assert stale['status']=='rejected' and stale['error']['code']=='stale_target',stale
            a.action('terminal:split-right');wait(lambda:a.snapshot()['project_state']['split'],'split missing')
            zoom=command(dict(kind='set_zoom',project=name,enabled=True));assert zoom['status']=='applied' and zoom['result']['zoomed']
            command(dict(kind='set_zoom',project=name,enabled=False))
            focused=command(dict(kind='focus_pane',project=name,direction='right'));assert focused['status']=='applied'
            assert not b.snapshot()['project_state'].get('pane_zoom') and not b.snapshot()['project_state']['split']
            a.action('sessions:open');wait(lambda:a.snapshot().get('modal_open'),'modal missing')
            busy=command(dict(kind='select_project',project='extra-a'))
            assert busy['status']=='rejected' and busy['error']['code']=='frontend_busy' and a.snapshot()['modal_open'],busy
            a.key('escape');wait(lambda:not a.snapshot().get('modal_open'),'modal did not close')
            a.action('pane-target:0:copy-mode');wait(lambda:bool(a.snapshot().get('terminal_review')),'history review missing')
            reviewing=command(dict(kind='select_project',project='extra-a'))
            assert reviewing['status']=='rejected' and reviewing['error']['code']=='frontend_busy' and a.snapshot()['terminal_review'],reviewing
            a.key('escape');wait(lambda:not a.snapshot().get('terminal_review'),'history review did not close')
            if not a.snapshot().get('app_view_available'):
                unsupported=command(dict(kind='select_space',project=name,space='apps'))
                assert unsupported['status']=='rejected' and unsupported['error']['code']=='unsupported_capability',unsupported
            controller=lease['pid'];argv=Path('/proc',str(controller),'cmdline').read_bytes().split(b'\0');assert b'--serve-view' in argv and a.identity.encode() in argv
            os.kill(controller,signal.SIGSTOP)
            try:
                expired_id=secrets.token_hex(16);cancelled_id=secrets.token_hex(16)
                assert command(dict(kind='select_project',project='extra-a'),expired_id,wait_ms=0,ttl_ms=100)['status']=='queued'
                time.sleep(.15);assert api('frontends.command_status',dict(command_id=expired_id))['status']=='expired'
                assert command(dict(kind='select_project',project='extra-a'),cancelled_id,wait_ms=0)['status']=='queued'
                assert write('frontends.cancel_command',dict(command_id=cancelled_id))['status']=='cancelled'
                first_live='f'*32;live_id='0'*32
                assert command(dict(kind='select_project',project='extra-a'),first_live,wait_ms=0)['status']=='queued'
                assert command(dict(kind='select_project',project='extra-b'),live_id,wait_ms=0)['status']=='queued'
            finally:os.kill(controller,signal.SIGCONT)
            wait(lambda:api('frontends.command_status',dict(command_id=live_id))['status']=='applied','queued navigation was not applied')
            wait(lambda:a.snapshot()['active_project']=='extra-b','queued result did not reach view')
            assert api('frontends.command_status',dict(command_id=first_live))['status']=='applied','queue lost FIFO order'
            assert api('frontends.command_status',dict(command_id=expired_id))['status']=='expired'
            assert api('frontends.command_status',dict(command_id=cancelled_id))['status']=='cancelled'
            queue_directory=state/(a.identity+'.commands')
            wait(lambda:not list(queue_directory.glob('*.json')),'previous queue markers did not drain')
            os.kill(controller,signal.SIGSTOP)
            try:
                # Stale/corrupt marker files must consume bounded storage and
                # be discarded without blocking healthy commands behind them.
                for index in range(31):(queue_directory/(f'{index:020}-bad{index:02}.json')).write_text('1')
                requests=[]
                for index in range(8):
                    operation=secrets.token_hex(16)
                    request=dict(version=1,id=index,method='frontends.command',operation_id=operation,params=dict(target=lease,command=dict(kind='select_project',project='extra-b'),wait_ms=0,ttl_ms=30000))
                    process=subprocess.Popen([str(app),'--api',json.dumps(request)],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE);requests.append((operation,process))
                wait(lambda:all(process.poll() is not None for _,process in requests),'concurrent queue publication did not finish')
                responses=[(operation,json.loads(process.communicate()[0])) for operation,process in requests]
                accepted=[operation for operation,response in responses if response['ok']]
                assert len(accepted)==1 and len(list(queue_directory.glob('*.json')))==32,'concurrent publishers exceeded queue capacity'
                assert all(response['ok'] or response['error']['code']=='frontend_backlog' for _,response in responses)
            finally:os.kill(controller,signal.SIGCONT)
            wait(lambda:api('frontends.command_status',dict(command_id=accepted[0]))['status']=='applied','corrupt queue markers blocked a valid command')
            os.kill(controller,signal.SIGSTOP)
            try:
                pending=secrets.token_hex(16);claimed=secrets.token_hex(16)
                command(dict(kind='select_project',project='extra-a'),pending,wait_ms=0)
                command(dict(kind='select_project',project='extra-a'),claimed,wait_ms=0)
                receipt=tmp/'data/starflux/automation/frontends'/(claimed+'.json')
                saved=json.loads(receipt.read_text());saved['status']='applying';receipt.write_text(json.dumps(saved))
                os.kill(controller,signal.SIGKILL)
            finally:
                if alive(controller):os.kill(controller,signal.SIGCONT)
            wait(lambda:a.process.poll() is not None,'terminated private controller did not detach')
            a=Client('v-session-a',remote=args.ssh);wait(lambda:a.scene and a.snapshot().get('controller_pid')!=controller,'private frontend did not restart')
            assert api('frontends.command_status',dict(command_id=pending))['status']=='rejected'
            assert api('frontends.command_status',dict(command_id=claimed))['status']=='indeterminate'
            assert api('frontends.command',dict(target=lease,command=payload),secrets.token_hex(16),ok=False)['code']=='stale_frontend'
            lease=target(a);session=write('sessions.create',dict(name='Command session',directory=str(tmp)))
            opened=command(dict(kind='open_session',session=session['session']));assert opened['status']=='applied' and opened['result']['session']==session['session'],opened
            wait(lambda:[v['frontend'] for v in api('frontends.list',dict(project=session['project']))['frontends']]==[a.identity],'project-scoped frontend discovery did not follow cross-session navigation')
            rejected=command(dict(kind='select_project',project=name));assert rejected['status']=='rejected' and rejected['error']['code']=='wrong_session'
            assert b.snapshot()['active_project']==name and b.snapshot()['controller_pid']==other and alive(original_pid)
            assert (tmp/'data/starflux/automation/frontends'/(pending+'.json')).stat().st_mode&0o777==0o600
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,targeted_acknowledgements=True,independent_views=True,epoch_guards=True,modal_protection=True,at_most_once=True,cancel_and_expiry=True,controller_restart_guards=True,lost_ack_not_repeated=True,cross_session_navigation=True,private_receipts=True,original_processes_preserved=True)))
            raise SystemExit(0)
        if args.query_upgrade:
            controller=a.snapshot()['controller_pid'];assert Path('/proc',str(controller),'exe').stat().st_ino==copied.stat().st_ino,'fixture controller did not load the disposable build'
            copied.unlink()
            assert str(Path('/proc',str(controller),'exe').readlink()).endswith(' (deleted)')
            picker(a,external=True)
            wait(lambda:any('Fixture' in str(row) for row in a.snapshot()['session_manager']['rows']),'source discovery failed after loaded executable was unlinked')
            assert a.snapshot()['controller_pid']==controller and alive(original_pid) and alive(source_pid)
            print(json.dumps(dict(result='PASS',ssh=args.ssh,loaded_executable_unlinked=True,source_discovery_retained=True,controller_not_restarted=True,terminal_processes_preserved=True)))
            raise SystemExit(0)
        if args.snapshot_service:
            namespace=hashlib.sha256(os.fsencode(str(tmp/'data/starflux/projects'))).hexdigest()[:16]
            def service():
                try:return json.loads((state/('snapshot-'+namespace+'.status.json')).read_text())
                except (OSError,ValueError):return {}
            def archive():
                try:return base64.b64decode(json.loads((tmp/'data/starflux/screens'/name/'p0.screen.json').read_text())['formatted_base64']).decode()
                except (OSError,ValueError):return ''
            wait(lambda:service().get('covered',0)>=1 and service().get('in_flight')==0,'collector did not recognize live controller coverage',timeout=25)
            initial=service();time.sleep(12)
            assert service()['captures']==initial['captures'],'collector duplicated live controller captures'
            assert service()['pid']==initial['pid'] and alive(initial['pid'])
            for client in [a,b]:
                subprocess.run([str(app),'--workspace',name,'--frontend',client.identity,'--close'],env=env,check=True,capture_output=True)
                client.process.wait(timeout=5)
            subprocess.run(['tmux','-L','starflux-'+name,'send-keys','-t','p0','AFTER_LAST_VIEW_CLOSED','Enter'],env=env,check=True,capture_output=True)
            wait(lambda:'REPLY=AFTER_LAST_VIEW_CLOSED' in archive(),'collector did not take over after last view closed',timeout=30)
            assert service()['pid']==initial['pid'] and alive(original_pid)
            wait(lambda:service()['covered']==0 and service()['captures']>initial['captures'],'collector handoff diagnostics did not advance')
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,live_controller_coverage=True,no_duplicate_captures=True,last_view_close_failover=True,retained_terminal=True)))
            raise SystemExit(0)
        if args.background_screens:
            def api(method,params,write=False):
                request=dict(version=1,id='background-proof',method=method,params=params)
                if write:request['operation_id']=secrets.token_hex(16)
                result=json.loads(subprocess.check_output([str(app),'--api',json.dumps(request)],env=env));assert result['ok'],result
                return result['result']
            def saved(project,pane):
                try:
                    result=json.loads((tmp/'data/starflux/screens'/project/('p'+str(pane)+'.screen.json')).read_text())
                    result['formatted']=base64.b64decode(result['formatted_base64']).decode();return result
                except (OSError,ValueError):return {}
            second=api('panes.create',dict(project=name,label='Never opened'),True)
            target=second['target'];assert target['pane']==2
            wait(lambda:len(a.snapshot()['project_state']['terminals'])==2,'background pane did not join model')
            api('panes.send',dict(target=target,text='UNOPENED_NATIVE_界',submit=True),True)
            wait(lambda:'UNOPENED_NATIVE_界' in saved(name,1).get('formatted',''),'unattached native pane did not save',timeout=25)
            assert all(t['pane']!=1 for c in [a,b] for t in c.snapshot().get('direct_terminals',[])),'background capture created a renderer attachment'
            assert saved(name,1)['pid']==target['pid'] and alive(original_pid) and alive(target['pid'])
            a.action('screen-saving:off');wait(lambda:not a.snapshot()['project_state']['save_terminal_screens'],'background saving did not pause')
            api('panes.send',dict(target=target,text='PAUSED_BACKGROUND',submit=True),True);time.sleep(7)
            assert not saved(name,1),'paused background capture recreated a screen'
            a.action('screen-saving:on');wait(lambda:'PAUSED_BACKGROUND' in saved(name,1).get('formatted',''),'background saver did not resume',timeout=25)
            source_session=tmux('display-message','-p','-t','Fixture','#{session_id}')
            imported='background-'+secrets.token_hex(4)
            projects=tmp/'data/starflux/projects';config=json.loads((projects/(name+'.json')).read_text())
            config.update(workspace=imported,label='Background tmux',tmux=dict(socket=str(source_socket),session=source_session))
            (projects/(imported+'.json')).write_text(json.dumps(config))
            model=json.loads((projects/(name+'.model.json')).read_text());model.update(terminal=1,split=False,pane_tree=None)
            (projects/(imported+'.model.json')).write_text(json.dumps(model))
            (projects/'catalog-version').write_bytes(os.urandom(16))
            wait(lambda:imported in a.snapshot().get('project_navigation',{}).get('visible',[]),'background source project did not enter catalog')
            a.action('project:'+imported);wait(lambda:a.snapshot().get('active_project')==imported and any(t['pane']==1 for t in a.snapshot().get('direct_terminals',[])),'background source project did not open its owned pane')
            tmux('send-keys','-t','Fixture','UNOPENED_TMUX_界','Enter')
            wait(lambda:'UNOPENED_TMUX_界' in saved(imported,0).get('formatted',''),'unattached imported tmux pane did not save',timeout=25)
            assert all(t['pane']!=0 for t in a.snapshot().get('direct_terminals',[]))
            assert alive(source_pid) and saved(imported,0)['pid']==source_pid
            assert tmux('list-panes','-s','-t','Fixture','-F','#{pane_id} #{pane_pid}')==source_topology
            if args.herdr:
                hp=snapshot['panes'][0];hidden='background-herdr-'+secrets.token_hex(3)
                source=dict(session='default',workspace_id=hp['workspace_id'],order=0,executable=shutil.which('herdr'),panes={'0':dict(terminal_id=hp['terminal_id'],pane_id=hp['pane_id'],tab_id=hp['tab_id'],label='Hidden Herdr',cwd=str(tmp),agent=None)})
                config.update(workspace=hidden,label='Background Herdr',tmux=None,herdr=source)
                (projects/(hidden+'.json')).write_text(json.dumps(config));(projects/(hidden+'.model.json')).write_text(json.dumps(model))
                (projects/'catalog-version').write_bytes(os.urandom(16))
                wait(lambda:hidden in a.snapshot().get('project_navigation',{}).get('visible',[]),'hidden Herdr project did not enter catalog')
                a.action('project:'+hidden);wait(lambda:a.snapshot().get('active_project')==hidden and any(t['pane']==1 for t in a.snapshot().get('direct_terminals',[])),'Herdr source project did not open owned pane')
                subprocess.run(['herdr','pane','send-text',hp['pane_id'],'UNOPENED_HERDR_界'],env=env,check=True,capture_output=True)
                subprocess.run(['herdr','pane','send-keys',hp['pane_id'],'enter'],env=env,check=True,capture_output=True)
                try:wait(lambda:'UNOPENED_HERDR_界' in saved(hidden,0).get('formatted',''),'unattached Herdr pane did not save',timeout=25)
                except AssertionError:
                    print(json.dumps(dict(source_sessions=[{k:s.get(k) for k in ['project','pane','health']} for s in a.snapshot().get('sessions',[])],saved=saved(hidden,0))),flush=True)
                    raise
                assert all(t['pane']!=0 for t in a.snapshot().get('direct_terminals',[]))
                assert all(alive(pid) for pid in herdr_pids)
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,native_never_attached=True,imported_tmux_never_attached=True,imported_herdr_never_attached=args.herdr,unicode_preserved=True,source_identity=True,pause_clear_resume=True,no_renderer_spawned=True,source_processes_preserved=True)))
            raise SystemExit(0)
        if args.panel_focus:
            def typed():return (tmp/'panel-input').read_bytes() if (tmp/'panel-input').exists() else b''
            a.action('terminal:split-down');wait(lambda:len(a.snapshot().get('panes',[]))==2 and all(p.get('pid') for p in a.snapshot()['panes']),'focus split missing')
            a.action('terminal:0');wait(lambda:a.snapshot()['project_state']['terminal']==0,'first terminal did not focus')
            before=typed();a.key('tab');wait(lambda:typed().count(b'\t')==before.count(b'\t')+1,'plain Tab did not reach terminal')
            assert not a.snapshot().get('panel_focus'),'plain Tab navigated panels'
            before=typed();a.shortcut('tab');wait(lambda:a.snapshot().get('panel_focus')=={'terminal':1},'prefix Tab did not navigate to next pane')
            a.shortcut('backtab',1);wait(lambda:a.snapshot().get('panel_focus')=={'terminal':0},'prefix Shift+Tab did not reverse to first pane')
            assert typed()==before,'prefix navigation leaked Tab into terminal'
            a.key('enter');wait(lambda:not a.snapshot().get('panel_focus'),'Enter did not resume terminal input')
            a.key('f:6');wait(lambda:a.snapshot().get('panel_focus')=='toolbar','F6 did not focus workspace controls')
            before=typed()
            for expected in ['tabs',{'terminal':0},{'terminal':1},'projects','toolbar']:
                a.key('tab');wait(lambda:a.snapshot().get('panel_focus')==expected,'Tab did not advance panel to '+repr(expected))
            a.key('backtab',1);wait(lambda:a.snapshot().get('panel_focus')=='projects','Shift+Tab did not reverse panel focus')
            assert typed()==before,'panel navigation leaked keys into a program'
            assert not b.snapshot().get('panel_focus'),'panel focus affected another frontend'
            a.key('enter');wait(lambda:'uncategorized' in a.snapshot().get('categories',{}).get('collapsed',[]),'focused category did not collapse');a.key('enter');a.action('workspace:projects');wait(lambda:bool(a.snapshot().get('context_menu')),'project picker did not activate')
            a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'project picker did not close')
            a.key('f:6');wait(lambda:not a.snapshot().get('panel_focus') and a.snapshot()['cell_terminal_focus'],'F6 did not return to terminal input')
            a.paste('AFTER_PANEL');wait(lambda:b'AFTER_PANEL' in typed(),'terminal input did not resume')
            a.key('escape',2);wait(lambda:bool(a.snapshot().get('panel_focus')),'Ctrl+Esc did not enter panel navigation')
            a.resize(40,12,400,240);a.key('tab');wait(lambda:a.snapshot().get('panel_focus')=='projects','compact view lost panel navigation')
            a.key('f:6');wait(lambda:not a.snapshot().get('panel_focus'),'compact view did not restore capture')
            keys=tmp/'config/starflux/keys.json';keys.parent.mkdir(parents=True,exist_ok=True);keys.write_text(json.dumps(dict(tab_switches_panels=False)))
            a.shortcut('char:R',1);time.sleep(.2)
            before=typed();a.key('tab');wait(lambda:typed().count(b'\t')==before.count(b'\t')+1,'configured captured Tab did not reach terminal')
            assert not a.snapshot().get('panel_focus'),'opt-out Tab also navigated panels'
            keys.write_text(json.dumps(dict(tab_switches_panels=True)))
            a.shortcut('char:R',1);time.sleep(.2)
            before=typed();a.key('tab');wait(lambda:a.snapshot().get('panel_focus')=='projects','configured captured Tab did not navigate panels')
            assert typed()==before,'configured Tab also reached terminal'
            a.key('f:6');wait(lambda:not a.snapshot().get('panel_focus'),'F6 did not restore content input')
            keys.write_text('{}');a.shortcut('char:R',1);time.sleep(.2)
            before=typed();a.key('tab');wait(lambda:typed().count(b'\t')==before.count(b'\t')+1,'reloaded default Tab did not reach terminal')
            before=typed();a.shortcut('char:b',2);wait(lambda:typed().count(b'\x02')==before.count(b'\x02')+1,'double prefix did not deliver literal Ctrl+B')
            keys.write_text(json.dumps(dict(prefix='Ctrl+a')));a.shortcut('char:R',1);time.sleep(.2)
            a.key('char:a',2);a.key('tab');wait(lambda:a.snapshot().get('panel_focus')=='projects','custom prefix did not navigate panels')
            a.key('f:6');wait(lambda:not a.snapshot().get('panel_focus'),'custom prefix navigation did not resume terminal')
            keys.write_text('{}');a.key('char:a',2);a.key('char:R',1);time.sleep(.2)
            assert all(alive(p['pid']) for p in a.snapshot()['panes']) and alive(original_pid)
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,tab_panel_cycle=True,shift_tab_reverse=True,split_pane_focus=True,modal_focus=True,captured_tab_preserved=True,configurable_capture_override=True,prefix_panel_cycle=True,compact_navigation=True,independent_frontends=True,processes_preserved=True)))
            raise SystemExit(0)
        if args.cold_start:
            screens=tmp/'data/starflux/screens'/name
            projects=tmp/'data/starflux/projects'
            if args.reboot_adopted:
                boot_model=tmp/'data/starflux/projects'/(name+'.model.json')
                initial=json.loads(boot_model.read_text());initial.setdefault('terminal_epochs',{})['0']=secrets.token_hex(16)
                boot_model.write_text(json.dumps(initial))
            a.action('terminal:split-down')
            wait(lambda:len(a.snapshot().get('panes',[]))==2 and all(p.get('pid') for p in a.snapshot()['panes']),'cold-start split missing')
            a.action('terminal:0');wait(lambda:a.snapshot()['project_state']['terminal']==0,'first pane did not focus');a.paste('BEFORE_COLD_RESTART');a.key('enter')
            def archive(pane,previous=False):
                try:
                    result=json.loads((screens/('p'+str(pane)+('.previous-screen.json' if previous else '.screen.json'))).read_text())
                    result['formatted']=base64.b64decode(result['formatted_base64']).decode();return result
                except (OSError,ValueError):return {}
            wait(lambda:'BEFORE_COLD_RESTART' in archive(0).get('formatted','') and archive(1).get('rows'),'screens were not saved',timeout=20)
            current_file=screens/'p0.screen.json';future=json.loads(current_file.read_text());future['captured_ms']+=86_400_000
            current_file.write_text(json.dumps(future))
            a.paste('AFTER_CLOCK_CORRECTION');a.key('enter')
            wait(lambda:'AFTER_CLOCK_CORRECTION' in archive(0).get('formatted','') and archive(0)['captured_ms']<future['captured_ms'],'wall-clock correction prevented a new screen',timeout=20)
            status=json.loads(subprocess.check_output([str(app),'--workspace',name,'--agent-status'],env=env))['panes'][0]
            process=status['status'];run=secrets.token_hex(16)
            report=dict(run_id=run,sequence=1,epoch=status['epoch'],agent_pid=process['agent_pid'],agent_start_ticks=process['agent_start_ticks'],boot_id=process['boot_id'],harness='codex',source='fixture:cold',state='working',conversation_id='cold-exact-thread',resume_argv=[str(shell),'cold-exact-thread'])
            subprocess.run([str(app),'--workspace',name,'--pane','1','--agent-report',json.dumps(report)],env=env,check=True,capture_output=True)
            assert (screens/'p0.screen.json').stat().st_mode&0o777==0o600
            old=archive(0);layout=a.snapshot()['project_state']['pane_tree']
            owners=[p['pid'] for p in a.snapshot()['panes']]
            for client in [a,b]:
                subprocess.run([str(app),'--workspace',name,'--frontend',client.identity,'--close'],env=env,check=True,capture_output=True)
                client.process.wait(timeout=5)
            old=archive(0)
            subprocess.run(['tmux','-L','starflux-'+name,'kill-server'],env=env,check=True)
            wait(lambda:all(not alive(pid) for pid in owners),'private cold-start owners survived')
            markers=[tmp/('MUST_NOT_AUTORESTART_AGENT_'+str(pane)) for pane in range(2)]
            model=projects/(name+'.model.json');saved=json.loads(model.read_text())
            saved['terminal_commands']={str(pane):shlex.join(['/bin/sh','-c','touch '+shlex.quote(str(marker))+'; exec '+shlex.quote(str(shell))]) for pane,marker in enumerate(markers)}
            # A stale view reintroduced one consumed intent; the other hint is
            # legacy metadata with no launch permission at all.
            saved['terminal_launch_pending']=[0];model.write_text(json.dumps(saved))
            receipt=screens/'p0.launch.json';receipt.write_text(json.dumps(dict(version=1,epoch=saved.get('terminal_epochs',{}).get('0'))))
            if args.reboot_adopted:
                previous_boot='088f251c-a7de-43cd-a07c-3032d2c12f71'
                protected=subprocess.Popen(['sleep','120'],env=env);processes.append(protected)
                proxy=tmp/'starflux-pty-proxy';proxy.write_text('#!/bin/sh\ntouch '+shlex.quote(str(tmp/'MUST_NOT_RUN_OLD_PROXY'))+'\n');proxy.chmod(0o700)
                cfg_path=projects/(name+'.json');cfg=json.loads(cfg_path.read_text());cfg['adopted']={}
                for pane in range(2):
                    epoch=saved.setdefault('terminal_epochs',{}).setdefault(str(pane),secrets.token_hex(16))
                    record=json.loads((screens/('p'+str(pane)+'.screen.json')).read_text())
                    record.update(pid=protected.pid,epoch=epoch,observer_boot_id=previous_boot)
                    (screens/('p'+str(pane)+'.screen.json')).write_text(json.dumps(record))
                    t=dict(broker=str(state/'dead-broker.sock'),key='reboot-'+str(pane),server='starflux-reboot-bridge',pid=protected.pid,epoch=epoch,proxy=str(proxy))
                    if pane==1:t['owner_boot_id']=previous_boot
                    cfg['adopted'][str(pane)]=t
                saved['terminal_launch_pending']=[0,1];model.write_text(json.dumps(saved));cfg_path.write_text(json.dumps(cfg))
                # The preferred session must survive complete runtime loss too.
                sessions=tmp/'data/starflux/sessions';sessions.mkdir(exist_ok=True)
                (sessions/'default-session').write_text(json.dumps('base'))
                (sessions/'base.json').write_text(json.dumps(dict(id='base',name='Preferred session',project=name)))
            # A cold runtime, retaining only durable data and per-view layouts.
            shutil.rmtree(runtime);runtime.mkdir(mode=0o700);state.mkdir(mode=0o700)
            a=Client('v-session-a',remote=args.ssh);b=Client('v-session-b')
            wait(lambda:a.scene and b.scene and len(a.snapshot().get('panes',[]))==2 and all(p.get('pid') for p in a.snapshot()['panes']),'cold runtime did not recover layout')
            assert a.snapshot()['project_state']['pane_tree']==layout,'cold start changed split tree'
            assert all(p['pid'] not in owners for p in a.snapshot()['panes'])
            assert not any(marker.exists() for marker in markers),'saved command restarted automatically'
            if args.reboot_adopted:
                assert protected.poll() is None,'reused PID was signalled during reboot recovery'
                assert not (tmp/'MUST_NOT_RUN_OLD_PROXY').exists(),'old transfer proxy executed after reboot'
                recovery=tmp/'data/starflux/terminal-recovery'/name
                assert all((recovery/('p'+str(pane)+'.json')).exists() for pane in range(2)),'durable transfer recovery receipts missing'
                assert subprocess.check_output([str(app),'--default-workspace'],env=env,text=True).strip()==name,'reboot lost the configured default session'
                check_startup(name,a.snapshot()['panes'][0]['pid'])
                a.paste('REBOOT_RESPONSIVE');a.key('enter')
                wait(lambda:'REBOOT_RESPONSIVE' in a.snapshot()['direct_terminals'][0]['text'],'restored terminal did not respond')
            wait(lambda:(a.snapshot()['panes'][0].get('agent_record') or {}).get('conversation_id')=='cold-exact-thread' and not a.snapshot()['panes'][0]['health']['report_process_matches'],'saved conversation did not survive cold start or was treated as the same process')
            wait(lambda:archive(0,True).get('captured_ms')==old['captured_ms'],'previous process screen was not retained',timeout=20)
            request=dict(version=1,id='saved-screen',method='panes.saved_screen',params=dict(project=name,pane=1,previous=True))
            command=[str(app),'--api-stdio']
            if args.ssh:command=ssh_args+[shlex.join(['env',*[k+'='+env[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','PATH')],*command])]
            result=json.loads(subprocess.check_output(command,input=(json.dumps(request)+'\n').encode(),env=env,timeout=15).splitlines()[0])
            assert result['ok'] and result['result']['read_only'] and 'BEFORE_COLD_RESTART' in result['result']['text'],'saved screen API did not return historical text'
            a.all_frames=[];a.action('pane-target:0:saved-previous')
            wait(lambda:'Saved screen' in (a.snapshot().get('terminal_review') or {}).get('source',''),'saved screen did not open')
            def restored():return any('BEFORE_COLD_RESTART' in ''.join(run['text'] for run in row['runs']) for frame in a.all_frames for row in frame['rows'])
            wait(restored,'saved screen pixels/cells were not restored')
            assert not b.snapshot().get('terminal_review'),'saved review affected another frontend'
            assert not a.clipboards and not any(marker.exists() for marker in markers)
            assert archive(0,True)['formatted']==old['formatted'],'review changed archive'
            a.key('escape');wait(lambda:not a.snapshot().get('terminal_review'),'saved review did not close')
            a.paste('AFTER_COLD_RESTART');a.key('enter')
            wait(lambda:any('REPLY=AFTER_COLD_RESTART' in t['text'] for t in a.snapshot().get('direct_terminals',[])),'new shell input failed')
            previous=screens/'p0.previous-screen.json';valid=previous.read_bytes();previous.write_text('{broken')
            a.action('pane-target:0:saved-previous')
            wait(lambda:'Read saved screen' in (a.snapshot().get('terminal_review') or {}).get('source',''),'invalid archive was not reported')
            assert not a.clipboards and all(alive(p['pid']) for p in a.snapshot()['panes'])
            previous.write_bytes(valid);a.key('escape');wait(lambda:not a.snapshot().get('terminal_review'),'failed preview did not close')
            a.action('screen-saving:off')
            wait(lambda:not a.snapshot()['project_state']['save_terminal_screens'] and not list(screens.glob('p*.screen.json')) and not list(screens.glob('p*.previous-screen.json')),'screen saving did not pause and clear')
            a.paste('WHILE_PAUSED');a.key('enter');time.sleep(6)
            assert not list(screens.glob('p*.screen.json')) and receipt.exists(),'paused saver wrote a screen or removed launch protection'
            a.action('screen-saving:on');wait(lambda:archive(0).get('rows'),'screen saving did not resume',timeout=15)
            current_owners=[p['pid'] for p in a.snapshot()['panes']]
            a.action('agent-resume:0:'+run)
            wait(lambda:any('RESUMED_COLD_EXACT_THREAD' in t['text'] for t in a.snapshot().get('direct_terminals',[])),'exact saved conversation did not resume after cold start')
            assert all(alive(pid) for pid in current_owners) and not any(marker.exists() for marker in markers)
            resumed=a.snapshot()['project_state']['terminal'];new_receipt=screens/('p'+str(resumed)+'.launch.json')
            wait(lambda:new_receipt.exists(),'cold resume did not consume its launch intent')
            assert new_receipt.stat().st_mode&0o777==0o600
            assert alive(source_pid) and tmux('list-panes','-s','-t','Fixture','-F','#{pane_id} #{pane_pid}')==source_topology,'cold recovery changed imported source'
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,cold_runtime_restored=True,split_layout_restored=True,styled_unicode_screen=True,previous_process_screen=True,clock_correction_safe=True,saved_commands_not_restarted=True,consumed_intents_not_replayed=True,cold_conversation_resume=True,invalid_archive_reported=True,pause_clear_resume=True,review_is_read_only=True,independent_frontends=True,new_shell_input=True,unrelated_source_preserved=True)))
            raise SystemExit(0)
        if args.narrow:
            def menu(title):return wait(lambda:(m if (m:=a.snapshot().get('context_menu')) and m['title']==title else None),'missing '+title)
            def actions():
                result=[]
                for component in a.scene['components']:
                    if component['kind']=='surface':result.extend(component['surface'].get('hits',[]))
                return result
            def visible_field(index):
                if args.cells:
                    text=''.join(r.get('text','') for r in a.scene.get('spans',[]))
                    return 'Field '+str(index+1) in text or a.snapshot()['viewport']['rows']>=24
                return any(h['action']=='modal:field:'+str(index) for h in actions())
            def fill(index,text):
                a.action('modal:field:'+str(index));wait(lambda:a.snapshot().get('modal_field')==index and visible_field(index),'focused field was hidden')
                a.key('char:e',2);a.key('char:u',2);a.paste(text)
            for columns,rows,width,height in [(40,12,320,240),(32,10,320,240),(60,16,480,320),(80,24,800,480)]:
                a.resize(columns,rows,width,height)
                wait(lambda:a.snapshot().get('panes') and a.snapshot()['panes'][0].get('pid')==original_pid,'resizing lost the terminal')
                assert a.snapshot()['terminal_presentation']['canvas'][0]>0 and a.snapshot()['terminal_presentation']['canvas'][1]>0
                assert b.snapshot()['viewport']['width']==1000 and b.snapshot()['panes'][0]['pid']==original_pid,'resizing changed the other frontend'
                a.shortcut('char:e');m=menu('Workspace menu')
                expected={'workspace:projects','sessions:open','attention:inbox','machines:list','workspace:terminal-menu','terminal:new','git:details','keyboard:help','ui:refresh'}
                assert expected<={i['action'] for i in m['items']},m
                if args.cells:assert not any(i['action'].startswith('view:apps') for i in m['items'])
                a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'workspace menu did not close')
                a.paste('NARROW_INPUT');a.key('enter')
                wait(lambda:any('REPLY=NARROW_INPUT' in t.get('text','') for t in a.snapshot().get('direct_terminals',[])),'terminal input failed after resize')
            a.resize(40,12,320,240)
            a.shortcut('char:t',1);m=menu('Terminal tabs')
            a.action('menu:item:'+str(next(i for i,item in enumerate(m['items']) if item['action']=='terminal:new')))
            wait(lambda:len(a.snapshot()['project_state']['terminals'])==2 and a.snapshot()['active']==1,'compact tab picker did not create a tab')
            a.shortcut('char:t',1);m=menu('Terminal tabs')
            a.action('menu:item:'+str(next(i for i,item in enumerate(m['items']) if item['action']=='terminal:0')))
            wait(lambda:a.snapshot()['active']==0 and not a.snapshot().get('context_menu'),'compact tab picker did not restore original terminal')
            # The last field of the five-field machine form is reachable and visible.
            a.shortcut('char:m',1);menu('Machines');a.key('enter');wait(lambda:a.snapshot().get('modal_open'),'machine form missing')
            for index,value in enumerate(['Narrow 界','proof','', 'starflux','remote-workspace']):fill(index,value)
            a.accept();menu('SSH machine');assert a.snapshot()['machine_profile']['workspace']=='remote-workspace'
            a.action('machines:remove');menu('Machines');a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'machine menu did not close')
            # Project selection and recovery are accessible without opening a sidebar.
            a.shortcut('char:p',1);m=menu('Projects');assert m['items'][-1]['action']=='project:new'
            a.action('menu:item:'+str(next(i for i,item in enumerate(m['items']) if item['action']=='project:extra-a')));wait(lambda:a.snapshot()['active_project']=='extra-a' and a.snapshot()['panes'][0].get('pid'),'compact project picker failed')
            a.shortcut('char:p',1);m=menu('Projects');a.action('menu:item:'+str(next(i for i,item in enumerate(m['items']) if item['action']=='project:'+name)));wait(lambda:a.snapshot()['active_project']==name,'return to original project failed')
            a.shortcut('char:x');wait(lambda:any(i['action']=='reconnect-terminal:0' for i in (a.snapshot().get('context_menu') or {}).get('items',[])),'recovery was inaccessible')
            a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'recovery menu did not close')
            a.shortcut('char:g');wait(lambda:a.snapshot().get('git_open') and (a.snapshot().get('git') or {}).get('files'),'compact Git view missing')
            for _ in range(30):a.key('down')
            def visible_text():
                if args.cells:return ''.join(s.get('text','') for s in a.scene.get('spans',[]))
                return ''.join(n.get('text','') for c in a.scene['components'] if c['kind']=='surface' for n in c['surface'].get('nodes',[]))
            wait(lambda:'narrow-file-15.txt' in visible_text(),'compact Git list could not scroll to its last file')
            a.key('escape');wait(lambda:not a.snapshot().get('git_open'),'Git did not close')
            # Normal forms keep the caret and buttons on-screen while traversing every field.
            a.shortcut('char:a');wait(lambda:a.snapshot().get('modal_open'),'project form missing')
            for index,value in enumerate(['new-narrow-project','local',str(tmp)+'/long-世界-directory-name']):fill(index,value)
            a.key('pageup');wait(lambda:a.snapshot()['modal_field']==1 and visible_field(1),'previous field paging failed')
            a.key('pagedown');wait(lambda:a.snapshot()['modal_field']==2 and visible_field(2),'next field paging failed')
            a.key('escape');wait(lambda:not a.snapshot().get('modal_open'),'project dialog did not cancel')
            assert not (tmp/'data/starflux/projects/new-narrow-project.json').exists()
            # Session picker can create, navigate, default and open in the compact viewport.
            a.shortcut('char:s');manager=wait(lambda:a.snapshot().get('session_manager'),'compact session picker missing')
            wait(lambda:not a.snapshot()['session_manager']['status'].startswith('Loading'),'session picker did not load')
            if not args.cells:
                assert any(h['action']=='sessions:choose' for h in actions()) and any(h['action'].startswith('sessions:row:') for h in actions()),actions()
            a.paste('base');a.key('left');a.paste('X');a.key('backspace')
            wait(lambda:(m:=a.snapshot().get('session_manager')) and m['query']=='base' and m['cursor']==3,'session query caret did not follow editing')
            a.key('char:e',2);a.key('char:u',2)
            a.key('char:n',2);wait(lambda:a.snapshot().get('modal_open') and not a.snapshot().get('session_manager'),'new session form missing')
            fill(0,'Narrow session');fill(1,str(tmp));a.accept()
            opened=wait(lambda:(s if (s:=a.snapshot()).get('session_id')!='base' and s.get('panes') and s['panes'][0].get('pid') else None),'small session creation failed')
            new_pid=opened['panes'][0]['pid']
            a.shortcut('char:s');wait(lambda:(m:=a.snapshot().get('session_manager')) and not m['status'].startswith('Loading'),'second session picker missing')
            a.key('end');a.key('home');a.key('down');a.key('up')
            a.key('char:d',2);wait(lambda:a.snapshot()['session_manager']['selected_default'],'default control failed in compact picker')
            a.key('enter');wait(lambda:not a.snapshot().get('modal_open'),'compact session did not open')
            assert alive(original_pid) and alive(new_pid)
            # Auto-collapse of a nested layout must not discard its saved tree or stop its owners.
            a.shortcut('char:%');wait(lambda:a.snapshot().get('project_state',{}).get('split'),'compact split did not open')
            tree=a.snapshot()['project_state']['pane_tree']
            a.resize(32,10,320,240)
            assert a.snapshot()['project_state']['pane_tree']==tree
            if not args.cells:
                a.resize(20,8,160,120)
                assert not any(c['kind'] in ('terminal','live_canvas') for c in a.scene['components']),'tiny presentation kept streaming over its resize notice'
                assert alive(original_pid) and alive(new_pid)
            # Restoring a larger viewport reuses terminal owners and stored layouts.
            a.resize(120,40,1200,800)
            assert a.snapshot()['project_state']['pane_tree']==tree
            assert a.snapshot()['viewport']['width']==1200 and b.snapshot()['active_project']==name
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,compact_terminal_input=True,all_controls_reachable=True,project_tab_pickers=True,focused_fields_visible=True,unicode_editing=True,compact_sessions=True,independent_viewports=True,processes_preserved=True)))
            raise SystemExit(0)
        if args.notifications:
            import uuid
            def api(method,params=None,write=False,ok=True):
                request=dict(version=1,id='notification-proof',method=method,params=params or {})
                if write:request['operation_id']=str(uuid.uuid4())
                response=json.loads(subprocess.check_output([str(app),'--api',json.dumps(request)],env=env))
                assert response['ok']==ok,response
                return response['result'] if ok else response['error']
            def configure(**changes):
                settings=api('notifications.settings')['settings'];updated={**settings,**changes}
                return api('notifications.configure',dict(settings=updated,expected_revision=settings['revision']),write=True)['settings']
            def query(**values):return api('attention.query',values)['items']
            def menu(title):return wait(lambda:(m if (m:=a.snapshot().get('context_menu')) and m['title']==title else None),'missing '+title)
            def report_for(project,conversation):
                pane=api('panes.get',dict(project=project,pane=1))
                health=pane['status'];target=pane['target']
                return dict(run_id=uuid.uuid4().hex,sequence=0,epoch=target['epoch'],agent_pid=health['agent_pid'],agent_start_ticks=health['agent_start_ticks'],boot_id=health['boot_id'],harness='codex',source='fixture:provider',state='working',conversation_id=conversation,resume_argv=None)
            def publish(project,report,state):
                report.update(sequence=report['sequence']+1,state=state)
                return api('agents.report',dict(project=project,pane=1,report=report),write=True)
            def calls():
                p=tmp/'desktop-notifications'
                return [json.loads(line) for line in p.read_text().splitlines()] if p.exists() else []
            wait(lambda:a.snapshot()['panes'][0].get('agent_pid'),'fixture agent identity missing')
            first=report_for(name,'conversation-café-界')
            old_settings=api('notifications.settings')['settings']
            assert old_settings['delivery']=='in_app'
            configure(delivery='system',suppress_active=True)
            for state_name in ['needs_input','completed','waiting_for_approval','failed','backend_unavailable']:
                publish(name,first,'working');publish(name,first,state_name)
            wait(lambda:len(query())==5,'reported attention transitions missing')
            wait(lambda:len(calls())==5,'desktop notifications did not coalesce across frontends',timeout=20)
            assert all('--' in c and 'STAR/FLUX' in c[0] for c in calls()),calls()
            wait(lambda:len(api('notifications.delivery')['attempts'])==5 and all(v['outcome']=='submitted' for v in api('notifications.delivery')['attempts'].values()),'desktop helper did not accept all five notifications')
            page=api('attention.query',dict(limit=2));next_page=api('attention.query',dict(limit=2,offset=2))
            assert page['total']==5 and len(page['items'])==2 and len(next_page['items'])==2
            assert not {i['event']['id'] for i in page['items']} & {i['event']['id'] for i in next_page['items']}
            api('attention.query',dict(limit=251),ok=False)
            a.shortcut('char:i');menu('Attention inbox')
            wait(lambda:len(a.snapshot().get('attention_items',[]))==5,'inbox event rows missing')
            view_query=tmp/'data/starflux/projects'/f'{a.identity}.attention-query.json'
            view_query.write_text(json.dumps({**a.snapshot()['attention_query'],'limit':2}))
            a=restart(a);a.shortcut('char:i');menu('Attention inbox')
            assert a.snapshot()['attention_total']==5 and len(a.snapshot()['attention_items'])==2
            a.action('attention:next');wait(lambda:a.snapshot()['attention_query']['offset']==2 and len(a.snapshot()['attention_items'])==2,'next inbox page failed')
            a.action('attention:previous');wait(lambda:a.snapshot()['attention_query']['offset']==0 and len(a.snapshot()['attention_items'])==2,'previous inbox page failed')
            view_query.write_text(json.dumps({**a.snapshot()['attention_query'],'limit':100}))
            a=restart(a);a.shortcut('char:i');menu('Attention inbox')
            assert len(a.snapshot()['attention_items'])==5
            a.key('char:/');wait(lambda:a.snapshot().get('modal_open'),'keyboard inbox search missing')
            a.paste('CAFÉ 界');a.accept();menu('Attention inbox')
            assert len(a.snapshot()['attention_items'])==5,'Unicode conversation search failed'
            a.action('attention:state');wait(lambda:a.snapshot()['attention_query']['state']=='needs_input' and len(a.snapshot()['attention_items'])==1,'state filter failed')
            a.action('attention:mark');wait(lambda:len(query())==4,'matching mark-read failed')
            menu('Attention inbox');wait(lambda:not a.snapshot()['attention_items'],'mark-read did not update filtered page')
            a.action('attention:read');wait(lambda:a.snapshot()['attention_query']['read']=='all' and len(a.snapshot()['attention_items'])==1,'read history missing')
            assert a.snapshot()['attention_items'][0]['event']['dismissed']
            a=restart(a);a.shortcut('char:i');menu('Attention inbox')
            assert a.snapshot()['attention_query']['search']=='CAFÉ 界' and a.snapshot()['attention_query']['read']=='all','frontend filters were not retained'
            a.action('attention:reset');wait(lambda:len(a.snapshot()['attention_items'])==4,'filter reset failed')
            assert b.snapshot()['attention_query']['read']=='unread' and not b.snapshot()['attention_query']['search'],'inbox filters leaked into another frontend'
            # Active visible pane suppression is shared by both presentation controllers.
            a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'inbox did not close')
            a.input(dict(kind='notice',message='focus-canvas:terminals'))
            wait(lambda:a.snapshot().get('notification_focused'),'focus state not recorded')
            publish(name,first,'working');publish(name,first,'needs_input')
            def outcomes():return list(api('notifications.delivery')['attempts'].values())
            wait(lambda:any(v['outcome']=='suppressed_active' for v in outcomes()),'active pane suppression failed')
            assert len(calls())==5,'active agent produced a desktop notification'
            a.input(dict(kind='notice',message='window-focus:lost'));wait(lambda:not a.snapshot().get('notification_focused'),'focus loss missing')
            # Helper failure is a visible, non-retried delivery outcome; the inbox keeps the item.
            (tmp/'desktop-fail').touch();publish(name,first,'working');publish(name,first,'failed')
            wait(lambda:any(v['outcome']=='failed' for v in outcomes()),'desktop failure was not recorded')
            assert len(calls())==6 and len(query())==6
            (tmp/'desktop-fail').unlink()
            b.action('sessions:new');wait(lambda:b.snapshot().get('modal_open'),'cross-session fixture dialog missing');b.paste('Other inbox session');b.accept()
            other=wait(lambda:(s if (s:=b.snapshot()).get('session_id')!='base' and s.get('panes') and s['panes'][0].get('agent_pid') else None),'second session agent missing')
            second=report_for(other['active_project'],'other-conversation');publish(other['active_project'],second,'waiting_for_approval')
            assert len(query(session=other['session_id']))==1
            assert len(query(project=name,source='reported'))==6 and not query(source='screen')
            assert len(query(search='fixture:provider completed'))==1
            assert len(calls())==6,'failed delivery was retried during a new event'
            wait(lambda:len(calls())==7,'other session notification missing')
            # Invalid/stale settings and an all-or-nothing per-project mark-read batch.
            latest=api('notifications.settings')['settings']
            api('notifications.configure',dict(settings={**latest,'retention_days':4000},expected_revision=latest['revision']),write=True,ok=False)
            configure(states=['waiting_for_approval'])
            api('notifications.configure',dict(settings=latest,expected_revision=latest['revision']),write=True,ok=False)
            prior=len(query(project=name))
            selected=query(project=name)[0]
            batch=api('attention.mark_read',dict(items=[dict(project=name,event=selected['event']['id']),dict(project=name,event=str(uuid.uuid4()))]),write=True)
            assert batch['errors'] and batch['updated']==0 and len(query(project=name))==prior
            api('attention.query',dict(state='working'),ok=False)
            # Retention is bounded by count and age, while conversations and live processes survive.
            configure(delivery='off',retention_days=0,max_items_per_project=10)
            for n in range(12):publish(name,first,'working');publish(name,first,'completed')
            assert len(query(project=name,read='all'))==10
            records=api('panes.get',dict(project=name,pane=1));assert records['target']['run_id']==first['run_id'] and query(project=name,read='all')[0]['conversation_id']=='conversation-café-界'
            store=tmp/'data/starflux/projects'/f'{name}.agents.json'
            saved=json.loads(store.read_text());saved['attention'][0]['observed_ms']=1;store.write_text(json.dumps(saved))
            configure(retention_days=1)
            pruned=api('notifications.prune',write=True);assert not pruned['errors'] and len(query(project=name,read='all'))==9
            assert alive(original_pid) and api('panes.get',dict(project=name,pane=1))['target']['run_id']==first['run_id'] and query(project=name,read='all')[0]['conversation_id']=='conversation-café-界'
            # Another frontend's project/pane selection and session ownership are independent.
            assert b.snapshot()['active_project']==other['active_project']
            a.shortcut('char:i');menu('Attention inbox');a.action('attention:settings');menu('Notification settings')
            settings=api('notifications.settings')['settings'];newer=configure(suppress_active=not settings['suppress_active'])
            a.action('attention:option:active');wait(lambda:'settings changed' in a.snapshot().get('status','').lower(),'stale notification settings UI overwrote a newer edit')
            assert api('notifications.settings')['settings']['revision']==newer['revision']
            a.action('attention:retention');wait(lambda:a.snapshot().get('modal_open'),'retention form missing');a.key('escape');wait(lambda:not a.snapshot().get('modal_open'),'retention dialog did not cancel')
            if args.ssh:
                request=json.dumps(dict(version=1,id='remote-capability',method='notifications.settings',params={}))
                command=['env',*[k+'='+v for k,v in env.items() if k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','PATH')],str(app),'--api',request]
                remote=json.loads(subprocess.check_output(ssh_args+[shlex.join(command)]))
                assert not remote['result']['system']['available'] and 'SSH' in remote['result']['system']['reason'],'SSH controller advertised notification delivery to the remote desktop'
            # Corrupt configuration never prevents lifecycle reports, nor silently replaces user settings.
            settings_path=tmp/'config/starflux/notifications.json';prior=settings_path.read_bytes();settings_path.write_text('{broken')
            publish(name,first,'working');publish(name,first,'waiting_for_approval')
            api('notifications.settings',ok=False);assert settings_path.read_text()=='{broken';settings_path.write_bytes(prior)
            if args.notification_service:
                native=[json.loads(line) for line in (tmp/'native-notifications').read_text().splitlines()]
                assert len(native)==6 and all(n['app']=='STAR/FLUX' and n['title'].startswith('STAR/FLUX') for n in native),native
            assert alive(original_pid) and a.process.poll() is None and b.process.poll() is None
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,native_dbus_service=args.notification_service,paginated_inbox=True,unicode_inbox_search=True,read_history=True,per_frontend_filters=True,state_source_session_filters=True,guarded_mark_read=True,desktop_deduplication=True,active_pane_suppression=True,helper_failure_visible=True,retention_bounds=True,conversation_identity_preserved=True,processes_preserved=True)))
            raise SystemExit(0)
        if args.machines:
            import uuid
            def api(method,params=None,write=False,ok=True,machine=None):
                request=dict(version=1,id='machine-proof',method=method,params=params or {},deadline_ms=35000)
                if write:request['operation_id']=str(uuid.uuid4())
                command=[str(app),'--api',json.dumps(request)]
                if machine:command+=['--machine',machine]
                response=json.loads(subprocess.check_output(command,env=env,stderr=subprocess.PIPE))
                assert response['ok']==ok,response
                return response['result'] if ok else response['error']
            def menu(title):return wait(lambda:(m if (m:=a.snapshot().get('context_menu')) and m['title']==title else None),'missing '+title)
            def fill(index,text):
                a.action('modal:field:'+str(index));wait(lambda:a.snapshot().get('modal_field')==index,'field did not focus');a.key('char:e',2);a.key('char:u',2);a.paste(text)
            assert api('machines.list')['machines']==[]
            # An installed helper with a path containing spaces, under private SSH environment.
            wrapper=tmp/'workspace helper'
            wrapper.write_text('#!/usr/bin/python3\nimport os,sys,json\nfrom pathlib import Path\nwith Path('+repr(str(tmp/'machine-invocations'))+').open("a") as f:f.write(json.dumps(sys.argv[1:])+"\\n")\nos.execv('+repr(str(app))+',['+repr(str(app))+',*sys.argv[1:]])\n');wrapper.chmod(0o700)
            a.shortcut('char:m',1);menu('Machines')
            a.key('enter');wait(lambda:a.snapshot().get('modal_open'),'machine onboarding missing')
            for i,value in enumerate(['Build machine 界','proof',str(host_config),str(wrapper),name]):fill(i,value)
            a.accept();menu('SSH machine')
            saved=api('machines.list')['machines'][0]
            assert saved['label']=='Build machine 界' and saved['workspace']==name
            assert (tmp/'config/starflux/machines.json').stat().st_mode&0o777==0o600
            a.action('machines:test')
            checked=wait(lambda:a.snapshot().get('machine_checks',{}).get(saved['id']),'connection probe missing',timeout=35)
            assert checked['connected'] and checked['project_terminals_ready'] and checked['workspace_api_ready'],checked
            assert (tmp/'machine-invocations').exists(),'probe did not invoke the remote helper'
            remote=api('sessions.list',machine=saved['id'])
            assert any(e['id']=='base' for e in remote['sessions']),remote
            check_startup(name,original_pid,machine=saved['id'])
            api('sessions.default',dict(session='base'),write=True)
            defaults=api('machines.save',dict(label='Remote default',target='proof',ssh_config=str(host_config),remote_executable=str(wrapper)),write=True)
            check_startup(name,original_pid,machine=defaults['label'])
            api('machines.remove',dict(machine=defaults['id'],expected_revision=defaults['revision']),write=True)
            # Profiles are catalog metadata; opening a controller and probing must keep existing processes.
            assert alive(original_pid) and b.snapshot()['active_project']==name
            a.action('machines:command');wait(lambda:a.clipboards and saved['id'] in a.clipboards[-1],'launch command not copied')
            a.action('machines:setup');wait(lambda:a.clipboards and str(host_config) in a.clipboards[-1] and 'ssh' in a.clipboards[-1],'SSH setup command not copied')
            a.action('machines:project');wait(lambda:a.snapshot().get('modal_open'),'remote project form missing')
            child='machine-child';fill(0,child);fill(1,str(tmp));a.accept()
            child_view=wait(lambda:(s if (s:=a.snapshot()).get('active_project')==child and s.get('panes') and s['panes'][0].get('pid') else None),'machine project failed',timeout=25)
            child_pid=child_view['panes'][0]['pid'];assert child_pid!=original_pid and alive(original_pid)
            config=json.loads((tmp/'data/starflux/projects'/f'{child}.json').read_text())
            assert config['host']=='proof' and config['ssh_config']==str(host_config) and config['desktop_helper']==str(wrapper)
            assert b.snapshot()['active_project']==name and b.snapshot()['panes'][0]['pid']==original_pid
            a.shortcut('char:m',1);menu('Machines');a.action('machines:entry:'+saved['id']);menu('SSH machine')
            a.action('machines:toggle');wait(lambda:(m:=a.snapshot().get('machine_profile')) and not m['enabled'],'machine did not disable')
            disabled=api('machines.list')['machines'][0];assert not disabled['enabled']
            fail=subprocess.run([str(app),'--machine',saved['id'],'--api',json.dumps(dict(version=1,method='capabilities',params={}))],env=env,capture_output=True)
            assert fail.returncode and b'disabled' in fail.stderr and not fail.stdout,'disabled machine silently fell back locally'
            assert alive(child_pid) and alive(original_pid),'disabling killed running sessions'
            update={k:v for k,v in disabled.items() if k not in ('id','revision')};update.update(id=disabled['id'],expected_revision=disabled['revision'],label='Renamed build machine',enabled=True)
            renamed=api('machines.save',update,write=True)
            api('machines.save',update,write=True,ok=False)
            api('machines.remove',dict(machine=saved['id'],expected_revision=saved['revision']),write=True,ok=False)
            # Every editor has a revision: a stale view must not overwrite a newer edit.
            a.action('machines:remove');wait(lambda:'changed' in a.snapshot().get('status','').lower(),'stale menu action was accepted')
            assert len(api('machines.list')['machines'])==1
            a.action('machines:entry:'+renamed['id']);menu('SSH machine')
            a.action('machines:edit');wait(lambda:a.snapshot().get('modal_open'),'profile editing missing')
            fill(0,'Edited build machine');a.accept();menu('SSH machine')
            edited=api('machines.list')['machines'][0];assert edited['label']=='Edited build machine'
            a.action('machines:remove');menu('Machines')
            assert api('machines.list')['machines']==[] and alive(child_pid) and alive(original_pid),'profile removal stopped sessions'
            a.action('machines:local');menu('Machine projects')
            assert any(i['action']=='machines:open:'+name for i in a.snapshot()['context_menu']['items'])
            a.action('machines:open:'+name);wait(lambda:a.snapshot().get('active_project')==name,'machine project navigation failed')
            # A missing helper is reported separately from working SSH/Python/tmux.
            absent=api('machines.save',dict(label='Missing helper',target='proof',ssh_config=str(host_config),remote_executable=str(tmp/'missing-helper')),write=True)
            absent_check=api('machines.test',dict(machine=absent['id']))
            assert absent_check['connected'] and absent_check['project_terminals_ready'] and not absent_check['workspace_api_ready'],absent_check
            api('machines.remove',dict(machine=absent['id'],expected_revision=absent['revision']),write=True)
            # Input diagnostics, duplicate labels, serialization and output limits.
            valid=dict(label='Validation',target='proof',ssh_config=str(host_config),remote_executable=str(wrapper))
            first=api('machines.save',valid,write=True)
            api('machines.save',valid,write=True,ok=False)
            api('machines.project',dict(machine=first['id'],project='invalid-group',directory=str(tmp),session='bad/name'),write=True,ok=False)
            assert not (tmp/'data/starflux/projects/invalid-group.json').exists()
            for key,value in [('target','-oProxyCommand=false'),('target','bad\nvalue'),('workspace','bad/name'),('remote_executable',''),('ssh_config',str(tmp/'missing'))]:
                api('machines.save',{**valid,'label':'Invalid '+key,key:value},write=True,ok=False)
            from concurrent.futures import ThreadPoolExecutor
            with ThreadPoolExecutor(max_workers=6) as pool:
                profiles=list(pool.map(lambda n:api('machines.save',{**valid,'label':'Concurrent '+str(n)},write=True),range(6)))
            assert len(api('machines.list')['machines'])==7,'concurrent edits lost profiles'
            noisy=tmp/'noisy helper';noisy.write_text('#!/usr/bin/python3\nprint("x"*1100000)\n');noisy.chmod(0o700)
            broken=api('machines.save',{**valid,'label':'Noisy helper','remote_executable':str(noisy)},write=True)
            failed_helper=api('machines.test',dict(machine=broken['id']))
            assert failed_helper['connected'] and not failed_helper['workspace_api_ready'] and 'limit' in failed_helper['workspace_error'],failed_helper
            unknown=tmp/'unknown-host-config'
            unknown.write_text(host_config.read_text().replace(str(tmp/'known_hosts'),str(tmp/'unknown-known-hosts')))
            untrusted=api('machines.save',{**valid,'label':'Unknown host','ssh_config':str(unknown)},write=True)
            error=api('machines.test',dict(machine=untrusted['id']),ok=False)
            assert 'host key' in error['message'].lower() and not (tmp/'unknown-known-hosts').exists(),error
            assert api('machines.test',dict(machine=first['id']))['workspace_api_ready'],'a failed endpoint disturbed a working endpoint'
            catalog=tmp/'config/starflux/machines.json';prior=catalog.read_bytes();catalog.write_text('{broken')
            api('machines.save',{**valid,'label':'Must not overwrite'},write=True,ok=False)
            assert catalog.read_text()=='{broken';catalog.write_bytes(prior)
            assert alive(original_pid) and alive(child_pid) and a.process.poll() is None and b.process.poll() is None
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,keyboard_onboarding=True,connection_probe=True,saved_workspace_launch=True,remote_api_forwarding=True,remote_project=True,profile_editing=True,concurrent_profiles=True,stale_edits_rejected=True,strict_host_key_check=True,failed_endpoint_isolation=True,removal_preserves_processes=True)))
            raise SystemExit(0)
        if args.worktrees:
            import uuid
            def api(method,params,write=False,ok=True):
                request=dict(version=1,id='worktree-proof',method=method,params=params)
                if write:request['operation_id']=str(uuid.uuid4())
                response=json.loads(subprocess.check_output([str(app),'--api',json.dumps(request)],env=env))
                assert response['ok']==ok,response
                return response['result'] if ok else response['error']
            path=tmp/'feature tree 界'
            a.shortcut('char:w',1)
            wait(lambda:(a.snapshot().get('context_menu') or {}).get('title')=='Git worktrees' and len(a.snapshot()['context_menu']['items'])==2,'worktree keyboard picker did not load')
            a.action('worktrees:create');wait(lambda:a.snapshot().get('modal_open'),'worktree create dialog missing')
            a.paste('feature/first');a.key('tab');a.paste(str(path));a.accept()
            opened=wait(lambda:(s if (s:=a.snapshot()).get('active_project')!=name and s.get('panes') and s['panes'][0].get('pid') else None),'worktree project did not open')
            child=opened['active_project'];child_pid=opened['panes'][0]['pid']
            assert child_pid!=original_pid and Path('/proc',str(child_pid),'cwd').resolve()==path
            assert b.snapshot()['active_project']==name and alive(original_pid),'worktree creation replaced another view or process'
            configs=tmp/'data/starflux/projects';child_config=json.loads((configs/(child+'.json')).read_text());association=json.loads((configs/(child+'.worktree.json')).read_text())
            assert child_config['collection']=='base' and association['origin']==name and child_config['host']==('proof' if args.ssh else None)
            listing=api('worktrees.list',dict(project=name));target=next(t for t in listing['worktrees'] if t['path']==str(path))
            assert api('worktrees.open',dict(project=name,target=target),True)['project']==child
            assert api('panes.get',dict(project=child,pane=1))['target']['pid']==child_pid,'reopening worktree replaced its shell'
            rejected=api('worktrees.remove',dict(project=name,target=target,confirm='REMOVE'),True,False)
            assert 'running terminal' in rejected['message'] and path.exists() and alive(child_pid),rejected
            b.action('project:'+child);wait(lambda:b.snapshot()['active_project']==child,'second view did not open grouped worktree project')
            reference=api('panes.get',dict(project=child,pane=1))['target'];api('panes.terminate',dict(target=reference,confirm='TERMINATE'),True)
            wait(lambda:all(c.snapshot()['project_state']['terminals']==[] for c in (a,b)),'worktree shell did not close in both views')
            note=path/'unsaved';note.write_text('keep draft')
            rejected=api('worktrees.remove',dict(project=name,target=target,confirm='REMOVE'),True,False)
            assert 'dirty_worktree' in rejected['message'] and note.read_text()=='keep draft',rejected
            note.unlink()
            a.action('worktrees:list');wait(lambda:(a.snapshot().get('context_menu') or {}).get('title')=='Git worktrees' and len(a.snapshot()['context_menu']['items'])==3,'worktree list did not refresh')
            a.action('worktrees:remove:1');wait(lambda:a.snapshot().get('modal_open'),'worktree removal dialog missing')
            a.key('escape');wait(lambda:not a.snapshot().get('modal_open'),'worktree removal did not cancel');assert path.exists()
            a.action('worktrees:remove:1');wait(lambda:a.snapshot().get('modal_open'),'second worktree removal dialog missing')
            a.paste('REMOVE');a.accept()
            wait(lambda:not path.exists(),'clean worktree did not remove')
            wait(lambda:all(c.snapshot()['active_project']==name for c in (a,b)),'removed worktree did not return both views to origin')
            assert all(c.snapshot()['panes'][0]['pid']==original_pid for c in (a,b)) and alive(original_pid),'removal replaced the original repository terminal'
            assert (project_directory/'source').read_text()=='source unchanged\n'
            assert subprocess.check_output(['git','-C',str(project_directory),'show-ref','--verify','refs/heads/feature/first'],env=env)
            removed=json.loads((configs/(child+'.worktree.json')).read_text());assert removed['state']=='removed'
            assert all(p['id']!=child for p in api('projects.list',{})['projects']),'removed project remained in session catalog'
            # A project already opened inside an external linked worktree keeps
            # its identity and subdirectory, then acquires repository grouping.
            external_path=tmp/'external tree';external=api('worktrees.create',dict(project=name,branch='existing/source',path=str(external_path)),True)['target']
            (external_path/'subdir').mkdir()
            existing='existing-tree';existing_config=json.loads((configs/(name+'.json')).read_text());existing_config.update(workspace=existing,label='Existing worktree',directory=str(external_path/'subdir'))
            (configs/(existing+'.json')).write_text(json.dumps(existing_config));(configs/(existing+'.model.json')).write_text((configs/(name+'.model.json')).read_text())
            current_target=next(t for t in api('worktrees.list',dict(project=existing))['worktrees'] if t['current'])
            assert api('worktrees.open',dict(project=existing,target=current_target),True)['project']==existing
            existing_reference=api('panes.get',dict(project=existing,pane=1))['target']
            assert Path('/proc',str(existing_reference['pid']),'cwd').resolve()==external_path/'subdir'
            assert json.loads((configs/(existing+'.worktree.json')).read_text())['origin']==name
            assert api('worktrees.open',dict(project=existing,target=current_target),True)['project']==existing
            assert api('panes.get',dict(project=existing,pane=1))['target']['pid']==existing_reference['pid']
            auxiliary=None
            if not args.ssh:
                auxiliary=api('sessions.create',dict(name='Only linked worktree',directory=str(external_path/'subdir')),True)
                own=next(t for t in api('worktrees.list',dict(project=auxiliary['project']))['worktrees'] if t['current'])
                associated=api('worktrees.open',dict(project=auxiliary['project'],target=own),True)
                assert associated['project']==auxiliary['project'] and associated['origin'] not in (auxiliary['project'],name)
                auxiliary['origin']=associated['origin']
                assert not api('panes.get',dict(project=associated['origin'],pane=1))['status']['alive'],'group discovery started an unrequested parent shell'
                own_reference=api('panes.get',dict(project=auxiliary['project'],pane=1))['target']
                api('panes.terminate',dict(target=own_reference,confirm='TERMINATE'),True)
            api('panes.terminate',dict(target=existing_reference,confirm='TERMINATE'),True)
            api('worktrees.remove',dict(project=name,target=external,confirm='REMOVE'),True)
            assert not external_path.exists() and alive(original_pid)
            if auxiliary:assert next(s for s in api('sessions.list',{})['sessions'] if s['id']==auxiliary['session'])['project']==auxiliary['origin']
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,create_open_remove=True,keyboard_picker=True,host_affinity=True,repository_grouping=True,existing_process_reattach=True,live_terminal_guard=True,dirty_guard=True,cancel_preserves_tree=True,other_views_return_to_origin=True,branch_retained=True,original_process_preserved=True)))
            raise SystemExit(0)
        if args.automation:
            import uuid
            def api(method,params,write=False):
                request=dict(version=1,id='ui-proof',method=method,params=params)
                if write:request['operation_id']=str(uuid.uuid4())
                command=[str(app),'--api',json.dumps(request)]
                if args.ssh:command=ssh_args+[shlex.join(['env',*[k+'='+v for k,v in env.items() if k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','PATH')],*command])]
                response=json.loads(subprocess.check_output(command,env=env));assert response['ok'],response
                return response['result']
            created=api('panes.create',dict(project=name,label='API tab'),True)
            wait(lambda:all(len(c.snapshot()['project_state']['terminals'])==2 for c in (a,b)),'API tab was not shared with both frontends')
            assert a.snapshot()['active']==0 and b.snapshot()['active']==0,'headless API stole frontend focus'
            target=api('panes.get',dict(project=name,pane=2))['target']
            api('panes.rename',dict(target=target,label='API renamed'),True)
            wait(lambda:all(c.snapshot()['project_state']['terminal_labels'].get('1')=='API renamed' for c in (a,b)),'API rename did not reach both frontends')
            api('panes.send',dict(target=target,text='API_EXACT_TARGET',submit=True),True)
            wait(lambda:'REPLY=API_EXACT_TARGET' in api('panes.capture',dict(project=name,pane=2))['text'],'API input missed its target')
            assert all(not any('API_EXACT_TARGET' in t['text'] for t in c.snapshot().get('direct_terminals',[])) for c in (a,b)),'API input reached a focused sibling pane'
            api('panes.terminate',dict(target=target,confirm='TERMINATE'),True)
            wait(lambda:all(len(c.snapshot()['project_state']['terminals'])==1 for c in (a,b)),'API termination did not update both views')
            assert all(c.snapshot()['panes'][0]['pid']==original_pid for c in (a,b)) and alive(original_pid)
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,api_changes_shared=True,independent_focus_preserved=True,guarded_pane_input=True,guarded_termination=True,original_process_preserved=True)))
            raise SystemExit(0)
        if args.terminal_history:
            def review():return a.snapshot().get('terminal_review') or {}
            def live(client,text):return any(text in t['text'] for t in client.snapshot().get('direct_terminals',[]))
            a.paste('HISTORY\n');wait(lambda:live(a,'ROW 119') and live(b,'ROW 119'),'history fixture did not finish')
            a.shortcut('char:/');wait(lambda:review().get('editing'),'search shortcut did not open')
            a.paste('NEEDLE 界éX');a.key('left');a.key('left');a.key('right');a.key('delete');a.key('enter')
            wait(lambda:review().get('matches')==2 and review().get('offset',0)>0,'search missed retained history or Unicode')
            def frame_text():return '\n'.join(''.join(run['text'] for run in row['runs']) for row in a.frames.get(0,{}).get('rows',[]))
            wait(lambda:'NEEDLE 界é' in frame_text(),'searched history was not rendered')
            assert not b.snapshot().get('terminal_review'),'copy mode leaked into the other frontend'
            first=review()['matched'];a.key('char:n')
            wait(lambda:review().get('matched')!=first,'next search match did not navigate')
            a.key('char:n',1);wait(lambda:review().get('matched')==first,'previous match did not navigate')
            retained=review()['retained_rows'];position=review()['cursor']
            b.paste('OUTPUT_WHILE_REVIEWING\n')
            wait(lambda:live(a,'REPLY=OUTPUT_WHILE_REVIEWING') and live(b,'REPLY=OUTPUT_WHILE_REVIEWING'),'review paused the underlying terminal')
            assert review()['retained_rows']==retained and review()['cursor']==position,'new output shifted the review snapshot'
            assert 'OUTPUT_WHILE_REVIEWING' not in frame_text(),'live output replaced the frozen rendered history'
            a.key('enter');wait(lambda:a.clipboards and not a.snapshot().get('terminal_review'),'search match did not copy and exit')
            assert a.clipboards[-1]=='NEEDLE 界é',a.clipboards
            a.shortcut('space');wait(lambda:review().get('retained_rows') and not review().get('loading'),'copy-mode shortcut did not open')
            if args.cells:
                cursor=review()['cursor'];a.key('f:6');wait(lambda:not a.snapshot().get('cell_terminal_focus'),'copy mode trapped workspace focus')
                a.key('down');a.key('f:6');wait(lambda:a.snapshot().get('cell_terminal_focus'),'terminal focus did not restore')
                assert review()['cursor']==cursor,'workspace control navigation moved the review cursor'
            a.key('f:1');wait(lambda:a.snapshot().get('context_menu'),'help did not open during review')
            a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'help did not close during review')
            assert review(),'closing help exited copy mode'
            a.key('home',2);a.key('space');a.key('pagedown');a.key('end');a.key('char:y')
            wait(lambda:len(a.clipboards)==2 and not a.snapshot().get('terminal_review'),'cross-page selection did not copy')
            assert 'ROW 000 history text' in a.clipboards[-1] and 'ROW 007 NEEDLE 界é' in a.clipboards[-1],a.clipboards[-1]
            a.shortcut('char:/');wait(lambda:review().get('editing'),'second search did not open')
            a.paste('ABSENT_NEVER_SENT');a.key('enter');wait(lambda:not review().get('editing'),'failed search did not finish')
            assert review().get('matches')==0
            a.key('enter');a.paste('DO_NOT_SEND');a.key('char:j')
            a.key('escape');wait(lambda:not a.snapshot().get('terminal_review'),'copy mode did not exit')
            a.paste('AFTER_REVIEW\n');wait(lambda:live(a,'REPLY=AFTER_REVIEW'),'terminal input did not resume')
            assert not live(a,'REPLY=DO_NOT_SEND') and not live(a,'REPLY=ABSENT_NEVER_SENT'),'review input reached the program'
            assert a.snapshot()['panes'][0]['pid']==original_pid and alive(original_pid)
            a.shortcut('pageup');wait(lambda:review().get('offset',0)>0 and not review().get('loading'),'keyboard PageUp did not load retained backend history')
            a.shortcut('end');wait(lambda:not review(),'keyboard End did not return to live output')
            a=restart(a)
            a.action('terminal:new');wait(lambda:a.snapshot().get('active')!=0 and len(a.snapshot().get('direct_terminals',[]))==1,'hidden history fixture tab did not open')
            a.action('pane-target:0:search-history');wait(lambda:review().get('editing'),'hidden tab history search did not open')
            a.paste('NEEDLE 界é');a.key('enter');wait(lambda:review().get('matches')==2 and not review().get('loading'),'history predating attachment was lost')
            a.key('escape');wait(lambda:not review(),'reattached history did not close')
            tmux('send-keys','-t','Fixture','-l','HISTORY');tmux('send-keys','-t','Fixture','Enter')
            wait(lambda:'ROW 119' in tmux('capture-pane','-p','-t','Fixture'),'tmux source history fixture missing')
            picker(a,external=True);choose(a,'Fixture')
            wait(lambda:a.snapshot().get('panes') and a.snapshot()['panes'][0].get('pid')==source_pid,'history tmux import did not attach')
            a.shortcut('char:/');wait(lambda:review().get('editing'),'imported tmux search did not open')
            a.paste('NEEDLE 界é');a.key('enter');wait(lambda:review().get('matches')==2 and review().get('source')=='tmux','imported tmux retained history missing')
            assert tmux('display-message','-p','-t','Fixture','#{pane_in_mode}')=='0','read-only search changed tmux copy mode'
            a.key('enter');wait(lambda:a.clipboards[-1:] == ['NEEDLE 界é'] and not review(),'imported history did not copy')
            assert tmux('list-panes','-s','-t','Fixture','-F','#{pane_id} #{pane_pid}')==source_topology
            if args.herdr:
                picker(a,external=True);choose(a,'default')
                wait(lambda:a.snapshot().get('collection')=='herdr-default' and a.snapshot().get('direct_terminals'),'Herdr history import missing')
                a.paste('HISTORY\n');wait(lambda:live(a,'ROW 119'),'Herdr history fixture missing')
                a.shortcut('char:/');wait(lambda:review().get('editing'),'Herdr search did not open')
                a.paste('NEEDLE 界é');a.key('enter');wait(lambda:review().get('matches')==2 and review().get('source')=='herdr','Herdr retained history missing')
                a.key('enter');wait(lambda:a.clipboards[-1:] == ['NEEDLE 界é'] and not review(),'Herdr history did not copy')
                assert all(alive(pid) for pid in herdr_pids),'history capture replaced a Herdr source process'
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,search_retained_history=True,unicode_search_and_copy=True,match_navigation=True,cross_page_selection=True,frozen_during_output=True,independent_frontends=True,terminal_input_preserved=True,pre_attachment_history=True,imported_tmux_history=True,imported_herdr_history=args.herdr)))
            raise SystemExit(0)
        if args.pane_layouts:
            def leaves(client=a):return client.snapshot().get('pane_layout',{}).get('panes',[])
            def native_frames_match():
                boxes=leaves()
                if not all(p['pane'] in a.frames for p in boxes):return False
                frames={p['pane']:a.frames[p['pane']]['rect'] for p in boxes}
                for p in boxes:
                    r=frames[p['pane']]
                    if min(r['width'],r['height'])<=0:return False
                    for q in boxes:
                        if p==q:continue
                        s,t=p['rect'],q['rect'];other=frames[q['pane']]
                        if s['x']>=t['x']+t['width'] and r['x']<other['x']+other['width']:return False
                        if s['y']>=t['y']+t['height'] and r['y']<other['y']+other['height']:return False
                return True
            wait(lambda:0 in a.frames,'initial native frame missing');full_rect=a.frames[0]['rect']
            a.shortcut('char:%');wait(lambda:len(leaves())==2,'right split missing')
            a.shortcut('char:"');wait(lambda:len(leaves())==3,'nested down split missing')
            a.action('terminal:0');a.action('terminal:split-down')
            wait(lambda:len(leaves())==4 and len(a.snapshot().get('direct_terminals',[]))==4,'four-pane layout missing')
            wait(native_frames_match,'native terminal frames overlap after nested split')
            for axis in ['down','right']:
                previous=a.snapshot()['project_state']['pane_tree']
                divider=next(d for d in a.snapshot()['pane_layout']['dividers'] if d['axis']==axis)
                r=divider['rect'];x=r['x']+r['width']/2;y=r['y']+r['height']/2
                a.pointer('down',x,y)
                if axis=='down':y+=2 if args.cells else 30
                else:x+=4 if args.cells else 40
                a.pointer('move',x,y);a.pointer('up',x,y)
                wait(lambda:a.snapshot()['project_state']['pane_tree']!=previous,'nested divider did not resize along '+axis)
                wait(native_frames_match,'native terminal frames overlap after divider resize')
            before_height=next(p['rect']['height'] for p in leaves() if p['pane']==3)
            a.shortcut('char:>')
            wait(lambda:next(p['rect']['height'] for p in leaves() if p['pane']==3)>before_height,'resize shortcut did not grow the focused second child')
            tree=a.snapshot()['project_state']['pane_tree']
            assert len(leaves(b))==1 and not b.snapshot()['project_state']['split'],'layout leaked into another frontend'
            pids=wait(lambda:(result if len(result:={s['pane']:s['health']['pid'] for s in a.snapshot()['sessions'] if (s.get('health') or {}).get('pid')})==4 else None),'nested terminal PIDs missing')
            for p in leaves():
                r=p['rect'];a.pointer('down',r['x']+r['width']/2,r['y']+r['height']/2);a.pointer('up',r['x']+r['width']/2,r['y']+r['height']/2)
                wait(lambda:a.snapshot()['active']==p['pane'],'pointer focused wrong row/column')
                marker='PANE_'+str(p['pane'])+'_INPUT';a.paste(marker);a.key('enter')
                wait(lambda:any(t['pane']==p['pane'] and 'REPLY='+marker in t['text'] for t in a.snapshot()['direct_terminals']),'input reached wrong nested pane')
            a.shortcut('char:z');wait(lambda:len(leaves())==1,'pane zoom did not hide other attachments')
            focused=a.snapshot()['active'];wait(lambda:a.frames[focused]['rect']==full_rect,'zoomed terminal did not fill canvas')
            a.shortcut('char:z');wait(lambda:len(leaves())==4 and a.snapshot()['project_state']['pane_tree']==tree,'unzoom lost pane tree')
            a=restart(a)
            # Closures use the reassigned frontend after reattachment.
            wait(lambda:len(a.snapshot().get('pane_layout',{}).get('panes',[]))==4,'nested layout did not persist')
            assert a.snapshot()['project_state']['pane_tree']==tree
            assert all(alive(pid) for pid in pids.values()) and alive(original_pid)
            a.action('terminal:3');a.action('terminal:focus-right');wait(lambda:a.snapshot()['active']==2,'directional pane focus failed')
            a.action('terminal:focus-up');wait(lambda:a.snapshot()['active']==1,'vertical pane focus failed')
            a.action('terminal:move-left');wait(lambda:a.snapshot()['project_state']['pane_tree']!=tree,'pane movement did not rearrange layout')
            assert a.snapshot()['active']==1 and all(alive(pid) for pid in pids.values())
            a.action('terminal:close-pane');wait(lambda:len(a.snapshot()['pane_layout']['panes'])==3,'closing a pane collapsed unrelated branches')
            assert len(a.snapshot()['project_state']['terminals'])==4 and all(alive(pid) for pid in pids.values()),'closing view terminated a session'
            a.shortcut('char:x');wait(lambda:a.snapshot().get('context_menu'),'pane menu did not open')
            a.input(dict(kind='pointer',action='scroll_down',button=0,x=5,y=5,modifiers=0))
            wait(lambda:a.snapshot()['context_menu']['selected']>=8,'context menu wheel navigation failed')
            a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'pane menu did not close')
            before=a.snapshot()['project_state']['pane_tree']
            b.action('terminal:new');wait(lambda:len(b.snapshot()['project_state']['terminals'])==5 and b.snapshot()['active']==4,'hidden terminal creation failed')
            wait(lambda:len(a.snapshot()['project_state']['terminals'])==5,'hidden terminal creation did not reach the other view')
            b.key('char:d',2);wait(lambda:all(len(c.snapshot()['project_state']['terminals'])==4 for c in (a,b)),'hidden terminal exit did not propagate')
            assert a.snapshot()['project_state']['pane_tree']==before,'hidden terminal exit damaged nested layout'
            staged=state/(a.identity+'.actions')/'00000000000000000000-unpublished.json.4242'
            staged.parent.mkdir(exist_ok=True);staged.write_text(json.dumps('terminal:new'))
            a.action('keyboard:help');wait(lambda:(a.snapshot().get('context_menu') or {}).get('title')=='Keyboard shortcuts','committed action did not execute')
            assert staged.exists(),'reader removed an unpublished temporary action'
            assert len(a.snapshot()['project_state']['terminals'])==4,('unpublished action changed membership',a.snapshot()['project_state']['terminals'])
            staged.unlink();a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'queue publication test did not close help')
            a.action('terminate-terminal:1');wait(lambda:a.snapshot().get('modal_open'),'termination confirmation missing')
            a.key('escape');wait(lambda:not a.snapshot().get('modal_open'),'termination did not cancel')
            assert alive(pids[1]),'canceling termination stopped a terminal'
            a.action('terminate-terminal:1');wait(lambda:a.snapshot().get('modal_open'),'stale confirmation test did not open')
            b.action('terminal:1');wait(lambda:b.snapshot()['active']==1,'other view did not select termination target')
            b.key('char:d',2);wait(lambda:1 not in a.snapshot()['project_state']['terminals'],'old terminal did not exit')
            b.action('terminal:new');wait(lambda:b.snapshot()['active']==1 and 1 in a.snapshot()['project_state']['terminals'],'terminal slot did not recycle')
            new_pid=wait(lambda:next((s['health']['pid'] for s in a.snapshot()['sessions'] if s['pane']==1 and (s.get('health') or {}).get('pid') and s['health']['pid']!=pids[1]),None),'recycled terminal health missing')
            a.paste('TERMINATE');a.accept();wait(lambda:a.snapshot().get('modal_open') and 'Terminal changed' in (a.snapshot().get('modal_error') or ''),'stale termination was not rejected')
            assert alive(new_pid),'stale confirmation killed a recycled terminal'
            a.key('escape');wait(lambda:not a.snapshot().get('modal_open'),'stale termination dialog did not close')
            a.action('terminate-terminal:1');wait(lambda:a.snapshot().get('modal_open'),'fresh termination dialog missing')
            a.paste('TERMINATE');a.accept()
            wait(lambda:1 not in a.snapshot()['project_state']['terminals'] and not alive(new_pid),'confirmed termination did not stop only its terminal')
            assert all(alive(pid) for pane,pid in pids.items() if pane!=1) and len(a.snapshot()['pane_layout']['panes'])==3,'termination damaged unrelated panes'
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,nested_right_down=True,native_frames_do_not_overlap=True,pointer_focus_and_input=True,zoom_fills_canvas=True,layout_reattach=True,directional_focus_and_move=True,close_keeps_process=True,independent_frontends=True,hidden_exit_preserves_tree=True)))
            raise SystemExit(0)
        if args.agent_lifecycle:
            def cli(*arguments,success=True,workspace=None):
                command=[str(app),'--workspace',workspace or name,*arguments]
                if args.ssh:command=ssh_args+[shlex.join(['env',*[k+'='+env[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','STAR_TERMINAL_RENDERER','STARFLUX_HERDR','PATH')],*command])]
                result=subprocess.run(command,env=env,capture_output=True,text=True)
                assert (result.returncode==0)==success,result.stderr
                return json.loads(result.stdout) if success else None
            info=cli('--agent-status')['panes'][0]
            assert info['status']['health']['agent_process']=='running'
            run=secrets.token_hex(16)
            report=dict(run_id=run,sequence=1,epoch=info['epoch'],agent_pid=info['status']['agent_pid'],agent_start_ticks=info['status']['agent_start_ticks'],boot_id=info['status']['boot_id'],harness='codex',source='fixture:hook',state='working',conversation_id='exact-thread',resume_argv=[str(shell),'exact-thread'])
            cli('--pane','1','--agent-report',json.dumps(report))
            def health():return next((s['health'] for s in a.snapshot().get('sessions',[]) if s['pane']==0),{}) or {}
            wait(lambda:health().get('health',{}).get('source')=='integration','structured lifecycle report did not reach the UI')
            assert health()['state']=='working' and health()['agent_record']['conversation_id']=='exact-thread'
            cli('--pane','1','--agent-report',json.dumps(report),success=False)
            stale={**report,'sequence':2,'epoch':'recycled-pane'}
            cli('--pane','1','--agent-report',json.dumps(stale),success=False)
            report.update(sequence=2,state='completed')
            cli('--pane','1','--agent-report',json.dumps(report))
            wait(lambda:health().get('state')=='completed','completion report did not replace working state')
            wait(lambda:a.snapshot().get('attention_unread',0)==1,'completion did not produce an in-app notification')
            a.shortcut('char:i')
            wait(lambda:(a.snapshot().get('context_menu') or {}).get('title')=='Attention inbox','attention shortcut did not open inbox')
            entries=[i for i in a.snapshot()['context_menu']['items'] if i['action'].startswith('attention:view:')];assert len(entries)==1 and entries[0]['action'].startswith('attention:view:'),entries
            alert=entries[0]['action'].removeprefix('attention:view:')
            a.action(entries[0]['action'])
            wait(lambda:(a.snapshot().get('context_menu') or {}).get('title')=='Attention item','attention item did not open')
            a.action('attention:dismiss:'+alert)
            wait(lambda:(a.snapshot().get('context_menu') or {}).get('items',[{}])[0].get('label')=='No unread attention items','dismissal did not update inbox')
            wait(lambda:a.snapshot().get('attention_unread')==0,'dismissed item remained in unread count')
            a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'attention inbox did not close')
            report['sequence']=3;cli('--pane','1','--agent-report',json.dumps(report))
            b.action('sessions:new');wait(lambda:b.snapshot().get('modal_open'),'second session dialog missing')
            b.paste('Attention other session');b.accept()
            wait(lambda:b.snapshot().get('session_id')!='base' and b.snapshot().get('panes',[{}])[0].get('agent_pid'),'second session agent missing')
            other_workspace=b.snapshot()['active_project']
            other=cli('--agent-status',workspace=other_workspace)['panes'][0]
            pending={**report,'run_id':secrets.token_hex(16),'sequence':1,'epoch':other['epoch'],'agent_pid':other['status']['agent_pid'],'agent_start_ticks':other['status']['agent_start_ticks'],'boot_id':other['status']['boot_id'],'state':'waiting_for_approval','conversation_id':'other-thread','resume_argv':None}
            cli('--pane','1','--agent-report',json.dumps(pending),workspace=other_workspace)
            a.shortcut('char:i');wait(lambda:(a.snapshot().get('context_menu') or {}).get('title')=='Attention inbox','global inbox missing')
            entries=[i for i in a.snapshot()['context_menu']['items'] if i['action'].startswith('attention:view:')];assert len(entries)==1 and entries[0]['action'].startswith('attention:view:'+other_workspace+':'),entries
            a.action(entries[0]['action'].replace('attention:view:','attention:open:'))
            wait(lambda:a.snapshot().get('active_project')==other_workspace and not a.snapshot().get('context_menu'),'attention item did not focus its session/project')
            assert alive(original_pid),'opening attention item stopped another project'
            picker(a);choose(a,'base');wait(lambda:a.snapshot().get('active_project')==name,'did not return to original project')
            a.paste('FAIL');a.key('enter')
            wait(lambda:health().get('health',{}).get('agent_backend')=='unavailable','backend failure was reported as healthy idle')
            assert health()['health']['terminal']=='alive' and health()['health']['agent_process']=='running'
            wait(lambda:a.snapshot()['project_activity'][name]['failed']==1,'failed agent missing from sidebar activity')
            a.action('agent-status:0')
            wait(lambda:any(i['action'].startswith('agent-resume:') for i in (a.snapshot().get('context_menu') or {}).get('items',[])),'exact conversation resume command missing')
            a.action('agent-resume:0:'+run)
            wait(lambda:len(a.snapshot()['project_state']['terminals'])==2 and any('RESUMED_EXACT_THREAD' in t['text'] for t in a.snapshot().get('direct_terminals',[])),'saved conversation did not resume in its own tab')
            assert alive(original_pid),'resuming terminated the original client'
            commands=a.snapshot()['project_state']['terminal_commands'];assert len(commands)==1 and 'exact-thread' in next(iter(commands.values()))
            resumed_pids={s['pane']:s['health']['pid'] for s in a.snapshot()['sessions'] if (s.get('health') or {}).get('pid')}
            a=restart(a)
            assert a.snapshot()['project_state']['terminal_commands']==commands,'resume command was not durable across refresh'
            wait(lambda:all(next((s['health'].get('pid') for s in a.snapshot()['sessions'] if s['pane']==pane and s.get('health')),None)==pid for pane,pid in resumed_pids.items()),'reattachment replaced a resumed process')
            saved=cli('--agent-status')['records'];assert len(saved)==1 and saved[0]['conversation_id']=='exact-thread'
            a.shortcut('char:i');wait(lambda:(a.snapshot().get('context_menu') or {}).get('title')=='Attention inbox','reattached inbox missing')
            entries=[i for i in a.snapshot()['context_menu']['items'] if i['action'].startswith('attention:view:')];assert len(entries)==1 and 'Backend unavailable' in entries[0]['label'],entries
            a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'inbox did not close after reattach')
            resumed_pane=int(next(iter(commands)))
            launch_receipt=tmp/'data/starflux/screens'/name/('p'+str(resumed_pane)+'.launch.json')
            wait(lambda:launch_receipt.exists(),'explicit resume did not consume a durable launch receipt')
            assert launch_receipt.stat().st_mode&0o777==0o600
            a.action('recover-terminal:'+str(resumed_pane))
            wait(lambda:a.snapshot().get('modal_open'),'recovery confirmation missing for resumed pane')
            a.paste('RECOVER');a.accept()
            wait(lambda:str(resumed_pane) not in a.snapshot()['project_state']['terminal_commands'],'recover to shell retained an automatic agent resume command')
            assert alive(original_pid),'recovery of resumed pane killed its source'
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,structured_lifecycle=True,layered_health=True,backend_failure_detected=True,stale_reports_rejected=True,exact_conversation_resume=True,original_process_preserved=True,durable_identity=True)))
            raise SystemExit(0)
        # Use both real input paths, including canvas keys with overlays open.
        a.key('f:1')
        wait(lambda:(a.snapshot().get('context_menu') or {}).get('title')=='Keyboard shortcuts','keyboard help did not open')
        help_items=a.snapshot()['context_menu']['items']
        assert not args.cells or not any(i['action']=='keyboard:apps' for i in help_items)
        a.key('end');wait(lambda:a.snapshot()['context_menu']['selected']==len(help_items)-1,'last help command not keyboard accessible')
        a.key('pageup');wait(lambda:a.snapshot()['context_menu']['selected']==max(0,len(help_items)-9),'help page navigation failed')
        a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'help did not close')
        a.shortcut('char:c');wait(lambda:len(a.snapshot()['project_state']['terminals'])==2,'new-tab shortcut failed')
        a.shortcut('char:p');wait(lambda:a.snapshot()['active']==0,'previous-tab shortcut failed')
        a.shortcut('char:n');wait(lambda:a.snapshot()['active']!=0,'next-tab shortcut failed')
        a.shortcut('char:1');wait(lambda:a.snapshot()['active']==0,'tab selection shortcut failed')
        a.shortcut('char:v');wait(lambda:a.snapshot()['project_state']['split'],'split shortcut failed')
        selected=a.snapshot()['active']
        a.shortcut('char:o');wait(lambda:a.snapshot()['active']!=selected,'pane focus shortcut failed')
        a.shortcut('char:v');wait(lambda:not a.snapshot()['project_state']['split'],'unsplit shortcut failed')
        a.shortcut('char:1');wait(lambda:a.snapshot()['active']==0,'original terminal did not select')
        a.shortcut('char:x');wait(lambda:any(i['action'].startswith('recover-terminal:') for i in (a.snapshot().get('context_menu') or {}).get('items',[])),'recovery menu not keyboard accessible')
        a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'pane menu did not close')
        a.shortcut('char:s');wait(lambda:a.snapshot().get('session_manager'),'session shortcut failed')
        a.key('escape');wait(lambda:not a.snapshot().get('modal_open'),'session picker did not close via canvas key')
        a.shortcut('char:r');wait(lambda:a.snapshot().get('modal_open'),'rename shortcut failed')
        a.key('escape');wait(lambda:not a.snapshot().get('modal_open'),'rename did not cancel')
        for character in 'KEYBOARD_PROOF':a.key('char:'+character)
        a.key('enter')
        wait(lambda:any('REPLY=KEYBOARD_PROOF' in t['text'] for t in a.snapshot().get('direct_terminals',[])),'ordinary terminal keys were intercepted')
        key_config=tmp/'config/starflux/keys.json';key_config.parent.mkdir(parents=True,exist_ok=True)
        key_config.write_text(json.dumps(dict(prefix='Ctrl+a',direct={'F2':'sessions','F1':'disabled'})))
        a.shortcut('char:r',1)
        a.key('f:2');wait(lambda:a.snapshot().get('session_manager'),'custom direct shortcut did not load')
        a.key('escape');wait(lambda:not a.snapshot().get('modal_open'),'custom shortcut picker did not close')
        key_config.write_text('{}')
        a.key('char:a',2);a.key('char:r',1)
        a.key('f:1');wait(lambda:(a.snapshot().get('context_menu') or {}).get('title')=='Keyboard shortcuts','default keymap did not restore')
        a.key('escape');wait(lambda:not a.snapshot().get('context_menu'),'restored help did not close')
        assert a.snapshot()['panes'][0]['pid']==original_pid and alive(original_pid),'shortcuts replaced the original process'
        if args.keyboard_only:
            print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,keyboard_navigation=True,custom_keymap=True,modal_focus=True,pane_recovery_menu=True,terminal_input_preserved=True)))
            raise SystemExit(0)
        before=a.snapshot()['project_order'];moved=before[-1];expected=[moved,*before[:-1]]
        def pointer(action,project):
            # Categories occupy real rows. Locate the project in the presented
            # sidebar, rather than treating its project-order index as a row.
            rows=a.snapshot()['categories']['rows']
            index=next(i for i,row in enumerate(rows) if row['id']==project)
            offset=a.snapshot().get('project_page',0)
            value=dict(kind='pointer',action=action,button=0,modifiers=0,x=5,y=5+index-offset)
            if not args.cells:
                height=lambda r:12 if r['id'].startswith('category-end:') else 44 if r['id'].startswith('category:') else 34
                y=122+sum(height(r) for r in rows[offset:index])+(height(rows[index])//2 if action=='down' else 1)
                value['pixel']=[40,y]
            a.input(value)
        pointer('down',moved);pointer('move',before[0]);pointer('up',before[0])
        wait(lambda:a.snapshot()['project_order']==expected,'drag order did not apply')
        wait(lambda:b.snapshot()['project_order']==expected,'session order was not shared')
        c=Client('v-session-order-proof',remote=args.ssh)
        wait(lambda:c.scene and c.snapshot().get('project_order')==expected,'fresh view lost drag order')
        subprocess.run([str(app),'--workspace',name,'--frontend',c.identity,'--close'],env=env,check=True,capture_output=True);c.process.wait(timeout=5)
        a.action('close-project:extra-a');wait(lambda:'extra-a' not in a.snapshot()['project_order'],'project did not close')
        assert b.snapshot()['project_order']==expected,'project close leaked to another window'
        a.action('project:new');wait(lambda:a.snapshot().get('modal_open'),'reopen dialog missing');a.paste('extra-a');a.accept()
        wait(lambda:a.snapshot()['project_order']==expected,'reopening a project lost its position')
        a.action('project:'+name);wait(lambda:a.snapshot().get('active_project')==name and a.snapshot()['panes'][0].get('pid')==original_pid,'return to original project failed')
        picker(a)
        assert any(r[1].removeprefix('● ')=='base' for r in a.snapshot()['session_manager']['rows'])
        a.action('sessions:new');wait(lambda:a.snapshot().get('session_manager') is None and a.snapshot().get('modal_open'),'new session dialog missing')
        a.paste('Proof café');a.accept()
        new=wait(lambda:(s if (s:=a.snapshot()).get('session_id')!='base' and s.get('panes') and s['panes'][0].get('pid') else None),'new session did not start')
        created=new['active_project'];created_pid=new['panes'][0]['pid'];created_group=new['session_id']
        assert b.snapshot()['active_project']==name and alive(original_pid),'Switching disturbed another view'
        picker(b);row(b,'Proof café');assert not b.snapshot()['session_manager']['selected_default']
        picker(a);row(a,'Proof café');a.action('sessions:default')
        wait(lambda:a.snapshot()['session_manager']['selected_default'],'default marker missing')
        wait(lambda:b.snapshot()['session_manager']['selected_default'],'other view did not update its default marker')
        b.action('modal:cancel');wait(lambda:not b.snapshot().get('modal_open'),'other view picker did not close')
        assert subprocess.check_output([str(app),'--default-workspace'],env=env,text=True).strip()==created
        # Setting it is durable, and a second action clears it without changing
        # session identity, running programs, or the active project.
        picker(a);row(a,'Proof café');assert a.snapshot()['session_manager']['selected_default']
        a.action('sessions:default');wait(lambda:not a.snapshot()['session_manager']['selected_default'],'default did not clear')
        a.action('sessions:default');wait(lambda:a.snapshot()['session_manager']['selected_default'],'default did not persist')
        check_startup(created,created_pid)
        check_startup(name,original_pid,explicit=True)
        picker(a);choose(a,'base');wait(lambda:a.snapshot().get('active_project')==name,'original session did not reopen')
        assert a.snapshot()['panes'][0]['pid']==original_pid and alive(created_pid)
        picker(a);row(a,'Proof café');a.action('sessions:delete');wait(lambda:a.snapshot().get('session_manager') is None,'delete dialog missing')
        a.input(dict(kind='key',code='escape',modifiers=0));wait(lambda:not a.snapshot().get('modal_open'),'delete did not cancel');assert alive(created_pid)
        delete(a,'Proof café')
        wait(lambda:not alive(created_pid),'owned terminal survived deletion')
        assert not (tmp/'data/starflux/projects'/(created+'.json')).exists()
        assert json.loads((tmp/'data/starflux/sessions/default-session').read_text()) is None,'deleting the default left a stale preference'
        assert alive(original_pid) and b.snapshot()['active_project']==name
        picker(a,external=True);choose(a,'Fixture')
        imported=wait(lambda:(s if (s:=a.snapshot()).get('session_id') not in ('base',created_group) and s.get('panes') and s['panes'][0].get('pid')==source_pid else None),'tmux source did not attach')
        a=restart(a)
        assert a.snapshot()['active_project']==imported['active_project'] and a.snapshot()['panes'][0]['pid']==source_pid,'Cross-session reattachment lost selection or replaced the source'
        a.paste('IMPORTED_INPUT')
        if args.cells:a.input(dict(kind='key',code='enter',modifiers=0))
        else:a.input(dict(kind='embedded',canvas='terminals',event=dict(timestamp_ns=time.monotonic_ns(),sequence=1,kind=dict(kind='key',code='enter',modifiers=0,phase='press'))))
        wait(lambda:'REPLY=IMPORTED_INPUT' in tmux('capture-pane','-p','-t','Fixture'),'tmux input did not reach the original process')
        assert tmux('list-panes','-s','-t','Fixture','-F','#{pane_id} #{pane_pid}')==source_topology
        delete(a,'tmux / Fixture')
        assert alive(source_pid) and tmux('list-panes','-s','-t','Fixture','-F','#{pane_id} #{pane_pid}')==source_topology,'Deleting import killed or changed original tmux'
        if args.herdr:
            picker(a,external=True);choose(a,'default')
            wait(lambda:a.snapshot().get('collection')=='herdr-default' and len(a.snapshot().get('project_order',[]))>=2,'Herdr projects did not import')
            delete(a,'Herdr / default')
            assert all(alive(pid) for pid in herdr_pids),'Deleting import killed original Herdr programs'
            assert {p['pane_id'] for p in herdr_cli('api','snapshot')['result']['snapshot']['panes']}=={p['pane_id'] for p in snapshot['panes']}
        assert a.process.poll() is None and b.process.poll() is None and alive(original_pid)
        a.action('sessions:new');wait(lambda:a.snapshot().get('modal_open') and a.snapshot().get('session_manager') is None,'final session dialog missing')
        a.paste('Keep session');a.accept()
        kept=wait(lambda:(s if (s:=a.snapshot()).get('session_id')!='base' and s.get('panes') and s['panes'][0].get('pid') else None),'final session did not open')
        delete(a,'base')
        wait(lambda:not alive(original_pid) and b.process.poll() is not None,'deleting the launch session did not close its other view')
        a=restart(a)
        assert a.snapshot()['active_project']==kept['active_project'] and a.snapshot()['panes'][0]['pid']==kept['panes'][0]['pid'],'Deleting the old launch session broke reattachment'
        assert alive(source_pid),'Deleting an owned session killed the tmux source'
        print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,list_select_create_delete=True,default_startup=True,explicit_workspace_overrides_default=True,drag_order_persists=True,closed_project_keeps_order=True,cancel_preserves_process=True,independent_views=True,cross_session_reattach=True,delete_launch_session=True,tmux_import_preserves_pids_and_topology=True,herdr_import_preserves_pids=args.herdr)))
    finally:
        for path in state.glob('v-*.state.json'):path.with_name(path.name.removesuffix('.state.json')+'.close').touch()
        for process in processes:
            if process.poll() is None:
                process.terminate()
                try:process.wait(timeout=3)
                except subprocess.TimeoutExpired:process.kill();process.wait()
        # SSH/client shutdown need not reap the detached controller. Wait for
        # private controllers before removing their data directory: notification
        # retention and background screen writes can otherwise race rmtree.
        def private_controllers():
            owners=[]
            for snapshot in state.glob('v-*.state.json'):
                try:
                    record=json.loads(snapshot.read_text());pid=record['controller_pid']
                    proc=Path('/proc',str(pid));argv=(proc/'cmdline').read_bytes().split(b'\0')
                    fields=(proc/'stat').read_text().rsplit(')',1)[1].split()
                    if fields[0]=='Z' or b'--serve-view' not in argv:continue
                    # This guard is deliberately stronger than a reused test
                    # frontend name or PID. Never signal a real user controller.
                    environment=dict(item.split(b'=',1) for item in (proc/'environ').read_bytes().split(b'\0') if b'=' in item)
                    if environment.get(b'XDG_RUNTIME_DIR')!=os.fsencode(runtime) or environment.get(b'XDG_DATA_HOME')!=os.fsencode(tmp/'data'):continue
                    snapshot.with_name(snapshot.name.removesuffix('.state.json')+'.close').touch()
                    owners.append((pid,fields[19]))
                except (OSError,ValueError,KeyError):continue
            return owners
        deadline=time.monotonic()+5
        while private_controllers() and time.monotonic()<deadline:time.sleep(.05)
        for stop_signal in [signal.SIGTERM,signal.SIGKILL]:
            for pid,start in private_controllers():
                try:
                    fields=Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()
                    if fields[0]!='Z' and fields[19]==start:os.kill(pid,stop_signal)
                except (OSError,ValueError):pass
            deadline=time.monotonic()+3
            while private_controllers() and time.monotonic()<deadline:time.sleep(.05)
        assert not private_controllers(),'private controllers did not retire before fixture cleanup'
        collectors=[]
        for status_path in state.glob('snapshot-*.status.json'):
            try:
                record=json.loads(status_path.read_text());pid=record['pid'];start=str(record['start_ticks'])
                proc=Path('/proc',str(pid));argv=(proc/'cmdline').read_bytes().split(b'\0')
                role=argv.index(b'--snapshot-worker')
                fields=(proc/'stat').read_text().rsplit(')',1)[1].split()
                environment=dict(item.split(b'=',1) for item in (proc/'environ').read_bytes().split(b'\0') if b'=' in item)
                if argv[role+1]!=os.fsencode(state) or fields[0]=='Z' or fields[19]!=start:continue
                if environment.get(b'XDG_RUNTIME_DIR')!=os.fsencode(runtime) or environment.get(b'XDG_DATA_HOME')!=os.fsencode(tmp/'data'):continue
                request=status_path.with_name(status_path.name.removesuffix('.status.json')+'.request.json')
                request.write_text(json.dumps(dict(version=max(2,record['version']),stop=True)))
                collectors.append((pid,start))
            except (OSError,ValueError,KeyError,IndexError):continue
        def collector_alive(owner):
            try:
                fields=Path('/proc',str(owner[0]),'stat').read_text().rsplit(')',1)[1].split()
                return fields[0]!='Z' and fields[19]==owner[1]
            except (OSError,ValueError):return False
        deadline=time.monotonic()+5
        while any(map(collector_alive,collectors)) and time.monotonic()<deadline:time.sleep(.05)
        for stop_signal in [signal.SIGTERM,signal.SIGKILL]:
            for owner in collectors:
                if collector_alive(owner):
                    try:os.kill(owner[0],stop_signal)
                    except ProcessLookupError:pass
            deadline=time.monotonic()+3
            while any(map(collector_alive,collectors)) and time.monotonic()<deadline:time.sleep(.05)
        assert not any(map(collector_alive,collectors)),'private collectors did not retire before fixture cleanup'
        subprocess.run(['tmux','-S',str(source_socket),'kill-server'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        for path in (tmp/'data/starflux/projects').glob('*.model.json'):
            workspace=path.name.removesuffix('.model.json')
            subprocess.run(['tmux','-L','starflux-'+workspace,'kill-server'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        if herdr:
            subprocess.run(['herdr','server','stop'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=5)
            if herdr.poll() is None:herdr.terminate();herdr.wait(timeout=3)
        if daemon:daemon.terminate();daemon.wait(timeout=3)
