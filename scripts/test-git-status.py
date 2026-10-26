#!/usr/bin/env python3
"""Read-only Git inspector checks using disposable repositories, including odd paths."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('git_status', repo/'scripts/git-status.py')
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)

with tempfile.TemporaryDirectory(prefix='starmux-git-') as temp:
    root = Path(temp)
    def git(*args, check=True):
        return subprocess.run(['git', '-C', str(root), *args], check=check, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    assert helper.snapshot(str(root))['kind'] == 'none'
    git('init', '-b', 'main')
    git('config', 'user.name', 'Inspector test')
    git('config', 'user.email', 'inspector@example.invalid')
    assert helper.snapshot(str(root))['unborn']
    (root/'tracked.txt').write_text('base\n')
    (root/'renamed source.txt').write_text('rename contents\n')
    (root/'binary.dat').write_bytes(b'\x00binary\x00')
    git('add', '.')
    git('commit', '-m', 'initial')
    assert helper.snapshot(str(root))['total_files'] == 0
    (root/'tracked.txt').write_text('base\nstaged\n')
    git('add', 'tracked.txt')
    (root/'tracked.txt').write_text('base\nstaged\nworking\n')
    (root/'binary.dat').write_bytes(b'\x00changed\x00')
    git('mv', 'renamed source.txt', 'renamed target.txt')
    odd = 'new file\nwith tab\tand café.txt'
    (root/odd).write_text('new\n')
    before = (root/'.git/index').read_bytes()
    s = helper.snapshot(str(root))
    assert s['branch'] == 'main' and s['staged'] == 2 and s['unstaged'] == 2 and s['untracked'] == 1, s
    assert s['staged_lines'] == {'additions': 1, 'deletions': 0, 'binary': 0}, s
    assert s['unstaged_lines'] == {'additions': 1, 'deletions': 0, 'binary': 1}, s
    assert any(f['path'] == odd for f in s['files'])
    assert any(f['original'] == 'renamed source.txt' for f in s['files'] if 'original' in f)
    assert before == (root/'.git/index').read_bytes(), 'Background refresh wrote the index'
    git('reset', '--hard', 'HEAD')
    (root/odd).unlink()
    git('checkout', '--detach')
    assert helper.snapshot(str(root))['branch'].startswith('detached @ ')
    git('checkout', 'main')
    git('checkout', '-b', 'other')
    (root/'tracked.txt').write_text('other\n')
    git('commit', '-am', 'other')
    git('checkout', 'main')
    (root/'tracked.txt').write_text('main\n')
    git('commit', '-am', 'main')
    git('merge', 'other', check=False)
    s = helper.snapshot(str(root))
    assert s['conflicts'] == 1 and s['operation'] == 'MERGING', s
    git('merge', '--abort')
    # A locally known remote-tracking ref proves ahead counts without fetching.
    git('update-ref', 'refs/remotes/origin/main', 'HEAD~1')
    git('config', 'branch.main.remote', 'origin')
    git('config', 'branch.main.merge', 'refs/heads/main')
    git('config', 'remote.origin.fetch', '+refs/heads/*:refs/remotes/origin/*')
    s = helper.snapshot(str(root))
    assert s['upstream'] == 'origin/main' and s['ahead'] == 1 and s['behind'] == 0, s
    print(json.dumps({'passed': True, 'staged_and_unstaged': True, 'rename_and_unusual_paths': True,
                      'binary_stats': True, 'unborn_and_detached': True, 'conflicts_and_merge_state': True,
                      'upstream_counts': True, 'index_unchanged_by_inspection': True}))
