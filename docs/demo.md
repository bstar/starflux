# STAR MUX development demo

The current application uses [App Shelf](app-shelf.md): project containers, agent and terminal spaces, terminal tabs with explicit splitting, and persistent local/SSH application desktops. The two-pane descriptions below document the earlier embedding baseline. Follow the App Shelf guide for the current workflow.

The demo contains a STAR/KIT graphical workspace, two actual Kitty terminal surfaces, a persistent terminal backend, local and SSH agent launch, and file delivery to the pane under a desktop drop. It runs inside an outer Kitty window and uses a private Smithay compositor for the inner Kitty windows.

## Build and install

Use the pinned Nix development shell. No sibling checkout or AUR packages are required.

```sh
./scripts/dev build --release
./scripts/prepare-runtime
ln -s "$PWD/scripts/run-local.sh" "$HOME/.local/bin/starmux"
starmux --version
```

Reuse an existing correct link rather than replacing another command. The runtime wrapper loads the Nix graphics library paths, and the development shell profile keeps their store paths rooted. The executable remains in `target/release/starmux`.

Runtime requirements are Linux, Wayland, an EGL device usable by Smithay, Kitty with graphics and OSC 72 file-drop support, Python 3, and tmux. Remote hosts need Python 3, tmux and the chosen agent command; no remote graphical desktop or installed STAR/MUX binary is required. The test harness uses a STAR/MUX fixture remotely, but normal remote agent launch sends only the embedded Python host helper.

The installed Kitty 0.48.2 successfully negotiates OSC 72. Graphics support alone does not establish file-drop support in another terminal build.

## Launch sessions

```sh
# Local shell plus real Codex, using your existing Codex setup.
starmux

# Shell plus local Codex.
starmux --workspace codex --agent codex

# Local shell plus Codex on another Linux host.
starmux --workspace remote --host user@host --directory /path/to/project --agent codex

# Honor an alternate OpenSSH configuration, including aliases and jump hosts.
starmux --workspace work --host workbox --ssh-config ~/.ssh/work-config --agent codex
```

When launched from outside Kitty, STAR/MUX opens a Kitty window. Inside Kitty it uses the current terminal. The first pane stays local. The second pane runs on the selected host. The directory option selects the second pane's working directory on its host. Agent commands are parsed as an argument vector; use an explicit shell command only when shell syntax is intended.

Each ordinary `starmux` launch generates an independent workspace, so different tabs never compete for a controller. `starmux --sessions` lists saved names; `starmux --workspace NAME` reconnects to one. Only one frontend may control a named workspace at a time; a second attachment is rejected instead of stealing the first tab.

Workspace names identify saved launch configuration and isolated tmux sessions. Reopening a name reuses its sessions. A conflicting explicit host, directory or agent produces an error instead of silently switching an existing session. Stop that workspace or choose another name to change its configuration.

Normal launches open a local shell and real Codex. Use `--agent "bash -l"` for a second shell or pass another harness command explicitly. Existing real processes are retained. When reopening a legacy local workspace with Codex already running in the first pane, the verified old demo pane becomes a shell instead of starting a duplicate Codex. Pane labels follow the actual process. The fixture is reserved for explicit internal acceptance tests. Agent panes display process health; complete Codex lifecycle and approval integration is still a later task.

## Drop a file into an agent

Drag a regular file from the desktop file manager onto the desired terminal pane. The destination is frozen at the drop, independently of later keyboard focus. STAR/MUX copies and hashes its bytes, stages the file privately on the destination host, commits the complete file, and pastes its destination into the existing prompt. Originals remain intact. For the tested Codex version, a dropped PNG becomes an image attachment in the composer.

Transfers show byte progress. Use **Cancel transfer**, or release embedded input with **Ctrl+Esc** and press **Esc**, to cancel an active transfer. Partial uploads are cleaned after EOF. A file committed just before cancellation can remain retained, but the canceled batch is not subsequently pasted. There is no automatic prompt submission.

The demo accepts up to 32 regular UTF-8-named files per offer and at most 64 MiB per file. Directory, symlink, clipboard-image, promised-file and cross-machine URI offers need further integration. Multiple ordinary paths are supported by the transport; each harness's interpretation of multiple image paths still needs its own acceptance check. The actual Codex image check covers a single PNG.

The destination must be readable by the agent. A separate container or mount namespace may require a mapped attachment directory. Uploaded files are retained under the destination host's `$XDG_DATA_HOME/starmux/attachments`, or `~/.local/share/starmux/attachments`; this demo does not automatically expire conversation attachments.

For scripted delivery through the same host upload and paste implementation:

```sh
starmux --workspace remote --deliver '/local/path/screenshot.png' --pane 2
```

## Close and reconnect

```sh
starmux --workspace remote --inspect
starmux --workspace remote --capture --pane 2
starmux --workspace remote --close
starmux --workspace remote

# Explicitly end both sessions and clear their launch configuration.
starmux --workspace remote --stop
```

Closing the frontend or workspace preserves tmux-owned shells and agents. The controller runs in its own process group, so presentation loss does not kill it. If an inner Kitty or its SSH terminal client exits, the controller attempts to reopen that view after five seconds. Inspect reports live session health rather than treating its cached graphical snapshot as current process state.

The demo uses the authenticated OpenSSH transport for attach, inspection and transfers. It does not pool every control operation into one connection. Use SSH keys or an existing SSH agent for background operations; interactive password entry for each background helper is not implemented. Host identity checking follows OpenSSH configuration.

Host reboot ends the original processes. Layout restore after reboot, native harness resume, terminal image reconstruction, daemon upgrades and transparent compositor recovery are production backlog items. Runtime workspace configuration currently lives under `$XDG_RUNTIME_DIR/starmux`, so it is not a reboot-persistent project store.

## Verify the demo

```sh
python3 scripts/test-host.py
./scripts/dev test --release
./scripts/dev clippy --all-targets --release -- -D warnings -A dead_code

# Creates temporary SSH keys, a loopback daemon and real desktop windows.
# Injects mouse input on the current Hyprland desktop and restores focus/cursor.
python3 scripts/test-desktop.py --inject-host-mouse

# Additionally runs an existing real Codex TUI and submits one image question.
python3 scripts/test-desktop.py --inject-host-mouse \
  --codex /absolute/path/to/codex \
  --name 'screen shot λ.png' --output target/codex-proof-final
```

The desktop proof requires Nautilus, OpenSSH server tools, Hyprland, grim and writable `/dev/uinput`. It creates private floating fixture windows without changing desktop configuration or the user's SSH configuration. Codex verification uses available authentication, a disposable directory, a read-only sandbox, and one explicit test submission. The test report identifies loopback SSH rather than claiming a separate-machine or WAN check.

On the tested environment, a genuine Nautilus drop crossed Kitty, STAR/MUX and SSH into an already-running Codex 0.162.1 TUI. The composer showed `[Image #1]`; Codex answered “Green” for the fixture image. The proof also verifies process identity across SSH terminal disconnection and workspace closure. The final regression additionally checks presenter death and reattachment and a filename with spaces and Unicode.

Artifacts are written below the selected output directory: rendered screenshots, composer and answer captures, sanitized logs, and `result.json`. Remaining production requirements are tracked in [TASKS.md](../TASKS.md); these demo checks do not mark the entire M0 comparison or the release matrix complete.

## Tab refresh regression

The initial demo attached every unnamed launch to `demo`. Concurrent tabs repeatedly replaced each other and reconnected, alternating the controller viewport and rendered frames. Unnamed launches now create independent workspaces. A relay holds a per-workspace attachment lease, and new controllers reject simultaneous controlling frontends.

`python3 scripts/test-tabs.py` verifies independent controller and process identities across real Kitty tabs, eight tab switches, explicit duplicate-attachment rejection, and reattachment after closing a tab. The regression report is in `target/tab-proof.json`.

## GPU candidate

The installed local build includes the unreleased sibling STAR/KIT GPU candidate. CPU remains the default. To select hardware rendering:

```sh
STAR_GRAPHICS_RENDERER=gpu starmux
# Prefer hardware, with a CPU fallback if unavailable:
STAR_GRAPHICS_RENDERER=auto starmux
```

Each ordinary launch creates a new workspace. Use `--workspace NAME` to reattach an existing session after closing its previous frontend. Existing controller processes keep their original executable until restarted; do not stop a workspace merely to change its frontend renderer.

To reproduce the local candidate build without changing the portable Git dependency or Cargo.lock:

```sh
STAR_KIT_CHECKOUT=/home/bstar/projects/star/starkit STAR_KIT_GPU=1 ./scripts/dev build --release
./scripts/prepare-runtime
```

The ordinary build command continues to use the portable pinned framework. Candidate builds use a disposable manifest and lockfile. Hardware rendering accelerates scene composition; text coverage, GPU readback and Kitty transport still cost CPU time. See [the framework GPU documentation](../../starkit/docs/gpu-renderer.md) for API details and measurements.

`STAR_GRAPHICS_RENDERER=gpu python3 scripts/test-tabs.py` passed with two actual Kitty tabs. The GPU build also passed a desktop PNG drop into real Codex 0.162.1 over authenticated loopback SSH, image recognition, frontend kill and reattachment with preserved backend PIDs. Reports are `target/tab-proof.json` and `target/gpu-codex-proof/result.json`. This does not yet establish behavior on a separate remote machine.

## Focused Codex drop and exit cleanup

The desktop acceptance test now clicks the embedded Codex pane before dragging a PNG from Nautilus. It checks that the composer receives `[Image #1]` without automatic submission or protocol text, explicitly asks for the square color, and checks Codex's answer. A parent-terminal monitor also verifies that a subsequent drag after frontend closure receives no OSC 72 reports. The framework cancels pending data requests and unregisters the drop target before restoring ordinary terminal input, on both normal and error exits.

```sh
STAR_GRAPHICS_RENDERER=gpu python3 scripts/test-desktop.py --inject-host-mouse --codex /home/bstar/.local/share/mise/installs/codex/0.162.1/bin/codex --name 'screen shot λ.png' --output target/codex-focused-drop-proof
```

This passed with Codex 0.162.1 and authenticated loopback SSH. It verifies image attachment delivery to real Codex in STAR/MUX; a separate remote machine remains an additional acceptance check. The script uses its own file-manager process group and restores desktop focus and the cursor after testing.

## Held drag under backpressure

The frontend's bounded input outbox now coalesces complete OSC 72 hover reports, carrying forward the MIME offer when Kitty omits it from later positions. It retains drop and transfer-data messages, chunk boundaries, client identities, viewport generations and enter/leave transitions. Ordinary keys and other ordered controls remain bounded and ordered.

The framework regression stalls the writer and sends 10,000 hover updates before the drop; it verifies the retained MIME offer, final position, drop ordering and writer wakeup. The desktop test below holds a real drag for 4,000 motion events over a 1500×1900 GPU workspace before dropping into focused Codex over authenticated loopback SSH. It verifies attachment without automatic submission, image understanding, reconnects, and no protocol leakage after exit.

```sh
STAR_GRAPHICS_RENDERER=gpu python3 scripts/test-desktop.py --inject-host-mouse --codex /home/bstar/.local/share/mise/installs/codex/0.162.1/bin/codex --hover-events 4000 --tall-workspace --name 'screen shot λ.png' --output target/codex-hover-proof
```

The desktop proof passed; the artificially stalled writer regression covers timing pressure that the desktop alone may not reproduce on a fast local connection.

## Markdown-file and mixed-event regression

Hover coalescing also crosses interleaved position streams and frame/effect acknowledgments. It stops at keys, buttons, drops, data, client changes and geometry boundaries; retained input IDs remain increasing. A stalled-writer test covers 10,000 mixed hover/mouse updates with presentation acknowledgments. Queue overflow errors include message-type counts to diagnose remaining source-specific failures without logging typed text or file contents.

```sh
STAR_GRAPHICS_RENDERER=gpu python3 scripts/test-desktop.py --inject-host-mouse --codex /home/bstar/.local/share/mise/installs/codex/0.162.1/bin/codex --text-file --name AGENTS.md --hover-events 4000 --tall-workspace --output target/codex-markdown-proof
```

This passed from a private Nautilus window to focused Codex over authenticated loopback SSH. The drop inserted a staged path without submission. After an explicit prompt, Codex read the Markdown file and returned its test sentinel. A successful isolated run does not establish the cause of every source-app-specific queue overflow; preserve the new message counts if another failure occurs.

## Blueprint presentation and themes

Blueprint is the default live treatment, implemented from `design/treatments.html`: square one-pixel edges, dashed inactive frames, the drafting grid between terminals, monospaced headings, a 42.5/57.5 terminal split, and a separated outer focus rule. Physical-pixel geometry and a shared layout drive terminal placement, hit testing and file-drop targeting. Text is shaped at its destination size rather than scaled from a screenshot. The runtime wrapper selects Liberation Mono to match the design study.

```sh
starmux --themes
STAR_GRAPHICS_RENDERER=gpu starmux --agent codex --theme kanagawa
starmux --workspace remote --theme catppuccin-latte
```

The last command changes a running workspace's palette without opening a duplicate frontend. Click the theme label in the top bar, or release embedded input with Ctrl+Esc and press **t**, to cycle palettes. The selected theme is stored with the workspace configuration. Chrome, terminal backgrounds, foregrounds, cursors, selections and ANSI colors update together through private Kitty sockets. There are sixteen shared STAR/KIT built-ins plus `system`; system uses KIT's supported desktop detection and falls back to Kanagawa. Existing shell/agent processes are preserved during palette changes. Applications emitting their own true-color escapes still own those colors.

Controllers started before the Blueprint build need to be closed once and reopened to load the new presentation; their tmux-backed agents survive:

```sh
starmux --workspace NAME --close
STAR_GRAPHICS_RENDERER=gpu starmux --workspace NAME
```

`python3 scripts/test-blueprint.py` exercises the live GPU view, dark/light palette changes, the inner Kitty colors, keyboard theme control and actual terminal input. It records screenshots in `target/blueprint-proof`. Ten Rust tests check the measured HTML geometry, square bounded surfaces through 4K and portrait sizes, readable theme roles, legacy workspace compatibility and existing transfer behavior. The pinned portable CPU build also passes `./scripts/dev check --all-targets --locked --offline`.

The real Codex test also passes under Blueprint: a held 4,000-event drag of `AGENTS.md`, remote staging over authenticated loopback SSH, explicit submission and Codex reading the file's test value. Evidence is in `target/blueprint-codex-proof`. The design uses actual process/connection state; the mock gallery's simulated approval state and artifact count are not presented as real harness integrations.
