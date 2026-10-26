#!/usr/bin/env python3
"""Real GPU context menus, durable renames and local/SSH Git inspection.

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
out = repo/'target/project-tools-proof'
out.mkdir(parents=True, exist_ok=True)
prefix = 'tools-'+uuid.uuid4().hex[:7]
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
        for i, name in enumerate(names):
            root=temp/str(i);root.mkdir()
            def git(*args):
                return subprocess.run(['git','-C',str(root),*args],check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
            git('init','-b', 'agent/local' if i==0 else 'agent/ssh')
            git('config','user.name','Tools proof');git('config','user.email','tools@example.invalid')
            (root/'tracked.txt').write_text('base\n');git('add','.');git('commit','-m','Project baseline')
            if i==0:
                (root/'tracked.txt').write_text('base\nstaged\n');git('add','tracked.txt')
                (root/'tracked.txt').write_text('base\nstaged\nworking\n')
            (root/'new.txt').write_text('new\n')
            cfg={'workspace':name,'collection':prefix,'label':'Local project' if i==0 else 'SSH project','theme':'kanagawa','host':None if i==0 else 'tools-host','ssh_config':None if i==0 else str(sshconfig),'directory':str(root),'agent':f'python3 {repo}/scripts/direct-terminal-fixture.py {temp}/{i}-input.jsonl','executable':str(repo/'target/release/starmux'),'project_host':True,'desktop_helper':str(app)}
            path=runtime/(name+'.json');path.write_text(json.dumps(cfg));path.chmod(0o600)
        kitty=subprocess.Popen(['/usr/bin/kitty','--hold','--class','starmux-tools-proof','--listen-on',address,'-o','allow_remote_control=yes',str(app),'--workspace',names[0]],env=dict(os.environ,STAR_GRAPHICS_RENDERER='gpu',STAR_TERMINAL_RENDERER='kitty'),stdout=subprocess.DEVNULL,stderr=(out/'kitty.log').open('w'))
        children.append(kitty)
        s=wait(lambda: state() if state().get('git',{}).get('branch')=='agent/local' and state().get('desktop',{}).get('project_windows') and all(p.get('pid') for p in state()['panes']) else None,'Local project unavailable')
        assert s['git']['staged']==1 and s['git']['unstaged']==1 and s['git']['untracked']==1
        pids=[p['pid'] for p in s['panes']]
        # Right-clicking another project must not activate it.
        click(70,143,right=True)
        menu=wait(lambda:state().get('context_menu'),'Project context menu missing')
        assert state()['active_project']==names[0] and menu['title']=='SSH project'
        screenshot('project-menu.png')
        rc('send-key','down','enter');rename('Remote / test rig')
        assert state()['active_project']==names[0]
        assert json.loads((data_root/(names[1]+'.json')).read_text())['label']=='Remote / test rig'
        click(70,108,right=True);wait(lambda:state().get('context_menu'),'Active project menu missing')
        rc('send-key','down','enter');rename('Agent workshop')
        wait(lambda:state()['project_label']=='Agent workshop','Project name not updated')
        # Outside click dismisses without switching to that project underneath.
        click(70,108,right=True);wait(lambda:state().get('context_menu'),'Dismiss test menu missing')
        click(20,143);wait(lambda:state().get('context_menu') is None,'Menu did not dismiss')
        assert state()['active_project']==names[0]
        action('view:terminals')
        s=wait(lambda:state() if state()['project_state']['space']=='terminals' else None,'Terminal view missing')
        side=s['project_navigation']['sidebar_width']
        click(side+40,183,right=True)
        menu=wait(lambda:state().get('context_menu'),'Tab context menu missing')
        assert menu['title']=='Shell'
        screenshot('tab-menu.png')
        # Mouse selection, independent of keyboard menu selection.
        b=menu['bounds'];click(b['x']+35,b['y']+79)
        rename('Build / café')
        wait(lambda:state()['project_state']['terminal_labels'].get('0')=='Build / café','Tab name not persisted')
        assert [p['pid'] for p in state()['panes']]==pids
        click(side+40,183,right=True);wait(lambda:state().get('context_menu'),'Renamed tab menu missing')
        rc('send-key','down','down','down','enter')
        wait(lambda:state()['project_state']['split'],'Context split failed')
        # The header status opens the detailed inspector and blocks terminal keys.
        action('view:agent');wait(lambda:state()['project_state']['space']=='agent','Agent view missing')
        rc('send-key','ctrl+escape');click(side+80,67)
        wait(lambda:state()['git_open'],'Git inspector did not open')
        screenshot('git-inspector.png')
        rc('send-key','escape');wait(lambda:not state()['git_open'],'Header inspector did not close')
        click(side+80,235);rc('send-key','x')
        wait(lambda:(temp/'0-input.jsonl').exists() and any(e.get('kind')=='byte' and e.get('value')=='x' for e in map(json.loads,(temp/'0-input.jsonl').read_text().splitlines())), 'Positive control: fixture did not receive terminal key')
        action('git:details');wait(lambda:state()['git_open'],'Inspector action did not open')
        before=(temp/'0-input.jsonl').read_text()
        rc('send-key','x','down','r');time.sleep(.4)
        assert ((temp/'0-input.jsonl').read_text() if (temp/'0-input.jsonl').exists() else '')==before,'Inspector leaked input to agent'
        rc('send-key','escape');wait(lambda:not state()['git_open'],'Inspector did not close')
        # Background refresh notices ongoing work without navigating away.
        (temp/'0/new-second.txt').write_text('second\n')
        wait(lambda:state()['git']['untracked']==2,'Git did not refresh')
        action('project:'+names[1])
        s=wait(lambda:state() if state()['active_project']==names[1] and state().get('git',{}).get('branch')=='agent/ssh' else None,'SSH Git status missing')
        assert s['git']['staged']==0 and s['git']['unstaged']==0 and s['git']['untracked']==1
        assert s['project_label']=='Remote / test rig'
        action('git:details');wait(lambda:state()['git_open'],'SSH Git inspector missing');screenshot('ssh-git-inspector.png')
        action('git:close');action('project:'+names[0])
        wait(lambda:state()['active_project']==names[0],'Local project return failed')
        assert state()['git']['branch']=='agent/local'
        # Close and reopen only the disposable controller; labels and processes survive.
        controller=state()['controller_pid'];invoke(names[0],'--close');time.sleep(.5)
        rc('launch','--type=tab',str(app),'--workspace',names[0])
        s=wait(lambda:state() if state()['controller_pid']!=controller and state().get('desktop',{}).get('project_windows') else None,'Reopen failed')
        assert s['project_label']=='Agent workshop'
        assert s['project_state']['terminal_labels']['0']=='Build / café'
        assert [p['pid'] for p in s['panes']]==pids
        screenshot('workspace.png')
        result={'passed':True,'project_and_tab_context_menus':True,'mouse_and_keyboard_selection':True,'outside_click_consumed':True,'unicode_labels_persist_across_restart':True,'session_pids_preserved':pids,'local_and_ssh_git':True,'automatic_refresh':True,'inspector_input_isolated':True,'ssh_scope':'authenticated loopback SSH','original_herdr_sessions_touched':False}
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    except BaseException:
        try:(out/'failure-state.json').write_text(json.dumps(state(),indent=2))
        except Exception:pass
        raise
    finally:
        for name in names:
            try:invoke(name,'--stop')
            except subprocess.SubprocessError:pass
        for child in reversed(children):
            if child.poll() is None:child.terminate();child.wait(timeout=8)
        if original:subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{original}"}}))'],stdout=subprocess.DEVNULL)
