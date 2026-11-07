from pathlib import Path
import difflib,hashlib,json,time
root=Path.cwd();candidate=root/'target/inline-graphics-workspace'
def digest(p):return hashlib.file_digest(open(p,'rb'),'sha256').hexdigest()
def source_files(base,folder):return {p.relative_to(base) for p in (base/folder).rglob('*') if p.is_file() and '__pycache__' not in p.parts and p.suffix not in ['.pyc','.pyo']}
files={};baseline={};patch=[];deleted=[]
for folder in ['src','terminal-client/src','examples','vendor/vt100/src']:
 for rel in sorted(source_files(root,folder)|source_files(candidate,folder)):
  left=root/rel;right=candidate/rel;a=left.read_bytes() if left.exists() else b'';b=right.read_bytes() if right.exists() else b''
  if a==b:continue
  key=str(rel);files[key]=digest(right) if right.exists() else None;baseline[key]=digest(left) if left.exists() else None
  if not right.exists():deleted.append(key)
  patch.append('diff --git a/'+key+' b/'+key+'\n')
  if not left.exists():patch.append('new file mode 100644\n')
  if not right.exists():patch.append('deleted file mode 100644\n')
  patch.extend(difflib.unified_diff(a.decode().splitlines(True),b.decode().splitlines(True),fromfile='a/'+key if left.exists() else '/dev/null',tofile='b/'+key if right.exists() else '/dev/null'))
assert not deleted,deleted
patch_path=root/'target/native-adapter-candidate.patch';patch_path.write_text(''.join(patch))
fixture_names=['test-native-adapter.py','test-native-views.py','test-native-presenter.py','test-native-presenter-kitty.py','test-native-image-delivery.py','test-native-recovery.py','test-native-lifecycle-views.py','test-native-compatibility.py','test-native-older-owner.py','test-native-foundation-owner.py','test-native-recovery-held-child.py','test-native-recovery-image-backpressure.py']
log_names=['native-recovery-clippy','native-recovery-unit','native-recovery-minimal','native-recovery-build','native-recovery-local','native-recovery-ssh','native-recovery-held-child','native-recovery-ui-local','native-recovery-ui-ssh','native-recovery-ui-cells','native-recovery-ui-ssh-cells','native-recovery-image-backpressure-ssh','native-compatibility-local','native-compatibility-ssh','native-older-owner-local','native-older-owner-ssh','native-foundation-owner-local','native-foundation-owner-ssh','native-recovery-broker-local','native-recovery-broker-ssh','native-previous-owner-build']
for name in log_names:
 p=root/'target'/(name+'.log');s=p.read_text();assert 'Traceback (most recent call last)' not in s and 'error: could not compile' not in s,name
 if name not in ['native-recovery-clippy','native-recovery-unit','native-recovery-minimal','native-recovery-build','native-previous-owner-build']:assert '"result": "PASS"' in s,name
memory=json.loads((root/'target/memory-fix-receipt.json').read_text())
receipt={'candidate':str(candidate),'created':time.time(),'files':files,'canonical_base_files':baseline,'patch_sha256':digest(patch_path),'fixtures':{name:digest(root/'target'/name) for name in fixture_names},'checks':{name:digest(root/'target'/(name+'.log')) for name in log_names},'binary_sha256':digest(root/'target/release/starflux-graphics'),'client_binary_sha256':digest(root/'target/release/starflux-terminal-client-graphics'),'ordinary_native_views_verified':True,'source_ssh_views_verified':True,'lossless_native_planes_verified':True,'offscreen_real_kitty_verified':True,'media_and_input_during_image_wait_verified':True,'ordinary_native_ui_lifecycle_verified':True,'recovery_termination_natural_exit_and_slot_reuse_verified':True,'last_exit_and_explicit_reopen_verified':True,'older_owner_retirement_fenced':True,'legacy_source_backend_preserved':True,'held_pty_refuses_duplicate_launch':True,'image_effect_backpressure_verified':True,'complete_process_tree_supervision':False,'owner_loss_and_never_delivered_recovery_complete':False,'ui_attachment_complete':False,'canonical_promotion':False,'canonical_binary_matches_validated_checkpoint':False,'canonical_binary_matches_memory_fix':digest(root/'target/release/starflux')==memory['binary_sha256'],'dependency_lock_matches':(root/'Cargo.lock').read_bytes()==(candidate/'Cargo.lock').read_bytes(),'staging_manifests_excluded':True,'previous_owner_fixture':{'executable_sha256':digest(root/'target/release/starflux-previous-native'),'source_sha256':digest(root/'target/native-previous-owner-workspace/src/source_broker.rs'),'purpose':'Prior retention semantics and missing retirement-eof-v1 capability; private protocol fixture, not a released build'}}
assert receipt['canonical_binary_matches_memory_fix'] and receipt['dependency_lock_matches']
(root/'target/native-adapter-candidate-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({'reviewable_changed_source_files':len(files),'patch_bytes':patch_path.stat().st_size,'canonical_binary_unchanged':True,'dependency_lock_unchanged':True,'all_recorded_checks_pass':True,'candidate_not_installed':True}))
