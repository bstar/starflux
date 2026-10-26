# S T A R / M U X

An agent workspace built with STAR/KIT, with Kitty-rendered terminal panes, persistent local and SSH sessions, and private graphical app desktops.

The App Shelf workspace keeps a project’s agent, terminal tabs and application desktops together, with a persistent project sidebar instead of a paged tab strip. It uses single borders, a drafting grid and shared chrome/terminal themes. Normal launches use real Codex; the fixture is reserved for explicit tests.

```sh
starmux --workspace my-app --directory /path/to/project --app 'cargo run --release'
starmux --workspace remote --host user@host --directory /work/project --app 'starfold .'
```

New projects run their agents and shells on the project host. **+ Agent** starts another independent Codex session in the same directory; agent tabs retain their running sessions and support right-click rename. Additional terminal tabs stay hidden until selected or explicitly split. **Run app** launches the application under development; **Launch app** adds supporting applications to a persistent local or SSH virtual desktop. App cards remain in the shelf beside the project workflow.

Working project names use bold accent text with a broad, bright activity wave and an accent-colored working count; idle and waiting projects stay still. Terminal capture uses input-driven pacing to reduce typing delay.

Right-click projects and terminal tabs for rename and navigation menus. The header shows live Git status; click it for staged/unstaged files, branch/upstream details and work totals on the project’s local or SSH host.

Click to type; **Ctrl+Esc** releases input. Closing the workspace retains agents, shells and app workers. Use `--workspace NAME` to reconnect and `--stop` to explicitly end that project’s processes. Dragged files are copied to the destination host before a usable reference is pasted into the agent.

Drag with the left mouse button to select text in Codex or a terminal; releasing copies it to your local clipboard, including over SSH. In a split terminal view, **Close pane** dismisses the focused half and keeps its session available in its tab. **Unsplit** also returns to one pane without ending sessions.

- [App Shelf usage, SSH requirements and acceptance](docs/app-shelf.md)

- [Visual treatment and theme gallery](design/treatments.html)
- [Build and use the demo](docs/demo.md)
- [Opt-in GPU candidate](docs/demo.md#gpu-candidate)
- [Development baseline](docs/baseline.md)

- [Architecture and implementation plan](docs/plan.md)
- [Implementation tasks and acceptance criteria](TASKS.md)

Linux and Wayland are the initial platform. The frontend uses an outer Kitty window. Agents and shells use actual Kitty rendering at native pixel resolution by default. `STAR_TERMINAL_RENDERER=direct` opts into the experimental STAR/KIT text renderer. A capture comparison verifies matching glyph shapes and placement against standalone Kitty; the GPU copy can change some antialiased colors by one 8-bit level. Broader harness lifecycle integration and the remaining hardening gates stay in the implementation backlog.
