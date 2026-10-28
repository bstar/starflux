# Cells and graphical presentations

`starmux --cells` runs directly in the launching terminal. Kitty, a desktop display,
and a private compositor are not needed for this presentation. Embedded terminals
are persistent PTY sessions rendered into the parent terminal's cell grid, using
its font and cell size. Cells are text, rather than terminal screenshots.

```sh
starmux --cells --workspace my-project --directory /path/to/project
starmux --cells --workspace remote-project --host user@host --directory /work/project
starmux --cells --ssh user@host --workspace existing-project
starmux --graphical --workspace my-project
```

Without either flag, the displaying frontend prefers graphical presentation when
STAR/KIT supports it and falls back to cells. A launch without an interactive
terminal retains the desktop Kitty launcher. `--cells` requires an interactive
terminal; `--graphical` reports an error when graphical presentation is unavailable.
The flags conflict. Presentation selection survives Refresh UI and is independent
for each window; cells and graphical windows can share the same workspace.
A shared terminal follows the most recently active attachment's dimensions, so an
older, smaller window does not constrain the active window's terminal area.
Only visible panes attach to the backend; hidden tabs and projects keep their
processes and monitoring without creating default-size clients that shrink a
visible split. Selecting a hidden tab reconnects it at its pane's dimensions.

Both modes use the same project and terminal model. Closing a frontend keeps its
sessions running. Reattach with the same `--workspace`; use `--stop` only to end
project processes explicitly. SSH project terminals and `--ssh` workspace attachment
retain the existing Python/tmux/OpenSSH requirements. The shipped binary still links
its graphical dependencies; cells mode does not start those graphical services.

## Projects

Use **Add project** at the bottom of the project sidebar to create a project or
reopen one by its saved name. In narrow windows, open the sidebar with **Projects**. Existing names retain their saved host, directory, tabs and running processes.
Use **Close project**, the **x** beside a sidebar row, or the project context menu to
remove a project from this window. Closing never stops its terminals or agents.
Closed rows stay hidden through refresh and catalog updates. Other windows keep
independent project lists. Closing the last project closes that frontend.

With workspace controls focused, **+** (or **p**) opens Add project and **x** closes
the selected project. Press **F6** first if a terminal currently has keyboard focus.

## Keyboard and mouse

Typing and control keys go to the focused terminal, including keys that are workspace
shortcuts when controls have focus. Click a terminal to focus it. Use **Ctrl+Esc** or
**F6** to release input to workspace controls; F6 also works in terminals that cannot
distinguish Ctrl+Esc. **Escape** or **F6** returns from controls to the terminal.

| With workspace controls focused | Action |
| --- | --- |
| Tab / Shift+Tab | Move through visible controls, projects and tabs |
| Enter | Activate the focused control |
| `n` | New terminal |
| `+` / `p` | Add or reopen a project |
| `x` | Close the selected project, keeping sessions running |
| `[` / `]` | Previous / next project |
| F2 | Rename the selected terminal tab |
| `g` / `t` / `f` | Git details / next theme / deliver a file |
| Ctrl+Up / Ctrl+Down | Move the focused project in Manual ordering |
| Ctrl+Left / Ctrl+Right | Resize the terminal split |
| `q` | Close this frontend, retaining sessions |

**F5** refreshes the UI. Dialogs keep input until accepted or dismissed. Tab moves
between fields; Enter advances to the next field and accepts the final field;
Ctrl+Enter accepts immediately. Escape cancels. Menus use Up/Down and Enter.
Git details use Up/Down or PageUp/PageDown to scroll, `r` to refresh, and Escape to close.

Mouse support is optional. Clickable controls and project names use a hand pointer
on terminals supporting OSC 22. Right-click projects or terminals for compact menus
at the pointer, with hover highlighting; click outside or press Escape to dismiss.
The terminal stays visible behind the menu. Drag project rows in Manual ordering, and drag the split separator to resize.
Drag inside a terminal to select text; releasing copies through the parent's clipboard
protocol. Shift selects text in applications that enable terminal mouse reporting.
The terminal's clipboard permissions still apply. Wheel scrolling over a terminal
uses persistent backend history, including output from before attachment or refresh.
Wheel scrolling over the sidebar scrolls the project list.

The sidebar appears automatically at 100 columns. Longer project lists scroll with
the mouse wheel; the Add project button stays pinned at the bottom. At narrower sizes, **Projects**
opens it. The tab strip scrolls to keep the selected terminal visible. Below 60×16,
a resize message replaces the interface and processes continue running.

## Feature parity

| Feature | Cells | Graphical |
| --- | --- | --- |
| Persistent local/SSH terminals and agent activity | Yes | Yes |
| Project creation, renaming, ordering and sorting | Yes | Yes |
| Terminal tabs, renaming, splits and retained panes | Yes | Yes |
| Themes, context menus, dialogs and Git details | Yes | Yes |
| Selection, copy and paste | Parent terminal capabilities | Yes |
| File delivery | File-path dialog, CLI, supported drop protocols | Existing delivery protocols and CLI |
| Terminal font | Inherits parent; change font in parent | Existing font controls |
| App Space, app launch UI and app audio controls | Hidden | Available with graphical frame support, including SSH |

The file-path dialog accepts a regular file **on the workspace host** and uses the
existing upload-and-paste operation for the selected project's host. For a workspace
attached with `--ssh`, this is a path on that remote workspace machine, not the local
displaying machine. It pastes the destination reference without submitting it.
Local CLI delivery remains available as `starmux --workspace NAME --deliver PATH`.

App Space requires a graphical canvas. It is hidden in cells mode and in frontends
that advertise no local or streamed app frames. Supported graphical frontends,
including Kitty over SSH, expose **App Space** in the terminal header and
**Open App Space** in the project context menu. Availability is negotiated for each
frontend, so a cells window does not disable apps in another graphical window.

Saved app commands, targets and workers are retained. Cells views restore terminal
space; graphical views may restore App Space. An app desktop reconnects when
entering App Space, including after a refresh of an app view. Merely opening a
terminal workspace does not start an app worker. Explicit `--app-launch`,
`--app-list` and `--stop` remain available for managing desktops. Unsupported
frontends reject stale app UI actions.
