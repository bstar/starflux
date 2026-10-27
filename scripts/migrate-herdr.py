#!/usr/bin/env python3
"""Prepare/back up and apply an explicitly requested, live default-Herdr migration."""
import base64
import copy
import datetime
import hashlib
import json
import os
from pathlib import Path
import shlex
import signal
import socket
import subprocess
import sys
import tempfile
import time
import uuid


def atomic(path,value):
    path=Path(path);path.parent.mkdir(parents=True,exist_ok=True)
    data=value if isinstance(value,bytes) else json.dumps(value,ensure_ascii=False,indent=2).encode()
    with tempfile.NamedTemporaryFile(dir=path.parent,delete=False) as f:
        f.write(data);f.flush();os.fsync(f.fileno());temp=Path(f.name)
    temp.chmod(0o600);temp.replace(path)

def alive(pid):
    try:return Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()[0]!='Z'
    except OSError:return False

def query(*args,raw=False):
    result=subprocess.run(['herdr','--session','default',*args],stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=15)
    if result.returncode:raise RuntimeError('Herdr read failed: '+result.stderr.decode(errors='replace')[:300])
    return result.stdout if raw else json.loads(result.stdout)

def runtime():return Path(os.environ.get('XDG_RUNTIME_DIR',f'/run/user/{os.getuid()}'))/'starmux'
def project_root():return Path(os.environ.get('XDG_DATA_HOME',Path.home()/'.local/share'))/'starmux/projects'
def active_views(root,projects):
    result=[];seen=set()
    for path in root.glob('*.state.json'):
        try:
            state=json.loads(path.read_text());pid=state.get('controller_pid');active=state.get('active_project')
            if not pid or pid in seen or active not in projects or not alive(pid):continue
            args=Path('/proc',str(pid),'cmdline').read_bytes().split(b'\0')
            if b'--serve' not in args and b'--serve-view' not in args:continue
            identity=path.name.removesuffix('.state.json')
            if not (root/(identity+'.sock')).exists():continue
            if state.get('ui_refresh') is not True:raise RuntimeError('An old frontend needs to be reopened before live migration: '+identity)
            descriptor=root/(identity+'.frontend.json')
            workspace=json.loads(descriptor.read_text())['workspace'] if descriptor.exists() else identity
            result.append(dict(id=identity,pid=pid,workspace=workspace,project=active,view_state=state.get('project_state')));seen.add(pid)
        except (OSError,ValueError,KeyError):continue
    return result

def ui_clients(config_path,owned_pids):
    result=[]
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit():continue
        try:
            argv=[v.decode() for v in (proc/'cmdline').read_bytes().split(b'\0') if v]
            if len(argv)!=1 or Path(argv[0]).name!='herdr':continue
            env={}
            for field in (proc/'environ').read_bytes().split(b'\0'):
                key,_,value=field.partition(b'=')
                if key in (b'HERDR_CONFIG_PATH',b'XDG_CONFIG_HOME',b'HERDR_SESSION'):
                    env[key.decode()]=value.decode()
            client_config=Path(env.get('HERDR_CONFIG_PATH',str(Path(env.get('XDG_CONFIG_HOME',str(Path.home()/'.config')))/'herdr/config.toml')))
            if client_config.resolve()!=config_path.resolve() or env.get('HERDR_SESSION','default')!='default':continue
            ancestor=int(proc.name);inside=False
            for _ in range(64):
                if ancestor in owned_pids:inside=True;break
                if ancestor<=1:break
                ancestor=int(Path('/proc',str(ancestor),'stat').read_text().rsplit(')',1)[1].split()[1])
            if inside:continue
            start=(proc/'stat').read_text().rsplit(')',1)[1].split()[19]
            result.append(dict(pid=int(proc.name),start=start))
        except (OSError,ValueError,UnicodeError,IndexError):continue
    return result

def prepare(executable,proxy):
    root=runtime();data=project_root();root.mkdir(mode=0o700,parents=True,exist_ok=True);data.mkdir(mode=0o700,parents=True,exist_ok=True)
    stamp=datetime.datetime.now().strftime('%Y%m%d-%H%M%S')+'-'+uuid.uuid4().hex[:6]
    run=data.parent/'migrations'/('herdr-default-'+stamp);run.mkdir(mode=0o700,parents=True)
    snapshot=query('api','snapshot');live=snapshot['result']['snapshot'];panes=live['panes']
    if not panes:raise RuntimeError('Herdr has no live panes to migrate')
    config_path=Path(os.environ.get('HERDR_CONFIG_PATH',Path(os.environ.get('XDG_CONFIG_HOME',Path.home()/'.config'))/'herdr/config.toml'))
    source=config_path.parent/'session.json';saved=json.loads(source.read_text())
    atomic(run/'herdr-session.json',source.read_bytes());atomic(run/'herdr-snapshot.json',snapshot)
    if config_path.exists():atomic(run/'herdr-config.toml',config_path.read_bytes())
    documents=[];entries=[];projects=[]
    for order,workspace in enumerate(live['workspaces']):
        wid=workspace['workspace_id'];name='herdr-default-'+wid
        members=[p for p in panes if p['workspace_id']==wid]
        if not members:continue
        projects.append(name)
        path=data/(name+'.json')
        if path.exists():config=json.loads(path.read_text())
        elif (root/(name+'.json')).exists():config=json.loads((root/(name+'.json')).read_text())
        else:raise RuntimeError('Import the Herdr project metadata first: '+name)
        source_meta=config.get('herdr')
        if not source_meta or source_meta['session']!='default' or source_meta['workspace_id']!=wid:
            raise RuntimeError('Refusing to overwrite a project that no longer references this Herdr session: '+name)
        config=copy.deepcopy(config)
        model_path=data/(name+'.model.json');layout_path=data/(name+'.layout.json')
        state=json.loads((model_path if model_path.exists() else layout_path).read_text())
        old=source_meta['panes'];canonical={}
        for slot,p in old.items():canonical.setdefault(p['pane_id'],[]).append(int(slot))
        canonical={pane:min(ids) for pane,ids in canonical.items()}
        aliases={int(slot):canonical[p['pane_id']] for slot,p in old.items()}
        ids=list(dict.fromkeys(aliases.get(int(i),int(i)) for i in state.get('terminals',[0])+state.get('agents',[])))
        missing={slot for pane,slot in canonical.items() if not any(p['pane_id']==pane for p in members)}
        ids=[i for i in ids if i not in missing]
        reserved=[i for i in state.get('reserved_slots',[]) if i not in aliases]
        state.update(terminal_first=True,agents=[],reserved_slots=reserved,terminals=ids)
        state.setdefault('terminal_epochs',{});state.setdefault('terminal_labels',{})
        for slot,label in state.get('agent_labels',{}).items():state['terminal_labels'].setdefault(str(aliases.get(int(slot),int(slot))),label)
        state['agent_labels']={}
        server='starmux-adopt-'+wid+'-'+hashlib.sha256(str(root).encode()).hexdigest()[:8]+'-'+stamp.rsplit('-',1)[-1]
        adopted=config.setdefault('adopted',{})
        for pane in members:
            pane_id=pane['pane_id'];slot=canonical.get(pane_id)
            if slot is None:
                slot=next((i for i in range(32) if i not in ids and i not in reserved),None)
                if slot is None:raise RuntimeError('No terminal slot available in '+name)
            if slot not in ids:ids.append(slot)
            process=query('pane','process-info','--pane',pane_id)['result']['process_info']
            pid=process.get('shell_pid')
            if not pid or not alive(pid):raise RuntimeError('Cannot hand off a pane without a live shell: '+pane_id)
            epoch=uuid.uuid4().hex;key=uuid.uuid4().hex
            screen=query('pane','read',pane_id,'--source','visible','--format','ansi','--raw',raw=True)
            if len(screen)>8*1024*1024:raise RuntimeError('Pane screen exceeds migration limit')
            cwd=pane.get('cwd') or config.get('directory') or str(Path.home())
            cwd=str(Path(cwd).expanduser())
            if not Path(cwd).is_dir():raise RuntimeError('Terminal directory is missing: '+cwd)
            state['terminal_epochs'][str(slot)]=epoch
            tab=next((t for t in live['tabs'] if t['tab_id']==pane['tab_id']),{})
            state['terminal_labels'].setdefault(str(slot),tab.get('label') or 'Terminal '+str(slot+1))
            entry=dict(key=key,pid=pid,epoch=epoch,workspace=name,source_pane=pane_id,terminal_id=pane['terminal_id'],slot=slot,server=server,session='p'+str(slot),target='p'+str(slot)+':0.0',cwd=cwd,screen=base64.b64encode(screen).decode())
            entries.append(entry)
            adopted[str(slot)]=dict(broker=str(root/('adopted-'+stamp.rsplit('-',1)[-1]+'.sock')),key=key,server=server,pid=pid,epoch=epoch,proxy=str(Path(proxy).resolve()))
        state['terminals']=ids
        state['terminal']=aliases.get(int(state.get('terminal',0)),int(state.get('terminal',0)))
        if state['terminal'] not in ids:state['terminal']=ids[0]
        if state.get('split_panes'):
            pair=[aliases.get(i,i) for i in state['split_panes']]
            if len(set(pair))!=2 or any(i not in ids for i in pair):state.update(split=False,split_panes=None)
            else:state['split_panes']=pair
        config.update(herdr=None,agent=None)
        for target,value in [(path,config),(root/(name+'.json'),config),(model_path,state)]:
            if target.exists():atomic(run/'backups'/target.parent.name/target.name,target.read_bytes())
            before=json.loads(target.read_text()) if target.exists() else None
            documents.append(dict(path=str(target),value=value,before=before,kind='project'))
        atomic(run/'project-summaries'/(name+'.json'),dict(label=config.get('label'),cwd=config.get('directory'),source=wid,terminals=len(members)))
    if len(entries)!=len(panes) or len({p['pid'] for p in entries})!=len(entries):raise RuntimeError('Terminal inventory is not one-to-one')
    views=active_views(root,projects)
    # A later accidental Herdr launch must not duplicate the migrated agents.
    retired=copy.deepcopy(saved);retired.update(workspaces=[],active=None,selected=0)
    documents.append(dict(path=str(source),value=retired,kind='herdr-retired'))
    for view in views:
        if view['id'].startswith('v-'):continue
        identity='v-'+uuid.uuid4().hex[:24]
        view['successor']=identity
        documents.extend([
            dict(kind='view',path=str(root/(view['workspace']+'.resume-view.json')),value=dict(id=identity)),
            dict(kind='view',path=str(data/(identity+'.workspace.json')),value=view['project']),
            dict(kind='view',path=str(data/'views'/identity/(view['project']+'.json')),value=view['view_state']),
        ])
    launch=Path(executable).resolve().parents[2]/'scripts/run-local.sh'
    if not launch.is_file():launch=Path(executable).resolve()
    clients=ui_clients(config_path,{p['pid'] for p in entries})
    with socket.socket(socket.AF_UNIX) as peer:
        peer.connect(str(config_path.parent/'herdr.sock'))
        source_pid=__import__('struct').unpack('3i',peer.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))[0]
    plan=dict(source_pid=source_pid,source_clients=clients,source_config=str(config_path.resolve()),version=1,run=str(run),runtime_root=str(root),project_root=str(data),broker=str(root/('adopted-'+stamp.rsplit('-',1)[-1]+'.sock')),
              executable=str(launch),proxy_executable=str(Path(proxy).resolve()),panes=entries,projects=projects,documents=documents,views=views,
              environment={k:str(v) for k,v in dict(XDG_RUNTIME_DIR=root.parent,XDG_DATA_HOME=data.parent.parent,SHELL=os.environ.get('SHELL','/bin/bash')).items()})
    # A stable shared broker identity is used by every migrated project.
    broker=str(root/('adopted-'+stamp.rsplit('-',1)[-1]+'.sock'));plan['broker']=broker
    for document in documents:
        if 'adopted' in document['value']:
            for terminal in document['value']['adopted'].values():terminal['broker']=broker
    gates=list(dict.fromkeys(projects+[v['workspace'] for v in views]));plan['gates']=[str(root/(name+'.migration')) for name in gates]
    atomic(run/'plan.json',plan)
    subprocess.run([str(launch),'--validate-pty-plan',str(run/'plan.json')],check=True,stdout=subprocess.DEVNULL)
    wrapper=run/'accept';wrapper.write_text('#!/bin/sh\nshift 2\nexec '+shlex.quote(str(launch))+' --native-pty-import '+shlex.quote(str(run/'plan.json'))+' "$@"\n');wrapper.chmod(0o700)
    print(json.dumps(dict(phase='prepared',plan=str(run/'plan.json'),projects=len(projects),terminals=len(entries),frontends=len(views),source_processes_restarted=False)),flush=True)
    return run/'plan.json'

def apply(path):
    path=Path(path);plan=json.loads(path.read_text());run=Path(plan['run']);root=Path(plan['runtime_root'])
    if (run/'status.json').exists() and json.loads((run/'status.json').read_text()).get('phase')=='active':raise RuntimeError('This migration is already active')
    pids={p['pid'] for p in plan['panes']}
    if not all(alive(pid) for pid in pids):raise RuntimeError('A source process exited since preflight; prepare a fresh migration')
    gates=plan['gates']
    try:
        atomic(run/'status.json',dict(phase='quiescing',pid=os.getpid()))
        for gate in gates:atomic(gate,dict(run=str(run),pid=os.getpid()))
        for view in plan['views']:
            directory=root/(view['id']+'.actions');directory.mkdir(mode=0o700,exist_ok=True)
            atomic(directory/(str(time.time_ns())+'.json'),'ui:refresh')
        deadline=time.monotonic()+15
        while any(alive(v['pid']) for v in plan['views']):
            if time.monotonic()>deadline:raise RuntimeError('A frontend could not quiesce; the Herdr server and terminals were left running')
            time.sleep(.05)
        for document in plan['documents']:
            if document.get('kind')!='project':continue
            target=Path(document['path']);current=json.loads(target.read_text()) if target.exists() else None
            if current!=document['before']:raise RuntimeError('Project metadata changed during preflight; prepare a fresh migration: '+target.name)
        # Retire UI-only Herdr clients. They own no shell/agent PTYs.
        retired=[]
        for client in plan.get('source_clients',[]):
            try:
                proc=Path('/proc',str(client['pid']));argv=[s for s in (proc/'cmdline').read_bytes().split(b'\0') if s]
                start=(proc/'stat').read_text().rsplit(')',1)[1].split()[19]
                if start==client['start'] and len(argv)==1 and Path(os.fsdecode(argv[0])).name=='herdr':
                    os.kill(client['pid'],signal.SIGTERM);retired.append(client['pid'])
            except (OSError,ValueError,IndexError):pass
        atomic(run/'status.json',dict(phase='handoff-requested',pid=os.getpid(),retired_ui_clients=retired))
        command=['herdr','--session','default','server','live-handoff','--import-exe',str(run/'accept')]
        with (run/'handoff-request.log').open('wb') as log:
            result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=50)
        status=json.loads((run/'status.json').read_text())
        if result.returncode or status.get('phase')!='active':raise RuntimeError('Live transfer did not complete; inspect '+str(run/'broker.log'))
        if not all(alive(pid) for pid in pids):raise RuntimeError('A transferred process exited during verification; native owner retained all remaining PTYs')
        print(json.dumps(dict(phase='active',projects=len(plan['projects']),terminals=len(pids),broker_pid=status['pid'],original_pids_preserved=True,backup=str(run))),flush=True)
    except BaseException as error:
        status=json.loads((run/'status.json').read_text()) if (run/'status.json').exists() else {}
        if status.get('phase') not in ['active','retained-error','committing']:
            atomic(run/'status.json',dict(phase='failed',error=str(error)))
            for gate in gates:Path(gate).unlink(missing_ok=True)
        raise

if __name__=='__main__':
    mode=sys.argv[1]
    if mode in ('prepare','migrate'):
        path=prepare(*sys.argv[2:4])
        if mode=='migrate':apply(path)
    elif mode=='apply':apply(sys.argv[2])
    else:raise SystemExit('Unknown migration command')
