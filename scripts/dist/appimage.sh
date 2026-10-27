#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
arch=$(uname -m)
case "$arch" in x86_64|aarch64) ;; *) exit 1 ;; esac
. scripts/dist/deps-debian.sh
ver=$(sed -n '0,/^version = /s/^version = "\(.*\)"/\1/p' Cargo.toml)
mkdir -p dist
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cargo build --workspace --release --locked
cargo test --workspace --release --locked --all-features
bin="${CARGO_TARGET_DIR:-target}/release"
cc -std=c11 -O2 -Wall -Wextra -Werror native/pty-proxy.c -o "$bin/starmux-pty-proxy"
appdir="$work/AppDir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/lib" "$appdir/usr/libexec" "$appdir/usr/share/doc/starmux/third-party"
for executable in starmux starmux-terminal-client starmux-pty-proxy; do
    scripts/dist/glibc-floor.sh "$bin/$executable"
    install -m755 "$bin/$executable" "$appdir/usr/bin/"
done
# Internal process management and audio capture work without distro packages.
for executable in tmux pactl parec dbus-daemon dbus-launch; do
    install -m755 "$(command -v "$executable")" "$appdir/usr/bin/"
done
install -m755 /usr/bin/dbus-run-session "$appdir/usr/libexec/dbus-run-session"
cat > "$appdir/usr/bin/dbus-run-session" <<'DBUS'
#!/bin/sh
HERE=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
exec "$HERE/libexec/dbus-run-session" --dbus-daemon="$HERE/bin/dbus-daemon" --config-file="$HERE/share/dbus-1/session.conf" "$@"
DBUS
chmod +x "$appdir/usr/bin/dbus-run-session"
install -Dm644 /usr/share/dbus-1/session.conf "$appdir/usr/share/dbus-1/session.conf"
mkdir -p "$appdir/usr/share/terminfo"
for directory in /usr/share/terminfo /lib/terminfo; do cp -a "$directory/." "$appdir/usr/share/terminfo/"; done
install -m755 /usr/bin/python3.9 "$appdir/usr/libexec/python3.9"
cp -a /usr/lib/python3.9 "$appdir/usr/lib/"
find "$appdir/usr/lib/python3.9" -type d -name __pycache__ -prune -exec rm -rf {} +
cat > "$appdir/usr/bin/python3" <<'PYTHON'
#!/bin/sh
HERE=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
PYTHONHOME="$HERE" exec "$HERE/libexec/python3.9" "$@"
PYTHON
chmod +x "$appdir/usr/bin/python3"
# Keep plugin lookup inside this build; do not accidentally load host plugins
# linked to a different GStreamer/GLib into the persistent app worker.
mkdir -p "$appdir/usr/lib/gstreamer-1.0" "$appdir/usr/lib/alsa-lib" "$appdir/usr/libexec/gstreamer-1.0"
cp /usr/local/lib/gstreamer-1.0/*.so "$appdir/usr/lib/gstreamer-1.0/"
# h264parse/avdec_h264 are also used by the release transport tests.
for plugin in libgstvideoparsersbad.so libgstlibav.so; do
    cp "/usr/lib/$(gcc -dumpmachine)/gstreamer-1.0/$plugin" "$appdir/usr/lib/gstreamer-1.0/"
done
cp /usr/lib/"$(gcc -dumpmachine)"/alsa-lib/*.so "$appdir/usr/lib/alsa-lib/"
cp /usr/local/libexec/gstreamer-1.0/gst-plugin-scanner "$appdir/usr/libexec/gstreamer-1.0/"
cp /usr/local/bin/gst-inspect-1.0 /usr/local/bin/gst-launch-1.0 "$appdir/usr/bin/"
install -Dm644 packaging/starmux.desktop "$appdir/starmux.desktop"
install -Dm644 packaging/starmux.svg "$appdir/starmux.svg"
cp "$appdir/starmux.svg" "$appdir/.DirIcon"
install -Dm644 packaging/starmux.desktop "$appdir/usr/share/applications/starmux.desktop"
install -Dm644 packaging/starmux.svg "$appdir/usr/share/icons/hicolor/scalable/apps/starmux.svg"
cp -a /usr/share/alsa "$appdir/usr/share/"
cp -R /usr/local/share/licenses/starmux/. "$appdir/usr/share/doc/starmux/third-party/"
curl --retry 5 -fsSL https://raw.githubusercontent.com/bstar/starkit/f60a8fb3ffb41a5625c0c3d43e97394f2249ce61/LICENSE -o "$appdir/usr/share/doc/starmux/third-party/STAR-KIT-LICENSE"
cp README.md LICENSE "$appdir/usr/share/doc/starmux/"
cp -R docs "$appdir/usr/share/doc/starmux/"
python3 scripts/dist/bundle-libraries.py "$appdir"
# The source archive URLs/hashes are immutable in deps-debian.sh.
cp scripts/dist/deps-debian.sh "$appdir/usr/share/doc/starmux/third-party/build-sources.sh"
cat > "$appdir/usr/share/doc/starmux/third-party/README.txt" <<'NOTICE'
STAR/MUX and STAR/KIT: MIT. Python, tmux and D-Bus retain their upstream licenses.
GStreamer core/base: LGPL-2.1-or-later. x264 plugin and x264: GPL-2.0-or-later.
FFmpeg and audio/system libraries retain the licenses in the bundled Debian copyright files.
Exact Debian binary/source versions are in debian-packages.txt. Corresponding source packages:
https://snapshot.debian.org/archive/debian/20260830T000000Z/pool/
https://snapshot.debian.org/archive/debian-security/20260830T000000Z/pool/
GStreamer source archive URLs and SHA-256 hashes are in build-sources.sh.
Runtime libraries remain dynamically linked; the packaged shared objects can be replaced.
NOTICE
# Hash the payload, so changed dependency bytes never reuse an older cache.
build=$(find "$appdir/usr" -type f -exec sha256sum {} + | sed "s|$appdir/||" | sort | sha256sum | cut -c1-24)
sed "s/@BUILD@/$ver-$arch-$build/g" packaging/AppRun > "$appdir/AppRun"
chmod +x "$appdir/AppRun"
case "$arch" in
    x86_64) digest=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d ;;
    aarch64) digest=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444 ;;
esac
curl --retry 5 -fsSL "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-$arch" -o "$work/runtime"
printf '%s  %s\n' "$digest" "$work/runtime" | sha256sum -c -
mksquashfs "$appdir" "$work/fs.squashfs" -root-owned -noappend -comp gzip -no-progress
output="dist/starmux-$ver-$arch.AppImage"
cat "$work/runtime" "$work/fs.squashfs" > "$output"
chmod +x "$output"
env -u LD_LIBRARY_PATH APPIMAGE_EXTRACT_AND_RUN=1 "$output" --check-bundle
