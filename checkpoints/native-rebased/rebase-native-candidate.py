#!/usr/bin/env python3
"""Three-way merge into a disposable workspace; never writes canonical sources."""
from pathlib import Path
import hashlib,json,shutil,subprocess,tempfile
ROOT=Path.cwd();BASE='72d017696';OLD=ROOT/'target/inline-graphics-workspace';OUT=ROOT/'target/native-rebase-workspace'
FOLDERS=['src','terminal-client/src','examples','vendor/vt100/src']
assert not OUT.exists(),'Rebase workspace already exists; inspect it rather than recreating it'
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def source_files(tree):return {p.relative_to(tree) for folder in FOLDERS for p in (tree/folder).rglob('*') if p.is_file() and '__pycache__' not in p.parts and p.suffix not in ('.pyc','.pyo')}
# Verify the final source state against the complete saved checkpoint stack.
with tempfile.TemporaryDirectory(prefix='sf-rebase-base-') as directory:
 base=Path(directory);archive=subprocess.check_output(['git','archive',BASE,*FOLDERS,'Cargo.toml','Cargo.lock','terminal-client/Cargo.toml'],cwd=ROOT)
 import io,tarfile
 with tarfile.open(fileobj=io.BytesIO(archive)) as package:package.extractall(base,filter='data')
 baseline={p:(base/p).read_bytes() for p in source_files(base)}
 for path in ['checkpoints/native-terminal-candidate/native-adapter-candidate.patch',*[f'checkpoints/{stage}/source.patch' for stage in ['native-process-supervision','native-hyperlinks','native-metadata','native-launch-fence','native-owner-loss']]]:
  subprocess.run(['git','apply',str(ROOT/path)],cwd=base,check=True,capture_output=True)
 for path in source_files(base)|source_files(OLD):
  assert (base/path).exists()==(OLD/path).exists(),str(path)
  if (base/path).exists():assert digest(base/path)==digest(OLD/path),str(path)
 # Prove staging manifest edits are only the separately recorded layout patch.
 for relative in ['Cargo.toml','terminal-client/Cargo.toml']:shutil.copy2(OLD/relative,base/relative)
 subprocess.run(['git','apply','--reverse',str(ROOT/'checkpoints/native-process-supervision/isolated-build-layout.patch')],cwd=base,check=True,capture_output=True)
 for relative in ['Cargo.toml','terminal-client/Cargo.toml']:
  assert (base/relative).read_bytes()==subprocess.check_output(['git','show',BASE+':'+relative],cwd=ROOT),relative
OUT.mkdir()
# Copy tracked application/build/documentation files, with actual current edits.
for relative in subprocess.check_output(['git','ls-files','-z'],cwd=ROOT).decode().split('\0'):
 if not relative:continue
 p=ROOT/relative
 if not p.exists():continue
 q=OUT/relative;q.parent.mkdir(parents=True,exist_ok=True)
 if p.is_symlink():q.symlink_to(p.readlink())
 else:shutil.copy2(p,q)
paths=set(baseline)|source_files(ROOT)|source_files(OLD);conflicts=[];changes=[]
with tempfile.TemporaryDirectory(prefix='sf-rebase-merge-') as directory:
 tmp=Path(directory)
 for path in sorted(paths):
  a=ROOT/path;b=OLD/path;dest=OUT/path
  original=baseline.get(path);canonical=a.read_bytes() if a.exists() else None;candidate=b.read_bytes() if b.exists() else None
  if candidate==original:result=canonical
  elif canonical==original:result=candidate
  elif canonical==candidate:result=canonical
  elif original is None and candidate is None:result=canonical
  elif original is None and canonical is None:result=candidate
  elif None in (original,canonical,candidate):
   conflicts.append(str(path));result=canonical if canonical is not None else candidate
  else:
   (tmp/'canonical').write_bytes(canonical);(tmp/'base').write_bytes(original);(tmp/'candidate').write_bytes(candidate)
   merge=subprocess.run(['git','merge-file','-p','--diff3','-L','current STAR/FLUX','-L','shared base','-L','native candidate',str(tmp/'canonical'),str(tmp/'base'),str(tmp/'candidate')],capture_output=True)
   assert merge.returncode>=0 and merge.returncode<=127,merge.stderr.decode()
   result=merge.stdout
   if merge.returncode:conflicts.append(str(path))
  if result is None:
   if dest.exists():dest.unlink()
  else:
   assert not dest.is_symlink(),str(path)
   dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(result)
  if result!=canonical:changes.append(str(path))
# Unique binary names keep every executable/fingerprint isolated during rebase.
p=OUT/'Cargo.toml';s=p.read_text().replace('[package]\n','[package]\nautobins = false\nautoexamples = false\n',1)
s+='\n[[bin]]\nname = "starflux-native-rebased"\npath = "src/main.rs"\n'
for path in sorted((OUT/'examples').glob('*.rs')):
 s+='\n[[example]]\nname = "rebased-'+path.stem+'"\npath = "examples/'+path.name+'"\n'
p.write_text(s)
p=OUT/'terminal-client/Cargo.toml';p.write_text(p.read_text().replace('[[bin]]\nname = "starflux-terminal-client"','[[bin]]\nname = "starflux-terminal-client-rebased"',1))
report=dict(canonical_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),base=BASE,source_changes=changes,conflicts=conflicts,canonical_ui={str(p):digest(ROOT/p) for p in ['src/main.rs','src/ui.rs','src/shelf.rs','src/graphical.rs']})
(ROOT/'target/native-rebase-report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(dict(workspace=str(OUT),source_changes=len(changes),conflicts=conflicts)))
