# Omarchy shell widget

STAR/FLUX includes an optional bar widget for Omarchy. It shows the agents
running in your STAR/FLUX workspaces, lists the ones waiting on you first, and
opens an agent's workspace when you click it. It's a companion to Omarchy's
built-in Agents panel, which tracks usage and limits. This widget tracks the
work itself.

The widget only reads. Clicking an agent navigates to it and never sends text,
approves anything or dismisses attention. Everything it shows comes from the
[automation API](automation.md), so the widget keeps no state of its own.

## Install

Install STAR/FLUX first and make sure `starflux` is on `PATH`. On Arch Linux
and Omarchy, add the signed [star] package repository once (its
[setup page](https://github.com/bstar/star-packages) has the steps), and
STAR/FLUX then updates with `pacman -Syu` and `omarchy update`:

```sh
sudo pacman -S starflux
```

Other packaged builds are on the
[releases page](https://github.com/bstar/starflux/releases). Then add the
plugin from this repository:

```sh
omarchy plugin add https://github.com/bstar/starflux.git --enable
```

The manifest is at the repository root because Omarchy requires it there.
Omarchy clones the repository into its plugin folder and doesn't build or
start STAR/FLUX. To update the widget:

```sh
omarchy plugin update bstar.starflux
```

To remove the widget, which leaves STAR/FLUX and its workspaces untouched:

```sh
omarchy plugin remove bstar.starflux
```

## What it shows

The bar icon shows how many agents are working. When any agent needs input or
approval, has failed, or has lost its backend, the icon switches to the bar's
urgent color and shows that count instead. The popup lists those agents under
**Needs you** and every other agent under **Running**, each with its project,
pane, harness and host.

Clicking an agent:

1. Focuses a local STAR/FLUX window that is already showing the project.
2. Otherwise, asks an idle local window in the same session to select the
   project, then focuses it.
3. Otherwise, opens a new window on that workspace with
   `starflux --workspace PROJECT --focus-pane N`.

Windows attached over SSH from another machine aren't reused, because they
have no local window to focus. The action lives in `omarchy/starflux-open`.
Run it with `STARFLUX_OPEN_DRY_RUN=1` to print what it would do without doing
it.

The widget refreshes every 15 seconds, or every 3 seconds while the popup is
open. Change the interval with the widget's `refreshIntervalSec` setting.

## Compatibility

The widget and STAR/FLUX share a release version, checked by
`scripts/check-version.py`. They don't have to be installed at the same
version, though, because the plugin updates by git and the app by its package
manager. On each refresh the widget reads the `capabilities` reply and needs
API version 1 with `projects.list`, `agents.live`, `frontends.list` and
`frontends.command`.

| Widget sees | Shows |
| --- | --- |
| `starflux` not on `PATH` | STAR/FLUX isn't installed, with a link to this page |
| No reply from the API | STAR/FLUX isn't responding |
| A different API version or missing methods | Update STAR/FLUX to use this widget |
| A compatible API | Live agents |

The first release covers agents on the local controller. Agents on saved SSH
machines (`machines.agents`) are planned once the widget can show a
disconnected host without stalling its refresh.
