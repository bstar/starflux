#!/usr/bin/env bash
set -euo pipefail
script_path="$(readlink -f -- "${BASH_SOURCE[0]}")"
app_root="$(cd -- "$(dirname -- "$script_path")/.." && pwd)"
if [[ ! -f "$app_root/target/run-env.sh" || ! -x "$app_root/target/release/starflux" ]]; then
  echo "Build STAR/FLUX with ./scripts/dev build --release and ./scripts/prepare-runtime" >&2
  exit 1
fi
# Host programs STAR/FLUX starts (tmux, python3) get the caller's library
# path back instead of the private runtime one below; see src/host_env.rs.
export STARFLUX_HOST_LD_LIBRARY_PATH="${STARFLUX_HOST_LD_LIBRARY_PATH-${LD_LIBRARY_PATH-}}"
source "$app_root/target/run-env.sh"
exec "$app_root/target/release/starflux" "$@"
