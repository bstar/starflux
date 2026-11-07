#!/usr/bin/env python3
"""Headless domain automation against private tmux processes, optionally over SSH."""
import argparse
import getpass
import json
import os
from pathlib import Path
import queue
import secrets
import shlex
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import uuid

parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--ssh',action='store_true');parser.add_argument('--session-imports',action='store_true');parser.add_argument('--herdr',action='store_true');args=parser.parse_args()
if args.herdr and not args.session_imports:parser.error('--herdr requires --session-imports')
app=Path(__file__).resolve().parents[1]/'scripts/run-local.sh'
with tempfile.TemporaryDirectory(prefix='sf-api-') as directory:
    tmp=Path(directory);runtime=tmp/'runtime';runtime.mkdir(mode=0o700);(tmp/'tmux').mkdir(mode=0o700)
    env={k:v for k,v in os.environ.items() if not k.startswith('HERDR_') and k not in ('TMUX','SSH_CONNECTION','SSH_TTY')}
    shell=tmp/'agent'
    shell.write_text("""#!/usr/bin/python3
import ctypes,os,sys,tty
ctypes.CDLL(None).prctl(15,ctypes.c_char_p(b'codex'),0,0,0)
tty.setraw(0);os.write(1,b'\\x1b[?2004hAPI_READY\\r\\n')
data=b'';draft=b'';pastes=0;submits=0
while True:
 chunk=os.read(0,65536)
 if not chunk:break
 data+=chunk
 while data:
  if data.startswith(b'\\x1b[200~'):
   end=data.find(b'\\x1b[201~',6)
   if end<0:break
   draft+=data[6:end];data=data[end+6:];pastes+=1
   os.write(1,('PASTE '+str(pastes)+' '+repr(draft.decode())+'\\r\\n').encode())
  elif data[:1] in (b'\\r',b'\\n'):
   data=data[1:];submits+=1
   os.write(1,('SUBMIT '+str(submits)+' '+repr(draft.decode())+'\\r\\n').encode());draft=b''
  elif data.startswith(b'\\x1b') and len(data)<6:break
  else:draft+=data[:1];data=data[1:]
""");shell.chmod(0o700)
    env.update(XDG_RUNTIME_DIR=str(runtime),XDG_DATA_HOME=str(tmp/'data'),XDG_CONFIG_HOME=str(tmp/'config'),TMUX_TMPDIR=str(tmp/'tmux'),SHELL=str(shell))
    clients=[];daemon=None;ssh_args=[];herdr=None;herdr_named=None;source_socket=None
    if args.herdr:
        config=tmp/'config/herdr';config.mkdir(parents=True);(config/'config.toml').write_text('onboarding = false\n');env['HERDR_CONFIG_PATH']=str(config/'config.toml')
    else:env['STARFLUX_HERDR']=str(tmp/'herdr-not-installed')
    class Client:
        def __init__(self,overrides=None):
            self.messages=queue.Queue();self.counter=0
            client_env={**env,**(overrides or {})}
            command=[str(app),'--api-stdio']
            if args.ssh:command=ssh_args+[shlex.join(['env',*[k+'='+client_env[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','PATH','HERDR_CONFIG_PATH','STARFLUX_HERDR') if k in client_env],*command])]
            self.log=(tmp/('client-'+secrets.token_hex(3)+'.log')).open('w')
            self.process=subprocess.Popen(command,env=client_env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log);clients.append(self)
            def read():
                for line in self.process.stdout:
                    try:self.messages.put(json.loads(line))
                    except ValueError:self.messages.put(dict(invalid=repr(line)))
            threading.Thread(target=read,daemon=True).start()
        def send(self,method,params=None,operation=None,deadline_ms=30000):
            self.counter+=1;identity=self.counter
            self.process.stdin.write(json.dumps(dict(version=1,id=identity,method=method,params=params or {},operation_id=operation,deadline_ms=deadline_ms)).encode()+b'\n');self.process.stdin.flush();return identity
        def take(self,timeout=10):
            try:return self.messages.get(timeout=timeout)
            except queue.Empty:raise AssertionError(('API response timed out',self.process.poll(),Path(self.log.name).read_text()))
        def call(self,method,params=None,operation=None,ok=True,deadline_ms=30000):
            identity=self.send(method,params,operation,deadline_ms);response=self.take()
            assert response.get('id')==identity and response.get('ok')==ok,response
            return response['result'] if ok else response['error']
        def write(self,method,params,operation=None):return self.call(method,params,operation or str(uuid.uuid4()))
    def wait(predicate,label,timeout=8):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            value=predicate()
            if value:return value
            time.sleep(.05)
        raise AssertionError(label)
    try:
        if args.ssh:
            for key in ('host','client'):subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(tmp/key)],check=True)
            (tmp/'authorized_keys').write_text((tmp/'client.pub').read_text())
            with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
            (tmp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(tmp/'host.pub').read_text())
            (tmp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
            daemon=subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd','-D','-e','-f',str(tmp/'sshd_config')],stderr=(tmp/'sshd.log').open('w'))
            def ready():
                assert daemon.poll() is None
                try:
                    with socket.create_connection(('127.0.0.1',port),timeout=.1):return True
                except OSError:return False
            wait(ready,'private SSH listener missing')
            ssh_args=['ssh','-F','/dev/null','-T','-p',str(port),'-i',str(tmp/'client'),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(tmp/'known_hosts'),'127.0.0.1']
            wrapper=tmp/'remote-starflux';wrapper.write_text('#!/bin/sh\nexec '+shlex.join(['env',*[k+'='+env[k] for k in ('XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME','TMUX_TMPDIR','SHELL','PATH')],str(app)])+' "$@"\n');wrapper.chmod(0o700)
            config=tmp/'ssh-config';config.write_text(f'Host proof\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {tmp}/client\n IdentitiesOnly yes\n BatchMode yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {tmp}/known_hosts\n')
            result=json.loads(subprocess.check_output([str(app),'--ssh','proof','--ssh-config',str(config),'--remote-executable',str(wrapper),'--api',json.dumps(dict(version=1,id='relay',method='capabilities'))],env=env))
            assert result['ok'] and result['id']=='relay',result
        if args.herdr:
            herdr=subprocess.Popen(['herdr','server'],env=env,stdout=(tmp/'herdr.log').open('w'),stderr=subprocess.STDOUT)
            wait(lambda:(tmp/'config/herdr/herdr.sock').exists(),'private Herdr server missing')
            def herdr_cli(*arguments):return json.loads(subprocess.check_output(['herdr',*arguments],env=env))
            herdr_cli('workspace','create','--cwd',str(tmp),'--label','Native import café','--no-focus')
            herdr_cli('workspace','create','--cwd',str(tmp),'--label','Other workspace','--no-focus')
            wait(lambda:(tmp/'config/herdr/session.json').exists(),'Herdr layout missing')
            herdr_named=subprocess.Popen(['herdr','--session','named','server'],env=env,stdout=(tmp/'herdr-named.log').open('w'),stderr=subprocess.STDOUT)
            wait(lambda:(tmp/'config/herdr/sessions/named/herdr.sock').exists(),'named Herdr socket missing')
            herdr_cli('--session','named','workspace','create','--cwd',str(tmp),'--label','Named source','--no-focus')
        c=Client();assert c.call('projects.list')['projects']==[],'API created an implicit demo project'
        if args.session_imports:
            sockets=tmp/'tmux'/('tmux-'+str(os.getuid()));sockets.mkdir(parents=True,exist_ok=True,mode=0o700);source_socket=sockets/'original'
            def tmux(*arguments):return subprocess.check_output(['tmux','-S',str(source_socket),*arguments],env=env,text=True).strip()
            source_directory=tmp/'import café space ';source_directory.mkdir()
            tmux('new-session','-d','-s','Original café','-c',str(source_directory),str(shell))
            tmux('new-window','-d','-t','Original café','-c',str(tmp),str(shell))
            def topology():return tmux('list-panes','-a','-F','#{session_id} #{window_id} #{pane_id} #{pane_pid}')
            before=topology()
            def source():return next(s for s in c.call('sessions.external')['sources'] if s['source']=='tmux' and s['name']=='Original café')
            initial=source();assert initial==source(),'discovery reference was not stable'
            assert c.call('sessions.import',dict(reference={**initial['reference'],'fingerprint':'invalid'}),str(uuid.uuid4()),ok=False)['code']=='invalid_params'
            operation=str(uuid.uuid4());params=dict(reference=initial['reference'])
            other=Client();first_id=c.send('sessions.import',params,operation);second_id=other.send('sessions.import',params,str(uuid.uuid4()))
            first=c.take();second=other.take();assert first['id']==first_id and second['id']==second_id and first['ok'] and second['ok'],(first,second)
            imported=first['result'];assert second['result']['project']==imported['project'],'concurrent imports duplicated the source'
            assert c.write('sessions.import',params,operation)==imported,'operation replay duplicated import'
            assert c.write('sessions.import',params)['project']==imported['project'],'a second operation duplicated source membership'
            assert len(c.call('projects.list')['projects'])==1 and topology()==before
            assert c.call('projects.list')['projects'][0]['directory']==str(source_directory),'import changed the source working directory'
            assert c.call('frontends.list',dict(project=imported['project']))['frontends']==[],'headless import opened a presentation'
            assert c.call('panes.get',dict(project=imported['project'],pane=1))['status']['source']=='tmux'
            tmux('rename-session','-t','Original café','Renamed source')
            assert c.call('sessions.import',params,str(uuid.uuid4()),ok=False)['code']=='stale_source'
            assert c.write('sessions.import',params,operation)==imported,'replay re-ran an import against a changed source'
            tmux('rename-session','-t','Renamed source','Original café')
            changed=source();tmux('new-window','-d','-t','Original café','-c',str(tmp),str(shell))
            assert c.call('sessions.import',dict(reference=changed['reference']),str(uuid.uuid4()),ok=False)['code']=='stale_source'
            assert c.write('sessions.import',dict(reference=source()['reference']))['project']==imported['project']
            assert len(c.call('projects.list')['projects'])==1
            obsolete=source()['reference'];old_socket=source_socket.stat().st_ino
            tmux('kill-server')
            assert c.call('sessions.import',dict(reference=obsolete),str(uuid.uuid4()),ok=False)['code']=='source_unavailable'
            tmux('new-session','-d','-s','Original café','-c',str(tmp),str(shell))
            assert c.call('sessions.import',dict(reference=obsolete),str(uuid.uuid4()),ok=False)['code']=='stale_source','a replaced source server accepted an old reference'
            assert c.write('sessions.import',dict(reference=source()['reference']))['project']==imported['project']
            if args.herdr:
                original=herdr_cli('api','snapshot')['result']['snapshot'];panes=original['panes']
                pids=[herdr_cli('pane','process-info','--pane',p['pane_id'])['result']['process_info']['shell_pid'] for p in panes]
                selected=next(s for s in c.call('sessions.external')['sources'] if s['source']=='herdr' and s['name']=='default')
                params=dict(reference=selected['reference']);op=str(uuid.uuid4())
                result=c.write('sessions.import',params,op);assert result==c.write('sessions.import',params,op)
                assert result['session']=='herdr-default' and len(result['projects'])>=2
                assert c.write('sessions.import',params)['project']==result['project']
                assert {p['pane_id'] for p in herdr_cli('api','snapshot')['result']['snapshot']['panes']}=={p['pane_id'] for p in panes}
                assert all(Path('/proc',str(pid)).exists() for pid in pids),'Herdr import restarted a terminal'
                herdr_cli('workspace','create','--cwd',str(tmp),'--label','Changed topology','--no-focus')
                assert c.call('sessions.import',params,str(uuid.uuid4()),ok=False)['code']=='stale_source'
                named=next(s for s in c.call('sessions.external')['sources'] if s['source']=='herdr' and s['name']=='named')
                result=c.write('sessions.import',dict(reference=named['reference']));assert result['session']=='herdr-named' and result['projects']
                latest=next(s for s in c.call('sessions.external')['sources'] if s['source']=='herdr' and s['name']=='default')['reference']
                herdr.terminate();herdr.wait(timeout=5)
                assert c.call('sessions.import',dict(reference=latest),str(uuid.uuid4()),ok=False)['code']=='source_unavailable','import restarted a missing Herdr server'
                assert not any(s['source']=='herdr' and s['name']=='default' for s in c.call('sessions.external')['sources'])
                herdr=subprocess.Popen(['herdr','server'],env=env,stdout=(tmp/'herdr-restarted.log').open('w'),stderr=subprocess.STDOUT)
                wait(lambda:any(s['source']=='herdr' and s['name']=='default' for s in c.call('sessions.external')['sources']),'restarted private Herdr server missing')
                assert c.call('sessions.import',dict(reference=latest),str(uuid.uuid4()),ok=False)['code']=='stale_source','restarted Herdr server accepted an old reference'
            slow=tmp/'slow-discovery';record=tmp/'query-processes'
            slow.write_text('#!/usr/bin/python3\nimport os,subprocess,time,json\nfrom pathlib import Path\nchild=subprocess.Popen(["sleep","6"])\nPath('+repr(str(record))+').write_text(json.dumps([os.getpid(),child.pid]))\nchild.wait()\nprint("{\\\"sessions\\\":[]}")\n');slow.chmod(0o700)
            delayed=Client(dict(STARFLUX_HERDR=str(slow)))
            failure=delayed.call('sessions.external',ok=False,deadline_ms=500)
            assert failure['code']=='deadline_exceeded' and record.exists(),failure
            query_pids=json.loads(record.read_text())
            def query_alive(pid):
                try:return Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()[0]!='Z'
                except OSError:return False
            wait(lambda:all(not query_alive(pid) for pid in query_pids),'cancelled discovery left read helpers running',timeout=.6)
            assert delayed.call('capabilities')['version']==1,'cancellation broke the API connection'
            retained=topology();began=time.monotonic()
            assert any(s['source']=='tmux' for s in delayed.call('sessions.external')['sources'])
            assert time.monotonic()-began<5.8,'source watchdog did not bound a slow CLI'
            query_pids=json.loads(record.read_text());wait(lambda:all(not query_alive(pid) for pid in query_pids),'source timeout left read helpers running',timeout=.6)
            assert topology()==retained,'query cleanup changed the original source'
            print(json.dumps(dict(result='PASS',ssh=args.ssh,herdr=args.herdr,headless_import=True,stable_discovery=True,guarded_source_identity=True,stale_topology_rejected=True,replaced_server_rejected=True,idempotent_import=True,source_processes_and_topology_preserved=True,json_stdout_clean=True,cancelled_query_cleanup=True,source_watchdog_cleanup=True)))
            raise SystemExit(0)
        caps=c.call('capabilities');assert caps['version']==1 and caps['operation_replay']
        c.process.stdin.write(b'{invalid\n');c.process.stdin.flush();assert c.take()['error']['code']=='invalid_request'
        assert c.call('capabilities')['version']==1,'malformed input broke the connection'
        assert c.call('unknown',ok=False)['code']=='method_not_found'
        assert c.call('sessions.create',dict(name='No operation',directory=str(tmp)),ok=False)['code']=='operation_id_required'
        operation=str(uuid.uuid4());create=dict(name='API Proof',directory=str(tmp))
        created=c.write('sessions.create',create,operation);project=created['project'];session=created['session']
        assert c.write('sessions.create',create,operation)==created,'replayed create made a new project'
        assert c.call('operations.get',dict(operation_id=operation))['status']=='completed'
        assert c.call('sessions.create',dict(name='Conflict',directory=str(tmp)),operation,ok=False)['code']=='operation_id_conflict'
        c.write('sessions.default',dict(session=session));assert any(s['is_default'] for s in c.call('sessions.list')['sessions'])
        assert c.call('sessions.default',{},str(uuid.uuid4()),ok=False)['code']=='invalid_params'
        observed=wait(lambda:(r if (r:=c.call('panes.get',dict(project=project,pane=1)))['status'].get('agent_pid') else None),'agent identity missing')
        original=observed['target'];assert original['pid'] and original['agent_pid']
        assert uuid.UUID(original['boot_id'])
        old_boot={**original,'boot_id':str(uuid.uuid4())}
        for method,params in [('panes.send',dict(target=old_boot,text='OLD_BOOT_INPUT',submit=True)),('panes.terminate',dict(target=old_boot,confirm='TERMINATE'))]:
            assert c.call(method,params,str(uuid.uuid4()),ok=False)['code']=='stale_target'
        incomplete={k:v for k,v in original.items() if k!='boot_id'}
        assert c.call('panes.send',dict(target=incomplete,text='MISSING_BOOT_INPUT',submit=True),str(uuid.uuid4()),ok=False)['code']=='invalid_params'
        wait(lambda:'API_READY' in c.call('panes.capture',dict(project=project,pane=1))['text'],'fixture did not initialize')
        send=str(uuid.uuid4());payload=dict(target=original,text='hello 界\nsecond line',submit=False)
        delivered=c.write('panes.send',payload,send);assert delivered['delivered'] and not delivered['submitted']
        assert c.write('panes.send',payload,send)==delivered
        text=c.call('panes.capture',dict(project=project,pane=1))['text'];assert 'PASTE 1' in text and 'PASTE 2' not in text and 'SUBMIT ' not in text,text
        enter=str(uuid.uuid4());c.write('panes.send',dict(target=original,text='',submit=True),enter);c.write('panes.send',dict(target=original,text='',submit=True),enter)
        text=c.call('panes.capture',dict(project=project,pane=1))['text'];assert 'SUBMIT 1' in text and 'SUBMIT 2' not in text,text
        assert c.call('panes.send',dict(target=original,text='\x1b[31m',submit=False),str(uuid.uuid4()),ok=False)['code']=='invalid_params'
        result=c.call('panes.wait',dict(target=original,state='idle',timeout_ms=200),ok=False);assert result['code']=='wait_timeout',result
        watch=Client();watch_id=watch.send('events.subscribe',dict(project=project,resources=['agents'],duration_ms=6000,max_events=2));initial=watch.take();assert initial['ok'] and initial['result']['snapshot']['agents']==[]
        report=dict(run_id=str(uuid.uuid4()),sequence=1,epoch=original['epoch'],agent_pid=original['agent_pid'],agent_start_ticks=original['agent_start_ticks'],boot_id=original['boot_id'],harness='codex',source='test:api',state='working',conversation_id='api-thread',resume_argv=['codex','resume','api-thread'])
        assert c.call('agents.report',dict(project=project,pane=1,report={**report,'boot_id':str(uuid.uuid4())}),str(uuid.uuid4()),ok=False)['code']=='operation_failed'
        record=c.write('agents.report',dict(project=project,pane=1,report=report));assert record['pane']==1
        event=watch.take();assert event['event']['data']['state']=='working' and event['event']['sequence']>0,event
        first_sequence=event['event']['sequence']
        current=c.call('panes.get',dict(project=project,pane=1))['target'];assert current['run_id']==report['run_id']
        report['sequence']=2;c.write('agents.report',dict(project=project,pane=1,report=report))
        waiting=Client();waiting_id=waiting.send('panes.wait',dict(target=current,state='completed',timeout_ms=5000))
        time.sleep(.6);assert watch.messages.empty(),'heartbeat created a false lifecycle transition'
        report['sequence']=3;report['state']='completed';c.write('agents.report',dict(project=project,pane=1,report=report))
        result=waiting.take();assert result['id']==waiting_id and result['ok'] and result['result']['observation']['status']['health']['confidence']=='reported',result
        event=watch.take();assert event['event']['sequence']>first_sequence and event['event']['data']['state']=='completed',event
        ended=watch.take()['ended'];assert ended['reason']=='event_limit';cursor=ended['cursor']
        attention=c.call('attention.list',dict(project=project))['items'];assert len(attention)==1 and attention[0]['pane']==1
        c.write('attention.dismiss',dict(project=project,event=attention[0]['id']));assert c.call('attention.list',dict(project=project))['items']==[]
        # Both transitions happen before the new consumer connects. Replay must
        # return each, rather than only the latest sampled lifecycle state.
        report['sequence']=4;report['state']='working';c.write('agents.report',dict(project=project,pane=1,report=report))
        report['sequence']=5;report['state']='completed';c.write('agents.report',dict(project=project,pane=1,report=report))
        replay=Client();replay.send('events.subscribe',dict(project=project,resources=['agents'],after=cursor,duration_ms=3000,max_events=2))
        initial=replay.take();assert initial['ok'] and initial['result']['snapshot'] is None and initial['result']['resume_supported']
        transitions=[replay.take()['event'],replay.take()['event']]
        assert [event['data']['state'] for event in transitions]==['working','completed'],transitions
        assert all(event['cursor'] and event['emitted_ms'] for event in transitions)
        assert replay.take()['ended']['events']==2
        create_pane=str(uuid.uuid4());second=c.write('panes.create',dict(project=project,label='Second'),create_pane)
        assert c.write('panes.create',dict(project=project,label='Second'),create_pane)==second
        assert len(c.call('panes.list',dict(project=project))['panes'])==2
        wait(lambda:'API_READY' in c.call('panes.capture',dict(project=project,pane=2))['text'],'second pane did not initialize')
        target=c.call('panes.get',dict(project=project,pane=2))['target'];assert target and target['pane']==2
        c.write('panes.rename',dict(target=target,label='Renamed'));assert c.call('panes.get',dict(project=project,pane=2))['label']=='Renamed'
        poison={**target,'start_ticks':target['start_ticks']+1}
        assert c.call('panes.send',dict(target=poison,text='WRONG_PROCESS',submit=True),str(uuid.uuid4()),ok=False)['code']=='stale_target'
        c.write('panes.terminate',dict(target=target,confirm='TERMINATE'))
        replacement=c.write('panes.create',dict(project=project,label='Replacement'))
        assert replacement['target']['pane']==2 and replacement['target']['epoch']!=target['epoch']
        assert c.call('panes.send',dict(target=target,text='WRONG_GENERATION',submit=True),str(uuid.uuid4()),ok=False)['code']=='stale_target'
        assert 'WRONG_' not in c.call('panes.capture',dict(project=project,pane=2))['text']
        assert c.call('panes.get',dict(project=project,pane=1))['target']['pid']==original['pid'],'unrelated agent was replaced'
        data=tmp/'data/starflux/projects';slow='slow-proof';slow_source=tmp/'slow-source'
        slow_source.write_text('#!/bin/sh\nsleep 10\n');slow_source.chmod(0o700)
        config=json.loads((data/(project+'.json')).read_text());config.update(workspace=slow,herdr=dict(session='private',workspace_id='w1',order=0,executable=str(slow_source),panes={'0':dict(terminal_id='private',pane_id='p1',tab_id='t1',label='Slow',cwd=str(tmp),agent=None)}))
        (data/(slow+'.json')).write_text(json.dumps(config));(data/(slow+'.model.json')).write_text((data/(project+'.model.json')).read_text())
        interrupted=str(uuid.uuid4());slow_report={**report,'run_id':str(uuid.uuid4()),'sequence':1}
        started=time.monotonic();result=c.call('agents.report',dict(project=slow,pane=1,report=slow_report),interrupted,ok=False,deadline_ms=400)
        assert result['code']=='deadline_exceeded' and result['outcome']=='unknown' and time.monotonic()-started<3,result
        assert c.call('operations.get',dict(operation_id=interrupted))['status']=='indeterminate'
        result=c.call('agents.report',dict(project=slow,pane=1,report=slow_report),interrupted,ok=False)
        assert result['code']=='operation_indeterminate',result
        assert c.call('frontends.list',dict(project=project))['frontends']==[],'headless API started a presentation'
        c.write('sessions.default',dict(session=None))
        print(json.dumps(dict(result='PASS',ssh=args.ssh,headless=True,versioned_jsonl=True,idempotent_writes=True,guarded_input=True,explicit_submit=True,lifecycle_wait=True,filtered_events=True,durable_event_replay=True,heartbeat_coalescing=True,attention=True,stale_target_rejected=True,deadline_recovery=True,unrelated_agent_preserved=True)))
    finally:
        for client in clients:
            if client.process.poll() is None:
                client.process.terminate()
                try:client.process.wait(timeout=3)
                except subprocess.TimeoutExpired:client.process.kill();client.process.wait()
        if source_socket:subprocess.run(['tmux','-S',str(source_socket),'kill-server'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        if herdr:herdr.terminate();herdr.wait(timeout=5)
        if herdr_named:herdr_named.terminate();herdr_named.wait(timeout=5)
        for model in (tmp/'data/starflux/projects').glob('*.model.json'):
            project=model.name.removesuffix('.model.json')
            subprocess.run(['tmux','-L','starflux-'+project,'kill-server'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        if daemon:daemon.terminate();daemon.wait(timeout=3)
