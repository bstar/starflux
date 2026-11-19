# Herdr parity: implementation checkpoint

Resumed at the user's request on 2026-11-01 after the 2026-10-30 pause.
The full parity objective remains unfinished. This document is a resumption checklist, not a completion claim.
Changes are in the working tree; this checkpoint does not commit, push or release
them. Preserve unrelated changes and the existing STAR/KIT checkout.


## Resumption audit: 2026-11-01

The latest reboot report was checked against the installed release again. The
open graphical controller was still using the older placement build. A preserving
refresh loaded the current executable without replacing its live shell or agent;
the resulting view reported one live pane and zero reconnecting panes. Private
graphical local/SSH and cells reboot tests passed, including restored input,
split layouts, saved conversations and no replay of consumed launch intents.
Delayed backend startup over SSH also recovered automatically. Evidence:
`target/reboot-latest-{local,ssh,cells,retry,refresh}.log`. The saved default is
still `herdr-default`; its replacement has been requested from the user and has
not been guessed. This validates recovery, not the unfinished parity objective.

The preceding goal work made verified progress: reboot recovery now retires
transferred PTYs using boot evidence, preserves conversation/layout metadata,
reopens shells without replaying agent commands, retries failed initial setup,
and durably saves default-session metadata. The release build, 123 Rust tests
(3 ignored), graphical/cells local/SSH reboot fixtures, startup-retry fixtures
and live-transfer regression passed. See `target/reboot-*` and
`target/startup-retry-*`. The eligible real frontend was refreshed and its two
previously dead terminals recovered; no live terminal was replaced. At that
post-reboot refresh there were zero App Space workers. The older count of 12
below is historical, not a current inventory.

Follow-up validation on 2026-11-01 found a separate exited-pane failure:
current tmux can retain a dead screen with an empty `pane_pid`. The host helper
now reports a nullable process identity instead of raising `int('')`, and its
fixture migration preserves leading empty status fields. The isolated host
suite verifies exited status, no implicit command replay, explicit shell
restart and preservation of another live pane. Local and SSH reboot fixtures,
release workspace build, formatting and strict release Clippy passed again
(`target/reboot-terminal-*`, `target/reboot-host-recovery.log`). The live view
was refreshed; its existing live shell and agent PIDs were preserved, while
the already exited empty-PID pane was reaped normally. The saved default remains
`herdr-default`; the user's intended replacement has been requested and has not
been inferred or changed. This does not complete the broader parity goal.

A further reboot audit on 2026-11-01 revalidated the installed local release
with graphical local/SSH and cells cold-runtime recovery, plus delayed startup
over SSH (`target/reboot-current-{local,ssh,cells,retry-ssh}.log`). These isolated
fixtures verify restored input, retained layouts and conversations, no replay of
old commands and preservation of unrelated processes. Read-only inspection of
the real active frontend reported one live pane and no reconnecting panes. No
real terminal received test input or was restarted during this audit. The saved
default remains unchanged pending the user’s intended session/project.

At resumption, `scripts/test-plugins.py` and its SSH variant both passed against
the current release (`target/plugins-resume-local.log`,
`target/plugins-resume-ssh.log`). This revalidates the existing process/API
foundation, not managed installation, plugin panes, link handling or full parity.
Native UI acceptance and complete command/log review are now implemented and
verified as described below. This turn is classified as progress: it changes the
shipping UI, adds executable acceptance coverage and records direct evidence.

## Preserve the running workspace

- Never restart or stop real terminal backends, brokers, agents, tmux servers or
  App Space workers to validate changes. Use private test namespaces and fixtures.
- Never type test input into a real agent. Do not print complete real configs,
  snapshots, process command lines or environments: they can contain broker keys.
- Retain compatibility paths for the old `starmux` directory and runtime/data
  names while older processes are alive.
- Build through `./scripts/dev` and retain `~/.local/bin/starflux` pointing to
  `scripts/run-local.sh`. Do not change the immutable STAR/KIT dependency or edit
  the unrelated dirty sibling checkout.
- Refresh only eligible frontends after validation, verifying existing terminal
  identities and the current App Space worker inventory. The historical Tab
  navigation refresh preserved 12 workers; the reboot recovery refresh preserved
  its zero-worker inventory. Do not infer today's count from either checkpoint.
  No real plugin has been enabled to test this work.

## Implemented foundations and their remaining scope

| Area | Implemented and previously checked | Still needed for full parity |
| --- | --- | --- |
| Agent identity and recovery | Durable conversation/run records, guarded lifecycle reports, health diagnostics, explicit exact-conversation resume in a new tab | Automatic provider adapters that populate identity and lifecycle correctly; real provider acceptance without prompting live user agents |
| Attention and notifications | Durable global inbox, filtering/search/read/dismissal, guarded navigation, configurable local desktop delivery and retention | Desktop notification delivery to the SSH client and provider integration coverage |
| Panes | Nested horizontal/vertical splits, focus, resizing, movement/swap, zoom, per-view layouts, close-without-stopping and guarded explicit termination | Cross-project moves and tab-group workflows; complete acceptance coverage |
| Automation | Versioned JSONL API, operation receipts, guarded input, event subscriptions/cursors, session imports, acknowledged per-window commands | Remaining session-management operations and source-owned input adapter gaps; audit the advertised API against actual Herdr workflows |
| Worktrees | Host-aware list/create/open/remove, identity and cleanliness guards, process/launch leases, grouping | Broader acceptance and integration with remaining agent/machine workflows |
| History | Search, match navigation, frozen review/copy mode, native/tmux/Herdr capture | Inline graphics are not retained by this text-history implementation |
| Machines | Saved SSH profiles, revision guards, onboarding/probes, host-specific projects and failure handling | Combined machine/agent controller view and broader multi-host coverage |
| Keyboard | Configurable prefix/direct controls, command help, keyboard menus/dialogs, pane navigation | Exercise new plugin surfaces; continue auditing all remaining surfaces |
| Cold start | Private bounded text snapshots, previous snapshot, launch receipts, read-only previews, independent background collector | Inline graphics and legacy pixel archives; complete restoration guarantees and failure coverage |
| Narrow screens | Compact menus/sidebar collapse, field scrolling, minimum-size handling, focused input and unscaled App Space crop | Continue visual/input acceptance as new surfaces are added |
| Plugins | Linked/managed Git packages, updates and offline rollback, guarded runner/API/native UI, complete command/log review, executable example, local/SSH graphical/cells UI acceptance, queue/backpressure coverage; native OSC 8 destinations and retention; normal persistent plugin terminals and transient popups with configurable keyboard controls | Browser handoff, broader hook semantics, complete multi-plugin/source/lifecycle coverage and remaining acceptance |
| Inline terminal graphics | APC framing, bounded RGB/RGBA/static PNG and zlib direct-transfer decoding, generation-bound image/placement store; private local/SSH raw transport acceptance | Connect backend ownership and source responses, complete protocol behavior, graphical/cells presentation, aggregate resource limits and restoration; no live image support advertised yet |

The original [TASKS.md](../TASKS.md) is the broader project backlog and contains
older acceptance criteria. Do not interpret this checkpoint as closing those tasks.
Existing feature documentation: [agents](agent-lifecycle.md),
[attention](attention.md), [panes](panes.md), [automation](automation.md),
[worktrees](worktrees.md), [history](terminal-history.md), [machines](machines.md),
[cold start](cold-start.md), [keyboard](keyboard.md), [compact windows](compact.md).

## Previous keyboard checkpoint: prefix-based panel navigation

Ctrl+B followed by Tab now hops between STAR/FLUX panels; prefix followed by
Shift+Tab reverses. Plain Tab passes through to captured terminal/app input.
The legacy global Tab behavior is opt-in via `"tab_switches_panels": true`.
Dialogs retain field traversal.
This was release-built, checked locally and over SSH in graphical/cells modes,
and refreshed into both eligible live views with session/worker preservation.

Evidence: `target/prefix-keyboard-build.log`, `target/prefix-keyboard-local.log`,
`target/prefix-keyboard-ssh.log`, `target/prefix-keyboard-cells.log`,
`target/prefix-keyboard-ssh-cells.log`, `target/prefix-keyboard-refresh.log`.
The Rust suite passed 123 tests (3 ignored). `target/` evidence is
local build output and is not a durable shipped artifact.

## Plugin checkpoint: new code, not a finished feature

Code is in `src/plugins.rs`, `src/plugin_store.rs`, `src/plugin_runner.rs` and
`src/plugin_ui.rs`, with dispatch in `main.rs`, `automation.rs` and `ui.rs`.
The manifest is native version-1 JSON named `starflux-plugin.json`; it is not
compatible with Herdr's TOML manifest. No new Cargo dependency was introduced.

Implemented:

- Local directory linking, initially disabled; revision-guarded configuration,
  explicit enable tied to the manifest digest, unlink without deleting source.
- Declared literal argv actions, package/project working directories and finite
  deadlines. Project-directory actions reject projects on another controller
  host. Scripts can explicitly invoke SSH themselves.
- Trusted user-code execution: this is not a sandbox. Manifest approval pins
  manifest bytes, not the contents of every linked script. No implicit shell.
- Detached finite jobs, private process groups, two concurrent slots, queue bound
  of 32, queue expiry, separate bounded stdout/stderr, cancellation/timeouts,
  committed outcomes and explicit indeterminate status after lost workers.
  Interrupted actions are not automatically repeated.
- JSON context on stdin and `STARFLUX_PLUGIN_CONTEXT_JSON`, plus plugin/action/job
  IDs, config/state directories and `STARFLUX_BIN_PATH` environment variables.
- Startup and domain-event hooks with enable-time cursors, activation guards,
  deterministic event job IDs and retention-gap reporting. The service reconciles
  domain metadata periodically and can continue without a frontend/API connection.
- Private, directory-incarnation-bound metadata writes; namespace removal retires
  the service. Child state directories use directory-relative operations.
- API methods `plugins.list`, `.link`, `.unlink`, `.configure`, `.run`, `.jobs`,
  `.job`, `.cancel`, `.service`; mutations use operation IDs.
- Async Plugins menu, local-link prompt, command/hook review, enable/disable,
  manual actions, job cancellation and paginated log text. Workspace menu and
  Ctrl+B, Shift+U entry points are wired in both presentations.

Focused fixture test: `scripts/test-plugins.py` (also `--ssh`). It covers literal
argv/Unicode, disabled/changed-manifest rejection, revision guards, operation
replay, logs/exit status, timeout and cancel child cleanup, output bounds including
escaped bytes, queued activation invalidation, startup deduplication, headless
project hooks, lost-worker indeterminate outcomes and namespace removal.
The first local run passed (`target/plugins-local.log`) before the final UI and
child-reaping checkpoint. Final checkpoint build/test results belong in
`target/plugins-pause-build.log`, `target/plugins-pause-local.log` and
`target/plugins-pause-ssh.log`.

Checkpoint results: the release workspace build completed successfully (nine
existing compiler warnings), and the focused local and SSH process/API suites
both returned `PASS`. `git diff --check` passed. These focused checks do not cover
the new UI, concurrency saturation or the full release matrix. The broader
release gate and strict Clippy were not rerun for the plugin checkpoint.

## Verified plugin UI and queue checkpoint: 2026-11-01

- The native Plugins workspace has stable geometry across loading, command and
  output pages. Complete argument arrays, descriptions, paths, hooks and long log
  lines wrap without losing their tails; wrapping respects terminal cell widths.
- Plugin log display renders control bytes visibly instead of passing active
  terminal control sequences through. Raw bounded output remains available in
  the API. Informational/loading rows no longer dismiss a menu when activated.
- Frontend diagnostics expose the plugin view, load state and request generation,
  allowing tests to distinguish a settled request from a presented pointer target.
- `scripts/test-session-manager.py --plugin-ui`, also with `--cells`, `--ssh`,
  and both flags, passed. It exercises keyboard entry, mouse/keyboard controls,
  linking disabled by default, complete review, enabling, execution, Unicode,
  all output pages, cancellation with child cleanup, stale registry conflicts,
  narrow-window controls, closed/replaced callbacks, independent frontends and
  unlinking with files/logs retained. Original terminal PIDs are preserved.
- The UI fixture runs the shipped `examples/plugins/project-summary` against a
  private Git repository and project, proving the documented executable ABI.
  User documentation is in [plugins.md](plugins.md), linked from API/key docs.
- `scripts/test-plugins.py --capacity`, also with `--ssh`, passed. Eight
  concurrent publishers fill exactly 32 queued jobs; the next enqueue fails.
  Two blocked actions run, further scans do not exceed that worker count,
  startup-hook queue backpressure still permits draining, and every queued
  command executes once. The original local/SSH process/API suites also passed.
- The release workspace build, formatting, Python/shell syntax, strict release
  Clippy for all workspace targets/features, and 124 Rust tests (3 ignored) pass.
  Compiler-recommended style fixes remove the prior strict-Clippy blockers.
  The entire broader release gate has **not** been run for this checkpoint.
- Release-gate wiring now includes both plugin process/API suites, both capacity
  suites and all four UI variants. It has no dependency or STAR/KIT pin changes.
- The eligible live frontend was refreshed from the final release build. Its
  terminal process identities and zero-worker App Space inventory were preserved.
  The real plugin registry was empty; no real plugin was linked or enabled.
  Evidence: `target/plugins-ui-refresh.log`.

Evidence: `target/plugins-ui-build.log`, `target/plugins-ui-{local,cells,ssh,ssh-cells}.log`,
`target/plugins-resume-{local,ssh}.log`, `target/plugins-capacity-{local,ssh}.log`,
`target/plugins-ui-clippy-release.log`, `target/plugins-ui-unit.log`,
`target/plugins-category-selection-regression.log`, `target/plugins-ui-format.log`.
These files are local output, not durable shipped artifacts.

## Resume plugin work here

### Managed packages and declared links: 2026-11-01

Managed Git/GitHub installation is implemented in `src/plugin_packages.rs` and
`scripts/plugin-package.py`, with native graphical/cells dialogs and API methods
for installation, updates, retained versions, offline activation and uninstall.
Packages are pinned to a commit and complete tree digest, initially disabled,
and require reapproval after updates/version changes. Existing jobs retain their
version's files; busy uninstall is rejected. Fetch/archive does not execute
package code. Two fetch slots, time/byte/output limits, owned staging cleanup,
parent-death cleanup and namespace-incarnation guards are implemented.

The managed package fixtures previously passed locally and over real private
Git SSH transport (`target/plugin-packages-{local,ssh}.log`), including unsafe
trees, changed code/assets, failed-update retention, in-use versions, rollback,
busy uninstall and interrupted fetches. A real HTTPS GitHub tag fetch/archive
smoke passed (`target/plugin-packages-github.log`); that smoke did not install or
execute a public plugin. Native UI fixtures cover managed install/update/version
switch/uninstall in all four presentation/transport combinations.

Declared `link_handlers` are implemented in `src/plugin_links.rs`: strict parsed
scheme/exact-host matching, optional custom schemes, read-only paginated
discovery, and explicit handler selection through Plugins → Open link with a
plugin… or `plugins.resolve_link` / `plugins.open_link`. URLs are literal JSON
context, never terminal input or command interpolation. Manifest approval,
project/host context and queued activation guards apply. One broken enabled
package does not suppress another package's matching handler. The native choice
view paginates, preserves its originating project context and uses existing
callback incarnation guards. Visible terminal URL activation is implemented in the next checkpoint; OSC 8
destinations are still unfinished.

Plugin service version 2 understands the extended manifests. New receipt version
2 prevents version-one services from claiming incompatible new work during
handoff; retained version-one job receipts remain readable. A replacement waits
briefly for cooperative service-lock release without stopping running jobs.
The private protocol fixture models an older service, checks new-receipt
exclusion, queue draining and preservation of an already running version-one
receipt. It is not a claim about arbitrary older third-party implementations.

Remaining plugin work includes persistent plugin panes with focus/lifetime/
restoration, OSC 8 destinations, broader hooks/provider interactions,
managed-version metadata corruption isolation, large registry/API pagination,
retention/pruning, and full source/lifecycle/packaging acceptance. Retained
versions currently require explicit uninstall to release their disk space.
The native JSON manifest is not Herdr TOML compatibility.

Current final-build acceptance passed:

- Declared handlers and cooperative service-upgrade protocol, locally and over
  SSH: `target/plugin-links-routing-{local,ssh}.log`. Includes read-only matching,
  pagination, multiple packages, corrupt-source isolation, unchanged literal URL
  context, project/foreign-host checks, approval/activation guards and replay.
- All four native UI variants: `target/plugin-links-ui-{local,cells,ssh,ssh-cells}.log`.
  Includes handler entry, explicit execution, next/previous handler pages, and
  the existing managed installation/update/rollback/uninstall acceptance.
- Local/SSH finite-runner and capacity regressions:
  `target/plugin-links-runner-{local,ssh}.log`,
  `target/plugin-links-capacity-{local,ssh}.log`.
- Release workspace build, formatting, Python/shell syntax, strict all-target/
  all-feature release Clippy, and release workspace unit tests: 127 passed with
  all features, 126 without defaults, 3 ignored in each configuration.
  Evidence: `target/plugin-links-{build,format,clippy,unit,unit-no-defaults}.log`.

The existing GIO dependency now enables its GLib 2.66 URI parser feature; no new
crate or STAR/KIT pin is introduced. The broader release/Nix packaging gate has
not been run for this checkpoint. These focused results do not establish full
plugin or Herdr parity. The real plugin registry was empty; tests did not enable
a real plugin, prompt a user agent or stop user terminal/App Space processes.
The eligible live frontend was refreshed from the final build; terminal process
identities and its current zero-worker App Space inventory were preserved
(`target/plugin-links-refresh.log`).

### Visible terminal URL activation: 2026-11-01

`src/terminal_links.rs` indexes visible native-grid URL text with cell-aware
regions, wrapped lines, scrollback and alternate-screen handling. Cache updates
follow grid/wrap changes, rather than pointer motion; hover does not read the
registry or query SSH. Limits are 512 visible links per terminal and 8192 bytes
per logical wrapped line; oversized lines and incomplete edge URLs are skipped.
This is visible URL text, not OSC 8 destination metadata.

`src/terminal_link_ui.rs` routes right-click to copy/review/options, Ctrl-click to
the explicit handler chooser, and Ctrl+B, L to a keyboard URL picker. The Plugins
menu also opens that picker. Ordinary selection/application input is preserved.
Pointer regions are suppressed by overlays and App Space. Embedded graphical
canvas coordinates and window pointer coordinates map to the same native grid.
Menu routing preserves the originating link view until its action validates it;
opening options cannot silently execute a handler. The legacy pixel renderer
does not gain native-grid URL controls.

Acceptance passed in graphical/cells modes locally and over SSH:
`target/terminal-links-ui-{local,cells,ssh,ssh-cells}.log`. The fixture verifies
emitted URL hit regions, raw-window and embedded-canvas routing, exact URL copy,
the keyboard picker, explicit handler selection and independent frontends, then
runs the existing managed package and plugin UI regressions. It preserves its
private project's terminal PID. Actual cells-client PTY tests also passed locally
and over SSH (`target/terminal-links-host-{cells,ssh-cells}.log`), observing OSC 22
hand-pointer feedback and exact URL delivery through the clipboard protocol;
link controls do not leak into the private program's input.
Native sidebar-menu preservation and graphical render-cache reuse regressions
passed separately (`target/terminal-links-{context,cache}-regression.log`). The
harness selects one mode per run; combining both switches does not exercise both.

The release workspace build, formatting, strict release Clippy for all targets/
features and release Rust tests passed: 129 tests with all features, 128 without
defaults, 3 ignored in each. Evidence: `target/terminal-links-{build,format,
clippy,unit,unit-no-defaults}.log`. The complete release/Nix packaging gate remains
unrun for this milestone. The full goal stays active: OSC 8 hidden destinations
and their lifetime/restoration, persistent plugin panes, the remaining plugin
audit and every other unfinished area in the table above still require work.

After preparing the runtime, the eligible real frontend was refreshed from this
build. Its terminal process identities and current zero-worker App Space
inventory were preserved (`target/terminal-links-refresh.log`). The real plugin
registry was still empty; no user plugin or agent was started for validation.

### Native OSC 8 retention and parser throughput: 2026-11-01

The checked-in vt100 extension now retains hidden destinations and parameters
through paint/erase, insert/delete, scrolling, resize, alternate buffers, cloned
history and full/diff/cold-screen serialization. The native link controls use
these attributes ahead of URL-shaped labels. Native/tmux attachments advertise
hyperlinks per client and history captures retain escape attributes. Invalid or
oversized OSC strings cannot publish partial destinations. UTF-8 continuation
bytes are not mistaken for C1 terminators; C1 encoding remains unsupported.

The parser preserves upstream Unicode storage and 32-byte cells without a
per-cell destructor. Optional row vectors hold shared immutable link metadata.
This replaces an initial per-cell Arc implementation that measurably slowed
ordinary output. `scripts/bench-terminal-parser.py` uses the hash-verified original
crate and matching optimization/overflow settings in a private offline build.
The final seven-trial median comparison reported 15.516/15.796 ms for plain text
and 12.622/12.375 ms for styled Unicode (new/original, ratios 0.982/1.020).
These are parser throughput observations, not claims about network or input
latency. Evidence: `target/osc8-final-bench.log`.

Bounds: 8192-byte destinations, 1024-byte parameters, 128-KiB OSC bodies and an
8-MiB shared target-allocation budget. Saved-grid serialization stops at 1 MiB;
host captures stop reading beyond 2 MiB. Backend retention limits still apply;
private tmux probes showed sufficiently long destinations can be discarded before
reaching Starflux. Adjacent pointer regions are merged without making unlinked
cells clickable. Hover never reads plugins or SSH.

The OSC 8 release snapshot passed formatting, strict all-target/all-feature
Clippy, 141 all-feature / 140 no-default release tests (3 ignored each), dependency
advisory/license/source checks, release build and the full isolated Python gate.
The original serial run (`target/osc8-release-gate.log`, session 11908) stopped at
a stale drag-test coordinate assumption: category headers now occupy real sidebar
rows. The corrected fixture still asserts the exact order, cross-view sharing
and restart persistence. Its local run passed (`target/osc8-drag-fixture-local.log`),
and the remaining gate commands, including SSH cells session management,
render-cache/font/exit checks, multiwindow SSH scrollback and actual local/SSH
cells clients, passed in `target/osc8-gate-tail.log` (session 71113). Both handles
are completed; do not restart them merely to observe their results. Together
these runs cover the gate on the unchanged OSC 8 release binary. An archived copy
and checksum are at `target/osc8-validated-starflux` and
`target/osc8-validated-binary.sha256`.

All four plugin UI variants verify hidden destination copy, review, explicit
handler delivery and cold-preview retention. After preparing the runtime, the
eligible real frontend was refreshed; terminal process identities and the current
zero-worker App Space inventory were preserved (`target/osc8-refresh.log`). The
real plugin registry was not enabled or used for acceptance.

The separate Nix flake check completed successfully against a private source
snapshot with canonical manifests/lock and the unchanged KIT pin (x86_64-linux
package, 141 release tests and installed executable smoke checks). No temporary
KIT override was used. Evidence: `target/osc8-nix-check.log` and source snapshot
path in `target/osc8-nix-source-path`. ARM64 was not checked locally. Documentation
updates after the snapshot do not change the validated executable source.

Persistent plugin panes and the full parity table remain unfinished. Browser
handoff and broader hook/source lifecycle acceptance are still needed. Continue
with actual terminal-pane placement/focus/ownership/restore behavior after
finishing the active gate; finite command jobs do not satisfy plugin-pane parity.

### Persistent plugin terminals: 2026-11-01

`src/plugin_panes.rs` implements native persistent PTY entrypoints rather than
finite jobs. Pane-only manifests, complete command review, explicit enable/open,
view-local tab/split/zoom/overlay placement, an owned-terminal menu and guarded
list/open/close API are wired into the application. Overlay restoration stores
only presentation data, so closing or restarting an overlay preserves the prior
layout and concurrent shared terminal membership. Dismissing a pending open
leaves its terminal available without stealing focus or replacing another menu.
Popup presentation is explicitly rejected before allocation; it is not being
represented as an ordinary saved project terminal.

Durable ownership records bind the plugin/source version, digest/activation,
project, terminal slot/epoch, boot and process identity. A launch receipt is
consumed before exec. Reattachment never replays the command or substitutes a
shell; explicit exited-terminal recovery revokes ownership and changes epoch.
Independent binding records preserve ownership if an older view drops an unknown
model field. Close checks reject stale epochs and previous-boot pending launch
records; source updates preserve the running version. Managed uninstall includes
owned terminals in its busy guard. Plugin worker service/job protocol version 3
protects new manifests during cooperative upgrades and still reads v1/v2 job
receipts. The v2 service fixture skips new receipts while a v1 running job survives.

Validation on the current release build:

- Headless PTY/API local and SSH: `target/plugin-panes-{local,ssh}.log`. Tests
  literal argv, interactive input, operation replay, consumed launcher rejection,
  old-boot/stale close guards, managed source update/reapproval, busy uninstall,
  independent owned terminals and preservation of the ordinary project shell.
- All four graphical/cells local/SSH UI variants:
  `target/plugin-panes-ui-{local,cells,ssh,ssh-cells}.log`. Each exercises all four
  normal placements, real PTY keyboard input, owned-terminal focus, overlay
  frontend restart without command replay, prior layout restoration, independent
  windows and delayed callback preservation of focus/an unrelated help menu.
  The private shell fixture now implements tmux's standard `-c` execution mode;
  it previously swallowed the launcher and left a receipt pending. Product code
  was not weakened to accommodate that fixture.
- Service upgrade/link regressions local/SSH:
  `target/plugin-panes-upgrade-{local,ssh}.log`; SSH cells reboot recovery:
  `target/plugin-panes-reboot-regression.log`.
- Release build, formatting, strict all-target/all-feature Clippy and release
  tests: 143 all-feature / 142 no-default tests passed, 3 ignored in each.
  Evidence: `target/plugin-panes-{build,clippy,all-unit,no-default-unit}.log`.
  The installed PATH wrapper and harmless version command were verified.

These test handles have completed. The broad release/Nix checks recorded above
validate the earlier OSC 8 binary, not these later plugin-pane changes. The new
focused suites are added to `scripts/check-release`; run the updated complete
release/Nix gates when the pane implementation reaches its next integrated
checkpoint. No real plugin was enabled, no real agent received test input and no
terminal/App Space backend was stopped. The real frontend was refreshed only to
the validated OSC 8 snapshot; it has not been refreshed to this new pane build.

Next implementation work:

1. Implement a singleton transient popup terminal, with cell/percentage sizing,
   modal-busy refusal, full terminal input (including Escape), process-exit/close
   cleanup and focus restoration. Keep it outside project pane/layout/snapshot
   membership and pane lifecycle/agent APIs. Scope its reservation to the actual
   session and preserve other frontends.
2. Add frontend-targeted, acknowledged placement/focus API controls. The present
   open API reports `presentation: "not_requested"`; it must not silently claim
   that headless creation applied the manifest placement to a window.
3. Cover pending-launch cancellation/failure, corrupted ownership/bindings,
   unlinked/replaced controller executables, deleted projects, retained record
   limits, native/remote source affinity and App Space coexistence. In particular,
   launcher argv currently uses `current_exe`; a removed loaded executable needs
   a retained compatible launcher or an explicit pre-launch failure, not a dead
   terminal that appears to have started. Do not infer this from read-query helper
   upgrade coverage; that uses a different direct-spawn path.
4. Finish pane move/ownership integration as cross-project/tab-group workflows
   are implemented. Continue browser handoff, broader hooks and every original
   parity area below; normal plugin terminals are an increment, not full parity.

### Frontend-targeted plugin placement: 2026-11-01

The normal pane API now accepts optional `frontend` and `placement` on open,
and adds `plugins.panes.focus` for acknowledged placement without execution.
`frontends.list` advertises `plugin_pane_commands`; an older frontend is rejected
before an unknown command can enter its queue. Commands bind controller
incarnation, active project and owned pane id/epoch. Stale/busy/wrong-project
creation targets are rejected before backend allocation. The controller checks
again while applying placement, so a newly opened dialog or replaced terminal
cannot be overridden by a stale request.

Creation and presentation have separate outcomes. A lost window after creation
leaves its backend available and returns its record plus an unconfirmed/error
presentation result, rather than hiding the new terminal behind an API failure.
A controlled registry-lock barrier in the UI fixture verifies this exact race.
Open operation replay returns the same record and does not rerun the command.
An explicit normal placement can override a declared popup default, while actual
popup presentation remains unimplemented and cannot allocate an ordinary pane.

Launcher resolution now handles the Linux ` (deleted)` path annotation after
binary replacement. A missing launcher with no replacement is rejected before
terminal/model allocation; its existing controller and shell remain alive. The
private upgrade fixture reopens the same disposable entrypoint across overlay
restart, replaces/unlinks only its own ELF and checks both paths. It does not
rename the installed executable.

Focused acceptance passed for graphical local/SSH (including launcher upgrade)
and cells local/SSH: `target/plugin-placement-upgrade-{local,ssh}.log` and
`target/plugin-placement-ui-{cells,ssh-cells}.log`. Coverage includes complete
command review, all normal placements, independent views, PTY input, overlay
restoration/restart, dismissed callback/menu protection, targeted open/focus,
preflight refusal, controller-side modal refusal, operation replay and retention
of a created backend when its presentation target disappears. Formatting,
release build and strict Clippy passed (`target/plugin-placement-{build,clippy}.log`).
The broader release/Nix gates for these later pane/placement changes are now
running, not yet proven complete. Poll exec session **27226** for the serial
release gate (`target/plugin-placement-release-gate.log`) and **97153** for the
Nix package/smoke check (`target/plugin-placement-nix-check.log`). Both were
confirmed live after release/unit/host checks and the first new pane fixture had
passed, while Nix was compiling dependencies. The Nix source snapshot is recorded
in `target/plugin-placement-nix-source-path`; it contains canonical manifests and
lockfile, with no KIT override. Do not restart either check merely because an
observation times out. The earlier OSC 8 checks are not reused as proof of these
later changes. Live frontends have not been refreshed to this pane/placement build.

The previous turn made implementation progress (normal plugin terminals and
executable local/SSH/UI acceptance), and this turn extends their actual native
API. Full parity remains active. Next implement the singleton transient popup
surface and its dimensions/input/cleanup, then continue the original remaining
areas. A popup must have no project pane membership, pane id, launch restoration,
agent identity or pane lifecycle events. Keep the focused underlying pane in its
context, preserve App Space workers, and validate owner death, child cleanup,
modal/singleton refusal, resizing and independent frontends with private fixtures.

1. Inspect the current code and checkpoint evidence; revalidate tests when new
   changes or failures justify it. Full plugin and Herdr parity remain unproven.
2. Extend graphical/cells acceptance as managed packages, panes, links and hooks
   are implemented; keep narrow-window, keyboard and independent-window coverage.
3. Expand process tests for project CWD/foreign-host guards, worktree removal
   leases, service restart/version handoff, cursor gaps and independent plugins.
   The two-slot/32-queue/concurrent-publisher/hook-backpressure cases above pass.
4. Review lifecycle and error semantics. Check source changes between approval,
   enqueue and execution; stale/corrupt registry/job/cursor files; output-reader
   completion; child reaping; direct-child parent-death behavior. Trusted code can
   deliberately escape a process group, so do not promise sandbox containment.
5. Review callback routing for multi-host contexts and installed/deleted binary
   paths. Jobs execute on the workspace controller; remote-host execution is not
   automatically staged. Define and test broader behavior before advertising it.
6. Managed installation is implemented (see the checkpoint below). Finish plugin
   panes with focus/lifetime/restore semantics and integrate declared handlers
   with complete hyperlink lifetime/restoration. OSC 8 retention is implemented;
   its gate is complete as recorded above. Continue the native manifest audit.
7. Extend the documented native ABI, executable example, API/keyboard references
   and release gates as the remaining capabilities are implemented. Current
   documentation explicitly lists the unfinished parity work.
8. Run formatting, appropriate unit tests, strict Clippy configurations, focused
   local/SSH suites and the broader release gate after resolving failures. A prior
   release gate passed before plugin changes; that does not validate this code.
9. Only after validation, refresh eligible real frontends while verifying terminal
   identity and retained App Space worker preservation. Do not enable real plugins
   merely to test the feature. Do not commit/push/release without authorization.

## Continue the full parity objective

After plugins, tackle automatic agent adapters, inline graphics/restoration,
cross-project/tab-group pane workflows, remaining session APIs, combined machine
view and SSH-client notifications. Preserve the original scope and validate each
feature incrementally. Finish with cross-feature reconnect/crash/recovery,
multi-host, responsiveness and long-running acceptance coverage. Full parity is
unproven until every remaining acceptance item has direct current evidence.

### Placement gate completion and reboot follow-up: 2026-11-01

The placement checkpoint's serial release gate finished with exit 0
(`target/plugin-placement-release-gate.log`, exec session 27226). Its Nix
package/smoke check also finished with exit 0 and `all checks passed!`
(`target/plugin-placement-nix-check.log`, session 97153), for x86_64 Linux.
ARM64 remains unverified locally. The checkpoint executable is preserved at
`target/plugin-placement-validated-starflux` with its SHA-256 alongside it.
These checks do not validate the subsequent popup source changes.

The real eligible frontend was refreshed to that placement executable during
the reboot follow-up. Its existing shell/agent identities were preserved, and a
previously absent shell was restored. No real plugin was enabled and no real
agent received test input. Saved default `herdr-default` remains unchanged pending
the user's intended selection. Private reboot recovery passed again locally and
over SSH, as did startup retry over SSH (`target/reboot-user-{local,ssh,retry}.log`).
The observed saved-screen error also exposed a valid parser cursor at the wrap
boundary; validation now accepts column == width, while rejecting coordinates
beyond it. ASCII and wide-Unicode archive roundtrips cover this source fix.

Transient popup implementation and its dedicated UI/PTY/owner-death/App Space
acceptance are now in progress. Strict Clippy and release all-feature/no-default
unit tests passed before the latest small changes. Do not treat these as popup
runtime acceptance: the new popup executable still needs focused tests and its
own broader release/Nix checks. Preserve the ongoing original parity scope.

### Transient popup terminal checkpoint: 2026-11-01

Native plugin popups now use a dedicated controller-owned PTY supervisor and
per-session reservation, without allocating a project pane, agent identity or
saved layout/snapshot membership. Declarations and API overrides accept bounded
cell/percentage dimensions. Input, visible rendering, resize and exact-ID close
are scoped to the popup. The supervisor verifies current source/approval, the
actual approved command and owner identity before spawning. Ordinary descendant
cleanup covers close, application exit, supervisor death and controller death;
PID reuse protection cleans a still-reserved group before reaping. Trusted code
can deliberately leave a process group; this is not sandbox containment.

The API exposes popup capability negotiation, targeted open, read-only status
and guarded close with durable operation replay. Its PTY-started acknowledgement
is explicitly separate from application readiness. Managed versions stay pinned
while running, and uninstall refuses live popups. The finite-job service/receipt
ABI is now version 4 so older version-3 services hand off before parsing popup
dimensions; version-one running jobs remain readable and are retained.

Focused graphical/cells local/SSH runtime acceptance passed, including command
review/opt-in, literal keys (Escape/Ctrl+B/F1), Unicode paste/output, PTY mouse,
resize down to 12x6, modality/session singleton, stale-ID refusal, operation
replay, no saved membership, application success/failure, child cleanup,
supervisor/owner SIGKILL and independent sibling processes/views. Logs:
`target/plugin-popups-ui-{local,cells,ssh,ssh-cells}.log`. The last tiny-viewport
background rendering change was rechecked in local graphical, SSH cells and SSH
managed fixtures. Actual cells-client local/SSH acceptance passed separately:
`target/plugin-popup-real-{cells,ssh-cells}.log`. Managed update/uninstall passed
locally/SSH (`target/plugin-popups-managed-{local,ssh}.log`), and finite-job
service v3-to-v4 handoff with a retained v1 running receipt passed both transports
(`target/plugin-popup-service-upgrade-{local,ssh}.log`). Normal pane placements
and loaded-executable replacement were rechecked over SSH
(`target/plugin-popup-normal-pane-regression.log`).

The private App Space/video/audio fixture also passed over authenticated loopback
SSH (`target/plugin-popup-app-space-ssh.log`). Popup input is isolated and the
next app key reacquires compositor focus after close; restoring the old captured
flag had skipped that request. Its worker survived, video continued at about
60 fps at unscaled 1920x1080 Y444, and PCM delivery continued. These private
measurements do not prove internet latency or physical audio playback. Terminal
background surfaces now honor native padding and safely clip tiny underlines.
The saved-screen wrap-boundary source fix is included in this new executable.

The broader popup release gate and a separate Nix source snapshot/check are next.
Their logs are `target/plugin-popup-release-gate.log` and
`target/plugin-popup-nix-check.log`; the source path is recorded in
`target/plugin-popup-nix-source-path`. Do not reuse the earlier placement gate as
proof of the popup code. Poll confirmed live handles instead of restarting on an
observation timeout. Live real frontends remain on the validated placement build;
no real plugin has been enabled, and no real agent received test input.

Popup implementation is an increment, not full Herdr/plugin parity. Continue
bounded record retention, corrupt/stale ownership repair, project/foreign-host
worktree guards, source races, transient keyboard escape controls and actual
graphical-host focus/capture acceptance. Then continue the original agent
adapters, cross-project/tab groups, inline graphics/restoration, session APIs,
combined machine view, SSH-client notifications, broader hooks and browser
handoff. Keep full parity active and do not commit/push/release without approval.

### Popup keyboard controls: 2026-11-01

The previous turn implemented transient plugin terminals and executable private
acceptance; it was progress. This turn adds native popup keyboard close controls.
`Shift+F6` (configurable `popup_controls`, nullable) focuses the close control;
Enter closes the owned popup group, Escape returns input to its program. Plain
Escape/Ctrl+B/F6 stay available to the program. Header navigation/paste are
contained, focus survives resize, and pointer down in the content returns input.
The popup keeps the same protocol input scope while its header owns focus, so
shortcut/Enter can arrive back-to-back without a presentation round trip.
The host repaint-key notice now sends Ctrl+L to the popup rather than the
underlying terminal, and is ignored during header navigation. The active popup
shortcut is exposed in the header/status and allowlisted diagnostic snapshot.

Focused candidate acceptance passed for graphical/cells local/SSH:
`target/plugin-popup-controls-ui-{local,cells,ssh,ssh-cells}.log`. This includes
the full core popup lifecycle suite as well as keyboard remap/disable/pass-through,
no header-key/paste leakage, resize and exact-group close. The actual cells client
also passed locally/SSH: `target/plugin-popup-controls-real-{cells,ssh-cells}.log`.
A test variable incorrectly reused the saved project-state variable for input
bytes; that fixture bug was fixed before both actual-client checks passed.
The private animated App Space/PCM acceptance passed over authenticated loopback
SSH with immediate keyboard close and restored app input, retaining the same
worker and about 60 fps at unscaled 1920x1080 Y444:
`target/plugin-popup-controls-app-space-ssh.log`. This does not prove physical
audio playback, internet latency or native graphical-host capture.

Canonical source formatting, strict release Clippy and release unit tests passed:
`target/plugin-popup-controls-clippy.log`,
`target/plugin-popup-controls-all-unit.log` (145 passed, 3 ignored), and
`target/plugin-popup-controls-no-default-unit.log` (144 passed, 3 ignored).
The configurable shortcut contract has its own unit test and remains outside
the global direct-key router. Full keyboard/app parity remains unproven.

To preserve the running release gate's original executable, these runtime tests
used a disposable release candidate. Its only manifest change is the binary
target name `starflux-controls`; dependencies, Cargo.lock and the immutable KIT
pin are unchanged. Source path: `target/plugin-popup-controls-source-path`.
The Nix development shell builds it into a separate artifact, then the test
workspace freezes its own executable. The canonical executable was hash-checked
unchanged after the build. The candidate checksum is recorded at
`target/plugin-popup-controls-checkpoint.sha256`. This is focused candidate
acceptance, not a completed canonical release gate or an installed/live update.

The preceding popup checkpoint's canonical Nix package/smoke check finished with
exit 0 and all checks passed (`target/plugin-popup-nix-check.log`, exec session
87793). Its serial release gate is still running as confirmed handle 43094
(`target/plugin-popup-release-gate.log`). Do not rebuild the canonical main
executable during that gate. The earlier placement executable remains loaded
in the real frontend; terminal/agent processes are preserved. The completed Nix
check predates these keyboard changes. A new canonical-source Nix snapshot/check
for keyboard controls uses `target/plugin-popup-controls-nix-check.log` and
`target/plugin-popup-controls-nix-source-path`; verify its handle and outcome
before claiming it passed. Source documentation can advance while its binary
checkpoint is being checked, but evidence must stay scoped to the tested source.

Continue bounded popup/job/pane history retention, corrupt/stale ownership repair,
project/foreign-host worktree and approval/source-race acceptance, and actual
graphical host focus/capture. Add the dedicated keyboard acceptance commands to
the canonical gate after its current run completes. Then build/test the complete
canonical source, preserve sessions during any eligible refresh, and continue
agent adapters, cross-project/tab groups, inline graphics/restoration, remaining
session APIs, combined machines, SSH-client notifications, hooks and browser
handoff. Full parity is active, not complete; do not commit/push/release without
explicit authorization.

### Collector handoff and inline graphics foundation: 2026-11-02 UTC

The popup keyboard checkpoint's complete canonical release gate and x86_64
Nix package/smoke check have finished successfully:
`target/plugin-popup-controls-{release-gate,nix-check}.log`. This supersedes the
in-progress status above; it does not validate later graphics source edits.
The current real frontend was subsequently refreshed as recorded in the reboot
audit above, preserving shell/agent identities and the zero-worker App Space
inventory.

Snapshot collectors now request service version 2 so loaded older collectors
cooperatively yield to code that accepts a wrap-pending saved-screen cursor.
Private local/SSH upgrade acceptance passed with both a v1 fixture and the
actual archived placement binary. Tests prove cooperative exit, preserved source
identity, new captures, old-client compatibility without downgrade, and an
archive accepted by the new binary but rejected by the old one. The first SSH
test dropped the fixture's service-disable environment option and raced a new
collector; forwarding that option fixed the test setup. Evidence:
`target/collector-v2-upgrade-{local,ssh,fixture-local,fixture-ssh}.log`.
The complete headless collector suite also passed locally/SSH:
`target/collector-v2-service-{local,ssh}.log`. Handoff fixture coverage is in
the canonical release gate now.

Inline graphics framing has moved from the isolated prototype into canonical
source. `src/graphics_transfer.rs` adds bounded direct chunk assembly and native
RGB/RGBA/static PNG decoding, with no filesystem reads or capability advertising.
Tests include fragmented APC framing, cancellation, final-chunk cursor placement,
exact palette/alpha pixels, malformed input and resource limits. Local and
authenticated loopback SSH control-mode transport tests passed while preserving
the fake source PID, geometry and input; these are now gate commands. Their
logs are `target/graphics-observer-{local,ssh}.log`.

This is not image display: backend ownership, placement/storage, graphical/cells
presentation, protocol replies, compression and restoration remain unfinished.
See [inline graphics](inline-graphics.md) for the concrete remaining work.
Current canonical graphics source checks use
`target/graphics-transfer-{clippy,all-unit,no-default-unit}.log`; inspect their
terminal results before claiming completion. Strict Clippy passed; the canonical
serial gate has re-run all-feature tests (158 passed, 3 ignored) and no-default
tests (157 passed, 3 ignored) against the final source, including bounded
arithmetic for maximum 32-bit image dimensions. The complete gate is still live
as exec session 41787 (`target/graphics-transfer-release-gate.log`). A frozen
canonical-source x86_64 Nix check is live as session 63265; source path and log:
`target/graphics-transfer-nix-source-path`,
`target/graphics-transfer-nix-check.log`. Poll these exact handles or verify their
process state; a log observation timeout is not a failed build. Do not rebuild
or replace the canonical main executable while the serial gate is running.

The paired parser benchmark against the preceding keyboard-controls parser
passed: ASCII ratio 1.013 and styled Unicode ratio 0.939 in this run
(`target/graphics-transfer-parser-benchmark.log`). The benchmark now accepts
`--baseline-path` and records a baseline source digest. These measurements do not
prove UI or SSH latency. The prior full release/Nix checks predate these source
edits. No live terminal has been restarted for this work,
and no real agent received test input. The full parity goal remains active.

### Retained graphics model and compression: 2026-11-02 UTC

The preceding transfer-foundation Nix package/smoke check finished successfully
on x86_64 (`target/graphics-transfer-nix-check.log`, session 63265). Its serial
release gate finished with exit 1 (session 41787): popup behavior passed locally
and over SSH, then the SSH cells fixture's temporary-directory cleanup raced a
detached controller writing notification-retention metadata. This was not a
completed release gate. The fixture now verifies private controller namespace
and PID/start identities, requests their closure and waits before directory
removal. It also cooperatively retires verified private snapshot collectors.
Only verified fixture processes can receive fallback termination signals.
SSH graphical/cells popup teardown passed after the controller fix; final
collector-aware teardown uses `target/graphics-gate-cleanup-*` logs.

Canonical source now includes `src/graphics_store.rs`: host/boot/PID/start/epoch
guards, ordered decoded-command commits, buffer-reset invalidation, atomic
image replacement/query behavior, named/anonymous placements, two-buffer quotas,
eviction, basic deletion, crop/fit/letterboxing and cursor-delta metadata. The
store caps total primary/alternate retained data at 64 MiB, 128 images and 1024
placements per terminal. Application-wide budgeting and production integration
remain unfinished. Letterboxed pixel bounds and requested cursor rectangle are
kept separate; extreme aspect ratios fail before narrowing arithmetic.

Direct transfers now support bounded zlib compression. Tests check exact
RGB/RGBA/PNG bytes, missing/oversized PNG size declarations, truncated streams,
extra compressed bytes and excess decompressed output. `flate2` is now a direct
dependency, using the same already-locked 1.1.10 package and checksum; no package
versions or STAR/KIT pins changed. The isolated candidate remains at
`target/inline-graphics-workspace`, with its explicit `starflux-graphics` binary
name and staging-only vendored-parser workspace declaration. Only the intended
graphics sources/dependency were promoted to canonical source.

The final canonical source passed strict Clippy, 167 all-feature release tests
and 166 no-default release tests (3 ignored in each):
`target/graphics-store-canonical-{clippy,all-unit,no-default-unit}.log`.
Candidate checks are `target/graphics-store-{clippy,all-unit,transfer-unit}.log`.
The transfer-only executable was archived at
`target/graphics-transfer-checkpoint-starflux` with its checksum before the new
store changes. This archival does not claim its failed runtime gate passed.

No terminal advertises or renders these images yet. The next required work is
the backend-owned observer/unwrapper and ordered decode/response pipeline, then
actual graphical/cells rendering, scrolling, source-aware archives and complete
protocol coverage. Other parity requirements in the table above are still
required; this foundation does not substitute for them. Do not stop real
backends or send agent prompts to test any of this work.

The complete new canonical serial release gate is running as exec session
43119 (`target/graphics-store-release-gate.log`); the frozen-source x86_64 Nix
check is session 50913 (`target/graphics-store-nix-check.log`, source path in
`target/graphics-store-nix-source-path`). Preserve their tested source/executable
until they finish; continue implementation in an isolated candidate meanwhile.
Do not restart a check because a log read or handle poll times out. Neither run
is a passing gate until its actual terminal result is verified.

### Native graphics stream candidate: 2026-11-02 UTC

The preceding store-source x86_64 Nix package/smoke check completed successfully
(`target/graphics-store-nix-check.log`, session 50913). Its canonical serial gate
is still live as session 43119. It has passed the SSH cells popup check that
previously failed during fixture teardown, then continued through managed
popups, actual cells input/close controls and plugin acceptance. Poll the same
handle; do not infer completion from these partial results.

The isolated candidate now includes `src/graphics_stream.rs`: bounded control
notifications, exact-pane octal output decoding, extended output, guarded command
blocks, explicit output gaps and nested tmux passthrough. Four wrapper levels
are accepted; a fifth, malformed framing or overflow faults publication until
the owner resets to a verified baseline. Plain non-escape output uses a borrowed
fast path. No production observer process, source input or graphics capability
advertisement is installed by this module.

`examples/graphics-stream-proof.rs` in that candidate is a disposable native
transport acceptance executable. Private local/SSH tests pass actual tmux output
through control parsing, unwrapping, vt100 callbacks, image decoding and the
retained store, checking the exact RGBA digest and placement. Pause/continue
acceptance also passed locally/SSH while the fake source kept receiving input,
retained its PID/start and geometry, and survived observer detach. Logs:
`target/graphics-stream-{local,ssh,flow-local,flow-ssh}.log`. The initial flow
test used an unquoted `%pane:pause` argument, which tmux rejected as syntax;
typed flow commands now quote that argument and have a regression test.

Candidate source is `target/inline-graphics-workspace`; its manifest still names
only the separate `starflux-graphics` binary, with the immutable KIT pin and same
canonical dependency versions. It has a staging-only `[workspace]` declaration
in its vendored parser manifest to make nested formatting work. Do not copy
that manifest over canonical source. Canonical source/executable remain frozen
for the running serial gate; the stream module, proof executable and enhanced
observer script have not been promoted yet.

Current candidate verification logs are
`target/graphics-stream-{unit,all-unit,no-default-unit,clippy,proof-build}.log`.
Inspect the actual final handles/results before claiming the latest edit passed.
The proof's synchronous decoder is deliberately confined to the disposable test
process. Production needs bounded asynchronous decoding, backend singleton
ownership, process/generation checks, initial-output coverage and explicit
source reply ordering. tmux itself handles ordinary DA queries, so merely
observing its output cannot establish that reply-order requirement. See
[inline graphics](inline-graphics.md) for the remaining pipeline and presentation
work. The wider parity requirements remain unchanged and unfinished.

The final candidate source passed strict all-target/all-feature Clippy and
release tests (175 all-feature / 174 no-default, 3 ignored in each configuration).
The proof executable was rebuilt after the last stream edit, and all four
local/SSH ordinary/flow-control native pixel checks passed again. Evidence is
the candidate logs above; these results do not prove production rendering or
the full canonical release gate. The canonical gate remains on confirmed live
handle 43119 at this checkpoint. Continue with source ownership/query ordering
and actual presentation after promoting validated candidate changes when that
gate has finished. Preserve the full original parity objective.

### Source response ordering candidate: 2026-11-02 UTC

The previous goal turn made concrete progress: the native stream candidate
decoded actual private local/SSH tmux output into exact retained pixels. Its
bounded pause/continue acceptance also passed. The canonical store-source serial
gate remains live on confirmed handle 43119, progressing through snapshots,
cold-runtime recovery and narrow-window acceptance. Its completed Nix check is
recorded above; do not claim the serial gate has finished yet.

Re-inspecting the existing local Herdr 0.9.1 source confirms that the parity
target includes Kitty retained images, media queries, Unicode placeholders and
`pane.graphics.info/set/clear/stream` APIs, not just forwarding a picture. The
relevant source lives at `target/herdr-source-v0.9.1`; no Herdr implementation
code was copied into the new modules. Full graphics parity remains unproven.

The isolated candidate adds `src/graphics_reply.rs`: generation-bound tickets,
64 bounded outstanding replies, exact source order, quiet/error/ID encoding and
stale/duplicate guards. An immediately ready DA waits for an earlier delayed
graphics query. Its four dedicated unit tests passed
(`target/graphics-reply-unit.log`). The candidate assembler also now honors
deletion during a partial upload: it discards incomplete pixels and yields the
delete command, rather than rejecting it as an invalid continuation.

`examples/graphics-owner-proof.rs` owns only a new private synthetic child PTY.
It deliberately completes each DA before resolving the earlier graphics query,
then checks exact replies at the child: success, invalid pixels, quiet mode,
unsupported file media and chunked queries, both direct and wrapped (ten cases).
This source-side proof passed locally and when launched over authenticated
private loopback SSH, together with actual tmux pixel decoding and observer
pause/continue: `target/graphics-owner-{proof-local,stream-local,stream-ssh}.log`.
It is not an installed terminal owner, does not attach to real sources, and
does not demonstrate that tmux can delegate its own query replies. No presenter
or real agent was opened or prompted.

The candidate passed 180 all-feature / 179 no-default release tests (3 ignored
in each), `target/graphics-owner-{all-unit,no-default-unit}.log`. Strict checks
and proof builds use `target/graphics-owner-{clippy,examples-build}.log`; verify
their terminal outcomes after the last source change. Earlier reply-only Clippy
passed (`target/graphics-reply-clippy.log`). Candidate root and staging manifest
differences remain as recorded above. Keep canonical source and executable
unchanged while serial gate 43119 runs.

Next: finish the source-owner request lifecycle, including early query barriers,
partial-transfer errors/cancellation and decode admission; connect verified
source ownership before tmux query handling; implement actual graphical/cells
composition, scrolling and archives, plus plugin graphics APIs. Do not treat
these small standalone proofs as shipping graphics support. The wider parity
matrix still applies, including agent adapters, session/machine workflows,
notifications and the remaining plugin/keyboard/acceptance work.

The final candidate strict Clippy and both proof-executable builds finished
with exit 0 (`target/graphics-owner-{clippy,examples-build}.log`, exec 78320).
The local and SSH owner/pixel/flow checks were re-run against those rebuilt
executables and passed. Serial gate 43119 remains confirmed live after passing
narrow-window acceptance in local/SSH graphical and SSH cells modes; preserve
its source/executable and continue polling that handle. None of these checks
marks the broader goal complete.

### Partial graphics request lifecycle: 2026-11-02 UTC

The previous goal turn was progress: it implemented source-generation response
tickets and tested them against a private PTY locally/SSH. This continuation
adds `src/graphics_requests.rs` in the isolated candidate, tying that response
queue to transfer assembly. Order is reserved on the first chunk rather than
waiting for the final image; DA cannot overtake an incomplete query. Initial
request identity survives malformed continuation errors, valid quiet overrides
apply, cancellation releases the earlier barrier, and deletion receives its own
identity after canceling the old upload. Source reset rejects old decode jobs.
Full reply queues reject new uploads before buffering their data. Unsupported
reply identities and oversized ready messages cannot wedge subsequent DA.

Seven dedicated request-lifecycle tests passed
(`target/graphics-requests-unit.log`). The private owner proof now uses this
driver and checks eight cases in each direct/wrapped form, including DA between
chunks, malformed base64 and APC cancellation. Eighteen DA replies are checked.
Local and authenticated loopback SSH acceptance passed, together with the
retained-pixel digest and observer pause/continue checks:
`target/graphics-requests-{proof-local,stream-local,stream-ssh}.log`.

Final isolated-candidate verification passed: strict all-target/all-feature
Clippy, 187 all-feature / 186 no-default release tests (3 ignored in each), and
both proof-executable builds. Evidence:
`target/graphics-requests-{clippy,all-unit,no-default-unit,examples-build}.log`,
completed exec 64915. The candidate root/manifest differences remain as described
above. No code from this pipeline has been promoted into the canonical source
being checked by serial gate 43119 yet.

The canonical gate is still confirmed live, having passed native agent lifecycle,
nested pane layout and local/SSH graphical/cells history acceptance. Keep its
source/executable unchanged until its actual terminal result is available.
Next still requires bounded byte/decode admission and a production source owner
before tmux replies, followed by actual graphical/cells rendering, scrolling,
restoration and graphics APIs. This candidate is not installed graphics support.
All other requirements of the original parity goal remain active as well.

### Shared graphics admission and canonical promotion: 2026-11-02 UTC

The preceding canonical image-store serial release gate finished successfully
with exit 0 (exec 43119, `target/graphics-store-release-gate.log`). Its matching
x86_64 Nix package/smoke checks also passed as recorded above. The tested binary
was archived at `target/graphics-store-validated-starflux`, with checksum in
`target/graphics-store-validated.sha256`. This supersedes its in-progress status
above, and validates that source checkpoint rather than later pipeline edits.

The next candidate added `graphics_resources.rs` and `graphics_decode.rs`:
shared in-flight accounting, two active decoders across resource clones,
bounded input/results, conservative admission before allocations, and result
leases that release after consumption. Upload reservation is 32 MiB while
assembling, then shrinks to actual vector capacity; the shared upload ceiling
is 128 MiB. Working/results ceiling is 512 MiB, with a 160 MiB scratch reservation
per decode and eight-entry queues. Results shrink their scratch charge to RGBA
capacity. Saturation is an ordered `EBUSY`, not a synchronous UI wait. Placement
and deletion commands remain available with no upload quota. Aggregate retained
images, fairness, idle-upload timers and production wiring remain unfinished;
these reservations are not a claimed bound on total RSS.

Tests prove shared accounting, concurrent reservation limits, cancellation/
error/reset/job-drop release, actual asynchronous decoding, saturated-pool
shutdown cleanup and no reply barrier left behind by decoder saturation.
The final focused candidate graphics suite passed 49 tests, and strict
all-target/all-feature Clippy and both native proof builds passed:
`target/graphics-resources-{unit,clippy,examples-build}.log` (exec 46212).
The prior full candidate suite passed 194/193 release tests with 3 ignored;
the final added shared-worker-slot test requires the canonical full recheck.

The private source-owner proof now sends jobs through the actual asynchronous
pool and checks zero leaked reservations after all direct/wrapped replies.
Local and authenticated loopback SSH owner/pixel/flow checks passed with this
pool: `target/graphics-resources-stream-{local,ssh}.log`.

After the preceding canonical gate was confirmed terminal, the validated stream,
reply, request-lifecycle and resource/decoder modules were promoted to canonical
source, with the two disposable proof examples and enhanced private observer
script. The gate now builds examples and runs local/SSH owner/pixel/flow checks.
No staging manifest, dependency override or vendored-parser workspace marker
was copied over; Cargo.lock and the immutable KIT pin were unchanged.

Canonical strict Clippy, all-feature/no-default release tests, workspace release
build/runtime preparation and example builds are running as exec 43587, with
logs `target/graphics-pipeline-{clippy,all-unit,no-default-unit,build,runtime,
examples-build}.log`. Verify that handle before claiming completion. A new
full serial/Nix gate still needs to run against these promoted modules. No real
terminal has been restarted or prompted; graphics remains unadvertised until
actual backend ownership and both presentation paths are integrated and tested.

The promoted canonical source completed strict Clippy, 195 all-feature / 194
no-default release tests (3 ignored in each), release workspace build, runtime
preparation and both example builds with exit 0 (exec 43587). The final extra
test enforces a shared two-decoder slot limit across resource clones. Canonical
local and SSH asynchronous-owner/pixel/flow acceptance also passed, including
zero leaked reservations:
`target/graphics-pipeline-{local,ssh}.log`. The installed PATH wrapper still
resolves correctly and `starflux --version` returns 0.2.0. The running user
frontend was not refreshed or restarted for these internal graphics changes.

A full serial canonical gate is now live as exec 65649
(`target/graphics-pipeline-release-gate.log`), and a new frozen-source x86_64 Nix
check is exec 60768 (`target/graphics-pipeline-nix-check.log`, source path in
`target/graphics-pipeline-nix-source-path`). Poll those exact handles and keep
the canonical source, main executable and proof examples unchanged until they
finish; the gate now executes those example artifacts too. Neither is a pass
until its terminal status is verified.

For continued isolated work, the candidate manifest now also sets
`autoexamples = false` and explicitly names its examples
`graphics-next-owner-proof` and `graphics-next-stream-proof`. Build only those
names or `--bin starflux-graphics` in the shared target directory; do not overwrite
canonical proof artifacts while the gate runs. These are staging-only manifest
differences, not dependency/pin changes. Retained-image aggregate quotas,
production ownership/lifetime supervision, rendering and the wider goal scope
remain unfinished.

### Stalled graphics upload expiry: 2026-11-02 UTC

The preceding turn was progress: the shared-resource pipeline was promoted to
canonical source and its focused canonical checks passed. Its x86_64 Nix
package/smoke check has now completed with exit 0 (exec 60768,
`target/graphics-pipeline-nix-check.log`). The serial runtime gate remains live
on exec 65649; keep its source/main/proof artifacts unchanged.

The isolated candidate adds monotonic upload deadlines: 30 seconds without
payload progress and 300 seconds total. Empty chunks cannot extend the idle
deadline; trickling payload cannot evade the total limit. Expiry discards the
partial image, releases its upload reservation and sends the original identity's
ordered `ETIMEDOUT` (or an empty response for quiet mode), then releases queued
DA replies. Orphan continuations are ignored rather than starting an anonymous
replacement after cancellation/expiry. Source reset and older timer ticks
cannot expire a fresh upload. The source owner proof ticks expiry while no PTY
output arrives.

The focused candidate request suite passed 13 tests; the full release suites
passed 199 all-feature / 198 no-default tests, with 3 ignored in each:
`target/graphics-expiry-{unit,all-unit,no-default-unit}.log`. Both uniquely named
candidate proof examples were rebuilt (`target/graphics-expiry-examples-build.log`).
Clippy uses `target/graphics-expiry-clippy.log`; verify its final handle before
claiming the last edit passed.

Private actual-time acceptance passed locally and over authenticated loopback
SSH: `target/graphics-expiry-{local,ssh}.log`. These run nine direct and nine
wrapped source cases, including one real 30-second stalled upload in each form,
and verify 20 ordered DA replies, zero leaked reservations, native decoded pixel
digest and tmux observer pause/continue while preserving the private source PID
and geometry. Optional `--stalled-upload` requires the owner proof executable;
its SSH command timeout is extended only for this explicit acceptance mode.
The total-deadline test advances a monotonic test clock instead of waiting five
minutes. The expiry candidate has not been promoted into the running gate.

Production lifetime/PTY ownership, retained-image aggregate quotas, renderer
integration, archives, advanced protocol behavior and graphics APIs remain
required. Existing font-output proxy/native-canvas paths provide presentation
extension points without modifying the immutable KIT checkout; their suitability
for full image z-order and foreground masking still needs implementation and
acceptance. Do not substitute these request/transport proofs for shipping image
support or for any remaining requirement of the original broader parity goal.

Final candidate strict all-target/all-feature Clippy finished with exit 0
(exec 54406, `target/graphics-expiry-clippy.log`). Both actual-time local/SSH
expiry checks and the 199/198 release suites above are terminal passes. Canonical
serial gate 65649 remains confirmed live; its proof artifacts were preserved by
using only the separately named candidate example targets for expiry acceptance.


### Image composition and cells fallback: 2026-11-02 UTC

The preceding reboot audit was progress: current local/SSH cold-start and
startup-retry fixtures passed, and the real frontend was refreshed with its
shell/agent identity preserved (`target/reboot-current-user-*.log`). The default
remains `herdr-default`; the user's requested replacement is still unknown.

The canonical pipeline serial gate (exec 65649) has now completed with exit 0,
including session/default startup, nested panes, history, snapshot upgrade,
plugins, graphics transport/owner proofs and local/SSH cells acceptance. The
pipeline x86_64 Nix check had already completed with exit 0. Its executable is
archived as `target/graphics-pipeline-validated-starflux` with a SHA-256 receipt.

Added `graphics_paint` in the isolated candidate, then promoted it with the
previously validated upload-expiry owner to canonical source. The painter keeps
three alpha planes with protocol z/image-ID ordering, source crop and viewport
clipping, popup masks, independent alternate buffers and bounded shared working
reservations. Its cells fallback uses half blocks, handles wide clusters and
combining text, preserves source history, and rejects oversized KIT grids before
allocation. Missing planes bypass the pixel averaging loop. Explicit cell-sized
placement fitting across different frontend font sizes remains unfinished.

Eight new composition tests passed. The candidate release suites passed 207
all-feature / 206 no-default tests (3 ignored each), and strict all-target,
all-feature Clippy passed: `target/graphics-paint-{unit,all-unit,no-default-unit,
clippy}.log`. The private stream proof now validates exact composed source pixels,
a valid cells frame, immutable source history and released working reservations.
Local and authenticated SSH composition/flow-control checks passed in
`target/graphics-paint-{local,ssh}.log`; the SSH compositor executes remotely.
PNG and cells JSON artifacts were generated and checked under
`target/graphics-paint-artifacts`. This does not exercise a live host presenter.

Canonical post-promotion checks are running as exec 24348, with logs prefixed
`target/graphics-paint-canonical-`. Keep the source fixed until this chain
finishes. It checks formatting, strict Clippy, both release test configurations
and examples. A separate workspace binary release build is still required
because `--examples` alone does not build the installed main executable.
Manifests, lockfile and immutable KIT pin are unchanged. The release gate now
requires `--composition` on its existing local/SSH graphics proof commands.

Production source ownership/lifetime supervision, actual host presentation,
aggregate retained-image quotas, terminal mutation tracking, advanced protocol
operations, image archives and graphics APIs remain required. This is progress
toward the original full parity goal, not a smaller replacement objective.


Post-promotion verification finished: exec 24348 exited 0 (formatting, strict
Clippy, 207/206 release tests, examples), then exec 10037 exited 0 for the actual
workspace binary release build and runtime preparation. The wrapper still
resolves through `~/.local/bin/starflux`, and `starflux --version` is 0.2.0.
Vendored parser formatting and dependency advisories/bans/licenses/sources checks
passed (exec 33748, `target/graphics-paint-{vendor-format,dependencies}.log`).
Canonical combined composition, async owner ordering, real 30-second stalled
uploads, source PID/geometry preservation and observer flow control passed both
locally and over SSH (execs 16898/19706,
`target/graphics-paint-canonical-{local,ssh}.log`). Existing real controllers
were not refreshed for these unused graphics modules; their retained sources
were not touched.

A frozen x86_64 Nix package/check run is live as exec 62838, with source recorded
in `target/graphics-paint-nix-source-path` and log
`target/graphics-paint-nix-check.log`. The new full serial release gate uses
`target/graphics-paint-release-gate.log`; verify its tool handle before treating
it as a pass. Do not mutate canonical source, the main executable or proof
examples while this gate runs. Continue with uniquely named isolated candidate
artifacts for subsequent work. Font-dependent explicit-placement fit and the
production owner/presenter connection remain the next graphics requirements;
the larger original goal is still active.

The post-composition serial gate tool handle is exec 94627. Both this handle
and Nix exec 62838 were confirmed live at the checkpoint; neither has yet
been reported as a terminal pass.


### Independent image geometry and retained quotas: 2026-11-02 UTC

The preceding turn was progress: composition/expiry were promoted, canonical
strict checks and combined local/SSH proof acceptance passed, and both full
validation jobs were launched. Nix exec 62838 has now exited 0: the frozen
composition source builds/tests/packages and its smoke checks pass on x86_64.
This does not validate ARM64 or the newer isolated work below. The composition
serial gate exec 94627 is still live; keep canonical code/artifacts unchanged.

The isolated candidate now retains the original explicit column/row request on
each placement and resolves fitted pixels, aspect ratio, inset and cell extent
for each view's cell size. Natural-size pixels stay unscaled, their cell anchor
moves with the view, and source-owned cursor metadata does not change. Fixed
pixel offsets remain fixed; a font reduction cannot underflow the requested
rectangle. Four new tests cover both-axis letterboxing in two font geometries,
natural-size pixels, one-axis aspect/cursor behavior and reduced-cell offsets.
The twelve painter tests pass (`target/graphics-fit-unit.log`). The existing
canonical painter still has its earlier explicit-placement sizing limitation.

The candidate also adds a shared retained-pixel budget to Resources, defaulting
to 512 MiB per backend process. Stores can share a supplied budget, and default
stores use the backend's shared resources. Charges use actual vector capacity,
cover both terminal buffers, and release on delete, eviction, reset and drop.
New-image admission and replacement growth fail with EBUSY before changing valid
images/placements. Atomic charge resize allows equal-capacity replacement even
when the shared quota is full; failed growth leaves the prior charge intact.
Four tests cover cross-store pressure, replacement at the limit, denied growth,
capacity rather than length, query exemption, buffer clear/reset and independent
owner drop. The focused graphics suite passes 69 tests in
`target/graphics-retained-unit.log`. This is a per-backend payload quota, not a
cross-process application-wide memory guarantee; choosing/supervising the actual
shared production owner still matters. Metadata and total RSS are not covered.

Strict candidate all-target/all-feature Clippy passes in
`target/graphics-fit-clippy.log`. The all-feature suite passes 215 tests with
3 ignored (`target/graphics-fit-all-unit.log`). The rest of exec 96657 is still
running: no-default tests and uniquely named candidate proof examples. Its stream
proof now validates two cell geometries; the composition transport fixture asks
for a cell-sized image so that source data actually exercises refitting.
Local/SSH proof execution remains to be run after those example artifacts finish.


The isolated fit/quota chain exec 96657 has now exited 0: strict Clippy,
215 all-feature / 214 no-default release tests (3 ignored each), and both
separately named candidate proof examples. Combined actual private-source
acceptance passed locally and over authenticated loopback SSH:
`target/graphics-fit-{local,ssh}.log` (SSH exec 1156 exited 0). The source sends
a cell-sized image; its exact pixels and valid fallback frames are verified
at 8x16 and 12x24 cell sizes, while preserving source text, PID and geometry,
ordered source replies, observer flow control and released working reservations.
These checks do not invoke the stalled-upload option again; its unchanged owner
was already verified with real deadlines in the preceding milestone.

Canonical composition gate exec 94627 was polled again and remains live. The
fit/quota code stays only in `target/inline-graphics-workspace` until that gate
finishes. Promote the candidate's `src/graphics_store.rs`,
`src/graphics_resources.rs`, `src/graphics_paint.rs`,
`examples/graphics-stream-proof.rs` and `scripts/test-graphics-observer.py`
together; its manifest/lock/pin remain staging-only/unchanged. Then validate the
canonical build/checks before claiming the fit/quota changes are installed.
The next substantial requirement remains the real retained source owner and
host presenter; these model/proof improvements alone do not display images in
live terminals. Preserve real backend and App Space workers throughout.


### Source-owner engine: 2026-11-02 UTC

The preceding turn was progress: independent cell-sized fit and retained-pixel
admission were implemented and validated in the isolated candidate, while the
canonical composition gate stayed fixed. That gate (exec 94627) is still live;
its most recent completed acceptance covers categories and panel focus. Its
frozen-source x86_64 Nix check completed with exit 0 in the preceding turn.

The candidate adds `src/graphics_owner.rs`, combining a native parser, ordered
requests, retained store and externally dispatched async decode jobs. It is
an engine for the eventual retained source backend, not a process launcher or
an installed PTY owner. The caller owns the PTY and shared decoder pool. No
existing real controller, terminal, broker or App Space worker was refreshed,
restarted or replaced for this work.

Raw input is admitted atomically in reads up to 8192 bytes into a 64 KiB queue,
including a bound for pending passthrough prefixes. Parsing runs in bounded
turns and keeps ordinary text in batches. ESC/ST boundaries are split so a
completed image forms a barrier before following text/queries. The backend
can continue sending ordinary user input while decoding is pending. Worker
completions must match both a unique job tag and an outstanding generation-bound
response ticket before they can mutate the store. The engine commits pixels,
placement and requested cursor movement before parsing following bytes. It
clamps cursor movement through parser-relative operations at screen/scroll
limits; those overflow positions are implementation-defined by the protocol.
It never injects generated cursor escapes into the source PTY.

Graphics, DA and cursor/status replies share the ordered queue. Full queues
pause parser consumption without dropping requests; an idle partial upload
still expires and releases blocked replies. Busy submissions can be completed
as ordered EBUSY. Error/quiet handling releases the commit barrier without moving
the cursor. Decoder/store/input reservations are released on relevant failure,
clear and drop paths. Malformed passthrough invalidates in-flight publication
and requires replacement by a backend with a verified source baseline; it does
not silently continue with stale images. Resize admission cannot overtake a
pending image operation. Alternate-buffer entry clears the alternate store.

Eight engine tests pass (`target/graphics-owner-engine-unit.log`), including
all boundaries of a split APC terminator, cursor/text/query order, quiet decode
failure, queue saturation and resume, partial timeout under saturation,
malformed-stream invalidation, replacement-generation completion rejection and
bounded input retry. Strict all-target/all-feature Clippy passed, followed by
223 all-feature / 222 no-default release tests, with 3 ignored each:
`target/graphics-owner-engine-{clippy,all-unit,no-default-unit}.log`. The final
candidate check/example chain exec 64547 and test chain exec 66342 exited 0.

A new unique candidate example, `graphics-next-authority-proof`, creates only a
private raw PTY and synthetic child. Six direct and six wrapped cases verify
actual source-observed graphics/DA/DSR ordering, C=0 and C=1 cursor/text behavior,
chunked uploads, quiet and unsupported-file handling. The child receives user
input while the engine's decode barrier is held. Three retained images consume
12 bytes and every reservation releases after engine drop. The observer script
can run it with `--authority-executable`, separately from the older query-only
owner proof. Both combined acceptance runs passed locally and over authenticated
loopback SSH (exec 39061 exit 0):
`target/graphics-owner-engine-{local,ssh}.log`. They also recheck the observed
pixel digest, two font geometries, cells frame validity, source PID/geometry,
observer pause/continue and post-detach input. No presenter is involved.

This engine remains in `target/inline-graphics-workspace`. In addition to the
previous fit/quota promotion set, future promotion needs `src/main.rs`,
`src/graphics_owner.rs`, `src/graphics_reply.rs`, `src/graphics_requests.rs` and
`examples/graphics-authority-proof.rs`. The candidate manifest names that proof
explicitly; canonical Cargo autoexamples will discover its file normally.
Never copy the staging manifest or change the immutable KIT pin/lockfile.

Remaining source-owner requirements include the actual retained broker and
lifetime supervision, verified attachment/baseline and reconnect behavior,
scroll/reset/erase mutation tracking, source scheduling and host presentation.
The engine alone must not enable capability advertisements on real terminals.
Cold image archives, advanced protocol behavior, image APIs and every other
unfinished original parity item remain required. Avoid treating these private
proofs as installed graphics support or as completion of the broader goal.


### Scroll/reset tracking and canonical promotion: 2026-11-02 UTC

The preceding owner-engine work was progress. Its private PTY proofs and 223/222
release suites passed, leaving actual ownership/presentation unfinished. The
canonical composition serial gate exec 94627 has now finished with exit 0,
including local/SSH cells and multiwindow acceptance. Its executable is archived
as `target/graphics-paint-validated-starflux` with a SHA-256 receipt. This green
gate proves the preceding composition checkpoint, not the new code below.

Added opt-in parser scroll events for automatic wrap, LF, reverse index,
explicit scrolling and line insertion/deletion, plus typed display erase,
alternate-buffer clear and reset events. Scroll operations now clamp excessive
counts to their page; inserting/deleting lines outside the margins is a no-op.
The source owner applies these events in order between image commit barriers.
Signed placement offsets and cell-space clip intervals preserve pixels above
the visible display, retain at most the source's 1000-row primary image history,
and clip margin scrolls without resurrecting discarded pixels when reversed.
Cross-margin placements are unchanged by a margin operation. Full display erase
keeps reusable assets, repeated alternate entry clears the alternate store,
and reset releases both buffers and aborts partial uploads. A failed mutation
invalidates image state instead of allowing stale rendering to continue.

Six new engine tests cover signed history/raster pixels and bounded eviction,
margin clipping and reversal, clear/redisplay/reset/cancel behavior, repeated
alternate entry, cross-margin images/line edits outside the page, and IL/DL,
reverse-index and automatic-wrap movement. The candidate release suites pass
229 all-feature / 228 no-default tests (3 ignored each), with strict all-target
Clippy passing. Logs: `target/graphics-mutation-{all-unit,no-default-unit,
clippy}.log`, final candidate chain exec 10428 exit 0. The standalone vendored
parser check needed an isolated lockfile: the initial --locked invocation
failed because none existed, then --offline completed with exit 0 (exec 46160).
That trimmed vendor tree has no unit/integration tests and one doc test; the
application's parser/graphics/owner tests provide the meaningful mutation checks.
No vendor lockfile or staging manifest was promoted.

Actual private PTY mutation acceptance passed locally and over authenticated
SSH (exec 31602 exit 0): `target/graphics-mutation-{local,ssh}.log`. The authority
child synchronizes at image-scroll, display-clear and reset markers, so the proof
checks exact composed pixels and stored state before acknowledging each phase.
Its earlier twelve direct/wrapped cursor/reply/input cases also run. The same
acceptance includes the existing query owner, two-font composition, native pixel
digest and observer flow-control/PID/geometry/input-preservation checks. It does
not open a renderer, real source, desktop or agent.

The first parser microbenchmark found an approximately 19% regression. Making
scroll delivery opt-in removes the unconditional character-path work for ordinary
parsers. Two final paired runs compared against the prior canonical parser:
ASCII ratios 1.013 and 1.000; styled ratios 1.088 and 1.073. See
`target/graphics-mutation-parser-benchmark-{final,repeat}.log` (execs 37661/89772
exit 0). This is a throughput microbenchmark, not input-to-display latency or
proof of no styled-text performance cost. Preserve this limitation when assessing
production performance; the tracked owner will also need its own profiling.

After gate 94627 completed, promoted the validated fit/quota, owner engine and
mutation code to canonical source, including the five modified vendor Rust files.
The lockfile and immutable KIT pin/manifests are unchanged. The release gate now
requires the authority/mutation proof alongside its existing combined local/SSH
graphics commands. A final small failure-path correction invalidates the owner
when a mutation cannot be applied; canonical checks cover that final version.
Canonical format/vendor format, strict Clippy, workspace release tests, workspace
binary/examples release build and runtime preparation are running as exec 74552,
logs prefixed `target/graphics-mutation-canonical-`. Poll it before claiming the
new main executable or final canonical suites passed. Real loaded controllers
and their retained source/App Space processes remain untouched.

Production broker/lifetime supervision, verified source attachment/reconnect,
actual host presentation, broader scrollback/resize semantics and image archives,
advanced protocol commands/APIs and every other incomplete parity item remain
required. The new engine is not connected to live terminal callbacks, and no
real terminal advertises graphics support based on these private proofs.


Post-promotion canonical check chain exec 74552 exited 0: formatting (including
vendor), strict workspace all-target/all-feature Clippy, 229 all-feature / 228
no-default release tests (3 ignored), workspace binary/examples release builds
and runtime preparation. `~/.local/bin/starflux` still resolves to the correct
runtime wrapper, and its harmless version check reports 0.2.0. Combined promoted
local/SSH authority, mutation, query-order, composition and observer-flow proofs
passed (`target/graphics-mutation-canonical-{local,ssh}.log`, SSH exec 53235
exit 0). No live controller refresh was needed for these inactive graphics paths.

The next full canonical serial gate is live as exec 20050, log
`target/graphics-mutation-release-gate.log`. Its new combined graphics commands
require both `--authority-executable` and `--mutations`. A frozen-source x86_64
Nix check is live as exec 99191, log `target/graphics-mutation-nix-check.log`,
source path recorded in `target/graphics-mutation-nix-source-path`. These handles
must be polled to terminal completion before reporting either as a pass. Preserve
the canonical source/main/proof artifacts until the serial gate completes;
continue any independent next step in the uniquely named isolated candidate.
The next substantial requirement is actual retained source ownership and host
presentation; the full original parity goal remains unfinished and active.


### Persistent native source broker: 2026-11-02 UTC

The preceding mutation/owner turn was progress: it promoted scroll/reset state
and passed focused canonical checks. Its full canonical serial gate exec 20050
has now completed with exit 0, including native/cells local/SSH and multiwindow
acceptance. Frozen x86_64 Nix exec 99191 also exited 0. Their logs are
`target/graphics-mutation-{release-gate,nix-check}.log`; the source path is in
`target/graphics-mutation-nix-source-path`. The validated executable is archived
as `target/graphics-mutation-validated-starflux` with its SHA-256 receipt. Neither
old gate validates the new broker below.

Implemented `src/source_broker.rs` in the isolated candidate, then promoted it
after private acceptance. The broker retains multiple newly created native PTYs
independently of view/SSH-client connections, with one source-owner engine per
PTY and a shared decoder pool/retained-image budget. Nonblocking descriptor I/O
and rotating source service avoid a blocked source monopolizing other sources.
Private Unix sockets, same-UID peer credentials, constant-time token comparison
and full source ID/epoch/boot/PID/start references protect control operations.
No RPC/control command becomes PTY input; only explicit decoded Input bytes do.
The broker is not integrated into normal session startup or backend selection.

Durable private receipts are consumed before spawning. Reconnecting to a live
or exited source ID returns the retained instance; changing its launch intent is
rejected. Broker loss, a failed spawn or uncertain acknowledgement cannot replay
the same source ID. A fresh explicit launch gets a new ID. Argv is passed literally
without a shell. Receipts retain a digest/identity, not command arguments. Opens
and fsync/PTY initialization run on two bounded workers. Concurrent same-intent
opens share one initialization; a client closing stdin still receives its result.
Failed post-spawn initialization cleans up that newly owned child off the I/O loop.

Input accepts up to 1 MiB per request, with a 2 MiB per-source queue and shared
16 MiB input / 16 MiB reply payload budgets. Owned write chunks retain leases until
written/dropped. Reply capacity and queue slots are reserved separately from user
input. Peers/requests/output are bounded (16 peers, 2 MiB incoming, 4 MiB outgoing),
and slow/disconnected peers do not remove sources. These payload quotas are not
whole-process RSS limits. Source capacity is bounded to 1024; kernel FD and other
resource limits can refuse earlier. Retained image payloads share 512 MiB in this
broker process. The source-owned palette/window/cell-size queries now use the
ordered queue, including after resize. Read-only cells views can use independent
size/font cells/palettes without changing the source's kernel geometry. Actual
kernel resize is a separate guarded operation. Input mode metadata is returned
for the future frontend adapter.

Close uses a retained pidfd and returns a closing acknowledgement immediately.
A stubborn process receives a delayed SIGKILL through that same identity; other
sources keep servicing input. Natural exit retains readable state and never
restarts implicitly. Explicit shutdown rejects new mutations and waits for owned
sources/initializers to retire. Exited PTYs are removed from readiness polling;
closing the write half of a control connection does not discard pending replies.
The SSH relay explicitly flushes small packets, fixing the first SSH probe's
buffering timeout. An early local PONG assertion also needed to place its marker
outside the deliberately overlaid image; source input itself remained correct.

The candidate passed strict all-target/all-feature Clippy, 234 all-feature /
233 no-default release tests (3 ignored each), and its separately named worker
example build: `target/source-broker-{clippy,all-unit,no-default-unit,
examples-build}.log`, chain exec 79415 exit 0. Five new unit tests cover concurrent
initializer deduplication, existing/replaced Unix socket preservation, indexed/
cube/gray/true-color palettes, and ordered source palette/size queries. The socket
fixture initially lacked an explicit 0700 directory mode; corrected the fixture,
not the privacy requirement. The stronger private worker acceptance passes
locally and over authenticated loopback SSH (exec 44575 exit 0):
`target/source-broker-{local,ssh}.log`. It checks a detached SSH worker, same PID
reattachment, graphics produced while every view is absent, no launch replay,
literal Unicode/metacharacter argv, independently rendered views/palettes,
near-1 MiB input, kernel resize, stale close/input rejection, a hung source's
asynchronous close, natural exit, broker restart, consumed failed launch IDs,
an explicit fresh launch and a half-closed asynchronous open. No real agent,
project, controller, tmux owner or App Space worker is used.

Promoted only the new broker, owner query/usage helpers, Linux-gated module,
private worker example and acceptance script. Staging manifests/locks were not
copied; the immutable KIT pin and canonical Cargo lock are unchanged. Canonical
format, strict workspace Clippy, 234/233 release tests, workspace binary/examples
release builds and runtime preparation pass (exec 45434 exit 0), logs prefixed
`target/source-broker-canonical-`. PATH still resolves to the runtime wrapper and
its harmless version check is 0.2.0. The release gate now includes the private
broker fixture locally and over SSH. The current canonical SSH worker proof is
exec 33199; poll it before reporting the final promoted acceptance complete.

Still required: actual singleton discovery/daemon supervision and durable
project-slot references; normal local/SSH backend adapters preserving existing
owners; native history/snapshot/lifecycle and automation mappings; streaming
native frames and sharp graphical image layers; asynchronous cached render
production; clipboard/title/notification events; graphics-fault/resync and
interrupted-launch/process-group hardening. Raw Input acknowledgement means
queued, not guaranteed delivered, and must not be automatically replayed after
an uncertain connection. Existing durable automation operation guards must remain
around that boundary. Broker process death/reboot does not preserve live PTYs;
receipts prevent rerunning commands, while explicit shell/conversation recovery
still needs the existing cold-start/model adapters. Do not enable this component
as the default or advertise broad graphics support based on these private proofs.
The full original parity goal and its other unfinished items remain active.


Final promoted private broker acceptance passes locally and over SSH (exec 31434
exit 0), `target/source-broker-canonical-{local,ssh}.log`. The close-isolation
assertion now waits for a new unique PONG marker so an old visible reply cannot
satisfy it. Reports explicitly include half-closed launch, palette, asynchronous
close, natural/failed launch non-replay and explicit fresh-launch checks.
The canonical check/build chain remains a terminal pass (exec 45434), and the
installed wrapper/version was verified without refreshing real controllers.

Frozen-source x86_64 Nix validation uses `target/source-broker-nix-check.log`,
with exact source path in `target/source-broker-nix-source-path`. The new full
serial release gate uses `target/source-broker-release-gate.log`; it includes the
new local/SSH worker checks as well as the existing behavior suite. Poll their
specific process handles before reporting completion. Preserve canonical code,
main executable and proof artifacts while that serial gate runs. Continue the
next integration step in uniquely named isolated candidate targets.

Next integration must retain legacy/native/transferred/imported sources without
restarting them: discover a compatible retained broker and persist per-pane source
references, then route health/input/history/snapshot and frontend frames through
the adapter. Do not restart a live source broker merely to refresh its frontend
or load a new executable; support compatible old owners or an explicit verified
handoff. Native render delivery must be pushed/cached with bounded replacement,
not one SSH round trip per frame. Keep source parser authority and immutable
view-specific geometry, and preserve App Space's independent media/input paths.
Other prerequisites before broad activation include transport-fault resync,
source environment parity with packaged host commands, clipboard/title/attention
forwarding, lifecycle/automation guards and process-group/interrupted-creation
supervision. These remain required work, not reasons to mark the goal blocked or
to redefine it around the private component.

Current native-broker validation handles: Nix exec 24014 and full serial gate
exec 95078. Both were confirmed live at this checkpoint; neither is yet a
terminal pass. The original full parity goal remains active.

### Pushed native-source cells presentation

Previous goal turn made progress: reboot-recovery/startup-retry acceptance passed
locally and over SSH, and a guarded UI refresh preserved the live terminal PID
and selected project. The saved default remains `herdr-default` pending the
user's intended replacement; it was not guessed or changed.

The preceding native-source broker's full serial gate (exec 95078) and frozen
x86_64 Nix check (exec 24014) both reached verified exit 0. Its binary and SHA
receipt are `target/source-broker-validated-starflux` and
`target/source-broker-validation-receipt.json`. These results validate that
checkpoint, not the subsequent streaming changes.

The next implementation has been promoted from the isolated candidate:

- Visible parser snapshots copy the current grid/styles/links/cursor without
  copying history or the inactive buffer. Retained image allocations are shared
  immutably and remain charged until the final source/snapshot drops them.
  Replacement at a full shared budget fails atomically while an old snapshot
  pins the pixels, then succeeds once that snapshot is released.
- One-shot cells requests and pushed `watch` frames compose and encode on two
  bounded workers. The source I/O loop keeps parsing input/output and answering
  queries independently. Presentation (128 MiB) and encoded transport (64 MiB)
  reservations are separate from decoder working/retained budgets.
- A view has one frame awaiting acknowledgement. Exact delivered-frame
  epoch/sequence acknowledgement releases credit; the next render captures
  latest source state. Acknowledged frames supply row-delta bases, retaining
  only frame runs, not pixels/history. Host generation, focus, geometry and
  palette remain per view. Cancelling/reconfiguring a view drops unsent obsolete
  packets and fences partially sent/late frames with their original epoch.
- Source switches reset delta bases. Stale ack/unwatch/watch requests cannot
  revive a retired view. Close and natural-exit events preserve source identity;
  an invalid resized view reports a deduplicated error and backs off until the
  client supplies a new configuration. Half-closed asynchronous one-shot callers
  retain their pending reply. No owner is restarted to refresh its view.

The isolated candidate passed strict all-target Clippy, 238/237 all/minimal
release unit checks before moving the parser snapshot test into the ordinary
root suite, plus 86 broker-example unit tests. Its final streamed acceptance
passes locally and over private loopback SSH:
`target/source-stream-{local,ssh}.log`. Coverage includes withheld credits while
source input/query replies continue, exact row-delta reconstruction, source-switch
fences, independent fonts/palettes, resize error/recovery, source-close/natural-exit
frames, half-closed cells and released snapshot reservations. Existing one-shot
behavior also passed in `target/source-stream-compat-{local,ssh}.log`.

Canonical checks/builds/runtime preparation are running in exec 74367, with
logs `target/source-stream-canonical-*`. A new frozen-source x86_64 Nix check is
exec 37874 (`target/source-stream-nix-check.log`, exact frozen path in
`target/source-stream-nix-source-path`). Poll these exact handles. The serial
release gate now includes `test-source-broker.py --streams` locally and over SSH;
its new run must follow the canonical builds. Do not confuse the previous full
gate's success with validation of these subsequent edits.

This remains private component/transport acceptance. The installed application
has not been switched to this backend and live views were not refreshed for this
change. Next integration still requires compatible singleton discovery and
project-slot references, normal local/SSH backend adapters, retained imported
and transferred source handling, history/snapshot/lifecycle/automation mappings,
sharp host graphical layers, clipboard/title/notification forwarding, verified
transport fault resync, packaged-command environment parity and startup/process-
group supervision. The original full parity goal remains active; the other
unfinished requirements above are not superseded by this checkpoint.

The promoted streamed implementation's canonical check/build chain (exec 74367)
has reached verified exit 0: strict workspace/all-target Clippy, 239 all-feature
and 238 minimal-feature release tests (three explicitly ignored in each),
workspace binaries, separately built examples and runtime preparation. The frozen
x86_64 Nix check (exec 37874) also reached verified exit 0. The frozen source's
seven implementation/parser files, acceptance fixture and gate script were
byte-compared to the promoted worktree. PATH resolves to the retained runtime
wrapper and `starflux --version` reports 0.2.0. No live controller was refreshed
for the streamed implementation. Final canonical local/SSH streamed acceptance
uses `target/source-stream-canonical-{local,ssh}.log`; confirm those exact runs
before reporting that acceptance as passed.

Final canonical streamed acceptance passes locally and over private SSH (execs
58217 and 80762, both exit 0). The new full serial release gate is exec 51822,
`target/source-stream-release-gate.log`, and was confirmed live after launch.
Poll this handle before reporting its completion. Keep canonical source and
main/proof executables stable while it runs; use uniquely named isolated
candidate artifacts for the next integration. Continue compatible broker
singleton discovery and durable pane references, followed by source adapters
and actual host presentation. The original complete parity goal remains active.

### Native-owner discovery and durable pane references

The previous goal turn made progress by implementing bounded pushed native cells
frames and acknowledged row deltas. Its full serial release gate (exec 51822)
has now reached verified exit 0. The validated binary and SHA receipt are
`target/source-stream-validated-starflux` and
`target/source-stream-validation-receipt.json`. These results cover that streamed
checkpoint, not the subsequent managed-owner changes.

The managed-owner implementation has been promoted from the isolated candidate:

- A private, persistent `native-sources/` namespace owns the socket, token,
  boot/PID/start/instance descriptor, lifetime/startup locks and source receipts.
  Descriptor-relative Linux socket paths support long data paths; runtime loss
  does not remove the socket. Metadata is private, bounded and atomically
  fsynced; symlink/hardlink files and replaced namespaces are rejected.
- Concurrent discovery shares one detached owner. Peer credentials and required
  capabilities are verified before reuse. A live incompatible/unreachable owner
  is preserved; neither a timeout nor a newer binary authorizes restarting it.
  Existing-only attachment cannot create a replacement. Internal local/SSH
  stdio connectors inject the token on the source host and reject caller tokens.
  The installed workspace's normal backend remains unchanged.
- Durable pane records bind model epoch, owner, source UUID/epoch and launch
  digest. The intent is saved before delivery. Concurrent identical explicit
  opens share one source. Read-only recovery can find an existing source after
  an uncertain Open result, but cannot execute saved argv. Wrong epochs/digests,
  stale writers, changed owner boots and lost owners are rejected. Explicit
  recovery uses a fresh epoch/source intent with compare-and-swap of the old
  record; unrelated sources remain live.
- Native commands restore packaged host PATH/media settings and clear inherited
  Kitty/TMUX protocol markers. Linux internal worker/connect/discover roles use
  the loaded executable, so removing its pathname does not require restarting
  retained sources. No real controller, terminal or App Space worker was
  refreshed, migrated or stopped for these changes.

The final isolated implementation passed strict all-target/all-feature Clippy,
244 all-feature and 243 minimal-feature release tests (three ignored each),
plus 91 broker-example unit tests. Private managed acceptance passes locally and
through authenticated loopback SSH using the actual application worker/connector:
`target/source-service-{local,ssh}.log` (execs 99721/95355, exit 0). This includes
concurrent cold discovery, concurrent pane opens, pending-launch recovery,
runtime loss, loaded-executable removal, unsupported-version preservation,
wrong digest/stale record rejection, explicit fresh recovery and a deliberately
incorrect bundled Python/media environment. Raw worker/stream compatibility also
passes (`target/source-service-raw-compat-{local,ssh}.log`, execs 22958/15310).

The exact final formatted implementation passed frozen-source x86_64 Nix checks
(exec 18032 exit 0): `target/source-service-nix-check.log`, exact source path in
`target/source-service-nix-source-path`, file hashes in
`target/source-service-candidate-receipt.json`. Earlier initial and pre-format
Nix passes are separate logs and are not substitutes for the final source.

Canonical workspace checks/builds/runtime preparation are running in exec
18010, logs `target/source-service-canonical-*`. Poll that handle; then validate
the promoted raw/managed worker fixtures, wrapper/version and start the next
full serial release gate. The gate now includes managed local/SSH acceptance
with `--service --streams --worker-executable target/release/starflux`.
`docs/native-sources.md` describes the protocol and remaining integration.

The normal local/SSH backend adapters, model transactions/slot cleanup, compatible
existing-source observation/adoption, health/lifecycle/history/snapshot and
automation mapping, actual sharp graphical image delivery, clipboard/title/
notification forwarding, verified resync and interrupted-creation/process-group
supervision still need implementation. Do not enable a narrower cells-only or
new-sources-only path and call the full graphics/Herdr parity goal complete.
All other unfinished original goal requirements remain active.

The promoted managed-owner canonical chain (exec 18010) reached verified exit
0: strict workspace/all-target Clippy, 244 all-feature and 243 minimal-feature
release tests (three ignored each), workspace binaries, separately built
examples and runtime preparation. Canonical managed local/SSH acceptance passes
(execs 50175/37464), as does raw stream compatibility (51620/73607), all exit 0;
logs are `target/source-service-canonical-{local,ssh,raw-local,raw-ssh}.log`.
The promoted implementation, fixture and gate script were byte-compared against
the final formatted Nix source. PATH still uses the runtime wrapper and the
harmless version check is 0.2.0. No live UI refresh was needed for this unused
backend component.

The new full serial release gate is exec 24588,
`target/source-service-release-gate.log`. It was confirmed live at launch; poll
this exact handle before reporting completion or changing its source/artifacts.
Continue integration in uniquely named isolated candidate targets while it runs.
The original full parity goal remains active, with the normal source adapters
and actual host graphical presentation still required, along with the other
unfinished audited features. There is no user-input blocker for that work.

### Native workspace adapter — isolated integration in progress

The preceding managed-owner full serial gate (exec 24588) reached verified exit
0. Its validated binary and SHA receipt are `target/source-service-validated-starflux`
and `target/source-service-validation-receipt.json`. The installed normal UI still
uses its existing backends. No live owner/controller was refreshed for the next
work, which remains isolated under `target/inline-graphics-workspace`.

The candidate now has shared durable identity DTOs (`native_protocol.rs`), typed
per-pane source references in the project model, and a native workspace adapter
(`native_backend.rs`). New native sources remain opt-in. The model records the
exact intent before delivery; its saved prior remote binding supplies the CAS
rather than treating a newly fetched remote record as authorization to overwrite
it. A competing frontend inspects the winning intent, and an uncertain result
reattaches without replaying a command. Existing/imported/transferred/plugin
sources keep their backend. An inconclusive legacy probe cannot create a new
source. Missing native capabilities preserve the old owner instead of restarting
it or silently falling back to another terminal backend.

Normal automation `sessions.create`/`panes.create`, inspection, guarded input,
retained history and screen capture now route through the candidate adapter.
Local/SSH helper requests use bounded stdin and private temporary output with a
time limit; raw user input is not put in shell command history. Source-side
foreground inspection exports only verified harness identity, not command args.
Guarded input rechecks foreground identity in the source owner immediately before
queueing it. Acknowledgement is explicitly queued, not a claim of consumption;
unknown delivery never grants replay permission.

Native screen/history reads run on bounded presentation workers. Active-buffer
history snapshots omit the inactive buffer, keep at most 1,000 retained rows and
leave the authoritative parser untouched. Formatted visible reads are bounded
to 1 MiB and history to 2 MiB. Snapshot formatting does not replay terminal
callbacks, queries or clipboard/title operations. Normal native history uses the
same frontend history interface, and metadata/capture uses the standard helper
path. Legacy model/config data defaults to the existing backend.

The ordinary API acceptance fixture `target/test-native-adapter.py` passes locally
and over authenticated private SSH against the actual candidate binary. It
checks distinct sources, source/foreground identities, submitted input, stale
agent rejection, no operation replay, retained history, runtime loss and native
capture. Its SSH variant also routes a new source and metadata over a saved SSH
host configuration, with a forced private command environment. The old automation
fixture against this candidate passes locally/SSH and confirms legacy operation
creates no native owner. Logs: `target/native-adapter-{local,ssh,legacy-local,legacy-ssh}.log`.

Candidate strict all-target Clippy and release tests pass: 247 all/default and
246 minimal tests (three explicitly ignored), plus 92 broker-example tests.
Logs are `target/native-adapter-{clippy,unit,minimal-unit,build}.log`. The current
review patch/checksums are `target/native-adapter-candidate.patch` and
`target/native-adapter-candidate-receipt.json`; regenerate them after further
candidate edits. Candidate manifest/lock changes are staging only; never promote
the candidate Cargo manifests/lockfiles.

This is not a completed UI integration and must not be promoted as one. The
candidate's native CLI/API preference is not in the installed binary. GUI/cells
attachment is still the old PTY-based frontend and cannot yet attach to these
sources. Next implement the persistent native view adapter, with independent
palette/font/viewport updates, model/transport/source fences, keyboard/paste/mouse
modes and read-only selection/history. Use a shared connection hub per owner/host,
with multiple watched panes per connection and bounded replacement/credits.
Do not add an SSH round trip per frame or simply allocate one persistent SSH
connection per hidden pane: the source protocol intentionally bounds peers and
watch identities. Handle hidden-view detach/reactivation and tombstone rollover
without restarting sources or replaying uncertain input. Apply/parse snapshots
off the UI input loop, retain only the latest bounded frame, and reset host frame
bases after reconnect/visibility/generation changes.

Actual sharp graphical layers, OSC 8/clipboard/title/notification forwarding,
complete recovery/termination/slot cleanup, automation/lifecycle/snapshot
integration acceptance, compatible old-source observation/adoption, verified
resync and creation/process-group supervision still remain. A half-block cells
fallback is not sufficient graphical parity. Keep the original full goal and all
other unfinished audited items active.

Final candidate verification for this adapter checkpoint: exec 91981 reached
exit 0 (strict Clippy, 247/246 release tests plus 92 example tests and release
builds). A subsequent string output-fingerprint correction and bounded helper
I/O checks passed strict Clippy/release build in exec 51530, exit 0. The final
normal API fixture passes locally and over private SSH (exec 50869 confirms the
final SSH run); legacy automation compatibility execs 34771/63469 also exit 0.
The current candidate review receipt matches every changed source file and the
fixture, and the canonical/candidate dependency lockfiles remain byte-identical.
Nothing in this adapter checkpoint has been promoted, installed or activated in
live windows; the frontend connection hub/view integration remains required.
There are no pending validation handles from this checkpoint and no user-input
blocker. Continue the full original parity goal from the candidate worktree.

### Multiplexed native pane presentation candidate

The previous goal turn produced verified reboot/SSH startup-retry evidence and
refreshed the live view while preserving its selected project and terminal
process. This turn advances the isolated native-source adapter rather than
changing that user's saved default or replacing retained sources.

The candidate now has `src/native_view.rs`, a shared connector hub per owner and
host. Multiple native pane watches share one persistent local/SSH connection.
The normal graphical/cells terminal path uses these views when the candidate's
explicit native preference is enabled. No attachment PTY or shell is spawned.
Raw input, source-side bracketed paste, viewport/palette changes, watch epochs,
acknowledged row deltas and source identity checks use that connection. Frame
reconstruction runs on a bounded worker queue away from connector input. Hidden
views unwatch and source processes persist when attachments close.

A larger frontend recovers from another frontend shrinking the retained grid
through inspection and a new clamped watch epoch; its remaining display area is
blank. It does not continually resize an unchanged source. Presentation errors
leave input available. Input rejection discards unsent queued/tail packets and
preserves uncertain partially transmitted requests without replay. A connector
accepts at most its protocol allowance of historical view identities, then new
views use another connector. Native wheel review requests retained source history
through the shared controller tick in both presentations. Terminal epochs fence
that asynchronous capture.

Final candidate acceptance uses only private fake sources and loopback SSH:
`target/test-native-views.py` passes locally, over SSH, in cells, and over SSH in
cells. The tests verify shared source identity/input, initial pushed output,
splits, fresh output in both frontends after resizing, retained native history
from wheel review, review exit and surviving source processes. Logs are
`target/native-view-{local,ssh,cells,ssh-cells}.log`. A missing action-completion
wait in the test was corrected; the stronger test requires newly produced text,
not an old cached frame. The cells history dispatch failure found by this test
was fixed before the final four runs.

Normal native automation and legacy automation compatibility pass locally and
over SSH: `target/native-view-{api-local,api-ssh,legacy-local,legacy-ssh}.log`.
Strict all-target/all-feature Clippy passes in `target/native-view-clippy.log`.
All-feature release tests pass (250 passed, three explicitly ignored) in
`target/native-view-all-unit.log`. Three new tests cover rendering failure with
continued input, cancellation of unsent input while retaining indeterminate
packets, and background Unicode/style/mode conversion with independent blank
viewport padding. The final minimal release check also passes (249 passed, three explicitly
ignored) in `target/native-view-minimal-unit.log`. Both test process handles
reached verified exit 0. Release candidate binary: `target/release/starflux-graphics`, build log
`target/native-view-build.log`. The dependency lock remains byte-identical to
the canonical immutable KIT pin.

This candidate is **not promoted or installed**, and no live backend was
switched by these checks. Its updated review patch and checksums remain
`target/native-adapter-candidate.patch` and
`target/native-adapter-candidate-receipt.json`. Candidate manifests/lockfiles
must never be promoted. The installed managed owner checkpoint retains its
previous release/Nix evidence; these candidate checks are not a new full release
gate or Nix package validation.

Required next work remains substantial: account for frontend ingress/input,
parser, snapshot and transport allocation lifetimes across all hubs; exercise
connection loss, identity rollover, large styled output and rapid visibility
changes under load; complete natural-exit/recovery/termination/slot cleanup and
snapshot/lifecycle acceptance; retain OSC 8/clipboard/title/attention semantics;
implement sharp native graphical image planes instead of accepting half-block
fallback as graphical parity; connect compatible existing-source observation and
verified transport resync; and finish process-group/interrupted-launch handling.
Then validate the integrated candidate through the canonical release and Nix
gates before installation. Continue all other unchecked audited requirements in
`docs/plan.md`; the original full parity goal stays active. There is no user-input
blocker to these next implementation steps.

### Native presentation resource lifetimes and connector recovery

Previous goal turn made implementation and verification progress on shared
native views. This turn continues that same isolated candidate. The installed
binary/backends, saved session default and live App Space remain unchanged.

Frontend admission now has shared process-wide payload budgets: input 16 MiB,
presentation/history 128 MiB, incoming wire/JSON work 192 MiB, encoded output
32 MiB and 32 connector slots. Input leases follow queued data, raw tails and
in-flight acknowledgements. Encoded messages are counted/admitted before buffer
allocation. Incoming messages reserve before buffer growth and generic JSON
parsing. Presentation leases follow worker canvases, accepted UI screens and
frozen history; source-history parser/search allocations reserve before parsing.
The limits describe payload/working admission, not total allocator RSS.

Failed connectors now reattach to the exact recorded retained owner/source,
without any owner creation, command launch or input replay. The old input sink
is closed and queued payloads are released; uncertain delivery gets a visible
notice after reattachment. A shared sink slot updates cloned delivery-socket
writers along with keyboard/paste input. Existing owner loss still requires an
explicit new source intent; it is not hidden by view-level reconnect.

The styled-output acceptance found a source-grid growth bug: a view clamped by
an earlier shrink never enlarged its watch again. Pushed frames now include the
retained grid under a required `view-grid-v1` capability. Views establish new
watch epochs when their available extent changes, without resizing an unchanged
source. Incompatible owners are preserved, not restarted. The private fixture
also fixed a process-enumeration race by checking one captured connector list.

Final graphical/cells UI acceptance passes locally and over private loopback
SSH in `target/native-view-resources-{local,ssh,cells,ssh-cells}.log`. The SSH
runs now exercise both an SSH presentation and a configured SSH source host,
using the candidate helper and forced private environment. Each run verifies
one connector shared by two panes, guarded destruction of only that private
connector, automatic reattachment, delivery-socket input afterward, unchanged
source PIDs, 180 lines of per-character styled output reaching both frontends,
and successful input after the burst. The existing resize, independent review,
retained-history and source-preservation checks remain. These checks do not
establish WAN latency or sustained performance for every workload.

Strict all-target/all-feature Clippy and the candidate release build pass in
`target/native-view-resources-{clippy,build}.log`. All-feature release tests
pass (254 passed, three explicitly ignored) in
`target/native-view-resources-all-unit.log`. New regression tests cover ingress
admission/release after malformed JSON, encoded transport lifetime, shared input
limits across panes, uncertain-input cleanup and presentation leases retained
by canvases/UI after handoff. Minimal tests, the uniquely named broker example,
API compatibility and streamed owner checks have separate logs; record their
terminal outcomes rather than assuming them from this subset.

Continue next with sharp native graphical image planes, rich terminal events
and links, explicit native source recovery/natural-exit/termination/slot cleanup,
existing-source observation and transport resync. Further load work includes
rapid visibility/identity rollover, aggregate admission contention and sustained
network impairment. Keep the full audited checklist in `docs/plan.md` active;
this is neither full feature parity nor a new canonical release/Nix gate.

Final remaining outcomes for this checkpoint: minimal release tests pass
(253 passed, three explicitly ignored); the separately named broker example
build passes; ordinary native API and legacy automation fixtures pass locally
and over SSH; and the full private managed-owner/stream suite passes locally
and over SSH. Logs use the `target/native-view-resources-` prefix with
`minimal-unit`, `example-build`, `api-local`, `api-ssh`, `legacy-local`,
`legacy-ssh`, `broker-local` and `broker-ssh`. All corresponding process handles
were observed at terminal exit 0. Updated source/fixture checksums and review
patch are in `target/native-adapter-candidate-receipt.json` and
`target/native-adapter-candidate.patch`. The candidate lock still matches the
canonical lock byte for byte. No validation process remains pending at this
checkpoint and there is no user-input blocker. Continue with actual sharp host
image-plane delivery and the remaining original requirements, without treating
these internal component checks as completed inline graphics or full parity.

### Sharp native image planes and nonblocking display delivery

Previous goal turn made verified progress on native view reconnection/resource
lifetimes. This turn implements the missing sharp-image vertical path in the
same isolated candidate, including the actual display-side presenter. The full
original parity goal stays active. No live session, App Space worker, default
preference, canonical executable or immutable KIT dependency was replaced.

New `graphics_planes.rs` produces lossless PNG planes from retained source
images while keeping the terminal grid as native text. Under-background pixels
respect source cell backgrounds/inverse, negative layers composite in order,
and above-text pixels remain separate. Cached image payloads are shared across
unchanged image states instead of re-encoding/resending on each text update.
`Watch.pixels` defaults false; `image-planes-v1` advertises owner support. The new
local helper reports support alongside its existing font metadata. Older
helpers and cells views retain their previous paths.

New `graphics_frontend.rs` validates inline PNG geometry and emits scoped Kitty
placements below/above native text and below the existing chrome. It chooses its
own IDs and accepts no paths, commands or arbitrary escapes. Replacements precede
retirement; deletion and process exit clean up only owned placements. Actual
Kitty testing found that scoped clears needed an explicit flush; that bug was
fixed before the final physical proof. Removed/replaced panes and invalidated
geometry now clear old placements rather than leaving a stale image behind.
Review hides/restores retained images; entering/leaving an empty App Space clears
and restores them without creating an app worker or changing source PIDs.

New `graphics_delivery.rs` isolates presentation from the relay's packet pump.
Its 16 MiB/64-view pending queue replaces obsolete requests per view and keeps
view ordering fair, plus one bounded in-flight request. Queue admission is the
meaning of the extension's effect ACK. Neither presenter waiting nor the
font/probe mutex blocks media/control packets and keyboard input. Incoming
memory admission now charges raw/string bytes and JSON structure separately,
avoiding the previous blanket 40x charge for large PNG strings. Retained/source
and frontend working budgets, output limits and protocol guards still apply.

Verified evidence:

- `target/native-planes-{local,ssh}.log`: actual ordinary native graphical views
  receive lossless planes preserving a 20x20 one-pixel checkerboard, delete them,
  hide/restore them in review, and switch to/from empty App Space with unchanged
  source processes. SSH covers both presentation and configured source host.
  Existing styled-output, shared-source, resize, retained-history, shared
  connector and reconnect/delivery-socket tests remain passing.
- `target/native-planes-{cells,ssh-cells}.log`: those paths preserve input,
  output/history and source identities while sending no graphical extension.
- `target/native-planes-presenter.log`: the actual separate local relay and
  terminal-output service preserve PNG bytes, z ordering, cached updates,
  replacement order, scoped deletion, exit cleanup and unrelated protocol data.
  It pauses only its own output process and verifies input still travels upstream.
- `target/native-planes-delivery.log`: a private output service withholds its
  image ACK while media packets, presented/control and literal keyboard input
  continue. Twenty updates coalesce to the latest pending image without replay
  of obsolete requests. This tests packet flow, not physical speaker playback.
- `target/native-planes-kitty.log`: real Kitty in private Xvfb with cached Nix
  Mesa software GLX confirms native glyphs visible over the lower plane, hidden
  by the upper plane, one-pixel image geometry and scoped deletion restoring
  native text. Source/relay PNG bytes are exact; framebuffer comparisons allow
  the observed maximum one-level 8-bit GPU edge/glyph rounding. Baseline,
  below-text, above-text and restored screenshots plus measured pixels are in
  `target/native-planes-kitty-artifacts/`. No user desktop window is opened.
- `target/native-planes-clippy.log`: strict workspace/all-target/all-feature
  Clippy passes. Release workspace all-feature tests pass with 259 root and
  four terminal-client tests; minimal tests pass with 258 root and four client
  tests (three explicitly ignored root tests in each).
- `target/native-planes-frontend-build.log`: uniquely named release application,
  terminal client and presenter example build successfully. The broker example
  retains its separate uniquely named build. Managed service/stream proofs
  pass locally and over SSH in `target/native-planes-broker-{local,ssh}.log`;
  ordinary native API and legacy automation compatibility pass over SSH in
  `target/native-planes-{api-ssh,legacy-ssh}.log`.

All corresponding process handles were checked at terminal exit 0. Source/UI
proofs and display-service proofs cover separate parts of the same protocol;
the offscreen display proof is not a full real-agent/WAN/audio acceptance run.
The standalone fixture corrected its process layout to match deployment: the
relay is separate from the process owning terminal stdout. The virtual X server
also needed an explicit cached Mesa driver environment to expose GLX; no global
configuration/package change was made. No validation process is pending here.

The updated patch/checksum receipt must include the new source modules, presenter
example and terminal-client source, plus all native image fixtures. Candidate
manifests and binary/example renames are staging-only and must never be promoted.
The canonical/candidate lockfiles remain byte-identical. There is no new
canonical release gate or Nix package validation, and no installed backend switch.

Next implement explicit native pane recovery/natural-exit/termination/slot
cleanup with the durable source references, then compatible observation/adoption
of existing sources. Rich OSC 8/clipboard/title/attention forwarding, buffer
switch/graphics history, host placement-loss repair, chunked large images,
shared-memory/file/animation/placeholder support, constrained-network/media load
and interrupted-creation/process-group handling remain required. Validate the
integrated product through the canonical release/Nix gates before installation.
Do not mark inline graphics or the entire audited checklist complete from this
candidate. Continue all other unchecked requirements in `docs/plan.md`; no
user-input blocker prevents the next implementation work.

Final sharp-image checkpoint follow-up: the delivery queue implementation now
passes its blocked-presenter regression (`target/native-planes-delivery.log`),
including literal keyboard input and a media packet while output is stalled.
The actual local relay/output proof passes with that queue in
`target/native-planes-presenter.log`. Real offscreen Kitty passes after scoped
clear flushing was corrected; its screenshots and `pixels.json` are retained
under `target/native-planes-kitty-artifacts/`. One-level GPU rounding is recorded
explicitly instead of treating framebuffer antialias values as byte-exact PNGs.

Strict workspace/all-target Clippy passes. Final release workspace tests are
259 root + four terminal-client tests with all features, and 258 root + four
client tests without defaults (three ignored root tests in each). The uniquely
named application/client/presenter release build passes. The four ordinary UI
modes, source broker/managed streams locally and over SSH, native API over SSH
and legacy API over SSH retain their terminal passes recorded above; subsequent
changes were confined to the display delivery/presenter path and its fixtures.
All validation handles are terminal. Updated code/fixture/artifact checksums are
in `target/native-adapter-candidate-receipt.json`, with the reviewable source
patch at `target/native-adapter-candidate.patch`. Include terminal-client source
and the presenter example in that review, excluding staging manifests/renames.

This still needs native recovery/termination/exit/slot lifecycle work and mixed
older-frontend/backend compatibility guards before activation; merely preserving
an old owner is not proof that an older frontend can safely open a new native
project. Continue those integrations and the original audited requirements.
Nothing here authorizes restarting retained user processes or replaying agent
commands/input to upgrade them. The installed backend is unchanged and this is
not a completed goal or a new canonical release/Nix validation.

### Installed memory-retention fixes (November 5)

The installed canonical build now explicitly services Smithay's deferred GL
resource deletion after dropping each offscreen framebuffer binding. An idle
damage pass skips `GlesFrame::finish`, which otherwise performs that deletion;
repeated idle bindings could therefore retain framebuffer objects indefinitely.
A hardware regression test verifies actual GL object retention before cleanup
and deletion afterward across 256 passes, without opening any user windows.

The legacy native-cell view also keeps at most one unsent delta per pane while
reliable output is congested. Further parser changes remain dirty until that
delta is admitted, preserving the receiver's baseline. The private
`scripts/test-memory-backpressure.py` fixture verifies the queue bound, eventual
catch-up and an unchanged terminal source PID/start identity. Its diagnostic
counter is `terminal_presentation.queued_cell_frames` in the view snapshot.

Release workspace tests with all features and without defaults, strict Clippy,
the hardware GL regression, the slow-consumer test, native render-cache proof
and the release build pass. Private SSH App Space acceptance also passes with
1080p video at approximately 60 fps and continuous PCM delivery. Evidence is in
`target/memory-*.log` and `target/memory-fix-receipt.json`.

All 11 original STAR/FLUX process PID/start identities were preserved. No live
controller, terminal backend, agent or App Space owner was restarted. The large
existing App Space owner still has its older executable loaded. A normal Expose
repaint of its verified private image-viewer window did not return its retained
RSS to the OS; do not claim that its approximately 13 GB heap was reclaimed.
It must load the fixed executable when the user can safely reopen that App
Space. Never stop its running application just to apply this fix.

The isolated native-adapter candidate remains unpromoted. Preserve these
canonical fixes when rebasing that candidate; its earlier validation receipt
does not cover the memory changes above.

### Native pane lifecycle and compatibility candidate (November 6)

Continued in `target/inline-graphics-workspace`, without replacing the installed
binary or refreshing any real frontend. The installed November 5 memory fixes
are now also present in the candidate.

Native **Recover to shell…** now retires the exact saved source, confirms process
exit and PTY EOF, persists a fresh pane epoch/launch intent, and opens a shell.
Saved agent commands and pending launch permissions are removed. Lost Open
responses resolve through read-only discovery rather than command replay.
Concurrent/stale results cannot overwrite another view's newer binding.
Termination additionally fences the pane epoch immediately before retirement.
Natural exits can be reaped and their slots reused. The final exit leaves an
empty project; explicitly reopening creates one source shared by both views.

Candidate automation adds `panes.recover` (epoch plus `confirm: "RECOVER"`) and
`panes.reap` (epoch, exited sources only), using existing durable operation
receipts. Replaying completed operations does not stop a reused slot or launch
another shell. A parent which exited while a child still holds its PTY remains
observable and blocks replacement; the held-child fixture verifies refusal,
no duplicate launch, unrelated-source preservation and explicit retry after
the private child releases the terminal.

New retirement requires `retirement-eof-v1` in the source owner's Hello. API
capabilities and UI menus suppress unsupported native recovery/termination.
Compatible pre-retirement owners keep read/input service and are never stopped
to upgrade them. The older installed foundation owner lacks workspace-adapter
capabilities: new adapter activation refuses it while its original source stays
alive. An isolated executable reproduces the preceding candidate's older Close
semantics for the narrower retirement-capability test; this is a protocol fixture,
not a separately released product version.

The compatibility fixture opens a source created by the validated legacy binary
in a new frontend, then adds a native sibling. Both keep their original backend
and survive frontend detachment. Invalid/null ownership entries, invalid maps,
conflicting owners and wrong pane/project references fail closed rather than
creating a legacy fallback. Native metadata helpers also refuse malformed
ownership rather than silently invoking the old helper.

Native image effects now defer generating another full image command while one
is queued, complementing terminal delta backpressure. With 64 image effect ACKs
withheld, private SSH acceptance observes a maximum of one queued image command,
then verifies catch-up, scoped image cleanup during recovery and source reuse.
Failed Watch requests re-probe the retained source grid before retrying; input
and frame acknowledgements no longer wait for presentation retry backoff.

Evidence: `target/native-recovery-{local,ssh,held-child}.log`,
`target/native-recovery-ui-{local,ssh,cells,ssh-cells}.log`,
`target/native-recovery-image-backpressure-ssh.log`,
`target/native-compatibility-{local,ssh}.log`,
`target/native-older-owner-{local,ssh}.log`, and
`target/native-foundation-owner-{local,ssh}.log`.
Managed broker/stream regression logs are
`target/native-recovery-broker-{local,ssh}.log`. Strict Clippy and release tests
with all features/without defaults are recorded in `target/native-recovery-*.log`.
The current review patch and checksum receipt remain
`target/native-adapter-candidate.patch` and
`target/native-adapter-candidate-receipt.json`; staging manifest/bin names must
not be promoted.

This is progress on the full parity objective, not completion or installation.
Still required before activation: complete process-tree supervision, owner-loss
and never-delivered launch recovery, broader existing-source observation/adoption,
and compatible frontend/backend activation handling. Rich OSC 8/clipboard/title/
attention forwarding, buffer/graphics history, placement-loss repair, larger and
additional graphics transports, constrained-network/media stress and final
canonical release/Nix validation also remain. Continue every other unchecked
requirement in `docs/plan.md`; no user-input blocker prevents further work.

### Sidebar navigation refinement (November 13)

The graphical sidebar now uses a continuous theme-derived surface, quiet section
labels and native one-pixel square plus/minus disclosure controls. Category
headers no longer use separate panel blocks or hover outlines. Project rows
retain full-width selection, a subtle selected surface, host/agent detail,
working animation and error/question/approval markers. Category project totals,
hidden counts, progress/attention summaries and coordinator/menu access remain.
The footer reports actual projects shown rather than counting category headers
and drop targets as projects or claiming collapsed projects are visible.

The existing shelf geometry tests and strict workspace Clippy passed. Private
category acceptance passed locally and over authenticated loopback SSH, covering
creation, assignment, drag/drop and append targets, keyboard expansion,
independent collapse, rename, hidden-project counts, reveal, stale-edit fencing
and persistence through private controller replacement. Original fixture source
PIDs survived. Evidence: `target/sidebar-navigation-{tests,clippy,local,ssh}.log`;
the private rendered inspection is `target/sidebar-navigation-preview.png`.
This is a UI refinement, not completion of the remaining parity requirements.
No live user frontend, agent, terminal source or App Space was restarted.

### Native supervision and foreground identity acceptance (November 14)

Resumed the isolated native-source candidate without replacing the installed
executable or refreshing a live frontend. Completed the previously paused
supervision acceptance matrix: stubborn PTY holders, detached children with
closed stdio, nested subreapers, children forked during retirement, natural exit,
operation replay, source/slot reuse and unrelated-source preservation. Actual
Bash foreground job control, Ctrl+C and background-job cleanup pass.

Forced supervisor loss leaves descendant retirement unverified. Both local and
SSH fixtures observe final EOF but prove that the source binding stays unchanged,
recovery is refused and no duplicate shell is launched. Older owners advertising
only exit/EOF retain read/input and have unsupported retirement controls disabled.
Idle sampling confirms that supervisors do not periodically poll.

Fixed a metadata gap introduced by the supervisor layer: foreground program and
working-directory inspection now follows the verified program rather than the
supervisor. Private local/SSH checks cover a recognized agent and a plain source
which changes directory; agent PID and source/supervisor PID remain separate.

Graphical and cells UI lifecycle fixtures pass locally and over SSH. They cover
real recovery/termination dialogs, both views following the new epoch, obsolete
monitors being fenced, final natural exit staying empty, and explicit reopening
creating one shared source. Strict workspace Clippy and release tests pass:
259 root tests plus 4 client tests with all features, 258 root plus 4 client
without defaults (4 root tests ignored in each matrix).

Evidence: `target/native-tree-resumed-{clippy,unit,minimal,build}.log`,
`target/native-tree-resumed-{loss-local,loss-ssh,tree-ssh,shell}.log`,
`target/native-tree-resumed-ui-{local,ssh,cells,ssh-cells}.log`, and
`target/native-tree-resumed-older-{local,ssh}.log`. Updated source/fixture patches
and checksums are in `checkpoints/native-process-supervision/`. The installed
binary SHA-256 and all 5 original PID/start identities are unchanged; see
`target/native-tree-resumed-live-safety.json`.

Remaining work is still substantial. Broker/owner-loss and never-delivered intent
recovery, broader existing-source adoption, metadata/links/clipboard/title/attention,
graphics/history/placement compatibility, constrained-network/media stress and
canonical release/Nix gates are unproven. Inspection confirms that the owner
retains OSC 8 attributes but native view reconstruction currently rebuilds only
text/styles; it needs bounded link metadata forwarding, including link-only
deltas, review, cleanup and reconnect tests. Before activation, rebase the native
candidate onto current global settings, portfolio/UI changes and glyph fixes;
never replace current code or manifests with the staging baseline wholesale.
Every other unchecked requirement in `docs/plan.md` remains in scope.

### November 14 — bounded native OSC 8 projection and retained review

Implemented the hyperlink gap identified above in the isolated candidate, without
changing the immutable shared KIT dependency or the production backend. Every
cells projection includes a complete target dictionary and disjoint spans, even
when no visible text changed. New views preflight counts/strings before owned
decode, reserve metadata memory and replace annotations without repainting text
or styles. Older owners can omit the optional table.

Limits are 1,024 targets, 4,096 spans and 256 KiB target text, with existing parser
limits of 8 KiB per URI and 1 KiB per parameter string. Invalid control characters,
parameter separators, overlaps and geometry are rejected. Excess fails visibly
rather than dropping targets or redirecting links. Repeated targets share entries.

The actual two-view UI test exposed a second gap: native history export retained
styles but discarded hidden destinations. Fixed the bounded exporter to preserve
OSC 8 transitions, including wide/combining labels and wrapped rows, without
changing the retained source. Frozen review now retains its original targets
while another view receives new live destinations.

Strict all-target/all-feature Clippy and release workspace build pass. Release
tests pass with all features (264 root plus 4 client) and without defaults
(263 root plus 4 client); 4 root tests remain ignored in each matrix. Private
real-controller tests pass in graphical/cells modes locally and over SSH, covering
hidden targets, semicolon-containing URIs, identical labels with new/cleared
destinations, frontend copy responses and pointer regions, independent frozen
review, resize, connector reconnection and explicit source recovery clearing old
targets across both views. Older-owner local/SSH checks preserve read/input and
refuse unsupported lifecycle commands.

Evidence is in `target/native-links-{clippy,unit,minimal,build}.log`,
`target/native-links-ui-{local,ssh,cells,ssh-cells}.log` and
`target/native-links-older-{local,ssh}.log`. The separate
`checkpoints/native-hyperlinks/` source patch and receipt preserve reconstruction
and evidence hashes. All 5 original production PID/start identities and the
installed executable are unchanged relative to this stage's baseline.

This completes the tested OSC 8 forwarding slice, not native migration or full
Herdr parity. General clipboard/title/bell/attention forwarding, owner-loss and
never-delivered intent recovery, broader source adoption, graphics/history and
placement compatibility, network/media stress, current application/UI rebasing
and canonical release/Nix gates remain. Every other unchecked requirement in
`docs/plan.md` remains in scope; the goal stays active.

### November 14 — native titles, clipboard writes and metadata diagnostics

Implemented source-owned bounded metadata in the isolated native candidate.
OSC 0/1/2 values survive native reconstruction and feed default tab labels, while
explicit user names, agent labels and imported names retain precedence. Empty
titles restore generated labels. Audible/visual bell counters are retained and
forwarded for diagnostics; they do not fabricate lifecycle or attention events.

OSC 52 UTF-8 clipboard writes now reach existing frontend clipboard responses.
A source-scoped counter and per-watch attachment/acknowledgement baseline prevent
replay on late attachment, hidden-tab reveal, resize and reconnect. Source recovery
resets the metadata with the new source; both views accept its new copy sequence.
Slow views have independent credit and coalesce to the latest retained write.
Subsequent frames omit acknowledged clipboard text, avoiding a retained 64 KiB
payload being sent repeatedly during animation. Read-only snapshots omit copy
payloads, and clipboard read requests are never serviced or forwarded.

Limits are 4 KiB per title/icon and 64 KiB UTF-8 clipboard text. Accepted metadata
lazily reserves 192 KiB from the shared owner retained budget; invalid values leave
accepted state intact and increment rejection diagnostics. Incoming metadata is
preflighted before owned decode and charged to the existing snapshot budget.
State-file diagnostics expose titles/counters/sequences, not copied text.

The managed wire fixture found an additional supervision bug: a loaded owner
whose executable had been unlinked could no longer launch sources because
`current_exe()` returned a deleted filename. Fixed supervisor launch to use the
loaded `/proc/self/exe` image. The local/SSH managed wire tests explicitly unlink
an owner copy before opening supervised sources, and verify clipboard-only empty
text deltas, independent acknowledgement floors, payload omission, stale-ack
rejection and read-only snapshot behavior.

Strict all-target/all-feature Clippy, the release workspace build and release
unit matrices pass: 270 root plus 4 client with all features, 269 root plus 4
client without defaults, with 4 root tests ignored in each matrix. Real graphical
and cells UI tests pass locally and over SSH, using three views to cover retained
titles/icons, explicit tab-name precedence, bell diagnostics, Unicode and a 60 KiB
payload, empty writes, invalid/oversized/binary rejection, query suppression,
late attachment, hidden reveal, resize, connector recovery and explicit source
recovery. Existing hyperlink and older-owner checks pass locally and over SSH.

Evidence is `target/native-metadata-{clippy,unit,minimal,build}.log`,
`target/native-metadata-ui-{local,ssh,cells,ssh-cells}.log`,
`target/native-metadata-wire-{local,ssh}.log`,
`target/native-metadata-links-{local,ssh,cells,ssh-cells}.log`, and
`target/native-metadata-older-{local,ssh}.log`. The separate
`checkpoints/native-metadata/` preserves the incremental source patch, fixtures
and evidence hashes. Original production PID/start identities and the installed
executable remain unchanged relative to this stage's recorded baseline.

Remaining native gates include attention/OS notification delivery, owner-loss
and never-delivered intent recovery, broader source adoption, graphics/history
and placement compatibility, constrained-network/media stress, application/UI
rebasing and canonical release/Nix acceptance. Clipboard delivery is bounded
latest-value delivery, not durable offline event history; separate primary/cut
buffer selection and system clipboard reads remain outside the existing text
clipboard interface. Every other unchecked requirement in `docs/plan.md` remains
in scope, and the goal stays active.

### November 14 — guarded recovery of never-delivered native intents

Added live-owner cancellation for a saved launch intent that was never accepted.
The owner serializes launch-status resolution with its active/initializing maps.
Explicit recovery durably consumes an unstarted ID with an owner/source/epoch/
digest-bound fence before assigning a fresh terminal epoch and launching a default
shell. A delayed original Open is rejected by exclusive receipt creation; saved
commands and one-shot launch hints are cleared for the recovered pane. Inspection
alone does not consume the ID. Opening, consumed/uncertain, malformed and identity-
mismatched cases remain unavailable for cancellation.

Existing lost-Open-reply recovery still resolves the retained source through
read-only discovery before guarded tree retirement. It does not require the new
fence capability merely to discover an already-existing source. Pending-slot
checks compare complete launch intent while allowing a durable ready source to
have been recorded before its view received the reply.

A real UI fixture found that startup failure stopped the health monitor before
pending recovery could appear. Saved native references now retain a read-only
monitor after startup failure, so graphical/cells recovery remains accessible
without another launch attempt. The menu and confirmation explain cancellation
of an unstarted command and opening a fresh shell. Explicit termination controls
retain their separate retirement requirements.

Receipt reads are bounded, reject linked/nonregular files and use nonblocking
opens. Unit coverage verifies idempotent sealing, mismatched identities, damaged
records, an initializer held at its queue boundary, refusal of a delayed spawn,
and a FIFO receipt returning promptly rather than freezing the broker.

Strict all-target/all-feature Clippy, release workspace build and unit matrices
pass: 271 root plus 4 client with all features, 270 root plus 4 client without
defaults; 4 root tests remain ignored in both matrices. Private local/SSH automation
checks cover read-only health, a durable fence, one fresh shell, operation replay,
a delayed old Open, lost-reply discovery and sibling survival. Graphical/cells
UI fixtures locally and over SSH use the real menu and confirmation and attach
two independent views to the fresh source. Metadata and older-owner regressions
remain passing. One earlier full unit run observed the pre-existing PTY fixture's
file-creation/write race; its isolated retry and subsequent full matrices pass.

Evidence: `target/native-launch-fence-{clippy,unit,minimal,build}.log`,
`target/native-launch-fence-{local,ssh}.log`,
`target/native-launch-fence-ui-{local,ssh,cells,ssh-cells}.log`,
`target/native-launch-fence-metadata-{local,ssh}.log`, and
`target/native-launch-fence-older-{local,ssh}.log`. The separate
`checkpoints/native-launch-fence/` contains the incremental patch, fixtures and
validation identities. Original production PID/start identities and the installed
executable remain unchanged relative to this stage's baseline.

This resolves the tested live-owner never-delivered case, not general owner loss.
Durable supervisor retirement/adoption proof after broker death, broader source
adoption, remaining attention/graphics/history/network cases, current UI/core
configuration rebasing and canonical release/Nix gates remain. Every other
unchecked requirement in `docs/plan.md` remains in scope; the goal stays active.

### November 14 — certified native recovery after broker loss

Added a durable retirement journal to new native supervisors. It binds the exact
owner, source reference, source epoch, launch digest and receipt-directory device/
inode. The single-threaded supervisor pins its actual parent with a pidfd,
retains only close-on-exec handles, and on owner death retires its owned tree.
Only after all descendants are reaped and the kernel reports `ECHILD` can it
atomically write/fsync a certificate. The owner pidfd is disabled in the poll set
after observation, avoiding a retirement busy loop; idle supervisors still have
no periodic timer.

Read-only owner-loss health verifies that certificate and supervisor completion.
Explicit recovery uses the same real graphical/cells menu and confirmation, then
finds a compatible replacement owner and assigns fresh source/terminal epochs for
a default shell. Saved commands and stale input are never replayed. Lost Open
replies are handled even when only the durable slot records the source reference;
remote slot intent and full known-source identities remain checked. Explicit
termination can remove a certified dead source without creating an owner, while
automatic reap leaves owner-loss panes visible for the user's decision.

Missing/damaged proof, identity/directory mismatch, a still-finishing supervisor,
unsupported owners and supervisor death with unverified descendants remain
unavailable for replacement. Unit checks cover certificate identity/digest/
directory binding, unfinished records and live-supervisor rejection. A private
negative runtime case kills a supervisor, preserves its unverified descendants,
and proves that recovery changes neither binding nor owner and launches nothing.

Strict all-target/all-feature Clippy, release workspace build and unit matrices
pass: 272 root plus 4 client with all features; 271 root plus 4 client without
defaults, with 4 root tests ignored in both matrices. Local/SSH automation kills
only private owners of stubborn detached descendants/nested subreapers, proves
all descendants reaped before accepting the certificate, rejects corrupt proof,
and recovers one new owner/shell. An unrelated live process survives. Real
UI menu/confirmation checks pass in graphical/cells modes locally and over SSH,
including shared post-recovery input. Unstarted-intent, metadata, older-owner,
Bash job-control and idle/process-tree regressions pass.

Evidence: `target/native-owner-loss-{clippy,unit,minimal,build}.log`,
`target/native-owner-loss-{local,ssh,pending-local,pending-ssh,guardian-local,guardian-ssh}.log`,
`target/native-owner-loss-ui-{local,ssh,cells,ssh-cells}.log`,
`target/native-owner-loss-{fence-local,fence-ssh,metadata-local,metadata-ssh,older-local,older-ssh,shell,tree-ssh}.log`.
The separate `checkpoints/native-owner-loss/` saves incremental source, fixtures
and reconstruction/validation identities. Original production PID/start identities
and the installed executable remain unchanged relative to this stage's baseline.

This establishes the tested certificate-backed owner-loss case, not unrestricted
owner-loss recovery or source adoption. Pre-supervisor startup uncertainty,
missing/lost journals, unsupported legacy sources and broader adoption still
refuse replacement. Remaining attention, graphics/history/placement and network/
media cases, application/UI rebasing and canonical release/Nix gates remain;
every other unchecked requirement in `docs/plan.md` stays in scope. The candidate
is uninstalled and the full Herdr parity goal remains active.

### November 14 — native candidate rebased onto current code and UI

Reconstructed and hash-checked the entire saved native checkpoint chain, then
three-way merged it with canonical `5a956fdf` and the user's current uncommitted
source edits into `target/native-rebase-workspace`. Canonical source and installed
binaries were not overwritten. The rebase has unique executable/example names
and a fresh `target/native-rebase-build` directory, avoiding shared-parser artifact
reuse across workspaces.

Resolved two conflicts: runtime keeps the native opt-in alongside ignored legacy
project-theme serialization, and snapshots keep global appearance plus native
metadata. Two native-only palette initializers now use core application settings
rather than the legacy project field. Current portfolio/coordinator terminology,
glyph fixes, dark Gruvbox sidebar, diagonal grid, hover/gutter and 11px boxes with
3px disclosure marks remain intact. The canonical Cargo.lock is byte-identical;
no immutable shared dependency was replaced.

Strict all-target/all-feature Clippy and the release workspace build pass. Release
unit matrices pass: 276 main plus 4 client with all features; 275 main plus 4
client without defaults, with 4 main tests ignored in both matrices. Native-enabled
theme/session tests pass locally and over SSH in graphical/cells modes, including
stale project palettes, rename, global preference changes and session switches.
A private Gruvbox Dark reference render is PNG-byte-identical to the installed
build's same-model render, proving preservation in that reference case.

Rebased lifecycle/menu, OSC 8, clipboard/title, unstarted recovery and portfolio
create/assign/drag/collapse/order checks pass locally in graphical mode and over
SSH in cells mode. Owner-loss and missing-supervisor-proof automation also pass
over SSH. All fixtures use private namespaces and fake programs. Evidence is in
`target/native-rebase-{clippy,unit,minimal,build,chrome}.log`,
`target/native-rebase-theme-{local,cells,ssh,ssh-cells}.log`,
`target/native-rebase-{owner-ui-local,owner-ui-ssh-cells,owner-api-ssh,guardian-api-ssh}.log`,
`target/native-rebase-{links-local,links-ssh-cells,metadata-local,metadata-ssh-cells,fence-local,fence-ssh-cells}.log`,
and `target/native-rebase-portfolio-{local,ssh-cells}.log`.

The separate `checkpoints/native-rebased/` archives a patch against the current
canonical commit and a saved canonical UI patch, replacing the earlier base for
integration. Its reconstruction/validation receipt verifies all controlled source
files and original production PID/start identities. The installed executable and
five tracked production processes remain unchanged.

The candidate remains uninstalled. Remaining uncertified recovery/adoption,
attention, graphics/history/placement and constrained-network/media cases,
canonical release/Nix acceptance and production integration remain. This completes
the current-code rebase slice, not the Herdr parity goal; every unchecked item in
`docs/plan.md` remains in scope.

### November 14 — production-layout release acceptance started

Created an independent release checkout outside the canonical Cargo workspace
from the rebased candidate, reversing only its isolated executable/example and
vendored-workspace manifest changes. All three production manifests and
`Cargo.lock` match canonical byte for byte. This checks the shipping names and
helper lookup paths without installing the candidate or changing live views.

The unmodified `scripts/check-release` is running there. Version/pin consistency,
AppRun relocation/argument handling, both formatting checks and strict
all-target/all-feature Clippy have passed. Both shipping release test matrices
passed (276 main plus 4 client with all features; 275 plus 4 without defaults;
4 main ignored in each). Dependency policy and executable/example builds passed.
Private behavioral checks remain pending until that command finishes; the gate
is not yet a pass.

A separate source snapshot, excluding build outputs, is running `nix flake check`
with package and installed-executable smoke checks. That command passed on local
Linux x86_64, including 276 main plus 4 client tests and all three installed
executables. ARM64 was not built by this workstation run. Logs are
`target/native-release-gate.log` and `target/native-release-nix-gate.log`.
`target/native-release-gate-state.json` records both workspaces, runner PID/start
identities, execution handles and exit-file paths so a later turn can inspect the
same runs rather than restart them after an observation timeout. A current safety
check preserves all five tracked production process identities and the installed
executable hash. No production integration or default-backend change occurred.

The shared `test-session-manager.py` now accepts `--native-sources`, selects the
native backend explicitly for fixture projects and verifies that both frontends
share its durable source binding. This prevents passing native acceptance by
silently using the existing default backend. Cleanup authenticates a shutdown of
the fixture's private source owner, checks its boot/process/directory/environment
identity and waits for retirement before deleting its namespace. Ordinary runs
retain their existing default-backend coverage. The tmux-specific startup-failure
injection rejects this native mode rather than claiming the wrong coverage.

Running a copy of that fixture against the production-named candidate passed
nested pane layouts and searchable history locally in graphical mode and over
SSH in cells mode. Coverage includes split/resize/zoom/focus/move, stale
termination confirmation, independent views, frozen Unicode history search/copy
and preserved pre-attachment tmux history. Native lifecycle, automation,
cold-start, background screens and notification cases are running separately;
their per-case exits are retained in `target/native-release-ui-matrix.json`.
These fixtures start no real agents and do not change production App Space.

### November 14 — saved conversation references survive native recovery

Extended native acceptance beyond the earlier theme/portfolio checks. Pane
layouts, searchable history, structured lifecycle and automation pass locally in
graphical mode and over SSH in cells mode. Background screen collection and
notification/inbox behavior also pass in those two presentations. These checks
assert native source bindings; imported tmux sources retain their original
ownership. The background fixture now removes project-bound native identities
when constructing a different imported project, instead of transplanting them.

Cold-start acceptance found an actual integration gap: native recovery replaces
the terminal epoch, which hid the registered conversation and rejected explicit
resume. The candidate now saves a separate link from the recovered epoch to its
original registered run. The report's epoch, boot/process identity, provider,
conversation and exact resume vector remain unchanged. Inherited reports are
stale and never establish live health, even if process observations happen to
match; old heartbeats and unrelated recycled slots remain rejected. A new
registered run supersedes the inherited reference. Repeated deliberate recovery
retains it. Publishing the link under the project transaction before the new
epoch does not grant permission to replay saved program launches.

The same-boot cold-runtime fixture now shuts down the actual private native owner
and uses real recovery confirmations after removing runtime state. It checks
both restored panes, split persistence, styled Unicode/historical screen review,
exact registered conversation resume in a new tab, corrupt archives, screen
pause/clear/resume, command non-replay and an unrelated imported source. This does
not establish all reboot/missing-certificate cases. Final-binary checks pass
locally in graphical mode and over SSH in cells mode. Legacy lifecycle checks
also pass in both cases. Strict Clippy passes; release tests pass with all
features (277 main plus 4 client) and without defaults (276 plus 4), with four
main tests ignored in each matrix.

The two-file source increment, shared-fixture changes, reconstruction verifier
and eight validation log identities are saved in
`checkpoints/native-conversation-recovery/`. It follows the rebased checkpoint;
the older patch and its evidence have not been rewritten to claim this fix.
Logs use `target/native-conversation-recovery-final-*.log`. The increment's own
Nix package/smoke check is running separately in
`target/native-conversation-recovery-nix.log`; the earlier Nix pass proves only
the preceding source. All five tracked production processes and the installed
executable hash still match the release-validation baseline.

The production-layout release run stopped at `test-agent-activity.py`: its
`exec -a codex python3` spoofed Python's executable identity, loaded the system
standard library under Nix and failed importing `_ctypes`. Launching the fixture
with `sys.executable` fixes this without changing its `prctl` agent identity or
activity expectations. The corrected check passes. The original failed log and
exit remain intact; `target/native-release-gate-resume.log` continues from that
check, retaining completed packaging/build/plugin evidence. The complete release
gate is still pending. Six obsolete source owners from earlier private rebase
fixtures with already-removed namespaces were identified by exact executable and
private environment and retired; no production owners were touched.

The candidate remains uninstalled and has not been promoted into canonical Rust
source. Broader adoption/uncertified recovery, attention forwarding, graphics/
placement/network/media compatibility, production integration and every other
unchecked requirement in `docs/plan.md` remain in scope. Full Herdr parity is not
complete.

### November 14 — requested program startup is acknowledged before publication

The saved-conversation increment's Linux x86_64 Nix package, 277 main plus 4
client tests and all three installed-executable smoke checks passed. Its receipt
now verifies the complete reconstructed source against that exact Nix snapshot.
This acceptance applies to that increment, before the startup work below.

The production-layout release continuation found another product regression in
the streamed source-broker fixture: `Open` returned a live source for a nonexistent
requested executable because starting the supervisor itself succeeded. The
supervisor now atomically records the actual command exec result. The initializer
validates its full owner/source/digest/directory identity and waits for that
acknowledgement before publishing the source. The wait is bounded and off the
render/input path. Missing acknowledgement consumes the launch and retires the
guardian rather than authorizing replay or leaving its executed children behind.

Verified failed exec can be resolved only with matching startup failure and
ECHILD retirement records and a dead supervisor. Missing proof remains unknown;
wrong identity or directory is rejected; a successfully executed program cannot
be labeled never started. Explicit pending recovery then uses the existing real
confirmation and a fresh shell, without replaying the saved command. Owners
advertise the additive `supervised-startup-v1` capability.

Strict Clippy and binary/example builds pass. Release unit matrices pass: 278
main plus 4 client with all features, 277 plus 4 without defaults, with 4 main
ignored in each. Standalone and managed-service streamed broker checks pass
locally and over SSH. The actual failed-exec UI menu/confirmation and shared fresh
source pass locally in graphical mode and over SSH in cells mode, preserving the
sibling pane and never running its saved launch hint. The reconstructed four-file
increment, fixture changes and ten validation log identities are in
`checkpoints/native-startup/`; logs are `target/native-startup-*.log`.

The release checkout now contains both increments. Its separate continuation,
`target/native-release-gate-startup-resume.log`, reruns formatting, strict Clippy,
both unit matrices and binary/example builds, then retries the failing streamed
broker command and the remaining release checks. The previous failed command/log
is retained. The startup increment also has a fresh Nix package/smoke run in
`target/native-startup-nix.log`; both jobs remain pending at this checkpoint.
Their handles, source identities and workspaces are recorded under `target/`.

The user emphasized that AI sessions must resume. Preserving a shell or launching
a mock exact-ID resume vector is not sufficient evidence that a real provider's
conversation resumed. Provider-confirmed identity capture, thread-switch handling,
native resume and missing-metadata diagnostics remain priority acceptance work.
No provider identity will be guessed from a directory, title, recent activity or
`--last`. The installed Codex 0.169.0 CLI/schema and official hook documentation
were inspected read-only; no real agents were prompted, restarted, reconfigured
or migrated. This work is still uninstalled, all original production process
identities and the installed executable remain unchanged, and the full goal
remains active.

### November 14 — pause at the launch-authority checkpoint

The user requested a pause at a sensible point. Completed the already-running
startup-revision Nix package/smoke and production-layout release continuation;
both exited successfully. Their source identities and original failed logs are
preserved. Additional native cold-start checking reproduced a cross-frontend
race: one view consumed the resume permission while another published a default
shell before the resume intent. Private diagnostics establish the wrong-shell
outcome; this was not accepted as AI recovery.

The latest staged fix derives the command from shared pending metadata and
combines one-shot permission consumption, pending-state removal and native
intent publication in one project transaction. Discovery/default-shell RPCs stay
outside that transaction. The pane generation and selected command are checked
again before publication; an identical already-ready reference from another view
is accepted. Legacy/imported/plugin permission handling is retained.

This two-file increment reconstructs exactly in
`checkpoints/native-launch-authority/`. Strict Clippy, all-feature release tests
(278 main plus 4 client; 4 main ignored), binary build and private final-binary
cold-runtime/saved-vector resume locally in graphical mode and over SSH in cells
mode pass. Deterministically forcing the handoff race, further repetition,
minimal-feature and Nix checks on this exact increment remain necessary. Earlier
startup acceptance must not be relabeled as validation of this later change.

The complete pause handoff is [parity-handoff-2026-11-14.md](parity-handoff-2026-11-14.md).
It preserves the full remaining Herdr parity scope and prioritizes real provider
identity capture/resume. A fake program executing a saved vector does not prove
that Codex/Claude restored an AI conversation. No native increment was installed
or promoted into canonical Rust source, no live sessions were migrated and all
five tracked production process identities plus the installed executable hash
remain unchanged. All validation jobs started in this turn have finished; no new
work should run until the user resumes the goal.


### November 17 — resumed: deterministic native resume handoff

Resumed the active parity goal from the November 14 launch-authority checkpoint.
The previous goal work made progress by fixing shared command/intent publication;
this turn adds direct regression evidence rather than treating the old private
cold-start passes as proof that the race was eliminated. The full goal remains
unfinished and active.

The new `--native-launch-race` fixture gates one private host-helper inspect call
before native intent publication. Another frontend must run the registered exact
resume vector once while that opener remains blocked. Both views must still send
ordinary terminal input and subsequently share the same resumed source. The older
startup build fails this test; the staged candidate passes all four combinations
of local/SSH and graphical/cells. The gate's process must still be alive and
unreleased when the resumed command and ordinary input are observed.

The latest Rust increment passed its previously outstanding no-default-feature
release matrix (277 main + 4 client, four main ignored), Linux x86_64 Nix package
and all-feature tests (278 main + 4 client, four main ignored), and three packaged
executable smoke checks. Private cold-runtime/recovery repetitions passed locally
in cells and over SSH in graphical mode. The Nix package's underlying Rust binary
also passed forced handoff locally in graphical mode and over SSH in cells mode.
The package wrapper's fixed Python PATH prevented the private gate from being
reached in the initial two wrapper attempts; those failed fixture logs remain
preserved, and wrapper smoke coverage is distinct from binary race coverage.
ARM64 and a complete unchanged canonical release-gate invocation remain unproven.

Evidence/source/snapshot hashes are saved in
`checkpoints/native-launch-authority/validation-resumed.json`; logs use
`target/native-launch-race-*` and `target/native-launch-authority-resumed-*`.
The staged Rust source reconstructs to the same two-file launch-authority
increment. Canonical Rust source and dependency pins are untouched. Installed
binary hash and all ten production PID/start identities observed at this turn's
baseline remained unchanged. No live frontends or App Space were migrated.

The user explicitly prohibited taking control of the mouse. All work uses private
protocol pipes and disposable namespaces; no desktop mouse or keyboard automation
was used. Keep this constraint for subsequent acceptance work.

Next priority is provider-confirmed native conversation capture and resumption,
including thread switches, late callbacks, pane/launch generations and retained
provider/model/authentication choices. The tests above use fake agents and do not
prove real AI conversation recovery. Broader ownership/adoption, uncertainty,
graphics/history/placement, network/media, canonical integration and every other
unchecked requirement in `docs/plan.md` remain required.

The canonical terminal monitor now resolves a pending launch's command from the
durable project model inside the same transaction that consumes the launch
receipt. Previously the worker could obtain that receipt using a command read
from an earlier frontend snapshot. Reconcile this change with the staged native
handoff when promoting that backend. The staged checkpoint has its own command
selection and does not require changes to its archived evidence.

The canonical recovery record now keeps the current and proposed pane
generations during publication. If writing the pane model is interrupted after
writing the conversation record, a repeated recovery can still resolve the same
saved conversation from the current generation. A retry replaces the abandoned
proposal. Neither generation grants the inherited report live health authority.

## SSH desktop notifications

The optional SSH receiver samples the workspace selected in its own frontend.
When focused-pane suppression is enabled, delivery uses the remote view's
current pane and generation. A missing or expired focus sample defers delivery.

The private loopback SSH test sends focus-gained and focus-lost reports through
a PTY to the real presenter. It verifies forwarding, focused-pane suppression,
project switching, deduplication, and preservation of the original terminal
process. The fixture sets raw mode on its own slave PTY so focus reports do not
wait for a newline. The local notification suite also passed. Evidence is in
`target/ssh-focus-raw-slave.log` and
`target/ssh-notification-focus-final-1.log`.

## Saved-session plugin hooks

The plugin runner now dispatches `session.created`, `session.changed` and
`session.removed` from the durable automation journal. A session event carries
its session resource and data independently of any project context. Deletion
delivers the removal record with null data. The worker version was bumped so the
existing cooperative upgrade path loads the new dispatch behavior.

The private headless plugin suite exercises hooks for session creation, rename
and removal alongside existing project hooks. Logs are in
`target/session-hooks-v5-local.log` and `target/session-hooks-v5-ssh.log`.

## Single-byte OSC 8 hyperlinks

The native terminal parser accepts C1 OSC and ST alongside the ESC-prefixed
forms. It checks incomplete UTF-8 prefixes across reads so characters sharing
those continuation bytes remain intact in both visible text and destinations.
Cancellation and overflow still discard incomplete targets.

All 13 focused OSC 8 tests passed, including mixed delimiters, byte-sized
chunks, URI and label Unicode, malformed sequences, and saved-screen round
trips. See `target/c1-osc8-regression.log`. Parser throughput comparisons
were variable across runs and do not measure UI latency. Browser handoff
remains a separate outstanding part of plugin link support.

## Browser handoff

The terminal link menu and plugin handler chooser offer **Open in browser** for
HTTP and HTTPS destinations on presenters that advertise the STAR/KIT browser
action. The controller sends the selected destination to the presenting computer
and displays the opener result. This path applies when the controller is over
SSH as well as when it is local.

Private cells presenters verified both menu paths, literal OSC 8 destination
delivery to a fake local opener and no opening before explicit selection. The
graphical plugin UI regression and the private SSH App Space streaming regression
passed with the new STAR/KIT pin. Evidence is in
`target/browser-handoff-{chooser-local-recheck,chooser-ssh-recheck,graphical-plugin,app-space}.log`.
The browser opener was simulated, and these runs did not launch a real browser.

## Pane edge placement

A terminal tab's **Move pane to edge…** menu places a visible pane or hidden
tab on any outer edge of its project's split layout. The other branches keep
their structure. The moved pane takes 35% initially and can be resized.

The pane-layout fixture passed in local graphical, SSH graphical and SSH cells
presentations. It verifies tab-menu activation, moving a visible pane,
restoring a hidden tab at an edge, retaining all processes, independent
frontend layouts and reattachment with the moved pane in place. Results are in
`target/pane-edge-final-{graphical,ssh-graphical,ssh-cells}.log`.
Cross-project and separate-tab-group movement remain open.

## Guarded input for imported tmux panes

`panes.send` now works on the active pane of an imported tmux session. It
checks the host boot, pane and foreground process identities, and uses a tmux
command-queue guard to reject changed selections and copy mode before input.
The API retains distinct paste and submit operations and operation replay
protection. Imported pane termination remains owned by the source.

The private import suite passed locally and over SSH. It verified Unicode
paste, explicit submission, replay protection, changed target and copy-mode
rejection, and preserved source topology. The owned-pane automation suite
also passed over SSH. Logs are in
`target/imported-tmux-input-final-{local,ssh,owned-ssh}.log`.
Imported Herdr guarded input remains open.

## Imported Herdr process diagnostics

`panes.get` verifies the same owned Herdr socket and server process before
and after querying pane process information. The imported pane and terminal
IDs must match before STAR/FLUX publishes the shell PID, start time and
foreground process group. Legacy records without a terminal ID report that
identity is unavailable.

The private Herdr import fixtures passed locally and over loopback SSH. They
matched the reported shell PID and terminal ID to the original live panes,
verified that diagnostics did not restart a stopped source, and confirmed
that an inspected pane exposes neither an action target nor write
capabilities. The owned automation regression passed. Evidence is in
`target/herdr-identity-{local,ssh,owned}.log`. Herdr's input schema has no
expected process or terminal identity, so guarded writes remain open.

The tab menu's **Agent status / conversation** view now loads the imported
Herdr identity asynchronously. Private graphical and SSH cells fixtures
verified the read-only row matched an original shell PID and the source
processes survived. Results are in `target/herdr-menu-final-{local,ssh-cells}.log`.

## Per-machine agent view

Enabled machine profiles open a scoped agent view from their details menu.
Refresh and page navigation stay on that controller. Profile revision checks
reject navigation from a stale menu after the profile changes.

Private loopback SSH fixtures in graphical and cells modes verified that the
view excluded other controllers, paged to the ninth pane on the selected
controller, and rejected a stale profile action. See
`target/machine-agent-scope-final-{graphical,cells}.log`. Switching the same
frontend between independent controllers remains open.

Saved-machine agent rows now copy a command that focuses the selected
one-based pane in a fresh frontend. The remote launch verifies that the pane
exists before starting the view. The focus flag is a one-time launch option
and is omitted from refresh arguments. Private graphical and cells menu
fixtures verified that a real SSH cells presenter attached to the original
pane 9 process on an independent controller and rejected a missing pane.
See `target/machine-pane-launch-attached-{graphical,cells}.log`.

A local presenter can now open a saved machine's live agent pane in the same
terminal via the agent list. It checks the destination before closing the
local view, saves the local frontend state, and re-execs with a fresh remote
frontend ID. Only a registered local presenter can consume the handoff.
The private loopback fixtures verified rendering the original remote process
in the same PTY, keeping the local process and other view alive, and rejecting
a missing destination without closing the original view. The actual switch
was exercised in a cells presenter. Evidence is in
`target/machine-switch-short-{graphical,cells}.log`.

Detaching after a machine handoff returns to the original local frontend in
the same terminal. The origin command is held only by the local presenter.
An SSH launch failure after a successful destination check also restores the
origin. Loopback fixtures verified the same presenter rendered both original
processes and recovered from a deliberately rejected remote launch. See
`target/machine-switch-return-{graphical,cells}.log`. Direct remote-to-remote
switching remains open.

The remote Machines menu now offers **Return to local view** after a
same-terminal handoff. Private loopback fixtures activated the action,
verified the local view rendered again in the same PTY, and confirmed the
remote pane process survived. The actual terminal handoff was tested in
cells mode. Logs are in `target/machine-return-menu-{graphical,cells}.log`.

A pending saved-machine destination check now has an explicit Cancel action.
Dismissing the check also invalidates its eventual result. The loopback
fixture held a successful check on the destination, canceled it, then let
the response arrive. The same local controller stayed open and no remote
frontend was created. The normal handoff and return paths also passed.
See `target/machine-cancel-{graphical,cells}.log`.

Machine details now offer **Open workspace here** from a local presenter.
The destination check resolves the profile's configured workspace or its
controller's saved default without requiring an agent or a specific pane.
Private loopback fixtures verified default-workspace launch and return through
the same cells presenter. See `target/machine-workspace-{graphical,cells}.log`.

Saved-machine agent links now preserve the pane generation when the source
reports one. Both the same-window check and copied launch command reject an
obsolete generation. Startup verifies it again before queuing focus, and the
controller checks it when applying the action. Private loopback fixtures
recycled pane 9 and verified its copied command failed before opening a
frontend. They also verified a stale same-window request retained the local
view. See `target/pane-generation-{graphical,cells}.log`.

## History search pagination

The headless history search returns a continuation cursor for additional
matches. The cursor binds the project, pane generation, source, query and
captured bytes. If live output changes that capture between requests, the
next page returns `stale_history` instead of skipping or repeating matches.
Local graphical and SSH cells fixtures verified two ordered Unicode results
and rejection after new PTY output. See
`target/history-pagination-{local,ssh-cells}.log`.

History review and the headless search now support opt-in Unicode ignore-case
matching. It is still a literal search, and matches retain the original
text, byte ranges and terminal columns. Case mode is bound into pagination
cursors. The unit regression covers different-width Unicode encodings and
literal punctuation. Local graphical and SSH cells fixtures verified the
search toggle, original-case clipboard text and the headless results. See
`target/history-ignore-case-{unit,local,ssh-cells}.log`.

Imported Herdr history now checks the source socket, server process and
terminal ID on either side of the capture. A stopped source is rejected
before invoking the read command. Private Herdr fixtures passed locally and
over SSH, including case-insensitive API paging, interactive search and a
read against a stopped source that left it stopped. See
`target/herdr-history-guard-{local,ssh-cells}.log`.

The `agents.live` API now accepts project and session filters. They apply
before pagination and terminal inspection, so unrelated panes cannot consume
the requested page. A private loopback SSH fixture verified a filtered page
with the ninth pane and an empty result for an unmatched session. See
`target/live-agent-filter-ssh.log`.

Multi-machine `machines.agents` requests can forward optional project and
session filters to the independent controllers. Omitted filters leave the
previous wire request unchanged. The loopback fixture verified matching
pagination, an empty page on an unrelated controller, and an independent
unreachable-host error. See `target/machine-agent-filter-ssh.log`.

A valid live snapshot now repairs a corrupt current preview without rotating
the corrupt data over the previous-process preview. Private cold-start
fixtures deliberately corrupted both paths at different times, verified the
corrupt preview still reports a read error, then verified live repair and
byte-for-byte preservation of the previous-process preview. See
`target/cold-corrupt-repair-{local,ssh-cells}.log`.

Saved-screen deduplication now treats cursor and alternate-buffer metadata
changes as updates even when the formatted bytes are identical. The unit test
covers byte-identical records. Private local and loopback SSH cold-start
fixtures verified correction of stale alternate metadata and retention of the
previous-process preview. See `target/cold-screen-metadata-unit.log` and
`target/cold-screen-metadata-final-{local,ssh}.log`.

Saved-screen review now opens at the archived cursor. A smaller pane clamps
it to the nearest visible cell. Unit coverage verifies clamping and private
local and SSH cold-start fixtures verify the previous screen opens at its
original cursor without changing the live pane. See
`target/cold-review-cursor-{unit,local,ssh-cells}.log`.

The tmux graphics observer now ignores oversized output notifications that
identify an unrelated pane. Watched or unidentifiable oversized output still
reports a gap. Parser tests exercise both normal and extended foreign
notifications across fragmented reads. The private local and SSH composition
fixtures verified graphics decoding and retained source geometry. See
`target/graphics-foreign-output-{unit,local,ssh}.log`.

Inline graphics now has a resize regression that checks a retained image is
clipped completely when its placement is outside the smaller view and that
expansion restores the entire original RGBA plane byte for byte. Sixteen
graphics-owner tests passed. The private native broker suites also passed
locally and over SSH, including source retention and streamed-view resize
recovery. See `target/graphics-resize-exact-unit.log` and
`target/graphics-resize-broker-{local,ssh}.log`.

Native broker resize now acknowledges duplicate dimensions without publishing
a new frame revision. For a changed size, the broker validates graphics
readiness before resizing the PTY and updates parser geometry only after the
kernel accepts the resize. Private local and loopback SSH streamed fixtures
verified that duplicate requests preserve the revision and retained graphics
usage. See `target/broker-resize-idempotent-{local,ssh,unit}.log`.

Terminal history now resolves a search committed during capture from the
queued review page. An initial search that wraps to the final match shows
the wrap in the status line. The regression covers both paths. See
`target/history-queued-search-verified-unit.log`.

The plugin service now recognizes recorded hook jobs when a journal cursor
replays an event whose project was deleted or changed after delivery. The
replayed event must still match the job and plugin activation. Private
fixtures restarted the service from a cursor preceding project creation,
verified the original hook ran only once, and verified delivery of later
events. Both local and loopback SSH fixtures passed. See
`target/plugin-hook-replay-final-{local,ssh}.log`.

Plugin hook cursor updates now take the registry lock and confirm the same
activation is still enabled before publishing. This prevents an older service
scan from replacing a newly enabled activation's starting cursor. Unchanged
cursors are left on disk without atomic replacement. Private local and loopback
SSH suites passed activation guards, replay after project removal, and a check
that repeated idle scans preserve the cursor inode. See
`target/plugin-hook-cursor-{local,ssh}.log`.

The plugin service now saves the last fully queued event when a later hook
hits the queue limit. This reduces rescans and preserves delivered progress
under sustained backpressure. Private local and loopback SSH tests filled the
queue with 32 jobs, blocked both workers, and verified that the first of two
session hooks was checkpointed while the second waited. Both hooks ran once
after the queue drained. See `target/plugin-hook-prefix-{local,ssh}.log`.

The plugin service now resets its cursor only when the automation journal
reports actual expiry. Other journal-read errors retain the cursor for retry.
Private local and loopback SSH plugin fixtures verified that an invalid
future position remains intact with a diagnostic and that a genuinely
expired journal identity triggers a reported reset. See
`target/plugin-journal-errors-{local,ssh}.log`.

An open automation subscription now returns its subscription ID and last
processed cursor on journal polling or reconciliation failure. Private
session-event fixtures interrupted an open stream with invalid journal bytes,
restored the journal and resumed on the same transport using the error cursor.
Local and loopback SSH tests passed. See `target/event-stream-error-{local,ssh}.log`.

Resumed automation streams now validate their cursor by reading the first
retained batch directly. They deliver that batch without rebuilding a
workspace snapshot and reconcile live metadata on the normal polling
schedule. Private session-event tests verified a bounded replay while
metadata awaited reconciliation, locally and over loopback SSH. See
`target/event-stream-replay-{local,ssh}.log`.

The combined saved-machine agent view now links directly to a host's
next pane page using its reported continuation offset. The link keeps
the profile revision guard and opens the existing machine-scoped view.
Graphical and cells fixtures followed the new link to pane 9 through
private loopback SSH controllers. See
`target/machine-direct-page-{graphical,cells}.log`.

Machine-specific agent pages opened from the combined overview retain
their origin while paging or refreshing. The return action restores that
combined page. Private graphical and cells fixtures verified the return
path around pane 9 over loopback SSH. See
`target/machine-agent-return-{graphical,cells}.log`.

Combined agent queries now resolve saved profiles from one catalog snapshot
and report missing profiles per machine while returning intact agent pages
from other controllers. The overview also labels results whose known profile
changed or disappeared while the request was open. Both private graphical
and cells machine suites verified a mixed request over loopback SSH with
one valid controller and one missing profile. See
`target/machine-profile-isolation-{graphical,cells}.log`.

Agent health now reports why a structured observation lacks authority,
including process mismatch, archival, heartbeat expiry, clock skew or a
live backend failure overriding a current heartbeat. The pane inspector
explains the reason. Nine unit tests passed, and private local graphical
and loopback SSH cells lifecycle fixtures checked registration status
and the backend-failure explanation. See `target/agent-report-status-final-unit.log`
and `target/agent-report-status-{local,ssh-cells}.log`.

A fresh structured lifecycle report can no longer overwrite an explicit
terminal transport failure. The pane inspector identifies the transport as
the source and retains the saved report for recovery. A disconnected bridge
with a reachable terminal owner remains a separate health signal. The nine
agent-state unit tests passed, including both disconnect cases. See
`target/agent-transport-authority-unit.log`.

The pane observation API now offers an opt-in unavailable result with saved
identity and layered disconnect diagnostics. It never grants a live target or
write capabilities from a failed inspection. Private local and loopback SSH
import tests stopped their isolated Herdr servers and confirmed that the
observation left those servers stopped. See `target/pane-unavailable-{local,ssh}.log`.

Pane input capability and guarded send now share a transport readiness check.
A valid target with a disconnected input transport receives a typed
not-applied error before delivery. Transferred panes also expose the observed
bridge state. Automation unit tests, the private transferred-PTY fixture and
the loopback SSH automation suite passed. See
`target/pane-input-transport-{unit,transfer,ssh}.log`.

The transferred-PTY fixture now detaches its private presenter and stops
only the remaining pane's bridge. The original shell stays alive with the
same guarded target. The API reports input unavailable while termination
remains supported. An attempted write and its operation replay both return
`not_applied`, and the shell never receives the marker command. See
`target/pane-input-disconnected-fixture.log`.

The same private transferred-pane fixture now reopens the presenter after
a bridge disconnect. The original shell and guarded target survive. The
rejected operation remains not applied after reconnect, and a fresh operation
reaches that same shell. See `target/pane-input-reconnection-fixture.log`.

Closing a targeted split pane now removes that leaf directly, preserving
focus on a different open pane. An already hidden tab cannot replace and
then close the focused leaf. Private local graphical and loopback SSH
cells fixtures verified both cases and retained all terminal processes.
See `target/pane-targeted-close-{graphical,ssh-cells}.log`.

A pane can now split right or down with an existing hidden terminal tab.
The native menu lists eligible tabs, and the action validates both
memberships against the current layout before changing it. Private
graphical and loopback SSH cells fixtures placed a hidden tab below a
selected pane, verified all original processes survived, and restored
the prior tree by closing the added leaf. See
`target/pane-split-existing-{graphical,ssh-cells}.log`.

The keyboard action `split-existing` opens the existing-tab split picker
for the focused pane. The default is Ctrl+B, then S (lowercase). Private
graphical and loopback SSH cells suites verified opening and completing
the split with the keyboard. See `target/pane-split-shortcut-{graphical,ssh-cells}.log`.

The existing-tab split picker now binds both terminals to their observed
generations. The selection checks those generations after synchronizing
the project under the model lock. Local graphical and loopback SSH cells
fixtures verified that a mismatched generation leaves the tree intact
and the current selection still succeeds. See
`target/existing-split-epoch-{graphical,ssh-cells}.log`.

The typed frontend command `split_existing_tab` places a hidden tab
next to the focused pane. The controller checks both terminal generations
under the project lock, and frontend discovery advertises support.
Private graphical and loopback SSH cells fixtures proved stale generation
rejection, placement acknowledgement, operation replay and preservation
of the original process and the other window's layout. See
`target/api-existing-split-{graphical,ssh-cells}.log`.

The typed frontend command `close_split_pane` removes a terminal from
the addressed window's split while retaining its tab and process.
Private graphical and loopback SSH cells fixtures verified that closing
a nonfocused pane preserves focus, other windows stay independent,
stale and already hidden targets are rejected, and operation replay
returns the original receipt. See `target/api-close-split-{graphical,ssh-cells}.log`.

The typed frontend command `move_pane_to_edge` exposes the existing
outer-edge layout operation with frontend, project and pane generation
guards. Local graphical and loopback SSH cells fixtures moved a hidden
tab to the bottom, verified the original branches and process survived,
and checked stale-target rejection and idempotent receipts. See
`target/api-move-edge-{graphical,ssh-cells}.log`.

Native broker pane lookup, reattachment, and saved-owner connection now
open existing metadata directories without creating missing ones. This
keeps namespace probes safe when checking source and destination records
for future cross-project movement. The unit regression covered missing
roots, absent project subdirectories and a symlink. The private managed
broker and streaming fixtures passed locally and over loopback SSH. See
`target/native-slot-read-only-{unit,local,ssh}.log`.

The configurable `move-pane-to-edge` action now opens the edge picker
for the focused pane. Ctrl+B followed by Shift+E is the default. Private
graphical and loopback SSH cells fixtures completed bottom-edge placement
through the keyboard picker. The pane guide also records the separate
tmux servers and retained-owner handoff needed for cross-project moves.
See `target/pane-edge-shortcut-{graphical,ssh-cells}.log`.

The outer-edge picker now binds selections to the project and terminal
generation observed when it opened. Playback checks both under the project
lock. Private graphical and loopback SSH cells fixtures verified rejection
of a stale selection and success of the current one. The SSH typed frontend
command suite also passed. See `target/pane-edge-epoch-{graphical,ssh-cells,api}.log`.

The default existing-tab split shortcut is now Ctrl+B, Shift+S. The
initial lowercase binding collided with Ctrl+B, s for the session picker,
which has been restored. A key-routing regression checks both bindings.
The pane-layout and narrow session-picker suites passed in graphical
mode and loopback SSH cells mode. See `target/split-session-shortcuts-*.log`.

Attention item details now explain when the original terminal generation
is gone and omit the open action while retaining dismissal. Private
graphical and loopback SSH cells fixtures terminated only an isolated
notification source, then verified the retained item, unchanged
selection and available dismissal. See
`target/attention-stale-pane-{graphical,ssh-cells}.log`.

Attention navigation now synchronizes and checks the destination generation
under the project lock before focusing and marking the alert read.
Loopback SSH cells fixtures verified normal cross-session navigation and
rejection of a direct action targeting a terminated pane. See
`target/attention-navigation-guard-{notifications,agent-lifecycle}.log`.

Reported attention details now offer the exact saved conversation ID through
the frontend clipboard. The lookup binds the alert's original run, pane and
generation. Private graphical and loopback SSH cells fixtures copied the
same conversation after the notification source terminal was closed. See
`target/attention-conversation-copy-{graphical,ssh-cells}.log`.

Attention queries now return offset zero for empty results. When a saved
page lies beyond the available results, the inbox persists the corrected
offset so the old page does not reappear after refresh. Private graphical
and loopback SSH cells fixtures verified API empty results and restoration
from a saved offset past the last available page. See
`target/attention-page-clamp-{graphical,ssh-cells}.log`.

Page-level mark read now requires the inbox to be visible, so a queued or
direct action cannot apply to cached items after the page closes or while
a new page loads. Private graphical and loopback SSH cells fixtures
verified rejection with the inbox closed and successful marking of the
filtered page through its menu item. See
`target/attention-visible-mark-{graphical,ssh-cells}.log`.

The attention inbox now supports Left and Right for previous and next
result pages while preserving the existing item navigation keys. Private
graphical and loopback SSH cells notification suites paged both ways with
the keyboard. See `target/attention-keyboard-paging-{graphical,ssh-cells}.log`.

Worktree automation now supports guarded Git lock and unlock with an
optional lock reason and an updated reference. The host fixture verified
Unicode, removal refusal while locked, and stale-reference rejection.
Private local graphical and loopback SSH cells worktree suites exercised
the new API before reopening and removing the checkout. See
`target/worktree-lock-{local,ssh}.log`.

The worktree picker now offers lock and unlock for linked checkouts,
runs the guarded host operation in the background, and refreshes the
catalog. Private graphical and loopback SSH cells fixtures changed a
live checkout's lock, verified removal was hidden while locked, and
kept the project and its shell active. See `target/worktree-lock-ui-{local,ssh}.log`.

The worktree picker now refreshes after a failed lock change and reports
the original error. It does not retry the write. Private graphical and
loopback SSH cells tests changed the lock from another client, verified
the stale unlock preserved the newer lock, and then unlocked through the
refreshed picker. See `target/worktree-stale-lock-ui-{local,ssh}.log`.

Worktree automation now reports typed outcomes for confirmed repository
host refusals and preserves unknown outcomes for Git or transport failures.
Private graphical and loopback SSH cells fixtures verified
`stale_worktree` and `locked_worktree` responses with `not_applied` and
then unlocked and removed the original checkout normally. See
`target/worktree-error-codes-{local,ssh}.log`.

Successful worktree lock changes now return the updated catalog from
the same host request, removing an extra SSH round trip in the picker.
Failed changes still refresh independently and retain the original
diagnostic. The host fixture checked the returned catalog, and private
graphical and loopback SSH cells worktree fixtures passed. See
`target/worktree-lock-one-query-{local,ssh}.log`.

Worktree lock refreshes now reselect the same checkout by directory
identity. Private graphical and loopback SSH cells tests used Enter to
reopen that row after both a successful lock and a rejected stale action.
See `target/worktree-lock-focus-{local,ssh}.log`.

Worktree details can copy the exact checkout path and full HEAD commit.
Private local graphical and loopback SSH cells fixtures checked exact
clipboard values including the Unicode path. See
`target/worktree-copy-details-{local,ssh}.log`.

The release gate now includes opt-in SSH notification forwarding and
local/SSH saved-session event replay, including resumption from a journal
read error. All three private fixtures passed independently. The parity
checklist now reflects the implemented combined machine view and opt-in
forwarding alongside their remaining acceptance gaps. See
`target/parity-release-{forward,event-local,event-ssh}.log`.

The release script can now gate on the real offline provider fixtures
when the Codex or Claude Code executable is supplied. Both probes passed
against the working candidate. Codex resumed and registered the exact ID
in cells over loopback SSH. Claude Code registered the same ID through
its callback in a new STAR/FLUX cells pane in print mode. See
`target/provider-gate-{codex,claude}.log`.

The provider hook now rejects a delayed Codex startup callback that
would replace a different conversation registered by the same process.
Malformed event and ID shapes return a bounded hook diagnostic. The
real offline Codex SSH cells fixture verified an injected late startup
did not change the stored conversation before the exact resume.
The Claude Code print-mode callback fixture also passed. See
`target/provider-late-startup-{codex,claude}.log`.

Provider registration now requests status only for its bound pane.
The CLI's scoped observation leaves project-wide status available
and filters saved records to the requested pane. The offline Codex SSH
cells fixture verified scoping after starting a second pane and an empty
response for an absent pane. The Claude Code print-mode probe also passed.
See `target/agent-pane-scoped-{codex,claude}.log`.

The provider startup integration is now available as
`starflux --agent-session-hook`. The executable includes the adapter and
uses its own path for scoped status and report calls. Private offline
fixtures verified direct callbacks through this entry point for Claude
Code print mode and Codex cells over loopback SSH. The Codex fixture now
waits for the working report after the callback's initial idle report.
See `target/embedded-agent-hook-{codex,claude}.log`.

The embedded provider hook now reads Linux process ancestry without
decoding executable names, and rejects vanished, malformed or zombie
process records. Focused checks exercised those cases. The private
offline Codex SSH cells and Claude print-mode recovery probes passed.
See `target/provider-ancestry-{codex,claude}.log`.

Provider hook setup now has a read-only JSON generator built into the
STAR/FLUX executable. The default generated command resolves the public
launcher from PATH, and an explicit launcher path can be provided. CLI
checks round-tripped spaces, quotes and literal shell metacharacters
for both providers and rejected a missing session placeholder.
Offline Codex SSH and Claude print-mode recovery probes passed after
the change. See `target/provider-config-fragment-{codex,claude}.log`.

Real offline provider fixtures now consume the generated SessionStart
configuration. Claude Code loads the generated group in its disposable
JSON settings. The Codex fixture invokes the generated command through its
private managed callback recorder. Both registered the resumed conversation
against the new pane. See `target/provider-generated-config-{codex,claude}.log`.

The current parity candidate passed workspace tests with all features
and with default features disabled, using the development profile.
Strict all-target workspace Clippy also passed after updating the
retained-image regression to check complete four-byte RGBA pixels.
This is Rust gate evidence rather than a fresh full release run.
See `target/parity-workspace-{all-features,minimal,lint-all-targets}.log`.

Native graphics now follows the terminal parser's explicit alternate
grid clear event. The retained alternate grid selected by DEC mode 47
keeps image placements when switching away and back, and 1049 still
clears only the alternate grid. Seventeen graphics-owner tests and
private broker streaming tests locally and over loopback SSH passed.
See `target/graphics-retained-alternate-{unit,local,ssh}.log`.

The broker streaming fixture now opens a retained alternate grid, emits
an image, returns to the main grid and reenters the alternate grid. It
checks that the main grid has no alternate image and that the restored
stream contains the same text and image-derived cells. Both private
local and loopback SSH runs passed. See
`target/graphics-alternate-stream-{local,ssh}.log`.

Native terminal mouse reports now honor the requested UTF-8 encoding
and use the same encoder for pointer events and synthetic release.
The terminal tests verify extended coordinates and both legacy and SGR
release bytes. See `target/terminal-mouse-encoding-unit.log`.

The guarded launch-receipt build passed the private cold-start suites
in local graphical and loopback SSH cells presentations using the default
backend. An additional run with `--native-sources` stopped at the fixture's
initial source-binding check because this candidate's workspace did not
select the retained native broker. That run provides no evidence for native
cold recovery. See `target/cold-receipt-integration-{graphical,ssh-cells,native}.log`.

## Native broker integration checkpoint

The verified native broker changes were merged into an isolated checkout at
`target/native-promotion-iyk4jraj`. The merge inputs and three conflict
resolutions are tracked in its `merge-audit.json` and source tree.
`target/native-promotion-check.log` records one remaining compiler error.
The native branch of the terminal startup worker in `src/ui.rs` reads
`command` before the worker loads the pending command from durable state.

Resume by reconciling that startup branch with the native broker's
launch-intent transaction and the current pending-command lookup. Then
build an isolated executable and run the private native cold-start suite
locally in graphical mode and over loopback SSH in cells mode before
promoting the merge. The installed executable and active sessions were
left untouched.
