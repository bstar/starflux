# Native launch authority handoff — pause checkpoint, November 14

This uninstalled two-file increment follows `native-startup`. Apply the rebased
canonical UI/source patches, then the conversation-recovery, startup and this
source patch, in that order. The optional rebased manifest layout is for staging
names only. The source reconstructs exactly; `receipt.json` records its identities.

Repeated cold-start checks found a real cross-frontend race: one view consumed
the saved resume permission before native discovery, then another view started
a plain shell before that first view published its intent. A private diagnostic
captured an alive source with only shell output, although the shared model still
contained the exact resume command. This was not a provider-resumption success.

Native command selection now comes from the shared pending model, rather than a
frontend-local hint. The backend revalidates the pane generation and command,
consumes the one-shot launch permission and publishes the native source intent
under the same project transaction. Discovery and shell-default RPCs remain
outside that transaction. Another view may discover the resulting source, but
cannot substitute its own default shell or replay the requested program. An
already-published identical ready reference is accepted during final publication.
Legacy/imported/plugin paths retain their earlier launch-permission handling.

Strict Clippy, the release all-feature workspace tests (278 main plus 4 client;
4 main ignored) and binary builds pass. Private cold-runtime/saved-vector resume
checks pass locally in graphical mode and over SSH in cells mode on the final
binary. These use fake agents and do not prove real provider conversation resume.

Still required: a deterministic regression that forces the pre-intent handoff
window, repeated native cold-start checks, the no-default-feature matrix and Nix
checks on this exact increment, remaining native ownership/adoption acceptance
and production integration. The successful release continuation and Nix package
for `native-startup` predate this two-file change; do not attribute them to it.

The saved diagnostic fixture belongs in `target/` when rerun. It records only its
own disposable fake-agent namespace on failure, excluding the owner token.
See `docs/parity-handoff-2026-11-14.md` for the complete pause handoff and remaining
Herdr parity scope. No installed program or live session was replaced.
