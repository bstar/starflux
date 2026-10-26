#!/usr/bin/env python3
"""Read-only repository snapshot; also sent verbatim to the project SSH host."""
import json
import os
from pathlib import Path
import subprocess
import sys


def snapshot(directory):
    def git(*args, required=True):
        result = subprocess.run(
            ['git', '--no-optional-locks', '-C', directory, *args],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env=dict(os.environ, LC_ALL='C', GIT_OPTIONAL_LOCKS='0'), timeout=8)
        if required and result.returncode:
            raise RuntimeError(result.stderr.decode('utf-8', 'replace').strip()[:300])
        return result.stdout if result.returncode == 0 else b''

    root = git('rev-parse', '--show-toplevel', required=False).decode('utf-8', 'replace').strip()
    if not root:
        # Distinguish an ordinary directory from an inaccessible host/path or missing Git.
        result = subprocess.run(['git', '--no-optional-locks', '-C', directory, 'rev-parse', '--is-inside-work-tree'],
                                capture_output=True, timeout=8, env=dict(os.environ, LC_ALL='C'))
        error = result.stderr.decode('utf-8', 'replace').strip()
        if 'not a git repository' in error:
            return {'kind': 'none'}
        return {'kind': 'error', 'error': error[:300] or 'Not a Git working tree'}

    raw = git('status', '--porcelain=v2', '-z', '--branch', '--show-stash', '--untracked-files=all')
    entries = iter(raw.split(b'\0'))
    headers, files = {}, []
    staged = unstaged = untracked = conflicts = 0
    for entry in entries:
        if entry.startswith(b'# '):
            key, _, value = entry[2:].partition(b' ')
            headers[key.decode()] = value.decode('utf-8', 'replace')
        elif entry.startswith((b'1 ', b'2 ', b'u ')):
            kind = entry[:1]
            fields = entry.split(b' ', {b'1': 8, b'2': 9, b'u': 10}[kind])
            xy = fields[1].decode()
            path = fields[-1].decode('utf-8', 'replace')
            original = next(entries, b'').decode('utf-8', 'replace') if kind == b'2' else None
            conflict = kind == b'u'
            conflicts += conflict
            staged += not conflict and xy[0] != '.'
            unstaged += not conflict and xy[1] != '.'
            files.append({'path': path, 'original': original, 'index': xy[0], 'worktree': xy[1], 'conflict': conflict})
        elif entry.startswith(b'? '):
            untracked += 1
            files.append({'path': entry[2:].decode('utf-8', 'replace'), 'index': '?', 'worktree': '?', 'conflict': False})

    def stats(*args):
        data = git('diff', '--no-ext-diff', '--no-textconv', '--numstat', '-z', *args).split(b'\0')
        additions = deletions = binary = 0
        entries = iter(data)
        for entry in entries:
            if not entry:
                continue
            a, d, name = entry.split(b'\t', 2)
            if not name:  # NUL-delimited rename: two extra path fields.
                next(entries, None)
                next(entries, None)
            if a == b'-':
                binary += 1
            else:
                additions += int(a)
                deletions += int(d)
        return {'additions': additions, 'deletions': deletions, 'binary': binary}

    oid = headers.get('branch.oid', '')
    branch = headers.get('branch.head', '')
    if branch == '(detached)':
        branch = 'detached @ ' + oid[:8]
    ahead, behind = headers.get('branch.ab', '+0 -0').split()
    operation = ''
    for name, label in [('rebase-merge', 'REBASING'), ('rebase-apply', 'REBASING'), ('MERGE_HEAD', 'MERGING'), ('CHERRY_PICK_HEAD', 'CHERRY-PICK'), ('REVERT_HEAD', 'REVERTING'), ('BISECT_LOG', 'BISECTING')]:
        path = git('rev-parse', '--git-path', name).decode('utf-8', 'replace').strip()
        if (Path(directory) / path).exists():
            operation = label
            break
    last = git('log', '-1', '--format=%h %s', required=False).decode('utf-8', 'replace').strip()
    return {'kind': 'repo', 'root': root, 'branch': branch, 'upstream': headers.get('branch.upstream', ''),
            'ahead': int(ahead), 'behind': abs(int(behind)), 'unborn': oid == '(initial)',
            'staged': staged, 'unstaged': unstaged, 'untracked': untracked, 'conflicts': conflicts,
            'stash': int(headers.get('stash', '0')), 'operation': operation, 'last_commit': last,
            'staged_lines': stats('--cached'), 'unstaged_lines': stats(),
            'files': files[:1000], 'total_files': len(files), 'truncated': len(files) > 1000}


if __name__ == '__main__':
    try:
        result = snapshot(sys.argv[1])
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        result = {'kind': 'error', 'error': str(error)[:300]}
    print(json.dumps(result))
