# Saved conversations across native recovery — November 14

This uninstalled increment follows `checkpoints/native-rebased/source.patch`.
Apply its saved canonical UI patch and source patch first, then this directory's
`source.patch`. `fixtures.patch` adds native-source selection to the shared UI
fixture and fixes the independent Python activity fixture. Apply the rebased
isolated build layout only when using the private staging executable names.

Native recovery correctly changes the terminal generation. Previously that also
hid the registered conversation and rejected its explicit resume action. The
agent store now retains a separate conversation link to the recovered generation.
It keeps the original report, provider/conversation identity and exact resume
argument vector unchanged. Repeated deliberate recovery retains the reference;
an unrelated recycled pane generation cannot use it. A newly registered run takes
precedence. No report authority crosses generations: inherited reports are stale,
cannot establish live process/backend health and cannot accept old heartbeats.

The link is written under the existing project transaction before publishing the
new generation. A crash before publication leaves an unused link; it cannot
authorize replaying a saved launch command. Only the existing explicit resume
action starts a conversation, in a new tab, retaining the recovered shell.

The shared UI fixture verifies actual native source bindings in both views. Its
cold-start mode retires the private native owner, removes runtime state and uses
real recovery confirmations for both panes. It then checks split persistence,
read-only historical screens, exact conversation resume, command non-replay,
corrupt archives, pause/clear/resume and preservation of an unrelated tmux source.
Same-boot owner loss is covered; this does not establish every reboot/uncertain
launch scenario. Fixture cleanup authenticates shutdown only inside its private
namespace. The background-source fixture does not transplant another project's
native identities when constructing a new imported project.

`refresh-checkpoint.py` reconstructs the previous stage from its canonical commit
and patches, verifies this two-file increment and both fixture patches, records
passing evidence and verifies the original production process identities and
installed executable hash. It refuses missing/failed evidence. The saved test
fixture must be copied into `target/` to rerun its relative-root launcher.

The original production-layout release run stopped at a Python fixture bug:
`exec -a codex python3` lost the Nix interpreter identity and loaded incompatible
system standard-library modules. The fixture already names itself with `prctl`;
launching it with `sys.executable` preserves detection and fixes the failure.
The failed log remains intact and a separate continuation starts at that check.
The initial Nix package/smoke pass applies to the preceding rebased source; the
new increment has a separate Nix run. Do not attribute either run to the other.

No installed binary, real agent, live terminal owner or App Space was replaced.
Full Herdr parity, the remaining native integration gates and production
promotion remain incomplete. Detailed evidence/status is in the receipt and
`docs/parity-progress.md`.
