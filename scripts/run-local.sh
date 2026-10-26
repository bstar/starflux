#!/usr/bin/env bash
set -euo pipefail
script_path="$(readlink -f -- "${BASH_SOURCE[0]}")"
app_root="$(cd -- "$(dirname -- "$script_path")/.." && pwd)"
if [[ ! -f "$app_root/target/run-env.sh" || ! -x "$app_root/target/release/starmux" ]]; then
  echo "Build STAR/MUX with ./scripts/dev build --release and ./scripts/prepare-runtime" >&2
  exit 1
fi
source "$app_root/target/run-env.sh"
exec "$app_root/target/release/starmux" "$@"
