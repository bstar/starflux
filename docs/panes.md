# Terminal pane layouts

Terminal tabs own persistent sessions. Pane layouts place those terminals in the current frontend. Changing a layout, zooming or closing a split pane keeps the terminal processes running. Each frontend saves its own layout and focus.

Right-click a terminal tab for **Split right**, **Split down**, **Zoom pane**, **Swap with next pane**, and **Close split pane (keep tab)**. Splitting creates a fresh shell in the project directory beside or below the targeted terminal. Repeat either direction to build a nested layout. Drag any divider to resize that part of the tree.

The existing **Split / Unsplit** button keeps its toggle behavior: Split starts a right split, while Unsplit returns to the focused terminal and retains the other tabs. Closing an individual split pane collapses only its branch. Selecting a hidden tab replaces the focused leaf; focusing an already-visible tab keeps its geometry.

Closing a split pane from its tab menu keeps focus on the current pane if it remains open. A close request targeting an already hidden tab leaves the split layout intact.

**Split with existing tab…** adds a hidden terminal to the right of or below the target pane. It reuses that process and keeps the other branches in place. If either terminal has been replaced since the picker opened, reopen the picker to select its replacement.

Use **Move pane to edge…** from a terminal tab menu to place that terminal on
the left, right, top or bottom of the split layout. This also works for a
hidden tab. The remaining branches keep their internal layout and the moved
terminal keeps its process. Reopen the picker if its terminal was replaced.
The new outer split starts at 35% for the moved
pane and can be resized with its divider. The equivalent action is `pane-edge:ID:left`,
`pane-edge:ID:right`, `pane-edge:ID:up` or `pane-edge:ID:down`.

## Keyboard controls

Release the default **Ctrl+B** prefix, then press:

| Key | Action |
| --- | --- |
| `%` | Split right |
| `"` | Split down |
| Shift+S | Split with an existing tab |
| Arrow | Focus the nearest pane in that direction |
| Alt+Arrow | Move the focused terminal in that direction by exchanging positions |
| Shift+E | Choose an outer edge for the focused terminal |
| o | Cycle pane focus, including while zoomed |
| Shift+O | Swap with the next pane |
| z | Zoom / restore focused pane |
| < / > | Shrink / grow the focused pane within its parent split |
| ! | Close a split pane, retaining its terminal tab |
| x | Pane menu, including recovery and termination |

Bindings are configurable; F1 shows the active keymap. Arrow movement stops at layout boundaries. CLI actions include `terminal:split-right`, `terminal:split-down`, `terminal:zoom`, `terminal:focus-left`, `terminal:move-down`, `terminal:resize-more` and `terminal:close-pane`; use `--frontend` when targeting a particular window.

## Persistence and small canvases

The tree, split weights, focused pane and zoom flag are saved per frontend/project. Existing two-pane layouts are read without replacing their sessions. Terminal exits prune affected leaves in each frontend independently. An unrelated hidden terminal's exit keeps the visible tree intact.

The project limit remains 32 terminals. Layouts use minimum pane sizes based on the cell grid. If a branch cannot fit, it shows one terminal while retaining its tree; the focused terminal remains visible. Grow the canvas or cycle focus to reach the other terminals. Hidden terminals keep running and do not acquire a new attachment or resize simply because another view changes its layout.

## Terminating a terminal

Choose **Terminate terminal…** in the tab menu and type **TERMINATE**. The confirmation names the target and applies to that terminal in all windows. This ends the terminal; closing a split pane only changes its presentation. App Space workers have separate ownership.

Termination verifies the terminal generation and the process identity, including its process start time. If the target was replaced while confirmation was open, the operation is rejected. Ordinary backend termination runs outside the UI's model lock and the host verifies identity again before stopping its session. Completion cannot remove a newly reused slot or stop its new monitor.

Imported tmux/agent-multiplexer terminals retain their source ownership and do not expose this action. Transferred terminals owned by STAR/FLUX can be terminated individually; the PTY broker and sibling terminal owners are retained.

Choose **Move to project…** in a native terminal's tab menu to move it to another project on the same host; the equivalent automation method is `panes.move_project`. The terminal keeps its process, its terminal generation and its conversation records, so resume and attention follow it. Directional moves exchange terminal positions, edge placement changes the split tree within the current project, and **Split with existing tab…** brings another tab's terminal into the current split.

Only native terminals move between projects. The default backend keeps each project's tmux sessions on a separate tmux server, and imported, transferred and plugin terminals keep their owners, so those are not offered. The move is a durable handoff: under both projects' model locks, the destination records the moved binding, the host then moves the authoritative broker slot (destination written before the source record is removed), and only then does the source project drop the pane. If the host's reply is lost, STAR/FLUX asks the host which slot holds the terminal before choosing; an interruption leaves the project without the slot reporting an error rather than two projects driving one terminal. A dead or exited terminal is not moved.
