#!/usr/bin/env python3
"""Real Git worktrees; all files and processes belong to a private fixture."""
import base64,json,os
from pathlib import Path
import subprocess,tempfile

helper=(Path(__file__).resolve().parent/'git-worktrees.py').read_text()
with tempfile.TemporaryDirectory(prefix='sf-worktrees-') as directory:
    tmp=Path(directory);repo=tmp/'source repo 界';repo.mkdir()
    def git(*args,cwd=repo):return subprocess.check_output(['git','-C',str(cwd),*args],stderr=subprocess.PIPE).decode().strip()
    def call(action,ok=True,directory=repo,**params):
        value=json.loads(subprocess.check_output(['python3','-c',helper,base64.b64encode(json.dumps(dict(action=action,directory=str(directory),**params)).encode()).decode()],cwd='/'))
        assert value['ok']==ok,value
        return value['result'] if ok else value['error']
    def tree(path):return next(t for t in call('list')['worktrees'] if t['path']==str(path))
    git('init','-b','main');git('config','user.name','Private fixture');git('config','user.email','fixture@example.invalid')
    (repo/'file').write_text('original\n');(repo/'.gitignore').write_text('build/\n');git('add','.');git('commit','-m','initial')
    (repo/'private draft').write_text('keep exactly')
    initial=call('list');assert len(initial['worktrees'])==1 and initial['worktrees'][0]['main']
    assert call('remove',target=initial['worktrees'][0],ok=False)['code']=='primary_worktree'
    created=call('create',branch='feature/one',path=str(tmp/'feature one 界'),base='HEAD')['worktree'];path=Path(created['path'])
    assert created['branch']=='refs/heads/feature/one' and (path/'file').read_text()=='original\n'
    assert (repo/'private draft').read_text()=='keep exactly'
    assert call('create',branch='other',path=str(path),ok=False)['code']=='path_exists'
    assert call('create',branch='-bad',ok=False)['code']=='invalid_branch'
    assert not call('create',branch='main',ok=False).get('ok',False)
    (path/'file').write_text('unsaved')
    assert call('remove',target=tree(path),ok=False)['code']=='dirty_worktree';assert (path/'file').read_text()=='unsaved'
    git('restore','file',cwd=path)
    (path/'build').mkdir();(path/'build'/'artifact').write_text('keep ignored')
    assert call('remove',target=tree(path),ok=False)['code']=='dirty_worktree'
    (path/'build'/'artifact').unlink();(path/'build').rmdir()
    (path/'untracked').write_text('keep untracked');assert call('remove',target=tree(path),ok=False)['code']=='dirty_worktree';(path/'untracked').unlink()
    sleeper=subprocess.Popen(['sleep','20'],cwd=path)
    try:assert call('remove',target=tree(path),ok=False)['code']=='worktree_busy'
    finally:sleeper.terminate();sleeper.wait(timeout=3)
    git('worktree','lock','--reason','keep this',str(path));assert call('remove',target=tree(path),ok=False)['code']=='locked_worktree';git('worktree','unlock',str(path))
    old=tree(path);(path/'file').write_text('committed\n');git('add','file',cwd=path);git('commit','-m','worktree change',cwd=path)
    assert call('remove',target=old,ok=False)['code']=='stale_worktree'
    # Directory validation tolerates normal commits, removal confirmation does not.
    assert call('directory',target=old,directory=path)['worktree']['head']!=old['head']
    assert call('directory',target=old,ok=False)['code']=='stale_worktree'
    current=tree(path);call('remove',target=current);assert not path.exists()
    assert git('show','feature/one:file')=='committed' and (repo/'private draft').read_text()=='keep exactly'
    detached=tmp/'detached';git('worktree','add','--detach',str(detached),'HEAD')
    (detached/'file').write_text('detached new\n');git('add','file',cwd=detached);git('commit','-m','unreferenced',cwd=detached)
    assert call('remove',target=tree(detached),ok=False)['code']=='unreferenced_commit'
    git('branch','retain-detached',cwd=detached);call('remove',target=tree(detached));assert not detached.exists()
    assert len(call('list')['worktrees'])==1
    print(json.dumps(dict(result='PASS',create=True,unicode_paths=True,dirty_source_preserved=True,changed_untracked_ignored_guard=True,live_process_guard=True,locked_and_primary_guard=True,stale_identity_guard=True,detached_commit_guard=True,branches_retained=True)))
