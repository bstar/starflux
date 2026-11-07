#!/usr/bin/env bash
set -euo pipefail
app_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "$app_root/target/run-env.sh"
export LIBVA_DRIVERS_PATH="${LIBVA_DRIVERS_PATH:-$LIBGL_DRIVERS_PATH}"
exec "$app_root/target/release/starflux-terminal-client" "$@"
