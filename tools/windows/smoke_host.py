"""Exercise the real Windows renderer over IPC without moving the user's cursor."""
import argparse
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--log", type=Path, required=True)
    args = parser.parse_args()
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user32.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.GetForegroundWindow.restype = wintypes.HWND

    with tempfile.TemporaryDirectory(prefix="desktop-pet-smoke-") as temp:
        layout = Path(temp) / "placement.json"
        environment = os.environ.copy()
        environment["DESKTOPPET_LAYOUT"] = str(layout)
        with args.log.open("wb") as log:
            process = subprocess.Popen([str(args.host.resolve()), str(args.model.resolve())],
                cwd=temp, env=environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                stderr=log, creationflags=subprocess.CREATE_NO_WINDOW)
            replies = queue.Queue()

            def read():
                for line in process.stdout:
                    replies.put(line)
                replies.put(None)

            threading.Thread(target=read, daemon=True).start()
            sequence = 0

            def exchange(kind, payload=None, expected="result"):
                nonlocal sequence
                sequence += 1
                request = dict(protocol_version=6, session_id="windows-smoke", sequence=sequence,
                    request_id="smoke-" + str(sequence), type=kind, payload=payload or {})
                process.stdin.write(json.dumps(request).encode("utf-8") + b"\n")
                process.stdin.flush()
                line = replies.get(timeout=6)
                if line is None:
                    raise RuntimeError("host EOF; inspect " + str(args.log))
                reply = json.loads(line)
                assert all(reply[key] == request[key] for key in ("protocol_version", "sequence", "session_id", "request_id")), reply
                assert reply["type"] == expected, reply
                if expected == "result":
                    assert reply["payload"]["accepted"], reply
                return reply["payload"]

            def visible_count():
                windows = []

                @callback_type
                def visit(hwnd, _):
                    pid = wintypes.DWORD()
                    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
                    if pid.value == process.pid and user32.IsWindowVisible(hwnd):
                        title = ctypes.create_unicode_buffer(256)
                        user32.GetWindowTextW(hwnd, title, len(title))
                        windows.append(title.value)
                    return True

                user32.EnumWindows(visit, 0)
                return sum(title == "DesktopPet" for title in windows)

            def visibility(expected):
                deadline = time.monotonic() + 2
                while visible_count() != expected and time.monotonic() < deadline:
                    time.sleep(0.05)
                assert visible_count() == expected, ("visible windows", visible_count(), expected)

            try:
                ready = exchange("hello", expected="ready")
                assert ready["max_frame_bytes"] == 256 * 1024
                visibility(0)
                exchange("desktop", {"type": "set_scale", "payload": 75})
                exchange("desktop", {"type": "set_external_snap_enabled", "payload": True})
                foreground = user32.GetForegroundWindow()
                exchange("desktop", {"type": "set_visible", "payload": True})
                visibility(1)
                assert user32.GetForegroundWindow() == foreground, "show stole keyboard focus"
                for _ in range(10):
                    exchange("ping", expected="pong")
                    time.sleep(0.1)
                exchange("desktop", {"type": "set_visible", "payload": False})
                visibility(0)
                assert layout.is_file(), "placement was not saved"
                assert json.loads(layout.read_text())["version"] == 1
                exchange("shutdown", expected="stopped")
                assert process.wait(timeout=5) == 0
                print("PASS: real renderer, IPC, show without focus, heartbeat, hide, placement, shutdown")
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)


if __name__ == "__main__":
    main()
