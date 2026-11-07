# Attention inbox and notifications

Open **Inbox** below the graphical sidebar's brand, use the cells toolbar or a pane/project context menu, or press **Ctrl+B**, then **i**. The inbox combines notifications from every saved STAR/FLUX session on the controller, including projects outside the current session. It lists newest items first. Arrows, Home/End and PageUp/PageDown navigate the menu; Enter opens the selected item, `/` opens search and Escape closes it.

## Find and review work

The inbox controls provide:

- Literal, case-insensitive Unicode search across project names/identities, session, host, provider, conversation identity, lifecycle label and event source. All whitespace-separated search terms must match. Search does not read agent transcripts.
- Unread, all or read-only history; question, approval, completed, failed and backend-unavailable states; project, current-session or all-session scope; and reported-event or screen-observation sources.
- Previous/next pages. Pages default to 100 items, keeping UI size and query responses bounded. The count shows all matching items; incoming events can shift page positions.
- **Mark unread items on this page read**, which captures exactly those event identities. It does not include newly arriving items or other pages. Each project's batch is checked before changing any of its items; errors for another project do not silently change that project's read state.

Filters and page position persist for each frontend, including refresh and reattachment. They do not change another window's filters. **Reset filters** returns to all unread events.

Choose **Open pane** to enter the originating session/project and focus its terminal. Opening marks the notification read. **Dismiss notification** marks it read without switching projects or sending input. Read state is durable and shared between frontends. Reading or dismissing a notification does not answer a question or approve a command.

Pane generations are checked before and after switching sessions. If the original pane has gone away, the item remains available for review and dismissal until retention expires it. Project/session deletion removes that project's saved items. A project whose attention data cannot be read is shown as unavailable; other projects remain accessible.

## Event identity

Structured lifecycle reports create items for questions, approvals, completion, failure and unavailable agent backends. An item retains its source, run identity, sequence, pane generation and observation time. When its provider report exists, query results also include the provider and conversation identity.

Labeled screen observations also create question, approval and backend-failure items for detected agents. A screen observation cannot establish completion or conversation identity. Reported and heuristic sources can be filtered separately. Repeated heartbeats and unchanged screen observations coalesce; a later transition back to an attention state creates a new item.

## Delivery and settings

Choose **Notification settings…** from the inbox to configure popup delivery, event categories, active-pane suppression and retention. Settings are stored on the controller in `$XDG_CONFIG_HOME/starflux/notifications.json` (normally `~/.config/starflux/notifications.json`). Updates use revisions to reject stale edits; malformed settings are preserved and reported. They do not prevent valid agent lifecycle reports from being saved.

- **In-app status** is the default. New eligible events update the status message and unread count.
- **Desktop + in-app status** additionally submits new events to the controller's local desktop service. Linux uses `notify-send`; the macOS path uses `osascript`. Linux needs a desktop session, a session D-Bus and libnotify's helper. Nix/AppImage packaging includes the Linux helper; Arch lists it as optional.
- **Off** disables new popup/status delivery while retaining the inbox and unread count. Event category switches affect delivery, not event storage or inbox visibility.

Desktop mode is offered only when a local desktop is available. A controller launched through SSH does not send notifications to the remote computer's desktop. Forwarding an OS notification to the presenting SSH client still needs presentation-protocol support; SSH users retain the inbox and in-app delivery.

Desktop delivery runs outside the input/render path, with bounded helper time and a maximum of three helper attempts per scan. Independent frontends share a durable claim ledger, preventing duplicate OS requests for an event. Visible, focused panes can suppress a popup without marking its item read. Focus metadata includes the controller process identity and expires after six seconds. App Space does not count as a visible terminal pane.

A successful helper exit means the request was **submitted** to the desktop service; the OS controls whether it appears. Failed or timed-out helpers retain the inbox item and record an error. A crash after a claim leaves an **indeterminate** attempt. These outcomes are not retried automatically, avoiding duplicate popups. **Desktop delivery diagnostics** shows recent attempts, failures and capability information. Frontend startup does not replay old inbox events as desktop popups.

## Retention

Defaults retain up to **2,000 items per project for 30 days**, including read and unread items. The settings form accepts 0–3650 days (0 disables age expiry) and 10–50,000 items per project. Age expiry runs first; the count limit then keeps the newest items. Future-dated items remain subject to the count limit.

Retention runs when new attention is saved, periodically in background scans, or through **Apply retention now**. It removes notification metadata only: terminal processes, layouts, current agent records and saved conversation references remain intact. Failed metadata reads preserve the affected project's file. Desktop delivery records for expired items can be collected once the ledger grows; retained item claims are preserved. A full delivery ledger fails closed and leaves the inbox available.

## Automation and verification

The [versioned API](automation.md) provides `attention.query`, `attention.mark_read`, `notifications.settings`, `notifications.configure`, `notifications.prune` and `notifications.delivery`, alongside the existing list/dismiss and event subscription methods. Queries default to 100 results and allow 1–250 per page with `offset`; responses include `total`, the effective `offset`, `items` and read warnings. Configuration and mark-read/prune writes require an operation UUID. Configuration requires `expected_revision` and a settings object. Desktop diagnostics return at most 200 recent attempts and a total count.

Private graphical/cells SSH fixtures exercise filters, read history, pagination, identity preservation, retention, active-pane suppression and deduplication. `scripts/test-session-manager.py --notifications --notification-service` additionally exercises the real Linux helper against an isolated D-Bus notification service, without contacting the user's desktop. That optional check needs the system Python GI bindings, D-Bus daemon and `notify-send`.
