# STAR MUX development baseline

The Linux demo uses the following source and runtime baseline, inspected on 2026-10-23.

| Component | Baseline |
| --- | --- |
| STAR/KIT | Git revision `7c4e223fb8e3db1df7e2f25348d2f83cf1d1c8a1`, package 0.12.2 |
| STAR/SCREEN compositor source | Adapted from the local implementation inspected at repository HEAD `919be6768871c74ee8b0a3c13f2dfbe75aed5909`; no dependency on its recording application |
| Smithay | 0.7.0, resolved in Cargo.lock |
| Rust | 1.98.1 through the Nix development shell |
| Cargo | 1.98.0 through the Nix development shell |
| Kitty | System 0.48.2, real Wayland surfaces and successful OSC 72 negotiation |
| tmux | System 3.7c for runtime verification |
| OpenSSH | 10.5p1 |
| Python | System 3.14.7 |
| Hyprland | 0.56.2 |
| Kernel | Linux 7.2.5-3-omarchy |
| Codex | CLI 0.162.1, existing interactive session over loopback SSH |

`Cargo.lock` retains STAR/KIT's Git source. `flake.lock` pins Nix inputs. Builds snapshot only the flake files into `target/nix-shell` so graphical test sockets and large build artifacts are not treated as flake sources. The project-local development-shell profile roots the native runtime dependencies.

The vendored Crossterm parser comes from STAR/FOLD's existing MIT-licensed copy. It includes OSC 72 input events and zero-coordinate guards required by this terminal path. It is deliberately a root manifest patch because Cargo ignores dependency-level patches.

The SSH acceptance host is a private, authenticated loopback server created by `scripts/test-desktop.py`. Its keypair, host key, known-hosts file, port and configuration are disposable. It requires no changes to `~/.ssh` and proves real OpenSSH framing and process behavior. A separate physical host, network latency and container filesystem mappings remain additional checks.

The demo's terminal ownership uses isolated tmux servers named `starmux-WORKSPACE`. Native session workers and the standalone presentation alternative remain design tasks. Existing STAR/KIT and STAR/SCREEN working edits were left untouched.
