"""Bounded worktree operations on the repository host. No shell interpolation."""
import base64,fcntl,json,os,pathlib,re,stat,subprocess,sys

class Refused(Exception):
    def __init__(self,code,message):self.code=code;super().__init__(message)

def git(directory,*args):
    p=subprocess.run(['git','-C',str(directory),*args],stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=25,env={**os.environ,'GIT_TERMINAL_PROMPT':'0','GIT_OPTIONAL_LOCKS':'0'})
    if p.returncode:raise Refused('git_failed',p.stderr.decode(errors='replace').strip()[:2000])
    return p.stdout

def repository(directory):
    common=pathlib.Path(os.fsdecode(git(directory,'rev-parse','--path-format=absolute','--git-common-dir')).strip()).resolve(strict=True)
    st=common.stat()
    return dict(common_dir=str(common),device=st.st_dev,inode=st.st_ino,boot_id=pathlib.Path('/proc/sys/kernel/random/boot_id').read_text().strip())

def entries(directory,repo):
    try:current_path=os.fsdecode(git(directory,'rev-parse','--show-toplevel')).strip()
    except Refused:current_path=''
    records=[];current=None
    for field in git(directory,'worktree','list','--porcelain','-z').split(b'\0'):
        if not field:continue
        key,_,value=field.partition(b' ');key=key.decode()
        if key=='worktree':
            current=dict(path=str(pathlib.Path(os.fsdecode(value)).resolve()),head='',branch='',detached=False,bare=False,locked=None,prunable=None);records.append(current)
        elif current is not None:
            if key=='HEAD':current['head']=value.decode()
            elif key=='branch':current['branch']=value.decode()
            elif key in ('detached','bare'):current[key]=True
            elif key in ('locked','prunable'):current[key]=os.fsdecode(value)
    for i,row in enumerate(records):
        path=pathlib.Path(row['path'])
        row['main']=i==0 or row['bare']
        try:
            st=path.stat();row['device']=st.st_dev;row['inode']=st.st_ino;row['available']=stat.S_ISDIR(st.st_mode)
        except FileNotFoundError:row['device']=None;row['inode']=None;row['available']=False
        row['repository']=repo
        row['current']=row['path']==current_path
    return records

def matches(expected,actual):
    return all(expected.get(k)==actual.get(k) for k in ['repository','path','head','branch','detached','bare','main','locked','prunable','device','inode','available'])

def inside(path,root):
    try:path.relative_to(root);return True
    except ValueError:return False

def busy(path):
    users=[]
    for process in pathlib.Path('/proc').iterdir():
        if not process.name.isdigit() or int(process.name)==os.getpid():continue
        try:
            if process.stat().st_uid!=os.getuid():continue
            links=[process/'cwd',process/'exe']
            try:links+=list((process/'fd').iterdir())
            except OSError:pass
            for link in links:
                try:target=os.readlink(link)
                except OSError:continue
                if target.startswith('/') and inside(pathlib.Path(target.removesuffix(' (deleted)')),path):
                    users.append(dict(pid=int(process.name),command=(process/'comm').read_text().strip()));break
        except (OSError,ValueError):continue
    return users[:16]

def main(req):
    action=req.get('action');directory=pathlib.Path(req['directory']).expanduser().resolve(strict=True)
    repo=repository(directory)
    lock=pathlib.Path(repo['common_dir'])/'starflux-worktrees.lock'
    fd=os.open(lock,os.O_CREAT|os.O_RDWR|os.O_NOFOLLOW,0o600)
    with os.fdopen(fd,'a') as locked:
        fcntl.flock(locked,fcntl.LOCK_EX)
        rows=entries(directory,repo)
        if action=='list':return dict(repository=repo,worktrees=rows)
        if action=='create':
            branch=req['branch']
            if not isinstance(branch,str) or not branch or branch.startswith('-') or len(branch.encode())>200:
                raise Refused('invalid_branch','Choose a valid new branch name')
            git(directory,'check-ref-format','refs/heads/'+branch)
            base=req.get('base') or 'HEAD'
            commit=git(directory,'rev-parse','--verify','--end-of-options',base+'^{commit}').decode().strip()
            if not re.fullmatch(r'[0-9a-f]{40,64}',commit):raise Refused('invalid_base','Base is not a commit')
            primary=pathlib.Path(rows[0]['path'])
            proposed=req.get('path') or str(primary.parent/(primary.name+'-'+re.sub(r'[^A-Za-z0-9._-]+','-',branch)[:80]))
            raw=pathlib.Path(proposed).expanduser()
            if not raw.is_absolute():raw=directory/raw
            if raw.exists() or raw.is_symlink():raise Refused('path_exists','Worktree destination already exists')
            path=raw.resolve();parent=path.parent
            if not parent.is_dir():raise Refused('invalid_path','Choose an existing parent directory')
            if inside(path,pathlib.Path(repo['common_dir'])):raise Refused('invalid_path','A worktree cannot be created inside Git metadata')
            git(directory,'worktree','add','-b',branch,'--',str(path),commit)
            created=next((r for r in entries(directory,repo) if r['path']==str(path)),None)
            if not created:raise Refused('outcome_unknown','Git completed without returning the new worktree; refresh the list')
            return dict(repository=repo,worktree=created)
        if action not in ('validate','directory','remove'):raise Refused('invalid_action','Unknown worktree operation')
        expected=req['target'];row=next((r for r in rows if r['path']==expected.get('path')),None)
        if action=='directory':
            if row is None or any(row.get(k)!=expected.get(k) for k in ['repository','path','device','inode','available']) or not row['available'] or not inside(directory,pathlib.Path(row['path'])):
                raise Refused('stale_worktree','Worktree directory identity changed; reopen it from the repository picker')
            return dict(repository=repo,worktree=row)
        if row is None or not matches(expected,row):raise Refused('stale_worktree','Worktree identity changed; refresh the list')
        if action=='validate':
            if not row['available']:raise Refused('unavailable','Worktree directory is unavailable')
            return dict(repository=repo,worktree=row)
        if row['main'] or row['bare']:raise Refused('primary_worktree','The primary/bare worktree cannot be removed')
        if row['locked'] is not None:raise Refused('locked_worktree','Worktree is locked: '+row['locked'])
        if not row['available']:raise Refused('unavailable','Missing worktree metadata needs explicit Git repair/pruning')
        path=pathlib.Path(row['path'])
        if git(path,'status','--porcelain=v1','-z','--untracked-files=all','--ignored=matching'):
            raise Refused('dirty_worktree','Worktree has changed, untracked or ignored files; preserve or remove them before removal')
        if row['detached'] and not git(path,'for-each-ref','--contains',row['head'],'--format=%(refname)').strip():
            raise Refused('unreferenced_commit','Detached HEAD is not retained by a named ref; create a branch before removal')
        users=busy(path)
        if users:raise Refused('worktree_busy','Programs still use this worktree: '+', '.join(str(p['pid'])+' '+p['command'] for p in users))
        # Git performs its own final cleanliness/lock checks. Never force,
        # reset, stash, delete a branch or recursively delete files ourselves.
        git(directory,'worktree','remove','--',str(path))
        return dict(repository=repo,removed=row)

try:
    request=json.loads(base64.b64decode(sys.argv[1],validate=True))
    print(json.dumps(dict(ok=True,result=main(request))))
except Refused as e:print(json.dumps(dict(ok=False,error=dict(code=e.code,message=str(e)))))
except Exception as e:print(json.dumps(dict(ok=False,error=dict(code='host_error',message=str(e)[:2000]))))
