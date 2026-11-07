# Cold-start recovery

Projects, terminal membership, tab names, split trees, project order and per-window layouts live under `$XDG_DATA_HOME/starflux`. They survive loss of the runtime directory. Reopening the same frontend restores its layout. A retained backend is reattached; a process that ended is not brought back by restoring its screen.

Transferred Herdr PTYs are live processes and do not survive a host reboot.
Startup now retires an old transfer when its recorded owner boot differs from
the current host boot. Older transfer records use a matching saved screen's
observer boot, PID and terminal epoch as proof. A missing broker alone is not
proof of a reboot. Recovery starts an ordinary shell in the same project/pane;
it preserves tab names, splits, saved screens and registered conversations.
It does not run the previous agent command or transfer proxy, and does not signal
an old PID that could now belong to another process. Choose **Resume saved
conversation in new tab** to restart a registered agent conversation explicitly.

Private recovery receipts at `terminal-recovery/PROJECT/pN.json` survive preview
clearing and later captures, so a recovered pane cannot revert to its dead
transfer mapping. Receipts identify the exact transfer generation; a new pane in
the same slot can still launch an explicitly requested agent. Session deletion
removes its receipts. New migrations record the owner boot directly.

The default session is saved separately from runtime state, in
`sessions/default-session`. An ordinary launch opens that session at its last
selected project; an explicit `--workspace` takes precedence. Choose **Make
default** in the session picker to change it. A reboot does not select a new
default or automatically resume an agent conversation.

If initial terminal setup fails (for example, a project directory has not mounted
or SSH is temporarily unavailable), the UI reports **reconnecting** and retries
every three seconds. It does not leave the pane permanently registered without
an attachment. One-shot agent launch intents are still never replayed.

An exited tmux pane can retain its screen while reporting an empty process ID.
STAR/FLUX reports this as an exited terminal, with no process identity, so its
recovery menu remains usable. **Reopen in a shell** starts a fresh shell only
when explicitly selected; observing or reconnecting an exited pane does not
silently rerun its previous command.

Right-click a terminal tab, or press **Ctrl+B, x**, and choose **View saved screen** or **View previous process screen**. The native renderer supports these previews in graphical and cells mode. They use the history review controls: arrows, PageUp/PageDown, `/` search, Space to select, Enter/y to copy and Escape to return to the current terminal. A preview is visibly labeled **Saved screen / read only**, with its original dimensions and capture time. Smaller panes crop the preview to their current geometry. The saved original remains unchanged.

The screen saver retains the parsed visible terminal grid, including Unicode, cell colors/attributes, cursor position and alternate-screen metadata. It captures changed attached terminals at most once every five seconds. Visible terminals reuse their rendered grid. Unattached terminals are captured through existing health monitors, using bounded read-only inspection at intervals of at least five seconds, with at most two concurrent captures per controller. Native, transferred PTY and imported tmux/Herdr sources are supported without opening hidden renderer attachments. Native/remote tmux frame requests reuse the health RPC; Herdr uses its local read APIs and a read-only terminal-size query. Captures verify process/geometry and terminal generation before saving. Failed or slow sources release their capture budget so other terminals can continue. Full scrollback is not periodically collected. Serialization, private file writes and fsync run on a bounded background queue. Busy project transactions are skipped and retried so a stalled edit cannot hold up the shared capture queue. Disk pressure can delay a capture; the last few seconds before an abrupt shutdown may be absent.

A cursor waiting to wrap after the final column is a valid saved state, including
after a wide Unicode character. Screen validation accepts this parser state;
coordinates beyond it or below the last row remain invalid.

Each pane slot retains one current screen and one screen from its previous process/generation. Changing a process, boot, host or terminal epoch rotates the current screen into the previous slot. Capture order uses the controller boot UUID and monotonic clock, so a wall-clock correction or reboot does not prevent new previews. Older captures and queued updates for removed/replaced terminal generations are ignored. A project has at most 32 slots. Each formatted grid is limited to 1 MiB; its versioned JSON document contains base64 bytes and is bounded to 2 MiB plus 4 KiB. Oversized or corrupt documents report an error and cannot inject input into a terminal. Existing valid previews are preserved if saving fails.

Files are private (`0600`) at `screens/PROJECT/pN.screen.json` and `screens/PROJECT/pN.previous-screen.json` under the STAR/FLUX data directory. They are kept outside the project catalog so catalog refreshes do not read screen contents. They are stored on the controller machine, including when the terminal itself is on an SSH host. The records identify that host and its boot/process where available. Default terminal colors follow the current theme. Pixel graphics and the terminal's hidden primary/alternate buffer are not archived; inline graphics remain a separate parity task. The legacy Kitty pixel renderer does not display these native-grid previews. An independent read-only collector keeps registered terminal screens current after every view closes, including terminals created through the headless API. It reads existing backends and never launches a saved command, agent or application. Live controllers publish short-lived process-verified coverage so the collector avoids duplicate captures; it takes over after their coverage expires. It scans saved membership every five seconds and captures uncovered live terminals at intervals of at least ten seconds, with at most two concurrent requests. Requests are served oldest-due first so slow sources cannot starve later registered panes. Unavailable terminals are retried no more than once per minute. A collector with no live terminals or controller coverage retires after one minute. Opening a view or an API connection starts it again. Each data directory has its own collector lock and private diagnostics, so separate data namespaces stay isolated. Removing or replacing the runtime directory retires its old collector without recreating that directory or stopping retained terminals. A newer collector version requests a cooperative handoff; older binaries cannot downgrade it. Set `STARFLUX_SNAPSHOT_SERVICE=off` to disable starting this service in a process; project pause controls apply to collectors as well as views.

**Pause screen saving and clear previews**, in the same pane menu, disables saving for the entire project and clears its existing previews. Other frontends respect this shared setting, including queued writes. **Resume screen saving** starts capturing again. Deleting a session removes its screens and launch receipts. Clearing previews leaves launch receipts intact.

Saved command hints are not automatic restart instructions. A new agent or an explicit **Resume saved conversation in new tab** gets a one-shot launch intent. STAR/FLUX durably consumes a separate receipt before invoking the backend, so a stale layout save or uncertain SSH result cannot reuse that intent. If a backend is later lost, reopening its tab starts a shell. Choose an explicitly registered conversation to resume the agent in a new tab. A launch interrupted after consuming its receipt may require a new explicit resume action; it is never retried automatically.

Archived bytes are only processed by a parser without terminal callbacks for display. They are never replayed into a PTY, remote shell or host terminal. Screen restoration does not restore mouse-reporting modes, bracketed paste, clipboard operations, title changes, notifications or query responses. App Space reconnects to retained workers independently; screen archives do not start applications or restore dead processes.

The read-only automation method `panes.saved_screen` accepts `project`, one-based `pane` and optional `previous` (default false). It returns `historical: true`, `read_only: true`, parsed `text` and the screen record. This is historical evidence, not an authorization target; use `panes.get` for current process identity.

The read-only `snapshots.status` automation method reports whether the collector is running, its process/boot identity, registered and covered pane counts, in-flight requests, scan/capture/error counters and last scan time. It does not return screen contents.
