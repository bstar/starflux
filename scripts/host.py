"""One-shot host helper. Payload is base64 JSON, file bytes use stdin."""
import base64, fcntl, hashlib, json, os, pathlib, re, shlex, signal, subprocess, sys, tempfile, termios, time

def boot_id():
    return pathlib.Path('/proc/sys/kernel/random/boot_id').read_text().strip()

def check_boot(req):
    if req.get('expected_boot_id') != boot_id():
        raise ValueError('Host boot identity missing or changed; action rejected')

def agent_family(args):
    families={'codex':'codex','claude':'claude','claude-code':'claude','gemini':'gemini','gemini-cli':'gemini','opencode':'opencode','aider':'aider'}
    for arg in args[:2]:
        path=pathlib.PurePath(arg)
        names=[path.name]
        if 'node_modules' in path.parts:
            names.extend(path.parts[path.parts.index('node_modules')+1:])
        for name in names:
            name=name.removesuffix('.js').removesuffix('.mjs').lower()
            if name in families:return families[name]
    return None

def foreground_agent(pid, tty, command):
    # Follow this terminal's process tree, filtering out background jobs. The
    # command line is inspected on its host; only the family name is exported.
    try:
        foreground=int((pathlib.Path('/proc')/str(pid)/'stat').read_text().rsplit(')',1)[1].split()[5])
        pending=[int(pid)];seen=set()
        while pending and len(seen)<64:
            current=pending.pop()
            if current in seen:continue
            seen.add(current);root=pathlib.Path('/proc')/str(current)
            try:
                info=(root/'stat').read_text();stat=info.rsplit(')',1)[1].split()
                if int(stat[2])==foreground:
                    args=[s.decode(errors='replace') for s in (root/'cmdline').read_bytes().split(b'\0') if s]
                    family=agent_family(args) or agent_family([info.partition('(')[2].rsplit(')',1)[0]])
                    if family:return family,current
                pending.extend(map(int,(root/'task'/str(current)/'children').read_text().split()))
            except (OSError,ValueError,IndexError):continue
    except (OSError,ValueError):pass
    return agent_family([command]),None

def activity_from_screen(text):
    # Only inspect the status footer, never publish conversation contents.
    footer='\n'.join(text.splitlines()[-32:]).lower()
    choice=r'(?m)^\s*[│┃|>›❯●* ]*(?:\d+[.)]|[yn]\))\s*'
    yes=re.search(choice+r'(?:yes\b|allow once\b)',footer)
    no=re.search(choice+r"(?:no\b|don't allow\b|deny this\b)",footer)
    question=re.search(r'(?m)^\s*[│┃|>›❯●* ]*(?:question[ :]|user input requested)',footer)
    if question and re.search(choice,footer) and any(hint in footer for hint in ['enter to submit','enter to confirm','enter to select']):
        return 'needs_input'
    if yes and no and ('esc' in footer or 'enter' in footer):
        return 'waiting_for_approval'
    for line in reversed(footer.splitlines()):
        line=line.lstrip(' \t│┃|●*•■')
        if line.startswith('automatic reconnect could not restore this session') or line.startswith('server connection could not be restored'):
            return 'backend_unavailable'
        if line.startswith('worked for '):return 'idle'
        if re.match(r'^(?:working|thinking|running|executing)\s+\([0-9][hms0-9. ·•]*esc to interrupt\)',line):return 'working'
    return 'idle'

def main():
    req=json.loads(base64.b64decode(sys.argv[1],validate=True))
    workspace=req['workspace']; pane=req.get('pane','p0')
    if not re.fullmatch(r'[a-zA-Z0-9_-]{1,32}',workspace) or not re.fullmatch(r'p(?:[0-9]|[12][0-9]|3[01])',pane):
        raise ValueError('Invalid workspace or pane')
    server=req.get('server') or 'starflux-'+workspace
    if not re.fullmatch(r'starflux-[a-zA-Z0-9_-]{1,88}',server):raise ValueError('Invalid terminal server')
    target=pane+':0.0'
    tm=['tmux','-L',server,'-f','/dev/null']
    if req.get('source_socket'):
        if req['action']!='inspect' or not os.path.isabs(req['source_socket']) or not re.fullmatch(r'\$[0-9]+',req.get('source_session','')):
            raise ValueError('Imported tmux inspection is read only')
        tm=['tmux','-u','-S',req['source_socket']]
        target=req['source_session']
    def shell_command(argv):
        # The AppImage's tools are private to its controller. Project commands
        # must find the host's Python, codecs and audio configuration.
        if 'STARFLUX_SYSTEM_PATH' in os.environ:
            clean=['/usr/bin/env']
            for key in ['STARFLUX_SYSTEM_PATH','GST_PLUGIN_SYSTEM_PATH_1_0','GST_PLUGIN_SCANNER','GST_REGISTRY','ALSA_CONFIG_DIR','ALSA_CONFIG_PATH','ALSA_PLUGIN_DIR']:
                clean+=['-u',key]
            argv=clean+['PATH='+os.environ['STARFLUX_SYSTEM_PATH']]+argv
        return shlex.join(argv)
    def run(*args,**kw):
        if args[0]=='capture-pane':
            # OSC 8 can amplify one shared URI into many serialized runs. Stop
            # reading before buffering an arbitrarily large capture in Python.
            with tempfile.TemporaryFile() as errors:
                child=subprocess.Popen(tm+list(args),stdout=subprocess.PIPE,stderr=errors,**kw)
                try:
                    output=child.stdout.read(2*1024*1024+1)
                    if len(output)>2*1024*1024:
                        child.kill();child.wait()
                        raise RuntimeError('Terminal capture exceeds 2 MiB')
                    status=child.wait()
                    if status:
                        errors.seek(0)
                        raise RuntimeError('tmux capture-pane: '+errors.read(8192).decode(errors='replace').strip())
                    return output
                finally:
                    child.stdout.close()
                    if child.poll() is None:child.kill();child.wait()
        result=subprocess.run(tm+list(args),stdout=subprocess.PIPE,stderr=subprocess.PIPE,**kw)
        if result.returncode:
            raise RuntimeError('tmux '+args[0]+': '+result.stderr.decode(errors='replace').strip())
        return result.stdout
    def exists():return subprocess.run(tm+['has-session','-t',target if req.get('source_socket') else pane],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0
    action=req['action']
    lifecycle_lock=None
    if action in ['ensure','restart','recover','reap','stop','terminate','input'] or (action=='attach' and req.get('ensure_shell')):
        lock_root=pathlib.Path(os.environ.get('XDG_RUNTIME_DIR',tempfile.gettempdir()))/('starflux-host-'+str(os.getuid()))
        lock_root.mkdir(mode=0o700,exist_ok=True)
        lifecycle_lock=(lock_root/(workspace+'-'+pane+'.lock')).open('a')
        fcntl.flock(lifecycle_lock,fcntl.LOCK_EX)
    if action=='ensure' or (action=='attach' and req.get('ensure_shell')):
        replaced=False
        cwd=os.path.expanduser(req.get('directory') or '~')
        argv=req.get('argv') or [os.environ.get('SHELL','/bin/bash'),'-l']
        if req.get('recover_proxy'):
            if len(argv) not in (3,5) or pathlib.Path(argv[0]).name!='starflux-pty-proxy':
                raise ValueError('Only a transferred PTY proxy can be recovered automatically')
            if exists() and run('display-message','-p','-t',target,'#{pane_dead}').decode().strip()=='1':
                run('respawn-pane','-t',target,'-c',cwd,shell_command(argv))
        if exists() and req.get('replace_fixture') and pane=='p1':
            info=run('display-message','-p','-t',target,'#{pane_pid}\t#{pane_dead}\t#{pane_start_command}').decode().rstrip('\n').split('\t',2)
            try:
                old=shlex.split(info[2]) if info[1]=='1' else [s.decode() for s in pathlib.Path('/proc/'+info[0]+'/cmdline').read_bytes().split(b'\0') if s]
                if info[1]=='1' and len(old)==1:old=shlex.split(old[0])
                if info[1]=='1' and 'STARFLUX_SYSTEM_PATH' in os.environ:
                    prefix=shlex.split(shell_command([]))
                    if old[:len(prefix)]==prefix:old=old[len(prefix):]
            except (OSError,ValueError,UnicodeError):old=[]
            # Never replace a shell or real harness, even in a legacy workspace.
            if len(old)==3 and pathlib.Path(old[0]).name=='starflux' and old[1:]==['--fixture',workspace]:
                run('respawn-pane','-k','-t',target,'-c',cwd,shell_command(argv))
                run('set-option','-p','-u','-t',target,'@starflux_state')
                replaced=True
        if not exists():
            run('new-session','-d','-s',pane,'-c',cwd,'-x','100','-y','32',shell_command(argv))
            run('set-option','-g','status','off')
            run('set-option','-g','history-limit','20000')
            run('set-option','-g','allow-passthrough','on')
            run('set-option','-g','extended-keys','on')
            run('set-option','-g','remain-on-exit','on')
            run('set-option','-g','set-clipboard','on')
            run('set-option','-g','default-terminal','tmux-256color')
        # The workspace owns keyboard commands. Its private transport must not
        # intercept literal input (especially an escaped Ctrl+B).
        run('set-option','-g','prefix','None')
        run('set-option','-g','prefix2','None')
        run('set-option','-g','mouse','on')
        run('set-option','-w','-t',target,'window-size','latest')
        if action=='attach':
            lifecycle_lock.close()
            os.execvp('tmux',tm+['-T','hyperlinks','attach-session','-t',pane])
        print(json.dumps({'session':pane,'server':server,'replaced_fixture':replaced}))
    elif action=='recover':
        if req.get('original_pid'):
            # A transferred terminal's tmux process is only a proxy. Recover
            # the original foreground job, keeping its shell and PTY owner.
            pid=int(req['original_pid']); proc=pathlib.Path('/proc')/str(pid)
            stat=(proc/'stat').read_text().rsplit(')',1)[1].split()
            if proc.stat().st_uid!=os.getuid() or int(stat[3])!=pid:
                raise ValueError('Original shell identity changed; recovery refused')
            tty=os.readlink(proc/'fd/0')
            if not tty.startswith('/dev/pts/'):
                raise ValueError('Original terminal is unavailable')
            foreground=int(stat[5])
            if foreground<1:raise ValueError('Original shell has no foreground terminal')
            if foreground>1 and foreground!=pid:
                members=[]
                for entry in pathlib.Path('/proc').iterdir():
                    if not entry.name.isdigit():continue
                    try:
                        info=(entry/'stat').read_text().rsplit(')',1)[1].split()
                        if int(info[2])==foreground:
                            if int(info[3])!=pid or entry.stat().st_uid!=os.getuid():
                                raise ValueError('Foreground group belongs to another session')
                            members.append(int(entry.name))
                    except (FileNotFoundError,ProcessLookupError,PermissionError):continue
                if members:
                    try:os.killpg(foreground,signal.SIGKILL)
                    except ProcessLookupError:pass
                deadline=time.monotonic()+2
                while time.monotonic()<deadline:
                    current=(proc/'stat').read_text().rsplit(')',1)[1].split()
                    if int(current[5])==pid:break
                    time.sleep(.02)
                else:raise ValueError('Foreground job stopped, but the shell has not reclaimed the terminal')
            fd=os.open(tty,os.O_RDWR|os.O_NOCTTY|os.O_NONBLOCK)
            try:
                termios.tcflow(fd,termios.TCOON)
                subprocess.run(['stty','sane'],stdin=fd,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
                os.write(fd,b'\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[0m\x1b[?25h\x1b[2J\x1b[H')
            finally:os.close(fd)
            if exists():run('send-keys','-t',target,'C-l')
            print(json.dumps({'recovered':True,'shell_preserved':True}))
        else:
            if not exists():raise ValueError('Session is missing; reconnect it first')
            argv=[os.environ.get('SHELL','/bin/bash'),'-l']
            run('respawn-pane','-k','-t',target,'-c',os.path.expanduser(req.get('directory') or '~'),shell_command(argv))
            run('set-option','-p','-u','-t',target,'@starflux_state')
            print(json.dumps({'recovered':True,'shell_preserved':False}))
    elif action=='restart':
        if not exists():raise ValueError('Session is missing; attach it first')
        dead=run('display-message','-p','-t',target,'#{pane_dead}').decode().strip()
        if dead!='1':raise ValueError('The existing session is still running; it was not restarted')
        argv=req.get('argv') or [os.environ.get('SHELL','/bin/bash'),'-l']
        run('respawn-pane','-t',target,'-c',os.path.expanduser(req.get('directory') or '~'),shell_command(argv))
        print(json.dumps({'restarted':True}))
    elif action=='attach':
        run('set-option','-g','prefix','None')
        run('set-option','-g','prefix2','None')
        run('set-option','-g','mouse','on')
        run('set-option','-w','-t',target,'window-size','latest')
        os.execvp('tmux',tm+['-T','hyperlinks','attach-session','-t',pane])
    elif action=='inspect':
        if not exists():print(json.dumps({'alive':False,'state':'missing'}));return
        fields=run('display-message','-p','-t',target,'#{pane_pid}\t#{pane_dead}\t#{pane_current_command}\t#{@starflux_state}\t#{pane_height}\t#{pane_tty}\t#{pane_width}\t#{cursor_x}\t#{cursor_y}\t#{alternate_on}\t#{cursor_flag}\t#{pane_current_path}').decode().rstrip('\n').split('\t')
        screen=run('capture-pane','-p','-t',target) if fields[1]=='0' else b''
        agent,agent_pid=foreground_agent(fields[0],fields[5],fields[2]) if fields[1]=='0' else (None,None)
        cwd=fields[11]
        if agent_pid:
            try:cwd=os.readlink('/proc/'+str(agent_pid)+'/cwd')
            except OSError:pass
        state=fields[3] if fields[3] and agent else 'unknown'
        if agent in ['codex','claude'] or (state=='unknown' and agent):
            state=activity_from_screen(screen.decode(errors='replace'))
        try:agent_start_ticks=int(pathlib.Path('/proc/'+str(agent_pid)+'/stat').read_text().rsplit(')',1)[1].split()[19]) if agent_pid else None
        except (OSError,ValueError,IndexError):agent_start_ticks=None
        try:pid_start_ticks=int(pathlib.Path('/proc/'+fields[0]+'/stat').read_text().rsplit(')',1)[1].split()[19])
        except (OSError,ValueError,IndexError):pid_start_ticks=None
        # tmux can clear pane_pid after the process exits while retaining its
        # screen. Report that exited pane instead of failing status/recovery
        # with int(''); a missing PID must never become an authorization target.
        pid=int(fields[0]) if fields[0].isdigit() else None
        result={'alive':fields[1]=='0' and pid is not None,'exited':fields[1]=='1','pid':pid,'pid_start_ticks':pid_start_ticks,'boot_id':boot_id(),'command':fields[2],'cwd':cwd,'agent':agent,'agent_pid':agent_pid,'agent_start_ticks':agent_start_ticks,'state':state,'output_fingerprint':hashlib.sha256(screen).hexdigest()}
        if req.get('include_screen') and fields[1]=='0':
            try:
                rows,columns=int(fields[4]),int(fields[6])
                if not 0<rows*columns<=500000:raise ValueError('Screen geometry exceeds its limit')
                formatted=run('capture-pane','-p','-e','-N','-t',target)
                if len(formatted)>1024*1024:raise ValueError('Screen exceeds 1 MiB')
                current=run('display-message','-p','-t',target,'#{pane_pid}\t#{pane_dead}\t#{pane_width}\t#{pane_height}\t#{cursor_x}\t#{cursor_y}\t#{alternate_on}\t#{cursor_flag}').decode().strip().split('\t')
                if current[:2]!=fields[:2] or current[2:4]!=[fields[6],fields[4]] or current[6]!=fields[9]:raise ValueError('Terminal changed during screen capture')
                result['saved_screen']={'rows':rows,'columns':columns,'cursor':[min(int(current[5]),rows-1),min(int(current[4]),columns-1)],'alternate':current[6]=='1','cursor_visible':current[7]=='1','ansi_base64':base64.b64encode(formatted).decode()}
            except Exception as exc:result['screen_error']=str(exc)
        print(json.dumps(result))
    elif action=='reap':
        # Reclaim only a terminal whose process has already exited. A live
        # session is never killed, including if another client revived it.
        if not exists():print(json.dumps({'reaped':True}));return
        if run('display-message','-p','-t',target,'#{pane_dead}').decode().strip()!='1':
            print(json.dumps({'reaped':False}));return
        run('if-shell','-F','-t',target,'#{pane_dead}','kill-session -t '+pane)
        print(json.dumps({'reaped':not exists()}))
    elif action=='capture':
        sys.stdout.buffer.write(run('capture-pane','-p','-t',target,'-S','-100'))
    elif action=='history':
        # Read-only, independent of any client's copy mode or mouse reporting.
        sys.stdout.buffer.write(run('capture-pane','-p','-e','-J','-t',target,'-S','-4000'))
    elif action=='paste':
        text=req['text'].encode()
        if len(text)>1024*1024:raise ValueError('Paste exceeds 1 MiB')
        if not text:
            print(json.dumps({'delivered':True,'submitted':False}));return
        name='starflux-'+os.urandom(8).hex()
        run('load-buffer','-b',name,'-',input=text)
        try:run('paste-buffer','-b',name,'-d','-p','-t',target)
        finally:subprocess.run(tm+['delete-buffer','-b',name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        print(json.dumps({'delivered':True,'submitted':False}))
    elif action=='queue_conversation':
        check_boot(req)
        pid=req.get('expected_agent_pid');ticks=req.get('expected_agent_start_ticks')
        if not isinstance(pid,int) or not isinstance(ticks,int):raise ValueError('Native agent identity is required')
        process=pathlib.Path('/proc')/str(pid)
        if int((process/'stat').read_text().rsplit(')',1)[1].split()[19])!=ticks:raise ValueError('Agent process changed')
        argv=[os.fsdecode(v) for v in (process/'cmdline').read_bytes().split(b'\0') if v]
        if (agent_family(argv) or agent_family([(process/'comm').read_text().strip()]))!='codex':raise ValueError('Native queued delivery is supported only by Codex')
        conversation=req.get('conversation');text=req.get('text')
        if not isinstance(conversation,str) or len(conversation)>256 or not isinstance(text,str) or len(text.encode())>32768:raise ValueError('Invalid queued handoff')
        # The native queue leaves the running CLI's draft and terminal input alone.
        result=subprocess.run(['codex','queue','--thread',conversation,'--message',text],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=12)
        if result.returncode:raise ValueError('Native queue did not acknowledge the handoff')
        print(json.dumps({'queued':True}))
    elif action=='input':
        check_boot(req)
        # API input never traverses a frontend's focus or copy mode. Verify the
        # owner and foreground agent immediately before touching its input.
        fields=run('display-message','-p','-t',target,'#{pane_pid}\t#{pane_dead}\t#{pane_current_command}\t#{pane_tty}\t#{bracket_paste_flag}').decode().strip().split('\t')
        proxy_pid=int(fields[0]);pid=int(req.get('original_pid') or proxy_pid)
        ticks=int(pathlib.Path('/proc/'+str(pid)+'/stat').read_text().rsplit(')',1)[1].split()[19])
        if fields[1]!='0' or pid!=req['expected_pid'] or ticks!=req['expected_start_ticks']:
            raise ValueError('Terminal identity changed; input rejected')
        if req.get('original_pid'):
            argv=[os.fsdecode(arg) for arg in pathlib.Path('/proc/'+str(proxy_pid)+'/cmdline').read_bytes().split(b'\0') if arg]
            if len(argv)<3 or pathlib.Path(argv[0]).name!=pathlib.Path(req['expected_proxy']).name or argv[1:3]!=[req['expected_broker'],req['expected_terminal_key']]:
                raise ValueError('Transferred terminal bridge changed; input rejected')
            tty=os.readlink('/proc/'+str(pid)+'/fd/0');command=pathlib.Path('/proc/'+str(pid)+'/comm').read_text().strip()
        else:tty,command=fields[3],fields[2]
        agent,agent_pid=foreground_agent(str(pid),tty,command)
        try:agent_ticks=int(pathlib.Path('/proc/'+str(agent_pid)+'/stat').read_text().rsplit(')',1)[1].split()[19]) if agent_pid else None
        except OSError:agent_ticks=None
        if agent_pid!=req['expected_agent_pid'] or agent_ticks!=req['expected_agent_start_ticks']:
            raise ValueError('Foreground agent changed; input rejected')
        text=req['text'];submit=req['submit']
        if not isinstance(text,str) or len(text.encode())>32768 or type(submit) is not bool:
            raise ValueError('Invalid input payload')
        if any(ord(c)<32 and c not in '\n\t' or ord(c)==127 for c in text):
            raise ValueError('Input contains control characters')
        if ('\n' in text or '\t' in text) and fields[4]!='1':
            raise ValueError('Multiline/tab input requires bracketed paste support')
        if text:
            name='starflux-'+os.urandom(8).hex()
            run('load-buffer','-b',name,'-',input=text.encode())
            try:run('paste-buffer','-b',name,'-d','-p','-t',target)
            finally:subprocess.run(tm+['delete-buffer','-b',name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        if submit:run('send-keys','-t',target,'Enter')
        print(json.dumps({'delivered':True,'submitted':submit,'bytes':len(text.encode())}))
    elif action=='key':
        if req['key'] not in ['Enter','C-c','Escape']:raise ValueError('Unsupported key')
        run('send-keys','-t',target,req['key']);print('{}')
    elif action=='upload':
        name=req['name']; size=req['size']
        if pathlib.Path(name).name!=name or name in ['.','..'] or any(ord(c)<32 for c in name):raise ValueError('Invalid file name')
        if not 0<=size<=64*1024*1024:raise ValueError('File exceeds 64 MiB demo limit')
        root=pathlib.Path(os.environ.get('XDG_DATA_HOME',os.path.expanduser('~/.local/share')))/'starflux'/'attachments'/workspace/pane
        root.mkdir(parents=True,exist_ok=True,mode=0o700)
        stage=pathlib.Path(tempfile.mkdtemp(prefix='drop-',dir=root));os.chmod(stage,0o700)
        partial=stage/'.upload'; dest=stage/name; digest=hashlib.sha256(); count=0
        try:
            with partial.open('xb') as out:
                os.chmod(partial,0o600)
                while count<size:
                    chunk=sys.stdin.buffer.read(min(65536,size-count))
                    if not chunk:raise ValueError('Transfer interrupted')
                    out.write(chunk);digest.update(chunk);count+=len(chunk)
                out.flush();os.fsync(out.fileno())
            if digest.hexdigest()!=req['sha256']:raise ValueError('Checksum mismatch')
            os.replace(partial,dest)
            print(json.dumps({'path':str(dest),'size':count,'sha256':digest.hexdigest()}))
        except BaseException:
            partial.unlink(missing_ok=True);stage.rmdir();raise
    elif action=='stop':
        if exists():run('kill-session','-t',pane)
        print('{}')
    elif action=='terminate':
        check_boot(req)
        if not exists():raise ValueError('Terminal no longer exists')
        pid=int(run('display-message','-p','-t',target,'#{pane_pid}').decode().strip())
        start=int(pathlib.Path('/proc/'+str(pid)+'/stat').read_text().rsplit(')',1)[1].split()[19])
        if pid!=req['expected_pid'] or start!=req['expected_start_ticks']:raise ValueError('Terminal process changed; termination canceled')
        run('kill-session','-t',pane)
        print(json.dumps({'terminated':True}))
    else:raise ValueError('Unknown action')
try:main()
except Exception as exc:
    print(str(exc),file=sys.stderr);sys.exit(1)
