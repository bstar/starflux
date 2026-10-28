# Building and verification

Use Nix for builds and checks. The application and terminal client belong to one Cargo workspace and inherit the same immutable STAR/KIT pin. Never commit a lockfile generated with a path override. `scripts/dev-candidate.py` keeps candidate manifests and locks disposable.

```sh
./scripts/build-local.sh
nix develop -c scripts/check-release
nix flake check --print-build-logs
```

The release gate follows STAR/FOLD: manifest/lock/tag consistency, formatting, strict Clippy across every target and feature (`dead_code` is the one allowed lint), tests of the shipping release profile with all features and without defaults, dependency advisories/licences/source checks, and a release build. Linux builds include the native C PTY handoff helper. Release overflow checks remain enabled.

The headless behavior gate uses private fake shells, tmux servers and an SSH daemon. It checks process persistence, multiple frontends, selection and split state, tab exit, sidebar overlays and terminal font commands. `scripts/test-cells.py` and its `--ssh` variant run the actual cells frontend in a private PTY without Kitty or display variables, covering keyboard/mouse routing, clipboard output, file delivery, resize, mixed presentations, refresh and reattachment. It opens no desktop windows and sends no prompts to live agents. `scripts/test-multiwindow.py --ssh --apps` additionally creates a disposable GTK app in a private virtual desktop, checking App Space availability, real frame streaming, input and retained workers over authenticated loopback SSH. Add `--video` to exercise an animated H.264 stream and record delivery rate. This separate acceptance test requires a usable graphics runtime. `scripts/test-herdr-adoption.py` additionally checks private Herdr handoff and rollback when Herdr is installed. Physical audio, file-manager dragging and sustained app streaming require separate acceptance checks; unit tests do not prove them.

The Nix package includes `starmux`, `starmux-terminal-client`, and `starmux-pty-proxy`, plus desktop metadata and the runtime environment. Its smoke check loads each installed executable without opening a window. CI runs the gate and Nix checks on Linux x86_64 and ARM64. Only the local architecture is verified by a workstation run.

AppImage, native Arch and macOS distribution packages are not implemented here yet. A local Nix binary is not an old-glibc portable AppImage. STAR/FOLD's cross-distribution AppImage/FUSE verification must be implemented and pass before claiming that portability for STAR/MUX.

`~/.local/bin/starmux` points to `scripts/run-local.sh` on the development workstation. Preserve this wrapper's runtime environment. Rebuild in release mode, run `scripts/prepare-runtime`, and verify `command -v starmux` and `starmux --version`. Running processes keep their loaded executable until refreshed; terminal backends remain persistent.
