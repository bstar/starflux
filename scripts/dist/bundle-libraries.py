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
exclude = re.compile(r'^(ld-linux|libc\.so|libm\.so|libdl\.so|libpthread\.so|librt\.so|libresolv\.so|libutil\.so|libnsl\.so|libgcc_s\.so|libstdc\+\+\.so)')
def is_elf(path):
    with path.open('rb') as stream:
        return stream.read(4) == b'\x7fELF'

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
        rel = os.path.relpath(libdir, elf.parent)
        subprocess.run(['patchelf', '--set-rpath', '$ORIGIN/' + rel, str(elf)], check=True)
        subprocess.run(['strip', '--strip-unneeded', str(elf)], check=True)
notice = root / 'usr/share/doc/starmux/third-party'
notice.mkdir(parents=True, exist_ok=True)
for package in sorted(packages | {'python3.9', 'tmux', 'pulseaudio-utils', 'dbus', 'alsa-plugins'}):
    copyright_file = Path('/usr/share/doc') / package / 'copyright'
    if copyright_file.exists():
        shutil.copy2(copyright_file, notice / (package + '.copyright'))
with (notice / 'debian-packages.txt').open('w') as stream:
    subprocess.run(['dpkg-query', '-W', '-f=${binary:Package}\t${Version}\t${source:Package}\t${source:Version}\n'], stdout=stream, check=True)
