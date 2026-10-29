#!/usr/bin/env python3
"""Terminal-first local/SSH tracking of agents launched from ordinary shells.

Only disposable projects and a private authenticated loopback SSH server are used.
"""
import getpass
import json
import os
from pathlib import Path
import socket
import shlex
import subprocess
import tempfile
import time
import uuid

repo = Path(__file__).resolve().parents[1]
app = repo/'scripts/run-local.sh'
runtime = Path(os.environ['XDG_RUNTIME_DIR'])/'starflux'
data_root = Path.home()/'.local/share/starflux/projects'
out = repo/'target'/os.environ.get('STARFLUX_AGENT_PROOF','multi-agent-proof')
out.mkdir(parents=True, exist_ok=True)
prefix = 'agents-'+uuid.uuid4().hex[:7]
names = [prefix+'-local', prefix+'-ssh']
original = json.loads(subprocess.check_output(['hyprctl', 'activewindow', '-j'])).get('address')
children = []


def wait(fn, label, timeout=35):
    end = time.monotonic()+timeout
    while time.monotonic() < end:
        try:
            value = fn()
            if value:
                return value
        except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
            pass
        time.sleep(.1)
    raise AssertionError(label)


def invoke(name, *args):
    return subprocess.check_output([str(app), '--workspace', name, *args], stderr=subprocess.PIPE, timeout=25)


def state():
    return json.loads((runtime/(names[0]+'.state.json')).read_text())


def action(value):
    invoke(names[0], '--action', value)


with tempfile.TemporaryDirectory(prefix='starflux-tools-') as temp:
    temp = Path(temp)
    address = 'unix:'+str(temp/'kitty.sock')
    def rc(*args, data=None):
        if args[0]=='send-text' and data and not data.startswith(b'\x1b['):
            data=b'\x1b[200~'+data+b'\x1b[201~'
        if args[0] in ['send-text','send-key']:
            args=(args[0],'--match','state:focused',*args[1:])
        return subprocess.check_output(['kitten', '@', '--to', address, *args], input=data, stderr=subprocess.PIPE, timeout=12)
    def click(x, y, right=False):
        b = 2 if right else 0
        rc('send-text', '--stdin', data=f'\x1b[<{b};{x};{y}M\x1b[<{b};{x};{y}m'.encode())
    def screenshot(name):
        def client():
            return next((c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['pid']==kitty.pid), None)
        c = wait(client, 'Outer Kitty missing')
        subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{c["address"]}"}}))'],stdout=subprocess.DEVNULL,check=True)
        time.sleep(.8)
        c = client()
        x, y = c['at']; w, h = c['size']
        subprocess.run(['grim', '-g', f'{x},{y} {w}x{h}', str(out/name)], check=True)
    def rename(text):
        wait(lambda: state()['modal_open'], 'Rename dialog missing')
        focus_cycle()
        rc('send-key', 'end', 'ctrl+u')
        rc('send-text', '--stdin', data=('\x1b[200~'+text+'\x1b[201~').encode())
        rc('send-key', 'enter')
        wait(lambda: not state()['modal_open'], 'Rename did not save')
    try:
        for key in ['host', 'client']:
            subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(temp/key)], check=True)
        (temp/'authorized_keys').write_text((temp/'client.pub').read_text())
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0)); port=sock.getsockname()[1]
        (temp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(temp/'host.pub').read_text())
        (temp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {temp}/host\nPidFile {temp}/sshd.pid\nAuthorizedKeysFile {temp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM yes\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
        sshconfig=temp/'ssh_config'
        sshconfig.write_text(f'Host tools-host\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {temp}/client\n IdentitiesOnly yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {temp}/known_hosts\n')
        daemon=subprocess.Popen(['/usr/bin/sshd', '-D', '-e', '-f', str(temp/'sshd_config')], stdout=subprocess.DEVNULL, stderr=(out/'sshd.log').open('w'), env={k:v for k,v in os.environ.items() if k!='NOTIFY_SOCKET'})
        children.append(daemon)
        wait(lambda: subprocess.run(['ssh','-F',str(sshconfig),'tools-host','true'], stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0, 'SSH unavailable')
        fixture=temp/'codex'
        fixture.write_text('''import json,os,sys,termios,tty,signal
old=termios.tcgetattr(0);tty.setraw(0)
def record(value):
 with open(sys.argv[1],'a') as f:f.write(json.dumps(dict(pid=os.getpid(),cwd=os.getcwd(),**value))+'\\n')
mode=False
def draw(waiting=None):
 global mode
 if waiting is not None:mode=waiting
 height=os.get_terminal_size().lines
 status='Would you like to run this command?\\n1. Yes, proceed (y)\\n2. No, and tell Codex (esc)' if mode else 'Working (1s · esc to interrupt)'
 os.write(1,('\\x1b[2J\\x1b[%d;1H%s'%(max(1,height-2),status)).encode())
signal.signal(signal.SIGWINCH,lambda *_:draw())
record({'kind':'ready'});os.write(1,b'\\x1b[?1049h');draw()
try:
 while True:
  chunk=os.read(0,4096)
  if not chunk:break
  record({'kind':'input','hex':chunk.hex()})
  if b'\\x04' in chunk:break
  if b'NEEDS_ATTENTION' in chunk:draw(True)
  if b'WORKING_AGAIN' in chunk:draw(False)
finally:
 os.write(1,b'\\x1b[?1049l');termios.tcsetattr(0,termios.TCSANOW,old)
''')
        for i,name in enumerate(names):
            folder=temp/str(i);folder.mkdir()
            cfg={'workspace':name,'collection':prefix,'label':'Local agents' if i==0 else 'SSH agents','theme':'kanagawa','host':None if i==0 else 'tools-host','ssh_config':None if i==0 else str(sshconfig),'directory':str(folder),'agent':None,'executable':str(repo/'target/release/starflux'),'project_host':True,'desktop_helper':str(app)}
            path=runtime/(name+'.json');path.write_text(json.dumps(cfg));path.chmod(0o600)
        kitty=subprocess.Popen(['/usr/bin/kitty','--hold','--class','starflux-agents-proof','--listen-on',address,'-o','allow_remote_control=yes',str(app),'--workspace',names[0]],env=dict(os.environ,STAR_GRAPHICS_RENDERER='gpu',STAR_TERMINAL_RENDERER=os.environ.get('STAR_TERMINAL_RENDERER','kitty')),stdout=subprocess.DEVNULL,stderr=(out/'kitty.log').open('w'))
        children.append(kitty)
        sink=subprocess.Popen(['/usr/bin/kitty','--class','starflux-focus-sink','--title','STAR/FLUX focus acceptance','sleep','300'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        children.append(sink)
        def focus_cycle(resize=False):
            for process in [sink,kitty]:
                def client():
                    return next((c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['pid']==process.pid),None)
                c=wait(client,'Focus test window missing')
                subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{c["address"]}"}}))'],stdout=subprocess.DEVNULL,check=True)
                wait(lambda:json.loads(subprocess.check_output(['hyprctl','activewindow','-j'])).get('pid')==process.pid,'Tiler did not change focus')
                if resize:
                    rc('set-font-size','--','+1' if process==sink else '-1')
                time.sleep(.25)
        def records(i):
            path=temp/f'{i}-input.jsonl'
            return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        def data(i,pid):
            return b''.join(bytes.fromhex(e['hex']) for e in records(i) if e['pid']==pid and e['kind']=='input')
        def agents():
            return {e['pane']:e['health']['agent_pid'] for e in state()['sessions'] if e['agent'] and (e.get('health') or {}).get('agent_pid')}
        identities={};shells={}
        for i,name in enumerate(names):
            if i:action('project:'+name)
            s=wait(lambda:state() if state()['active_project']==name and len(state()['sessions'])==1 and (state()['sessions'][0].get('health') or {}).get('alive') else None,'Initial shell missing')
            assert s['project_state']['terminal_first'] and s['project_state']['space']=='terminals'
            assert s['project_state']['agents']==[] and agents()=={}
            side=s['project_navigation']['sidebar_width']
            shell=state()['sessions'][0]['health']['pid']
            command='python3 '+shlex.quote(str(fixture))+' '+shlex.quote(str(temp/f'{i}-input.jsonl'))+'\r'
            rc('send-key','ctrl+escape');click(side+70,250)
            rc('send-text','--stdin',data=command.rstrip('\r').encode());time.sleep(.8);rc('send-key','enter')
            wait(lambda:0 in agents(),'Shell-launched Codex was not detected')
            first=agents()[0]
            assert first!=shell
            wait(lambda:any(e['pid']==first for e in records(i)),'Agent fixture not ready')
            wait(lambda:state()['project_activity'][name]['working']==1,'Shell agent did not drive project activity')
            payload=b'Case:aAzZ!@#$%^&*()_+{}:<>?END'
            rc('send-text','--stdin',data=payload)
            wait(lambda:payload in data(i,first),'Uppercase or punctuation bytes changed')
            rc('send-key','shift+a','shift+z','shift+1','ctrl+a','alt+x','ctrl+c')
            wait(lambda:b'AZ!' in data(i,first) and b'\x01\x1bx\x03' in data(i,first),'Shift/Control/Alt forwarding changed')
            focus_cycle(resize=True)
            rc('send-text','--stdin',data=b'AFTER_WINDOW_SWITCH')
            wait(lambda:b'AFTER_WINDOW_SWITCH' in data(i,first),'Terminal did not regain focus')
            before=data(i,first)
            rc('send-key','ctrl+escape');click(state()['viewport']['width']-140,183)
            wait(lambda:state()['project_state']['terminal']==1 and len(state()['sessions'])==2,'+ Tab did not open a shell')
            wait(lambda:any(e['pane']==1 and (e.get('health') or {}).get('alive') for e in state()['sessions']),'Second shell missing')
            assert 1 not in agents()
            rc('send-key','ctrl+escape');click(side+70,250)
            rc('send-text','--stdin',data=command.rstrip('\r').encode());time.sleep(.8);rc('send-key','enter')
            wait(lambda:len(agents())==2,'Codex launched in the second terminal was not detected')
            second=agents()[1];assert second!=first
            assert data(i,first)==before,'Second terminal input reached the first agent'
            rc('send-text','--stdin',data=b'SECOND_ONLY')
            wait(lambda:b'SECOND_ONLY' in data(i,second),'Second Codex session lost input')
            action('terminal:0');focus_cycle()
            rc('send-text','--stdin',data=b'FIRST_ONLY')
            wait(lambda:b'FIRST_ONLY' in data(i,first),'First agent input missing')
            assert b'FIRST_ONLY' not in data(i,second)
            # Every tab uses the same menu, regardless of its current process.
            rc('send-key','ctrl+escape');click(side+70,183,right=True)
            menu=wait(lambda:state().get('context_menu'),'Terminal menu missing')
            assert not any('agent' in item['label'].lower() for item in menu['items'])
            rc('send-key','down','enter');rename('Review / test')
            assert state()['project_state']['terminal_labels']['0']=='Review / test'
            focus_cycle();rc('send-text','--stdin',data=b'NEEDS_ATTENTION')
            wait(lambda:any(e['pane']==0 and (e.get('health') or {}).get('state')=='waiting_for_approval' for e in state()['sessions']),'Waiting agent status missing')
            rc('send-text','--stdin',data=b'WORKING_AGAIN')
            wait(lambda:state()['project_activity'][name]['working']==2,'Both shell agents were not tracked')
            action('terminal:split');wait(lambda:state()['project_state']['split'],'Split missing')
            focus_cycle();rc('send-text','--stdin',data=b'SPLIT_SELECTED_ONLY')
            wait(lambda:b'SPLIT_SELECTED_ONLY' in data(i,first),'Selected split terminal lost focus')
            assert b'SPLIT_SELECTED_ONLY' not in data(i,second)
            action('terminal:close-pane');wait(lambda:not state()['project_state']['split'],'Close pane failed')
            assert len(agents())==2
            action('terminal:0');focus_cycle();rc('send-key','ctrl+d')
            wait(lambda:any(e['pane']==0 and (e.get('health') or {}).get('alive') and e['health'].get('agent') is None for e in state()['sessions']),'Agent badge did not clear after Codex returned to its shell')
            assert next(e['health']['pid'] for e in state()['sessions'] if e['pane']==0)==shell
            rc('send-text','--stdin',data=command.rstrip('\r').encode());time.sleep(.8);rc('send-key','enter')
            wait(lambda:0 in agents() and agents()[0]!=first,'Agent was not rediscovered in the same shell')
            first=agents()[0]
            assert state()['project_state']['agents']==[], 'Detection created designated agent slots'
            assert all(e['cwd']==str(temp/str(i)) for e in records(i))
            identities[name]=dict(agents())
            shells[name]={e['pane']:e['health']['pid'] for e in state()['sessions']}
            screenshot('local-agents.png' if i==0 else 'ssh-agents.png')
        # Reopen an authentic legacy layout over the existing live terminals.
        action('project:'+names[0]);action('terminal:1')
        old=state()['controller_pid'];invoke(names[0],'--close');time.sleep(.6)
        layout=data_root/(names[0]+'.layout.json');legacy=json.loads(layout.read_text())
        legacy.pop('terminal_first',None);legacy.update(space='agent',agent=1,agents=[1],terminal=0,terminals=[0],agent_labels={'1':'Legacy conversation'})
        layout.write_text(json.dumps(legacy))
        for root in [runtime,data_root]:
            path=root/(names[0]+'.json');cfg=json.loads(path.read_text());cfg['agent']='python3 '+shlex.quote(str(fixture))+' '+shlex.quote(str(temp/'0-input.jsonl'));path.write_text(json.dumps(cfg))
        rc('launch','--type=tab',str(app),'--workspace',names[0])
        wait(lambda:state()['controller_pid']!=old and state()['project_state']['terminal_first'],'Legacy layout did not migrate')
        assert state()['project_state']['space']=='terminals' and state()['project_state']['terminal']==1
        for name in names:
            action('project:'+name);wait(lambda:state()['active_project']==name and len(agents())==2,'Retained agents missing')
            assert dict(agents())==identities[name],'Migration/reopen replaced a running agent'
            assert {e['pane']:e['health']['pid'] for e in state()['sessions']}==shells[name]
        result={'passed':True,'new_project_starts_in_one_shell':True,'no_designated_agent_controls':True,'shell_launched_agents_detected_in_any_tab':True,'independent_codex_sessions_in_same_directory':True,'working_and_waiting_status':True,'agent_exit_clears_badge_and_shell_survives':True,'agent_restart_in_same_shell_detected':True,'uppercase_and_modifiers':True,'window_focus_and_split_input':True,'terminal_rename_and_close_pane':True,'legacy_layout_migration_preserves_live_pids':True,'local_and_authenticated_ssh':True,'original_herdr_sessions_touched':False}
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    except BaseException:
        try:(out/'failure-state.json').write_text(json.dumps(state(),indent=2))
        except Exception:pass
        for path in temp.glob('*-input.jsonl'):
            (out/path.name).write_text(path.read_text())
        raise
    finally:
        for name in names:
            try:invoke(name,'--stop')
            except subprocess.SubprocessError:pass
        for child in reversed(children):
            if child.poll() is None:child.terminate();child.wait(timeout=8)
        if original:subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{original}"}}))'],stdout=subprocess.DEVNULL)
