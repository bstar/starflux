# Small windows

Cells mode supports windows down to **32 columns × 10 rows**. Graphical mode supports **320 × 240 pixels**. Smaller viewports show a resize message while retaining terminal processes and App Space workers; resizing back restores presentation.

At graphical widths below 600 px, **Menu** and **Projects** replace the persistent sidebar. The terminal uses the available width. Cells mode keeps its toolbar and provides **More** for controls that do not fit; below 60 columns, **Projects** opens a picker instead of taking width away from the terminal. Wider windows keep their existing sidebars and project ordering.

The same controls are keyboard-accessible in both presentations:

| Shortcut | Action |
| --- | --- |
| Ctrl+B, e | Workspace menu |
| Ctrl+B, Shift+P | Project picker |
| Ctrl+B, Shift+T | Terminal tab picker |
| Ctrl+B, s | Session picker |
| F1 | All available shortcuts |

The workspace menu includes project and terminal menus, reconnect/recovery, tabs, splits, zoom, Git/worktrees, session and machine management, the attention inbox, file delivery, sorting, theme and refresh/detach. Graphical capabilities add App Space/window selection, launch and app controls. Unsupported App Space and terminal-font controls stay hidden in cells mode, which inherits the parent's font.

Project creation stays at the bottom of the project picker. The terminal picker retains all tabs even when only one tab fits in the strip. Menus support arrows, Home/End, PageUp/PageDown and Enter; Escape closes them.

## Dialogs and history

Short dialogs scroll their fields to follow keyboard focus. Tab/Shift+Tab and PageUp/PageDown move between fields, Ctrl+Enter accepts and Escape cancels. The focused field is also a pointer target in cells mode. A field count indicates hidden fields. Long text keeps the editing cursor visible, including UTF-8 paths; data is not truncated when its display is clipped.

Compact session pickers retain search, selection, creation, import, default, deletion and opening controls. Up/Down and Home/End navigate sessions. Ctrl+N creates, Ctrl+I imports, Ctrl+D changes the default, Delete starts the existing deletion confirmation and Enter opens. Ctrl+A/Ctrl+E move the search cursor to its ends. Session list offsets adapt when resizing so the selected row remains visible.

Compact Git status combines repository information and changed files in one wrapped, scrollable view. Up/Down and PageUp/PageDown scroll, `r` refreshes and Escape closes. Long paths can be read without widening the window.

Resizing does not rewrite the saved pane tree. A tree that cannot fit collapses its presentation around the selected terminal, then restores when space is available. Pane/tab keyboard navigation still accesses retained terminals. Each frontend has its own viewport and navigation.

## App Space

App pixels remain one-to-one: a small view crops its app image instead of scaling it. Resizing resubmits the crop even if the app has reused a static image buffer. Opening a menu releases held input; the first subsequent key or paste restores the selected app's focus. New workers report mouse-selected window focus, including when multiple app windows share a desktop. Existing workers remain usable; if an older worker cannot identify the focused window, clicking or choosing an app window resumes keyboard input safely. Normal key delivery does not require a focus round trip for each key.

The private SSH app test exercises streaming, app menus, keyboard return after menus, worker retention and native pixel geometry across compact and full-size views:

```sh
python3 scripts/test-multiwindow.py --ssh --apps --narrow
```

The release suite also checks compact graphical/cells controls and forms, actual cells PTY input at 32×10, Unicode editing, session selection/defaults, Git scrolling, independent frontends and preserved processes. The GTK app test uses a disposable virtual desktop and never sends input to a live user app.
