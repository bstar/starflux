"""One-shot host helper. Payload is base64 JSON, file bytes use stdin."""
import base64, fcntl, hashlib, json, os, pathlib, re, shlex, subprocess, sys, tempfile

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
        line=line.lstrip(' \t│┃|●*•')
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
        result=subprocess.run(tm+list(args),stdout=subprocess.PIPE,stderr=subprocess.PIPE,**kw)
        if result.returncode:
            raise RuntimeError('tmux '+args[0]+': '+result.stderr.decode(errors='replace').strip())
        return result.stdout
    def exists():return subprocess.run(tm+['has-session','-t',pane],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0
    action=req['action']
    lifecycle_lock=None
    if action in ['ensure','restart','reap','stop'] or (action=='attach' and req.get('ensure_shell')):
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
            info=run('display-message','-p','-t',target,'#{pane_pid}\t#{pane_dead}\t#{pane_start_command}').decode().strip().split('\t',2)
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
        run('set-option','-g','mouse','on')
        run('set-option','-w','-t',target,'window-size','latest')
        if action=='attach':
            lifecycle_lock.close()
            os.execvp('tmux',tm+['attach-session','-t',pane])
        print(json.dumps({'session':pane,'server':server,'replaced_fixture':replaced}))
    elif action=='restart':
        if not exists():raise ValueError('Session is missing; attach it first')
        dead=run('display-message','-p','-t',target,'#{pane_dead}').decode().strip()
        if dead!='1':raise ValueError('The existing session is still running; it was not restarted')
        argv=req.get('argv') or [os.environ.get('SHELL','/bin/bash'),'-l']
        run('respawn-pane','-t',target,'-c',os.path.expanduser(req.get('directory') or '~'),shell_command(argv))
        print(json.dumps({'restarted':True}))
    elif action=='attach':
        run('set-option','-g','mouse','on')
        run('set-option','-w','-t',target,'window-size','latest')
        os.execvp('tmux',tm+['attach-session','-t',pane])
    elif action=='inspect':
        if not exists():print(json.dumps({'alive':False,'state':'missing'}));return
        fields=run('display-message','-p','-t',target,'#{pane_pid}\t#{pane_dead}\t#{pane_current_command}\t#{@starflux_state}\t#{pane_height}\t#{pane_tty}').decode().strip().split('\t')
        screen=run('capture-pane','-p','-t',target) if fields[1]=='0' else b''
        agent,agent_pid=foreground_agent(fields[0],fields[5],fields[2]) if fields[1]=='0' else (None,None)
        state=fields[3] if fields[3] and agent else 'unknown'
        if agent in ['codex','claude'] or (state=='unknown' and agent):
            state=activity_from_screen(screen.decode(errors='replace'))
        print(json.dumps({'alive':fields[1]=='0','exited':fields[1]=='1','pid':int(fields[0]),'command':fields[2],'agent':agent,'agent_pid':agent_pid,'state':state,'output_fingerprint':hashlib.sha256(screen).hexdigest()}))
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
    else:raise ValueError('Unknown action')
try:main()
except Exception as exc:
    print(str(exc),file=sys.stderr);sys.exit(1)
