# Saved machines

Press **Ctrl+B, Shift+M**, or choose **Machines…** in a project's context menu. The same keyboard-driven manager works in graphical and cells presentations, including when the controller itself runs over SSH.

Choose **Add SSH machine…** and enter a label, an OpenSSH target/alias, an optional OpenSSH config path, the remote STAR/FLUX executable, and an optional remote workspace identity. A blank workspace uses that machine's saved default. Credentials, ports, identity files, proxies and known-hosts settings belong to OpenSSH. STAR/FLUX does not store passwords or private keys.

Profiles live in `$XDG_CONFIG_HOME/starflux/machines.json` (normally `~/.config/starflux/machines.json`) on the controller machine. This is distinct from the computer displaying an SSH frontend. The file is private, versioned and written atomically under a cross-process lock; revisions reject stale edits and removal. Invalid or unsupported catalogs produce an error instead of being overwritten.

## Check a connection

**Test connection / retry** runs a bounded, read-only SSH check in the background. It checks Python 3 and tmux for project terminals, then checks the remote STAR/FLUX automation API independently. An installed STAR/FLUX helper is needed for attaching to a remote workspace and App Space; ordinary remote project terminals use Python 3 and tmux. Current project terminal workers require Linux.

Checks use batch authentication and strict host-key verification. If SSH is not set up, use **Copy SSH setup command** and run it in an interactive terminal, complete authentication or verify the host key there, then retry. The error retains OpenSSH's diagnostic. Tests do not install software, start servers or create terminal sessions. A failed machine check does not disconnect another machine or alter running projects.

## Use a saved connection

**Add project on this machine…** creates a new project on the saved endpoint, in the current STAR/FLUX session. Its terminal owner reconnects through the existing SSH transport. The project stores the endpoint's host, OpenSSH config and helper executable. Later profile edits do not silently migrate running projects.

**Projects on this machine** lists projects registered by this controller, across its saved sessions. Opening a project retains its existing terminal processes; the corresponding local-project list returns to local work. Recovery and reconnect controls remain available in each terminal's menu.

To attach to a machine's own saved STAR/FLUX workspace, run this **on the machine holding the profile**:

```sh
starflux --machine 'Build machine'
starflux --machine machine-0123456789abcdef0123456789abcdef --cells
```

The manager can copy this command. Labels must be unique; stable identities are preferable for scripts. An explicit `--workspace NAME` overrides the profile's workspace. A blank saved workspace resolves the remote default. `--machine` also forwards `--api` and `--api-stdio` to the saved remote executable; a connection failure never falls back to local execution. It cannot be combined with `--host`, `--ssh`, `--ssh-config` or an explicit `--remote-executable`.

**Disable new connections** prevents new saved-machine launches and project creation. **Remove saved profile** removes only the connection metadata. Both retain existing projects, terminals, agent conversations and App Space workers. Existing projects retain their endpoint configuration.

## Automation

The [versioned API](automation.md) exposes `machines.list`, `machines.save`, `machines.remove`, `machines.test` and `machines.project`. Writes require an operation UUID. Updating an existing profile requires `id` and `expected_revision`; removal requires `machine` and `expected_revision`. `machines.project` registers a project without starting a program. Connection checks can take up to 24 seconds; use a `deadline_ms` of at least 30000 when checking an endpoint with a potentially unresponsive helper.

Example save parameters:

```json
{
  "label": "Build machine",
  "target": "workbox",
  "ssh_config": null,
  "remote_executable": "starflux",
  "workspace": null,
  "enabled": true
}
```

Call `machines.agents` with one to eight saved machine IDs in `machines` to
query their controllers concurrently. The response preserves profile identity
and revision and returns either an agent page or an error per host. Use
`offset` and `limit` (1–16 panes, default 8) to page through each controller's
pane catalog. Offsets count inspected panes, so a page can contain fewer agent
rows than its limit. Each successful page supplies `next_offset`. Disabled and
unreachable profiles have their own errors. An unknown or removed profile
also returns a per-machine error with the requested identifier. In the Machines menu, choose
**Agents on saved machines** to see the same observations in graphical or
cells mode. **Refresh agent status** updates the current page. Each enabled machine’s
details menu also offers **Agents on this machine**, with paging and refresh
scoped to that controller. Machine
headings open the existing machine details. **More agents on …** opens the
next page for that machine. **Back to combined agents** restores the overview
page you came from, including after paging or refreshing within the machine.
Select an agent row to copy a
launch command for its saved machine, workspace, and one-based pane number in
the current presentation mode. `--focus-pane` selects that existing pane on
launch. Agent rows use the pane generation when the controller reports one, so a
reused pane number is rejected. Copied commands carry it in
`--focus-pane-epoch`.
Later refreshes retain the frontend’s current selection. Refresh the overview after editing a machine profile.
When the current controller and presenter are local, **Open workspace here**
in machine details opens the configured workspace or the remote default.
From an agent row, **Open … here** checks
the remote pane and opens it in the same terminal window. The previous
workspace’s terminal and App Space processes keep running. The destination
gets its own frontend identity. Choose **Cancel** or press **Esc** while the
check is pending to keep the current view open. A failed check also leaves
it open. Choose **Return to local view** from the Machines menu, or detach, to restore
the original local view in the same terminal. A failed remote launch also returns to the local view. Direct
switching between remote controllers remains open.
