#!/usr/bin/env python3
"""Archive a rebase against the actual current source, including requested UI edits."""
from pathlib import Path
import hashlib,io,json,runpy,shutil,subprocess,tarfile,tempfile,time
ROOT=Path(__file__).resolve().parents[2];OUT=Path(__file__).resolve().parent;TREE=ROOT/'target/native-rebase-workspace'
helpers=runpy.run_path(str(ROOT/'checkpoints/native-process-supervision/refresh-checkpoint.py'))
digest,files,patch=(helpers[k] for k in ('digest','files','patch'));FOLDERS=helpers['FOLDERS']
LOGS=['clippy','unit','minimal','build','chrome','theme-local','theme-cells','theme-ssh','theme-ssh-cells','owner-ui-local','owner-ui-ssh-cells','links-local','links-ssh-cells','metadata-local','metadata-ssh-cells','fence-local','fence-ssh-cells','owner-api-ssh','guardian-api-ssh','portfolio-local','portfolio-ssh-cells']
FIXTURES=['test-native-rebase-theme.py','test-native-rebase-current-ui.py','test-native-rebase-owner-ui.py','test-native-rebase-fence-ui.py','test-native-rebase-links-ui.py','test-native-rebase-metadata-ui.py','test-native-rebase-chrome.py','run-native-rebase-candidate','rebase-native-candidate.py']
def main():
 report=json.loads((ROOT/'target/native-rebase-report.json').read_text());head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip();assert head==report['canonical_head']
 for relative,expected in report['canonical_ui'].items():assert digest(ROOT/relative)==expected,relative
 evidence={}
 for name in LOGS:
  p=ROOT/'target'/f'native-rebase-{name}.log';text=p.read_text()
  assert not any(bad in text for bad in ('Traceback','test result: FAILED','could not compile')),name
  if name in ('clippy','build'):assert 'Finished `' in text,name
  elif name in ('unit','minimal'):assert 'test result: ok.' in text,name
  else:assert any(json.loads(line).get('result')=='PASS' for line in text.splitlines() if line.startswith('{')),name
  evidence[name]=dict(sha256=digest(p),mtime=p.stat().st_mtime)
 scope=[*FOLDERS,'Cargo.toml','Cargo.lock','terminal-client/Cargo.toml','vendor/vt100/Cargo.toml']
 ui_patch=subprocess.check_output(['git','diff','--binary',head,'--',*scope],cwd=ROOT)
 (OUT/'canonical-ui.patch').write_bytes(ui_patch)
 with tempfile.TemporaryDirectory(prefix='sf-native-rebase-proof-') as directory:
  base=Path(directory);archive=subprocess.check_output(['git','archive',head,*scope],cwd=ROOT)
  with tarfile.open(fileobj=io.BytesIO(archive)) as package:package.extractall(base,filter='data')
  subprocess.run(['git','apply',str(OUT/'canonical-ui.patch')],cwd=base,check=True,capture_output=True)
  paths=set().union(*(files(base,f)|files(ROOT,f)|files(TREE,f) for f in FOLDERS))
  for relative in paths:
   a,b=base/relative,ROOT/relative
   assert a.exists()==b.exists(),str(relative)
   if a.exists():assert digest(a)==digest(b),str(relative)
  delta,identities=patch(base,TREE,paths);(OUT/'source.patch').write_text(delta)
  subprocess.run(['git','apply',str(OUT/'source.patch')],cwd=base,check=True,capture_output=True)
  for relative in paths:
   a,b=base/relative,TREE/relative
   assert a.exists()==b.exists(),str(relative)
   if a.exists():assert digest(a)==digest(b),str(relative)
  manifests={Path('Cargo.toml'),Path('terminal-client/Cargo.toml'),Path('vendor/vt100/Cargo.toml')}
  layout,_=patch(base,TREE,manifests);(OUT/'isolated-build-layout.patch').write_text(layout)
  subprocess.run(['git','apply','--check',str(OUT/'isolated-build-layout.patch')],cwd=base,check=True,capture_output=True)
  assert (TREE/'Cargo.lock').read_bytes()==(ROOT/'Cargo.lock').read_bytes()
 for fixture in FIXTURES:shutil.copy2(ROOT/'target'/fixture,OUT/fixture)
 safety_path=ROOT/'target/native-rebase-live-safety.json';safety=json.loads(safety_path.read_text());assert safety['installed_binary_unchanged'] and not safety['gone_or_changed']
 before=json.loads((ROOT/'target/native-rebase-live-before.json').read_text());assert digest(ROOT/'target/release/starflux')==before['binary_sha256']
 for process in safety['preserved']:assert int(Path('/proc',str(process['pid']),'stat').read_text().rsplit(')',1)[1].split()[19])==process['start_ticks']
 binary=ROOT/'target/native-rebase-build/release/starflux-native-rebased'
 assert binary.stat().st_mtime>=max((TREE/p).stat().st_mtime for p in identities)
 receipt=dict(saved_at=time.time(),canonical_base_commit=head,canonical_ui_patch_sha256=digest(OUT/'canonical-ui.patch'),canonical_ui=report['canonical_ui'],source_patch_sha256=digest(OUT/'source.patch'),files=identities,isolated_layout_sha256=digest(OUT/'isolated-build-layout.patch'),previous_owner_loss_patch_sha256=digest(ROOT/'checkpoints/native-owner-loss/source.patch'),candidate_binary_sha256=digest(binary),canonical_lock_unchanged=True,candidate_lock_sha256=digest(TREE/'Cargo.lock'),validation_evidence=evidence,fixtures={name:digest(OUT/name) for name in FIXTURES},live_safety_sha256=digest(safety_path),original_processes_preserved=len(safety['preserved']),installed_binary_unchanged=True,current_theme_and_portfolio_rebased=True,pixel_identical_private_reference_case=True,global_theme_native_session_checks=True,shipping_activation=False,canonical_source_promotion=False,full_parity_complete=False)
 (OUT/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
 print(json.dumps(dict(source_files=len(identities),patch_bytes=len(delta.encode()),validation_logs=len(evidence),current_source_rebase=True,candidate_not_installed=True)))
if __name__=='__main__':main()
