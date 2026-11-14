# Certified native owner-loss recovery — 2026-11-14

This isolated, **uninstalled** stage follows `native-launch-fence`. It lets the
user recover a source whose broker died, once its PTY supervisor has durably
certified that every owned descendant was reaped. A disappeared broker/PID alone
still never authorizes replacement.

Reconstruct in a disposable archive of canonical `72d017696`, applying:

1. `../native-terminal-candidate/native-adapter-candidate.patch`
2. `../native-process-supervision/source.patch`
3. `../native-hyperlinks/source.patch`
4. `../native-metadata/source.patch`
5. `../native-launch-fence/source.patch`
6. This directory's `source.patch`
7. `../native-process-supervision/isolated-build-layout.patch`, only for staging
   executable names. Preserve production manifests and immutable dependencies.

Use `target/inline-graphics-workspace` with the separate
`target/native-supervision-build` output. Copy fixtures into `target/` before
running them so relative root lookup remains valid. The copied launcher is
workstation specific. Tests use private namespaces, fake programs, optional
private authenticated loopback SSH and pidfds for exact owned-process cleanup.
The automation fixture accepts `--executable <candidate>`, `--ssh`,
`--pending-reference` and `--guardian-loss`; the UI fixture accepts `--ssh/--cells`.
No real agent or production App Space is used.

`refresh-checkpoint.py` verifies reconstruction through every previous source
stage, validation logs, candidate freshness and original production identities,
then archives source/fixture/binary/evidence hashes. Earlier receipts remain
historical evidence; do not regenerate them from this later workspace.

## Certificate and process authority

Each new source passes its exact owner identity and launch identity/digest to its
single-threaded subreaper, alongside the private receipt directory's inode/device.
The supervisor verifies its actual parent, pins that owner with a pidfd, and
retains a close-on-exec directory handle. The program inherits neither handle.
An idle supervisor blocks on signals and owner death; there is no polling timer.
After owner death it retires its owned tree, including detached processes and
nested subreapers. The pidfd is removed from polling once observed so it cannot
create a busy loop during retirement.

Only after `waitpid` reports `ECHILD` does the supervisor atomically write and
fsync `<source-id>.retired.json`. The certificate binds the complete owner identity,
source reference, source epoch, launch digest and exact directory. The additional
finish check requires a single-threaded process with no children. Source-program
exit and normal explicit retirement also produce a certificate. This follows the
existing source policy that command exit retires its remaining descendants.
Closing a frontend or SSH connector leaves its persistent owner alive and does
not trigger this path.

Recovery requires authoritative owner death, a matching private bounded record,
a reaped supervisor, and unchanged slot/launch identity. Missing, corrupt,
nonregular, linked, mismatched or unfinished evidence is refused. Verification
sends no signals. Existing owners are never killed/upgraded to satisfy a new
client. A new/current compatible owner is discovered only after proof and the
user's explicit confirmation; a fresh terminal/source epoch then opens a default
shell, clearing saved launch hints without replaying commands. Lost Open replies
are supported even if only the durable slot contains the old source reference.

Read-only health exposes certified recovery through existing graphical/cells
menus. Owner-loss panes remain visible for explicit action rather than being
automatically removed by reap. Explicit termination can remove a certified dead
known source without starting a replacement owner.

## Evidence and remaining gates

Strict Clippy, release unit matrices and release build pass. Local/SSH automation
checks use stubborn detached descendants and nested subreapers, kill only a private
broker, prove every descendant gone before certificate acceptance, then recover one
fresh shell with a new owner/epoch. They reject corrupt proof and stale input,
preserve operation replay and never run saved commands. An unrelated live process
remains alive. A separate supervisor-kill case leaves descendants unverified and
proves no replacement/binding change/global kill occurs. Graphical and cells UI
checks locally and over SSH use the actual menu/confirmation and verify shared
input after replacement. Unstarted-intent, metadata, older-owner, job-control and
idle/process-tree regressions are included; exact counts/hashes are in the receipt.

This does not prove recovery for missing certificates, pre-supervisor startup
uncertainty, damaged/lost journals, unsupported older owners, or arbitrary retained
source adoption. Those cases continue to refuse replacement. Broader ownership,
attention, graphics/history/placement and network/media cases, current UI/core
configuration rebasing and canonical release/Nix acceptance remain. The production
backend is unchanged, and every other unchecked requirement in `docs/plan.md`
stays in scope. Full Herdr parity is not complete.
