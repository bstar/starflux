#!/usr/bin/env bash
# Bullseye's patchelf predates fixes needed by Debian's Python executable.
set -euo pipefail
if ! patchelf --version | grep -q 'patchelf 0.18.0'; then
    patch_work=$(mktemp -d)
    curl --retry 5 -fsSL https://github.com/NixOS/patchelf/releases/download/0.18.0/patchelf-0.18.0.tar.bz2 -o "$patch_work/source.tar.bz2"
    printf '%s  %s\n' 1952b2a782ba576279c211ee942e341748fdb44997f704dd53def46cd055470b "$patch_work/source.tar.bz2" | sha256sum -c -
    tar -xjf "$patch_work/source.tar.bz2" -C "$patch_work"
    (cd "$patch_work/patchelf-0.18.0" && ./configure --prefix=/usr/local && make -j "$(nproc)" && make install)
    rm -rf "$patch_work"
    hash -r
fi
