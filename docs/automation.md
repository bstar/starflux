# Domain automation API

STAR/FLUX serves version 1 JSON Lines requests without opening a presentation:

```sh
starflux --api '{"version":1,"id":1,"method":"capabilities"}'
starflux --api-stdio
starflux --ssh HOST --api-stdio
```

`--api` executes one request. `--api-stdio` reads one JSON object per line and flushes one response per request. `--ssh-config FILE` and `--remote-executable PATH` apply to the SSH form. An existing SSH account and STAR/FLUX installation are sufficient; no network API listener or additional authentication service is started. Scripts can also run `ssh HOST starflux --api-stdio`.

Requests always name their project/target in `params`. They do not use the UI's current selection or implicitly create a demo workspace. API calls can run while all frontends are detached. Each connection processes requests sequentially; use a separate connection for a subscription or a concurrent wait.

```json
{"version":1,"id":"read-1","method":"panes.get","params":{"project":"PROJECT","pane":1}}
```

Responses contain `version`, the supplied `id`, `ok`, and either `result` or `error`. Pane numbers throughout the API are **one based**. `id` can be a string up to 128 bytes, a 64-bit integer, or explicit null. Unknown fields and methods are rejected. Protocol errors are JSON responses; process exit status concerns the transport. Check `ok` when deciding whether an operation succeeded.

| Method | Parameters / result |
| --- | --- |
| `capabilities` | Empty params; version, methods, limits and event capabilities |
| `sessions.list` | Empty params; saved sessions and default status |
| `sessions.create` | `name`, local `directory`, optional `theme`; creates an owned session and initial shell |
| `sessions.rename` | `session`, `expected_name`, and `name`. Updates the saved label while retaining the session identity, default selection, and live processes. |
| `sessions.delete` | `session`, complete `expected_projects` from `projects.list`, a surviving `fallback_project`, and `confirm: "DELETE"`. Closes the target session’s views, stops its owned programs, and removes the saved session. Imported source processes are retained. |
| `sessions.external` | Empty params; discover running tmux/agent-multiplexer sources with guarded import references and per-source warnings |
| `sessions.import` | `reference` from discovery; register the existing source without opening a frontend or restarting terminals |
| `sessions.default` | Explicit `session` ID, or null to clear |
| `projects.list` | Empty params; project/session IDs, labels, host and directory |
| `panes.list` | `project`; terminal membership, epochs and labels |
| `panes.get` | `project`, `pane`, optional `allow_unavailable`; layered health, guarded target and capabilities |
| `panes.search_history` | `project`, `pane`, literal `query` (1–128 printable characters), optional `limit` (1–100, default 30), `ignore_case` (default false), and `cursor`. Returns the total, bounded excerpts and `next_cursor` in capture order. Pass the cursor back with the same project, pane, query and case mode to read more. If the captured history changed, the API returns `stale_history`. Rows and Unicode character columns are one-based within that capture. ANSI styles and hyperlink metadata are excluded. |
| `panes.capture` | `project`, `pane`; health plus bounded retained backend text |
| `panes.saved_screen` | `project`, `pane`, optional `previous`; historical read-only screen and parsed text |
| `snapshots.status` | Empty params; collector liveness, process identity and capture diagnostics |
| `panes.create` | `project`, optional `label`; creates and starts an owned shell |
| `panes.rename` | `target`, `label` |
| `panes.send` | `target`, `text`, optional `submit` (default false) |
| `panes.terminate` | `target`, `confirm: "TERMINATE"`; ends that terminal in all views |
| `panes.wait` | `target`, condition, optional timeout; see below |
| `agents.live` | Optional `project`, `session`, `offset` and `limit` (1–16). Filters are applied before counting panes or checking agent processes. Returns foreground observations and per-pane errors. |
| `agents.report` | `project`, `pane`, structured `report`; same validation as the lifecycle CLI |
| `attention.list` | Optional `project`, `include_dismissed` (default false) |
| `attention.forward` | Launch `workspace` and registered `frontend`. Returns unread attention for the visible workspace, remote sampling time, and the frontend’s verified focus when available. |
| `attention.query` | Optional `search`, `read` (`unread`/`read`/`all`), `state`, `project`, `session`, `source` (`all`/`reported`/`screen`), `offset`, `limit` (1–250); returns matching total, effective offset, items and warnings |
| `attention.mark_read` | Explicit `items: [{project, event}]`; per-project guarded batches with updated count and errors |
| `notifications.settings` | Settings revision and desktop capability |
| `notifications.configure` | `settings` object and `expected_revision`; see [notification delivery and retention](attention.md) |
| `notifications.prune` | Empty params; apply current retention policy, reporting per-project removals/errors |
| `notifications.delivery` | Local capability and up to 200 recent desktop attempt outcomes |
| `attention.dismiss` | `project`, attention `event` ID |
| `frontends.list` | Optional `project`; live presentation identities, guarded targets, presentation/capabilities and sampled busy state |
| `frontends.command` | `target`, typed `command`, optional `ttl_ms` / `wait_ms`; durable controller acknowledgement |
| `frontends.command_status` | `command_id` (the command's UUID operation ID); current durable receipt |
| `frontends.cancel_command` | `command_id`; cancel a command that is still queued; requires its own new UUID operation ID |
| `machines.list` | Read: List saved OpenSSH connection profiles. |
| `machines.save` | Write: Create a profile or update with `id` / `expected_revision`. |
| `machines.remove` | Write: Remove metadata using `machine` / `expected_revision`; retain processes. |
| `machines.agents` | `machines` (1–8 saved profile IDs), optional `project`, `session`, `offset` and `limit`. Each controller applies the same filter and returns its own live agent page or error. |
| `machines.test` | Read: Bounded SSH/Python/tmux/workspace API diagnostics for `machine`. |
| `machines.project` | Write: Register a project on `machine` with `project`, `directory`, optional `session` / `theme`. |
| `plugins.list` / `.install` / `.update` / `.versions` / `.activate` / `.uninstall` / `.link` / `.unlink` / `.configure` / `.run` / `.resolve_link` / `.open_link` / `.jobs` / `.job` / `.cancel` / `.service` | [Plugin workflow ABI and parameters](plugins.md) |
| `plugins.popups.get` / `.close` | Transient popup `id`; close also requires a complete `frontend` target; see [popup terminals](plugins.md#transient-popup-terminals) |
| `worktrees.list` / `.create` / `.open` / `.remove` | [Repository worktree workflows](worktrees.md), including host affinity and removal guards |
| `operations.get` | UUID `operation_id`; recorded result or indeterminate/unknown status |
| `events.subscribe` | Optional project/resource filters and replay cursor; see below |

`panes.saved_screen` reads a historical native-grid preview using `project`, one-based `pane` and optional `previous`. It returns text, screen metadata and explicit historical/read-only flags; it cannot authorize input. See [cold-start recovery](cold-start.md).

To import an existing session, call `sessions.external` on the source host, then pass an entire returned `reference` object to `sessions.import` with a new UUID `operation_id`. Discovery does not import anything. References identify a specific source session rather than a picker row. They bind the source's host boot, owned socket identity and native topology; References also bind server PID/start time; the agent multiplexer verifies the peer credentials of its Unix socket, while tmux additionally binds session creation. `stale_source` means the session was renamed, its membership changed or its server was replaced: discover it again before retrying with a new operation ID. `source_unavailable` means it is no longer discoverable or cannot be read. Replay of an already completed operation returns its original result without re-running import, including when the source has since changed.

Imports serialize metadata creation, so concurrent operations for the same source reuse the same STAR/FLUX copy. The result includes `session`, selected `project`, all imported `projects` and `source_processes_preserved: true`. The agent multiplexer imports retain all source workspaces and terminal references, including named sessions. A missing saved layout does not block a live import: the native snapshot supplies membership and selection, and saved layout metadata enriches the project directory when available. Corrupt saved layout files still report an error. The source still owns its processes, socket and topology; import does not attach a renderer, send agent input, change source options or launch an app. Source applications can continue to change while import runs; the reference is checked immediately before metadata registration and does not freeze the external session. Saved native membership, not terminal output or current activity, determines its fingerprint. Missing or failed individual sources are omitted or reported as warnings. Each discovery query is bounded to four seconds and 2 MiB; the overall request deadline also applies. A separate watchdog owns each read-only CLI query group and removes its helper children on completion, timeout or caller loss. Cancelling an API request therefore also ends its in-flight discovery helpers without signalling the source servers. On Linux, a running view can continue to discover sources after its loaded executable has been replaced on disk; its helper uses the loaded image until the view is refreshed. The agent multiplexer reads require an existing owned live socket and do not intentionally start a missing source server.

## Targets, delivery and retries

All writes require a fresh UUID **`operation_id`** in the request envelope. Keep that UUID when retrying the same operation. The controller durably records the request fingerprint and result; a retry returns the original response with `replayed: true`, including when the request ID differs. Reusing an operation UUID with different method/parameters returns `operation_id_conflict`. Changing only the transport deadline is allowed. Records are private files under `$XDG_DATA_HOME/starflux/automation/operations/` and do not automatically expire.

`panes.get` includes `saved_conversation` when the pane has a registered conversation reference. It contains the conversation and run IDs, harness, report source, original report epoch and whether a resume command was registered. This metadata remains available after explicit recovery and is separate from the live write target.

With `allow_unavailable: true`, a failed backend inspection returns the saved pane identity, disconnected health, and `status.error`. The target is null and input and termination are disabled. This option is for diagnostics and recovery discovery. The default still returns the inspection error.

An otherwise valid target can temporarily lose input capability. `panes.send` checks the current transport again and returns `input_transport_unavailable` before delivery if it is disconnected. Transferred panes expose `status.bridge_connected` when their bridge was inspected.

Use the complete `target` from a fresh `panes.get` response for pane writes. It contains project, pane, epoch, owner PID/start ticks, host boot UUID (`boot_id`), foreground agent PID/start ticks and registered run ID. Keep explicit null fields. Omitting an epoch or agent identity field is rejected. A changed generation, process or registered run produces `stale_target`; it never redirects the write to a newly reused pane. Do not construct a target from a pane number alone.

```json
{
  "version": 1,
  "id": "send-1",
  "operation_id": "a4d52db7-8fa5-4e28-9c5e-dfc5db715242",
  "method": "panes.send",
  "params": {
    "target": {"project":"PROJECT","pane":1,"epoch":null,"pid":123,"start_ticks":456,"boot_id":"7a307521-f2e5-4647-8d7e-b1330366037e","agent_pid":null,"agent_start_ticks":null,"run_id":null},
    "text": "Review the changes",
    "submit": false
  }
}
```

The example target is illustrative; copy actual identities from `panes.get`. Input reaches the named backend independently of STAR/FLUX frontend focus, selection or copy mode. The host verifies its boot identity, the owner and foreground process again immediately before delivery. Targets saved before a reboot cannot authorize input or termination, even if process numbers and start ticks repeat. Targets from older releases without `boot_id` must be refreshed with `panes.get`. Transferred PTYs also verify the original owner and the proxy's broker/terminal identity; their existing broker is retained.

`submit: false` pastes without sending Enter. To submit an existing draft, issue another operation with empty text and `submit: true`. Multiline/tab input requires the application's bracketed-paste mode; otherwise it is rejected before delivery. Other control characters, including Escape and carriage return, are rejected. This API does not provide arbitrary terminal key injection. Input is limited to 32 KiB.

Inspect each pane's capabilities. Guarded delivery supports native owned,
transferred, and imported tmux terminals. For imported tmux, the target
identifies the source session's active pane at inspection. If another pane is
active by delivery time, the operation is rejected. The tmux server checks
the exact pane before applying paste or Enter, and rejects delivery in source
copy mode. `source_input_rejected` reports this final guard rejecting the
operation before input. Imported terminals retain source ownership and expose
no termination capability. For an imported agent-multiplexer pane, `panes.get` reports
the source pane and terminal IDs, shell PID, process start time, and foreground
process group when the agent multiplexer can verify them. The response keeps `target` null.
The current the agent multiplexer input schema accepts a pane ID and payload without an expected
terminal or process identity, so guarded API input for imported agent-multiplexer remains
open. See the agent multiplexer's socket API documentation.

An interrupted write can have an unknown outcome. Error `outcome` is `not_applied` for known rejected preconditions and `unknown` when side effects cannot be ruled out. A worker stopped after recording an operation but before recording its response leaves an indeterminate record. Retrying that UUID returns `operation_indeterminate` and does **not** repeat the operation. Inspect `operations.get` and the target before deliberately issuing a new UUID. Completed error responses are replayed too. Removing operation records discards this duplicate protection.

Requests are limited to 64 KiB; ordinary worker responses to 4 MiB. `plugins.job` permits 8 MiB so both bounded 512-KiB output streams remain readable after JSON control-byte escaping. `deadline_ms` defaults to 30,000 and accepts 100–120,000. Workers run in their own process group; deadline cancellation stops only that request's helpers. It does not restart a backend or a presenter. A shell already created before interruption may remain, so inspect the project after an indeterminate create.

## Waiting

`panes.wait` polls the exact target without sending input:

```json
{"version":1,"id":"wait-1","method":"panes.wait","params":{"target":{},"state":"completed","source":"reported","timeout_ms":30000}}
```

Replace the empty target with an actual returned target. A state condition accepts the [lifecycle values](agent-lifecycle.md). `source` defaults to `reported`, so a screen heuristic does not establish successful task completion. Use `source: "any"` explicitly to accept labeled heuristics. The response retains observation source/confidence. Heartbeats must remain fresh as documented by the lifecycle interface.

Instead of `state`, provide `terminal: "exited"` to wait for process exit, or `terminal: "removed"` to wait for membership removal. Removal is distinct from process exit. Timeouts accept 100–600,000 ms; `wait_timeout` means the condition was not observed. An outer worker deadline bounds stalled host inspection as well. A replaced target aborts the wait rather than following its replacement.

## Event subscriptions

```json
{"version":1,"id":"watch-1","method":"events.subscribe","params":{"project":"PROJECT","resources":["panes","agents","attention"],"duration_ms":30000}}
```

The initial successful response includes a snapshot, a subscription ID and a replay `cursor`. Subsequent lines contain `event`, `heartbeat`, or `ended`. The default event resources are `projects`, `panes`, `agents`, and `attention`. Request `sessions` explicitly to get the session catalog and `session.created`, `session.changed`, and `session.removed` events. Omit the project filter for session events, since a session can span several projects. Resource filters apply to initial snapshots and later events. Kinds identify creation, change or removal, such as `pane.created` and `agent.reported.changed`. Removal carries a null data tombstone. Each event has a global sequence, emission time, resource key, source and cursor. Report events describe accepted lifecycle reports, including archived runs; they do not independently prove that the agent is still alive. Use `panes.get` for fresh health.

Pane metadata changes and accepted report/attention transitions write directly to the journal. Working/completed/working transitions remain distinct even if they occur before a consumer reads. Report sequence/heartbeat timestamps alone do not create false lifecycle changes. File/catalog reconciliation discovers changes from older controllers or external metadata edits; those events are labeled `reconciled`, while producer events are labeled `domain`. This reconciliation runs without probing SSH hosts. Snapshot warnings preserve unavailable sources instead of turning corrupt metadata into false removals.

To reconnect, supply `after` with your last saved cursor. A resumed subscription returns a null snapshot and replays later matching events from the same durable journal, including across API process restarts. Cursors advance through filtered-out events too. A heartbeat/ended frame provides the latest processed cursor. Connections can observe a change already reflected in their initial snapshot; consumers should apply resource updates idempotently.

If journal polling or reconciliation fails after a stream has opened, the terminal `ok: false` response includes `error.subscription` and `error.cursor`. Resume from that cursor once the journal is available. A `cursor_expired` response requires a fresh snapshot.

A resumed stream reads its first batch directly from the retained journal. Live metadata is reconciled on the normal polling schedule if the stream stays open.

Retention is bounded to 2,048 events and 2 MiB of event records. If a cursor falls behind retention or its journal was replaced, the stream returns `cursor_expired` instead of silently omitting changes. Subscribe without `after` for a fresh snapshot. The journal and resource index live privately under `$XDG_DATA_HOME/starflux/automation/events.json`.

`duration_ms` defaults to 30,000 and accepts 100–600,000. Optional `max_events` accepts 1–100,000 matching events; replay counts toward the limit. Heartbeats do not. Streams apply stdout backpressure without an unbounded outbound queue. Close the output pipe or terminate the API process to disconnect; source processes and presenters remain running.

The API currently covers the methods above. Structured frontend-command acknowledgements, remaining session/import workflows, guarded input for imported agent-multiplexer panes and APIs for forthcoming machine/plugin features remain part of the broader parity work.

`snapshots.status` takes no parameters and reports collector liveness, process/boot identity and bounded capture diagnostics. Live views provide expiring coverage; the read-only collector takes over after the last view closes. See [cold-start recovery](cold-start.md).

## Acknowledged presentation commands

Call `frontends.list` and copy the entire `target` of the intended window into `frontends.command`. Targets bind the window's routing workspace, frontend ID, controller PID/start time, host boot and runtime directory identity. They remain usable when that controller switches projects or sessions. Refreshing the controller, replacing its runtime directory or rebooting invalidates them. A stale heartbeat prevents submission. Older controllers without command support must be refreshed first. `frontends.list` can filter by routing workspace or the currently selected project; its selected project and busy fields are sampled from the latest controller heartbeat.

`frontends.command` requires a UUID `operation_id`, like other writes. It accepts these command objects:

| `kind` | Additional fields | Behavior |
| --- | --- | --- |
| `select_project` | `project` | Select/reopen a registered project in the window's current session |
| `select_pane` | `project`, one-based `pane`, `epoch` | Focus a terminal with the matching membership/generation in the currently selected project; copy its epoch from `panes.list` |
| `select_space` | `project`, `space: "terminals"` or `"apps"` | Change the current project's space; unavailable App Space is rejected |
| `split_pane` | `project`, focused `pane` (one-based), `epoch`, `axis` (`right` or `down`) | Split the named pane in this window and return the new tab's pane number. Shell startup is asynchronous. |
| `split_existing_tab` | `project`, focused `pane`, `epoch`, hidden `tab`, `tab_epoch`, `axis` (`right` or `down`) | Split with a running tab and focus it. Both numbers are one-based. Requires `split_existing_tab_commands` from `frontends.list`. |
| `close_split_pane` | `project`, `pane` (one-based), `epoch` | Remove the pane from this window’s split layout and retain its terminal tab. Requires `close_split_pane_commands` from `frontends.list`. |
| `move_pane_to_edge` | `project`, `pane` (one-based), `epoch`, `direction` (`left`, `right`, `up`, `down`) | Place a visible pane or hidden tab at the outer edge of this window’s split. Requires `move_pane_to_edge_commands` from `frontends.list`. |
| `move_pane` | `project`, focused `pane` (one-based), `epoch`, `direction` | Swap the focused pane with its nearest visible neighbor in the direction. Returns `no_neighbor` if none is visible. Both terminal processes are retained. |
| `resume_conversation` | `project`, source `pane` (one-based), `epoch`, `run_id` | Open the saved conversation in a new tab using its registered command. Copy the run ID from `panes.get.saved_conversation`. The acknowledgement means the tab was accepted and queued for launch. |
| `open_session` | `session` | Open that saved session in this window |
| `focus_pane` | `project`, `direction: "left"`, `"right"`, `"up"` or `"down"` | Directional focus among visible terminal panes |
| `set_zoom` | `project`, `enabled` | Set terminal pane zoom explicitly |
| `present_plugin_pane` | `project`, owned plugin-pane `id`, `epoch`, optional `placement` | Place an existing owned plugin terminal; never run its command |

Plugin pane commands additionally require the `plugin_pane_commands` capability
from `frontends.list`. Old frontends are rejected before publication to their
command queue. Prefer `plugins.panes.open/focus` for creation plus placement or
owned-terminal focus, as documented in [Plugin workflows](plugins.md).

Project-bound commands reject a changed active project. Select the project first, or explicitly open its session before crossing session boundaries. STAR/FLUX dialogs, menus, panel navigation, Git details and terminal history review reject commands with `frontend_busy`, preserving the interactive task. Commands use normal UI navigation and release captured keys. They never type into programs, submit prompts, terminate terminals or launch an app command. Opening saved projects/sessions can attach or ensure their shells, and selecting App Space can reconnect its desktop, following the same behavior as the UI and its one-shot launch protections.

The default `ttl_ms` is 5000 (range 100–30000); it limits how long a queued command may wait before being claimed. `wait_ms` defaults to 3000 (range 0–10000) and must be at least 500 ms below the outer request deadline. Zero returns immediately after submission. Each frontend has a bounded queue of 32 commands, processed in publication order in small batches during its regular controller tick. A full queue reports `frontend_backlog`.

The result is a receipt with `operation_id`, `target`, `command`, `status`, `result` and `error`. Check `status`, not just the outer `ok`. `applied` means the named controller completed the command; `result` captures its selected project/session/pane/space at that moment and labels the acknowledgement `controller_applied`. This does not acknowledge that a presentation frame was displayed, that a backend has finished attaching, or that no later input changed the selection. Use pane health and the presentation protocol for those separate conditions.

Receipts move through `queued`, `applying`, then `applied`, `rejected`, `failed`, `expired`, `cancelled` or `indeterminate`. Rejections and queued cancellation/expiry have no command effect; an action failure may have an unknown partial outcome. The controller durably records its claim before applying anything. If it dies after that claim, the outcome is indeterminate and the action is never repeated. A late completed acknowledgement can resolve an earlier unknown outcome. Retrying the same operation ID replays its original API response, even if that response was still queued; use `frontends.command_status` for the current receipt. Inspect the current window before issuing a new operation after an unknown outcome.

An outer API deadline or disconnect does not cancel an already submitted command. Its lease still applies. `frontends.cancel_command` can cancel only `queued` commands; it returns `command_committed` after the controller has claimed one. Controller incarnation checks prevent old queued requests from reaching a replacement window. Private versioned receipts live under `$XDG_DATA_HOME/starflux/automation/frontends`, independently of the ephemeral queue in the runtime directory. Corrupt receipts are preserved for inspection and are never interpreted as permission to repeat an action.
