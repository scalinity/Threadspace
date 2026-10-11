#!/usr/bin/env python3
"""Disposable non-measuring HTTP Unix socket; one nonce, no clock samples."""
from __future__ import annotations
import ctypes
import json
import os
import signal
import socket
from pathlib import Path

LIMIT = 4


def acknowledgement(config):
    return {"kind": "ACTIVATION_ONLY_READY", **{k: config[k] for k in
        ("runId", "responderId", "nonce", "activationSha256", "fixtureSha256", "observerSha256", "bootId")}}


def reply(request, config, used):
    expected = "/" + "/".join(("ready", config["runId"], config["responderId"], config["nonce"], config["activationSha256"]))
    if request != f"GET {expected} HTTP/1.1": return 403, {"kind": "READINESS_REJECTED"}, False
    if used: return 409, {"kind": "READINESS_REPLAY_REJECTED"}, False
    return 200, acknowledgement(config), True


def serve(root, config):
    root = Path(root).resolve(strict=True)
    if root.stat().st_uid != os.getuid() or root.stat().st_mode & 0o777 != 0o700:
        raise RuntimeError("readiness directory is not private and owned")
    transcript = (root / "readiness.jsonl").open("x")
    os.chmod(root / "readiness.jsonl", 0o600)
    def record(value):
        transcript.write(json.dumps(value, sort_keys=True, allow_nan=False) + "\n")
        transcript.flush(); os.fsync(transcript.fileno())
    path = root / "s.sock"
    if path.exists() or path.is_symlink(): raise RuntimeError("readiness socket already exists")
    lib = ctypes.CDLL(None)
    lib.getpeereid.argtypes = (ctypes.c_int, ctypes.POINTER(ctypes.c_uint), ctypes.POINTER(ctypes.c_uint))
    stop = False
    def terminate(signum, frame):
        nonlocal stop
        stop = True
    signal.signal(signal.SIGTERM, terminate)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    attempts = answered = accepted = generated = 0
    used = False
    try:
        server.bind(str(path)); os.chmod(path, 0o600); server.listen(2); server.settimeout(.1)
        record({"event": "start", "pid": os.getpid(), "uid": os.getuid(), "identity": acknowledgement(config)})
        while not stop and attempts < LIMIT:
            try: connection, _ = server.accept()
            except socket.timeout: continue
            attempts += 1
            with connection:
                uid, gid = ctypes.c_uint(), ctypes.c_uint()
                if lib.getpeereid(connection.fileno(), ctypes.byref(uid), ctypes.byref(gid)) != 0 or uid.value != os.getuid():
                    record({"event": "incomplete", "attempt": attempts, "reason": "foreign-peer"}); continue
                connection.settimeout(1)
                request = b""
                try:
                    while b"\r\n\r\n" not in request and len(request) < 4096:
                        part = connection.recv(4096 - len(request))
                        if not part: break
                        request += part
                    if b"\r\n\r\n" not in request: raise ValueError("incomplete-header")
                    line = request.split(b"\r\n", 1)[0].decode("ascii")
                    status, body, success = reply(line, config, used)
                    used |= success; accepted += int(success)
                    record({"event": "received", "attempt": attempts, "peerUid": uid.value, "status": status, "nonceMatched": success})
                    data = json.dumps(body, sort_keys=True, separators=(",", ":")).encode()
                    generated += 1
                    record({"event": "generated", "attempt": attempts, "status": status, "body": body})
                    response = f"HTTP/1.1 {status} Ready\r\nContent-Type: application/json\r\nContent-Length: {len(data)}\r\nConnection: close\r\n\r\n".encode() + data
                    connection.sendall(response); answered += 1
                    record({"event": "answered", "attempt": attempts, "status": status})
                except (OSError, ValueError, UnicodeError) as error:
                    record({"event": "incomplete", "attempt": attempts, "reason": type(error).__name__})
    finally:
        server.close()
        if path.exists(): path.unlink()
        record({"event": "stop", "attempts": attempts, "generated": generated, "answered": answered, "accepted": accepted})
        transcript.close()


if __name__ == "__main__":
    import sys
    root = Path(sys.argv[1])
    serve(root, json.loads((root / "readiness-config.json").read_text()))
