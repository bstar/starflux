# STAR/FLUX parser extension

This directory contains vt100 0.16.2 from crates.io, under the MIT license retained
in `LICENSE` and `LICENSES/vt100.txt` at the repository root. The original archive
SHA-256 is `054ff75fb8fa83e609e685106df4faeffdf3a735d3c74ebce97ec557d5d36fd9`.
Upstream: https://github.com/doy/vt100-rust. `Cargo.toml.orig`, README and changelog
retain the original package attribution and metadata.

This checked-in source is the canonical dependency. STAR/FLUX adds shared
immutable OSC 8 cell targets in optional row metadata; target lifetime through painting, erasing, scrolling,
resize, clones and formatting; revision tracking; and bounded formatting for saved
previews. BEL and ESC-backslash terminators are supported. URI semicolons are
preserved independently of VTE's callback parameter limit. OSC bodies are bounded
to 128 KiB; overflow, cancellation and malformed OSC 8 cannot publish a partial
target. Unsupported C1 encodings are not advertised.

URIs are limited to 8192 bytes, parameters to 1024 bytes. Retained allocations
share an 8-MiB arena budget with cloned screens. Erase/overwrite/drop releases
references. Parameters, including `id`, survive serialization; STAR/FLUX uses
pointer feedback instead of group underlining and does not group hover decoration
by `id`. Application GIO URL policy and explicit plugin approval remain outside
this parser. OSC 8 never opens a URL or executes a command.

Saved screens use `contents_formatted_bounded` before JSON/base64 encoding, so
repeated URI runs cannot amplify one shared allocation into an unbounded archive.
Ordinary cells retain their 32-byte upstream layout and need no destructor;
unlinked rows allocate no link vector. `Screen::hyperlink` and
`Screen::hyperlink_params` expose the visible destination and parameters. The
public text/style API is otherwise retained. Tests live in `src/osc8_tests.rs`,
terminal cache tests, native plugin UI fixtures and actual cells-client PTY
fixtures. Workspace membership excludes this crate, so upstream development-only
dependencies are not introduced. The STAR/KIT pin is unchanged.

Run `python3 scripts/bench-terminal-parser.py` for an isolated comparison with
the hash-verified original crate. It reports median plain/styled scrolling-text
throughput; it does not measure network, drawing or end-to-end input latency.

Protocol reference: [Hyperlinks in Terminal Emulators](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda).
