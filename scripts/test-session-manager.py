#!/usr/bin/env python3
"""Private sessions, independent views, live tmux/Herdr imports and scoped deletion."""
import argparse
import fcntl
import getpass
import json
import os
from pathlib import Path
import queue
import pty
import secrets
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
args=parser.parse_args()
root=Path(__file__).resolve().parents[1];app=root/'scripts/run-local.sh'
with tempfile.TemporaryDirectory(prefix='starflux-session-proof-') as directory:
    tmp=Path(directory);runtime=tmp/'runtime';runtime.mkdir(mode=0o700);state=runtime/'starflux';state.mkdir(mode=0o700)
    name='sessions-'+secrets.token_hex(4)
    shell=tmp/'fake-shell'
    shell.write_text("#!/usr/bin/python3\nimport os,sys\nprint('SESSION_PROOF_PID='+str(os.getpid()),flush=True)\nfor line in sys.stdin:print('REPLY='+line.strip(),flush=True)\n")
    shell.chmod(0o700)
    env={k:v for k,v in os.environ.items() if not k.startswith('HERDR_') and k not in ('TMUX','SSH_CONNECTION','SSH_TTY')}
    env.update(XDG_RUNTIME_DIR=str(runtime),XDG_DATA_HOME=str(tmp/'data'),XDG_CONFIG_HOME=str(tmp/'config'),TMUX_TMPDIR=str(tmp/'tmux'),SHELL=str(shell),STAR_TERMINAL_RENDERER='native',STARFLUX_HERDR='herdr' if args.herdr else str(tmp/'herdr-not-installed'))
    (state/(name+'.json')).write_text(json.dumps(dict(workspace=name,label='Original',collection='base',host=None,directory=str(tmp),agent=None,executable=str(app),project_host=True)))
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
        def __init__(self,identity,remote=False):
            self.identity=identity;self.scene=None;self.messages=queue.Queue();self.serial=0
            command=[str(app),'--graphical-relay',name+'@'+identity]
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
                    self.scene=message['scene'];self.send(dict(type='presented',revision=self.scene['revision'],generation=1))
        def snapshot(self):
            try:return json.loads((state/(self.identity+'.state.json')).read_text())
            except (OSError,ValueError):return {}
        def input(self,value):
            self.serial+=1;self.send(dict(type='input',id=self.serial,revision=self.scene['revision'],generation=1,input=value))
        def action(self,value):
            subprocess.run([str(app),'--workspace',name,'--frontend',self.identity,'--action',value],env=env,check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        def paste(self,text):self.input(dict(kind='paste',text=text))
        def accept(self):self.input(dict(kind='key',code='enter',modifiers=2))
    def wait(predicate,label,timeout=15):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            for client in clients:
                if client.process.poll() is None:client.pump()
            value=predicate()
            if value:return value
            time.sleep(.02)
        logs='\n'.join(p.read_text()[-1200:] for p in [*tmp.glob('v-*.log'),*state.glob('v-*.log')])
        logs+=repr([(c.identity,c.process.poll(),{k:c.snapshot().get(k) for k in ('active_project','session_id','status','session_manager')}) for c in clients])
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
        resumed=Client(client.identity,remote=args.ssh)
        wait(lambda:resumed.scene and resumed.snapshot().get('panes') and resumed.snapshot()['panes'][0].get('pid'),'view did not resume')
        return resumed
    def check_startup(expected,expected_pid,explicit=False):
        identity='v-startup-explicit' if explicit else 'v-startup-default'
        command=[str(app),'--cells','--frontend',identity]
        if explicit:command+=['--workspace',name]
        if args.ssh:
            wrapper=tmp/'remote-starflux'
            wrapper.write_text('#!/bin/sh\nexec '+shlex.join(['env',*[k+'='+v for k,v in env.items() if k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','STAR_TERMINAL_RENDERER','STARFLUX_HERDR','HERDR_CONFIG_PATH','PATH')],str(app)])+' "$@"\n');wrapper.chmod(0o700)
            config=tmp/'startup-ssh-config'
            config.write_text(f'Host proof\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {tmp}/client\n IdentitiesOnly yes\n BatchMode yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {tmp}/known_hosts\n')
            command+=['--ssh','proof','--ssh-config',str(config),'--remote-executable',str(wrapper)]
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
        if args.ssh:
            for key in ('host','client'):subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(tmp/key)],check=True)
            (tmp/'authorized_keys').write_text((tmp/'client.pub').read_text())
            with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
            (tmp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(tmp/'host.pub').read_text())
            (tmp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
            daemon=subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd','-D','-e','-f',str(tmp/'sshd_config')],stderr=(tmp/'sshd.log').open('w'))
            deadline=time.monotonic()+5
            while True:
                try:
                    with socket.create_connection(('127.0.0.1',port),timeout=.1):break
                except OSError:
                    assert daemon.poll() is None and time.monotonic()<deadline;time.sleep(.02)
            ssh_args=['ssh','-F','/dev/null','-T','-p',str(port),'-i',str(tmp/'client'),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(tmp/'known_hosts'),'127.0.0.1']
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
        a=Client('v-session-a',remote=args.ssh);b=Client('v-session-b')
        wait(lambda:a.scene and b.scene and a.snapshot().get('panes') and a.snapshot()['panes'][0].get('pid'),'initial views missing')
        original_pid=a.snapshot()['panes'][0]['pid']
        before=a.snapshot()['project_order'];moved=before[-1];expected=[moved,*before[:-1]]
        def pointer(action,index):
            value=dict(kind='pointer',action=action,button=0,modifiers=0,x=5,y=5+index)
            if not args.cells:value['pixel']=[40,122+index*34+(17 if action=='down' else 1)]
            a.input(value)
        pointer('down',len(before)-1);pointer('move',0);pointer('up',0)
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
        subprocess.run(['tmux','-S',str(source_socket),'kill-server'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        for path in (tmp/'data/starflux/projects').glob('*.model.json'):
            workspace=path.name.removesuffix('.model.json')
            subprocess.run(['tmux','-L','starflux-'+workspace,'kill-server'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        if herdr:
            subprocess.run(['herdr','server','stop'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=5)
            if herdr.poll() is None:herdr.terminate();herdr.wait(timeout=3)
        if daemon:daemon.terminate();daemon.wait(timeout=3)
