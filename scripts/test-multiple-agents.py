#!/usr/bin/env python3
"""Real GPU terminal case handling and independent local/SSH agent sessions.

Only disposable projects and a private authenticated loopback SSH server are used.
"""
import getpass
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import uuid

repo = Path(__file__).resolve().parents[1]
app = repo/'scripts/run-local.sh'
runtime = Path(os.environ['XDG_RUNTIME_DIR'])/'starmux'
data_root = Path.home()/'.local/share/starmux/projects'
out = repo/'target'/os.environ.get('STARMUX_AGENT_PROOF','multi-agent-proof')
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


with tempfile.TemporaryDirectory(prefix='starmux-tools-') as temp:
    temp = Path(temp)
    address = 'unix:'+str(temp/'kitty.sock')
    def rc(*args, data=None):
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
        fixture=temp/'agent.py'
        fixture.write_text('''import json,os,sys,termios,tty
old=termios.tcgetattr(0);tty.setraw(0)
def record(value):
 with open(sys.argv[1],'a') as f:f.write(json.dumps(dict(pid=os.getpid(),cwd=os.getcwd(),**value))+'\\n')
record({'kind':'ready'})
print('Agent input fixture '+str(os.getpid())+'\\r',flush=True)
try:
 while True:
  chunk=os.read(0,4096)
  if not chunk:break
  record({'kind':'input','hex':chunk.hex()})
finally:termios.tcsetattr(0,termios.TCSANOW,old)
''')
        for i,name in enumerate(names):
            folder=temp/str(i);folder.mkdir()
            cfg={'workspace':name,'collection':prefix,'label':'Local agents' if i==0 else 'SSH agents','theme':'kanagawa','host':None if i==0 else 'tools-host','ssh_config':None if i==0 else str(sshconfig),'directory':str(folder),'agent':f'python3 {fixture} {temp}/{i}-input.jsonl','executable':str(repo/'target/release/starmux'),'project_host':True,'desktop_helper':str(app)}
            path=runtime/(name+'.json');path.write_text(json.dumps(cfg));path.chmod(0o600)
        kitty=subprocess.Popen(['/usr/bin/kitty','--hold','--class','starmux-agents-proof','--listen-on',address,'-o','allow_remote_control=yes',str(app),'--workspace',names[0]],env=dict(os.environ,STAR_GRAPHICS_RENDERER='gpu',STAR_TERMINAL_RENDERER=os.environ.get('STAR_TERMINAL_RENDERER','kitty')),stdout=subprocess.DEVNULL,stderr=(out/'kitty.log').open('w'))
        children.append(kitty)
        def records(i):
            path=temp/f'{i}-input.jsonl'
            return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        def data(i,pid):
            return b''.join(bytes.fromhex(e['hex']) for e in records(i) if e['pid']==pid and e['kind']=='input')
        def agents():
            return {e['pane']:e['health']['pid'] for e in state()['sessions'] if e['agent'] and e.get('health',{}).get('pid')}
        identities={}
        for i,name in enumerate(names):
            if i:action('project:'+name)
            s=wait(lambda:state() if state()['active_project']==name and len(agents())==1 and (state().get('desktop',{}).get('project_windows') or state().get('direct_terminals')) else None,'Initial agent missing')
            first=agents()[1]
            wait(lambda:any(e['pid']==first for e in records(i)),'Agent fixture not ready')
            side=s['project_navigation']['sidebar_width']
            rc('send-key','ctrl+escape');click(side+80,250)
            payload=b'Case:aAzZ!@#$%^&*()_+{}:<>?END'
            rc('send-text','--stdin',data=payload)
            wait(lambda:payload in data(i,first),'Uppercase or punctuation bytes changed')
            rc('send-key','shift+a','shift+z','shift+1')
            wait(lambda:b'AZ!' in data(i,first),'Explicit Shift key events lost their level')
            rc('send-key','ctrl+a','alt+x','ctrl+c')
            wait(lambda:b'\x01\x1bx\x03' in data(i,first),'Control/Alt shortcuts changed')
            # An uppercase letter must not leave synthetic Shift stuck for later text.
            rc('send-text','--stdin',data=b'Abc_123z')
            wait(lambda:b'Abc_123z' in data(i,first),'Shift remained stuck after uppercase input')
            before=data(i,first)
            rc('send-key','ctrl+escape');click(s['viewport']['width']-65,183)
            s=wait(lambda:state() if len(agents())==2 and state()['project_state']['agent']!=1 else None,'+ Agent did not create another agent')
            second_id=s['project_state']['agent'];second=agents()[second_id]
            assert second!=first and s['project_state']['agents']==[1,second_id]
            wait(lambda:any(e['pid']==second for e in records(i)),'Second agent fixture not ready')
            assert all(e['cwd']==str(temp/str(i)) for e in records(i))
            click(side+80,250);rc('send-text','--stdin',data=b'SECOND_ONLY')
            wait(lambda:b'SECOND_ONLY' in data(i,second),'Second agent did not receive input')
            assert data(i,first)==before,'New agent input reached the original session'
            action('agent:1');wait(lambda:state()['project_state']['agent']==1,'First agent could not be selected')
            rc('send-key','ctrl+escape');click(side+80,250);rc('send-text','--stdin',data=b'FIRST_ONLY')
            wait(lambda:b'FIRST_ONLY' in data(i,first),'First agent input missing')
            assert b'FIRST_ONLY' not in data(i,second)
            action('agent:'+str(second_id));wait(lambda:state()['project_state']['agent']==second_id,'Second agent could not be selected')
            # Rename the second agent using its context menu.
            rc('send-key','ctrl+escape');click(side+210,183,right=True)
            wait(lambda:state().get('context_menu'),'Agent context menu missing')
            rc('send-key','down','enter');rename('Review / test')
            wait(lambda:state()['project_state']['agent_labels'].get(str(second_id))=='Review / test','Agent label missing')
            action('terminal:new')
            s=wait(lambda:state() if len(state()['project_state']['terminals'])==2 else None,'Terminal creation failed')
            assert not set(s['project_state']['terminals']) & set(s['project_state']['agents'])
            action('view:agent');wait(lambda:state()['project_state']['space']=='agent','Agent view missing')
            identities[name]=dict(agents());assert len(identities[name])==2
            screenshot('local-agents.png' if i==0 else 'ssh-agents.png')
        controller=state()['controller_pid'];invoke(names[0],'--close');time.sleep(.5)
        rc('launch','--type=tab',str(app),'--workspace',names[0])
        wait(lambda:state()['controller_pid']!=controller,'Controller did not reopen')
        for i,name in enumerate(names):
            action('project:'+name)
            wait(lambda:state()['active_project']==name and agents()==identities[name],'Agent processes changed after restart')
            assert state()['project_state']['agent_labels']['2']=='Review / test'
            assert {e['pid'] for e in records(i) if e['kind']=='ready'}==set(identities[name].values())
        result={'passed':True,'terminal_renderer':state()['terminal_renderer'],'uppercase_and_shifted_punctuation':True,'control_alt_shortcuts':True,'synthetic_shift_released':True,'two_independent_agents_per_project':True,'same_project_directory':True,'agent_context_menu_and_rename':True,'session_ids_and_pids_survive_restart':True,'local_and_ssh_agent_pids':identities,'ssh_scope':'authenticated loopback SSH','original_herdr_sessions_touched':False}
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
