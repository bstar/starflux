#!/usr/bin/env python3
"""Archive certified owner-loss recovery against the launch-fence stage."""
from pathlib import Path
import io,json,runpy,shutil,subprocess,tarfile,tempfile,time
ROOT=Path(__file__).resolve().parents[2]
OUT=Path(__file__).resolve().parent
CANDIDATE=ROOT/'target/inline-graphics-workspace'
helpers=runpy.run_path(str(ROOT/'checkpoints/native-process-supervision/refresh-checkpoint.py'))
digest,files,patch=(helpers[k] for k in ('digest','files','patch'))
FOLDERS=helpers['FOLDERS'];BASE=helpers['BASE']
LOGS=['clippy','unit','minimal','build','local','ssh','pending-local','pending-ssh','guardian-local','guardian-ssh','ui-local','ui-ssh','ui-cells','ui-ssh-cells','fence-local','fence-ssh','metadata-local','metadata-ssh','older-local','older-ssh','shell','tree-ssh']
def main():
 evidence={}
 for name in LOGS:
  log=ROOT/'target'/f'native-owner-loss-{name}.log';text=log.read_text()
  assert not any(bad in text for bad in ('Traceback','test result: FAILED','could not compile')),name
  if name in ('clippy','build'):assert 'Finished `' in text,name
  elif name in ('unit','minimal'):assert 'test result: ok.' in text,name
  else:assert any(json.loads(line).get('result')=='PASS' for line in text.splitlines() if line.startswith('{')),name
  evidence[name]=dict(sha256=digest(log),mtime=log.stat().st_mtime)
 old=ROOT/'checkpoints/native-terminal-candidate'
 supervisor=ROOT/'checkpoints/native-process-supervision'
 with tempfile.TemporaryDirectory(prefix='sf-metadata-checkpoint-') as directory:
  tree=Path(directory)
  archive=subprocess.check_output(['git','archive',BASE,*FOLDERS,'Cargo.toml','terminal-client/Cargo.toml'],cwd=ROOT)
  with tarfile.open(fileobj=io.BytesIO(archive)) as package:package.extractall(tree,filter='data')
  subprocess.run(['git','apply',str(old/'native-adapter-candidate.patch')],cwd=tree,check=True,capture_output=True)
  for relative,expected in json.loads((old/'native-adapter-candidate-receipt.json').read_text())['files'].items():assert digest(tree/relative)==expected,relative
  subprocess.run(['git','apply',str(supervisor/'source.patch')],cwd=tree,check=True,capture_output=True)
  for relative,expected in json.loads((supervisor/'receipt.json').read_text())['files'].items():
   assert digest(tree/relative)==expected['after'],relative
  links=ROOT/'checkpoints/native-hyperlinks'
  subprocess.run(['git','apply',str(links/'source.patch')],cwd=tree,check=True,capture_output=True)
  for relative,expected in json.loads((links/'receipt.json').read_text())['files'].items():
   assert digest(tree/relative)==expected['after'],relative
  metadata=ROOT/'checkpoints/native-metadata'
  subprocess.run(['git','apply',str(metadata/'source.patch')],cwd=tree,check=True,capture_output=True)
  for relative,expected in json.loads((metadata/'receipt.json').read_text())['files'].items():
   assert digest(tree/relative)==expected['after'],relative
  fence=ROOT/'checkpoints/native-launch-fence'
  subprocess.run(['git','apply',str(fence/'source.patch')],cwd=tree,check=True,capture_output=True)
  for relative,expected in json.loads((fence/'receipt.json').read_text())['files'].items():
   assert digest(tree/relative)==expected['after'],relative
  paths=set().union(*(files(tree,f)|files(CANDIDATE,f) for f in FOLDERS))
  delta,identities=patch(tree,CANDIDATE,paths)
  (OUT/'source.patch').write_text(delta)
  subprocess.run(['git','apply','--check',str(OUT/'source.patch')],cwd=tree,check=True,capture_output=True)
  subprocess.run(['git','apply',str(OUT/'source.patch')],cwd=tree,check=True,capture_output=True)
  for relative,expected in identities.items():assert digest(tree/relative)==expected['after'],relative
 for name in ('test-native-owner-loss.py','test-native-owner-loss-ui.py','run-native-tree-candidate'):
  shutil.copy2(ROOT/'target'/name,OUT/name)
 binary=ROOT/'target/native-supervision-build/release/starflux-graphics'
 assert binary.stat().st_mtime>=max((CANDIDATE/f).stat().st_mtime for f in identities)
 safety=json.loads((ROOT/'target/native-owner-loss-live-safety.json').read_text())
 assert safety['installed_binary_unchanged'] and not safety['gone_or_changed']
 baseline=json.loads((ROOT/'target/native-owner-loss-live-before.json').read_text())
 assert digest(ROOT/'target/release/starflux')==baseline['binary_sha256']
 for process in safety['preserved']:
  assert int(Path('/proc',str(process['pid']),'stat').read_text().rsplit(')',1)[1].split()[19])==process['start_ticks']
 receipt=dict(saved_at=time.time(),canonical_base_commit=subprocess.check_output(['git','rev-parse',BASE],cwd=ROOT,text=True).strip(),
  prior_launch_fence_patch_sha256=digest(fence/'source.patch'),source_patch_sha256=digest(OUT/'source.patch'),files=identities,
  validation_evidence=evidence,candidate_binary_sha256=digest(binary),candidate_lock_sha256=digest(CANDIDATE/'Cargo.lock'),
  fixtures={name:digest(OUT/name) for name in ('test-native-owner-loss.py','test-native-owner-loss-ui.py','run-native-tree-candidate')},
  live_safety_sha256=digest(ROOT/'target/native-owner-loss-live-safety.json'),original_processes_preserved=len(safety['preserved']),
  installed_binary_unchanged=True,shipping_activation=False,retained_titles_and_icons=True,
  certified_owner_loss_recovery=True,owner_pidfd_lifetime_monitor=True,
  certificate_requires_echild=True,missing_or_corrupt_evidence_refused=True,
  lost_open_reference_recovered=True,supervisor_loss_refuses_replacement=True,
  general_owner_loss_recovery_complete=False,
  rich_terminal_metadata_complete=False,canonical_promotion=False)
 (OUT/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
 print(json.dumps(dict(source_files=len(identities),patch_bytes=len(delta.encode()),validation_logs=len(evidence),candidate_not_installed=True)))
if __name__=='__main__':main()
