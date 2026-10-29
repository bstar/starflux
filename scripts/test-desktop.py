#!/usr/bin/env python3
"""Real Wayland desktop drop through Kitty and authenticated loopback SSH.

Creates private test windows, keys and an SSH daemon. Injects mouse input only
when --inject-host-mouse is explicitly supplied. Does not edit desktop config.
Optional --codex tests attachment in an existing real Codex TUI.
"""
import argparse,base64,fcntl,getpass,hashlib,json,os,pathlib,shlex,socket,struct,subprocess,sys,tempfile,time,uuid,zlib
if '--uinput-helper' in sys.argv:
    fd=os.open('/dev/uinput',os.O_WRONLY|os.O_NONBLOCK)
    for ev in [0,1,2]:fcntl.ioctl(fd,0x40045564,ev)
    fcntl.ioctl(fd,0x40045565,272)
    for ax in [0,1]:fcntl.ioctl(fd,0x40045566,ax)
    os.write(fd,struct.pack('80sHHHHi',b'STARFLUX test mouse',3,0x1234,1,1,0)+bytes(1024));fcntl.ioctl(fd,0x5501);time.sleep(.5)
    def ev(t,c,v):os.write(fd,struct.pack('qqHHi',0,0,t,c,v))
    try:
        for line in sys.stdin:
            p=line.split()
            if p[0]=='M':
                subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.cursor.move({{x={p[1]},y={p[2]}}}))'],stdout=subprocess.DEVNULL,check=True)
                ev(2,0,1);ev(0,0,0);time.sleep(.02);ev(2,0,-1);ev(0,0,0)
            elif p[0]=='B':ev(1,272,int(p[1]));ev(0,0,0)
            elif p[0]=='H':
                for i in range(int(p[1])):
                    ev(2,0,1 if i%2==0 else -1);ev(0,0,0);time.sleep(.002)
                print('hover-complete',flush=True)
            time.sleep(.08)
    finally:ev(1,272,0);ev(0,0,0);fcntl.ioctl(fd,0x5502);os.close(fd)
    sys.exit(0)
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--inject-host-mouse',action='store_true',required=True);p.add_argument('--codex');p.add_argument('--text-file',action='store_true');p.add_argument('--hover-events',type=int,default=0);p.add_argument('--tall-workspace',action='store_true');p.add_argument('--name',default='screenshot.png');p.add_argument('--output',default='target/desktop-proof');a=p.parse_args()
repo=pathlib.Path(__file__).resolve().parents[1];out=pathlib.Path(a.output).resolve();out.mkdir(parents=True,exist_ok=True)
workspace='proof-'+uuid.uuid4().hex[:8];runtime=pathlib.Path(os.environ['XDG_RUNTIME_DIR'])/'starflux';app=repo/'scripts/run-local.sh'
original_focus=json.loads(subprocess.check_output(['hyprctl','activewindow','-j'])).get('address')
original_cursor=json.loads(subprocess.check_output(['hyprctl','cursorpos','-j']))
children=[]
wx,wy,ww,wh=(2300,180,1500,1900) if a.tall_workspace else (2700,180,1100,860)
def wait_for(fn,message,timeout=30):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        try:
            result=fn()
            if result:return result
        except (OSError,ValueError,subprocess.SubprocessError):pass
        time.sleep(.1)
    raise AssertionError(message)
def inspect():return json.loads((runtime/(workspace+'.state.json')).read_text())
def clients():return json.loads(subprocess.check_output(['hyprctl','clients','-j']))
def position(c,x,y,w,h):
    addr=c['address']
    for expr in [f'hl.dsp.window.float({{action="set",window="address:{addr}"}})',f'hl.dsp.window.resize({{x={w},y={h},relative=false,window="address:{addr}"}})',f'hl.dsp.window.move({{x={x},y={y},relative=false,window="address:{addr}"}})']:
        subprocess.run(['hyprctl','eval','hl.dispatch('+expr+')'],check=True,stdout=subprocess.DEVNULL)
    return addr
with tempfile.TemporaryDirectory(prefix='starflux-desktop-') as tmp:
    tmp=pathlib.Path(tmp);files=tmp/'drop-source';files.mkdir();image=files/a.name
    width=height=192
    raw=b''.join(b'\x00'+b''.join(bytes((20,210,80)) if 40<=x<152 and 40<=y<152 else bytes((250,250,250)) for x in range(width)) for y in range(height))
    def chunk(kind,data):return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data))
    image.write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',width,height,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b''))
    if a.text_file:image.write_text('STAR_FLUX_DROP_SENTINEL=STARFLUX_MARKDOWN_DROP_OK_8317\n')
    for name in ['host','client']:subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(tmp/name)],check=True)
    (tmp/'authorized_keys').write_text((tmp/'client.pub').read_text())
    with socket.socket() as probe:probe.bind(('127.0.0.1',0));port=probe.getsockname()[1]
    (tmp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(tmp/'host.pub').read_text())
    (tmp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM yes\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
    sshconfig=tmp/'ssh_config';sshconfig.write_text(f'Host starflux-test\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {tmp}/client\n IdentitiesOnly yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {tmp}/known_hosts\n')
    daemon=subprocess.Popen(['/usr/bin/sshd','-D','-e','-f',str(tmp/'sshd_config')],stdout=subprocess.DEVNULL,stderr=(out/'sshd.log').open('w'),env={k:v for k,v in os.environ.items() if k!='NOTIFY_SOCKET'});children.append(daemon)
    wait_for(lambda:subprocess.run(['ssh','-F',str(sshconfig),'starflux-test','true'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0,'Private SSH did not start')
    agent=shlex.join([a.codex,'--sandbox','read-only','--ask-for-approval','never','--no-alt-screen','-c','check_for_update_on_startup=false']) if a.codex else shlex.join([str(app),'--fixture',workspace])
    address='unix:'+str(tmp/'kitty.sock')
    cmd=['/usr/bin/kitty','--class','starflux-test','--config','NONE','--listen-on',address,'-o','allow_remote_control=yes','-o','linux_display_server=wayland','-o','font_size=12','--title','STAR-FLUX-ACCEPTANCE',str(app),'--workspace',workspace,'--host','starflux-test','--ssh-config',str(sshconfig),'--directory',str(tmp),'--agent',agent]
    kitty=subprocess.Popen(cmd,stdout=subprocess.DEVNULL,stderr=(out/'kitty.log').open('w'));children.append(kitty)
    def rc(*args,data=None):return subprocess.check_output(['/usr/bin/kitten','@','--to',address,*args],input=data,stderr=subprocess.PIPE,timeout=10)
    pointer=None
    try:
        state=wait_for(lambda:inspect() if inspect()['desktop']['windows']==2 else None,'Two Kitty surfaces did not map')
        pids=[p['pid'] for p in state['panes']]
        c=wait_for(lambda:next((c for c in clients() if c['class']=='starflux-test'),None),'Test window missing')
        position(c,wx,wy,ww,wh);time.sleep(1)
        subprocess.run(['grim','-g',f'{wx},{wy} {ww}x{wh}',str(out/'workspace.png')],check=True)
        if a.codex:
            # Trust dialog is accepted only for this disposable test directory.
            time.sleep(3)
            def capture():return subprocess.check_output([str(app),'--workspace',workspace,'--capture','--pane','2'],text=True,timeout=20)
            text=capture();(out/'codex-before.txt').write_text(text)
            if 'trust' in text.lower():subprocess.run(['tmux','-L','starflux-'+workspace,'send-keys','-t','p1:0.0','Enter'],check=True);time.sleep(2)
        source=subprocess.Popen(['dbus-run-session','--','nautilus','--new-window',str(files)],stdout=subprocess.DEVNULL,stderr=(out/'nautilus.log').open('w'),start_new_session=True,env={**os.environ,'GDK_BACKEND':'wayland','NO_AT_BRIDGE':'1','GDK_SCALE':'1','GDK_DPI_SCALE':'1','GTK_MODULES':'','XDG_CACHE_HOME':str(tmp/'nautilus-cache'),'XDG_DATA_HOME':str(tmp/'nautilus-data'),'XDG_DATA_DIRS':str(tmp/'empty-data'),'GSETTINGS_SCHEMA_DIR':'/usr/share/glib-2.0/schemas','GSETTINGS_BACKEND':'memory'});children.append(source)
        def own_source(c):
            try:return c['title']=='drop-source' and ('XDG_CACHE_HOME='+str(tmp/'nautilus-cache')).encode() in pathlib.Path(f'/proc/{c["pid"]}/environ').read_bytes().split(b'\0')
            except OSError:return False
        c=wait_for(lambda:next((c for c in clients() if own_source(c)),None),'Nautilus source window missing')
        position(c,3820,180,560,800);time.sleep(3)
        subprocess.run(['grim','-g',f'{wx},{wy} {4380-wx}x{max(wh,860)}',str(out/'source-ready.png')],check=True)
        pointer=subprocess.Popen([sys.executable,__file__,'--uinput-helper'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True);time.sleep(.8)
        def motion(x,y):pointer.stdin.write(f'M {x} {y}\n');pointer.stdin.flush();time.sleep(.15)
        def button(v):pointer.stdin.write(f'B {v}\n');pointer.stdin.flush();time.sleep(.15)
        # Exercise dropping after an embedded terminal has keyboard focus.
        motion(3500,450);button(1);button(0)
        motion(3906,274);button(1)
        for x,y in [(3916,284),(3800,400),(3570,450),(3500,450)]:motion(x,y)
        if a.hover_events:
            # A held external drag must remain attached before any drop event.
            pointer.stdin.write(f'H {a.hover_events}\n');pointer.stdin.flush()
            assert pointer.stdout.readline().strip()=='hover-complete'
            assert kitty.poll() is None, 'Frontend exited while hovering before drop'
            assert json.loads(rc('ls'))[0]['tabs'][0]['windows'][0]['in_alternate_screen'], 'Frontend left graphics during drag hover'
            for pane in [1,2]:
                text=subprocess.check_output([str(app),'--workspace',workspace,'--capture','--pane',str(pane)],text=True,timeout=20)
                assert 'text/uri-list' not in text and 't=m:x=' not in text, 'Hover protocol reached a terminal'
        time.sleep(.4);button(0)
        result=wait_for(lambda:inspect() if any('Copied to' in s and ('pane 2' in s or 'session 2:' in s) for s in inspect()['history']) else None,'Real desktop drop did not finish',timeout=25)
        assert image.exists(),'Copy deleted original'
        capture=subprocess.check_output([str(app),'--workspace',workspace,'--capture','--pane','2'],text=True,timeout=20)
        (out/'after-drop.txt').write_text(capture)
        assert image.name in capture or '[Image' in capture,'Remote harness did not receive attachment'
        assert 'text/uri-list' not in capture and 't=m:x=' not in capture,'Drag protocol leaked into agent input'
        if not a.codex:
            assert 'Received ' not in capture,'Drop automatically submitted the prompt'
            subprocess.run(['tmux','-L','starflux-'+workspace,'send-keys','-t','p1:0.0','Enter'],check=True)
            wait_for(lambda:'Received ' in subprocess.check_output([str(app),'--workspace',workspace,'--capture','--pane','2'],text=True,timeout=20),'Remote fixture did not read file')
        else:
            if not a.text_file:assert '[Image' in capture,'Codex did not attach image content'
            # This explicit acceptance-test submission is separate from drop behavior.
            prompt=' Read the dropped AGENTS.md file and reply only with its STAR_FLUX_DROP_SENTINEL value.' if a.text_file else 'What color is the square in the attached image? Reply with only the color. Do not use tools.'
            subprocess.run(['tmux','-L','starflux-'+workspace,'load-buffer','-b','starflux-test-prompt','-'],input=prompt.encode(),check=True)
            subprocess.run(['tmux','-L','starflux-'+workspace,'paste-buffer','-d','-p','-b','starflux-test-prompt','-t','p1:0.0'],check=True)
            subprocess.run(['tmux','-L','starflux-'+workspace,'send-keys','-t','p1:0.0','Enter'],check=True)
            expected='• STARFLUX_MARKDOWN_DROP_OK_8317' if a.text_file else '• Green'
            def answered():
                text=subprocess.check_output([str(app),'--workspace',workspace,'--capture','--pane','2'],text=True,timeout=20)
                return text if expected in text else None
            answer=wait_for(answered,'Codex did not read the dropped file correctly',timeout=90)
            (out/'codex-answer.txt').write_text(answer)
        subprocess.run(['grim','-g',f'{wx},{wy} {4380-wx}x{max(wh,860)}',str(out/'delivered.png')],check=True)
        # Disconnect the SSH terminal client, preserving the server-owned process.
        cp=int(subprocess.check_output(['tmux','-L','starflux-'+workspace,'list-clients','-t','p1','-F','#{client_pid}'],text=True).splitlines()[0]);os.kill(cp,15)
        time.sleep(7)
        assert inspect()['panes'][1]['pid']==pids[1],'SSH disconnect replaced agent process'
        wait_for(lambda:subprocess.check_output(['tmux','-L','starflux-'+workspace,'list-clients','-t','p1','-F','#{client_pid}'],text=True).strip(),'SSH terminal did not reattach')
        # Kill the presenting process, then attach a new Kitty to the same controller.
        frontend=json.loads(rc('ls'))[0]['tabs'][0]['windows'][0]['pid']
        controller=inspect()['controller_pid']
        os.kill(frontend,9)
        kitty.wait(timeout=10)
        os.kill(controller,0)
        kitty=subprocess.Popen(cmd,stdout=subprocess.DEVNULL,stderr=(out/'kitty-reattach.log').open('w'));children.append(kitty)
        wait_for(lambda:json.loads(rc('ls'))[0]['tabs'][0]['windows'][0]['in_alternate_screen'],'Frontend did not reattach')
        assert inspect()['controller_pid']==controller,'Frontend death replaced the controller'
        for i in range(2):
            assert int(subprocess.check_output(['tmux','-L','starflux-'+workspace,'display-message','-p','-t',f'p{i}:0.0','#{pane_pid}'],text=True))==pids[i]
        # Closing the workspace is explicit; both sessions remain alive.
        subprocess.run([str(app),'--workspace',workspace,'--close'],check=True);time.sleep(2)
        for i in range(2):
            pid=int(subprocess.check_output(['tmux','-L','starflux-'+workspace,'display-message','-p','-t',f'p{i}:0.0','#{pane_pid}'],text=True));assert pid==pids[i]
        # Keep the same terminal alive after the app exits, as an ordinary
        # parent shell would, and capture anything a subsequent drag sends it.
        wrapper=tmp/'after-exit.py'
        wrapper.write_text("""import os,pathlib,select,subprocess,sys,time,tty
subprocess.run(sys.argv[3:])
tty.setraw(0)
pathlib.Path(sys.argv[1]).write_text('ready')
end=time.monotonic()+5; data=bytearray()
while time.monotonic()<end:
 if select.select([0],[],[],max(0,end-time.monotonic()))[0]: data.extend(os.read(0,65536))
pathlib.Path(sys.argv[2]).write_bytes(data)
time.sleep(20)
""")
        ready=tmp/'shell-ready';after_exit=out/'after-exit-input.bin'
        wrapped=cmd[:cmd.index(str(app))]+[sys.executable,str(wrapper),str(ready),str(after_exit)]+cmd[cmd.index(str(app)):]
        # The previous presentation has exited; use its backend sessions again.
        kitty.wait(timeout=10)
        kitty=subprocess.Popen(wrapped,stdout=subprocess.DEVNULL,stderr=(out/'kitty-exit.log').open('w'));children.append(kitty)
        c=wait_for(lambda:next((c for c in clients() if c['class']=='starflux-test'),None),'Exit test window missing')
        position(c,wx,wy,ww,wh)
        wait_for(lambda:inspect() if inspect()['controller_pid']!=controller and inspect()['drop_enabled'] else None,'Exit test did not register drops')
        subprocess.run([str(app),'--workspace',workspace,'--close'],check=True)
        wait_for(ready.exists,'Parent shell monitor did not start')
        motion(3906,274);button(1)
        for x,y in [(3916,284),(3800,400),(3570,450),(3500,450)]:motion(x,y)
        button(0)
        wait_for(after_exit.exists,'Parent shell input capture missing')
        leaked=after_exit.read_bytes()
        assert b'\x1b]72;' not in leaked and b't=m:x=' not in leaked, 'Drag reports remained enabled after frontend exit'
        (out/'result.json').write_text(json.dumps({'passed':True,'ssh':'authenticated loopback','real_desktop_drop':True,'frontend_kill_and_reattach':True,'codex_image_understanding':bool(a.codex) and not a.text_file,'codex_markdown_read':bool(a.codex) and a.text_file,'hover_events_before_drop':a.hover_events,'viewport_test_size':[ww,wh],'no_protocol_in_agent_input':True,'no_drag_reports_after_exit':True,'pids_preserved':pids,'workspace':workspace},indent=2)+'\n')
        print((out/'result.json').read_text())
    finally:
        if 'source' in locals():
            try:os.killpg(source.pid,15)
            except ProcessLookupError:pass
        if pointer:
            try:pointer.stdin.close();pointer.wait(timeout=3)
            except Exception:pointer.terminate()
        try:subprocess.run([str(app),'--workspace',workspace,'--stop'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15)
        except Exception:pass
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
                try:child.wait(timeout=5)
                except subprocess.TimeoutExpired:child.kill();child.wait()
        if original_cursor:subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.cursor.move({{x={original_cursor["x"]},y={original_cursor["y"]}}}))'],stdout=subprocess.DEVNULL)
        if original_focus:subprocess.run(['hyprctl','eval','hl.dispatch(hl.dsp.focus({window="address:'+original_focus+'"}))'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
