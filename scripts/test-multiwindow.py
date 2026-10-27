#!/usr/bin/env python3
"""Two private protocol frontends sharing fake terminal processes; no GUI or real agents."""
import argparse
import concurrent.futures
import getpass
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
args=parser.parse_args()
root=Path(__file__).resolve().parents[1]
app=root/'scripts/run-local.sh'
with tempfile.TemporaryDirectory(prefix='starmux-multiwindow-') as tmp:
    tmp=Path(tmp); runtime=tmp/'runtime';runtime.mkdir(mode=0o700)
    state=runtime/'starmux';state.mkdir(mode=0o700)
    workspace='multi-'+secrets.token_hex(4)
    shell=tmp/'shell';shell.write_text("#!/bin/sh\nprintf 'SHARED_TERMINAL_READY\\r\\n'\nexec cat\n");shell.chmod(0o700)
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
        ssh_args=['ssh','-T','-p',str(port),'-i',str(tmp/'client'),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(tmp/'known_hosts'),'127.0.0.1']
        deadline=time.monotonic()+3
        while True:
            try:
                with socket.create_connection(('127.0.0.1',port),timeout=.1):break
            except OSError:
                assert daemon.poll() is None and time.monotonic()<deadline,(tmp/'sshd.log').read_text()
                time.sleep(.02)
    class Client:
        def __init__(self,identity,cols,rows):
            self.identity=identity;self.cols=cols;self.rows=rows;self.messages=queue.Queue();self.types=set();self.fonts=[];self.scene=None;self.allow_refresh=False;self.refresh_requested=False
            self.log=(tmp/(identity+'.log')).open('a')
            command=[str(app),'--graphical-relay',workspace+'@'+identity]
            if ssh_args and identity.startswith('v-b'):
                command=ssh_args+[shlex.join(['env',*[k+'='+env[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','SHELL','STAR_TERMINAL_RENDERER')],*command])]
            self.process=subprocess.Popen(command,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log)
            processes.append(self.process)
            def receive():
                for line in self.process.stdout:
                    try:self.messages.put(json.loads(line))
                    except ValueError:self.messages.put({'type':'bad','line':repr(line)})
            threading.Thread(target=receive,daemon=True).start()
            self.send(dict(type='hello',version=1,client=identity,viewport=dict(columns=cols,rows=rows,width=cols*10,height=rows*20,generation=1),capabilities=dict(native_terminal_cells=True,native_font_millipoints=12500,image_transport='kitty',pixel_geometry='measured',pointer_precision='cells',keyboard=True,paste=True,presentation_ack=True,native_surfaces=True,physical_text=True,pixel_layout=True)))
        def send(self,message):
            self.process.stdin.write(json.dumps(message).encode()+b'\n');self.process.stdin.flush()
        def pump(self):
            while True:
                try:m=self.messages.get_nowait()
                except queue.Empty:return
                self.types.add(m['type'])
                if m['type']=='scene':
                    self.scene=m['scene'];self.send(dict(type='presented',revision=self.scene['revision'],generation=self.scene['viewport']['generation']))
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
        raise AssertionError(label+': '+repr([dict(frontend=c.identity,exit=c.process.poll(),active=c.snapshot().get('active'),tabs=tabs(c),status=c.snapshot().get('status'),stderr=(tmp/(c.identity+'.log')).read_text()) for c in clients])+(' sshd: '+(tmp/'sshd.log').read_text() if daemon else ''))
    def terminal_text(c,pane=0):return next((t['text'] for t in c.snapshot().get('direct_terminals',[]) if t['pane']==pane),'')
    def tabs(c):return c.snapshot().get('project_state',{}).get('terminals',[])
    def pid():return subprocess.check_output(['tmux','-L','starmux-'+workspace,'display-message','-p','-t','p0:0.0','#{pane_pid}'],env=env,text=True).strip()
    try:
        a=Client('v-a'+secrets.token_hex(5),100,35);b=Client('v-b'+secrets.token_hex(5),130,44);clients=[a,b]
        wait(lambda:all('SHARED_TERMINAL_READY' in terminal_text(c) for c in clients),'both shared attachments')
        first_pid=pid()
        assert a.scene['viewport']['columns']==100 and b.scene['viewport']['columns']==130
        a.input(dict(kind='paste',text='SHARED_FROM_FIRST_WINDOW'))
        wait(lambda:all('SHARED_FROM_FIRST_WINDOW' in terminal_text(c) for c in clients),'shared terminal input')
        with concurrent.futures.ThreadPoolExecutor() as pool:list(pool.map(lambda c:c.action('terminal:new'),clients))
        wait(lambda:all(len(tabs(c))==3 for c in clients),'concurrent tab allocation')
        assert set(a.snapshot()['project_state']['terminals'])=={0,1,2}
        assert {a.snapshot()['active'],b.snapshot()['active']}=={1,2},'selected tabs leaked across views'
        a.action('terminal:split')
        wait(lambda:a.snapshot().get('project_state',{}).get('split'),'first split')
        assert not b.snapshot()['project_state']['split'],'split leaked across views'
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
        b.input(dict(kind='pointer',action='down',button=1,x=2,y=6,pixel=[20,130],modifiers=0))
        wait(lambda:b.snapshot().get('context_menu') is not None,'independent context menu')
        assert a.snapshot().get('context_menu') is None
        assert any(c['kind']=='terminal' for c in b.scene['components'])
        b.action('menu:dismiss')
        b.input(dict(kind='resize',viewport=dict(columns=140,rows=45,width=1400,height=900,generation=2)))
        wait(lambda:b.scene['viewport']['columns']==140,'independent viewport resize')
        assert a.scene['viewport']['columns']==100
        a.action('project:'+workspace)
        wait(lambda:a.snapshot().get('active_project')==workspace,'return to original project')
        assert a.snapshot()['project_state']['split'] and not b.snapshot()['project_state']['split']
        assert pid()==first_pid,'second window restarted the terminal'
        # Exit only our private fake cat process. Both views must remove that shared tab.
        subprocess.run(['tmux','-L','starmux-'+workspace,'send-keys','-t','p0:0.0','C-d','C-d'],env=env,check=True)
        wait(lambda:all(0 not in tabs(c) for c in clients),'shared exit removal')
        a.action('terminal:new')
        wait(lambda:all(0 in tabs(c) for c in clients),'safe slot reuse')
        wait(lambda:all('SHARED_TERMINAL_READY' in terminal_text(c,0) for c in clients),'replacement attachments')
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
        print(json.dumps(dict(result='PASS',ssh=args.ssh,simultaneous_frontends=2,shared_input=True,independent_projects=True,independent_menus=True,independent_viewports=True,independent_selection=True,independent_splits=True,independent_fonts=True,concurrent_tab_allocation=True,shared_exit=True,safe_slot_reuse=True,window_scoped_refresh=True,detach_preserves_other_window=True,reattach_preserves_process=True)))
    finally:
        for c in clients:
            try:c.process.stdin.close()
            except OSError:pass
        for p in processes:
            if p.poll() is None:
                try:p.wait(timeout=3)
                except subprocess.TimeoutExpired:p.terminate();p.wait(timeout=3)
        if daemon:
            daemon.terminate();daemon.wait(timeout=3)
        # Controllers are private to this temporary runtime; leave actual workspaces alone.
        for descriptor in state.glob('*.frontend.json'):
            identity=json.loads(descriptor.read_text())['id'];(state/(identity+'.close')).touch()
        for name in (workspace,other):
            subprocess.run(['tmux','-L','starmux-'+name,'kill-server'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
