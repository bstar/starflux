# Native terminal candidate checkpoint

This is unfinished STAR/FLUX work, saved separately from the shipping source.
It is not installed or enabled by this checkpoint. See `docs/native-sources.md`
and `docs/parity-progress.md` for acceptance evidence and remaining gaps.

The patch applies to the source tree in this commit, excluding staging Cargo
manifests and lockfiles. Verify it with:

```sh
git apply --check checkpoints/native-terminal-candidate/native-adapter-candidate.patch
```

For isolated development, copy the repository into
`target/inline-graphics-workspace`, apply the patch there, and use separate Cargo
output and executable names. Do not apply it to a live installation. The saved
fixtures and launcher retain their original development paths under `target/`;
copy them there when reconstructing the private test workspace. The generator
also expects the original validation logs and local binaries.

The receipt records historical checks and SHA-256 identities from November 6,
2026; it is evidence of that run, not a claim that those binaries or log files
are included here. All fixture Python sources are saved alongside it. The older
owner compatibility fixture used a private candidate build without the
`retirement-eof-v1` capability and with the prior exit-only retention behavior.

Remaining work includes complete descendant-process supervision, owner-loss
and never-delivered launch recovery, richer terminal metadata and graphics
compatibility, WAN stress testing, and final canonical release validation.
