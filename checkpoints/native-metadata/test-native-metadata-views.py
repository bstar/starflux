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
root=Path(__file__).resolve().parents[1];app=root/'target/run-native-tree-candidate'
with tempfile.TemporaryDirectory(prefix='starflux-session-proof-') as directory:
    tmp=Path(directory);runtime=tmp/'runtime';runtime.mkdir(mode=0o700);state=runtime/'starflux';state.mkdir(mode=0o700)
    name='sessions-'+secrets.token_hex(4)
    shell=tmp/'fake-shell'
    shell.write_text("#!/usr/bin/python3\nimport os,sys\nprint('SESSION_PROOF_PID='+str(os.getpid()),flush=True)\nfor line in sys.stdin:print('REPLY='+line.strip(),flush=True)\n")
    shell.write_text("#!/usr/bin/python3\nimport os,sys,base64\nprint('SESSION_PROOF_PID='+str(os.getpid()),flush=True)\nfor line in sys.stdin:\n command=line.strip()\n if command=='TITLE_SET':sys.stdout.write('\\x1b]0;Native shell 界\\x1b\\\\')\n if command=='TITLE_SECOND':sys.stdout.write('\\x1b]2;Latest title\\x1b\\\\')\n if command=='TITLE_CLEAR':sys.stdout.write('\\x1b]2;\\x1b\\\\')\n if command=='TITLE_INVALID':sys.stdout.buffer.write(b'\\x1b]2;bad\\xff\\x1b\\\\');sys.stdout.buffer.flush()\n if command=='BELLS':sys.stdout.write('\\x07'*3+'\\x1bg'*2)\n if command.startswith('COPY_'):\n  text={'COPY_ONE':'native clip café\\nsecond line','COPY_TWO':'next clipboard','COPY_CLEAR':'','COPY_LARGE':'界'*20000+' end'}.get(command)\n  payload=None\n  if text is not None:payload=base64.b64encode(text.encode()).decode()\n  if command=='COPY_OVERSIZE':payload=base64.b64encode(b'x'*65537).decode()\n  if command=='COPY_BINARY':payload='/w=='\n  if command=='COPY_QUERY':payload='?'\n  if payload is not None:sys.stdout.write('\\x1b]52;c;'+payload+'\\x1b\\\\')\n sys.stdout.flush()\n print('REPLY='+command,flush=True)\n")
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
    (state/(name+'.json')).write_text(json.dumps(dict(workspace=name,label='Original',collection='base',host=None,directory=str(project_directory),agent=None,executable=str(app),project_host=True,native_terminals=True,desktop_helper=str(app))))
    for extra in ['extra-a','extra-b']:
        (state/(extra+'.json')).write_text(json.dumps(dict(workspace=extra,label=extra,collection='base',host=None,directory=str(tmp),agent=None,executable=str(app),project_host=True)))
    env['STARFLUX_SNAPSHOT_SERVICE']='off'
    env.pop('KITTY_WINDOW_ID',None)
    env.pop('STARFLUX_FONT_OUTPUT',None)
    processes=[];clients=[];daemon=None;herdr=None;ssh_args=[]
    socket_root=tmp/'tmux'/('tmux-'+str(os.getuid()));socket_root.mkdir(parents=True,mode=0o700);source_socket=socket_root/'fixture'
    def tmux(*arguments):
        return subprocess.check_output(['tmux','-S',str(source_socket),*arguments],env=env,text=True).strip()
    def alive(pid):
        try:return Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()[0]!='Z'
        except OSError:return False
    class Client:
        def __init__(self,identity,remote=False,executable=None):
            self.executable=executable;self.identity=identity;self.scene=None;self.messages=queue.Queue();self.serial=0;self.frames={};self.images=[];self.clipboards=[];self.all_frames=[]
            command=[str(executable or app),'--graphical-relay',name+'@'+identity]
            if remote:command=ssh_args+[shlex.join(['env',*[k+'='+v for k,v in env.items() if k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','STAR_TERMINAL_RENDERER','STARFLUX_HERDR','HERDR_CONFIG_PATH','PATH')],*command])]
            self.log=(tmp/(identity+'.log')).open('a')
            self.process=subprocess.Popen(command,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log);processes.append(self.process);clients.append(self)
            def read():
                for line in self.process.stdout:
                    try:self.messages.put(json.loads(line))
                    except ValueError:self.messages.put(dict(type='invalid',line=repr(line)))
            threading.Thread(target=read,daemon=True).start()
            self.send(dict(type='hello',version=1,client=identity,viewport=dict(columns=100,rows=35,width=1000,height=700,generation=1),capabilities=dict(image_transport="kitty",pixel_geometry="measured",pointer_precision="cells",native_terminal_cells=True,cell_presentation=args.cells,native_surfaces=True,streamed_frames=not args.cells,pixel_layout=not args.cells,physical_text=True,keyboard=True,paste=True,presentation_ack=True)))
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
                if message['type']=='osc72' and message.get('meta')=='starflux-image-planes-v1':
                    self.images.append(json.loads(message['payload']));self.send(dict(type='effect_ack',id=message.get('id',0)))
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
        logs+=repr([(c.identity,c.process.poll(),{k:c.snapshot().get(k) for k in ('active_project','session_id','status','session_manager','terminal_review','direct_terminals')}) for c in clients])
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
            forced=tmp/'private-native-environment';forced.write_text('#!/bin/bash\n'+''.join('export '+key+'='+shlex.quote(env[key])+'\n' for key in ['XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','STARFLUX_SNAPSHOT_SERVICE','STARFLUX_HERDR','STAR_TERMINAL_RENDERER'])+'exec /bin/sh -c \"$SSH_ORIGINAL_COMMAND\"\n');forced.chmod(0o700)
            (tmp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAcceptEnv XDG_RUNTIME_DIR XDG_DATA_HOME XDG_CONFIG_HOME TMUX_TMPDIR SHELL GIT_CONFIG_GLOBAL GIT_CONFIG_SYSTEM\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nForceCommand {forced}\nLogLevel ERROR\n')
            daemon=subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd','-D','-e','-f',str(tmp/'sshd_config')],stderr=(tmp/'sshd.log').open('w'))
            deadline=time.monotonic()+5
            while True:
                try:
                    with socket.create_connection(('127.0.0.1',port),timeout=.1):break
                except OSError:
                    assert daemon.poll() is None and time.monotonic()<deadline;time.sleep(.02)
            ssh_args=['ssh','-F','/dev/null','-T','-p',str(port),'-i',str(tmp/'client'),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(tmp/'known_hosts'),'127.0.0.1']
            if args.ssh or args.worktrees or args.machines:
                host_config=tmp/'worktree-ssh-config'
                host_config.write_text(f'Host proof\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {tmp}/client\n IdentitiesOnly yes\n BatchMode yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {tmp}/known_hosts\n SetEnv '+ ' '.join(k+'='+env[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','GIT_CONFIG_GLOBAL','GIT_CONFIG_SYSTEM') if k in env)+'\n')
                if args.ssh or args.worktrees:
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
        if not args.cells:
            for client in (a,b):client.input(dict(kind='notice',message='starflux-font-v1:'+json.dumps(dict(family='monospace',millipoints=13000,image_planes_v1=True))))
        def terminal_text(client):return '\n'.join(t['text'] for t in client.snapshot().get('direct_terminals',[]))
        wait(lambda:'SESSION_PROOF_PID=' in terminal_text(a) and 'SESSION_PROOF_PID=' in terminal_text(b),'native views did not receive initial output')
        a.paste('NATIVE_HUB café');a.key('enter')
        wait(lambda:'REPLY=NATIVE_HUB café' in terminal_text(a) and 'REPLY=NATIVE_HUB café' in terminal_text(b),'native shared input or output missing')
        assert a.snapshot()['panes'][0]['pid']==b.snapshot()['panes'][0]['pid']==original_pid
        first='native clip café\nsecond line';second='next clipboard';large='界'*20000+' end'
        def metadata(client,pane=0):
            return next((t.get('terminal_metadata',{}) for t in client.snapshot().get('direct_terminals',[]) if t['pane']==pane),{})
        def submit(client,command):
            client.paste(command);client.key('enter')
        def seen(command):return all('REPLY='+command in terminal_text(c) for c in (a,b))
        def counts():return [len(c.clipboards) for c in clients]
        def no_copy(label,expected):
            began=time.monotonic()
            wait(lambda:time.monotonic()-began>.75,label+' observation')
            assert counts()==expected,(label,counts(),expected)
        def title(command,expected):
            submit(a,command)
            wait(lambda:all(metadata(c).get('title')==expected for c in clients),'source title did not reach every view')
        title('TITLE_SET','Native shell 界')
        wait(lambda:all('Native shell 界' in json.dumps(c.scene,ensure_ascii=False) for c in clients),'retained title was not used for default tab label')
        assert all(metadata(c).get('icon_name')=='Native shell 界' for c in clients)
        title('TITLE_CLEAR','')
        wait(lambda:all('Terminal 1' in json.dumps(c.scene) for c in clients),'cleared title did not restore generated label')
        title('TITLE_SET','Native shell 界')
        submit(a,'COPY_ONE')
        wait(lambda:counts()==[1,1],'new OSC52 write did not reach both attached frontends')
        assert all(c.clipboards==[first] for c in clients)
        # A newly attached frontend receives retained title/state but no old copy.
        c=Client('v-session-c')
        wait(lambda:c.scene and metadata(c).get('clipboard_sequence')==1,'late frontend did not receive retained metadata')
        no_copy('late attachment replayed clipboard',[1,1,0])
        submit(a,'BELLS')
        wait(lambda:all(metadata(c).get('audible')==3 and metadata(c).get('visual')==2 for c in clients),'bell diagnostics were not retained')
        # A hidden pane must not overwrite its frontend clipboard on reveal.
        b.action('terminal:new')
        wait(lambda:b.snapshot()['project_state']['terminal']!=0,'second frontend did not select new pane')
        submit(a,'COPY_TWO')
        wait(lambda:counts()==[2,1,1],'hidden source copied to an inactive pane')
        b.action('terminal:0')
        wait(lambda:b.snapshot()['project_state']['terminal']==0 and metadata(b).get('clipboard_sequence')==2,'revealed view did not catch up')
        no_copy('reveal replayed old clipboard',[2,1,1])
        # Metadata-only changes survive viewport epochs without repeating writes.
        a.resize(120,42,1200,840)
        wait(lambda:metadata(a).get('clipboard_sequence')==2,'resized metadata did not arrive')
        no_copy('resize replayed clipboard',[2,1,1])
        submit(a,'COPY_LARGE')
        wait(lambda:counts()==[3,2,2],'bounded large clipboard was not forwarded')
        assert all(client.clipboards[-1]==large for client in clients)
        before=[metadata(c).get('rejected',0) for c in clients]
        for command in ('COPY_OVERSIZE','COPY_BINARY','COPY_QUERY','TITLE_INVALID'):submit(a,command)
        wait(lambda:all(metadata(c).get('rejected',0)>=before[i]+3 for i,c in enumerate(clients)),'invalid metadata was not rejected')
        assert all(metadata(c).get('title')=='Native shell 界' for c in clients)
        no_copy('invalid write/query changed clipboard',[3,2,2])
        a.action('rename-tab:0');wait(lambda:a.snapshot().get('modal_open'),'rename dialog missing')
        a.key('char:u',2);a.paste('Manual tab');a.accept()
        wait(lambda:not a.snapshot().get('modal_open') and a.snapshot()['project_state']['terminal_labels'].get('0')=='Manual tab','explicit tab name was not saved')
        title('TITLE_SECOND','Latest title')
        wait(lambda:'Manual tab' in json.dumps(a.scene),'source title replaced explicit tab name')
        # Connector replacement retains source identity and cannot replay a copy.
        controller=a.snapshot()['controller_pid']
        def connectors():
            found=set()
            for task in Path('/proc',str(controller),'task').iterdir():
                try:children=(task/'children').read_text().split()
                except OSError:continue
                for child in children:
                    try:
                        argv=Path('/proc',child,'cmdline').read_bytes().split(b'\0')
                        if any(b'--native-source-attach' in arg for arg in argv):found.add(int(child))
                    except OSError:continue
            return list(found)
        wait(lambda:len(connectors())==1,'connector was not shared')
        old_connector=connectors()[0];fd=os.pidfd_open(old_connector);signal.pidfd_send_signal(fd,signal.SIGKILL);os.close(fd)
        wait(lambda:(p:=connectors()) and old_connector not in p,'connector did not reconnect',timeout=25)
        wait(lambda:metadata(a).get('title')=='Latest title','reconnect lost retained title')
        no_copy('connector replayed clipboard',[3,2,2])
        submit(a,'COPY_CLEAR')
        wait(lambda:counts()==[4,3,3],'empty clipboard write was not forwarded')
        assert all(client.clipboards[-1]=='' for client in clients)
        assert all(any(p.get('pid')==original_pid for p in client.snapshot()['panes']) for client in clients),[client.snapshot()['panes'] for client in clients]
        a.action('recover-terminal:0');wait(lambda:a.snapshot().get('modal_open'),'recovery confirmation missing')
        a.paste('RECOVER');a.accept()
        wait(lambda:not a.snapshot().get('modal_open'),'recovery confirmation did not close',timeout=25)
        wait(lambda:all(metadata(client).get('clipboard_sequence')==0 and metadata(client).get('title')=='' for client in clients),'recovery retained old metadata',timeout=25)
        no_copy('source recovery replayed clipboard',[4,3,3])
        submit(a,'COPY_ONE')
        wait(lambda:counts()==[5,4,4],'fresh source clipboard sequence was rejected')
        assert all(client.clipboards[-1]==first for client in clients)
        print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,retained_title_and_icon=True,user_tab_name_preserved=True,bell_diagnostics=True,clipboard_write=True,unicode_and_60k_payload=True,empty_write=True,invalid_metadata_rejected=True,no_query_read=True,no_late_attach_replay=True,no_hidden_reveal_replay=True,no_resize_replay=True,no_reconnect_replay=True,recovery_resets_metadata=True,new_source_copy=True)))
        raise SystemExit(0)

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
        owner_path=tmp/'data/starflux/native-sources/owner.json'
        if owner_path.exists():
            try:
                record=json.loads(owner_path.read_text())['owner'];pid=record['pid']
                fields=Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()
                if fields[0]!='Z' and int(fields[19])==record['start_ticks'] and record['boot']==Path('/proc/sys/kernel/random/boot_id').read_text().strip():
                    token=json.loads(owner_path.with_name('token.json').read_text())
                    with socket.socket(socket.AF_UNIX) as peer:
                        peer.settimeout(3);peer.connect(str(owner_path.with_name('broker.sock')));peer.sendall((json.dumps(dict(version=1,id=1,token=token,action=dict(op='shutdown',instance=record['instance'])))+'\n').encode());peer.makefile('rb').readline()
                    deadline=time.monotonic()+4
                    while alive(pid) and time.monotonic()<deadline:time.sleep(.05)
                    if alive(pid) and int(Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()[19])==record['start_ticks']:
                        fd=os.pidfd_open(pid);signal.pidfd_send_signal(fd,signal.SIGKILL);os.close(fd)
            except (OSError,ValueError,KeyError):pass
        if daemon:daemon.terminate();daemon.wait(timeout=3)
