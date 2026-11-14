# Keyboard controls

Cells and graphical mode share a keymap. Press **F1** for a scrollable command list, or choose **Keyboard shortcuts** in a project or terminal context menu. Use Up/Down, Home/End and PageUp/PageDown to navigate it; Enter runs the selected command and Escape closes it. Dialogs also accept keys while the graphical canvas retains capture.

The default prefix is **Ctrl+B**. Release it, then press the command key. The status bar shows when a command is pending. Escape cancels; focus changes cancel pending commands. Press Ctrl+B twice to send a literal Ctrl+B to the terminal or app. Unrecognized sequences send the prefix and the following key through in order. Ordinary terminal shortcuts such as Ctrl+C and Ctrl+D pass through.

| After Ctrl+B | Action |
| --- | --- |
| ? | Shortcut help |
| e | Workspace menu / all controls |
| Shift+P | Project picker |
| Shift+T | Terminal tab picker |
| s | Session picker |
| Shift+M | Saved SSH machines |
| Shift+U | [Plugin workflows](plugins.md) |
| l | Choose a URL visible in the focused terminal |
| i | Global attention inbox |
| c | New terminal tab |
| n / p | Next / previous tab |
| 1–9 | Select tab by position |
| ] / [ | Next / previous project |
| a | Add project |
| m | Project context menu |
| x | Pane menu, including reconnect/recovery |
| v | Toggle split |
| % / " | Split right / down |
| Arrow / Alt+Arrow | Focus / move a pane in that direction |
| z | Zoom / restore focused pane |
| o / Shift+O | Cycle pane focus / swap with next pane |
| < / > | Shrink / grow focused pane |
| ! | Close split pane, keep terminal |
| r | Rename tab |
| g | Git status |
| Shift+W | Repository worktree picker |
| w / t | App Space / terminals (App Space requires graphical capabilities) |
| PageUp / PageDown | Scroll terminal history by a page |
| End | Return to the live terminal |
| / / Space | Search terminal history / enter copy mode |
| = / - / 0 | Increase / decrease / reset terminal font |
| Shift+R | Reload keyboard configuration |
| Tab / Shift+Tab | Focus next / previous workspace panel |
| q | Detach this frontend, retaining sessions |

The pane menu keeps the existing confirmation for stopping a program during recovery. Detaching does not stop terminals. Existing F5 and Ctrl+Shift+R refresh controls and cells F6 focus controls remain available.

In [copy mode](terminal-history.md), arrows and PageUp/PageDown move through frozen history, Space starts a selection, Enter/y copies, and Escape returns live. Use / to search and n/Shift+N to navigate matches. Search and selection keys do not reach the program.

The attention inbox accepts `/` for search. Read/state/source/scope filters, notification settings and previous/next pages are also keyboard-accessible menu actions. See [attention controls](attention.md).

In a plugin popup, **Shift+F6** focuses its close control. **Enter** then closes
that popup and its processes; **Escape** returns to the program. Typing and paste
stay out of the program while the close control has focus. Ordinary F6, Escape,
Ctrl+B and other application keys pass through during popup input. The popup
header shows its active shortcut. This binding applies only to an open popup;
it does not change workspace or App Space navigation.

## Panel navigation

In the Projects panel, portfolios support Up/Down row navigation, Left to
collapse or return to a portfolio header, Right to expand, and Enter to toggle a
header or select a project. See [project portfolios](categories.md) for saved
grouping, manual ordering and activity summaries.

**Ctrl+B, then Tab / Shift+Tab** hop between Projects, workspace controls, terminal tabs and each visible split pane (or App Space). **Ctrl+B, then an arrow** focuses an adjacent pane. Plain Tab and Shift+Tab pass through while the terminal or app has input capture. Dialogs keep Tab for their fields.

Once workspace navigation is active, plain Tab/Shift+Tab cycles panels, arrows navigate, and Enter returns to terminal/app input or activates the focused control. F6 toggles workspace navigation; Ctrl+Esc also enters it. Captured App Space F6 remains available to the application; use Ctrl+Esc to leave its input. Set `"direct": {"F6": "disabled"}` to disable the built-in F6 terminal focus shortcut.

The prefix is configurable, which helps when an outer tmux session already uses Ctrl+B. Press the prefix twice to send it literally. The legacy `"tab_switches_panels": true` option restores global Tab navigation; the default is false.

## Configuration

Edit `~/.config/starflux/keys.json` (or `$XDG_CONFIG_HOME/starflux/keys.json`) on the controller machine. For an SSH workspace, this is the remote machine. Entries override individual defaults. Use `disabled` to remove a binding; use `"prefix": null` to disable prefix handling. Direct bindings require a modifier or function key.

```json
{
  "prefix": "Ctrl+a",
  "popup_controls": "Shift+F6",
  "bindings": {
    "c": "new-tab",
    "q": "disabled"
  },
  "direct": {
    "F1": "disabled",
    "Ctrl+Alt+s": "sessions"
  }
}
```

Reload with the current prefix followed by Shift+R. Invalid configuration reports an error and keeps the current keymap. Startup errors retain the default keymap. Help reflects the active configuration. Supported modifiers are Ctrl, Alt and Shift; keys include characters, Space, arrows, Enter, Escape, Tab, Backtab, Home, End, PageUp, PageDown, Backspace, Delete and F1–F24. Use `Shift+r` for an uppercase letter and `Shift+=` for plus.

`popup_controls` may be another modified chord/function key, or `null` to disable
the popup-only shortcut. Reload while the popup is closed. Disabling it leaves
the header × and guarded popup API close available.

Short dialogs keep the focused field and cursor visible. Tab/Shift+Tab and PageUp/PageDown move between fields; Ctrl+Enter accepts and Escape cancels. Compact navigation and viewport limits are described in [small windows](compact.md).
