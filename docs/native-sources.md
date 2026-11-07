# Native source owner integration

The retained native PTY owner and its streaming connector are implemented as an
internal component. The normal workspace still uses its existing terminal
backends. These entry points do not migrate existing tmux/Herdr/transferred
terminals or enable inline graphics in the installed UI.

## Candidate recovery and compatibility (November 6)

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
checkpoint in [parity-progress.md](parity-progress.md).

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
of `docs/parity-progress.md`. In particular, sharp host image planes, rich
terminal events/links, broader recovery/lifecycle acceptance and complete
frontend resource accounting still need work. The candidate's cell fallback
does not establish graphical image parity.

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
stop ordinary protocol packet forwarding. See `inline-graphics.md` for the
implemented path, actual offscreen Kitty evidence and remaining compatibility,
recovery and resource/transport limits. None of this candidate has been activated
in the installed sessions.
