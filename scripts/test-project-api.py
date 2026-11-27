#!/usr/bin/env python3
"""Exercise projects.create and projects.rename through the headless API.

Runs STAR/FLUX's JSON API in a private XDG environment; no terminal or
frontend is started.
"""
import json
import os
import secrets
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[1]
app = Path(os.environ.get('STARFLUX_TEST_APP', str(root / 'scripts/run-local.sh'))).resolve()

from proof_directory import ProofDirectory
with ProofDirectory(prefix='starflux-project-api-', dir='/tmp') as directory:
    tmp = Path(directory)
    work = tmp / 'work'
    work.mkdir()
    env = {**os.environ, 'XDG_RUNTIME_DIR': str(tmp / 'run'), 'XDG_DATA_HOME': str(tmp / 'data'),
           'XDG_CONFIG_HOME': str(tmp / 'config'), 'TMUX_TMPDIR': str(tmp / 'tmux')}
    for key in ('XDG_RUNTIME_DIR', 'TMUX_TMPDIR'):
        Path(env[key]).mkdir(mode=0o700)

    def api(method, params, write=False, ok=True, operation=None):
        request = dict(version=1, id='project-api-proof', method=method, params=params)
        if write:
            request['operation_id'] = operation or secrets.token_hex(16)
        response = json.loads(subprocess.check_output([str(app), '--api', json.dumps(request)],
                                                      env=env, timeout=20))
        assert response['ok'] == ok, response
        return response.get('result', response.get('error'))

    session = api('sessions.create', dict(name='API session', directory=str(work)), write=True)
    session_id = session.get('session', session.get('id'))
    assert session_id, session
    operation = secrets.token_hex(16)
    created = api('projects.create', dict(session=session_id, label='Second project', directory=str(work)),
                  write=True, operation=operation)
    project = created['project']
    assert created['session'] == session_id and created['label'] == 'Second project', created
    # Replaying the same operation returns its original result instead of creating another project.
    assert api('projects.create', dict(session=session_id, label='Second project', directory=str(work)),
               write=True, operation=operation) == created
    listed = {p['id']: p for p in api('projects.list', {})['projects']}
    assert project in listed, listed.keys()
    renamed = api('projects.rename', dict(project=project, label='Renamed project'), write=True)
    assert renamed == dict(project=project, label='Renamed project'), renamed
    listed = {p['id']: p for p in api('projects.list', {})['projects']}
    assert listed[project].get('label') == 'Renamed project', listed[project]
    api('projects.create', dict(session='session-0000000000000000', label='Missing', directory=str(work)),
        write=True, ok=False)
    api('projects.create', dict(session=session_id, label='No directory', directory=str(tmp / 'missing')),
        write=True, ok=False)
    api('projects.rename', dict(project=project, label=''), write=True, ok=False)

print(json.dumps({'result': 'PASS', 'project_created': True, 'operation_replayed': True,
                  'project_renamed': True, 'invalid_requests_rejected': True}))
