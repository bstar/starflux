"""A private directory for a test that leaves no processes behind.

STAR/FLUX keeps terminals in a native source service that outlives the
windows showing them, by design. A test that only stops what it started
directly leaves that service, its supervisors and their shells running.
Every such process inherits an XDG directory inside the test's private
directory, which nothing else uses, so cleanup stops each of them and then
removes the directory.
"""
import os, shutil, signal, tempfile, time
from pathlib import Path

XDG = (b'XDG_RUNTIME_DIR', b'XDG_DATA_HOME', b'XDG_CONFIG_HOME')


def _start(pid):
    return Path('/proc', str(pid), 'stat').read_text().rsplit(')', 1)[1].split()


def private_processes(directory):
    prefix = os.fsencode(os.path.realpath(directory)).rstrip(b'/') + b'/'
    found = []
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit() or int(proc.name) == os.getpid():
            continue
        try:
            environment = (proc / 'environ').read_bytes().split(b'\0')
            fields = _start(proc.name)
        except (OSError, IndexError):
            continue
        if fields[0] == 'Z':
            continue
        for item in environment:
            key, _, value = item.partition(b'=')
            if key in XDG and (os.path.realpath(value) + b'/').startswith(prefix):
                found.append((int(proc.name), fields[19]))
                break
    return found


def _running(owner):
    try:
        fields = _start(owner[0])
        return fields[0] != 'Z' and fields[19] == owner[1]
    except (OSError, IndexError):
        return False


def stop_private_processes(directory):
    for stop_signal in (signal.SIGTERM, signal.SIGKILL):
        owners = private_processes(directory)
        for owner in owners:
            try:
                os.kill(owner[0], stop_signal)
            except ProcessLookupError:
                pass
        deadline = time.monotonic() + 3
        while any(map(_running, owners)) and time.monotonic() < deadline:
            time.sleep(.05)


class ProofDirectory(tempfile.TemporaryDirectory):
    """Stops the processes living in this directory, then removes it. Detached
    services may write briefly while they stop, so removal is retried."""

    def cleanup(self):
        self._finalizer.detach()
        stop_private_processes(self.name)
        for _ in range(50):
            try:
                shutil.rmtree(self.name)
                return
            except FileNotFoundError:
                return
            except OSError:
                time.sleep(.1)
        shutil.rmtree(self.name, ignore_errors=True)
