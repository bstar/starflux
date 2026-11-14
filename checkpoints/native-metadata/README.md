# Native terminal metadata candidate — 2026-11-14

This isolated, **uninstalled** stage follows the process-supervision and hyperlink
checkpoints. It adds source-owned terminal titles/icon names, bell counters and
OSC 52 clipboard writes to the native presentation path. No production backend
has been switched and no system clipboard is read by the owner or test fixtures.

Reconstruct in a disposable archive of canonical `72d017696`, applying:

1. `../native-terminal-candidate/native-adapter-candidate.patch`
2. `../native-process-supervision/source.patch`
3. `../native-hyperlinks/source.patch`
4. This directory's `source.patch`
5. `../native-process-supervision/isolated-build-layout.patch` for staging binary
   names only. Keep production manifests and immutable shared dependencies intact.

The workspace remains `target/inline-graphics-workspace`; use the separate
`target/native-supervision-build` output. The copied launcher is workstation
specific. Copy fixtures into `target/` before running them so their relative root
resolution remains valid. All use private namespaces, fake programs and optional
private authenticated loopback SSH. The wire fixture runs the real managed owner
with `--service --executable <candidate> --worker-executable <candidate>`.
It unlinks a private owner copy before launching sources, verifying that retained
owners use their loaded `/proc/self/exe` image after executable replacement.

`refresh-checkpoint.py` reconstructs and verifies every prior source stage,
checks the current validation logs and live-process identities, and saves source,
fixture, candidate-binary and evidence hashes in `receipt.json`. Earlier stages
remain historical records; do not regenerate their receipts from this workspace.

## Behavior

The retained owner captures OSC 0/1/2 title/icon values and OSC 52 writes; queries
never read or forward the user's system clipboard. Titles are valid UTF-8, at
most 4 KiB each, and contain no control characters. Clipboard text is valid UTF-8
and at most 64 KiB. Invalid values retain the previous accepted state and increment
a rejection counter. Accepted metadata lazily reserves 192 KiB from the owner's
shared retained budget; snapshots and view decoding have separate reservations.

Each new watch establishes a clipboard sequence baseline. Old copies are not
replayed on initial attachment, hidden-tab reveal or connector reconnection.
New writes carry source-scoped sequence numbers and use the existing reliable
frontend clipboard response. Receivers suppress duplicate sequences. A valid
frame acknowledgement advances only that watch's baseline; subsequent frames
omit the clipboard payload. Slow views retain independent credit and receive the
latest retained write when they resume. This is bounded latest-value delivery,
not a durable history of every rapid copy or an exactly-once offline clipboard.
Read-only screen/cells snapshots never include an actionable copy payload.

Retained titles update default tab labels, behind user-assigned names, agent
labels and imported names. Empty titles restore generated labels. Bell counters
are forwarded in diagnostics; they do not fabricate agent lifecycle events or yet
provide global attention/OS notification delivery. State-file diagnostics expose
titles, counters and clipboard sequence, never copied clipboard text.

## Evidence and remaining work

Strict workspace all-target/all-feature Clippy, release unit matrices and the
release build pass. Private graphical/cells UI checks locally and over SSH cover
three views, title clear/change, explicit name precedence, Unicode and a 60 KiB
clipboard payload, empty writes, invalid/oversized/binary rejection, late attach,
hidden reveal, viewport epochs, connector loss and explicit source recovery.
A wire-level local/SSH check proves clipboard-only updates carry empty text
deltas, acknowledgement removes subsequent payloads, stale acknowledgements are
rejected, and slow views do not share progress. Existing local/SSH hyperlink and
older-owner checks remain green. Exact counts/hashes are in the receipt and
`docs/parity-progress.md`.

Still outstanding: general terminal attention/OS notification forwarding,
owner-loss and never-delivered intent recovery, broader adoption, graphics and
history/placement compatibility, constrained-network/media stress, current UI/core
configuration rebasing and canonical release/Nix acceptance. Clipboard selector
mapping follows the existing text-clipboard interface; separate primary/cut-buffer
selection and clipboard reads are not implemented by that interface. This stage
does not establish full native migration or Herdr parity. Every other unchecked
requirement in `docs/plan.md` remains in scope.
