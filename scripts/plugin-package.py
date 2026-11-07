"""Fetch a pinned Git tree into private quarantine; never execute package code."""
import hashlib,json,os,resource,shutil,stat,subprocess,sys,tarfile,tempfile,time
from pathlib import PurePosixPath

PACKAGE_LIMIT=64*1024*1024
STAGING_LIMIT=256*1024*1024

def main(request):
    stage=request['stage'];root=os.open(stage,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
    identity=os.fstat(root)
    def current():
        try:s=os.stat(stage);return (s.st_dev,s.st_ino)==(identity.st_dev,identity.st_ino)
        except OSError:return False
    def budget():
        if not current():raise RuntimeError('Package staging directory changed')
        size=count=0
        for parent,dirs,files in os.walk(stage,followlinks=False):
            count+=len(dirs)+len(files)
            for name in files:
                try:size+=os.lstat(os.path.join(parent,name)).st_size
                except FileNotFoundError:pass
            if size>STAGING_LIMIT or count>20000:raise RuntimeError('Git source exceeds the staging limit (256 MiB / 20000 entries)')
    def limits():resource.setrlimit(resource.RLIMIT_FSIZE,(PACKAGE_LIMIT,PACKAGE_LIMIT))
    base=['git','-c','core.hooksPath=/dev/null','-c','core.fsmonitor=false','-c','protocol.allow=never','-c','protocol.https.allow=always','-c','protocol.ssh.allow=always','-c','protocol.file.allow=always','-c','protocol.ext.allow=never','-c','gc.auto=0']
    env=dict(os.environ,GIT_TERMINAL_PROMPT='0',GIT_LFS_SKIP_SMUDGE='1')
    env.setdefault('GIT_SSH_COMMAND','ssh -oBatchMode=yes -oConnectTimeout=10 -oStrictHostKeyChecking=yes')
    def git(args,cwd,extra=()):
        with tempfile.TemporaryFile() as out,tempfile.TemporaryFile() as err:
            proc=subprocess.Popen(base+args,cwd='/proc/self/fd/'+str(cwd),pass_fds=(root,cwd,*extra),env=env,stdin=subprocess.DEVNULL,stdout=out,stderr=err,preexec_fn=limits)
            try:
                while proc.poll() is None:
                    budget()
                    if os.fstat(out.fileno()).st_size>2*1024*1024 or os.fstat(err.fileno()).st_size>2*1024*1024:raise RuntimeError('Git diagnostics exceeded their limit')
                    time.sleep(.05)
                budget()
                if proc.returncode:raise RuntimeError('Git '+args[0]+' failed (exit '+str(proc.returncode)+'); check the repository, version and existing Git authentication')
                out.seek(0);return out.read(2*1024*1024)
            finally:
                if proc.poll() is None:proc.terminate();proc.wait(timeout=3)
    git(['init','--bare','--quiet','--template=','repository'],root)
    repository=os.open('repository',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=root)
    git(['fetch','--quiet','--depth=1','--no-tags','--',request['repository'],request['reference']],repository)
    commit=git(['rev-parse','--verify','FETCH_HEAD^{commit}'],repository).decode().strip()
    archive=os.open('package.tar',os.O_RDWR|os.O_CREAT|os.O_EXCL,0o600,dir_fd=root)
    args=['archive','--format=tar','--output=/proc/self/fd/'+str(archive),commit]
    prefix=PurePosixPath(request.get('subdirectory') or '').parts
    if prefix:args+=['--','/'.join(prefix)]
    git(args,repository,(archive,))
    if os.fstat(archive).st_size>PACKAGE_LIMIT:raise RuntimeError('Plugin package exceeds 64 MiB')
    os.mkdir('package',mode=0o700,dir_fd=root)
    payload=os.open('package',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=root)
    def directory(parts):
        fd=os.dup(payload)
        try:
            for part in parts:
                try:os.mkdir(part,mode=0o700,dir_fd=fd);os.fsync(fd)
                except FileExistsError:pass
                next_fd=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=fd);os.close(fd);fd=next_fd
            return fd
        except BaseException:os.close(fd);raise
    links=[];total=count=0;seen=set()
    with os.fdopen(os.dup(archive),'rb') as data,tarfile.open(fileobj=data,mode='r:') as tar:
        for member in tar:
            budget();parts=PurePosixPath(member.name).parts
            if member.name.startswith('/') or any(p in ('.','..') or p.lower()=='.git' or any(ord(c)<32 or 0xD800<=ord(c)<=0xDFFF for c in p) for p in parts):raise RuntimeError('Git tree contains an unsafe package path')
            if prefix:
                if member.isdir() and prefix[:len(parts)]==parts:continue
                if parts[:len(prefix)]!=prefix:raise RuntimeError('Git archive escaped the selected subdirectory')
                parts=parts[len(prefix):]
            if not parts:continue
            if len(member.name)>1024 or len(parts)>64 or parts in seen:raise RuntimeError('Duplicate or oversized package path')
            seen.add(parts);count+=1
            if count>8192:raise RuntimeError('Plugin package exceeds 8192 entries')
            if member.isdir():os.close(directory(parts));continue
            parent=directory(parts[:-1])
            try:
                if member.issym():links.append((parts,member.linkname));continue
                if not member.isfile() or member.size<0:raise RuntimeError('Plugin package contains an unsupported special file')
                total+=member.size
                if total>PACKAGE_LIMIT:raise RuntimeError('Plugin package exceeds 64 MiB')
                fd=os.open(parts[-1],os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o700 if member.mode&0o111 else 0o600,dir_fd=parent)
                with os.fdopen(fd,'wb') as target,tar.extractfile(member) as source:
                    remaining=member.size
                    while remaining:
                        chunk=source.read(min(65536,remaining))
                        if not chunk:raise RuntimeError('Incomplete Git archive')
                        target.write(chunk);remaining-=len(chunk)
                    target.flush();os.fsync(target.fileno())
            finally:os.fsync(parent);os.close(parent)
    # Create symlinks only after regular files. Their targets must remain inside
    # this package, including through other symlinks; no submodules are fetched.
    from pathlib import Path
    package=Path(stage)/'package'
    for parts,target in links:
        if os.path.isabs(target):raise RuntimeError('Absolute plugin symlink is not allowed')
        parent=directory(parts[:-1])
        try:os.symlink(target,parts[-1],dir_fd=parent)
        finally:os.fsync(parent);os.close(parent)
    for parts,_ in links:
        resolved=package.joinpath(*parts).resolve(strict=False)
        if not resolved.is_relative_to(package.resolve()) or '.git' in resolved.relative_to(package.resolve()).parts:raise RuntimeError('Plugin symlink escapes its package')
    os.fsync(payload);os.close(payload);os.close(archive);os.close(repository)
    if not current():raise RuntimeError('Package staging directory changed')
    # Publish no Git checkout, hooks, credential config or build artifacts.
    shutil.rmtree(os.path.join(stage,'repository'));os.unlink('package.tar',dir_fd=root)
    os.fsync(root);os.close(root)
    return dict(commit=commit)

if __name__=='__main__':
    try:result=main(json.loads(sys.argv[1]))
    except Exception as error:result=dict(error=str(error))
    print(json.dumps(result),flush=True)
