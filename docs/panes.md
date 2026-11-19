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

Imported tmux/Herdr terminals retain their source ownership and do not expose this action. Transferred terminals owned by STAR/FLUX can be terminated individually; the PTY broker and sibling terminal owners are retained.

Movement between projects or separate tab groups remains on the parity roadmap. Directional moves exchange terminal positions. Edge placement changes the split tree within the current project.

The default backend keeps each project's tmux sessions on a separate server. A cross-project move needs a durable handoff that keeps the original PTY owner and records one authoritative destination before retiring the old slot. The standalone native broker can verify retained slot bindings, but the normal workspace open path still needs to consume those bindings for a live transfer. Broker recovery, two-project layout updates and conversation identity must all follow that handoff.
