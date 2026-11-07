#!/usr/bin/env python3
"""Real Kitty font changes in a disposable process with private fake terminals.

--ssh exercises the terminal-owned Rust helper over a private SSH TTY/bridge.
No user agents, Kitty configuration, or existing Kitty windows are changed.
"""
import argparse, getpass, json, os, pathlib, secrets, shlex, shutil, socket, subprocess, tempfile, time
from kitty_font_bridge import augment
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--ssh',action='store_true');p.add_argument('--reload',action='store_true',help='Upgrade watcher callbacks in the existing Kitty window');args=p.parse_args()
repo=pathlib.Path(__file__).resolve().parents[1];app=repo/'scripts/run-local.sh'
kit=next((pathlib.Path.home()/'.cargo/git/checkouts').glob('starkit-*/f60a8fb/integrations/kitty/star_kit.py'))

def wait(fn,label,timeout=30):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
        time.sleep(.05)
    raise AssertionError(label)

with tempfile.TemporaryDirectory(prefix='starflux-font-proof-') as td:
    tmp=pathlib.Path(td);runtime=tmp/'runtime';runtime.mkdir(mode=0o700);state=runtime/'starflux';state.mkdir(mode=0o700)
    name='font-'+secrets.token_hex(5);shell=tmp/'shell';shell.write_text('#!/bin/sh\nprintf "FONT_RENDER_PROOF\\r\\n"\nexec cat\n');shell.chmod(0o700)
    (state/(name+'.json')).write_text(json.dumps(dict(workspace=name,collection=name,host=None,directory=str(tmp),agent=None,executable=str(app),project_host=True)))
    env={**os.environ,'XDG_RUNTIME_DIR':str(runtime),'XDG_CONFIG_HOME':str(tmp/'config'),'XDG_DATA_HOME':str(tmp/'data'),'SHELL':str(shell),'STAR_TERMINAL_RENDERER':'native'}
    # Kitty itself uses the real compositor runtime; only its child gets fixture paths.
    child_env=['env','-u','TMUX','-u','TMUX_PANE','-u','STY','TERM_PROGRAM=kitty','STAR_GRAPHICS_RENDERER=cpu',*[k+'='+env[k] for k in ['XDG_RUNTIME_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','SHELL','STAR_TERMINAL_RENDERER','PATH']],str(app),'--workspace',name,'--graphical']
    daemon=None;kitty=None
    address='unix:'+str(tmp/'kitty.sock');log=(tmp/'kitty.log').open('w')
    try:
        font_watcher=tmp/'starflux_fonts_initial.py'
        font_watcher.write_text((repo/'terminal-client/kitty/starflux_fonts.py').read_text().replace('def reply(window, value):', 'def reply(window, value):\n    from pathlib import Path\n    Path('+repr(str(tmp/'font-reply.json'))+').write_text(json.dumps(value))'))
        watchers=[font_watcher]
        if args.ssh:
            for key in ['host','client']:subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(tmp/key)],check=True)
            (tmp/'authorized_keys').write_bytes((tmp/'client.pub').read_bytes())
            with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
            (tmp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(tmp/'host.pub').read_text())
            config=tmp/'sshd_config';config.write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
            daemon=subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd','-D','-e','-f',str(config)],stderr=(tmp/'sshd.log').open('w'),env={k:v for k,v in os.environ.items() if k!='NOTIFY_SOCKET'})
            def listening():
                with socket.socket() as s:return s.connect_ex(('127.0.0.1',port))==0
            wait(listening,'Private sshd did not start')
            ssh=['ssh','-F','/dev/null','-tt','-p',str(port),'-i',str(tmp/'client'),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(tmp/'known_hosts'),'127.0.0.1']
            child_env=ssh+[shlex.join(child_env)]
            watcher=tmp/'star_kit_initial.py';watcher.write_text(augment(kit.read_text()).replace("STARFLUX_CLIENT = Path.home() / '.local/bin/starflux-terminal-client'", 'STARFLUX_CLIENT = Path('+repr(str(repo/'target/release/starflux-terminal-client'))+')').replace("CLIENT = Path.home() / '.local/bin/star-kit-terminal'", "CLIENT = Path('/nonexistent-fixture-shared-client')"))
            watchers.append(watcher)
        options=[]
        for watcher in watchers:options+=['-o','watcher='+str(watcher)]
        launcher=tmp/'launch.py'
        launcher.write_text('import subprocess,pathlib,time\np=pathlib.Path('+repr(str(tmp))+')\nr=subprocess.run('+repr(child_env)+',stderr=(p/"child.log").open("w"))\n(p/"exit").write_text(str(r.returncode))\ntime.sleep(45)\n')
        child_env=['python3',str(launcher)]
        kitty=subprocess.Popen(['/usr/bin/kitty','--config','NONE','--class','starflux-font-proof','--listen-on',address,'-o','allow_remote_control=yes','-o','font_family=JetBrainsMono Nerd Font','-o','font_size=12.5',*options,*child_env],env={**{k:v for k,v in os.environ.items() if k not in ('TMUX','TMUX_PANE','STY','TERM_PROGRAM','TERM_PROGRAM_VERSION')},'STARFLUX_FONT_PROOF':'1','STAR_GRAPHICS_RENDERER':'cpu'},stdout=log,stderr=log)
        def snapshot():
            if kitty.poll() is not None: raise RuntimeError('Kitty exited: '+str(kitty.returncode))
            for f in state.glob('*.state.json'):
                d=json.loads(f.read_text())
                if d.get('frontend') and d.get('terminal_font',{}).get('controls_available'):return d
            return {}
        s=wait(snapshot,'Font metadata missing; '+str(tmp/'kitty.log'))
        frontend=s['frontend'];snapfile=state/(frontend+'.state.json')
        def current():return json.loads(snapfile.read_text())
        def action(value):subprocess.run([str(app),'--workspace',name,'--frontend',frontend,'--action',value],env=env,check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        def rc(*cmd):return subprocess.check_output(['kitten','@','--to',address,*cmd],stderr=subprocess.PIPE,timeout=10)
        def window():return json.loads(rc('ls'))[0]
        def font_size():return current()['terminal_font']['millipoints']/1000
        def terminal_grid():return [(w['columns'],w['lines']) for tab in window()['tabs'] for w in tab['windows']][-1]
        owners=[(v.get('pane'),v.get('pid')) for v in s['panes']]
        if args.reload:
            replacements=[]
            for index, original in enumerate(watchers):
                replacement=tmp/('starflux_fonts_reload.py' if index==0 else 'star_kit_reload.py')
                replacement.write_text(original.read_text()+'\n# Private hot-reload proof\n')
                replacements.append(replacement)
            updated=tmp/'reload.conf'
            updated.write_text('font_family JetBrainsMono Nerd Font\nfont_size 12.5\nallow_remote_control yes\n'+''.join('watcher '+str(file)+'\n' for file in replacements))
            rc('load-config','--ignore-overrides',str(updated))
        before=terminal_grid();assert font_size()==12.5,font_size()
        action('terminal:font:increase');wait(lambda:font_size()==13.5,'Plus did not change the real Kitty font')
        wait(lambda:current()['terminal_font']['millipoints']==13500,'Plus was not acknowledged')
        bigger=terminal_grid();assert bigger[0]<before[0] and bigger[1]<before[1],(before,bigger)
        action('terminal:font:decrease');wait(lambda:font_size()==12.5,'Minus did not change the real Kitty font')
        action('terminal:font:increase');wait(lambda:font_size()==13.5,'Second plus did not apply')
        action('terminal:font:reset');wait(lambda:font_size()==12.5,'Reset did not restore the fractional initial size')
        action('terminal:font:select');s=wait(lambda:current() if (current().get('context_menu') or {}).get('title')=='Monospace fonts' else None,'Font picker did not open')
        font=next(v for v in s['context_menu']['items'] if v['label']=='Liberation Mono')
        action(font['action']);wait(lambda:current()['terminal_font']['family']=='Liberation Mono','Font family did not change')
        assert font_size()==12.5,'Changing family lost the window size'
        assert [(v.get('pane'),v.get('pid')) for v in current()['panes']]==owners,'Font controls replaced a terminal process'
        print(json.dumps(dict(result='PASS',ssh=args.ssh,hot_reload=args.reload,real_size_changes=[12.5,13.5,12.5],grid_before=before,grid_enlarged=bigger,family='Liberation Mono',terminal_processes_preserved=True)))
    except Exception:
        log.flush();print((tmp/'kitty.log').read_text()[-5000:]);
        try:print(subprocess.check_output(['kitten','@','--to',address,'get-text','--extent','all'],stderr=subprocess.PIPE,timeout=5).decode()[-4000:])
        except Exception:pass
        for f in [*state.glob('*.log'), tmp/'child.log',tmp/'exit',tmp/'font-reply.json']:
            if f.exists():print(f.name,f.read_text()[-3000:])
        raise
    finally:
        # Stop only this test's private workspace. No real sessions are addressed.
        subprocess.run([str(app),'--workspace',name,'--stop'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=20)
        if kitty:
            kitty.terminate()
            try:kitty.wait(timeout=10)
            except subprocess.TimeoutExpired:kitty.kill();kitty.wait()
        if daemon:daemon.terminate();daemon.wait(timeout=10)
