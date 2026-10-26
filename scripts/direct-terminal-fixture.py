#!/usr/bin/env python3
"""Interactive terminal fixture: alternate screen, menus, queries and bracketed paste."""
import json
import os
from pathlib import Path
import select
import sys
import termios
import tty

log = Path(sys.argv[1])
old = termios.tcgetattr(0)
tty.setraw(0)
def record(kind, value):
    with log.open('a') as f:
        f.write(json.dumps({'pid':os.getpid(), 'kind':kind, 'value':value})+'\n')
def draw():
    sys.stdout.write('\x1b[H\x1b[2J\x1b[1;36mDirect terminal menu\x1b[0m\r\nUnicode: café 世界 é\r\n\r\n')
    for i in range(3):
        sys.stdout.write(('\x1b[7m' if i == choice else '')+f'  Option {i+1}  '+'\x1b[0m\r\n')
    sys.stdout.write('\r\nUse arrows and Enter; file paths paste without submission.\r\n')
    sys.stdout.flush()
choice=0
buffer=b''
try:
    sys.stdout.write('\x1b[?1049h\x1b[?2004h\x1b[6n')
    sys.stdout.flush()
    draw()
    while True:
        buffer += os.read(0, 65536)
        while buffer:
            if buffer.startswith(b'\x1b[200~'):
                end=buffer.find(b'\x1b[201~')
                if end < 0: break
                record('paste',buffer[6:end].decode())
                buffer=buffer[end+6:]
            elif buffer.startswith((b'\x1b[A',b'\x1b[B',b'\x1bOA',b'\x1bOB')):
                choice=(choice+(1 if buffer[2:3]==b'B' else -1))%3
                buffer=buffer[3:]
                record('selection',choice)
                draw()
            elif buffer[:1]==b'\r':
                record('submitted',choice);buffer=buffer[1:]
            elif buffer[:1]==b'\x1b':
                # Cursor query replies may arrive in separate PTY reads.
                if b'R' in buffer:
                    end=buffer.index(b'R');record('cursor',buffer[:end+1].decode());buffer=buffer[end+1:]
                else: break
            else:
                record('byte',buffer[:1].decode(errors='replace'));buffer=buffer[1:]
finally:
    sys.stdout.write('\x1b[?2004l\x1b[?1049l');sys.stdout.flush()
    termios.tcsetattr(0,termios.TCSANOW,old)
