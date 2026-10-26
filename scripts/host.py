"""One-shot host helper. Payload is base64 JSON, file bytes use stdin."""
import base64, hashlib, json, os, pathlib, re, shlex, subprocess, sys, tempfile

def activity_from_screen(text):
    # Only inspect the status footer, never publish conversation contents.
    footer='\n'.join(text.splitlines()[-12:]).lower()
    if 'esc to interrupt' in footer:
        return 'working'
    if any(hint in footer for hint in ['would you like to run', 'needs your approval', 'approve this', 'esc to cancel', 'press enter to confirm']):
        return 'waiting_for_input'
    return 'idle'

def main():
    req=json.loads(base64.b64decode(sys.argv[1],validate=True))
    workspace=req['workspace']; pane=req.get('pane','p0')
    if not re.fullmatch(r'[a-zA-Z0-9_-]{1,32}',workspace) or not re.fullmatch(r'p(?:[0-9]|[12][0-9]|3[01])',pane):
        raise ValueError('Invalid workspace or pane')
    server='starmux-'+workspace; target=pane+':0.0'
    tm=['tmux','-L',server,'-f','/dev/null']
    def run(*args,**kw):return subprocess.run(tm+list(args),check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE,**kw).stdout
    def exists():return subprocess.run(tm+['has-session','-t',pane],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0
    action=req['action']
    if action=='ensure':
        replaced=False
        cwd=os.path.expanduser(req.get('directory') or '~')
        argv=req.get('argv') or [os.environ.get('SHELL','/bin/bash'),'-l']
        if exists() and req.get('replace_fixture') and pane=='p1':
            info=run('display-message','-p','-t',target,'#{pane_pid}\t#{pane_dead}\t#{pane_start_command}').decode().strip().split('\t',2)
            try:
                old=shlex.split(info[2]) if info[1]=='1' else [s.decode() for s in pathlib.Path('/proc/'+info[0]+'/cmdline').read_bytes().split(b'\0') if s]
                if info[1]=='1' and len(old)==1:old=shlex.split(old[0])
            except (OSError,ValueError,UnicodeError):old=[]
            # Never replace a shell or real harness, even in a legacy workspace.
            if len(old)==3 and pathlib.Path(old[0]).name=='starmux' and old[1:]==['--fixture',workspace]:
                run('respawn-pane','-k','-t',target,'-c',cwd,shlex.join(argv))
                run('set-option','-p','-u','-t',target,'@starmux_state')
                replaced=True
        if not exists():
            run('new-session','-d','-s',pane,'-c',cwd,'-x','100','-y','32',shlex.join(argv))
            run('set-option','-g','status','off')
            run('set-option','-g','mouse','off')
            run('set-option','-g','history-limit','20000')
            run('set-option','-g','allow-passthrough','on')
            run('set-option','-g','extended-keys','on')
            run('set-option','-g','remain-on-exit','on')
            run('set-option','-g','set-clipboard','on')
            run('set-option','-g','default-terminal','tmux-256color')
        print(json.dumps({'session':pane,'server':server,'replaced_fixture':replaced}))
    elif action=='restart':
        if not exists():raise ValueError('Session is missing; attach it first')
        dead=run('display-message','-p','-t',target,'#{pane_dead}').decode().strip()
        if dead!='1':raise ValueError('The existing session is still running; it was not restarted')
        argv=req.get('argv') or ['codex']
        run('respawn-pane','-t',target,'-c',os.path.expanduser(req.get('directory') or '~'),shlex.join(argv))
        print(json.dumps({'restarted':True}))
    elif action=='attach':
        os.execvp('tmux',tm+['attach-session','-t',pane])
    elif action=='inspect':
        if not exists():print(json.dumps({'alive':False,'state':'missing'}));return
        fields=run('display-message','-p','-t',target,'#{pane_pid}\t#{pane_dead}\t#{pane_current_command}\t#{@starmux_state}\t#{pane_height}').decode().strip().split('\t')
        state=fields[3] if fields[3] else 'unknown'
        if state=='unknown' and fields[1]=='0' and any(h in fields[2].lower() for h in ['codex','claude']):
            height=int(fields[4])
            footer=run('capture-pane','-p','-t',target,'-S',str(max(0,height-12)),'-E',str(height-1)).decode(errors='replace')
            state=activity_from_screen(footer)
        print(json.dumps({'alive':fields[1]=='0','pid':int(fields[0]),'command':fields[2],'state':state}))
    elif action=='capture':
        sys.stdout.buffer.write(run('capture-pane','-p','-t',target,'-S','-100'))
    elif action=='paste':
        text=req['text'].encode()
        if len(text)>1024*1024:raise ValueError('Paste exceeds 1 MiB')
        name='starmux-'+os.urandom(8).hex()
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
        root=pathlib.Path(os.environ.get('XDG_DATA_HOME',os.path.expanduser('~/.local/share')))/'starmux'/'attachments'/workspace/pane
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
