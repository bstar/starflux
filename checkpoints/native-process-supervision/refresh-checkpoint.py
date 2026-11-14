#!/usr/bin/env python3
"""Save the isolated candidate and verified private evidence, never install it."""
from pathlib import Path
import difflib
import hashlib
import io
import json
import shutil
import subprocess
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
CANDIDATE = ROOT / 'target/inline-graphics-workspace'
OUT = Path(__file__).resolve().parent
BASE = '72d017696'
FOLDERS = ['src', 'terminal-client/src', 'examples', 'vendor/vt100/src']
FIXTURES = ['test-native-process-tree.py', 'test-native-shell-supervision.py',
            'test-native-tree-older-owner.py', 'test-native-tree-views.py',
            'run-native-tree-candidate']
LOGS = ['clippy', 'unit', 'minimal', 'build', 'loss-local', 'loss-ssh',
        'tree-ssh', 'ui-local', 'ui-ssh', 'ui-cells', 'ui-ssh-cells',
        'older-local', 'older-ssh', 'shell']


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def files(tree, folder):
    return {p.relative_to(tree) for p in (tree / folder).rglob('*')
            if p.is_file() and '__pycache__' not in p.parts
            and p.suffix not in ('.pyc', '.pyo')}


def patch(left, right, paths):
    result, identities = [], {}
    for relative in sorted(paths):
        a, b = left / relative, right / relative
        before = a.read_text() if a.exists() else ''
        after = b.read_text() if b.exists() else ''
        if before == after:
            continue
        identities[str(relative)] = {'before': digest(a) if a.exists() else None,
                                     'after': digest(b) if b.exists() else None}
        result.append(f'diff --git a/{relative} b/{relative}\n')
        if not a.exists():
            result.append('new file mode 100644\n')
        if not b.exists():
            result.append('deleted file mode 100644\n')
        result.extend(difflib.unified_diff(
            before.splitlines(True), after.splitlines(True),
            fromfile=f'a/{relative}' if a.exists() else '/dev/null',
            tofile=f'b/{relative}' if b.exists() else '/dev/null'))
    return ''.join(result), identities


def main():
    evidence = {}
    for name in LOGS:
        log = ROOT / 'target' / f'native-tree-resumed-{name}.log'
        text = log.read_text()
        assert 'Traceback' not in text and 'test result: FAILED' not in text, name
        assert 'could not compile' not in text, name
        if name in ('clippy', 'build'):
            assert 'Finished `' in text, name
        elif name in ('unit', 'minimal'):
            assert 'test result: ok.' in text, name
        else:
            assert any(json.loads(line).get('result') == 'PASS'
                       for line in text.splitlines() if line.startswith('{')), name
        evidence[name] = {'sha256': digest(log), 'mtime': log.stat().st_mtime}
    original = ROOT / 'checkpoints/native-terminal-candidate/native-adapter-candidate.patch'
    old_receipt = json.loads((original.parent / 'native-adapter-candidate-receipt.json').read_text())
    with tempfile.TemporaryDirectory(prefix='starflux-supervision-checkpoint-') as directory:
        tree = Path(directory)
        archive = subprocess.check_output(['git', 'archive', BASE, *FOLDERS,
                                          'Cargo.toml', 'terminal-client/Cargo.toml'], cwd=ROOT)
        with tarfile.open(fileobj=io.BytesIO(archive)) as package:
            package.extractall(tree, filter='data')
        subprocess.run(['git', 'apply', str(original)], cwd=tree, check=True, capture_output=True)
        for relative, expected in old_receipt['files'].items():
            assert digest(tree / relative) == expected, relative
        paths = set().union(*(files(tree, f) | files(CANDIDATE, f) for f in FOLDERS))
        delta, identities = patch(tree, CANDIDATE, paths)
        (OUT / 'source.patch').write_text(delta)
        subprocess.run(['git', 'apply', '--check', str(OUT / 'source.patch')],
                       cwd=tree, check=True, capture_output=True)
        layout, _ = patch(tree, CANDIDATE, {Path('Cargo.toml'), Path('terminal-client/Cargo.toml')})
        (OUT / 'isolated-build-layout.patch').write_text(layout)
    for name in FIXTURES:
        shutil.copy2(ROOT / 'target' / name, OUT / name)
    baseline_lock = subprocess.check_output(['git', 'show', BASE + ':Cargo.lock'], cwd=ROOT)
    assert (CANDIDATE / 'Cargo.lock').read_bytes() == baseline_lock
    binary = ROOT / 'target/native-supervision-build/release/starflux-graphics'
    assert binary.stat().st_mtime >= max((CANDIDATE / f).stat().st_mtime for f in identities)
    safety_path = ROOT / 'target/native-tree-resumed-live-safety.json'
    safety = json.loads(safety_path.read_text())
    assert safety['installed_binary_unchanged'] and not safety['gone_or_changed']
    before = json.loads((ROOT / 'target/native-tree-resumed-live-before.json').read_text())
    assert digest(ROOT / 'target/release/starflux') == before['binary_sha256']
    for process in safety['preserved']:
        stat = Path('/proc', str(process['pid']), 'stat').read_text()
        assert int(stat.rsplit(')', 1)[1].split()[19]) == process['start_ticks']
    receipt = dict(
        saved_at=time.time(), canonical_base_commit=subprocess.check_output(
            ['git', 'rev-parse', BASE], cwd=ROOT, text=True).strip(),
        previous_candidate_patch_sha256=digest(original),
        source_patch_sha256=digest(OUT / 'source.patch'), files=identities,
        fixtures={name: digest(OUT / name) for name in FIXTURES},
        validation_evidence=evidence, candidate_binary_sha256=digest(binary),
        candidate_lock_sha256=digest(CANDIDATE / 'Cargo.lock'),
        live_safety_sha256=digest(safety_path),
        original_processes_preserved=len(safety['preserved']),
        installed_binary_unchanged=True, shipping_activation=False,
        supervision_runtime_verified=True, guardian_loss_fails_closed=True,
        foreground_command_and_cwd_verified=True, idle_signal_wait_verified=True,
        owner_loss_and_never_delivered_recovery_complete=False,
        rich_terminal_metadata_complete=False, canonical_promotion=False)
    (OUT / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(dict(source_files=len(identities), patch_bytes=len(delta.encode()),
                         validation_logs=len(evidence), candidate_not_installed=True)))


if __name__ == '__main__':
    main()
