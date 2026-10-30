# App Shelf

> App Space is available in graphical frontends with local or streamed app-frame
> support, including Kitty over SSH. Cells mode hides app controls. Open it with
> **App Space** in the terminal header or **Open App Space** in a project's
> right-click menu. Terminal space keeps the full terminal area.

STAR/FLUX now uses the App Shelf direction from the design studies. A project owns its agent, terminal tabs, launch command and virtual app desktops. Agents and shells now use Kitty’s own renderer, presented at native pixel resolution. Graphical apps retain their private compositor. The chrome uses single pixel borders, a drafting grid and the selected STAR/KIT palette.

## Start a project

Open **Sessions** beneath the STAR/FLUX name to manage saved groups of projects.
The searchable list lets you select and open a session, create a named session
with its first project, or delete one after typing `DELETE`. Switching retains
the other session's programs and affects only the current view. Deletion stops
STAR/FLUX-owned terminals and apps and closes other views of the deleted session.
The original tmux and Herdr sessions stay running when their imports are deleted.

**Import…** lists tmux sessions from the standard socket directory (including
named servers and the current `$TMUX` socket) and running Herdr sessions. A tmux
import retains its existing windows and panes inside a live terminal tab. A Herdr
import retains its projects and terminal tabs. Importing attaches to the existing
programs without restarting them. Saved sessions remain available across launches.

Select a saved session and choose **Make default** to open it on ordinary launches,
including `starflux --ssh HOST`. **Clear default** restores independent unnamed
launches. An explicit `--workspace` overrides the default. The picker marks the
default in its details, and deleting it clears the preference.

Manual project order and the sorting mode are saved with the session, so new
windows, refreshes, and session switches retain them. Closing a project remains
local to its window; reopening it restores its position in the saved order.

```sh
starflux --workspace my-app --directory /path/to/project --app 'cargo run --release'
starflux --workspace remote-app --host buildbox --directory /work/project \
  --app 'starfold .' --desktop-helper /home/you/.local/bin/starflux
```

New projects open a login shell. Run Codex, Claude, Gemini, OpenCode or Aider in any terminal; foreground process detection tracks the agent and clears its role when it returns to the shell. `--agent COMMAND` remains available for explicit startup commands and older workspaces. New projects put their shells and agent on the project host. Existing workspaces keep their original session IDs and host assignments, including legacy local shells beside SSH agents. Migration does not respawn running agents.

Project metadata is saved privately under `$XDG_DATA_HOME/starflux/projects` (normally `~/.local/share/starflux/projects`). Runtime sockets and inspection snapshots remain under `$XDG_RUNTIME_DIR/starflux`.

## Work inside the project

- Right-click a terminal tab and choose **Reconnect pane** to rebuild its attachment without stopping the program. For a hung program, choose **Recover to shell…** and type `RECOVER` to confirm. Recovery stops only the selected pane's program; transferred terminals keep their original shell when it is alive, while ordinary terminals start a fresh shell. Other tabs and App Space workers remain running.
- The terminal view keeps all shells and agents in the same tab strip. **+ Tab** adds a shell in the project directory. **Split** opens and focuses a fresh shell beside the current terminal, even when other tabs already exist. Setup and attachment share one connection on local and SSH projects. **Unsplit** retains both sessions. **Close pane** dismisses the focused half and leaves its terminal available in its tab; right-click a terminal tab for the same action. To end the shell itself, use `exit` or Ctrl+D at its prompt. Confirmed terminal exits remove their tabs and collapse the affected split; the last exit leaves an empty project until **+ Tab** is chosen. Connection failures do not remove tabs. Drag the separator to resize it. The ratio and selected tab are saved with the project.
- Open the virtual desktop using the bottom **Open app space** card. Use **‹ Terminals** in the app header to return to the selected terminal and its split layout.
- **Refresh** in the top header reloads the installed UI build while retaining terminal sessions and app workers.
- **+ Launch app** inside app space adds another application. Its command, directory, host and remote helper are editable. A supporting app can run locally or on another SSH host.
- App cards remain in the shelf while working with the agent or shells. Select a card to open that app's host desktop and focus its window.
- The virtual app space offers **Tile**, **Maximize**, **Close** and **Reconnect**. Application title bars and Alt-drag support moving windows; Alt-right-drag resizes. Each host has its own private desktop, so selecting an app also selects the correct host and input route.
- The persistent **Projects** sidebar lists project names and hosts in the saved manual or activity order, with the active container marked by an accent strip. Its full row, including padding and the activity mark, selects the project; more specific actions and menus sit above that selection target. Twenty projects fit at normal window heights; shorter windows use a scrollable list with a visible count and scrollbar. Switching keeps sessions alive. After releasing embedded input, `[` and `]` cycle saved projects. **+ New project** or `p` opens the new-project dialog.

Click a terminal or app to capture its input. **Ctrl+Esc** releases it. With input released: `1` or `2` selects Terminals, `3` selects Apps, `n` adds a terminal, `a` launches another app, `r` runs the project app, `t` cycles themes, and `q` closes the workspace. Project and space changes release embedded keys and pointer buttons.

Dialogs support mouse field selection, Tab/Shift+Tab, ordinary editing, pasted text, Escape to cancel and Ctrl+Enter to accept. Commands are parsed as argument vectors. Use an explicit shell such as `bash -lc '...'` when shell syntax is needed.

## Working-project color wave

**Manual** is the default sidebar mode. Drag a project row up or down; a thin accent line previews its new position. Dropping reorders without selecting the project. Escape or dropping outside the list cancels. Short lists can scroll while dragging near their edges. The saved arrangement survives restart; new projects append to it. Click **Manual ↕** to switch to **Activity**; click **Activity** to restore the manual arrangement.

Activity mode groups working projects first, then pending requests, live idle agents and other projects. Recent activity sorts each group. **High ↓** uses that order; **Low ↑** reverses the entire list, including ties and unknown activity, putting the busiest projects at the bottom. The text control has a border only on hover. Sorting retains the active project, sessions and split selection, and persists across restarts.

Recency updates from terminal output changes, state changes and typing/pasting. Native helpers return output fingerprints; imported Herdr panes use bounded current-footer fingerprints and backend state-change sequences to seed historical order. The first fingerprint sample for each pane establishes a baseline. Opening a project can resize or redraw its terminal; the initial presentation changes establish a new baseline without updating recency. Selecting a project, dragging rows and changing sort modes do not count as work. Terminal contents are not saved with navigation preferences.

Working project names use bold accent text with a broad, brighter wave that sweeps back and forth across the letters. The working count also uses the accent color, so activity remains recognizable between wave peaks. Idle, completed and disconnected agents do not animate; a confirmed pending question shows a static amber **?**, and an approval request shows **!**. Idle or completed agents have no icon. Generic blocked lifecycle flags do not request attention. The grid remains visible behind every row.

A small orbiting star appears beside each working project: a steady central spark with a bright point and fading trail moving around it. It uses the current theme and the same animation clock as the text. Reduced motion keeps the icon static; it disappears when the project is no longer working.

Imported Herdr metadata is queried in read-only batches per session, including projects that have not been selected. Known Codex/Claude panes also use a bounded 32-line current detection footer to override stale lifecycle flags. Native agents use explicit terminal state when available; Codex/Claude activity can also be read from their status footer on the project host. Native helpers return state metadata and a fingerprint; imported footer text is discarded after classification and hashing. Footer recognition depends on the harness keeping its current status wording; unrecognized agents stay static rather than being marked busy just because their process is alive. Summaries refresh in the background, roughly every two seconds.

The wave completes a back-and-forth cycle in about 5.3 seconds only while a project is working. Decorative updates pause for 150 ms after typing, and stop entirely when no projects are busy. `STARFLUX_REDUCED_MOTION=1` keeps the working count and a static text tint without animation. Unicode names retain their full text shaping and use a whole-name color pulse.

`python3 scripts/test-activity-wave.py` verifies changing working-name pixels, static idle/waiting rows, activity transitions, idle timer suspension, and reduced motion using a disposable Herdr API fixture. It does not attach to original Herdr sessions. `python3 scripts/test-agent-activity.py` checks native working/idle/approval detection against a disposable terminal fixture and verifies that only metadata leaves the helper. Evidence is in `target/activity-wave-proof`.

## Terminal response time

Wheel scrolling over a terminal uses the persistent backend history, including text produced before attachment or refresh. A plan remains readable while new output arrives; scrolling back to the bottom returns to the live approval prompt. In a split, the wheel targets the pane under the pointer without moving keyboard focus.

The default terminal path polls PTY updates independently of chrome and sends only changed styled rows. The presenting Kitty renders real character cells, using its own fonts and text engine. Terminal scrolling has no GPU readback, screenshot encoder, image upload or per-frame round-trip acknowledgment. App space retains its independent 60 Hz compositor and negotiated video transport.

The older offscreen path is available only with `STAR_TERMINAL_RENDERER=kitty-pixels`. `scripts/test-native-terminal.py` checks the native path without a display or real agent. `scripts/bench-terminal-latency.py` compares direct Kitty, cells and graphical native terminal input locally and over private authenticated SSH; see [render performance](render-performance.md) for measurements and limits.

## Multiple agents in a project

The **Agent** space has its own agent tabs and a **+ Agent** button. Each additional agent owns a separate persistent terminal session, starts in the same project directory on the same local or SSH host, and keeps running when another agent or project is selected. New Codex agents start with a fresh conversation; the original agent keeps its existing conversation and launch command. They share the project files, terminal tabs, Git view and app desktop.

Click an agent tab to switch; status dots show its health. Right-click to rename or focus the agent, or create another. When the strip fills, the arrow controls cycle through all agents. Names, selected agent and session identities survive closing/reopening the workspace. Additional agents and shell tabs use separate session IDs. **Attach agent** operates on the selected agent, and `--stop` cleans up every native agent in that project while preserving imported Herdr terminals.

Keyboard forwarding preserves uppercase letters and shifted punctuation in Kitty panes, including events where the terminal has already translated the character without a Shift modifier. Synthetic Shift is released after key-up. The experimental direct renderer uses the same Control/Alt modifier contract as the frontend.

Returning to the workspace from another desktop window automatically restores keyboard capture and native focus to the selected agent or terminal. Split panes retain the selected half. Resizes during window switching defer focus until the new scene is available. Rename and launch dialogs keep their text-field focus; a context menu dismisses on window blur before terminal focus is restored. Ctrl+Esc still releases input until you click the pane or activate the workspace again. The installed KIT candidate supports the explicit local `focus-canvas:terminals` activation notice; the portable baseline remains compatible with click-to-focus frontends.

`python3 scripts/test-multiple-agents.py` checks real GPU panes with two disposable agents per project, uppercase/Shift/punctuation and Control/Alt bytes, separate input, matching working directories, rename controls and persistent process IDs after controller restart. It covers local and authenticated loopback SSH projects without touching original Herdr sessions. Evidence is in `target/multi-agent-proof`.

## Project menus and repository state

The wordmark is **S T A R / F L U X**. Right-click a project in the sidebar or its header to rename it, open its agent, terminals or app space, or create another project. Right-click a terminal tab to rename it, focus it, split/unsplit, or add a tab. Menus use the active theme, single borders and inset selection. Use arrows and Enter, or the mouse; Escape and outside clicks dismiss without passing input through to a session.

Project and terminal names are durable display labels. Renaming retains workspace/session IDs and running processes; imported Herdr names remain unchanged. Names accept up to 64 characters, including Unicode.

The active project header shows its Git branch, staged/unstaged/untracked file counts, tracked additions/deletions and ahead/behind counts. Click that status line to open the working-tree inspector: separate staged and unstaged line totals, file-by-file index/worktree status, rename paths, conflicts, merge/rebase state, stashes, upstream and the latest commit. Scroll the file list; **r** refreshes and **Escape** closes. Untracked contents are counted as files and excluded from line totals. Binary changes are included in changed-file counts.

Repository inspection runs in the background on the project's configured host, locally or through authenticated SSH, and refreshes after three seconds. Requests are time bounded and do not write the Git index, stage files, commit, or fetch. Ahead/behind reflects locally known upstream refs. Each project keeps its own status cache; switching projects does not display another project's result. Missing repositories and inspection/SSH failures have explicit states.

`python3 scripts/test-git-status.py` verifies mixed staged/unstaged changes, unusual paths, renames, binary changes, conflicts, unborn/detached branches, upstream counts and unchanged index bytes. `python3 scripts/test-project-tools.py` checks the real GPU menus, Unicode renames, restart persistence, retained session PIDs, isolated inspector input, background refresh and authenticated loopback SSH Git inspection. Screenshots and results are in `target/project-tools-proof`.

## Kitty-rendered local and SSH terminals

The SSH bridge keeps protocol replies out of terminal echo and consumes queued replies before returning control to the shell, including when startup fails. Internal `STAR_KIT_BRIDGE` records should not appear in scrollback or shell command history. `scripts/test-bridge-history.py` checks both paths over a private SSH connection with delayed terminal replies.

Kitty on the presenting machine draws native terminal characters, including over SSH. Its configured font, glyph shaping and text-composition settings apply directly. STAR/KIT draws the surrounding chrome and leaves cell-aligned transparent terminal regions. The `− / size / +` header controls the current Kitty OS window's font size when the updated bridge is loaded; otherwise Kitty's own font shortcuts remain available. Session processes survive frontend restarts.

## Selecting and copying Codex text

Use a normal **left-button drag** in an agent or terminal pane. STAR/FLUX distinguishes a drag from a click and projects selection into PTY cells, even when Codex captures mouse input. Releasing copies the selected text to the presenting desktop clipboard, including over SSH. Single clicks still reach terminal menus; Ctrl+drag preserves application mouse handling. Selection remains attached to the pane where it began, including splits. Kitty renders the characters and their selection colors directly.

## Local and SSH apps

**Launch app** opens a searchable catalog from the selected host’s system application registry, including user-installed desktop entries. Type to filter by name or executable, use the arrow keys to select, and press Enter or **Launch**. **Custom / host…** retains manual commands and host settings. Changing the host reloads its catalog asynchronously; hidden desktop entries stay hidden. Registered launches run inside the private app desktop and inherit its audio route.

The terminal shelf shows one **Open app space** card for the virtual desktop, with a geometric map of all its windows and an app count. Multiple running apps share that card. Window rectangles reflect positions and sizes reported by the app host, preserve the desktop aspect ratio, and accent the selected window. **Launch app** adds another application to the desktop; individual window selection stays inside app space. Layout updates work locally and over SSH.

Graphical applications run in a persistent private compositor on their own host. A workspace on the app host receives shared raw frames; a remote frontend receives a bounded H.264 stream with local decoding. Input travels over the framed transport locally or through OpenSSH. The app helper negotiates a protocol version, preserves stable window identities and retains only the newest frame. Hover motion is coalesced separately from ordered controls.

For SSH graphical apps, install a compatible STAR/FLUX executable on that host, plus its runtime libraries, `dbus-run-session`, Xwayland and a usable EGL graphics device. `--desktop-helper` selects the executable or runtime wrapper on that host. No forwarding into the host's normal desktop is used. A private D-Bus session prevents applications from handing their launch to an existing desktop instance. Ordinary SSH agent terminals continue to need only Python 3, tmux and the agent command.

Closing or killing a frontend does not terminate the app compositor. **Reconnect** and reopening the workspace attach to the same app processes. A host reboot or explicit `--stop` ends them. Failed remote startup is reported in the workspace; the agent and shell spaces stay available.

The app compositor targets 60 frames per second, up to 1920×1080. App Space displays native pixels at 1:1 size, centered within larger workspaces; smaller simultaneous viewers crop the shared desktop rather than scaling it. Local worker IPC uses private, immutable raw frame files with one outstanding acknowledgement, avoiding PNG encoding and decoding. Compatible SSH frontends use STAR/SCREEN’s low latency H.264 model: local decoding, bounded frame credits, and coalescing before encoding. Encoding and GPU readback still use CPU memory; sustained rates depend on resolution, host, client and network. Older workers and app hosts on a separate SSH connection retain the PNG compatibility path, capped at 1920×1080. An already-running app desktop needs a one-time relaunch to adopt the new worker; reconnecting a frontend preserves existing apps and does not upgrade that worker. App audio uses a separate bounded SSH pipe and STAR/KIT’s negotiated 48 kHz stereo PCM relay.

Embedded Kitty terminal frames use a separate native-resolution, lossless PNG patch transport in compatible KIT candidate builds. Encoding runs on a worker, unchanged pixels are omitted, and acknowledgement-scoped bases prevent stale patches. Text bypasses the JPEG thumbnail compatibility path. Older frontends retain the image compatibility transport. Runtime snapshots identify the negotiated terminal transport and app video delivery counters.

## App audio and mute

The header's **Audio on / Muted** control mutes this STAR/FLUX workspace without changing system volume or stopping apps. The preference is saved across restarts and applies across its projects. With terminal input released using Ctrl+Esc, **m** toggles mute. The CLI actions `audio:mute`, `audio:unmute` and `audio:toggle` provide the same control.

The selected project's app host is audible even while its terminal view is selected. Apps in that host share a private output sink. Switching projects or app hosts closes the old playback route; other desktops stay isolated and keep running. Mute and disconnect close local playback and discard queued samples. Unmute and reconnect use a fresh epoch, so old sound is not replayed. Device errors report failure and keep the host sink isolated; they never switch sound to the host's speakers.

For a workspace on another machine, run the frontend on the machine whose speakers you want to use:

```sh
starflux --ssh HOST --workspace herdr-default-wV
# A custom server installation:
starflux --ssh HOST --remote-executable /path/to/starflux --workspace NAME
```

`--host` still selects an app/project host for a locally owned workspace; `--ssh` attaches a local frontend to an existing remote workspace. Matching audio-enabled builds are required on both ends. An existing SSH shell can use the STAR/KIT Kitty bridge; its local `star-kit-terminal` must also include audio output. An unbridged SSH TTY cannot reach the client's speakers and shows **Local audio missing**. Clicking that control explains the required local setup instead of toggling mute. The lightweight [local terminal client](../terminal-client/README.md) supports listening machines without a full STAR/FLUX installation. The installer uses a content-versioned watcher path to bypass Kitty’s module cache. After installing, reload Kitty configuration, then Refresh UI or reconnect STAR/FLUX; active bridge connections are preserved during the configuration reload.

The app host needs a running PulseAudio-compatible server (including PipeWire-Pulse), `pactl`, `parec`, and `pw-dump` for adopting existing native PipeWire streams. It does not need a physical audio device. New apps inherit their desktop's private sink. Already-running apps can be moved into it without restarting: only verified descendants of that private compositor are eligible, using the PulseAudio process ID or PipeWire client credentials. No microphone or system-wide output is captured.

Audio capture, metadata scanning and video use separate workers. Eight block credits bound delivery through the existing STAR/KIT frontend; a slow listener cannot grow memory or block app input/video. The live capture queue retains only recent samples instead of accumulating old audio. A missing host capture service or local output device is reported in the workspace.

`python3 scripts/test-app-audio.py` exercises authenticated loopback SSH, an actual native PipeWire app stream, bounded credits, reconnect without app restart, local STAR/KIT playback into a disposable output sink, mute silence and saved mute across restart. The test does not change user applications or send audio to physical speakers. Handy's sound must be enabled; do not launch with `--no-sound` when testing audio.

## Agent launches on the app host

An agent running on the app host can open a window without taking over the viewer or configuring DISPLAY:

```sh
starflux --app-launch my-app --app-directory /work/project -- ./game
starflux --app-list my-app
```

The identity must match the viewer's project workspace. This uses a separate local control socket, waits for a new window and reports it as JSON. A viewer may remain attached throughout. The worker injects its private display and D-Bus environment. Run this command through SSH on a remote app host; configure the project's app host/helper in **Launch app** so the local viewer connects to that same desktop. The helper must be installed on that host. `--app-list` starts an empty desktop if none exists.

The Zendocon checkout includes `python3 tools/run_in_starflux.py` (default desktop `herdr-default-w8`) and `docs/starflux-app-space.md`. It launches the existing cartridge, without rebuilding or replacing the game.

STAR/KIT's GPU frontend is opt-in with `STAR_GRAPHICS_RENDERER=gpu starflux ...`. The installed candidate includes GPU support. Kitty and the private app compositor have independent renderers; enabling the frontend does not enable hardware video encoding or make emulator logic run on the GPU.

## File delivery

Dropping onto an agent or terminal freezes the project, host and session at the drop. Files are copied, hashed and staged on that host before the destination path is pasted without Enter. Switching projects during transfer does not redirect delivery. This retains the Codex attachment path used by the earlier desktop acceptance tests.

An app-space drop stages the file on the selected app's host and offers `text/uri-list` through that app's clipboard with a paste gesture. Apps must support that MIME type to consume it. This is not native Wayland drag-and-drop emulation for every GUI toolkit. Text pasted from the outer terminal also crosses the app transport; exporting an app's clipboard to the outer desktop is not implemented yet.

## Inspect, detach and stop

```sh
starflux --workspace my-app --inspect
starflux --workspace my-app --action view:apps
starflux --workspace my-app --action terminal:new
starflux --workspace my-app --close
starflux --workspace my-app
starflux --workspace my-app --stop
```

`--close` detaches the controller while retaining tmux sessions and app workers. `--stop` explicitly stops that named project's terminals and app desktops. Other projects are retained. A project lease prevents two updated controllers from simultaneously resizing or typing into the same project.

Already-running controllers keep the executable they started with. To adopt App Shelf for an existing workspace, close its workspace frontend and reopen the same name. Its agent processes remain alive.

## Validation

`python3 scripts/test-app-shelf.py` uses disposable local and SSH projects, real Kitty, actual GTK applications and a private authenticated loopback SSH server. It checks terminal tabs and splitting, multiple apps, keyboard input to the SSH app, project isolation, process identity after app reconnect, and compositor/window identity after closing and reopening the workspace. Screenshots and results are written to `target/app-shelf-proof`.

The transport has been checked with loopback SSH; this does not establish latency or graphics-device compatibility on a separate remote host. Broader lifecycle and release hardening stay in `TASKS.md`.

## Import an existing Herdr session for testing

`starflux --import-herdr` imports the default Herdr session into a separate `herdr-default` collection. It preserves workspace labels/order, directories, tab labels and terminal identities. The import prints the workspace to open. Native test/proof projects are excluded from this collection.

Imported panes use `herdr terminal attach` against the existing terminal ID through Kitty by default, or a direct PTY when explicitly selected. They are interactive links to the same running shells and agents, not restarted or copied conversations. Herdr must remain running. No `--takeover` flag is used. Legacy Agent shortcuts migrate to their matching terminal tab, sharing one attachment. Pasted text uses the attached Kitty, or the direct PTY paste socket in experimental direct mode, with bracketed paste and no automatic Enter. Stopping the STAR/FLUX project never stops its original Herdr terminals.

The project sidebar follows the saved activity direction and keeps the collection visible. Wheel scrolling in a shorter list does not change the active project. The test import contains 20 workspaces, 24 original tabs/panes and 16 detected agents. The initial read-only mirrors were replaced after input testing; those mirrors are no longer the normal imported-terminal path.

`python3 scripts/test-handy-ssh.py` uses the local Zendocon checkout and a disposable authenticated loopback SSH server. It checks sideband launch with an attached viewer, cartridge rendering, keyboard start, changing game frames and stable compositor/window identity across reconnect. Handy replaces its initial SDL window during cartridge initialization; input targets the resulting game window. Evidence is saved in `target/handy-ssh-proof`.

`python3 scripts/test-font-inheritance.py` records real Kitty font/DPI queries and compares them with the negotiated terminal sizes/cells for both the normal user configuration and a Liberation Mono override. Native Kitty and STAR/FLUX screenshots are saved in `target/font-inheritance-proof`.

`python3 scripts/test-project-sidebar.py` uses 20 disposable projects with imported labels to verify that all project rows remain visible, the final row can be selected directly, and switching preserves order. Screenshots and results are in `target/sidebar-proof`. It does not attach to or type into original Herdr sessions.

Image drops accept local file lists and PNG/JPEG/WebP/GIF image data. When Kitty reports files from a different machine, STAR/KIT requests their bytes and stages private copies on the workspace host before delivery to the selected terminal or app. Original files are retained; the application sends no Enter. Browser image drags can fall back from their web URL to offered image bytes. Remote folders and symlinks are rejected explicitly. URI metadata is limited to 1 MiB, direct image data to 32 MiB, and remote staging to 64 files / 256 MiB total; destination uploads retain their existing 64 MiB per-file limit.

During drops, the frontend pauses terminal reads and its file-data producer before the outbound queue fills. Presentation and protocol replies retain queue capacity while the connection drains; file chunks remain ordered and are never coalesced. This prevents a slow SSH connection from terminating the frontend with a full control queue. The isolated regression check transfers 4 MiB through a deliberately slow local socket and verifies the image bytes and control replies without opening windows or contacting SSH hosts.

App input uses the actual worker frame dimensions, including while a resize is pending. App desktops keep a 60 Hz frame clock; the terminal-only input burst does not apply to them. App-space chrome refreshes periodically at 10 Hz, independently of live video. Repeated pointer modifier state is filtered before motion coalescing.

New app workers negotiate cursor tracking: software cursor surfaces are composited with their hotspot, and the frontend hides its local pointer over the app canvas when the app supplies or hides its own cursor. Controls and overlays retain the local pointer. Retained workers from older builds still reconnect, but need a deliberate app-desktop restart to gain worker-side cursor tracking and frame-clock changes; Refresh reloads the frontend without stopping those apps.

The Linux compositor advertises DMA-BUF v4 render-device feedback so embedded Kitty and app clients can discover the hardware GPU. Without that device identity, clients can silently select llvmpipe even while the compositor itself is accelerated. `scripts/profile-egl.py` reports a compositor's EGL renderer without creating a window; the ignored `wayland_client_discovers_hardware_gpu_without_a_window` diagnostic checks this against an isolated compositor. Periodic chrome uses SCREEN's 100ms cadence in both views, while live pixels retain their independent frame clock. Empty transparent foreground layers are omitted until needed by an overlay.

## Native terminal rendering

Terminals now follow the PTY/cell model used by Herdr: the backend tracks the terminal screen, sends styled row updates, and the host Kitty renders the actual text with its native font engine. A cell-aligned transparent opening in the graphical chrome exposes Kitty's text. There is no offscreen Kitty, Wayland compositor, screenshot readback, PNG delta encoder, or video decoder in the default terminal path. App space retains its separate virtual desktop, GPU renderer, SSH video and audio transport.

Selection, wide/combining characters, ANSI colors, bold, dim, italic, underline, cursor position, bracketed paste, menu keys, splits and file staging use the PTY state. Only changed rows cross the connection. Producer polling is independent of periodic chrome redraws. Newer Kitty bridge installations can adjust the current OS window's native font size from the header; older loaded watchers retain Kitty's own font-size shortcuts until the upgraded watcher is loaded. The installer versions watcher paths so a configuration reload actually loads the upgrade; Refresh UI then negotiates the native font controls. No global Kitty configuration is changed.

`scripts/test-native-terminal.py` runs a headless proof with a private fake shell, private tmux namespace and isolated state directories. It checks for native row delivery and zero terminal pixel frames, without opening windows or touching real agents. `STAR_TERMINAL_RENDERER=kitty-pixels` is an explicit compatibility/diagnostic choice rather than the default.

Sidebar context menus retain the main terminal view. Native terminal regions stay visible beneath graphical overlays; opening a menu changes input/cursor focus rather than hiding the terminal content.

## Multiple windows on one workspace

Open `starflux --workspace NAME` in more than one Kitty window, or attach with
`starflux --ssh HOST --workspace NAME`. Each connection gets a private frontend
controller. Project selection, terminal selection, splits, menus, viewport,
selection gestures, font controls, audio mute, and presentation/media credits
belong to that frontend. Refresh UI reloads only its own controller and restores
its view. Closing a window leaves the other windows and backend processes running.

Terminal processes, tab membership and names, application targets and the app
desktop belong to the shared project. Changes propagate to other views without
changing their selection. Project model edits use a per-project transaction and
three-way merging; new terminal IDs are allocated while holding that transaction.
Terminal generations prevent a delayed exit notification from removing a reused
slot. Terminal attachment sockets are private to their controller process.
Shared model files are separate from the old layout files, so a pre-upgrade
frontend cannot overwrite the new shared model with an old layout snapshot.

Each app desktop runs one compositor. Its transport broadcasts to bounded,
independent viewer queues, with separate shared-frame leases and acknowledgments.
The audio worker captures once and gives each subscriber its own credits and
connection lifetime. A slow or muted viewer does not replace another subscriber.
App input ownership transfers between viewers, releasing the preceding viewer's
held input; stale release events cannot release the current owner's keys.

A terminal has one underlying TUI screen. STAR-owned tmux sessions follow the most
recently active attachment's size. An inactive smaller window therefore does not
leave tmux's dotted unused-area filler around a larger cells frontend. Other
viewers clip the shared screen to their own viewport until they become active. The app desktop likewise has
shared geometry, negotiated from its viewers, while each frontend uses its own
placement and coordinate transform.

Refresh existing STAR/FLUX windows once to load this architecture. Existing app
workers retain their running applications: a worker created before this update
keeps its legacy single-viewer lease until it is restarted. New app workers
advertise multi-viewer support. No running application is restarted automatically.

`--inspect` includes a `frontends` array and each snapshot's `frontend` identity.
For management, `--frontend ID` targets a particular view. Without that selector,
`--close` closes all views opened for the named workspace while retaining its
terminals/apps. A model-changing `--action` executes once; shared metadata then
propagates to peers. Header controls always act on their own view.

Headless validation uses `scripts/test-multiwindow.py`: two protocol clients,
private fake shells and tmux servers, and isolated runtime/data directories. It
checks shared input, independent projects/menus/viewports/tabs/splits/font
requests, concurrent tab allocation, shared exits, safe slot reuse, window-local
refresh, disconnect isolation, and reattachment without restarting the shell.
Add `--ssh` to run one of those frontends over a private, temporary OpenSSH
server. The synthetic app-transport unit test verifies local/shared and remote/image
viewers, independent frame acknowledgments and disconnects without starting a
compositor, audio device, GUI window or real agent.

## Live migration out of Herdr

`--import-herdr` keeps Herdr as the terminal owner. To remove that dependency on a
Linux host, build the small helper with `scripts/build-pty-proxy`, then use
`starflux --plan-herdr-migration` to validate and back up the default session, or
`starflux --migrate-herdr` to prepare and apply the transfer.

The migration uses Herdr 0.9.1's Unix handoff version 1, protocol 22. It receives
the original PTY masters before the source releases them, stages native proxies
inside STAR-owned tmux sessions, and commits only after every proxy is connected.
The original shell and agent processes keep their PIDs, working state and native
conversations. tmux handles terminal parsing, history and multiple viewers; the
PTY owner waits on descriptor readiness and does not render or poll Herdr. The
proxy is a libc-only C executable with no periodic wakeup timer. Agent status
queries go directly to the native owner instead of launching Herdr CLI processes.

STAR/FLUX frontends refresh behind a migration gate while terminal ownership
changes. Legacy frontends receive a private view identity and restore their
selected project. Herdr UI-only clients for that session are retired; the server
exits through its handoff protocol. Its saved default layout is archived and
replaced by an empty layout only after commit, preventing an accidental Herdr
launch from restarting duplicate agents. Other Herdr sessions are untouched.

Backups, the live handoff manifest (including agent session references), screens,
original PIDs and ownership status are stored privately under
`~/.local/share/starflux/migrations/`. A pre-commit failure leaves the source owning
its original PTYs and resumes it. A post-commit failure retains the received
masters for diagnosis rather than restarting or discarding the user's processes.
Existing agent memory remains allocated because those agents remain alive.
Recent visible screens are carried into tmux; the agent's own conversation and
history remain in its existing process.

`scripts/test-herdr-adoption.py` uses an isolated Herdr server and fake shells,
without windows or real agent input. It verifies rollback after descriptor
receipt, original-PID preservation, old-server exit, post-transfer input/native
inspection, frontend restoration and retirement of the old restore snapshot.

Native Kitty presentation omits fully transparent chrome tiles instead of allocating image textures above terminal text. Graphics recovery batches visible tile uploads at most ten times per second; input, changed scenes, and app video retain their independent presentation cadence. Headless renderer tests cover overlay removal, lost-image recovery, and bounded recovery wakeups.
