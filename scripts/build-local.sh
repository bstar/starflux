#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
./scripts/dev build --locked --workspace --release
./scripts/build-pty-proxy
./scripts/prepare-runtime
./scripts/run-local.sh --version
./target/release/starmux-terminal-client --help
./target/release/starmux-pty-proxy --help
