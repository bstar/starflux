#!/usr/bin/env python3
"""Archive the recovered conversation fix against the immutable rebased stage."""
from pathlib import Path
import difflib, hashlib, io, json, shutil, subprocess, tarfile, tempfile, time

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
TREE = ROOT / 'target/native-rebase-workspace'

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    previous = ROOT / 'checkpoints/native-rebased'
    receipt = json.loads((previous / 'receipt.json').read_text())
    paths = ['src/agent_state.rs', 'src/native_backend.rs']
    folders = ['src', 'terminal-client/src', 'examples', 'vendor/vt100/src']
    logs = ['final-clippy', 'final-unit', 'final-minimal', 'final-build',
            'final-cold-local', 'final-cold-ssh-cells',
            'final-legacy-local', 'final-legacy-ssh-cells']
    evidence = {}
    for name in logs:
        path = ROOT / 'target' / f'native-conversation-recovery-{name}.log'
        text = path.read_text()
        assert not any(bad in text for bad in ['Traceback', 'test result: FAILED', 'could not compile']), name
        if name in ['final-clippy', 'final-build']:
            assert 'Finished `release`' in text, name
        elif name in ['final-unit', 'final-minimal']:
            assert 'test result: ok.' in text, name
        else:
            assert any(json.loads(line).get('result') == 'PASS'
                       for line in text.splitlines() if line.startswith('{')), name
        evidence[name] = dict(sha256=digest(path), mtime=path.stat().st_mtime)
    with tempfile.TemporaryDirectory(prefix='sf-conversation-reconstruction-') as directory:
        base = Path(directory)
        archive = subprocess.check_output(['git', 'archive', receipt['canonical_base_commit']], cwd=ROOT)
        with tarfile.open(fileobj=io.BytesIO(archive)) as package:
            package.extractall(base, filter='data')
        for name in ['canonical-ui.patch', 'source.patch']:
            subprocess.run(['git', 'apply', str(previous / name)], cwd=base, check=True, capture_output=True)
        changed = []
        for folder in folders:
            for path in (TREE / folder).rglob('*'):
                if not path.is_file(): continue
                relative = str(path.relative_to(TREE))
                if not (base / relative).exists() or digest(path) != digest(base / relative):
                    changed.append(relative)
        assert sorted(changed) == sorted(paths), changed
        delta = ''.join(''.join(difflib.unified_diff(
            (base / path).read_text().splitlines(keepends=True),
            (TREE / path).read_text().splitlines(keepends=True),
            fromfile='a/' + path, tofile='b/' + path)) for path in paths)
        (OUT / 'source.patch').write_text(delta)
        subprocess.run(['git', 'apply', str(OUT / 'source.patch')], cwd=base, check=True, capture_output=True)
        for path in paths: assert digest(base / path) == digest(TREE / path), path
        fixtures = ['scripts/test-session-manager.py', 'scripts/test-agent-activity.py']
        fixture_patch = subprocess.check_output(['git', 'diff', '--binary', receipt['canonical_base_commit'], '--', *fixtures], cwd=ROOT)
        (OUT / 'fixtures.patch').write_bytes(fixture_patch)
        subprocess.run(['git', 'apply', str(OUT / 'fixtures.patch')], cwd=base, check=True, capture_output=True)
        for path in fixtures: assert digest(base / path) == digest(ROOT / path), path
    state = json.loads((ROOT / 'target/native-release-gate-state.json').read_text())
    assert digest(ROOT / 'target/release/starflux') == state['installed_binary_sha256']
    for process in state['original_processes']:
        fields = Path('/proc', str(process['pid']), 'stat').read_text().rsplit(')', 1)[1].split()
        assert int(fields[19]) == process['start_ticks'] and fields[0] != 'Z'
    assert (TREE / 'Cargo.lock').read_bytes() == (ROOT / 'Cargo.lock').read_bytes()
    fixture = ROOT / 'target/test-native-conversation-recovery.py'
    shutil.copy2(fixture, OUT / fixture.name)
    binary = ROOT / 'target/native-rebase-build/release/starflux-native-rebased'
    result = dict(saved_at=time.time(), previous_source_patch_sha256=digest(previous / 'source.patch'),
                  source_patch_sha256=digest(OUT / 'source.patch'), files={p:digest(TREE / p) for p in paths},
                  fixtures_patch_sha256=digest(OUT / 'fixtures.patch'), validation_evidence=evidence,
                  candidate_binary_sha256=digest(binary), canonical_lock_unchanged=True,
                  original_processes_preserved=len(state['original_processes']), installed_binary_unchanged=True,
                  canonical_source_promotion=False, shipping_activation=False, full_parity_complete=False)
    (OUT / 'receipt.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(dict(source_files=len(paths), validation_logs=len(evidence), shipping_activation=False)))

if __name__ == '__main__': main()
