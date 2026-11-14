# Guarded unstarted-launch recovery candidate — 2026-11-14

This isolated, **uninstalled** stage follows `native-metadata`. It addresses a
saved launch intent which was never delivered to its still-live owner. It does
not establish general broker-loss recovery or authorize replay of saved commands.

Reconstruct from a disposable archive of canonical `72d017696`, applying:

1. `../native-terminal-candidate/native-adapter-candidate.patch`
2. `../native-process-supervision/source.patch`
3. `../native-hyperlinks/source.patch`
4. `../native-metadata/source.patch`
5. This directory's `source.patch`
6. `../native-process-supervision/isolated-build-layout.patch`, only for staging
   executable names. Preserve production manifests and immutable dependencies.

Use `target/inline-graphics-workspace` and the separate
`target/native-supervision-build` output. Copy fixtures into `target/` so their
root lookup is correct; the copied launcher is workstation specific. They use
private namespaces, fake programs and optional authenticated loopback SSH, never
real agent sessions. The automation fixture requires `--executable <candidate>`.

`refresh-checkpoint.py` reconstructs and checks every preceding source stage,
validates logs and original live-process identities, and archives source/fixture/
binary/evidence hashes. Previous checkpoint receipts remain historical evidence;
do not regenerate them from this later workspace.

## Recovery behavior

The optional `launch-fence-v1` owner operation distinguishes an existing source,
an initializer in progress, an unstarted ID and an uncertain consumed receipt.
Inspection alone writes nothing. Explicit recovery seals an unstarted ID with a
private durable receipt before assigning a fresh terminal/source epoch and
starting a default shell. A delayed old Open fails the existing exclusive receipt
creation and cannot run its saved command. Duplicate cancellation is idempotent;
the fence binds source ID, source epoch, launch digest and exact owner identity.

The live broker checks its active/initializing maps before sealing, so it cannot
cancel a launch already accepted by its initializer. Existing spawn receipts,
damaged data, mismatched identities, owner loss and unsupported owners refuse
unstarted cancellation. Receipt reads are bounded, reject links/nonregular files
and use nonblocking opens so a FIFO cannot freeze the owner. A lost Open reply
whose retained source exists still resolves through ordinary discovery, including
owners without the new capability; recovery then uses verified tree retirement.

UI startup failure for a saved native reference retains a read-only health
monitor, rather than hiding explicit recovery behind repeated startup attempts.
The pending pane's existing context menu and confirmation identify cancellation
of the unstarted launch and creation of a fresh shell. Termination controls retain
their separate verified-retirement requirements.

## Validation and limits

Strict all-target/all-feature Clippy, release unit matrices and release build are
recorded with runtime evidence. Private local/SSH automation tests cover read-only
pending health, durable cancellation, fresh-shell-only recovery, delayed old Open
rejection, operation replay, ordinary lost-reply discovery and sibling survival.
Graphical/cells UI fixtures locally and over SSH use the real recovery menu and
confirmation, then attach two independent views to the new source. Metadata and
older-owner regressions remain in the validation set. Exact counts and hashes are
in `receipt.json` and `docs/parity-progress.md`.

General owner loss remains a gate: absence of a source/owner is not proof that
its process tree retired. Durable supervisor retirement evidence, broader source
adoption, remaining attention/graphics/history/network cases, current UI/core
configuration rebasing and canonical release/Nix acceptance remain. This stage
changes no production backend and does not complete the Herdr parity goal; every
other unchecked requirement in `docs/plan.md` stays in scope.
