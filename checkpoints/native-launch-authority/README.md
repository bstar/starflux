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

## November 17 resumed validation

`validation-resumed.json` supplements the original receipt without relabeling
older evidence. Canonical `scripts/test-session-manager.py --native-launch-race`
now forces the pre-intent handoff using a private Python host-helper gate.
An older startup build fails while the gate is held. The current candidate
passes in graphical/cells modes locally and over SSH: the other view executes
the exact registered vector once before release, both views retain ordinary
terminal input, and both subsequently attach to the same resumed source.

The exact Rust increment also passed the no-default-feature release matrix
(277 main plus 4 client tests), Linux x86_64 Nix package/all-feature tests
(278 main plus 4 client) and executable smoke checks. Four main tests remain
ignored in each matrix. Repeated cold-runtime tests passed locally in cells and
over SSH in graphical mode. All these conversation fixtures use fake agents.

The forced race additionally passed against the Nix package's underlying Rust
executable. The package's shell wrapper prepends its Python path, bypassing the
injected gate; the original two wrapper attempts therefore failed to reach the
gate and are retained as fixture limitations. They are not product failures or
claimed passing wrapper race tests. Wrapper smoke checks passed separately.
ARM64 and real provider conversation resumption remain unverified.

No desktop or mouse control was used. The installed executable and all ten
production process identities observed at this resumption's baseline remained
unchanged. Native source remains staged and uninstalled. Provider-confirmed
identity capture/resumption, broader ownership/adoption and the other unchecked
requirements in `docs/plan.md` remain in scope.
