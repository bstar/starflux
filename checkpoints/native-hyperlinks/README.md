# Native hyperlink forwarding candidate — 2026-11-14

This is an **isolated, uninstalled** extension of the native process supervision
candidate. It preserves OSC 8 destinations independently of visible labels,
without changing STAR/KIT's shared text-cell protocol or immutable dependency.
The production backend has not been switched.

Apply these patches in order to a disposable archive of canonical `72d017696`:

1. `../native-terminal-candidate/native-adapter-candidate.patch`
2. `../native-process-supervision/source.patch`
3. `source.patch` from this directory
4. `../native-process-supervision/isolated-build-layout.patch` for unique staging
   binary names only. Never use that layout patch for production manifests.

`receipt.json` records the preceding patch hash, changed source hashes, candidate
binary/lock hashes, fixture hashes, validation logs and live-process safety.
`refresh-checkpoint.py` checks patch reconstruction, validation evidence, the
installed executable and original process identities before updating this stage.
The earlier supervision checkpoint remains a separate historical validation;
do not regenerate it from this later candidate.

The staged workspace is `target/inline-graphics-workspace`, with a dedicated
`target/native-supervision-build` target directory to avoid reusing incompatible
vendored-parser fingerprints. The copied launcher is workstation-specific.
The integration fixture must be copied into `target/` before running because
its root is resolved relative to that directory. It uses private runtime/data
namespaces, fake shells and a private authenticated loopback SSH daemon; cleanup
is scoped to those resources. It does not open URLs or access the system clipboard.

## Behavior and bounds

Each cells projection carries a complete dictionary of OSC 8 targets and sorted,
disjoint cell spans. This accompanies even an empty text delta, so changing or
clearing a target on an unchanged label cannot leave the previous destination.
The view validates and admits metadata before owned decode and updates parser
annotations without repainting glyphs, styles or terminal modes. Older owners
which omit the optional table remain readable.

Limits: 1,024 targets, 4,096 spans, 256 KiB target text; each URI is at most 8 KiB
and parameters at most 1 KiB. Control characters and parameter separators are
rejected. Out-of-budget projection fails visibly while retaining source/input;
links are never silently truncated or redirected. Parser replacement validates
and prepares all target allocations before clearing the accepted map.

Frozen native history exports OSC 8 transitions alongside style information,
including wide/combining labels and logical wraps, within the existing 2 MiB
history limit. Export operates on an isolated retained snapshot. Repeated targets
share dictionary entries in live projections.

## Validation and remaining work

Strict all-target/all-feature Clippy, release tests with and without defaults,
and the release workspace build pass. Graphical and cells fixtures locally and
over SSH exercise two independent views of one source, hidden destinations,
semicolon-containing URIs, unchanged labels, replacement/clear, frontend copy
responses, pointer regions, frozen review during another view's live updates,
resize, connector loss/reconnection and explicit recovery clearing old targets
in both views. Source identity is preserved until explicit recovery is confirmed.
Older-owner local/SSH compatibility checks preserve read/input and refuse
unsupported lifecycle actions. Exact counts and evidence hashes are in the
receipt and `docs/parity-progress.md`.

This does **not** implement general OSC 52 clipboard, terminal title/bell/attention
forwarding, owner-loss recovery, broader source adoption, all graphics/history
compatibility or canonical promotion. Before activation, rebase the candidate
onto current global configuration, portfolio/sidebar changes and glyph fixes;
retain the current shared dependency and production executable names. Every
other unchecked requirement in `docs/plan.md` remains in scope.
