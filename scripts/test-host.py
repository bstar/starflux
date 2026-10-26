#!/usr/bin/env python3
"""Exercise real isolated tmux persistence and atomic, checked attachment delivery."""
import base64, hashlib, json, os, pathlib, subprocess, tempfile, time, uuid
root=pathlib.Path(__file__).resolve().parents[1]
helper=root/'scripts/host.py'; workspace='test-'+uuid.uuid4().hex[:10]
with tempfile.TemporaryDirectory(prefix='starmux-host-') as tmp:
    env={**os.environ,'XDG_DATA_HOME':tmp}
    def call(action,pane='p0',data=None,check=True,**kw):
        req={'action':action,'workspace':workspace,'pane':pane,**kw}
        encoded=base64.b64encode(json.dumps(req).encode()).decode()
        result=subprocess.run(['python3',str(helper),encoded],input=data,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,timeout=20)
        if check:
            assert result.returncode==0,result.stderr.decode()
            return json.loads(result.stdout) if action!='capture' else result.stdout.decode()
        return result
    try:
        call('ensure',directory=tmp,argv=['bash','--noprofile','--norc'])
        first=call('inspect');assert first['alive']
        # A second independent helper/connection reuses the process, not a new shell.
        call('ensure',directory=tmp,argv=['bash']);assert call('inspect')['pid']==first['pid']
        marker=pathlib.Path(tmp)/'must-not-submit'
        call('paste',text='touch '+str(marker));time.sleep(.15);assert not marker.exists(),'Paste submitted a command'
        call('key',key='Enter')
        deadline=time.monotonic()+3
        while not marker.exists() and time.monotonic()<deadline:time.sleep(.05)
        assert marker.exists(),'Explicit submission failed'
        contents=b'png-like fixture\x00\xff'*20000; digest=hashlib.sha256(contents).hexdigest()
        result=call('upload',data=contents,name='screen shot λ.png',size=len(contents),sha256=digest)
        dest=pathlib.Path(result['path']);assert dest.read_bytes()==contents and dest.stat().st_mode&0o777==0o600
        assert dest.parent.stat().st_mode&0o777==0o700
        assert call('upload',data=b'partial',check=False,name='bad.png',size=20,sha256=digest).returncode!=0
        assert call('upload',data=b'wrong',check=False,name='bad.png',size=5,sha256=digest).returncode!=0
        assert call('upload',data=b'',check=False,name='../escape',size=0,sha256=digest).returncode!=0
        assert not list(pathlib.Path(tmp).rglob('.upload')),'Partial uploads leaked'
        # Migration may replace only our verified test fixture, never a real process.
        call('ensure',pane='p1',directory=tmp,argv=[str(root/'scripts/run-local.sh'),'--fixture',workspace])
        deadline=time.monotonic()+3
        while call('inspect',pane='p1')['command']!='starmux' and time.monotonic()<deadline:time.sleep(.05)
        old=call('inspect',pane='p1');assert old['alive'] and old['command']=='starmux',old
        upgraded=call('ensure',pane='p1',directory=tmp,argv=['bash','--noprofile','--norc'],replace_fixture=True)
        assert upgraded['replaced_fixture'],upgraded
        shell=call('inspect',pane='p1');assert shell['alive'] and shell['pid']!=old['pid']
        assert call('inspect')['pid']==first['pid'],'Replacing the fixture disturbed the first pane'
        kept=call('ensure',pane='p1',directory=tmp,argv=['false'],replace_fixture=True)
        assert not kept['replaced_fixture'] and call('inspect',pane='p1')['pid']==shell['pid'],'A real process was replaced'
        call('stop',pane='p1')
        call('ensure',pane='p1',directory=tmp,argv=[str(root/'target/release/starmux'),'--fixture',workspace])
        time.sleep(.15);call('paste',pane='p1',text='/quit');call('key',pane='p1',key='Enter')
        deadline=time.monotonic()+3
        while call('inspect',pane='p1')['alive'] and time.monotonic()<deadline:time.sleep(.05)
        assert not call('inspect',pane='p1')['alive'],'Fixture did not exit'
        upgraded=call('ensure',pane='p1',directory=tmp,argv=['bash','--noprofile','--norc'],replace_fixture=True)
        assert upgraded['replaced_fixture'],'Dead fixture was not migrated'
        call('stop');assert call('inspect',pane='p1')['alive'],'Scoped stop killed another session'
        print('PASS: persistent PID, bracketed paste without submission, explicit Enter, binary/Unicode upload, private staging, digest checks, interrupted upload cleanup, traversal rejection, fixture-only migration, protected real process and scoped stop')
    finally:
        for pane in ['p0','p1']:
            try:call('stop',pane=pane)
            except Exception:pass
