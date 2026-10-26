#!/usr/bin/env python3
"""Direct terminal acceptance with real GPU Kitty frontend and private loopback SSH."""
import getpass
import hashlib
import json
import os
from pathlib import Path
import shlex
import socket
import subprocess
import tempfile
import time
import uuid

repo=Path(__file__).resolve().parents[1]
app=repo/'scripts/run-local.sh'
runtime=Path(os.environ['XDG_RUNTIME_DIR'])/'starmux'
out=repo/'target/direct-terminal-proof'
out.mkdir(parents=True,exist_ok=True)
local='direct-'+uuid.uuid4().hex[:8]
remote='direct-ssh-'+uuid.uuid4().hex[:8]

def wait(fn,label,timeout=30):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
        time.sleep(.1)
    raise AssertionError(label)
def invoke(name,*args):return subprocess.check_output([str(app),'--workspace',name,*args],stderr=subprocess.PIPE,timeout=20)
def state():return json.loads((runtime/(local+'.state.json')).read_text())
def action(value):invoke(local,'--action',value)
def log(path):return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
def send(*args,data=None):return subprocess.check_output(['kitten','@','--to',address,*args],input=data,stderr=subprocess.PIPE,timeout=15)
def click():
    send('send-key','ctrl+escape')
    v=state()['viewport'];x=int(v['width']*.4);y=400
    send('send-text','--stdin',data=f'\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m'.encode())
def screenshot(name):
    c=next(c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['pid']==kitty.pid)
    x,y=c['at'];w,h=c['size']
    subprocess.run(['grim','-g',f'{x},{y} {w}x{h}',str(out/name)],check=True)

with tempfile.TemporaryDirectory(prefix='starmux-direct-') as tmp:
    tmp=Path(tmp);kitty=None;daemon=None
    try:
        for name in ['host','client']:subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(tmp/name)],check=True)
        (tmp/'authorized_keys').write_text((tmp/'client.pub').read_text())
        with socket.socket() as probe:probe.bind(('127.0.0.1',0));port=probe.getsockname()[1]
        (tmp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(tmp/'host.pub').read_text())
        (tmp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM yes\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
        sshconfig=tmp/'ssh_config';sshconfig.write_text(f'Host direct-host\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {tmp}/client\n IdentitiesOnly yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {tmp}/known_hosts\n')
        daemon=subprocess.Popen(['/usr/bin/sshd','-D','-e','-f',str(tmp/'sshd_config')],stderr=(out/'sshd.log').open('w'),env={k:v for k,v in os.environ.items() if k!='NOTIFY_SOCKET'})
        wait(lambda:subprocess.run(['ssh','-F',str(sshconfig),'direct-host','true'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0,'SSH unavailable')
        paths={local:tmp/'local.jsonl',remote:tmp/'remote.jsonl'}
        for name in [local,remote]:
            config={'workspace':name,'collection':local,'theme':'kanagawa','host':'direct-host' if name==remote else None,'ssh_config':str(sshconfig) if name==remote else None,'directory':str(tmp),'agent':shlex.join(['python3',str(repo/'scripts/direct-terminal-fixture.py'),str(paths[name])]),'executable':str(repo/'target/release/starmux'),'project_host':True,'desktop_helper':str(app)}
            (runtime/(name+'.json')).write_text(json.dumps(config));os.chmod(runtime/(name+'.json'),0o600)
        address='unix:'+str(tmp/'kitty.sock')
        def launch():return subprocess.Popen(['/usr/bin/kitty','--hold','--class','starmux-direct-acceptance','--config','NONE','--listen-on',address,'-o','allow_remote_control=yes',str(app),'--workspace',local],env=dict(os.environ,STAR_GRAPHICS_RENDERER='gpu',STAR_TERMINAL_RENDERER='direct'),stderr=(out/'kitty.log').open('a'),stdout=subprocess.DEVNULL)
        kitty=launch()
        checks={};backend_pids={}
        for name in [local,remote]:
            if name!=local:action('project:'+name)
            s=wait(lambda:state() if state().get('active_project')==name and any('Direct terminal menu' in t['text'] for t in state().get('direct_terminals',[])) else None,'Terminal did not render')
            assert s['terminal_renderer']=='direct' and s['desktop']['windows']==0
            wait(lambda:len(state()['sessions'])==2 and all(x['health'] and x['health'].get('pid') for x in state()['sessions']),'Missing persistent session IDs')
            backend_pids[name]={str(x['pane']):x['health']['pid'] for x in state()['sessions']}
            if name==local:
                expected_zoom=int(state()['terminal_typography']['millipixels']/1000+.5)+1
                send('send-key','ctrl+escape')
                v=state()['viewport'];x=v['width']-184;y=149
                send('send-text','--stdin',data=f'\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m'.encode())
                wait(lambda:state()['project_state']['terminal_font_size']==expected_zoom,'Header font increase failed')
            else:
                assert state()['project_state']['terminal_font_size']==0,'Font leaked to another project'
            screenshot('ssh.png' if name==remote else 'local.png')
            click();send('send-key','down','enter')
            wait(lambda:any(x['kind']=='submitted' and x['value']==1 for x in log(paths[name])),'Menu choice failed')
            before=len([x for x in log(paths[name]) if x['kind']=='submitted'])
            send('send-text','--stdin','--bracketed-paste','enable',data='café 世界'.encode())
            wait(lambda:any(x['kind']=='paste' and x['value']=='café 世界' for x in log(paths[name])),'Unicode paste failed')
            assert len([x for x in log(paths[name]) if x['kind']=='submitted'])==before
            file=tmp/'agents résumé.md';file.write_text('native terminal upload\n')
            invoke(name,'--deliver',str(file),'--pane','2')
            entry=wait(lambda:next((x for x in reversed(log(paths[name])) if x['kind']=='paste' and 'agents résumé.md' in x['value']),None),'File destination not pasted')
            staged=Path(shlex.split(entry['value'])[0]);assert staged.read_bytes()==file.read_bytes()
            assert len([x for x in log(paths[name]) if x['kind']=='submitted'])==before,'Delivery submitted a command'
            checks[name]={'arrows_and_enter':True,'unicode_bracketed_paste':True,'staged_file_without_enter':True,'sha256':hashlib.sha256(staged.read_bytes()).hexdigest()}
        action('project:'+local)
        action('terminal:split')
        wait(lambda:state()['active_project']==local and state()['project_state']['split'] and len(state()['direct_terminals'])==3,'Split terminals failed')
        screenshot('split.png')
        action('view:agent');wait(lambda:state()['project_state']['space']=='agent','Agent tab failed')
        # Exercise changing host geometry without resampling text or restarting the server.
        for size in ['14','12']:
            send('set-font-size',size);time.sleep(.5)
        old_controller=state()['controller_pid']
        invoke(local,'--close');send('close-window');kitty.wait(timeout=10)
        time.sleep(.3);kitty=launch()
        wait(lambda:state().get('terminal_renderer')=='direct' and state().get('controller_pid') not in [None,old_controller] and any('Direct terminal menu' in t['text'] for t in state().get('direct_terminals',[])),'Reattach failed')
        for name in [local,remote]:
            action('project:'+name)
            wait(lambda:state()['active_project']==name and all(x['health'] and x['health'].get('pid') for x in state()['sessions']),'Session return failed')
            current={str(x['pane']):x['health']['pid'] for x in state()['sessions']}
            assert all(current[pane]==pid for pane,pid in backend_pids[name].items()),(name,current,backend_pids[name])
        action('project:'+local)
        wait(lambda:state()['active_project']==local,'Local return failed')
        assert state()['project_state']['terminal_font_size']==expected_zoom,'Font preference did not survive reconnect'
        result={'header_font_controls':True,'font_preference_persists':True,'renderer':'direct','inner_kitty_windows':0,'terminal_compositor_windows':0,'transport':'local and authenticated loopback SSH','checks':checks,'split':True,'font_resize':True,'persistent_backend_pids':backend_pids,'reattach_preserves_sessions':True}
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
    finally:
        for name in [local,remote]:
            try:invoke(name,'--stop')
            except subprocess.SubprocessError:pass
        if kitty is not None:
            try:send('close-window')
            except subprocess.SubprocessError:kitty.terminate()
            kitty.wait(timeout=10)
        if daemon is not None:daemon.terminate();daemon.wait(timeout=10)
