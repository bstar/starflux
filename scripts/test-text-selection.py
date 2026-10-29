#!/usr/bin/env python3
"""Real GPU text selection and click routing with a mouse-capturing agent TUI.

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
runtime = Path(os.environ['XDG_RUNTIME_DIR'])/'starflux'
data_root = Path.home()/'.local/share/starflux/projects'
out = repo/'target/selection-proof'
out.mkdir(parents=True, exist_ok=True)
prefix = 'select-'+uuid.uuid4().hex[:7]
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


clipboard_type=None;clipboard_data=None
try:
    types=subprocess.check_output(['wl-paste','--list-types'],stderr=subprocess.DEVNULL,timeout=3).decode().splitlines()
    clipboard_type=next((t for t in ['image/png','text/plain;charset=utf-8','text/plain'] if t in types),types[0] if types else None)
    if clipboard_type:clipboard_data=subprocess.check_output(['wl-paste','--type',clipboard_type,'--no-newline'],stderr=subprocess.DEVNULL,timeout=3)
except subprocess.SubprocessError:pass

with tempfile.TemporaryDirectory(prefix='starflux-tools-') as temp:
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
        fixture=temp/'agent.py';token='SELECT_CODEX_café_TEXT'
        fixture.write_text('''import json,os,sys,tty,termios
old=termios.tcgetattr(0);tty.setraw(0)
os.write(1,('\\x1b[?1049h\\x1b[?25l\\x1b[?1002h\\x1b[?1006h\\x1b[>11u\\x1b[0m\\x1b[2J\\x1b[3;1H\\x1b[38;2;255;255;255m'+TOKEN+'\\x1b[7;1H\\x1b[34mClick target for agent menu test').encode())
try:
 while True:
  chunk=os.read(0,4096)
  if not chunk:break
  with open(sys.argv[1],'a') as f:f.write(json.dumps({'pid':os.getpid(),'hex':chunk.hex()})+'\\n')
finally:
 os.write(1,b'\\x1b[<u\\x1b[?1002l\\x1b[?1006l\\x1b[?1049l');termios.tcsetattr(0,termios.TCSANOW,old)
'''.replace('TOKEN',repr(token)))
        for i,name in enumerate(names):
            cfg={'workspace':name,'collection':prefix,'theme':'kanagawa','host':None if i==0 else 'tools-host','ssh_config':None if i==0 else str(sshconfig),'directory':str(temp),'agent':f'python3 {fixture} {temp}/{i}-input.jsonl','executable':str(repo/'target/release/starflux'),'project_host':True,'desktop_helper':str(app)}
            path=runtime/(name+'.json');path.write_text(json.dumps(cfg));path.chmod(0o600)
        kitty=subprocess.Popen(['/usr/bin/kitty','--hold','--class','starflux-selection-proof','--listen-on',address,'-o','allow_remote_control=yes',str(app),'--workspace',names[0]],env=dict(os.environ,STAR_GRAPHICS_RENDERER='gpu',STAR_TERMINAL_RENDERER='kitty'),stdout=subprocess.DEVNULL,stderr=(out/'kitty.log').open('w'))
        children.append(kitty)
        def inputs(i):
            path=temp/f'{i}-input.jsonl'
            return b''.join(bytes.fromhex(json.loads(line)['hex']) for line in path.read_text().splitlines()) if path.exists() else b''
        def pointer(action,x,y):
            code=32 if action=='drag' else 0;end='m' if action=='up' else 'M'
            rc('send-text','--stdin',data=f'\x1b[<{code};{round(x)};{round(y)}{end}'.encode())
        results=[]
        for i,name in enumerate(names):
            if i:action('project:'+name)
            s=wait(lambda:state() if state()['active_project']==name and state().get('desktop',{}).get('project_windows') and all(p.get('pid') for p in state()['panes']) else None,'Agent pane missing')
            inner='unix:'+str(runtime/f'kitty-{name}-1.sock')
            def inner_text(*args):return subprocess.check_output(['kitten','@','--to',inner,'get-text',*args],stderr=subprocess.PIPE,timeout=10)
            wait(lambda:token.encode() in inner_text(),'Agent specimen absent')
            rc('send-key','ctrl+escape');screenshot(f'{i}-before.png')
            path=out/f'{i}-before.png';iw,ih=map(int,subprocess.check_output(['magick','identify','-format','%w %h',str(path)]).split())
            raw=subprocess.check_output(['magick',str(path),'-depth','8','rgb:-'])
            side=s['project_navigation']['sidebar_width']
            # Find the white specimen ink, excluding all header and chrome text.
            points=[]
            for y in range(round(ih*.08),round(ih*.55)):
                for x in range(round(iw*.15),iw):
                    r,g,b=raw[(y*iw+x)*3:(y*iw+x)*3+3]
                    if min(r,g,b)>210 and max(r,g,b)-min(r,g,b)<=1:points.append((x,y))
            assert points,'Specimen ink absent'
            left=min(x for x,y in points);right=max(x for x,y in points)
            top=min(y for x,y in points);bottom=max(y for x,y in points)
            # Outer terminal pixels exclude its small native window border/padding.
            c=next(c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['pid']==kitty.pid)
            sw,sh=c['size'];v=s['viewport'];padx=(sw-v['width'])/2;pady=(sh-v['height'])/2
            # Stay inside the first cell: Kitty 0.48 ignores a press just
            # outside its text grid, even when white glyph ink touches its edge.
            sx=left*sw/iw-padx+2;ex=(right+2)*sw/iw-padx;ty=(top+bottom)/2*sh/ih-pady
            before=inputs(i)
            pointer('down',sx,ty)
            for step in range(1,9):pointer('drag',sx+(ex-sx)*step/8,ty)
            pointer('up',ex,ty)
            selected=wait(lambda:inner_text('--extent','selection').decode().rstrip('\n') or None,'Inner Kitty did not select text')
            assert selected==token,(selected,token)
            copied=wait(lambda:subprocess.check_output(['wl-paste','--no-newline'],stderr=subprocess.DEVNULL).decode() if subprocess.check_output(['wl-paste','--no-newline'],stderr=subprocess.DEVNULL).decode()==token else None,'Desktop clipboard did not receive selected text')
            assert inputs(i)==before,'A selection drag reached the agent as a mouse click'
            screenshot(f'{i}-selected.png')
            # A normal click must still reach the mouse-capturing application.
            pointer('down',sx+25,ty+55);pointer('up',sx+25,ty+55)
            wait(lambda:b'\x1b[<0;' in inputs(i)[len(before):],'Agent menu click was swallowed')
            assert state()['controller_pid']==s['controller_pid']
            action('terminal:split')
            split=wait(lambda:state() if state()['project_state']['split'] and all((e.get('health') or {}).get('pid') for e in state()['sessions']) else None,'Split shells not ready')
            tabs=split['project_state']['terminals'];closed=split['active']
            pids={e['pane']:e['health']['pid'] for e in split['sessions']}
            action('terminal:close-pane')
            wait(lambda:not state()['project_state']['split'],'Close pane did not collapse split')
            assert state()['active']!=closed
            assert state()['project_state']['terminals']==tabs,'Close pane removed a retained tab'
            assert {e['pane']:e['health']['pid'] for e in state()['sessions']}==pids,'Close pane ended a session'
            action('terminal:'+str(closed))
            wait(lambda:state()['active']==closed,'Closed pane tab could not be reopened')
            action('view:agent')
            results.append({'host':'local' if i==0 else 'authenticated loopback SSH','selected':selected,'clipboard':copied,'clicks_preserved':True,'close_split_pane_retains_tabs_and_processes':True})
        result={'passed':True,'plain_drag_selects_inside_mouse_capturing_agent':True,'copied_to_local_desktop_clipboard':True,'agent_clicks_preserved':True,'cases':results}
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    except BaseException:
        try:(out/'failure-state.json').write_text(json.dumps(state(),indent=2))
        except Exception:pass
        for path in temp.glob('*-input.jsonl'):(out/path.name).write_text(path.read_text())
        raise
    finally:
        for name in names:
            try:invoke(name,'--stop')
            except subprocess.SubprocessError:pass
        for child in reversed(children):
            if child.poll() is None:child.terminate();child.wait(timeout=8)
        if clipboard_type and clipboard_data is not None:subprocess.run(['wl-copy','--type',clipboard_type],input=clipboard_data,check=True)
        elif clipboard_type is None:subprocess.run(['wl-copy','--clear'],check=True)
        if original:subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{original}"}}))'],stdout=subprocess.DEVNULL)
