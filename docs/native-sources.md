# Native terminals

Every STAR/FLUX terminal is a native terminal. STAR/FLUX owns its PTY, parses
its output and keeps its screen and history; the graphical frontend sends the
resulting text cells to Kitty, which draws them, and cells mode draws them in
the parent terminal. tmux is used only to import existing tmux sessions.

## Lifetime

A per-host owner process (`--native-source-service`) holds every native
terminal on that host. It starts in its own session, so quitting STAR/FLUX,
closing a window or losing an SSH connection leaves terminals running; the next
window reattaches to the same processes. Saved commands are never replayed on
reattachment. A compatible running owner is reused after STAR/FLUX is upgraded,
even when its executable file was replaced; a live owner is never stopped just
to load a new build.

Terminals do not survive a reboot or the owner's death. Agents come back
through their saved conversations instead (see [cold start](cold-start.md)).
When an owner dies, each terminal's supervisor retires its whole process tree
and writes a retirement certificate; the pane stays visible until you recover it
explicitly.

## Migrating from tmux

Terminals opened by older builds ran on a per-project tmux server. They migrate
to native terminals when they are idle:

- an idle shell reopens natively in the same directory, with its last 2000
  lines of scrollback replayed;
- an idle Claude Code conversation is stopped gracefully and resumed with
  `--resume` and its original launch options (never its original prompt).

Anything else, including a busy agent or a running command, keeps its tmux
session and migrates the next time it is opened while idle. Opening a pane in a
window migrates it automatically. `starflux --migrate-terminals` migrates every
idle terminal of projects that have no open window; add `--dry-run` to only
report what would happen. `scripts/test-tmux-migration.py` is the headless proof.

A migration records its plan on the host before it stops anything, so a native
launch that fails afterwards is retried from the plan the next time the pane
opens or `--migrate-terminals` runs.

The Arch package lists tmux under optional dependencies; the AppImage and the
Nix wrapper do not carry it.

## Processes and identity

Each terminal runs under its own supervisor (a subreaper). The terminal's
identity is the supervisor's PID and start time; a recognised foreground agent
has its own identity, and program and working-directory reporting follows the
verified foreground process. Automation input and termination check these
identities, so a reused PID never receives input meant for an old process.

Closing or recovering a terminal asks its process tree to exit, escalates
through pidfds, and waits until every descendant is reaped and the PTY reaches
EOF, including detached children and nested subreapers. When the root command
exits on its own, its remaining descendants are retired too.

**Recover to shell…** (or `panes.recover` with `confirm: "RECOVER"`) retires
the exact recorded terminal, then opens a fresh shell under a new terminal
generation. `panes.reap` removes an exited terminal; `panes.terminate` ends a
live one. All three take the pane's exact `epoch`. An explicitly registered
conversation stays linked across a deliberate recovery, so it can be resumed in
a new tab.

## Launches are never replayed

Each pane records its project and pane generation, owner identity, source
ID/generation and a digest of its launch (not the command or screen contents)
under the owner's data directory. The launch intent is written before the
command reaches the owner, and the owner keeps a receipt for every launch ID,
so a lost reply, a restarted frontend or a reconnect can only reattach. A
different command needs a new terminal generation. Input whose delivery was
uncertain is discarded, not resent; the reconnected terminal says so and asks
you to check your draft.

## Views

Views receive pushed, acknowledged row frames over a local or SSH connector.
There is no per-frame polling, and attaching a view does not create another PTY
or proxy shell. Hidden tabs stop watching; closing a view leaves the terminal
running.

A terminal has one size. It follows the most recently active view; a larger
view shows blank space rather than resizing the terminal back and forth. Each
view keeps its own palette.

Scrollback review asks the owner for retained history, up to 4000 rows and
2 MiB (see [terminal history](terminal-history.md)). History keeps colours and
OSC 8 links.

## Terminal features

- Inline Kitty graphics, including animation and Unicode placeholders, reach
  graphical views as lossless image planes; cells views show text only. See
  [inline graphics](inline-graphics.md) for what is refused (file and
  shared-memory transfers) and unsupported (frame composition).
- OSC 8 hyperlinks are forwarded to every view.
- Window titles (OSC 0/1/2) can name a tab unless you, an agent label or an
  import named it. Titles are limited to 4 KiB.
- OSC 52 clipboard writes reach attached views, latest value only, up to 64 KiB.
  Clipboard reads are never answered.
- Bells are counted but do not raise attention alerts.
- DCS, SOS and PM strings are ignored, so Sixel graphics and DCS queries such as
  XTGETTCAP get no response.

## Storage and environment

On each project host, `native-sources/` inside STAR/FLUX's data directory holds
`owner.json`, `token.json`, `broker.sock`, startup and lifetime locks, launch
receipts and per-pane records. It is separate from `$XDG_RUNTIME_DIR`, so
clearing runtime state does not orphan running terminals. The data directory
must be private; STAR/FLUX tightens one it owns to mode 0700 (older builds
created it 0755). Files are private, bounded and written atomically; symlinks,
hard links and replaced directories are rejected.

Discovery checks the owner's boot ID, PID, start time, instance, peer
credentials and protocol capabilities. An owner that is alive but unreachable
or incompatible is kept and reported, never replaced by a competing one.

Terminals inherit the owner's environment, which comes from the STAR/FLUX
process that started it. Because the owner outlives that process, each local
terminal also takes the desktop session variables (`DISPLAY`,
`WAYLAND_DISPLAY`, `HYPRLAND_INSTANCE_SIGNATURE`, `DBUS_SESSION_BUS_ADDRESS`,
`SSH_AUTH_SOCK` and similar) from the STAR/FLUX process that opens it, the way
tmux refreshed them on attach. Every terminal also gets `STARFLUX_WORKSPACE`,
`STARFLUX_PANE` and `STARFLUX_PANE_EPOCH`, which agent hooks use to report
status for exactly that pane. Terminals already running keep the environment
they started with. The owner drops one agent session's markers (`CLAUDECODE`,
`CLAUDE_CODE_*`, `CLAUDE_PID`, `AI_AGENT`, `STARFLUX_PANE`, …) and the
presenter's Kitty and tmux markers. Packaged builds restore the host's
`PATH` and remove the bundle's media and audio overrides.

Internal Linux entry points:

- `--native-source-service DATA_NAMESPACE` runs the owner.
- `--native-source-discover DATA_NAMESPACE` discovers or starts it and prints
  its non-secret identity.
- `--native-source-stdio DATA_NAMESPACE` discovers or starts an owner and
  carries bounded, versioned JSON Lines over stdin/stdout.
- `--native-source-attach DATA_NAMESPACE OWNER_JSON` connects only to that
  exact owner.

The connector adds authentication on the project host; callers never see the
token.

## Resource limits

Process-wide budgets cover queued input (16 MiB), presentation and history
working data (128 MiB), incoming wire data (192 MiB) and encoded output
(32 MiB), with 32 connector slots. Exhaustion reports busy; a terminal is never
restarted to free resources.

## Performance

Input-to-frame latency is about 18 ms median and 25 ms p95, roughly twice the
old tmux path; see [render performance](render-performance.md#input-to-frame-latency).

## Tests

All use private fake shells and agents, and SSH tests use a private loopback
daemon:

```sh
python3 scripts/test-tmux-migration.py
python3 scripts/test-automation.py --without-tmux [--ssh]
python3 scripts/test-session-manager.py [--cells] [--ssh] [--native-images]
python3 scripts/test-source-broker.py --service --streams --executable target/release/examples/native-source-broker --worker-executable target/release/starflux
```

The development checkpoints from the candidate period are in the
[native terminals development log](history/native-terminals-development.md).
