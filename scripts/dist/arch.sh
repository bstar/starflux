#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
[ "$(id -u)" -ne 0 ] || { echo 'Run makepkg as an unprivileged user' >&2; exit 1; }
ver=$(sed -n '0,/^version = /s/^version = "\(.*\)"/\1/p' Cargo.toml)
mkdir -p dist
out=$(realpath dist)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# git archive cannot accidentally ship target/, private runtime state or tokens.
git archive --format=tar --prefix="starflux-$ver/" HEAD | gzip -n > "$work/starflux-$ver.tar.gz"
sum=$(sha256sum "$work/starflux-$ver.tar.gz" | cut -d' ' -f1)
sed -e "s/@VERSION@/$ver/g" -e "s/@SHA256@/$sum/g" packaging/arch/PKGBUILD.in > "$work/PKGBUILD"
cd "$work"
makepkg --noconfirm
cp PKGBUILD "starflux-$ver.tar.gz" "$out/"
while IFS= read -r package; do cp "$package" "$out/"; done < <(makepkg --packagelist)
