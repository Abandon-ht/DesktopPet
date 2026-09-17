#!/usr/bin/env python3
"""Opt-in macOS desktop process tests. Requires a local model and unlocked session."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
MODEL = str(Path(sys.argv[1]).resolve(strict=True))
OUTPUT = ROOT / "artifacts/local/p0/bridge"
OUTPUT.mkdir(parents=True, exist_ok=True)
HOST = ROOT / "target/release/p0-probe"
APP = ROOT / "target/release/p0-tauri-probe"


def message(kind="hello", seq=1, version=1):
    return json.dumps(dict(protocol_version=version, session_id="render-test",
                           request_id=f"r-{seq}", sequence=seq, type=kind, payload={})).encode() + b"\n"


for name, data, code, expected in [
    ("host-normal", message() + message("ping", 2) + message("shutdown", 3), 0, ["ready", "pong", "stopped"]),
    ("host-eof", message(), 0, ["ready"]),
    ("host-version", message(version=2), 1, []),
]:
    result = subprocess.run([str(HOST), "host", MODEL], input=data, capture_output=True, timeout=20)
    (OUTPUT / f"{name}.log").write_bytes(result.stderr)
    assert result.returncode == code, (name, result.stderr[-2000:])
    assert [json.loads(s)["type"] for s in result.stdout.splitlines()] == expected
    if code:
        assert b"unsupported_protocol_version" in result.stderr
    else:
        assert b'"event":"ready"' in result.stderr
    print("PASS", name, flush=True)


def rows(path):
    result = []
    for line in path.read_text(errors="replace").splitlines(keepends=True):
        # A live append may expose the last partial line; complete JSON records
        # must parse, otherwise logging corruption must fail the verification.
        if line.endswith("\n") and line.startswith("{"):
            result.append(json.loads(line))
    return result


def wait_ready(path, app, count=1):
    until = time.monotonic() + 20
    while time.monotonic() < until:
        ready = [r for r in rows(path) if r.get("event") == "bridge_ready"]
        if len(ready) >= count:
            return ready
        assert app.poll() is None, path.read_text(errors="replace")[-3000:]
        time.sleep(0.05)
    raise AssertionError("bridge readiness timeout")


def alive(pid):
    return bool(subprocess.run(["ps", "-p", str(pid), "-o", "pid="], capture_output=True).stdout.strip())


for scenario in ["normal", "recover", "parent-death", "restart-budget"]:
    path = OUTPUT / f"tauri-{scenario}.log"
    env = dict(os.environ, P0_MODEL=MODEL, P0_AUTO_EXIT_SECONDS="20" if scenario == "restart-budget" else "8")
    with path.open("wb") as log:
        app = subprocess.Popen([str(APP)], env=env, stdout=log, stderr=log)
        ready = []
        try:
            ready = wait_ready(path, app)
            if scenario == "recover":
                os.kill(ready[0]["host_pid"], signal.SIGKILL)
                ready = wait_ready(path, app, 2)
                assert ready[1]["host_pid"] != ready[0]["host_pid"]
            if scenario == "restart-budget":
                for attempt in range(1, 4):
                    ready = wait_ready(path, app, attempt)
                    os.kill(ready[-1]["host_pid"], signal.SIGKILL)
            if scenario == "parent-death":
                app.kill()
                app.wait(timeout=5)
            elif scenario == "restart-budget":
                code = app.wait(timeout=15)
                assert code == 1, f"restart-budget exit code {code}: {path.read_text(errors='replace')[-2000:]}"
                events = rows(path)
                assert len([r for r in events if r.get("event") == "bridge_ready"]) == 3
                assert any(r.get("event") == "bridge_failed" and "restart budget exhausted" in r["error"] for r in events)
            else:
                assert app.wait(timeout=20) == 0, path.read_text(errors="replace")[-3000:]
                assert any(r.get("event") == "bridge_exit_requested" for r in rows(path))
                assert any(r.get("event") == "bridge_stopped" and r["code"] == 0 for r in rows(path))
            until = time.monotonic() + 5
            while any(alive(r["host_pid"]) for r in ready) and time.monotonic() < until:
                time.sleep(0.05)
            assert not any(alive(r["host_pid"]) for r in ready), "orphan rendering host"
            print("PASS tauri", scenario, flush=True)
        finally:
            if app.poll() is None:
                app.kill()
                app.wait(timeout=5)
            for r in rows(path):
                if r.get("event") == "bridge_ready" and alive(r["host_pid"]):
                    os.kill(r["host_pid"], signal.SIGKILL)

print("PASS: real renderer normal/EOF/version and Tauri normal/recovery/parent-death/restart-budget")
