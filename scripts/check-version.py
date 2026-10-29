#!/usr/bin/env python3
"""Validate workspace versions, release tags and portable SDK pins."""
import os,re,tomllib
from pathlib import Path
root=Path(__file__).resolve().parents[1]
manifest=tomllib.loads((root/'Cargo.toml').read_text())
lock=tomllib.loads((root/'Cargo.lock').read_text())
for filename,name in [('Cargo.toml','starflux'),('terminal-client/Cargo.toml','starflux-terminal-client')]:
    version=tomllib.loads((root/filename).read_text())['package']['version']
    assert next(p['version'] for p in lock['package'] if p['name']==name)==version, f'{name}: manifest/lock version mismatch'
kit=manifest['workspace']['dependencies']['starkit']
assert re.fullmatch(r'[0-9a-f]{40}',kit['rev']), 'SDK must use an immutable revision'
entry=next(p for p in lock['package'] if p['name']=='starkit')
assert entry['source']==f"git+{kit['git']}?rev={kit['rev']}#{kit['rev']}", 'SDK lock must retain its Git source and revision'
assert entry['version'].startswith(kit['version']+'.'), 'SDK version mismatch'
assert 'cargoToml.package.version' in (root/'flake.nix').read_text()
assert (root/'docs/releases'/f"{manifest['package']['version']}.md").is_file(), 'Versioned release notes are missing'
if os.environ.get('GITHUB_REF_TYPE')=='tag':
    assert os.environ['GITHUB_REF_NAME']=='v'+manifest['package']['version'], 'Release tag/version mismatch'
print('Workspace versions and immutable Git dependency pin are consistent')
