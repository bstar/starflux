# Plugin workflows

Open **Plugins** from the workspace menu or press **Ctrl+B, Shift+U** in graphical
or cells mode. Choose **Link local plugin directory…**, enter the directory on
the workspace controller, and open the linked plugin. Review its description,
working directories, deadlines, literal arguments and declared hooks before
selecting **Trust these commands and enable**. Linking alone runs nothing.

The command review wraps complete argument arrays. Action results have separate
stdout/stderr, visible control characters, previous/next output pages, refresh
and cancellation controls. Arrow keys, Home/End, PageUp/PageDown and Enter work
in these views. Cancelling a job stops its worker process group, including normal
child processes. Unlinking disables future work and retains the package and job
logs. Disabling does not automatically cancel a job that already started; cancel
it from its job view when needed.

Plugins are trusted executable user code, with the normal user environment and
permissions. They are not sandboxed. For linked directories, approval pins the
manifest bytes; changing a linked script does not require reapproval unless the
manifest changes. Managed packages additionally pin the complete installed tree.
A changed manifest or managed file disables effective execution. A damaged
package can still be disabled, updated or uninstalled from its native menu. A plugin can deliberately escape a process group; cancellation is not
a sandbox guarantee. STAR/FLUX never turns a plugin action into terminal typing.

## Example

The repository contains [Project summary](../examples/plugins/project-summary/starflux-plugin.json),
a small Git-status action with no hooks. Link the absolute path to
`examples/plugins/project-summary` from the Plugins menu, enable it, and run
**Summarize repository changes** from a local project. The package's executable
`summary.py` reads its context from stdin and prints the project's Git status.

## Managed Git packages

Choose **Install from GitHub / Git repository…**. Enter `owner/repository`, an
HTTPS/SSH Git URL, or an absolute local Git repository, then a tag, branch or
commit and an optional package subdirectory. Blank version means `HEAD`; blank
subdirectory or `.` means the repository root. Installation fetches and archives
the selected commit on the workspace controller, records its exact commit ID,
and registers a **disabled** private snapshot. Review its commands and select
**Trust these commands and enable** before any package action or hook runs.

The package menu offers **Fetch an updated / different version…**, **Installed
versions / roll back**, and **Uninstall managed files / keep logs and settings…**.
Every update and version switch disables the plugin and clears approval, even
when only scripts or assets changed. Installed versions are retained, so rollback
works without the source repository or a network connection. Existing running
jobs keep their original files. Old queued activations are rejected; commands
are never silently resubmitted against a new version.

Uninstall requires the native confirmation and refuses while *any* version of
that plugin has queued/running jobs. Finish or cancel those jobs first. It removes
only that managed plugin's version directory. The original Git repository,
plugin settings, state and job logs remain. A linked directory instead offers
**Unlink / keep package and logs**. `plugins.unlink` can also unregister managed
metadata while retaining files; use `plugins.uninstall` to remove owned versions.

Managed staging currently needs a **Linux workspace controller**. A macOS/other
SSH client can use a Linux controller. Git and Python 3 must be available there.
Git uses the controller's existing credentials/SSH agent and configuration, with
interactive prompts disabled and SSH host-key verification required. Git URLs
must not contain passwords/tokens or query parameters. Authentication files are
not copied into the package. GitHub `owner/repository` expands to an HTTPS URL.
SSH and HTTPS Git sources are supported; command transports such as `ext::` are
rejected. A local `file://` URL is normalized to an absolute repository path.

Fetching does not execute checkout hooks, package entrypoints, install/build
scripts or content filters. It does not initialize submodules or resolve Git LFS
pointer files. Git archive/export rules apply. Executable bits are preserved;
regular files, directories and symlinks contained within the package are accepted.
Escaping/absolute symlinks, traversal, hardlinks and special files are rejected.
A failed fetch, bad manifest, unsafe tree, identity mismatch or stale registry
revision leaves the installed version unchanged.

Two source fetches may run concurrently. Each is limited to 60 seconds, 256 MiB
of staging data, 64 MiB/8192 package entries and bounded diagnostics. An API client
should set `deadline_ms: 90000` for installation/update; the ordinary shorter API
deadline can end observation before the operation finishes. Escape hides the UI
view while the authorized operation continues. Closing its controller or losing
a worker can leave an indeterminate operation; inspect `plugins.list` and
`plugins.versions` before issuing a new operation. The same operation ID is never
replayed as a new fetch after a lost result. Watchdogs stop owned Git/SSH children
when their worker dies. Later operations clean verified abandoned staging while
preserving live owners, retained versions and unowned source directories. A
removed/replaced namespace is not recreated by an old fetch.

Managed approval covers file names, executable modes, symlink targets, scripts
and assets, as well as the manifest. Integrity is checked before enable/enqueue
and worker execution, with a metadata-checked cache. Idle hook scans do not
continually rehash whole repositories. This detects changed snapshots; it is not
isolation against trusted plugin code running with the user's full permissions.
Configuration/state belong in the provided external directories, rather than
inside a managed snapshot. Retained versions consume disk until explicitly
uninstalled; automatic pruning is not implemented.

## Version-1 manifest

A package contains `starflux-plugin.json`:

```json
{
  "version": 1,
  "id": "example.workflow",
  "name": "Example workflow",
  "description": "A finite project action",
  "actions": [
    {
      "id": "check",
      "title": "Check current project",
      "argv": ["package:check", "literal argument"],
      "cwd": "project",
      "timeout_ms": 30000
    }
  ],
  "hooks": []
}
```

This is STAR/FLUX's native JSON ABI, not Herdr's TOML manifest. Plugin IDs permit
letters, digits, dots, underscores and hyphens, up to 64 bytes. Action/hook IDs
use the same set without dots. Unknown manifest fields are rejected. A manifest
is limited to 64 KiB, 32 actions and 32 hooks. An action has 1–64 literal argv
entries totalling at most 16 KiB. No implicit shell expansion occurs: `$()`, spaces
and glob characters are ordinary argument text. Use an explicit shell executable
only when shell execution is intended. A `package:` prefix on the executable
resolves it relative to the linked directory; other executable names use normal
process resolution.

`cwd` is `package` (default) or `project`. A project action requires a current
project with a directory on this controller. Project identity and worktree
availability are checked at enqueue and execution; removal leases protect active
worktree jobs. Package actions can explicitly invoke SSH, but automatic staging
or execution on another machine is not implemented. With an SSH frontend, plugins
run on the remote workspace controller. Package and log paths refer to that host.

The timeout defaults to 30,000 ms and accepts 100–3,600,000 ms. The runner permits
two active workers and up to 32 queued jobs. Queued jobs expire after 30 seconds
or a host reboot. Output is bounded to 512 KiB per stdout/stderr stream. An
output-limit outcome is explicit. Completed jobs retain status, error, exit code
and logs; lost workers are indeterminate and their command is never automatically
repeated. A queued job is rejected if its approval, activation or project context
changes before execution.

## Persistent terminal panes (in progress)

Native `panes` entrypoints declare `id`, `title`, literal `argv`, `cwd` (`package`
or `project`) and `placement` (`overlay`, `split`, `tab`, `zoomed`, or `popup`).
Titles use the project tab label limit of 64 characters. A pane-only manifest may
have an empty `actions` array. Full command review includes pane argv before a
plugin can be enabled. No implicit shell expansion or finite-job timeout applies
to a persistent terminal. Standard input/output belong to its PTY.

The normal terminal backend, view-local tab/split/zoom/overlay placement, native
menu controls and `plugins.panes.list/open/close` API have focused local/SSH
graphical/cells acceptance coverage. Popup presentation is a separate transient
surface, described below; it never allocates a project terminal.
For normal placements, without a `frontend` target the API creates a backend and returns
`presentation: "not_requested"`. With the complete target from `frontends.list`,
`plugins.panes.open` requests placement in that window and returns its durable
presentation receipt. Optional `placement` overrides the manifest default before
allocation. An explicit normal placement can override a popup default; it does
not turn that terminal into a popup. `plugins.panes.focus` places an existing
owned terminal without rerunning its command. Frontends advertise
`plugin_pane_commands`; older controllers require refresh before these commands
can be submitted.

### Transient popup terminals

An enabled pane entrypoint with `placement: "popup"` opens a native terminal over
the requesting window. Optional `width` and `height` are cell counts or strings
such as `"80%"`; percentages must be 1%–100%. They measure the outer box within
the current content area, including its title and border. Defaults are half the
area. Dimensions clamp to the viewport, with a minimum of 20×8 where space allows
and a maximum terminal grid of 512×192. Narrow viewports use the full screen as
the available area. Resizing updates both the grid and the PTY dimensions.

Choose the reviewed popup entrypoint in the plugin menu, or call
`plugins.panes.open` with a complete `frontend` target. Optional `placement:
"popup"`, `width` and `height` override the declaration for that invocation.
Opening without a frontend returns `frontend_required`. Older frontends must
refresh before they advertise `plugin_popup_commands`. The response has
`kind: "popup"`, a `popup.id`, and a separate durable `presentation` receipt.
`acknowledgement: "pty_started"` acknowledges PTY creation; command validation
and startup then run in its child. Inspect the nested receipt and
`plugins.popups.get {id}` for availability or launch failure. Operation replay
returns the same receipt rather than opening another popup.

A saved session permits one popup across all its windows. Existing dialogs,
menus, history review or panel navigation refuse an open request. Other windows
keep their focus, layout and input. The popup gets literal key bytes, including
Escape, Ctrl+B and ordinary function keys; these do not trigger STAR/FLUX
navigation. Its configurable popup-only shortcut defaults to **Shift+F6** and
focuses the close control: Enter closes the popup, Escape returns to program
input. Keys and paste do not reach the program while that control has focus.
Mouse reporting and paste otherwise use the popup PTY. Close it with its header ×, let its
program exit, or call `plugins.popups.close {id, frontend}`. A mismatched ID never
closes the current popup. Commands that change the underlying view are refused
until it closes. Close restores the previous terminal/App Space input state.

There is no project pane ID, agent registration, saved split/tab membership,
screen archive or automatic restart. The context contains `plugin_popup_id` and
`focused_pane: {pane, epoch}` for the underlying terminal; `STARFLUX_PANE` and
inherited pane/job identity variables are removed. `STARFLUX_PLUGIN_POPUP_ID`
identifies the transient surface. Config, state, controller command path and
project context use the same plugin directories and host affinity as normal
entrypoints. Popups run on the workspace controller; `cwd: "project"` requires
a local project directory.

The owning controller supervises a dedicated PTY process group. Explicit close,
program exit, view shutdown and controller death end that group, including
ordinary background children. Unexpected supervisor exit is cleaned before its
PID is reaped. Approval/source/activation and the actual declared command are
checked again before execution. This is trusted plugin code, not a sandbox:
code that deliberately leaves its process group needs its own lifetime policy.
Running popups also prevent package uninstall. App Space workers retain their
independent lifetime and continue streaming behind the popup.

A stale, already-busy or wrong-project target is rejected before creation. The
controller checks again before placement. Creation and presentation are distinct:
if the target changes after launch, the backend remains available and the result
still includes its ownership record. Inspect the nested presentation `status` and
`error`, not just the outer `ok`. If submission/acknowledgement cannot be confirmed,
`presentation.status` is `unconfirmed` and includes the operation ID and error;
query `frontends.command_status` and current window state before another focus
request. Replaying an open operation never starts a second terminal.

Ownership records retain the source version, approval/activation, project, slot,
epoch, boot and process identity. A durable launch receipt is consumed before
executing the plugin. Reopening a view does not restart the program or substitute
a shell. Explicit dead-terminal recovery can replace it with an ordinary shell.
A managed update retains the running source version; uninstall refuses while an
owned terminal might still be using package files. There is no automatic restart
policy. A Linux controller whose binary has been replaced can use the replacement
at the same pathname as its launcher. If that pathname is gone entirely, opening
fails before allocating a terminal and asks for a frontend refresh. Focused tests exercise these invariants, including old-boot close rejection,
managed updates and one-shot launch receipts. Broader lifecycle/cross-feature
acceptance and complete plugin-pane parity remain unfinished.

Pane processes receive `STARFLUX_PLUGIN_ENTRYPOINT_ID`,
`STARFLUX_PLUGIN_PANE_ID`, `STARFLUX_PROJECT`, `STARFLUX_PANE` and the existing
plugin config/state/binary/context environment. The context includes the project,
terminal slot/epoch and plugin-pane identity. Its standard input remains the
interactive PTY rather than the finite job JSON input stream.

## Declared link handlers

Choose **Open link with a plugin…** in the Plugins menu and paste an absolute
URL. STAR/FLUX lists matching enabled handlers; choose one to run it. Discovery
does not execute code, choose a default or open a browser. Multiple matches stay
separate choices. This workflow works with keyboard or mouse in graphical and
cells presentations, including an SSH frontend. The selected action runs on the
workspace controller, using the same job limits, logs, cancellation and approval
guards as other plugin commands.

A manifest can declare up to 32 `link_handlers`, each referring to an existing
action:

```json
"link_handlers": [
  {
    "id": "issue",
    "title": "Show issue",
    "action": "check",
    "schemes": ["https"],
    "hosts": ["issues.example.com"]
  }
]
```

Schemes match without case sensitivity. Optional `hosts` are exact DNS names
(ASCII/punycode) or IP addresses, without ports; omitted/empty hosts accepts any
host for the declared schemes, including opaque custom-scheme URLs. Ports do not
affect host matching. There are no host wildcards or substring matches. A handler
supports at most 16 schemes and 32 hosts. URLs are limited to 8192 bytes, require
a scheme, and reject embedded credentials, raw whitespace and control characters.
Encode spaces. The original URL, including its query and fragment, is delivered
unchanged in `context.event` as
`{"kind":"link.opened","handler":"issue","url":"…"}`. It is never interpolated
into argv or a shell command. Project-directory handlers require a project on
the controller. Approval and matching are checked again when a handler is chosen;
queued jobs cannot bypass subsequent disablement or a package change.

For URL text visible in a native terminal, **Ctrl+click** opens the handler
chooser. **Right-click** offers handler review, copy URL and terminal options.
**Ctrl+B, L** opens a keyboard picker for URLs in the focused terminal; it is
also available in the Plugins menu. Normal left-click/drag retains terminal
selection and application input. Pointer feedback uses local host regions and
does not query plugins or wait for SSH on hover. Choosing a URL never executes a
handler until it is explicitly selected in the review view.

Detection reads the displayed grid, including ordinary wrapped URL text,
wide/combining characters, scrollback and frozen history. It is bounded to 512
visible links per terminal and 8192 bytes per logical wrapped line. Longer
logical lines and incomplete URLs cut off at the viewport edge are skipped.
Use the manual URL prompt/API for those cases. Regions are cached when text, wrapping or link metadata changes; pointer motion does not rescan text. Menus, dialogs and App
Space suppress terminal link pointer regions. The legacy Kitty pixel renderer
does not expose these native-grid controls.

OSC 8 destinations are retained independently of their visible labels. The same
pointer, copy, keyboard picker and handler chooser controls use the actual
hidden destination, even if the label looks like a different URL. Links follow
painting, erasure, scrolling, resizing, alternate screens and frozen history;
saved-screen previews preserve destinations and parameters (including `id`).
Link-only repainting invalidates the region cache without resending unchanged
text. Adjacent pointer rectangles are merged without expanding clickable areas.

The parser accepts BEL and ESC-backslash OSC terminators, raw URI semicolons and
UTF-8. Destinations are limited to 8192 bytes, parameters to 1024 bytes, individual
OSC bodies to 128 KiB and shared retained target allocations to 8 MiB. Invalid,
truncated or oversized destinations cannot become partial clickable URLs. These
parser bounds do not bypass backend limits: for example, tmux can discard long
OSC 8 destinations before Starflux receives them. Native and imported tmux
attachments advertise hyperlink support per client; history capture retains
escape attributes. Saved-grid formatting stops at 1 MiB before encoding, and
host capture is capped at 2 MiB. Oversized captures report an error rather than
replacing a valid archive.

C1 OSC encodings and browser handoff remain unfinished. The controls above
review/copy destinations and route explicitly selected approved plugin handlers;
they never automatically open a browser or execute terminal-provided commands.

## Action context

The action receives one version-1 JSON object on stdin and the same JSON in
`STARFLUX_PLUGIN_CONTEXT_JSON`. Fields are `version`, `plugin_id`, `action_id`,
`job_id`, `project`, `event`, `config_dir`, `state_dir`, and `bin_path`.
`project` is null or `{id, host, directory, session}`. It does not contain terminal
history, broker keys or agent authentication. The action can still access user
files itself because it runs as trusted code.

Environment variables also expose `STARFLUX_PLUGIN_ID`,
`STARFLUX_PLUGIN_ACTION_ID`, `STARFLUX_PLUGIN_JOB_ID`,
`STARFLUX_PLUGIN_CONFIG_DIR`, `STARFLUX_PLUGIN_STATE_DIR` and `STARFLUX_BIN_PATH`.
The config/state directories are private and persistent; `STARFLUX_BIN_PATH`
identifies the controller executable. A plugin can call the structured automation
API explicitly. Mutations must use a new UUID operation ID, and the plugin must
inspect the returned outcome before deciding whether to issue another operation.

## Hooks

A hook declares `id`, `on`, `action` and optional `states`. Supported events are
`startup`, `project.created/changed/removed`, `pane.created/changed/removed`,
`agent.reported.created/changed/removed`, and `attention.created/changed/removed`.
State filters accept `unknown`, `idle`, `working`, `needs_input`,
`waiting_for_approval`, `completed`, `failed`, `stopped`, `backend_unavailable`.

For example, a hook can select `on: "agent.reported.changed"`,
`action: "check"`, `states: ["completed"]`. Enabling starts from the current event
cursor and does not replay older events. Startup is once per activation within a service incarnation; restarting the
service creates a new startup event.
A detached service observes events even when no frontend or API client remains.
Hook jobs have deterministic IDs for duplicate protection. Retention gaps are
reported rather than silently interpreted as a complete event history. Events
are observations; a plugin must query and validate current identity before
acting on a project that may have changed since the event.

## Automation

These methods use the standard [JSONL API](automation.md). Mutations require an
operation ID. Registry edits require the revision returned by `plugins.list`.
Enabling/running also requires the reviewed manifest digest.

| Method | Parameters |
| --- | --- |
| `plugins.list` | None; registry revision, manifests/digests, configured/effective enabled state and optional managed source metadata |
| `plugins.install` | `repository`, `expected_revision`, optional `reference` / `subdirectory`; fetch pinned source, initially disabled |
| `plugins.update` | `plugin`, `expected_revision`, optional `reference`; defaults to the saved ref, clears approval |
| `plugins.versions` | `plugin`; retained version source metadata, `release` IDs, current marker and registry revision |
| `plugins.activate` | `plugin`, `release`, `expected_revision`; choose a retained version offline, clears approval |
| `plugins.uninstall` | `plugin`, `expected_revision`; reject busy packages, remove only owned managed versions |
| `plugins.link` | `path`, `expected_revision`; initially disabled |
| `plugins.unlink` | `plugin`, `expected_revision`; retains source and logs |
| `plugins.configure` | `plugin`, `expected_revision`, `enabled`, `expected_digest` |
| `plugins.panes.list` | Optional `plugin`; owned terminal records, process liveness and status |
| `plugins.panes.open` | `plugin`, `entrypoint`, `expected_digest`, `project`, optional `frontend` target / `placement`; return backend ownership plus separate presentation result |
| `plugins.panes.focus` | `id`, `expected_epoch`, `frontend` target, optional `placement`; acknowledged placement of an existing owned terminal |
| `plugins.panes.close` | `id`, `expected_epoch`; stop only the matching owned terminal; repeated closed-record requests are harmless |
| `plugins.run` | `plugin`, `action`, `expected_digest`, optional `project` |
| `plugins.resolve_link` | `url`, optional `offset` / `limit` (default 64, maximum 128); read-only matching enabled handlers, digests, source warnings, total and next offset |
| `plugins.open_link` | `plugin`, `handler`, `url`, `expected_digest`, optional `project`; enqueue the explicitly chosen handler |
| `plugins.jobs` | Optional `plugin`; most recent 100 records |
| `plugins.job` | `job_id`; current status and complete bounded output (8-MiB response ceiling includes JSON escaping) |
| `plugins.cancel` | `job_id`; queued/running jobs only |
| `plugins.service` | None; detached service identity and status |

The registry is under `$XDG_CONFIG_HOME/starflux/plugins.json`; jobs, output and
service metadata live under `$XDG_DATA_HOME/starflux/plugins`. Managed snapshots
and quarantine are under `$XDG_DATA_HOME/starflux/plugin-packages`. Managed source
metadata records `repository`, `reference`, `commit`, `subdirectory`, `tree_digest`
and `release`. Corrupt records are
not permission to repeat an action. An operation replay returns its original API
result; query `plugins.job` for the current job state.

## Remaining parity work

Local linking, pinned managed Git/GitHub packages and finite command jobs are
implemented. Declared link handlers have native menu/API routing and activation
from visible native terminal URL text and OSC 8 metadata. Normal persistent plugin
terminals and frontend-targeted placement are implemented. Transient popup
terminals have focused native UI, actual cells-client and App Space acceptance;
broader lifecycle and actual graphical-host capture remain under audit. Browser handoff and
broader hook semantics remain unfinished. The native ABI should not be presented
as compatible with every Herdr plugin. The queue/worker bounds and hook backpressure have focused concurrent-publisher
coverage. Multi-plugin scheduling, source/lifecycle failure coverage and broader
service-restart semantics remain under audit.
