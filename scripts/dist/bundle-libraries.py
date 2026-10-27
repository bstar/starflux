#!/usr/bin/env python3
"""Copy transitive ELF libraries without replacing the host C/GPU runtimes."""
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

root = Path(sys.argv[1]).resolve()
libdir = root / 'usr/lib'
libdir.mkdir(parents=True, exist_ok=True)
exclude = re.compile(r'^(ld-linux|libc\.so|libm\.so|libdl\.so|libpthread\.so|librt\.so|libresolv\.so|libutil\.so|libnsl\.so\.1(?:\.|$)|libgcc_s\.so|libstdc\+\+\.so)')
def is_elf(path):
    with path.open('rb') as stream:
        header = stream.read(18)
        # Python's config directory also carries relocatable .o files. Only
        # executables/shared objects have a dynamic loader/RPATH to rewrite.
        if len(header) != 18 or header[:4] != b'\x7fELF':
            return False
        byteorder = 'little' if header[5] == 1 else 'big'
        return int.from_bytes(header[16:18], byteorder) in (2, 3)

def load_segments(path):
    with path.open('rb') as stream:
        header = stream.read(64)
        if header[4] != 2:
            sys.exit('Expected a 64-bit release ELF: ' + str(path))
        order = 'little' if header[5] == 1 else 'big'
        table = int.from_bytes(header[32:40], order)
        size = int.from_bytes(header[54:56], order)
        count = int.from_bytes(header[56:58], order)
        result = []
        for index in range(count):
            stream.seek(table + size * index)
            segment = stream.read(size)
            if int.from_bytes(segment[:4], order) == 1:
                result.append(tuple(int.from_bytes(segment[a:b], order) for a, b in [(8, 16), (16, 24), (48, 56)]))
        return result

elfs = [p for p in root.rglob('*') if p.is_file() and not p.is_symlink() and is_elf(p)]
packages = set()
for elf in elfs:
    result = subprocess.run(['ldd', str(elf)], text=True, capture_output=True)
    if 'not found' in result.stdout:
        sys.exit(str(elf) + ': missing dependency\n' + result.stdout)
    for filename in re.findall(r'=> (/\S+)', result.stdout):
        source = Path(filename)
        if exclude.match(source.name):
            continue
        target = libdir / source.name
        if not target.exists():
            shutil.copy2(source, target)
        owner = subprocess.run(['dpkg-query', '-S', str(source)], text=True, capture_output=True)
        if owner.returncode == 0:
            packages.add(owner.stdout.split(': ')[0].split(':')[0])
# Each extension/helper gets an RPATH relative to its own installed location.
for elf in root.rglob('*'):
    if elf.is_file() and not elf.is_symlink() and is_elf(elf):
        # strip must precede patchelf: old binutils can corrupt an ELF after
        # RPATH growth moves its dynamic string table to a new segment.
        subprocess.run(['strip', '--strip-unneeded', str(elf)], check=True)
        rel = os.path.relpath(libdir, elf.parent)
        # Old glibc enforces each PT_LOAD's advertised alignment. patchelf
        # 0.18 otherwise appends a 4 KiB offset to some 2 MiB-aligned x86 DSOs.
        page = max([4096] + [align for _, _, align in load_segments(elf)])
        subprocess.run(['patchelf', '--page-size', str(page), '--set-rpath', '$ORIGIN/' + rel, str(elf)], check=True)
        for offset, address, align in load_segments(elf):
            if align > 1 and (address - offset) % align:
                sys.exit('Invalid rewritten ELF load alignment: ' + str(elf))
notice = root / 'usr/share/doc/starmux/third-party'
notice.mkdir(parents=True, exist_ok=True)
for package in sorted(packages | {'python3.9', 'tmux', 'pulseaudio-utils', 'dbus', 'alsa-plugins'}):
    copyright_file = Path('/usr/share/doc') / package / 'copyright'
    if copyright_file.exists():
        shutil.copy2(copyright_file, notice / (package + '.copyright'))
with (notice / 'debian-packages.txt').open('w') as stream:
    subprocess.run(['dpkg-query', '-W', '-f=${binary:Package}\t${Version}\t${source:Package}\t${source:Version}\n'], stdout=stream, check=True)
