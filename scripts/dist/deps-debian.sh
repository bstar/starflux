#!/usr/bin/env bash
# Source in a Debian Bullseye release container. The old libc is intentional.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends \
  ca-certificates curl file git xz-utils build-essential pkg-config clang libclang-dev \
  libasound2-dev libasound2-plugins libavcodec-dev libavformat-dev libavutil-dev \
  libavfilter-dev libswscale-dev libswresample-dev libwayland-dev libxkbcommon-dev \
  libdrm-dev libgbm-dev libegl1-mesa-dev libpixman-1-dev libudev-dev \
  libglib2.0-dev libx264-dev liborc-0.4-dev libgstreamer1.0-dev \
  libgstreamer-plugins-base1.0-dev gstreamer1.0-plugins-bad gstreamer1.0-libav \
  python3 python3-pip python3-dev ninja-build patchelf squashfs-tools \
  libevent-dev libncurses-dev bison flex tmux openssh-client pulseaudio-utils dbus dbus-x11 xkb-data
# Lock both the Rust compiler and the source dependencies in the portable build.
if ! command -v rustup >/dev/null; then
  curl --proto '=https' --tlsv1.2 -fsS https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.98.1 --no-modify-path
fi
# shellcheck source=/dev/null
. "$HOME/.cargo/env"
rustup default 1.98.1
python3 -m pip install --disable-pip-version-check meson==1.7.2
export LIBCLANG_PATH=/usr/lib/llvm-11/lib
# GStreamer 1.24 is required by the cell/video transport. Build it against
# Bullseye's glibc, retaining ORC's SIMD converters. Old distro codec plugins
# remain ABI-compatible with the newer core and travel in the AppImage too.
export PKG_CONFIG_PATH=/usr/local/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}
export LD_LIBRARY_PATH=/usr/local/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
GST_PLUGIN_SYSTEM_PATH_1_0=/usr/local/lib/gstreamer-1.0:/usr/lib/$(gcc -dumpmachine)/gstreamer-1.0
export GST_PLUGIN_SYSTEM_PATH_1_0
source_work=$(mktemp -d)
while read -r component digest; do
  archive="$component-1.24.13.tar.xz"
  curl --retry 5 -fsSL "https://gstreamer.freedesktop.org/src/$component/$archive" -o "$source_work/$archive"
  printf '%s  %s\n' "$digest" "$source_work/$archive" | sha256sum -c -
  tar -xJf "$source_work/$archive" -C "$source_work"
  install -Dm644 "$source_work/$component-1.24.13/COPYING" "/usr/local/share/licenses/starflux/$component.COPYING"
  extra=()
  case "$component" in
    gstreamer) extra=(-Dtools=enabled) ;;
    gst-plugins-base) extra=(-Dapp=enabled -Dvideoconvertscale=enabled -Dvideorate=enabled -Daudioconvert=enabled -Daudioresample=enabled -Dvideotestsrc=enabled -Dorc=enabled) ;;
    gst-plugins-ugly) extra=(-Dgpl=enabled -Dx264=enabled) ;;
  esac
  meson setup "$source_work/$component-build" "$source_work/$component-1.24.13" \
    --prefix=/usr/local --libdir=lib --buildtype=release --wrap-mode=nodownload \
    -Dauto_features=disabled "${extra[@]}"
  ninja -C "$source_work/$component-build" -j "$(nproc)"
  ninja -C "$source_work/$component-build" install
  ldconfig
done <<'SOURCES'
gstreamer ed4678e1d0708db01a469ae5dd31c10cac73c0fb3f7c2c471b0d3cab0affc7d1
gst-plugins-base 31a4a34e02df0471274fd0e8016495475b670320d20a3349faf0634340166c42
gst-plugins-ugly dc08bb11dce0a43453466fb9034e4fe06709fb5af68475bcf6d288693b661a5d
SOURCES
rm -rf "$source_work"

# Native terminal transport needs extended-keys and allow-passthrough (tmux >=3.3).
tmux_work=$(mktemp -d)
curl --retry 5 -fsSL https://github.com/tmux/tmux/releases/download/3.5a/tmux-3.5a.tar.gz -o "$tmux_work/tmux.tar.gz"
printf '%s  %s\n' 16216bd0877170dfcc64157085ba9013610b12b082548c7c9542cc0103198951 "$tmux_work/tmux.tar.gz" | sha256sum -c -
tar -xzf "$tmux_work/tmux.tar.gz" -C "$tmux_work"
(cd "$tmux_work/tmux-3.5a" && ./configure --prefix=/usr/local && make -j "$(nproc)" && make install)
rm -rf "$tmux_work"
