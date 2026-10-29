#!/usr/bin/env python3
"""Check durable/concurrent AppImage relocation using a fake CLI, without a GUI."""
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix='starflux-apprun-') as temp:
    temp = Path(temp)
    source = temp / 'temporary-extraction'
    (source / 'usr/bin').mkdir(parents=True)
    launcher = source / 'AppRun'
    launcher.write_text((root / 'packaging/AppRun').read_text().replace('@BUILD@', 'proof'))
    launcher.chmod(0o755)
    cli = source / 'usr/bin/starflux'
    # System Python interprets this stub; no real STAR/FLUX endpoint is started.
    cli.write_text('#!/usr/bin/env python3\nimport json,os,sys\nprint(json.dumps([os.path.realpath(__file__),os.environ["PATH"],os.environ["STARFLUX_SYSTEM_PATH"],sys.argv[1:]]))\n')
    cli.chmod(0o755)
    env = {**os.environ, 'XDG_CACHE_HOME': str(temp / 'cache')}
    env.pop('STARFLUX_SYSTEM_PATH', None)
    def invoke(path):
        result = subprocess.run([str(path), '--version'], env=env, text=True, capture_output=True, check=True, timeout=10)
        return json.loads(result.stdout)
    with ThreadPoolExecutor(max_workers=6) as pool:
        results = list(pool.map(invoke, [launcher] * 6))
    durable = temp / 'cache/starflux/appimage/proof'
    for executable, path, original, args in results:
        assert executable == str(durable / 'usr/bin/starflux')
        assert path.split(':')[0] == str(durable / 'usr/bin')
        assert original == env['PATH'] and args == ['--version']
    shutil.rmtree(source)
    assert invoke(durable / 'AppRun') == results[0]
    assert not list(durable.parent.glob('.stage-*'))
print('PASS: concurrent durable relocation, original tool PATH and CLI arguments; helpers survive extraction removal')
