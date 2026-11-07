#!/usr/bin/env python3
"""Terminate one private transferred PTY through the UI; retain its sibling owner."""
import fcntl
import base64
import importlib.util
import json
import os
from pathlib import Path
import pty
import queue
import secrets
import shlex
import subprocess
import tempfile
import termios
import threading
import time
import uuid

root=Path(__file__).resolve().parents[1];app=root/'scripts/run-local.sh'
spec=importlib.util.spec_from_file_location('adopt',root/'scripts/adopt-pty.py')
adopt=importlib.util.module_from_spec(spec);spec.loader.exec_module(adopt)
# Leave room for AF_UNIX names when Nix adds its own temporary-directory prefix.
with tempfile.TemporaryDirectory(prefix='starflux-xfer-') as directory:
    tmp=Path(directory);runtime=tmp/'runtime';runtime.mkdir(mode=0o700);state=runtime/'starflux';state.mkdir(mode=0o700)
    (tmp/'tmux').mkdir(mode=0o700)
    name='terminate-'+secrets.token_hex(4);server='starflux-adoptproof-'+secrets.token_hex(4);frontend='v-transfer-proof'
    env={k:v for k,v in os.environ.items() if not k.startswith(('BASH_FUNC_','HERDR_')) and k not in ('TMUX','SSH_CONNECTION','SSH_TTY','PROMPT_COMMAND','BASH_ENV','ENV')}
    env.update(XDG_RUNTIME_DIR=str(runtime),XDG_DATA_HOME=str(tmp/'data'),XDG_CONFIG_HOME=str(tmp/'config'),TMUX_TMPDIR=str(tmp/'tmux'),SHELL='/bin/bash',HISTFILE='/dev/null',STAR_TERMINAL_RENDERER='native')
    originals=[];masters=[];process=None;broker=None;done=threading.Event();errors=[]
    def child_terminal():os.setsid();fcntl.ioctl(0,termios.TIOCSCTTY,0)
    try:
        for _ in range(2):
            master,slave=pty.openpty()
            original=subprocess.Popen(['/bin/bash','--noprofile','--norc','-i'],stdin=slave,stdout=slave,stderr=slave,env=env,preexec_fn=child_terminal)
            os.close(slave);originals.append(original);masters.append(master)
        keys=['proof-0','proof-1'];epochs=[secrets.token_hex(16),secrets.token_hex(16)]
        plan=dict(run=str(tmp),broker=str(tmp/'owner.sock'),panes=[dict(pid=p.pid,key=k,screen='') for p,k in zip(originals,keys)])
        broker=adopt.Broker(plan,dict(panes=[dict(child_pid=p.pid) for p in originals]),masters);broker.own()
        def service():
            try:
                while not done.is_set():broker.step(.01)
            except BaseException as error:errors.append(repr(error))
        thread=threading.Thread(target=service,daemon=True);thread.start()
        transferred={str(i):dict(broker=plan['broker'],key=keys[i],server=server,pid=p.pid,epoch=epochs[i],proxy=str(root/'target/release/starflux-pty-proxy')) for i,p in enumerate(originals)}
        config=dict(workspace=name,label='Transferred proof',collection='proof',host=None,directory=str(tmp),agent=None,executable=str(app),project_host=True,adopted=transferred)
        (state/(name+'.json')).write_text(json.dumps(config))
        projects=tmp/'data/starflux/projects';projects.mkdir(parents=True,mode=0o700)
        (projects/(name+'.json')).write_text(json.dumps(config))
        (projects/(name+'.model.json')).write_text(json.dumps(dict(terminal_first=True,terminals=[0,1],terminal=0,terminal_epochs={str(i):e for i,e in enumerate(epochs)})))
        log=(tmp/'frontend.log').open('w')
        process=subprocess.Popen([str(app),'--graphical-relay',name+'@'+frontend],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log)
        messages=queue.Queue();scene=None;serial=0
        def read():
            for line in process.stdout:messages.put(json.loads(line))
        threading.Thread(target=read,daemon=True).start()
        def send(message):process.stdin.write(json.dumps(message).encode()+b'\n');process.stdin.flush()
        send(dict(type='hello',version=1,client=frontend,viewport=dict(columns=100,rows=35,width=1000,height=700,generation=1),capabilities=dict(image_transport='kitty',pixel_geometry='measured',pointer_precision='cells',native_terminal_cells=True,native_surfaces=True,pixel_layout=True,physical_text=True,keyboard=True,paste=True,presentation_ack=True)))
        def snapshot():
            try:return json.loads((state/(frontend+'.state.json')).read_text())
            except (OSError,ValueError):return {}
        def wait(predicate,label):
            global scene
            deadline=time.monotonic()+20
            while time.monotonic()<deadline:
                while not messages.empty():
                    message=messages.get_nowait();assert message['type']!='error',message
                    if message['type']=='scene':
                        scene=message['scene'];send(dict(type='presented',revision=scene['revision'],generation=1))
                assert process.poll() is None and not errors,(label,errors,(tmp/'frontend.log').read_text()[-1500:])
                result=predicate()
                if result:return result
                time.sleep(.02)
            current=snapshot()
            details={k:current.get(k) for k in ('active','status','history','sessions','project_state')}
            raise AssertionError(label+' '+json.dumps(details)+' '+(tmp/'frontend.log').read_text()[-1500:])
        def action(value):subprocess.run([str(app),'--workspace',name,'--frontend',frontend,'--action',value],env=env,capture_output=True,check=True)
        def input(value):
            global serial
            serial+=1;send(dict(type='input',id=serial,revision=scene['revision'],generation=1,input=value))
        wait(lambda:scene and any(s.get('health',{}).get('pid')==originals[0].pid for s in snapshot().get('sessions',[]) if s.get('health')) and any(t['pane']==0 for t in snapshot().get('direct_terminals',[])),'transferred owner did not attach')
        def api(method,params,operation=None):
            request=dict(version=1,id='transferred-proof',method=method,params=params,operation_id=operation)
            result=json.loads(subprocess.check_output([str(app),'--api',json.dumps(request)],env=env));assert result['ok'],result
            return result['result']
        target=api('panes.get',dict(project=name,pane=1))['target']
        hidden=api('panes.get',dict(project=name,pane=2))['target']
        api('panes.send',dict(target=hidden,text="printf 'TRANSFER_BACKGROUND_界\\n'",submit=True),str(uuid.uuid4()))
        saved=tmp/'data/starflux/screens'/name/'p1.screen.json'
        def captured():
            try:
                frame=json.loads(saved.read_text())
                return frame if 'TRANSFER_BACKGROUND_界' in base64.b64decode(frame['formatted_base64']).decode() else None
            except (OSError,ValueError):return None
        frame=wait(captured,'unattached transferred terminal did not save')
        assert frame['pid']==originals[1].pid and not any(t['pane']==1 for t in snapshot().get('direct_terminals',[]))
        marker=tmp/'api-transfer-proof'
        paste_operation=str(uuid.uuid4());payload=dict(target=target,text='printf X >> '+shlex.quote(str(marker)),submit=False)
        api('panes.send',payload,paste_operation);api('panes.send',payload,paste_operation)
        assert not marker.exists(),'transferred API paste submitted without Enter'
        submit_operation=str(uuid.uuid4());payload=dict(target=target,text='',submit=True)
        api('panes.send',payload,submit_operation);api('panes.send',payload,submit_operation)
        wait(lambda:marker.exists(),'transferred API Enter did not reach original owner')
        assert marker.read_text()=='X' and all(p.poll() is None for p in originals),'transferred API duplicated input or replaced an owner'
        action('terminate-terminal:0');wait(lambda:snapshot().get('modal_open'),'termination dialog missing')
        input(dict(kind='key',code='escape',modifiers=0));wait(lambda:not snapshot().get('modal_open'),'termination did not cancel')
        assert all(p.poll() is None for p in originals),'cancel killed a transferred owner'
        action('terminate-terminal:0');wait(lambda:snapshot().get('modal_open'),'second termination dialog missing')
        input(dict(kind='paste',text='TERMINATE'));input(dict(kind='key',code='enter',modifiers=2))
        wait(lambda:0 not in snapshot().get('project_state',{}).get('terminals',[0]) and originals[0].poll() is not None,'transferred terminal did not terminate')
        assert originals[1].poll() is None,'terminating one owner killed its sibling'
        action('terminal:1')
        wait(lambda:any(t['pane']==1 for t in snapshot().get('direct_terminals',[])),'sibling terminal did not attach')
        marker=tmp/'sibling-proof'
        input(dict(kind='paste',text='touch '+shlex.quote(str(marker))))
        input(dict(kind='embedded',canvas='terminals',event=dict(timestamp_ns=time.monotonic_ns(),sequence=serial+1,kind=dict(kind='key',code='enter',phase='press',modifiers=0))))
        wait(lambda:marker.exists(),'sibling owner stopped executing input')
        print(json.dumps(dict(result='PASS',transferred_owner_terminated=True,cancel_preserves_owners=True,sibling_owner_preserved=True,sibling_input=True,guarded_api_input=True,idempotent_api_submit=True)))
    finally:
        (state/(frontend+'.close')).touch()
        if process and process.poll() is None:
            try:process.wait(timeout=5)
            except subprocess.TimeoutExpired:process.terminate();process.wait(timeout=5)
        subprocess.run(['tmux','-L',server,'kill-server'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        done.set()
        if broker:thread.join(timeout=2);broker.listener.close();broker.sel.close()
        for p in originals:
            if p.poll() is None:p.kill();p.wait(timeout=3)
        for fd in masters:
            try:os.close(fd)
            except OSError:pass
