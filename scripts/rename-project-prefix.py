#!/usr/bin/env python3
"""Rename the prefix of saved project and collection IDs, keeping live terminals.

    rename-project-prefix.py OLD NEW            # dry run: report what would change
    rename-project-prefix.py OLD NEW --apply    # back up, then rename
    rename-project-prefix.py --restore MANIFEST # undo an applied rename

Every `OLD-` in STAR/FLUX's data, runtime and config files (names and text
contents) becomes `NEW-`, and an `"OLD"` JSON key becomes `"agent_mux"`.
tmux servers named `starflux-OLD-*` keep running: only their socket files are
renamed. Run it while no STAR/FLUX view, relay or App Space desktop uses an
OLD project; the snapshot worker is stopped (it restarts with STAR/FLUX).
A backup tarball and a manifest are written before anything changes.
"""
import argparse
import json
import os
import re
import signal
import tarfile
import time
from pathlib import Path


def roots():
    home = Path.home()
    data = Path(os.environ.get('XDG_DATA_HOME') or home / '.local/share') / 'starflux'
    config = Path(os.environ.get('XDG_CONFIG_HOME') or home / '.config') / 'starflux'
    runtime = Path(os.environ.get('XDG_RUNTIME_DIR') or f'/run/user/{os.getuid()}') / 'starflux'
    tmux = Path(os.environ.get('TMUX_TMPDIR') or '/tmp') / f'tmux-{os.getuid()}'
    return dict(data=data, config=config, runtime=runtime), tmux


SKIP = {'browser-profiles'}  # browser state never names projects; too large to rewrite


def walk(root):
    for directory, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP] if Path(directory) == root else dirs
        for name in files + dirs:
            yield Path(directory) / name


def blockers(old, runtime):
    """Processes that hold an OLD project ID and would not survive the rename."""
    found, snapshot = [], []
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit() or int(proc.name) == os.getpid():
            continue
        try:
            argv = (proc / 'cmdline').read_bytes().split(b'\0')
        except OSError:
            continue
        words = [w.decode(errors='replace') for w in argv if w]
        if not words:
            continue
        line = ' '.join(words)
        program = Path(words[0]).name
        if '--snapshot-worker' in words:
            if str(runtime) not in words:
                continue  # another runtime's worker
            snapshot.append(int(proc.name))
        elif program in ('starflux', 'exe') and f'{old}-' in line:
            found.append((int(proc.name), line[:160]))
        elif program == 'tmux' and f'starflux-{old}-' in line and 'new-session' not in words:
            found.append((int(proc.name), line[:160]))  # an attached client, owned by a view
    return found, snapshot


def plan(old, new):
    paths, tmux = roots()
    token, key = f'{old}-'.encode(), f'"{old}"'.encode()
    edits, skipped, renames = [], [], []
    for label, root in paths.items():
        if not root.exists():
            continue
        for path in walk(root):
            if path.is_file() and not path.is_symlink() and not path.is_socket():
                data = path.read_bytes()
                if token in data or key in data:
                    try:
                        data.decode()
                        edits.append(path)
                    except UnicodeDecodeError:
                        skipped.append(path)
            if token.decode() in path.name:
                renames.append(path)
    sockets = sorted(p for p in tmux.glob(f'starflux-{old}-*') if p.is_socket()) if tmux.exists() else []
    renames.sort(key=lambda p: len(p.parts), reverse=True)  # children before parents
    return paths, edits, skipped, renames, sockets


def renamed(path, old, new):
    return path.with_name(path.name.replace(f'{old}-', f'{new}-'))


def apply(old, new, edits, renames, sockets, paths, snapshot):
    stamp = time.strftime('%Y%m%dT%H%M%S')
    backup_dir = paths['data'].parent / f'starflux-rename-{stamp}'
    backup_dir.mkdir(mode=0o700)
    tar_path = backup_dir / 'backup.tar'
    with tarfile.open(tar_path, 'w') as tar:
        for label, root in paths.items():
            if root.exists():
                tar.add(root, arcname=label, filter=lambda info: None if info.name.split('/')[1:2] == ['browser-profiles'] else info)
    for pid in snapshot:
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    manifest = dict(old=old, new=new, backup=str(tar_path), roots={k: str(v) for k, v in paths.items()},
                    sockets=[], files=[], renames=[])
    (backup_dir / 'manifest.json').write_text(json.dumps(manifest, indent=1))
    token, key = f'{old}-'.encode(), f'"{old}"'.encode()
    for path in edits:
        data = path.read_bytes().replace(token, f'{new}-'.encode()).replace(key, b'"agent_mux"')
        tmp = path.with_name(path.name + '.rename-tmp')
        tmp.write_bytes(data)
        os.chmod(tmp, path.stat().st_mode & 0o7777)
        os.replace(tmp, path)
        manifest['files'].append(str(path))
    for path in renames:
        target = renamed(path, old, new)
        if target.exists():
            raise SystemExit(f'Refusing to overwrite {target}')
        os.rename(path, target)
        manifest['renames'].append([str(path), str(target)])
    for socket in sockets:
        target = renamed(socket, old, new)
        os.rename(socket, target)
        manifest['sockets'].append([str(socket), str(target)])
    (backup_dir / 'manifest.json').write_text(json.dumps(manifest, indent=1))
    return backup_dir


def restore(manifest_path):
    manifest = json.loads(Path(manifest_path).read_text())
    for before, after in manifest['sockets']:
        if Path(after).exists():
            os.rename(after, before)
    stamp = time.strftime('%Y%m%dT%H%M%S')
    with tarfile.open(manifest['backup']) as tar:
        for label, root in manifest['roots'].items():
            root = Path(root)
            members = [m for m in tar.getmembers() if m.name == label or m.name.startswith(label + '/')]
            if not members:
                continue
            if root.exists():
                aside = root.with_name(root.name + f'.renamed-{stamp}')
                os.rename(root, aside)
                kept = aside / 'browser-profiles'
            else:
                kept = None
            for m in members:
                m.name = root.name + m.name[len(label):]
            tar.extractall(root.parent, members=members, filter='tar')
            if kept and kept.exists():
                os.rename(kept, root / 'browser-profiles')
    print(json.dumps({'restored': True, 'sockets': len(manifest['sockets'])}))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('old', nargs='?')
    parser.add_argument('new', nargs='?')
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--restore', metavar='MANIFEST')
    args = parser.parse_args()
    if args.restore:
        return restore(args.restore)
    if not args.old or not args.new:
        parser.error('OLD and NEW are required')
    for value in (args.old, args.new):
        if not re.fullmatch(r'[A-Za-z0-9_]+', value):
            parser.error(f'{value!r} must be letters, digits or underscores')
    paths, edits, skipped, renames, sockets = plan(args.old, args.new)
    found, snapshot = blockers(args.old, paths['runtime'])
    report = dict(files_to_rewrite=len(edits), names_to_rename=len(renames), tmux_sockets=len(sockets),
                  binary_files_skipped=[str(p) for p in skipped], blocking_processes=found,
                  snapshot_workers=len(snapshot), applied=False)
    if args.apply:
        if found:
            print(json.dumps(report, indent=1))
            raise SystemExit('Close the STAR/FLUX windows, views and desktops listed above, then retry.')
        report['backup'] = str(apply(args.old, args.new, edits, renames, sockets, paths, snapshot))
        report['applied'] = True
    print(json.dumps(report, indent=1))


if __name__ == '__main__':
    main()
