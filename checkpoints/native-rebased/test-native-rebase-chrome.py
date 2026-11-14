#!/usr/bin/env python3
"""Render two private copies of the same model; do not attach to user sessions."""
from pathlib import Path
import hashlib,json,os,socket,subprocess,tempfile,time
ROOT=Path(__file__).resolve().parents[1]
outputs=[]
for label,app in [('canonical',ROOT/'scripts/run-local.sh'),('rebased',ROOT/'target/run-native-rebase-candidate')]:
 with tempfile.TemporaryDirectory(prefix='sf-rebase-render-') as directory:
  tmp=Path(directory);runtime=tmp/'runtime';data=tmp/'data';config=tmp/'config'
  for p in [runtime,runtime/'starflux',data/'starflux/projects',config/'starflux']:p.mkdir(parents=True,exist_ok=True,mode=0o700)
  # Keep paths identical in displayed metadata across both temporary scopes.
  project=dict(workspace='rebase-chrome',label='Rendering proof',collection='default',host=None,directory='/tmp',agent=None,executable=str(app),desktop_helper=str(app),project_host=True,theme='kanagawa')
  (runtime/'starflux/rebase-chrome.json').write_text(json.dumps(project))
  (data/'starflux/projects/rebase-chrome.json').write_text(json.dumps(project))
  (config/'starflux/config.json').write_text(json.dumps(dict(theme='gruvbox-dark')))
  env={**os.environ,'XDG_RUNTIME_DIR':str(runtime),'XDG_DATA_HOME':str(data),'XDG_CONFIG_HOME':str(config),'TMUX_TMPDIR':str(tmp/'tmux'),'SHELL':'/bin/true','STARFLUX_KITTY':str(tmp/'kitty-must-not-run'),'STAR_TERMINAL_RENDERER':'native'}
  output=ROOT/'target'/('native-rebase-chrome-'+label+'.png')
  try:
   result=subprocess.run([str(app),'--workspace','rebase-chrome','--png',str(output)],env=env,capture_output=True,timeout=30)
   assert result.returncode==0,(label,result.stderr.decode())
   outputs.append(output.read_bytes())
  finally:
   subprocess.run(['tmux','-L','starflux-rebase-chrome','kill-server'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
   owner_path=data/'starflux/native-sources/owner.json'
   if owner_path.exists():
    owner=json.loads(owner_path.read_text())['owner'];token=json.loads(owner_path.with_name('token.json').read_text())
    with socket.socket(socket.AF_UNIX) as peer:
     peer.settimeout(3);peer.connect(str(owner_path.with_name('broker.sock')));peer.sendall((json.dumps(dict(version=1,id=1,token=token,action=dict(op='shutdown',instance=owner['instance'])))+'\n').encode());peer.makefile('rb').readline()
identical=outputs[0]==outputs[1]
print(json.dumps(dict(result='PASS' if identical else 'DIFFERENT',identical_png_bytes=identical,canonical_sha256=hashlib.sha256(outputs[0]).hexdigest(),rebased_sha256=hashlib.sha256(outputs[1]).hexdigest())))
assert identical,'Private rendering differs; inspect PNGs before accepting the rebase'
