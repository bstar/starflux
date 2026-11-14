# Native candidate rebased onto current STAR/FLUX — 2026-11-14

This **uninstalled** stage rebases the validated native terminal candidate onto
canonical `5a956fdf6bcf58ded14d03498c4ecf72cd3e804a` plus the user's current
uncommitted sidebar/grid/toggle edits. It replaces the old `72d017696` UI baseline
as the integration candidate. Neither canonical source nor installed binaries
were overwritten by this rebase.

Reconstruct from a disposable archive of the canonical commit:

1. Apply `canonical-ui.patch` to preserve the exact current uncommitted UI edits.
2. Apply `source.patch` for the rebased native features.
3. For private validation only, apply `isolated-build-layout.patch`. It gives
   binaries/examples unique names and makes the vendored parser a standalone
   staging workspace. Do not promote that manifest layout into production.

Unlike earlier incremental checkpoints, these source patches are based directly
on the current canonical commit. **Do not also apply the earlier native patches.**
Their chain was reconstructed and checked against the old candidate before this
three-way merge; its final owner-loss patch hash is retained as provenance.

The workspace is `target/native-rebase-workspace`, with separate build output in
`target/native-rebase-build`. The launcher is `target/run-native-rebase-candidate`;
the copied launcher is workstation specific. Copy archived fixtures into `target/`
when rerunning them because their root lookup is relative to that directory.
The copied rebase script records the original merge process and deliberately
refuses to overwrite an existing rebase workspace.

`refresh-checkpoint.py` checks the canonical commit/current UI hashes, recreates
both patches against an archive, verifies every controlled source file, records
all validation hashes and checks live-process safety. The canonical Cargo.lock
is byte-identical in the candidate; no shared dependency pin was changed.

## Resolved integration changes

Only two source conflicts required manual resolution. Runtime configuration now
contains the native opt-in flag while retaining the canonical rule that legacy
project themes are read but never serialized. Diagnostic snapshots retain both
the canonical global appearance and native terminal metadata. Two new native-only
palette initializers were changed to load core settings, so stale project themes
cannot override them. All current portfolio terminology and coordinator/UI changes
are retained. The sidebar grid/hover/gutter and 11px disclosure boxes with 3px
centered marks are preserved.

## Verification

Strict Clippy across all targets/features, release tests with/without defaults
and the release workspace build pass. Current theme/session acceptance runs with
native terminals enabled in graphical/cells modes locally and over SSH, covering
stale project themes, renaming, explicit global theme changes and session switches.
A private Gruvbox Dark reference rendering is byte-identical to the installed
build's PNG, including the diagonal sidebar grid and smaller disclosure marks.
This proves that reference case, not every physical display/backend combination.

Native owner-loss menu/confirmation, unstarted recovery, OSC 8 links, clipboard/
title metadata, corrupted/missing supervisor evidence and portfolio drag/order/
collapse workflows are checked with private fake programs and loopback SSH.
Exact test scope, counts and evidence identities are in `receipt.json` and
`docs/parity-progress.md`. These tests do not attach to real agent sessions,
open browser URLs, read system clipboards or change production App Space.

Still required before activation: broader existing-source adoption and remaining
uncertified recovery cases, attention integration, graphics/history/placement and
constrained-network/media acceptance, canonical release/Nix checks and an explicit
production integration step preserving existing backends. No default backend has
been switched. Full Herdr parity remains incomplete; every unchecked requirement
in `docs/plan.md` stays in scope.
