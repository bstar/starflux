#!/usr/bin/env python3
"""Check Codex command sandbox without sending a model turn or reading user secrets."""
import importlib.util,pathlib,tempfile,subprocess,json,select
root=pathlib.Path(__file__).resolve().parents[1]
s=importlib.util.spec_from_file_location('provider',root/'scripts/category-provider.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
with tempfile.TemporaryDirectory(prefix='starflux-category-policy-') as td:
 td=pathlib.Path(td);work=td/'workspace';work.mkdir();(work/'allowed').write_text('CATEGORY_ALLOWED');(td/'excluded').write_text('CATEGORY_EXCLUDED')
 argv=['codex']
 for key,value in m.codex_config(str(work)).items():argv+=['-c',key+'='+m.toml(value)]
 p=subprocess.Popen(argv+['app-server'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True)
 def rpc(identity,method,params):
  p.stdin.write(json.dumps(dict(id=identity,method=method,params=params))+'\n');p.stdin.flush()
  while True:
   if not select.select([p.stdout],[],[],20)[0]:raise RuntimeError('Policy probe timed out')
   value=json.loads(p.stdout.readline())
   if value.get('id')==identity:return value
 try:
  rpc(1,'initialize',dict(clientInfo=dict(name='starflux-policy-proof',version='1'),capabilities=dict(experimentalApi=True)))
  responses=[]
  for identity,path in enumerate([work/'allowed',td/'excluded'],2):
   value=rpc(identity,'command/exec',dict(command=['/usr/bin/cat',str(path)],cwd=str(work),permissionProfile='starflux_category',timeoutMs=3000))
   responses.append(value.get('result',value.get('error')))
  assert responses[0].get('stdout')=='CATEGORY_ALLOWED',responses
  assert responses[1].get('exitCode')!=0 and 'CATEGORY_EXCLUDED' not in responses[1].get('stdout',''),responses
  write=rpc(4,'command/exec',dict(command=['/usr/bin/touch',str(work/'must-not-write')],cwd=str(work),permissionProfile='starflux_category',timeoutMs=3000))
  assert write['result']['exitCode']!=0 and not (work/'must-not-write').exists(),write
  print(json.dumps(dict(result='PASS',workspace_read=True,outside_read_denied=True,repository_writes_denied=True,no_inference=True)))
 finally:p.terminate();p.wait(timeout=5)
