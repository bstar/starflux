#!/usr/bin/env python3
"""Private PTYs: stalled output must preserve the bridge, input and final bytes."""
import hashlib
import importlib.util
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import tempfile
import time
import termios
import fcntl
import tty
import sys

root = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('adopt', root / 'scripts/adopt-pty.py')
adopt = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adopt)
with tempfile.TemporaryDirectory(prefix='starflux-bridge-proof-') as temp:
    temp = Path(temp)
    original, slave = pty.openpty()
    tty.setraw(slave)
    output, terminal = pty.openpty()
    fcntl.ioctl(terminal, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
    key = 'private-proof'
    plan = dict(run=str(temp), broker=str(temp / 'owner.sock'),
                panes=[dict(pid=os.getpid(), key=key, screen='')])
    broker = adopt.Broker(plan, dict(panes=[dict(child_pid=os.getpid())]), [original])
    if '--legacy-owner' in sys.argv:
        # Retained owners predate output backpressure; the proxy must protect
        # their bounded socket queue using the source TTY's output flow control.
        def legacy_watch(key):
            t=broker.terms[key]
            if t['eof']:return
            events=(adopt.selectors.EVENT_READ if broker.committed and t['peer'] else 0)|(adopt.selectors.EVENT_WRITE if broker.committed and t['pending'] else 0)
            if t['registered']:
                if events:broker.sel.modify(t['fd'],events,('pty',key))
                else:broker.sel.unregister(t['fd']);t['registered']=False
            elif events:broker.sel.register(t['fd'],events,('pty',key));t['registered']=True
        broker.watch=legacy_watch
    broker.own()
    writer=subprocess.Popen([sys.executable,'-c',
        "import os,sys;sys.stdin.buffer.read(1);data=memoryview(bytes(range(256))*(128*1024));\nwhile data:data=data[os.write(1,data):]"],
        stdin=subprocess.PIPE,stdout=slave,stderr=subprocess.PIPE)
    proxy = subprocess.Popen([str(root / 'target/release/starflux-pty-proxy'),
                              plan['broker'], key]+([os.ttyname(slave),str(writer.pid)] if '--legacy-owner' in sys.argv else []), stdin=terminal, stdout=terminal,
                             stderr=subprocess.PIPE, start_new_session=True)
    os.close(terminal)
    try:
        deadline = time.monotonic() + 3
        while broker.terms[key]['peer'] is None:
            broker.step(.002)
            assert time.monotonic() < deadline, 'Private proxy did not attach'
        # Drain only bootstrap mode sequences before starting the payload.
        for _ in range(30):
            broker.step(.001)
            if select.select([output], [], [], 0)[0]:
                os.read(output, 65536)
        payload = bytes(range(256)) * (128 * 1024)  # 32 MiB, larger than both queues.
        writer.stdin.write(b'1');writer.stdin.close()
        stalled_until = time.monotonic() + .4
        while time.monotonic() < stalled_until:
            broker.step(.001)
            assert proxy.poll() is None, 'Output pressure disconnected the proxy'
        assert writer.poll() is None, 'Output was not backpressured'
        assert broker.terms[key]['peer'] is not None
        assert all(len(c['out']) <= adopt.QUEUE for c in broker.clients.values())
        token = b'INPUT_WHILE_OUTPUT_IS_BLOCKED'
        os.write(output, token)
        received_input = b''
        deadline = time.monotonic() + 2
        while received_input != token:
            broker.step(.001)
            if select.select([slave], [], [], 0)[0]:
                received_input += os.read(slave, len(token) - len(received_input))
            assert time.monotonic() < deadline, 'Blocked output also blocked input'
        checksum = hashlib.sha256()
        received = 0
        closed = False
        deadline = time.monotonic() + 20
        while received < len(payload) or proxy.poll() is None:
            broker.step(.001)
            if writer.poll() is not None and not closed:
                assert writer.returncode==0,writer.stderr.read().decode(errors='replace')
                os.close(slave)
                closed = True
            if select.select([output], [], [], 0)[0]:
                try:
                    data = os.read(output, 65536)
                except OSError:
                    data = b''
                checksum.update(data)
                received += len(data)
            assert time.monotonic() < deadline, 'Bridge did not drain its output'
        assert received == len(payload), (received, len(payload))
        assert checksum.digest() == hashlib.sha256(payload).digest(), 'Output was lost or reordered'
        assert proxy.returncode == 0, proxy.stderr.read().decode(errors='replace')
        print('PASS: bounded PTY backpressure, responsive input, 32 MiB exact output and final-byte drain')
    finally:
        if writer.poll() is None:writer.terminate();writer.wait(timeout=3)
        if proxy.poll() is None:
            proxy.terminate()
            proxy.wait(timeout=3)
        if not locals().get('closed'):
            os.close(slave)
        for peer in list(broker.clients):
            broker.disconnect(peer)
        if not broker.terms[key]['eof']:
            os.close(original)
        os.close(output)
        broker.listener.close()
        broker.sel.close()
