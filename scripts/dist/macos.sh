#!/usr/bin/env bash
# Apple Silicon presentation client. Workspaces and app desktops run on Linux.
set -euo pipefail
cd "$(dirname "$0")/../.."
[ "$(uname -s)" = Darwin ] || exit 1
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' terminal-client/Cargo.toml | head -1)
brew list ffmpeg pkg-config llvm >/dev/null 2>&1 || brew install ffmpeg pkg-config llvm
LIBCLANG_PATH="$(brew --prefix llvm)/lib"
export LIBCLANG_PATH
cargo build --locked --release -p starmux-terminal-client --target aarch64-apple-darwin
bin="${CARGO_TARGET_DIR:-target}/aarch64-apple-darwin/release/starmux-terminal-client"
"$bin" --help
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
stage="$work/starmux-client-$ver"
mkdir -p "$stage" dist
install -m755 "$bin" "$stage/"
cp scripts/install-kitty-audio.py "$stage/"
kit_rev=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml","rb"))["workspace"]["dependencies"]["starkit"]["rev"])')
curl --retry 5 -fsSL "https://raw.githubusercontent.com/bstar/starkit/$kit_rev/integrations/kitty/star_kit.py" -o "$stage/star_kit.py"
cp LICENSE "$stage/"
curl --retry 5 -fsSL "https://raw.githubusercontent.com/bstar/starkit/$kit_rev/LICENSE" -o "$stage/STAR-KIT-LICENSE"
cat > "$stage/INSTALL.txt" <<'INSTALL'
Apple Silicon presentation client for a Linux STAR/MUX workspace host.
Requires Kitty and Homebrew runtime codecs: brew install ffmpeg
Keep this directory in a permanent location, then run:
python3 install-kitty-audio.py --client ./starmux-terminal-client --watcher ./star_kit.py
Reload Kitty's configuration. SSH to the Linux host and run starmux there.
The watcher opens the matching client locally for video, audio and native text.
This archive is the local presentation endpoint, not a macOS workspace host.
INSTALL
tar -C "$work" -czf "dist/starmux-client-$ver-aarch64-apple-darwin.tar.gz" "starmux-client-$ver"
