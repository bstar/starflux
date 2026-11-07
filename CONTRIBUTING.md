# Building and verification

Use Nix for builds and checks. The application and terminal client belong to one Cargo workspace and inherit the same immutable STAR/KIT pin. Never commit a lockfile generated with a path override. `scripts/dev-candidate.py` keeps candidate manifests and locks disposable.

```sh
./scripts/build-local.sh
nix develop -c scripts/check-release
nix flake check --print-build-logs
```

The release gate follows STAR/FOLD: manifest/lock/tag consistency, formatting, strict Clippy across every target and feature (`dead_code` is the one allowed lint), tests of the shipping release profile with all features and without defaults, dependency advisories/licences/source checks, and a release build. Linux builds include the native C PTY handoff helper. Release overflow checks remain enabled.

The headless behavior gate uses private fake shells, tmux servers and an SSH daemon. It checks process persistence, multiple frontends, selection and split state, tab exit, sidebar overlays and terminal font commands. `scripts/test-cells.py` and its `--ssh` variant run the actual cells frontend in a private PTY without Kitty or display variables, covering keyboard/mouse routing, clipboard output, file delivery, resize, mixed presentations, refresh and reattachment. It opens no desktop windows and sends no prompts to live agents. `scripts/test-multiwindow.py --ssh --apps` additionally creates a disposable GTK app in a private virtual desktop, checking App Space availability, real frame streaming, input and retained workers over authenticated loopback SSH. Add `--video` to exercise an animated H.264 stream and record delivery rate. This separate acceptance test requires a usable graphics runtime. `scripts/test-herdr-adoption.py` additionally checks private Herdr handoff and rollback when Herdr is installed. Physical audio, file-manager dragging and sustained app streaming require separate acceptance checks; unit tests do not prove them.

The Nix package includes `starflux`, `starflux-terminal-client`, and `starflux-pty-proxy`, plus desktop metadata and the runtime environment. Its smoke check loads each installed executable without opening a window. CI runs the gate and Nix checks on Linux x86_64 and ARM64. Only the local architecture is verified by a workstation run.

Plugin popup acceptance uses `test-session-manager.py --plugin-popups-ui` in
graphical/cells modes locally and over SSH. `test-cells.py --plugin-popup` and its
SSH variant exercise the actual host executable. These run in private namespaces
and check raw input, resize, transient membership and owned process cleanup.
With a graphics runtime, additionally run
`python3 scripts/test-multiwindow.py --ssh --apps --video --audio --plugin-popup`
to check App Space keyboard capture/restoration and continued animated video and
PCM delivery while a popup is active. This separate check does not play audio
through physical speakers.

AppImage, native Arch and macOS distribution packages are not implemented here yet. A local Nix binary is not an old-glibc portable AppImage. STAR/FOLD's cross-distribution AppImage/FUSE verification must be implemented and pass before claiming that portability for STAR/FLUX.

`~/.local/bin/starflux` points to `scripts/run-local.sh` on the development workstation. Preserve this wrapper's runtime environment. Rebuild in release mode, run `scripts/prepare-runtime`, and verify `command -v starflux` and `starflux --version`. Running processes keep their loaded executable until refreshed; terminal backends remain persistent.

The vendored native parser retains OSC 8 destinations independently of labels.
`python3 scripts/bench-terminal-parser.py` compares plain and styled scrolling
output with the hash-verified original vt100 crate in a disposable offline build.
It requires the original crate download in Cargo's cache. This measures parser
throughput, not end-to-end rendering or SSH latency. Keep ordinary cells compact
and without reference-counted metadata; link retention and formatting tests run
in the release gate.

Inline graphics foundations have separate read-only acceptance executables:
`cargo build --release --locked --examples`, followed by
`python3 scripts/test-graphics-observer.py --owner-executable target/release/examples/graphics-owner-proof --decoder-executable target/release/examples/graphics-stream-proof --flow-control`
and the same command with `--ssh`. These use private PTYs, tmux and authenticated
loopback SSH, checking native decoded pixels, source query reply order,
asynchronous decoding, released reservations and observer flow control. They do
not attach to real panes, open a presenter or prove installed image rendering.
The canonical gate includes both commands. See `docs/inline-graphics.md` for
the unfinished production integration and rendering requirements.

The private native-source worker is checked with
`python3 scripts/test-source-broker.py --streams --executable target/release/examples/native-source-broker`
and the same command with `--ssh`. It requires Linux pidfds and private loopback
SSH support. It starts only disposable fake programs, checks retained PTYs and
images without views, source guards, large input, resize, launch receipts and
asynchronous termination, pushed frames, slow-view credits, row deltas and stale
view epochs, then cleans up its own worker. The canonical gate
includes both streamed checks. This helper does not switch the installed session backend.


Managed native-source acceptance additionally uses `test-source-broker.py
--service --streams --executable target/release/examples/native-source-broker
--worker-executable target/release/starflux` locally and with `--ssh`. This runs
the actual application worker/connector in private data/runtime namespaces and
checks durable discovery, concurrent pane intents, read-only uncertain-launch
recovery, runtime loss and loaded-owner upgrades. It does not enable the normal
workspace backend. See `docs/native-sources.md` for the integration boundary.
