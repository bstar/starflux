# Local Kitty frontend

This small client runs on the listening machine. It does not launch project
commands or need a local STAR/FLUX workspace. STAR/KIT owns rendering, bounded PCM
transport and playback; the Kitty watcher starts only a fixed local executable.

Build through this directory's Nix shell:

```sh
nix develop -c cargo build --release
python3 ../scripts/install-kitty-audio.py \
  --client target/release/starflux-terminal-client \
  --watcher /path/to/starkit/integrations/kitty/star_kit.py
```

The installer backs up changed files, installs `~/.local/bin/star-kit-terminal`,
and adds a versioned watcher to Kitty's config. Reload Kitty's config, then
use **Refresh UI** or reconnect STAR/FLUX. Versioned paths avoid Kitty's module
cache retaining an older bridge. Live connections are preserved during reload.
To upgrade only the bridge, omit `--client`; the installed helper is retained.

Use `~/.local/bin/star-kit-terminal --check-audio` for a silent three-second
check of the local output device. A positive result means PCM drained through
the local device worker; it does not change system volume.

After installation, use your normal SSH shell and launch STAR/FLUX on its host.
Matching client/server STAR/KIT capabilities negotiate playback automatically.
A server-side restart alone cannot install or activate a missing local player.

The installer also installs STAR/FLUX's font watcher from this repository.
The terminal header shows the current font family; click it to choose an installed
monospace family on the display computer. `−` and `+` change the current Kitty OS
window's size, including fractional initial sizes; the picker includes Reset font
size. Family changes use Kitty's native font engine and apply to all windows in
that Kitty process. The picker states that scope. These are runtime changes and
do not rewrite font settings in kitty.conf. Cells mode continues to inherit the
parent terminal font.

Both the local helper and watcher must be updated for the new font picker over an
existing SSH TTY. Updating only the remote STAR/FLUX executable cannot update the
rendering computer. STAR/FLUX-owned local frontends also use the watcher. Font
catalogues come from Kitty's platform APIs (Fontconfig on Linux, CoreText on macOS),
never the SSH server's fonts. Other rendering/media messages pass through without
an additional JSON decode.

`scripts/test-font-controls.py` and its `--ssh` variant use a disposable Kitty
process, fake shell, private state directories and (for SSH) a private sshd.
They check actual terminal-grid changes, acknowledged sizes, reset to the original
fractional size, font-family selection and preservation of terminal process IDs.

To retain a shared STAR/KIT helper used by other STAR apps, install this client
with `--starflux-client /path/to/starflux-terminal-client` instead of `--client`.
The versioned bridge consumes STAR/FLUX's one-shot application hint and selects
that fixed local helper only for STAR/FLUX. Other applications keep their current
helper and negotiated protocol capabilities.
