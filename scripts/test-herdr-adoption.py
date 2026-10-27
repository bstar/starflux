#!/usr/bin/env python3
"""Headless live transfer + rollback with a private Herdr server and fake shells.

No display, real workspace, agent prompt, or existing Herdr process is touched.
"""
import copy
import json
import os
from pathlib import Path
import secrets
import shlex
import socket
import subprocess
import tempfile
import threading
import time

root=Path(__file__).resolve().parents[1];app=root/'scripts/run-local.sh'
with tempfile.TemporaryDirectory(prefix='starmux-live-adopt-') as tmp:
    tmp=Path(tmp);config=tmp/'config/herdr';config.mkdir(parents=True)
    runtime=tmp/'runtime';runtime.mkdir(mode=0o700)
    config.joinpath('config.toml').write_text('onboarding = false\n')
    shell=tmp/'shell';shell.write_text("#!/usr/bin/python3\nimport os,sys,signal,fcntl,termios,struct\nprint('LIVE_ADOPTION_PID='+str(os.getpid()),flush=True)\ndef resize(*args):\n rows,cols=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,b'\\0'*8))[:2];print('SIZE='+str(rows)+'x'+str(cols),flush=True)\nsignal.signal(signal.SIGWINCH,resize)\nfor line in sys.stdin:print('REPLY='+line.strip(),flush=True)\n");shell.chmod(0o700)
    env={k:v for k,v in os.environ.items() if not k.startswith('HERDR_')}
    env.update(XDG_CONFIG_HOME=str(tmp/'config'),HERDR_CONFIG_PATH=str(config/'config.toml'),XDG_RUNTIME_DIR=str(runtime),XDG_DATA_HOME=str(tmp/'data'),SHELL=str(shell),STAR_TERMINAL_RENDERER='native')
    log=(tmp/'source.log').open('w')
    source=subprocess.Popen(['herdr','server'],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
    broker_pid=None;plan=None;view_processes=[];peer=None
    def cli(*args,raw=False):
        result=subprocess.run(['herdr','--session','default',*args],env=env,capture_output=True,timeout=10)
        assert result.returncode==0,(args,result.stderr.decode(errors='replace'),(tmp/'source.log').read_text()[-800:])
        return result.stdout if raw else json.loads(result.stdout)
    def wait(predicate,label,timeout=10):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            value=predicate()
            if value:return value
            time.sleep(.03)
        raise AssertionError(label+' '+(tmp/'source.log').read_text()[-800:])
    def is_alive(pid):
        try:return Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()[0]!='Z'
        except OSError:return False
    try:
        wait(lambda:(config/'herdr.sock').exists(),'private Herdr socket')
        one=cli('workspace','create','--cwd',str(tmp),'--label','Proof One','--no-focus')['result']
        two=cli('workspace','create','--cwd',str(tmp),'--label','Proof Two','--no-focus')['result']
        snapshot=cli('api','snapshot')['result']['snapshot']
        pids=[]
        for pane in snapshot['panes']:
            info=cli('pane','process-info','--pane',pane['pane_id'])['result']['process_info'];pids.append(info['shell_pid'])
            wait(lambda p=pane:'LIVE_ADOPTION_PID=' in cli('pane','read',p['pane_id'],'--source','visible','--raw',raw=True).decode(errors='replace'),'fake shell output')
        wait(lambda:(config/'session.json').exists(),'saved private layout')
        imported=subprocess.run([str(app),'--import-herdr'],env=env,capture_output=True)
        assert imported.returncode==0,imported.stderr.decode(errors='replace')
        prepared=subprocess.run([str(app),'--plan-herdr-migration'],env=env,check=True,capture_output=True,text=True)
        plan_path=Path(json.loads(prepared.stdout.strip().splitlines()[-1])['plan']);plan=json.loads(plan_path.read_text())
        assert not plan['source_clients'],'Private migration selected a real UI client'
        assert len(plan['panes'])==len(pids)
        # Fail after receiving descriptors: the source must resume and retain its PIDs.
        bad_run=tmp/'rejected';bad_run.mkdir()
        bad=copy.deepcopy(plan);bad['run']=str(bad_run);bad['broker']=str(runtime/'rejected.sock')
        for p in bad['panes']:p['server']='invalid/server'
        bad_path=bad_run/'plan.json';bad_path.write_text(json.dumps(bad))
        wrapper=bad_run/'accept';wrapper.write_text('#!/bin/sh\nshift 2\nexec '+shlex.quote(str(app))+' --native-pty-import '+shlex.quote(str(bad_path))+' "$@"\n');wrapper.chmod(0o700)
        failed=subprocess.run(['herdr','--session','default','server','live-handoff','--import-exe',str(wrapper)],env=env,capture_output=True,timeout=35)
        assert failed.returncode!=0,'Rejected importer unexpectedly committed'
        assert source.poll() is None and all(is_alive(pid) for pid in pids),'Rollback lost original processes'
        assert len(cli('api','snapshot')['result']['snapshot']['panes'])==len(pids)
        # A protocol-only legacy frontend must refresh into an independent native view.
        first=plan['projects'][0];second=plan['projects'][1]
        legacy=subprocess.Popen([str(app),'--serve',first],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE);view_processes.append(legacy)
        endpoint=runtime/'starmux'/(first+'.sock');wait(endpoint.exists,'legacy STAR/MUX controller')
        peer=socket.socket(socket.AF_UNIX);peer.connect(str(endpoint));transport=peer.makefile('rwb',buffering=0)
        caps=dict(native_terminal_cells=True,image_transport='kitty',pixel_geometry='measured',pointer_precision='cells',keyboard=True,paste=True,presentation_ack=True,native_surfaces=True,physical_text=True,pixel_layout=True)
        hello=dict(type='hello',version=1,client='adoption-proof',viewport=dict(columns=100,rows=35,width=1000,height=700,generation=1),capabilities=caps)
        transport.write(json.dumps(hello).encode()+b'\n')
        def frames():
            try:
                for line in transport:
                    message=json.loads(line)
                    if message['type']=='scene':
                        scene=message['scene'];transport.write(json.dumps(dict(type='presented',revision=scene['revision'],generation=scene['viewport']['generation'])).encode()+b'\n')
            except (OSError,ValueError):pass
        threading.Thread(target=frames,daemon=True).start()
        state=runtime/'starmux'/(first+'.state.json');wait(state.exists,'legacy state')
        subprocess.run([str(app),'--workspace',first,'--action','project:'+second],env=env,check=True,capture_output=True)
        wait(lambda:json.loads(state.read_text()).get('active_project')==second,'legacy project selection')
        prepared=subprocess.run([str(app),'--plan-herdr-migration'],env=env,check=True,capture_output=True,text=True)
        plan_path=Path(json.loads(prepared.stdout.strip().splitlines()[-1])['plan']);plan=json.loads(plan_path.read_text())
        assert len(plan['views'])==1 and plan['views'][0]['project']==second
        applied=subprocess.run(['python3',str(root/'scripts/migrate-herdr.py'),'apply',str(plan_path)],env=env,capture_output=True,text=True,timeout=45)
        if applied.returncode:
            raise AssertionError(applied.stderr+'\n'+(Path(plan['run'])/'broker.log').read_text()[-2000:])
        status=json.loads(applied.stdout.strip().splitlines()[-1]);broker_pid=status['broker_pid']
        source.wait(timeout=8);legacy.wait(timeout=5)
        successor=plan['views'][0]['successor']
        resumed=subprocess.Popen([str(app),'--graphical-relay',first],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE);view_processes.append(resumed)
        resumed.stdin.write(json.dumps(hello).encode()+b'\n');resumed.stdin.flush()
        def resumed_frames():
            try:
                for line in resumed.stdout:
                    message=json.loads(line)
                    if message['type']=='scene':
                        scene=message['scene'];resumed.stdin.write(json.dumps(dict(type='presented',revision=scene['revision'],generation=scene['viewport']['generation'])).encode()+b'\n');resumed.stdin.flush()
            except (OSError,ValueError):pass
        threading.Thread(target=resumed_frames,daemon=True).start()
        resumed_state=runtime/'starmux'/(successor+'.state.json')
        wait(lambda:resumed_state.exists() and json.loads(resumed_state.read_text()).get('active_project')==second,'native frontend restores selected project')
        assert all(is_alive(pid) for pid in pids),'Live ownership changed an original PID'
        for p in plan['panes']:
            token='AFTER_TRANSFER_'+secrets.token_hex(4)
            subprocess.run(['tmux','-L',p['server'],'send-keys','-t',p['target'],'-l',token],env=env,check=True)
            subprocess.run(['tmux','-L',p['server'],'send-keys','-t',p['target'],'Enter'],env=env,check=True)
            def captured():return subprocess.check_output(['tmux','-L',p['server'],'capture-pane','-p','-t',p['target']],env=env,text=True)
            wait(lambda:'REPLY='+token in captured(),'post-transfer interactive IO')
            # Inspect through STAR/MUX, not Herdr, and verify the original shell identity.
            with socket.socket(socket.AF_UNIX) as inspector:
                inspector.connect(plan['broker']);inspector.sendall(json.dumps(dict(op='inspect',key=p['key'])).encode()+b'\n')
                inspected=json.loads(inspector.makefile('rb').readline())
            assert inspected['pid']==p['pid'] and inspected['alive'] and inspected['source']=='starmux-adopted'
            project=json.loads(Path(plan['project_root'],p['workspace']+'.json').read_text())
            assert project['herdr'] is None and str(p['slot']) in project['adopted']
        assert not (config/'herdr.sock').exists(),'Old Herdr API is still running'
        assert json.loads((config/'session.json').read_text())['workspaces']==[], 'Old restore could duplicate live agents'
        stopped=plan['panes'][0];surviving=plan['panes'][1]
        result=subprocess.run([str(app),'--workspace',stopped['workspace'],'--stop'],env=env,capture_output=True,timeout=15)
        assert result.returncode==0,result.stderr.decode(errors='replace')
        wait(lambda:not is_alive(stopped['pid']),'explicit stop closes original fake session')
        assert is_alive(surviving['pid']),'Stopping one project ended another project'
        print(json.dumps(dict(result='PASS',panes=len(pids),rollback_preserved_pids=True,old_server_exited=True,original_pids_preserved=True,post_transfer_input=True,native_inspection=True,frontend_restored=True,old_restore_retired=True,explicit_stop_closes_original=True)))
    finally:
        if peer:peer.close()
        for child in view_processes:
            if child.stdin:
                try:child.stdin.close()
                except OSError:pass
            if child.poll() is None:
                child.terminate();child.wait(timeout=5)
        if plan:
            for server in {p['server'] for p in plan['panes']}:
                subprocess.run(['tmux','-L',server,'kill-server'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        if broker_pid and is_alive(broker_pid):os.kill(broker_pid,15)
        if source.poll() is None:source.terminate();source.wait(timeout=5)
        # Only fake shell PIDs from this private server are eligible for cleanup.
        for pid in locals().get('pids',[]):
            if is_alive(pid):os.kill(pid,15)
