# Paused native process supervision candidate

This checkpoint saves unfinished work from November 13, 2026. It is not part of
the installed backend and must not be enabled in live sessions. The parity goal
was paused before the final SSH, real UI and guardian-loss acceptance runs.

The candidate adds a per-PTY subreaper supervisor, pidfd signalling of owned
children, graceful escalation, descendant reaping, foreground shell job control,
and a retirement-tree capability. An abnormal supervisor exit is retained as
unverified; exit plus PTY EOF alone does not authorize a replacement. Older
owners keep read/input service while unsupported retirement is disabled. Idle
supervisors wait on signals rather than polling.

For isolated reconstruction, start from commit `72d017696`, apply the previous
`checkpoints/native-terminal-candidate/native-adapter-candidate.patch`, then this
directory's `source.patch`. The latter was verified against that reconstructed
baseline when saving. Apply `isolated-build-layout.patch` only in that disposable
copy to give candidate binaries and examples distinct names. Keep its immutable
Git dependency lock; do not replace it with a path-patched lock. Use a separate
Cargo target directory, such as `target/native-supervision-build`, because shared
parser fingerprints previously reused the shipping parser build incorrectly.

Fixture and launcher sources retain their original development paths. Copy them
under `target/` and adjust the launcher root when reconstructing elsewhere. The
older-owner fixture additionally needs the previously built November 6 candidate
`target/release/starflux-graphics`; it checks exit/EOF support without the newer
process-tree capability. No binaries or build directories are included here.

Strict Clippy, all-feature and minimal workspace release tests, and the release
build completed in the final isolated compile pipeline. Earlier local behavior
runs passed stubborn PTY holders, detached descendants with closed stdio, nested
subreapers, children forked during retirement, natural-exit cleanup, operation
replay and slot reuse, real Bash/Ctrl+C job control, and older-owner compatibility.
Those runtime runs preceded the final abnormal-exit guard and idle-wait changes.
Their hashes are historical evidence, not validation of the final source. The
new guardian-loss fixture and final SSH/UI runs remain unverified. Re-run the
private acceptance matrix before claiming this candidate ready for activation.

Owner-loss and never-delivered launch recovery, remaining source adoption, rich
terminal metadata and graphics compatibility, WAN/media stress, and canonical
release/Nix acceptance also remain. This checkpoint does not complete Herdr parity.
