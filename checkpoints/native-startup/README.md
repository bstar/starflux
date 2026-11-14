# Supervised command startup acknowledgement — November 14

This uninstalled increment follows `native-rebased` and
`native-conversation-recovery`. Apply those saved source patches first, then
`source.patch` here. `fixtures.patch` extends the existing broker acceptance
fixture; its added proof checks apply when the owner advertises
`supervised-startup-v1`. Earlier direct-PTY owners retain their existing checks.

The normal release gate exposed a real regression: spawning a supervisor could
return a live source before that supervisor discovered that the requested program
did not exist. The supervisor now records the actual `Command::spawn` result in
private, atomically written startup metadata. The bounded initializer waits for
that acknowledgement before publishing the source. It validates owner instance,
boot, source ID/epoch/PID/start, launch digest and directory identity. A failed or
missing acknowledgement never makes the consumed source ID replayable.

When exec fails before the requested program starts, the supervisor also records
retirement only after ECHILD. An exact matching failure plus retirement record
and a dead supervisor lets explicit pending recovery proceed with a fresh shell.
Missing, mismatched or damaged proof refuses that conclusion. A successful exec
followed by program exit cannot be misclassified as never started. Failure to
publish a successful acknowledgement starts supervised retirement rather than
abandoning an executed child.

The acknowledgement wait is confined to the existing initializer thread. It is
bounded to ten seconds and does not add polling to idle supervisors or work to
the render/input loop. Startup metadata has a bounded error description and
uses the existing private descriptor-relative metadata writer. Deep data paths
do not require a new Unix socket or inherited descriptor.

Strict Clippy, release workspace unit matrices and binary/example builds pass.
There are 278 main plus 4 client tests with all features, and 277 plus 4 without
defaults; four main tests remain ignored. Standalone and managed-service streamed
broker checks pass locally and over SSH, including failed exec, consumed-ID
protection, missing proof, fresh explicit launch and retained sibling sources.
The private UI fixture verifies actual failed-exec recovery menus/confirmation,
two independent views and an unaffected sibling locally in graphical mode and
over SSH in cells mode. `receipt.json` records the ten log identities and the
reconstructed four-file source increment. Copy the saved UI fixture into
`target/` to rerun its relative-root launcher.

The production-named release checkout now contains this increment and the saved
conversation fix. Its new continuation reruns formatting, Clippy, both unit
matrices and binary/example builds before retrying the failed source-broker
command and the remaining behavioral checks. Earlier failed logs remain intact;
their completed prefix checks are evidence for the preceding source revision,
not a whole-gate pass on this one. The new Nix package has its own run and state.

No installed binary, user terminal owner, live agent or App Space was replaced.
AI recovery is not complete merely because a shell was restored: exact provider
conversation registration and verified native resume remain a separate required
part of the full Herdr parity goal.
