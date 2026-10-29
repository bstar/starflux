# S T A R / F L U X

[![CI](https://github.com/bstar/starflux/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/bstar/starflux/actions/workflows/ci.yml)
[![Release builds](https://github.com/bstar/starflux/actions/workflows/release.yml/badge.svg)](https://github.com/bstar/starflux/actions/workflows/release.yml)
[![Arch package](https://github.com/bstar/starflux/actions/workflows/arch.yml/badge.svg)](https://github.com/bstar/starflux/actions/workflows/arch.yml)

An agent workspace built with STAR/KIT, with persistent local and SSH terminals,
a graphical interface, and a full text-cell interface for ordinary terminals.

```sh
starflux --cells --workspace my-app --directory /path/to/project
starflux --cells --ssh user@host --workspace my-app
starflux --graphical --workspace my-app
```

Automatic presentation prefers graphical mode where supported and falls back to
cells. Cells mode runs in the parent terminal, using its font and cell grid.
**Ctrl+Esc** releases terminal input to workspace controls; **F6** toggles focus.
Cells mode has **Add project** and **Close project** controls; with controls focused,
**+** adds or reopens a project and **x** closes it while keeping sessions running.
See [cells usage, keyboard controls and feature parity](docs/cells.md).

**App Space is available in supported graphical sessions, including Kitty over SSH.**
Use **App Space** in the terminal header or **Open App Space** in a project's
right-click menu. Cells mode and frontends without app-frame support hide these
controls. Opening terminals alone does not start an app desktop.

Download packaged builds from [GitHub Releases](https://github.com/bstar/starflux/releases). See the [release and installation notes](docs/releases/0.2.0.md) for platform requirements.

The workspace keeps a project's agents and terminal tabs together with a persistent
project sidebar and shared chrome/terminal themes. New projects open a shell. Run
Codex or another supported agent in any terminal; STAR/FLUX tracks it automatically.
**+ Tab** opens another PTY-backed terminal in the project directory, locally or on
its SSH host. Tabs retain their sessions and support rename and explicit splits.
**Refresh** reloads the UI while keeping those sessions running.

In graphical mode, working project names use bold accent text with a broad, bright activity wave and an accent-colored working count; idle and waiting projects stay still. Terminal capture uses input-driven pacing to reduce typing delay.

The sidebar defaults to **Manual** ordering: drag project rows to arrange them; an insertion line previews the drop, and the order survives restarts. Click **Manual** to switch to **Activity**, which groups active work first and sorts each group by recency. **Low** reverses the full **High** order. Selecting a project does not count as activity. Switching back restores your manual arrangement. Controls use quiet text with borders on hover. Working agents animate; finished or idle agents have no icon. A pending question shows a static amber **?**, and an approval request shows **!**.

Right-click projects and terminal tabs for rename and navigation menus. The header shows live Git status; click it for staged/unstaged files, branch/upstream details and work totals on the project’s local or SSH host.

Click to type; returning to the STAR/FLUX window restores input to the selected agent or terminal pane, including splits. Open dialogs keep their keyboard focus. **Ctrl+Esc** releases input. Closing the workspace retains agents, shells and app workers. Use `--workspace NAME` to reconnect and `--stop` to explicitly end that project’s processes. Dragged files are copied to the destination host before a usable reference is pasted into the agent.

Terminals use a PTY cell stream over local/SSH connections and are drawn directly
by the presenting terminal. They do not require a virtual desktop, terminal
screenshots, or video encoding.

Drag with the left mouse button to select text in Codex or a terminal; releasing copies it to your local clipboard, including over SSH. In a split terminal view, **Close pane** dismisses the focused half and keeps its session available in its tab. **Unsplit** also returns to one pane without ending sessions.

- [App Shelf usage, SSH requirements and acceptance](docs/app-shelf.md)

- [Visual treatment and theme gallery](design/treatments.html)
- [Build and use the demo](docs/demo.md)
- [Opt-in GPU candidate](docs/demo.md#gpu-candidate)
- [Development baseline](docs/baseline.md)

- [Architecture and implementation plan](docs/plan.md)
- [Implementation tasks and acceptance criteria](TASKS.md)

The graphical frontend uses Kitty on the presenting machine, including SSH clients. Cells mode uses the parent terminal directly. Agents and shells use PTY-backed cell updates rendered directly by that Kitty's native font engine. The separate Wayland desktop is used for graphical apps. Broader harness lifecycle integration and the remaining hardening gates stay in the implementation backlog.

The same named workspace can be opened in multiple windows, locally or over SSH.
Each window has independent navigation, splits, menus and presentation, while
terminal processes and tabs are shared. Cells mode inherits its parent terminal font. Refresh UI affects only its
own window. See [multiple-window behavior and upgrade notes](docs/app-shelf.md#multiple-windows-on-one-workspace).

The default terminal path uses native text cells. The virtual desktop belongs exclusively to app space. `STAR_TERMINAL_RENDERER=kitty-pixels` explicitly selects the older screenshot-based terminal path for diagnosis. Refresh both the host executable and local STAR/KIT frontend when updating the native cell protocol; the Mac helper build is separate from the host build.

Build and verification commands are documented in [CONTRIBUTING.md](CONTRIBUTING.md). The app and presentation-only client share one immutable STAR/KIT revision; standard builds enable native terminal transport and the optional GPU backend. GPU rendering remains selected with `STAR_GRAPHICS_RENDERER=gpu`.
