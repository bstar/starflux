#!/usr/bin/env python3
"""Verify Handy launch while a private authenticated SSH app viewer stays attached."""
import base64
import getpass
import hashlib
import json
import os
from pathlib import Path
import shlex
import socket
import struct
import subprocess
import tempfile
import threading
import time
import uuid

repo = Path(__file__).resolve().parents[1]
app = repo / "scripts/run-local.sh"
game = Path("/home/bstar/projects/zendocon-decompile")
out = repo / "target/handy-ssh-proof"
out.mkdir(parents=True, exist_ok=True)
session = "handy-ssh-" + uuid.uuid4().hex[:8]


def wait(predicate, label, timeout=25):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(.1)
    raise AssertionError(label)


with tempfile.TemporaryDirectory(prefix="handy-ssh-") as tmp:
    tmp = Path(tmp)
    for name in ["host", "client"]:
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(tmp / name)], check=True)
    (tmp / "authorized_keys").write_text((tmp / "client.pub").read_text())
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    (tmp / "known_hosts").write_text(f"[127.0.0.1]:{port} " + (tmp / "host.pub").read_text())
    (tmp / "sshd_config").write_text(f"Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM yes\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n")
    (tmp / "ssh_config").write_text(f"Host handy-test\n HostName 127.0.0.1\n Port {port}\n User {getpass.getuser()}\n IdentityFile {tmp}/client\n IdentitiesOnly yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {tmp}/known_hosts\n")
    ssh = ["ssh", "-F", str(tmp / "ssh_config"), "-T", "handy-test"]
    daemon = subprocess.Popen(["/usr/bin/sshd", "-D", "-e", "-f", str(tmp / "sshd_config")], stderr=(out / "sshd.log").open("w"), env={k:v for k,v in os.environ.items() if k != "NOTIFY_SOCKET"})
    viewer = None
    packets = []

    def connect():
        process = subprocess.Popen(ssh + [shlex.join([str(app), "--desktop-relay", session])], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=(out / "relay.log").open("a"))

        def read():
            def exact(size):
                data = b""
                while len(data) < size:
                    part = process.stdout.read(size-len(data))
                    if not part:
                        raise EOFError
                    data += part
                return data
            try:
                while True:
                    size = struct.unpack(">I", exact(4))[0]
                    assert 0 < size <= 24*1024*1024
                    packets.append(json.loads(exact(size)))
            except (EOFError, OSError):
                pass
        threading.Thread(target=read, daemon=True).start()
        return process

    try:
        wait(lambda: subprocess.run(ssh+["true"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0, "Private SSH failed")
        viewer = connect()
        wait(lambda: any(p["type"] == "hello" for p in packets), "No desktop handshake")
        worker = next(p["pid"] for p in packets if p["type"] == "hello")
        command = [str(app), "--app-launch", session, "--app-directory", str(game), "--", "python3", "tools/run_handy.py", "--app-space", "build/lynx/engine/live-effects-probe/engine-probe.lnx"]
        windows = json.loads(subprocess.check_output(ssh + [shlex.join(command)], timeout=30))
        assert len(windows) == 1 and windows[0]["app_id"] == "handy_sdl", windows
        wait(lambda: any(p["type"] == "windows" and p["windows"] == windows for p in packets), "Viewer lost live window notifications")
        wait(lambda: any(p["type"] == "frame" and len(p["png"]) > 150000 for p in packets), "Cartridge title did not render", timeout=40)
        time.sleep(1)
        frames = [p for p in packets if p["type"] == "frame"]
        assert len(frames) >= 2
        (out / "title.png").write_bytes(base64.b64decode(frames[-1]["png"]))
        # Handy replaces its initial SDL window while initializing the cartridge.
        windows = json.loads(subprocess.check_output(ssh + [shlex.join([str(app), "--app-list", session])], timeout=10))
        # Start through the live SSH input path; only the disposable game receives keys.
        data = json.dumps({"type":"focus", "window":windows[0]["id"]}).encode()
        viewer.stdin.write(struct.pack(">I", len(data))+data)
        viewer.stdin.flush()
        for pressed in [True, False]:
            data = json.dumps({"type":"input", "input":{"kind":"named_key", "code":"char:.", "pressed":pressed, "modifiers":0}}).encode()
            viewer.stdin.write(struct.pack(">I", len(data))+data)
            viewer.stdin.flush()
            time.sleep(.8)
        start = len(packets)
        time.sleep(3)
        changed = {hashlib.sha256(p["png"].encode()).hexdigest() for p in packets[start:] if p["type"] == "frame"}
        assert len(changed) > 3, "Game frames did not update"
        (out / "game.png").write_bytes(base64.b64decode(next(p["png"] for p in reversed(packets) if p["type"] == "frame")))
        before = json.loads(subprocess.check_output(ssh + [shlex.join([str(app), "--app-list", session])], timeout=10))
        assert {w["id"] for w in before} == {w["id"] for w in windows}, (before, windows)
        windows = before
        viewer.terminate()
        viewer.wait(timeout=10)
        packets.clear()
        time.sleep(.3)
        viewer = connect()
        wait(lambda: any(p["type"] == "windows" and {w["id"] for w in p["windows"]} == {w["id"] for w in windows} for p in packets), "Windows did not survive reconnect")
        assert next(p["pid"] for p in packets if p["type"] == "hello") == worker
        wait(lambda: any(p["type"] == "frame" for p in packets), "No frame after reconnect")
        audio_packets=[]
        audio=subprocess.Popen(ssh+[shlex.join([str(app),"--desktop-audio-relay",session,str(worker)])],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(out/"audio.log").open("w"))
        def audio_read():
            def exact(n):
                value=b""
                while len(value)<n:
                    part=audio.stdout.read(n-len(value))
                    if not part:raise EOFError
                    value+=part
                return value
            try:
                while True:audio_packets.append(json.loads(exact(struct.unpack(">I",exact(4))[0])))
            except (EOFError,OSError):pass
        threading.Thread(target=audio_read,daemon=True).start()
        try:
            wait(lambda:any(p["type"]=="ready" for p in audio_packets),"No Handy audio handshake")
            peak=0
            for _ in range(16):
                count=len([p for p in audio_packets if p["type"]=="chunk"])
                message=json.dumps({"blocks":8}).encode();audio.stdin.write(struct.pack(">I",len(message))+message);audio.stdin.flush()
                wait(lambda:len([p for p in audio_packets if p["type"]=="chunk"])>=count+8,"Handy audio credits stalled")
                peak=max((abs(x)for p in audio_packets if p["type"]=="chunk" for x in p["samples"]),default=0)
                if peak>100:break
            assert peak>100,("Handy soundtrack was silent",peak)
        finally:
            audio.terminate();audio.wait(timeout=10)
        result = {"handy_audio_peak_over_ssh":peak,"transport":"authenticated loopback SSH", "worker_pid":worker, "windows":windows, "launch_preserves_viewer":True, "worker_and_window_survive_reconnect":True, "distinct_game_frames_in_3_seconds":len(changed)}
        (out / "result.json").write_text(json.dumps(result, indent=2)+"\n")
        print(json.dumps(result))
    finally:
        subprocess.run(ssh + [shlex.join([str(app), "--desktop-stop", session])], timeout=10, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if viewer is not None:
            viewer.terminate()
            viewer.wait(timeout=10)
        daemon.terminate()
        daemon.wait(timeout=10)
