# Local Kitty audio frontend

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
