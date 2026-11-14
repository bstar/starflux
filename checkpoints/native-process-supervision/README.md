# Native process supervision candidate

This checkpoint saves isolated native-terminal work from November 13–14, 2026.
It is not part of the installed backend and is not enabled in live sessions.
The previously pending supervision acceptance matrix passed on November 14.

The candidate adds a per-PTY subreaper supervisor, pidfd signalling of owned
children, graceful escalation, descendant reaping, foreground shell job control,
and a retirement-tree capability. The supervisor remains alive until the kernel
confirms it has no children left. Exit plus PTY EOF alone cannot authorize a
replacement after abnormal supervisor loss. Older owners keep read/input service
while unsupported retirement is disabled. Idle supervisors wait on signals.
Foreground command/directory inspection follows the verified program rather
than reporting the supervisor; agent identity remains distinct from source identity.

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
`refresh-checkpoint.py` regenerates the patch/receipt from that workspace and the
private validation logs; it never builds, installs, or activates the candidate.

Verified in the latest isolated build:

- Strict all-target/all-feature Clippy; workspace release tests with all features
  and without defaults; release build.
- Local and SSH retirement of stubborn PTY holders, detached children with closed
  stdio, nested subreapers and children forked during retirement.
- Natural-exit cleanup, operation replay and slot reuse; no source replay after
  supervisor loss, even when a final EOF is observable.
- Correct foreground program/directory and separate agent/source identities;
  idle supervisors add no periodic polling load in private sampling.
- Real Bash foreground job control, Ctrl+C and background-job cleanup.
- Graphical/cells recovery dialogs locally and over SSH, two views following
  replacement epochs, final exit remaining empty, and explicit shared reopening.
- Older-owner read/input compatibility with unsupported retirement disabled.

`receipt.json` records source, binary, fixture and log identities. Validation logs
remain under `target/native-tree-resumed-*.log`. The installed executable and all
five original STAR/FLUX PID/start identities were unchanged during this run.

This candidate still needs broker/owner-loss and never-delivered launch recovery,
remaining source adoption, rich terminal metadata and graphics compatibility,
WAN/media stress, and canonical release/Nix acceptance. Before any activation,
rebase onto the current application without losing global theme settings,
portfolio terminology, glyph fixes or sidebar changes. The staging manifests and
old UI baseline must never replace shipping code wholesale. This checkpoint does
not complete Herdr parity.
