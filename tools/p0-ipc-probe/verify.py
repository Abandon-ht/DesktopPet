#!/usr/bin/env python3
"""Bounded real-process protocol tests; no assets or desktop interaction."""
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
BIN = ROOT / "target/release/p0-ipc-probe"


def request(kind="hello", sequence=1, **overrides):
    value = dict(protocol_version=1, session_id="p0-test", sequence=sequence,
                 request_id=f"r-{sequence}", type=kind, payload={})
    value.update(overrides)
    return json.dumps(value).encode() + b"\n"


def run(data, error=None, replies=()):
    result = subprocess.run([str(BIN)], input=data, capture_output=True, timeout=5)
    if error:
        assert result.returncode != 0, result
        assert error.encode() in result.stderr, result.stderr
    else:
        assert result.returncode == 0, result.stderr
        assert not result.stderr, result.stderr
        frames = [json.loads(line) for line in result.stdout.splitlines()]
        assert [frame["type"] for frame in frames] == list(replies), frames
        for index, frame in enumerate(frames, 1):
            assert frame["protocol_version"] == 1
            assert frame["session_id"] == "p0-test"
            assert frame["sequence"] == index
            assert frame["request_id"] == f"r-{index}"


run(request() + request("ping", 2) + request("shutdown", 3),
    replies=("ready", "pong", "stopped"))
run(request(), replies=("ready",))  # EOF after handshake.
run(b"")  # EOF before handshake.
for data, error in [
    (request(protocol_version=2), "unsupported_protocol_version"),
    (request("ping"), "handshake_required"),
    (request() + request("hello", 2), "unsupported_command"),
    (request() + request("ping", 1), "invalid_sequence"),
    (request() + request("ping", 2, session_id="other"), "session_mismatch"),
    (b"{\n", "invalid_json"),
    (request().rstrip(b"\n"), "truncated_frame"),
    (b"x" * (256 * 1024 + 1), "frame_too_large"),
    (request(payload=None), "invalid_payload"),
]:
    run(data, error)

# Kill a parent that owns stdin; its child must see EOF. This verifier keeps
# the stdout read end only, so it does not accidentally keep child stdin alive.
parent_code = '''
import subprocess, sys, time
child = subprocess.Popen([sys.argv[1]], stdin=subprocess.PIPE, stdout=sys.stdout)
print(child.pid, flush=True)
time.sleep(30)
'''
parent = subprocess.Popen([sys.executable, "-c", parent_code, str(BIN)],
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE)
try:
    # communicate has a deadline; force parent death without waiting for its sleep.
    import select
    assert select.select([parent.stdout], [], [], 5)[0], "parent startup timeout"
    child_pid = int(parent.stdout.readline())
    parent.kill()
    stdout, stderr = parent.communicate(timeout=5)
    # EOF of inherited stdout proves child released the pipe after parent death.
    assert stdout == b"" and stderr == b"", (stdout, stderr)
finally:
    if parent.poll() is None:
        parent.kill()
        parent.communicate(timeout=5)

print("PASS: 12 protocol/EOF cases and parent-death pipe cleanup")
