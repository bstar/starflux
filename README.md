# S T A R / M U X

An agent workspace built with STAR/KIT, with Kitty-rendered terminal panes, persistent local and SSH sessions, and private graphical app desktops.

The App Shelf workspace keeps a project’s agent, terminal tabs and application desktops together, with a persistent project sidebar instead of a paged tab strip. It uses single borders, a drafting grid and shared chrome/terminal themes. New projects open a shell. Run Codex or another supported agent in any terminal; STAR/MUX tracks it automatically.

```sh
starmux --workspace my-app --directory /path/to/project --app 'cargo run --release'
starmux --workspace remote --host user@host --directory /work/project --app 'starfold .'
```

New projects run their agents and shells on the project host. **+ Tab** opens another PTY-backed terminal in the same directory; the host Kitty renders its characters with its own font engine. Run independent agent sessions in as many tabs as needed; terminals retain their sessions and support right-click rename. Additional terminal tabs stay hidden until selected or explicitly split. **+ Launch app** inside app space adds the application under development or supporting applications to a persistent local or SSH virtual desktop. **Refresh** in the top header reloads the UI build. App cards remain in the shelf beside the project workflow.

Working project names use bold accent text with a broad, bright activity wave and an accent-colored working count; idle and waiting projects stay still. Terminal capture uses input-driven pacing to reduce typing delay.

The sidebar defaults to **Manual** ordering: drag project rows to arrange them; an insertion line previews the drop, and the order survives restarts. Click **Manual** to switch to **Activity**, which groups active work first and sorts each group by recency. **Low** reverses the full **High** order. Selecting a project does not count as activity. Switching back restores your manual arrangement. Controls use quiet text with borders on hover. Working agents animate; finished or idle agents have no icon. A pending question shows a static amber **?**, and an approval request shows **!**.

Right-click projects and terminal tabs for rename and navigation menus. The header shows live Git status; click it for staged/unstaged files, branch/upstream details and work totals on the project’s local or SSH host.

Click to type; returning to the STAR/MUX window restores input to the selected agent or terminal pane, including splits. Open dialogs keep their keyboard focus. **Ctrl+Esc** releases input. Closing the workspace retains agents, shells and app workers. Use `--workspace NAME` to reconnect and `--stop` to explicitly end that project’s processes. Dragged files are copied to the destination host before a usable reference is pasted into the agent.

Compatible KIT candidate builds stream app views at a 60 fps target using STAR/SCREEN’s bounded H.264 transport and local decoding. Local app-worker IPC uses shared raw frames. Terminals use a PTY cell stream over local/SSH connections and are drawn directly by the host Kitty. They do not use a virtual desktop, terminal screenshots, or video encoding. Existing app workers need a one-time relaunch to adopt the new transport; older endpoints retain compatibility image delivery. See [transport details](docs/app-shelf.md#local-and-ssh-apps).

App audio uses STAR/KIT's local PCM playback over a separate SSH pipe. The header's **Audio on / Muted** control is saved for the workspace. To hear a remote workspace locally, run `starmux --ssh HOST --workspace NAME` from your listening machine, or use the audio-enabled STAR/KIT Kitty bridge from an SSH shell. See [app audio requirements](docs/app-shelf.md#app-audio-and-mute).

Drag with the left mouse button to select text in Codex or a terminal; releasing copies it to your local clipboard, including over SSH. In a split terminal view, **Close pane** dismisses the focused half and keeps its session available in its tab. **Unsplit** also returns to one pane without ending sessions.

- [App Shelf usage, SSH requirements and acceptance](docs/app-shelf.md)

- [Visual treatment and theme gallery](design/treatments.html)
- [Build and use the demo](docs/demo.md)
- [Opt-in GPU candidate](docs/demo.md#gpu-candidate)
- [Development baseline](docs/baseline.md)

- [Architecture and implementation plan](docs/plan.md)
- [Implementation tasks and acceptance criteria](TASKS.md)

The graphical frontend uses Kitty on the presenting machine, including SSH clients. Agents and shells use PTY-backed cell updates rendered directly by that Kitty's native font engine. The separate Wayland desktop is used for graphical apps. Broader harness lifecycle integration and the remaining hardening gates stay in the implementation backlog.

The same named workspace can be opened in multiple windows, locally or over SSH.
Each window has independent navigation, splits, menus, font size and mute, while
terminal processes, tabs and app desktops are shared. Refresh UI affects only its
own window. See [multiple-window behavior and upgrade notes](docs/app-shelf.md#multiple-windows-on-one-workspace).

The default terminal path uses native Kitty text cells. The virtual desktop belongs exclusively to app space. `STAR_TERMINAL_RENDERER=kitty-pixels` explicitly selects the older screenshot-based terminal path for diagnosis. Refresh both the host executable and local STAR/KIT frontend when updating the native cell protocol; the Mac helper build is separate from the host build.

Build and verification commands are documented in [CONTRIBUTING.md](CONTRIBUTING.md). The app and presentation-only client share one immutable STAR/KIT revision; standard builds enable native terminal transport and the optional GPU backend. GPU rendering remains selected with `STAR_GRAPHICS_RENDERER=gpu`.
