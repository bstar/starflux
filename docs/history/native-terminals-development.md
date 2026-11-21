# Native terminals: development log

These are the dated checkpoints from when native terminals were an isolated
candidate beside the tmux backend, kept for their test evidence. The current
behaviour is described in [native terminals](../native-sources.md); "candidate"
and "uninstalled" below describe the build at that date, not today.

## Recovery and compatibility — November 6

The isolated candidate now implements native recovery, termination, natural-exit
reaping and slot reuse. None of this switches the installed backend or migrates
a running user terminal. The candidate also contains the installed November 5
framebuffer cleanup and terminal output backpressure fixes.

Recovery uses the existing **Recover to shell…** confirmation. It prepares a
fresh shell, retires the exact saved owner/source reference, waits for confirmed
process exit and PTY EOF, then records a new pane epoch and launch intent before
opening the replacement. Saved agent commands and pending launch permissions
are cleared. A lost Open response is resolved through discovery, never by
replaying the command. Existing views follow the new epoch independently.

Candidate automation adds mutating `panes.recover` and `panes.reap` operations.
Both take `project`, a one-based `pane`, and its exact `epoch`; recovery also
requires `confirm: "RECOVER"`. Use ordinary versioned operation IDs and their
durable receipts. Reaping refuses a live source and removes membership only
after the old source is retired. Existing `panes.terminate` also fences the pane
epoch immediately before retirement. Empty projects stay empty until a user
explicitly opens a new terminal.

Retirement requires the owner's additive `retirement-eof-v1` capability.
Compatible older owners can continue serving reads and input while unsupported
recovery/termination is rejected and advertised as unavailable. Older foundation
owners lacking workspace-adapter capabilities refuse activation entirely and
keep their original processes. No owner is stopped merely to upgrade it.

An existing tmux source keeps its backend when the new frontend enables native
terminals; new native siblings can coexist with it. Malformed ownership maps,
null/invalid references, wrong pane/project identities and conflicting external
ownership fail closed instead of choosing a legacy fallback.

A reaped parent whose descendant still holds the PTY remains observable and
blocks replacement. This prevents duplicate agents; automatically terminating
stubborn/detached process trees and safely resolving owner-loss or never-delivered
launch intents still need further supervision work. This is not a completed
native-backend activation or a full graphics-compatibility claim. See the latest
checkpoint in the [implementation log](implementation-log.md).

On the project host, a private data namespace owns `native-sources/`, containing
`owner.json`, `token.json`, `broker.sock`, lifetime/startup locks and `launches/`.
This is separate from `$XDG_RUNTIME_DIR`: removing runtime state does not remove
the native owner's socket or launch receipts. Descriptor-relative Linux socket
paths support deeply nested data paths without relocating the socket into an
ephemeral directory. Metadata is private, owned, bounded and written with
fsync/atomic rename. Symlinked/hardlinked files, replaced directories and
incompatible records are rejected.

Discovery serializes concurrent clients and validates boot UUID, PID, start time,
instance, Unix peer credentials and required protocol capabilities. A compatible
live worker is reused, including after its loaded executable is removed during
an upgrade. A live but unreachable/incompatible owner is preserved and reported;
a timeout is not authority to stop it or launch a competing owner. Lifetime locks
also protect the interval before a new worker publishes its descriptor. The
worker starts in its own session with no inherited SSH stdio. Existing-only
attachment requires the recorded owner and cannot launch a replacement.

Internal Linux entry points:

- `--native-source-service DATA_NAMESPACE` runs the retained owner.
- `--native-source-discover DATA_NAMESPACE` explicitly discovers/creates it and
  returns non-secret owner identity.
- `--native-source-stdio DATA_NAMESPACE` explicitly discovers/creates an owner
  and carries bounded versioned JSON Lines over stdin/stdout.
- `--native-source-attach DATA_NAMESPACE OWNER_JSON` connects only to that exact
  retained owner and carries the same stream.

The connector injects authentication on the project host. Callers supply version,
request ID and action; they neither receive nor supply the token. Half-closed
stdin retains outstanding asynchronous replies. A lost connection or failed
write is an uncertain delivery, never permission to replay terminal input.
Normal application adapters must use the source operation guards and durable
operation receipts around this boundary. New workers can start through the
caller's loaded `/proc/self/exe`, including after its disk pathname is removed.

`panes/PROJECT/pN.json` records the project/pane epoch, owner identity, source
UUID/epoch, launch digest and optional confirmed process reference. A launch
intent is persisted before the command reaches the owner. Repeated identical
explicit opens share one result; a changed intent cannot reuse that epoch.
Replacing a pane requires the expected prior record and a new terminal epoch.
The receipt layer independently prevents consumed source IDs from executing
again after worker loss or a failed exec.

Read-only reattachment validates the recorded owner, source and launch digest.
Looking up an absent pane or reconnecting to a missing owner leaves missing
metadata directories absent. Only the explicit creation path makes them.
A pending record can recover a source that exists despite a lost Open reply; it
cannot execute a saved command. If the owner/source is gone, recovery requires a
new explicit shell/conversation intent. A stale completion cannot overwrite a
newer pane, and a source created during a concurrent pane change is kept running
rather than killed implicitly. Records contain digests/references, not saved
argv or terminal contents.

Native commands restore `STARFLUX_SYSTEM_PATH` when launched from a packaged
bundle and remove the bundle's media/audio overrides, matching the existing
host helper. They do not inherit the presenter's Kitty/TMUX protocol markers.
This does not yet supply automatic agent-provider lifecycle hooks.

Acceptance uses only private fake sources and a private loopback SSH daemon:

```sh
python3 scripts/test-source-broker.py --service --streams --executable target/release/examples/native-source-broker --worker-executable target/release/starflux
python3 scripts/test-source-broker.py --ssh --service --streams --executable target/release/examples/native-source-broker --worker-executable target/release/starflux
```

These cover concurrent discovery/open, durable/pending references, stale writes,
runtime loss, loaded-owner upgrades, unsupported-version preservation, explicit
fresh recovery, host tool selection and the underlying streamed frame checks.
They do not prove real host graphics rendering or WAN latency.

Remaining production integration includes model transactions and slot cleanup;
local/SSH health, input, history, snapshot and lifecycle adapters; compatible
observation/adoption of existing sources; sharp host image layers; clipboard,
title and notification forwarding; verified graphics/transport resync; and
interrupted-creation/process-group supervision. App Space keeps its independent
media/input paths. Do not restart a live owner merely to load a new binary.

### Native presentation candidate

The isolated `target/inline-graphics-workspace` now connects ordinary native
panes to the retained owner through a multiplexed local/SSH connector. It uses
pushed, acknowledged row frames; it does not poll an SSH command for each frame.
Native attachments do not create another terminal PTY or execute a proxy shell.
They send raw input and source-side bracketed paste through the same connector.
Hidden attachments unwatch, and closing a view leaves its source running.

A view keeps its own palette and desired viewport. If another frontend shrinks
the source, a read-only inspection establishes the current grid and a new watch
epoch requests its available rows/columns; remaining view space stays blank.
This avoids repeatedly resizing the source to satisfy an unchanged old view.
Frame reconstruction runs on a bounded worker queue apart from connector input.
A presentation error does not revoke input. Rejected input cancels remaining
unsent queue/tail packets; partly transmitted input stays indeterminate and is
never replayed. Connectors stop accepting new view identities after their
protocol tombstone allowance, allowing later views to use a fresh connection.

Wheel review in native panes requests retained source history, since pushed view
projections contain only visible rows. The controller dispatches that request
for both graphical and cells presentations, with terminal-epoch checks before
and after capture. A view's review remains independent of other frontends.

This remains an isolated candidate, with no installed backend switch. Its
current acceptance evidence and remaining requirements are recorded at the end
of `docs/history/implementation-log.md`. In particular, sharp host image planes, rich
terminal events/links, broader recovery/lifecycle acceptance and complete
frontend resource accounting still need work. The candidate's cell fallback
does not provide sharp graphical images.

The candidate now retains presentation reservations with conversion canvases,
accepted UI screens and frozen review screens instead of releasing them at
handoff. Source-history parsing reserves its screen/search working set before
allocation. Process-wide payload budgets cover queued/in-flight input (16 MiB),
presentation/history working data (128 MiB), incoming wire/JSON working data
(192 MiB) and encoded output (32 MiB), plus 32 connector slots. Per-view input,
message-size, queue and protocol watch limits remain in force. Incoming
admission precedes buffer growth/JSON parsing; encoded output is measured and
admitted before allocating its byte buffer. These are conservative payload and
working reservations, not total process RSS limits. Exhaustion reports busy;
a retained source is never restarted to free presentation resources.

A failed connector reattaches only to its recorded owner/source, using a new
view identity. It does not rediscover/create an owner or repeat a launch/input
intent. All unsent input from the failed connection is discarded. When delivery
was uncertain, the reconnected terminal reports that queued input was not
replayed and asks the user to check their draft. The delivery socket shares the
new input sink, so desktop/file delivery does not remain attached to the failed
view. Losing an owner still needs explicit source recovery; view reattachment
cannot recreate its process.

Frames advertise `source_grid` under the `view-grid-v1` owner capability. A
clamped view requests a fresh watch epoch when the retained source grows again;
it does not remain stuck at its previously smaller crop. This does not resize
the source. Owners without the required view capabilities are kept running and
reported as incompatible with this candidate view adapter.

The candidate's sharp image path now uses optional `Watch.pixels` plus the
`image-planes-v1` owner capability. Graphical views negotiate the local helper,
receive lossless planes, and keep text native; cells retain the half-block
fallback. Native review, hidden/removed/replaced panes and App Space transitions
clear the view's image placements. Image updates use a bounded, asynchronous
latest-per-view delivery queue so rendering cannot hold the font/probe mutex or
stop ordinary protocol packet forwarding. See [inline graphics](../inline-graphics.md) for the
implemented path, actual offscreen Kitty evidence and remaining compatibility,
recovery and resource/transport limits. None of this candidate has been activated
in the installed sessions.

### Candidate process supervision acceptance — November 14

The isolated native candidate now uses one subreaper per PTY. Its source identity
is the supervisor PID/start identity; a recognized foreground agent has its own
PID/start identity. Program and working-directory inspection follows the verified
foreground process, including a plain shell with no recognized agent.

Closing or recovering a source requests graceful shutdown of its owned tree,
escalates through pidfds, and waits for every descendant to be reaped and for PTY
EOF. This covers stubborn PTY holders, detached children which closed stdio,
nested subreapers and children forked during retirement. Idle supervisors sleep
on signals rather than polling. A natural root-command exit also retires its
remaining descendants.

New retirement requires `retirement-tree-v1` as well as `retirement-eof-v1`.
Older owners remain readable and interactive, with unsupported lifecycle controls
disabled. Abnormal supervisor exit leaves `retirement_verified: false`, retains
the binding and refuses a replacement even if EOF is visible. This is safe
refusal, not completed broker/owner-loss recovery.

Private local/SSH automation and real graphical/cells confirmation dialogs pass,
including independent views, replay guards and empty-project reopening. Real Bash
job control and foreground command/directory checks also pass. Source and evidence
identities are saved in the `native-process-supervision` checkpoint (merged on 2026-11-19; kept in git history).
This is still uninstalled: the candidate must be rebased onto current application
settings/presentation and complete the remaining native ownership, metadata,
graphics and release gates before activation.

### Candidate hyperlink forwarding acceptance — November 14

The next isolated stage adds optional `osc8-spans-v1` projection metadata: a
complete bounded target dictionary and disjoint cell spans accompany each frame,
including link-only deltas. Views admit/validate the table before owned decoding
and replace parser annotations without repainting text/styles. Old owners may
omit the table. Invalid or excessive metadata refuses projection while preserving
source/input and the last accepted presentation.

Native retained-history export now preserves OSC 8 destinations and parameters
alongside styles, within its existing 2 MiB bound. Two-view graphical/cells checks
locally and over SSH cover hidden destinations, wide/combining labels, replacement
and clearing on unchanged text, copy responses, pointer regions, frozen review,
resize and connector reconnect. Explicit source recovery clears old targets in
both views. This is link forwarding, not general clipboard/title/notification
forwarding or completed native source migration.

Source patches and reproducible validation identities are saved separately in
the `native-hyperlinks` checkpoint (merged on 2026-11-19; kept in git history). Apply it after the supervision stage; that
earlier stage's historical evidence is not reused as proof of the new metadata.
The installed backend remains unchanged, and the candidate still requires current
application/UI rebasing and the remaining ownership, graphics and release gates.

### Candidate terminal metadata acceptance — November 14

The next isolated stage advertises optional `terminal-metadata-v1`. The owner
retains bounded OSC 0/1/2 title/icon values, bell counters and UTF-8 OSC 52 writes.
Native views receive that metadata separately from cell text. Default tab labels
can use the retained title; explicit user names, agent labels and imported names
keep precedence. Diagnostics expose sequences/counters, not clipboard contents.
Bell counters are informational and do not yet drive global attention delivery.

A watch captures a source-scoped clipboard baseline at attachment. New writes
reach attached frontend clipboard responses; late attachment, hidden-tab reveal,
resize and reconnect do not replay old values. Applied-frame acknowledgements
advance each watch independently, so later frames omit acknowledged clipboard
text. Slow views coalesce to the latest retained write. Read-only snapshot RPCs
omit actionable copy payloads. Clipboard reads are never serviced or forwarded.
This is bounded latest-value delivery, not durable exactly-once offline copying.

Titles are limited to 4 KiB each, clipboard text to 64 KiB, and accepted metadata
lazily reserves 192 KiB from the shared owner budget. Invalid values retain the
last accepted state and increment a diagnostic rejection counter. Source and view
snapshot allocations remain separately reserved.

The managed wire test also exposed a supervision upgrade bug: a retained owner
used `current_exe()` after its original file had been unlinked. Supervisors now
launch through `/proc/self/exe`, preserving the loaded owner image instead of
using a deleted path or accidentally executing its replacement. Private local
and SSH tests unlink an owner copy before opening supervised sources.

This stage's source and validation identities are in
the `native-metadata` checkpoint (merged on 2026-11-19; kept in git history). It remains uninstalled; broader owner-loss/adoption,
attention forwarding, graphics compatibility, current application/UI rebasing and
canonical release/Nix gates remain before activation.

### Candidate unstarted-launch recovery — November 14

The isolated stage after terminal metadata adds optional `launch-fence-v1` for a
saved intent whose owner is still live. Explicit recovery first discovers any
existing source. If none exists or is initializing, the owner can durably seal
an unstarted launch ID before a fresh shell is assigned a new terminal/source
epoch. Delayed delivery of the old Open then fails exclusive receipt creation.
Saved commands are not replayed. Existing sources still use guarded discovery
and verified tree retirement, including compatible owners without the new fence.

Status inspection does not consume the launch ID. Initializers in progress,
consumed/damaged receipts, mismatched identities and dead/unreachable owners
refuse cancellation. A fence binds the exact owner, source ID/epoch and digest;
bounded receipt reads reject links/nonregular files and use nonblocking opens.
Ordinary startup failure for a saved native reference now retains a read-only
health monitor, exposing the existing recovery menu/confirmation without another
attempt to run its command. Termination retains its separate retirement gate.

Source and validation identities are saved in the `native-launch-fence` checkpoint (merged on 2026-11-19; kept in git history).
This remains an uninstalled stage. General owner-loss recovery still needs
verified process-tree retirement/adoption evidence; absence of a broker or PID
is not sufficient proof. Remaining migration, presentation rebasing and release
requirements are unchanged.

### Candidate certified owner-loss recovery — November 14

New native supervisors pin their exact owner with a pidfd and retain a private
close-on-exec journal directory handle. On owner death they retire their kernel-
owned process trees, including detached descendants and nested subreapers.
After every descendant is reaped and `waitpid` reports `ECHILD`, they atomically
write/fsync a retirement certificate binding owner, source, epoch, launch digest
and exact directory. Idle supervisors still block without periodic polling.
Frontend or SSH-connector disconnection leaves the owner alive and does not
trigger this source-retirement path.

Read-only recovery health accepts only matching certificates and a finished
supervisor. An explicit graphical/cells confirmation or guarded automation call
can then discover a compatible replacement owner and open a default shell with
fresh owner/source/terminal identities, without replaying commands. Lost Open
replies are supported even when the durable slot alone contains the old source
reference. Owner-loss panes remain visible for explicit action; they are not
automatically reaped merely because their owner died.

Missing/corrupt records, wrong identities, active or unverified supervisors and
unsupported owners refuse replacement. Supervisor loss does not become safe
merely because the broker and PTY also disappeared. Verification sends no signals;
existing live owners are never killed or upgraded to satisfy a new client.

Source/evidence identities are in the `native-owner-loss` checkpoint (merged on 2026-11-19; kept in git history). The stage is
still uninstalled. Missing-certificate and pre-supervisor startup uncertainty,
broader adoption, remaining attention/graphics/history/network cases, current
UI/core configuration rebasing and canonical release/Nix acceptance remain gates.

### Current-source rebase — November 14

The native candidate is now rebased into `target/native-rebase-workspace` against
canonical `5a956fdf` plus the current sidebar/grid/toggle edits. The old `72d017696`
UI baseline is no longer the integration target. Only runtime-config and snapshot
conflicts needed manual resolution: the native opt-in coexists with global theme
serialization rules, and snapshots contain both global appearance and native
metadata. Native palette initialization now reads core settings too.

Current portfolio terminology, global theme behavior, glyph bounds and all
uncommitted sidebar changes are preserved. Local/SSH graphical/cells theme tests
with native sources pass; the private Gruvbox Dark reference PNG is byte-identical
to the installed build's rendering. Native lifecycle/link/metadata and portfolio
regressions pass on the rebase. The immutable shared pin and Cargo.lock are unchanged.

the `native-rebased` checkpoint (merged on 2026-11-19; kept in git history) reconstructs this candidate directly from the current
canonical commit plus its saved UI patch; do not apply earlier native patches on
top of it. It is still uninstalled. Remaining recovery/adoption, attention,
graphics/history/network/media and canonical release/Nix gates remain before
production integration and activation.

### Saved conversations after deliberate recovery — November 14

The uninstalled candidate now retains an explicitly registered conversation when
native recovery changes its pane generation. A separate durable link records the
new generation without changing the original provider report, conversation ID or
resume arguments. Inherited reports remain stale and cannot establish current
health or admit old heartbeats. Repeated deliberate recovery retains the link;
an unrelated recycled slot cannot use it, and a newly registered run takes
precedence. The user can explicitly resume the saved conversation in a new tab.

Native-enabled cold-runtime acceptance verifies real recovery confirmations,
split restoration, historical screens, exact resume and saved-command non-replay
locally and over SSH in cells mode. Legacy lifecycle acceptance also passes.
the `native-conversation-recovery` checkpoint (merged on 2026-11-19; kept in git history) archives the incremental source and
fixtures separately from the rebased stage. The complete release gate and the
new increment's Nix checks have separate logs; the earlier Nix pass applies only
to the preceding source. Production integration and the broader unfinished goal
remain in scope.
