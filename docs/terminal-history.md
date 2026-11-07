# Terminal history search and copy mode

Press **Ctrl+B**, release it, then **/** to search history or **Space** to enter copy mode. The terminal tab's right-click menu offers both commands. These controls work in graphical and cells presentations with the native terminal renderer, including SSH. They are hidden in the optional legacy Kitty pixel renderer. F1 lists the active bindings.

**Ctrl+B PageUp** also opens review and pages into retained backend history. **Ctrl+B PageDown** pages toward the bottom; **Ctrl+B End** returns to live output.

Review takes a frozen snapshot of retained backend history, including output from before the frontend attached. It reads owned or imported tmux panes and imported Herdr panes without changing their copy modes. Capture runs asynchronously; you can type the search query while it loads. The program continues running and other frontends can still interact with it. New output does not move the text being reviewed. Escape returns to the live screen. Switching away can discard the frontend's attachment and its review; resizing the terminal ends review so its geometry stays correct.

Search is literal and case sensitive. Type or paste a query and press Enter. Search initially chooses the closest match at or before the review cursor, wrapping to the last match when necessary. **n** finds the next occurrence and **Shift+N** finds the previous occurrence, with wrapping reported in the status bar. Soft-wrapped lines are searchable as one logical line. Unicode and combining characters retain their cell positions. Search queries are limited to 4096 bytes; arrows, Home/End, Backspace and Delete edit the query. Escape while editing a query cancels editing and keeps the previous committed search. Ctrl+U clears the query.

| In copy mode | Action |
| --- | --- |
| Arrows or h/j/k/l | Move the review cursor |
| PageUp / PageDown | Move by a screenful |
| Home / End | Start / end of row |
| Ctrl+Home / Ctrl+End, or g / Shift+G | Start / end of retained history |
| Space or v | Start / stop selection |
| / | Edit search query |
| n / Shift+N | Next / previous match |
| Enter or y | Copy selection or current search match, then return live |
| Escape, q or Ctrl+C | Return live without copying |

Selection can span multiple pages; copied soft wraps are joined and hard line breaks are retained. Clipboard delivery uses the existing frontend clipboard channel, including over SSH. Copying is limited to 65,536 characters. Ordinary review keys and pasted queries are consumed by the frontend and never delivered to the running program. Configured workspace shortcuts remain available. In cells mode F6 or Ctrl+Esc still moves focus to workspace controls.

Capture requests the latest 4000 retained backend rows and accepts at most 2 MiB within 12 seconds. The review parser retains up to 4000 rows plus the visible screen, rewrapping text to this frontend's terminal width. Backend retention may be smaller. If capture fails, the status names the error and review falls back to the attached screen and its local scrollback (up to 1000 rows). This is bounded terminal history, not a permanent transcript of everything ever emitted.

Review is frontend-local and does not change another view's scroll or selection. Late results cannot resurrect canceled review or replace another review request. Snapshot diagnostics include `terminal_review` with source, loading state, cursor, offset, search match count and selection/editing state; terminal text diagnostics continue reporting the live program.
