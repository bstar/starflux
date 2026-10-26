# App Shelf

STAR/MUX now uses the App Shelf direction from the design studies. A project owns its agent, terminal tabs, launch command and virtual app desktops. Agents and shells now use Kitty’s own renderer, presented at native pixel resolution. Graphical apps retain their private compositor. The chrome uses single pixel borders, a drafting grid and the selected STAR/KIT palette.

## Start a project

```sh
starmux --workspace my-app --directory /path/to/project --app 'cargo run --release'
starmux --workspace remote-app --host buildbox --directory /work/project \
  --app 'starfold .' --desktop-helper /home/you/.local/bin/starmux
```

The default agent is real Codex. `--agent 'bash -l'` or another command selects a different session command. New projects put their shells and agent on the project host. Existing workspaces keep their original session IDs and host assignments, including legacy local shells beside SSH agents. Migration does not respawn running agents.

Project metadata is saved privately under `$XDG_DATA_HOME/starmux/projects` (normally `~/.local/share/starmux/projects`). Runtime sockets and inspection snapshots remain under `$XDG_RUNTIME_DIR/starmux`.

## Work inside the project

- **Agent** opens the project's agent terminal. **Attach agent** reconnects to it, or restarts an exited agent after an explicit click.
- **Terminals** keeps shells in tabs. **+ Tab** adds a shell in the project directory. **Split** explicitly shows two shells; **Unsplit** retains both sessions. **Close pane** dismisses the focused half and leaves its terminal available in its tab; right-click a terminal tab for the same action. To end the shell itself, use `exit` or Ctrl+D at its prompt. Drag the separator to resize it. The ratio and selected tab are saved with the project.
- **Run app** executes the saved project app command. If no command is saved, it opens an editable dialog first.
- **Launch app** adds another application. Its command, directory, host and remote helper are editable. A supporting app can run locally or on another SSH host.
- App cards remain in the shelf while working with the agent or shells. Select a card to open that app's host desktop and focus its window.
- The virtual app space offers **Tile**, **Maximize**, **Close** and **Reconnect**. Application title bars and Alt-drag support moving windows; Alt-right-drag resizes. Each host has its own private desktop, so selecting an app also selects the correct host and input route.
- The persistent **Projects** sidebar lists project names and hosts in stable order, with the active container marked by an accent strip. Twenty projects fit at normal window heights; shorter windows use a scrollable list with a visible count and scrollbar. Switching keeps sessions alive. After releasing embedded input, `[` and `]` cycle saved projects. **+ New project** or `p` opens the new-project dialog.

Click a terminal or app to capture its input. **Ctrl+Esc** releases it. With input released: `1` selects Agent, `2` selects Terminals, `3` selects Apps, `n` adds a terminal, `a` launches another app, `r` runs the project app, `t` cycles themes, and `q` closes the workspace. Project and space changes release embedded keys and pointer buttons.

Dialogs support mouse field selection, Tab/Shift+Tab, ordinary editing, pasted text, Escape to cancel and Ctrl+Enter to accept. Commands are parsed as argument vectors. Use an explicit shell such as `bash -lc '...'` when shell syntax is needed.

## Working-project color wave

Working project names use bold accent text with a broad, brighter wave that sweeps back and forth across the letters. The working count also uses the accent color, so activity remains recognizable between wave peaks. Idle, completed and disconnected agents do not animate; a project that needs input has a static amber subtitle. The grid remains visible behind every row.

A small orbiting star appears beside each working project: a steady central spark with a bright point and fading trail moving around it. It uses the current theme and the same animation clock as the text. Reduced motion keeps the icon static; it disappears when the project is no longer working.

Imported Herdr activity is queried in one read-only batch per session, including projects that have not been selected. Native agents use explicit terminal state when available; Codex/Claude activity can also be read from their status footer on the project host. Only state metadata is returned, never terminal conversation contents. Footer recognition depends on the harness keeping its current status wording; unrecognized agents stay static rather than being marked busy just because their process is alive. Summaries refresh in the background, roughly every two seconds.

The wave completes a back-and-forth cycle in about 5.3 seconds only while a project is working. Decorative updates pause for 150 ms after typing, and stop entirely when no projects are busy. `STARMUX_REDUCED_MOTION=1` keeps the working count and a static text tint without animation. Unicode names retain their full text shaping and use a whole-name color pulse.

`python3 scripts/test-activity-wave.py` verifies changing working-name pixels, static idle/waiting rows, activity transitions, idle timer suspension, and reduced motion using a disposable Herdr API fixture. It does not attach to original Herdr sessions. `python3 scripts/test-agent-activity.py` checks native working/idle/approval detection against a disposable terminal fixture and verifies that only metadata leaves the helper. Evidence is in `target/activity-wave-proof`.

## Terminal response time

Terminal panes use a 60 Hz offscreen compositor. A short input-driven burst can capture newly committed output every 8 ms, avoiding the extra complete refresh interval between Kitty's frame callback and the compositor's capture. The burst ends after 150 ms. Unchanged frames are not copied. Graphical app desktops retain their independent 30 fps transport limit.

Embedded Kitty uses `sync_to_monitor=no`, `repaint_delay=4` and `input_delay=1`: its output is offscreen, while the presenting Kitty remains responsible for physical monitor synchronization. Font configuration and native pixel dimensions still come from Kitty. The path still includes GPU readback, bounded local frame files, and the outer terminal's image upload; it does not have standalone Kitty's direct presentation path. SSH adds the real connection's network delay.

`python3 scripts/bench-terminal-latency.py` measures RPC key injection to program receipt and a screen-capture-observed color update. It creates disposable windows at the same fixed size. Screen capture adds sampling delay, so these are software-path comparisons rather than hardware input-to-photon measurements. `STARMUX_TERMINAL_PACING=legacy` reproduces the previous 30 Hz pacing for an A/B run. `STARMUX_RENDER_TIMING=1` adds compositor renderer/readback and controller handoff timing to the workspace log. Samples and screen sizes are recorded in `target/terminal-latency-proof`.

## Multiple agents in a project

The **Agent** space has its own agent tabs and a **+ Agent** button. Each additional agent owns a separate persistent terminal session, starts in the same project directory on the same local or SSH host, and keeps running when another agent or project is selected. New Codex agents start with a fresh conversation; the original agent keeps its existing conversation and launch command. They share the project files, terminal tabs, Git view and app desktop.

Click an agent tab to switch; status dots show its health. Right-click to rename or focus the agent, or create another. When the strip fills, the arrow controls cycle through all agents. Names, selected agent and session identities survive closing/reopening the workspace. Additional agents and shell tabs use separate session IDs. **Attach agent** operates on the selected agent, and `--stop` cleans up every native agent in that project while preserving imported Herdr terminals.

Keyboard forwarding preserves uppercase letters and shifted punctuation in Kitty panes, including events where the terminal has already translated the character without a Shift modifier. Synthetic Shift is released after key-up. The experimental direct renderer uses the same Control/Alt modifier contract as the frontend.

`python3 scripts/test-multiple-agents.py` checks real GPU panes with two disposable agents per project, uppercase/Shift/punctuation and Control/Alt bytes, separate input, matching working directories, rename controls and persistent process IDs after controller restart. It covers local and authenticated loopback SSH projects without touching original Herdr sessions. Evidence is in `target/multi-agent-proof`.

## Project menus and repository state

The wordmark is **S T A R / M U X**. Right-click a project in the sidebar or its header to rename it, open its agent, terminals or app space, or create another project. Right-click a terminal tab to rename it, focus it, split/unsplit, or add a tab. Menus use the active theme, single borders and inset selection. Use arrows and Enter, or the mouse; Escape and outside clicks dismiss without passing input through to a session.

Project and terminal names are durable display labels. Renaming retains workspace/session IDs and running processes; imported Herdr names remain unchanged. Names accept up to 64 characters, including Unicode.

The active project header shows its Git branch, staged/unstaged/untracked file counts, tracked additions/deletions and ahead/behind counts. Click that status line to open the working-tree inspector: separate staged and unstaged line totals, file-by-file index/worktree status, rename paths, conflicts, merge/rebase state, stashes, upstream and the latest commit. Scroll the file list; **r** refreshes and **Escape** closes. Untracked contents are counted as files and excluded from line totals. Binary changes are included in changed-file counts.

Repository inspection runs in the background on the project's configured host, locally or through authenticated SSH, and refreshes after three seconds. Requests are time bounded and do not write the Git index, stage files, commit, or fetch. Ahead/behind reflects locally known upstream refs. Each project keeps its own status cache; switching projects does not display another project's result. Missing repositories and inspection/SSH failures have explicit states.

`python3 scripts/test-git-status.py` verifies mixed staged/unstaged changes, unusual paths, renames, binary changes, conflicts, unborn/detached branches, upstream counts and unchanged index bytes. `python3 scripts/test-project-tools.py` checks the real GPU menus, Unicode renames, restart persistence, retained session PIDs, isolated inspector input, background refresh and authenticated loopback SSH Git inspection. Screenshots and results are in `target/project-tools-proof`.

## Kitty-rendered local and SSH terminals

The default is actual Kitty rendering. Terminal panes load the normal Kitty configuration, including the selected font, font features and text-composition settings. Their private compositor renders at the exact terminal panel dimensions; the former 1440px height cap is removed from terminal views. Local frames are delivered as lossless pixels at 1:1 scale, without re-rasterizing glyphs in STAR/KIT. Obsolete-size frames are withheld while resizing. The sidebar, project state, agent/terminal tabs and app desktops remain shared across backends. A GPU copy can quantize an antialiased color by one 8-bit level, but glyph shapes and placement come from Kitty itself.

**A− / size / A+** controls Kitty’s font size; reset restores the presenting terminal’s inherited physical size. Agent and SSH session processes remain separate from the renderer and survive frontend restarts.

`python3 scripts/test-kitty-pixels.py` compares standalone Kitty and embedded Kitty text captures, checks native frame/panel dimensions, and records glyph shape/coverage comparison and raw RGB differences in `target/kitty-pixel-proof`.

## Selecting and copying Codex text

Use a normal **left-button drag** in an agent or terminal pane. S T A R / M U X distinguishes a drag from a click, uses Kitty's text selection even when the application captures the mouse, and copies the selected text to the presenting desktop clipboard on release. No Shift gesture or copy shortcut is required. Single clicks still reach Codex menus and other terminal controls. Hold **Ctrl** when you want an application drag instead of text selection.

Selection stays attached to the pane where it began, including split panes. Clipboard reads run asynchronously through that pane's private Kitty socket; SSH-backed Codex sessions still copy to the local desktop. Font rendering and selection highlighting come from Kitty. Graphical app-space input keeps its native behavior.

`python3 scripts/test-text-selection.py` checks Unicode selection and the local desktop clipboard against a mouse-capturing agent fixture, locally and through authenticated loopback SSH, and verifies that clicks still reach the agent. `python3 scripts/test-codex-selection.py` selects text in a disposable real Codex session without submitting a prompt or answering an approval. Evidence is in `target/selection-proof` and `target/codex-selection-proof`.

## Experimental direct terminals

Use `STAR_TERMINAL_RENDERER=direct` to opt into the experimental direct renderer. It uses a different glyph rasterizer and cannot guarantee Kitty’s exact font appearance. Terminal bytes come from a local PTY attached to the existing tmux or Herdr session; SSH terminals use that same PTY with an SSH attachment. STAR/KIT draws the cell grid as native text and shapes it through its selected CPU/GPU frontend. Terminal panels require neither an inner Kitty process nor a private display server, pixel capture or terminal frame encoding. The outer Kitty window remains the frontend's presentation host.

The Agent and Terminals headers offer **A−**, the current pixel size, and **A+**. By default the direct terminal inherits the presenting Kitty window’s active font face, fractional pixel size (including DPI) and measured cell dimensions. Click the current size to return to that inherited size. Explicit zooms range from 10–32px and are saved per project, shared by its agent and shell tabs. Grid dimensions, cursor placement and mouse coordinates use measurements from the selected font at each size. Untouched legacy 16px defaults migrate to inheritance; other saved zooms are retained.

Menus use normal/application cursor keys, modifier keys and function keys. The emulator supports alternate screens, ANSI/indexed/RGB colors, bold, underline, inverse video, wide/combining text, bracketed paste, cursor queries, mouse reporting and bounded scrollback. Drag to select text; releasing the mouse copies it to the desktop clipboard. A plain drag selects even when the application owns the mouse; Ctrl+drag preserves application mouse handling. Scrollback is local to an attachment; tmux/Herdr remain the persistent source of session history. Tabs, splits and project switching retain independent PTYs. No approval or Enter is sent when a file path is delivered.

The initial renderer uses vt100 0.16 and portable-pty 0.9. The installed KIT candidate negotiates font metrics from the presenting computer, so SSH hosts need no local copy of the font. It shapes complete styled runs to retain ligatures and combining marks, placing their clusters on measured cell boundaries. KIT and Kitty still use different rasterizers; identical font selection does not promise identical antialiasing. Older KIT frontends without typography negotiation retain compatibility sizing. `STAR_GRAPHICS_FONT` and `STAR_GRAPHICS_FONT_SIZE` remain explicit overrides; the launcher no longer forces Liberation Mono. `STAR_GRAPHICS_FONT_DEBUG=1` reports the probe, resolved family and capability handoff for diagnosis. It does not implement Kitty image graphics or the Kitty enhanced keyboard protocol, and italic text currently uses the normal face. To explicitly select the default Kitty backend, close/reopen the workspace with:

```sh
STAR_TERMINAL_RENDERER=kitty STAR_GRAPHICS_RENDERER=gpu starmux --workspace my-app
```

Renderer selection is made when the controller starts. A running controller keeps its renderer until closed and reopened; backend sessions stay alive. `--inspect`/runtime snapshots identify the active renderer, PTY attachment PIDs and direct screen text.

`python3 scripts/test-direct-terminal.py` checks a real GPU frontend with local and authenticated loopback SSH sessions: menu selection, Unicode bracketed paste, hashed file staging without Enter, split panes, font/geometry changes and stable backend PIDs after reconnect. Evidence lives in `target/direct-terminal-proof`. A separate disposable real Codex test verified `/model` and arrow selection, then cancelled without changing its model (`target/direct-codex-proof`).

## Local and SSH apps

Graphical applications run in a persistent private compositor on their own host. The workspace receives bounded PNG frames and sends input over the same framed transport locally or through OpenSSH. The app helper negotiates a protocol version, preserves stable window identities and retains only the newest frame. Hover motion is coalesced separately from ordered controls.

For SSH graphical apps, install a compatible STAR/MUX executable on that host, plus its runtime libraries, `dbus-run-session`, Xwayland and a usable EGL graphics device. `--desktop-helper` selects the executable or runtime wrapper on that host. No forwarding into the host's normal desktop is used. A private D-Bus session prevents applications from handing their launch to an existing desktop instance. Ordinary SSH agent terminals continue to need only Python 3, tmux and the agent command.

Closing or killing a frontend does not terminate the app compositor. **Reconnect** and reopening the workspace attach to the same app processes. A host reboot or explicit `--stop` ends them. Failed remote startup is reported in the workspace; the agent and shell spaces stay available.

The first app-desktop transport is lossless, capped at 2560×1440 and at most 30 changed frames per second. Apps use a 33 ms frontend update interval. GPU readback and PNG encoding still run through CPU memory; sustained game frame rates depend on resolution, host and SSH bandwidth. Audio transport and hardware video encoding remain later work.

## Agent launches on the app host

An agent running on the app host can open a window without taking over the viewer or configuring DISPLAY:

```sh
starmux --app-launch my-app --app-directory /work/project -- ./game
starmux --app-list my-app
```

The identity must match the viewer's project workspace. This uses a separate local control socket, waits for a new window and reports it as JSON. A viewer may remain attached throughout. The worker injects its private display and D-Bus environment. Run this command through SSH on a remote app host; configure the project's app host/helper in **Launch app** so the local viewer connects to that same desktop. The helper must be installed on that host. `--app-list` starts an empty desktop if none exists.

The Zendocon checkout includes `python3 tools/run_in_starmux.py` (default desktop `herdr-default-w8`) and `docs/starmux-app-space.md`. It launches the existing cartridge, without rebuilding or replacing the game.

STAR/KIT's GPU frontend is opt-in with `STAR_GRAPHICS_RENDERER=gpu starmux ...`. The installed candidate includes GPU support. Kitty and the private app compositor have independent renderers; enabling the frontend does not enable hardware video encoding or make emulator logic run on the GPU.

## File delivery

Dropping onto an agent or terminal freezes the project, host and session at the drop. Files are copied, hashed and staged on that host before the destination path is pasted without Enter. Switching projects during transfer does not redirect delivery. This retains the Codex attachment path used by the earlier desktop acceptance tests.

An app-space drop stages the file on the selected app's host and offers `text/uri-list` through that app's clipboard with a paste gesture. Apps must support that MIME type to consume it. This is not native Wayland drag-and-drop emulation for every GUI toolkit. Text pasted from the outer terminal also crosses the app transport; exporting an app's clipboard to the outer desktop is not implemented yet.

## Inspect, detach and stop

```sh
starmux --workspace my-app --inspect
starmux --workspace my-app --action view:apps
starmux --workspace my-app --action terminal:new
starmux --workspace my-app --close
starmux --workspace my-app
starmux --workspace my-app --stop
```

`--close` detaches the controller while retaining tmux sessions and app workers. `--stop` explicitly stops that named project's terminals and app desktops. Other projects are retained. A project lease prevents two updated controllers from simultaneously resizing or typing into the same project.

Already-running controllers keep the executable they started with. To adopt App Shelf for an existing workspace, close its workspace frontend and reopen the same name. Its agent processes remain alive.

## Validation

`python3 scripts/test-app-shelf.py` uses disposable local and SSH projects, real Kitty, actual GTK applications and a private authenticated loopback SSH server. It checks terminal tabs and splitting, multiple apps, keyboard input to the SSH app, project isolation, process identity after app reconnect, and compositor/window identity after closing and reopening the workspace. Screenshots and results are written to `target/app-shelf-proof`.

The transport has been checked with loopback SSH; this does not establish latency or graphics-device compatibility on a separate remote host. Broader lifecycle and release hardening stay in `TASKS.md`.

## Import an existing Herdr session for testing

`starmux --import-herdr` imports the default Herdr session into a separate `herdr-default` collection. It preserves workspace labels/order, directories, tab labels and terminal identities. The import prints the workspace to open. Native test/proof projects are excluded from this collection.

Imported panes use `herdr terminal attach` against the existing terminal ID through Kitty by default, or a direct PTY when explicitly selected. They are interactive links to the same running shells and agents, not restarted or copied conversations. Herdr must remain running. No `--takeover` flag is used. The Agent shortcut and matching terminal tab share one attachment so they do not compete for input ownership. Pasted text uses the attached Kitty, or the direct PTY paste socket in experimental direct mode, with bracketed paste and no automatic Enter. Stopping the STAR/MUX project never stops its original Herdr terminals.

The project sidebar retains stable order and keeps the collection visible. Wheel scrolling in a shorter list does not change the active project. The test import contains 20 workspaces, 24 original tabs/panes and 16 detected agents. The initial read-only mirrors were replaced after input testing; those mirrors are no longer the normal imported-terminal path.

`python3 scripts/test-handy-ssh.py` uses the local Zendocon checkout and a disposable authenticated loopback SSH server. It checks sideband launch with an attached viewer, cartridge rendering, keyboard start, changing game frames and stable compositor/window identity across reconnect. Handy replaces its initial SDL window during cartridge initialization; input targets the resulting game window. Evidence is saved in `target/handy-ssh-proof`.

`python3 scripts/test-font-inheritance.py` records real Kitty font/DPI queries and compares them with the negotiated terminal sizes/cells for both the normal user configuration and a Liberation Mono override. Native Kitty and STAR/MUX screenshots are saved in `target/font-inheritance-proof`.

`python3 scripts/test-project-sidebar.py` uses 20 disposable projects with imported labels to verify that all project rows remain visible, the final row can be selected directly, and switching preserves order. Screenshots and results are in `target/sidebar-proof`. It does not attach to or type into original Herdr sessions.
