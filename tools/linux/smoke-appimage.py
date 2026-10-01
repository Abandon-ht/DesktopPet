#!/usr/bin/env python3
"""Launch a public AppImage on an isolated Xvfb/D-Bus session without a model."""
import json
import os
from pathlib import Path
import runpy
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]


def main():
    image, report = [Path(p).resolve() for p in sys.argv[1:]]
    with tempfile.TemporaryDirectory(prefix="desktoppet-appimage-smoke-") as temp:
        directory = Path(temp)
        env = dict(os.environ, GDK_BACKEND="x11", APPIMAGE_EXTRACT_AND_RUN="1")
        for key, name in (("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"),
                          ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")):
            path = directory / name
            path.mkdir(mode=0o700)
            env[key] = str(path)
        for key in ("DESKTOPPET_MODEL", "DESKTOPPET_RESOURCE_DIR"):
            env.pop(key, None)
        logpath = directory / "app.log"
        with logpath.open("w") as log:
            app = subprocess.Popen([str(image), "--appimage-extract-and-run"],
                cwd=directory, env=env, stdout=log, stderr=log, start_new_session=True)
            native = None
            try:
                native = runpy.run_path(str(ROOT / "tools/linux/verify-x11.py"))["X11"]()
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline:
                    if app.poll() is not None:
                        raise AssertionError(f"AppImage exited during startup: {logpath.read_text()}")
                    window = native.find_window("DesktopPet · 角色与设置".encode())
                    if window and native.viewable(window):
                        break
                    time.sleep(.2)
                else:
                    raise AssertionError(f"settings window did not map: {logpath.read_text()}")
                # Wait for the main process, beyond the native window creation.
                time.sleep(3)
                assert app.poll() is None, logpath.read_text()
                events = []
                for row in logpath.read_text().splitlines():
                    try:
                        events.append(json.loads(row))
                    except ValueError:
                        pass
                assert any(e.get("event") == "app_status" for e in events), logpath.read_text()
                assert list((directory / "data").rglob("care.sqlite3")), "per-user persistence missing"
                report.write_text(json.dumps(dict(passed=True,
                    scope="FUSE-free AppImage startup, mapped settings and per-user saves on Xvfb; no character/GPU/audio/Wayland test"), indent=2) + "\n")
                print(report.read_text())
            finally:
                if app.poll() is None:
                    os.killpg(app.pid, signal.SIGTERM)
                    try:
                        app.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        os.killpg(app.pid, signal.SIGKILL)
                        app.wait(timeout=5)
                if native is not None:
                    native.close(restore_pointer=False)
                report.with_suffix(".log").write_text(logpath.read_text())


if __name__ == "__main__":
    main()
