#!/usr/bin/env python3
"""Exercise a real XFCE StatusNotifier tray and the owned settings window on the single-screen X11 baseline.

Usage: verify-tray.py PACKAGE_DIRECTORY NEW_OUTPUT_DIRECTORY
Requires the real session tray host, python3-dbus/python3-gi, X11 tools, and the
verify-x11.py dependencies. Copies the test package; never changes user saves.
"""
import hashlib
import json
from pathlib import Path
import runpy
import shutil
import sqlite3
import subprocess
import sys
import time

import dbus

ROOT = Path(__file__).resolve().parents[2]
PROPERTIES = "org.freedesktop.DBus.Properties"


def wait(check, description, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(.15)
    raise AssertionError(description)


def main():
    source, output = [Path(p).resolve() for p in sys.argv[1:]]
    output.mkdir(parents=True, exist_ok=False)
    package = output / "DesktopPet"
    package.mkdir()
    manifest = json.loads((source / "package-manifest.json").read_text())
    for name in manifest["files"]:
        relative = Path(name)
        assert not relative.is_absolute() and ".." not in relative.parts
        origin = source / relative
        assert origin.is_file() and not origin.is_symlink()
        assert hashlib.sha256(origin.read_bytes()).hexdigest() == manifest["files"][name], f"package checksum mismatch: {name}"
        target = package / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(origin, target)
    shutil.copy2(source / "package-manifest.json", package / "package-manifest.json")
    (package / "data").mkdir()
    (package / "data/placement.json").write_text(json.dumps(dict(version=1, monitor="VNC-0", x=.12, y=.15, floor=False)))
    native = runpy.run_path(str(ROOT / "tools/linux/verify-x11.py"))["X11"]()
    log_path = output / "main.log"
    log = log_path.open("w")
    app = None
    host_pid = None
    bus = dbus.SessionBus(private=True)
    daemon = dbus.Interface(bus.get_object("org.freedesktop.DBus", "/org/freedesktop/DBus"), "org.freedesktop.DBus")
    try:
        assert bus.name_has_owner("org.kde.StatusNotifierWatcher"), "real tray host required"
        watcher = bus.get_object("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher")
        assert watcher.Get("org.kde.StatusNotifierWatcher", "IsStatusNotifierHostRegistered", dbus_interface=PROPERTIES)
        app = subprocess.Popen([str(package / "run.sh")], stdout=log, stderr=log)

        def own_item():
            items = watcher.Get("org.kde.StatusNotifierWatcher", "RegisteredStatusNotifierItems", dbus_interface=PROPERTIES)
            for item in items:
                name = str(item).split("/", 1)[0]
                try:
                    if int(daemon.GetConnectionUnixProcessID(name)) == app.pid:
                        return str(item)
                except dbus.DBusException:
                    pass
            return None

        item = wait(own_item, "owned tray icon failed to register")
        service, _, path = item.partition("/")
        props = bus.get_object(service, "/" + path).GetAll("org.kde.StatusNotifierItem", dbus_interface=PROPERTIES)
        assert props["Status"] == "Active"
        assert Path(str(props["IconName"])).is_file(), "tray icon pixels missing"
        menu = dbus.Interface(bus.get_object(service, str(props["Menu"])), "com.canonical.dbusmenu")
        _, layout = menu.GetLayout(0, -1, dbus.Array([], signature="s"))
        labels = {}

        def visit(node):
            if "label" in node[1]:
                labels[str(node[1]["label"]).replace("_", "")] = int(node[0])
            for child in node[2]:
                visit(child)

        visit(layout)
        expected = ["显示角色", "隐藏角色", "角色与设置…", "重试连接", "停止当前互动", "开启免打扰", "关闭免打扰", "退出 DesktopPet"]
        assert all(label in labels for label in expected), labels

        def click(label):
            menu.Event(labels[label], "clicked", dbus.Int32(0), dbus.UInt32(0))

        def ready_pid():
            for row in log_path.read_text().splitlines():
                try:
                    event = json.loads(row)
                except ValueError:
                    continue
                if event.get("event") == "app_status" and event["status"]["phase"] == "ready":
                    return event["status"]["host_pid"]

        host_pid = wait(ready_pid, "main never connected to native avatar")
        pet = wait(native.find_pet, "native avatar window missing")
        assert native.viewable(pet)
        click("隐藏角色")
        wait(lambda: not native.viewable(pet), "tray hide did not unmap avatar")
        click("显示角色")
        wait(lambda: native.viewable(pet), "tray show did not map avatar")
        click("角色与设置…")
        settings = wait(lambda: native.find_window("DesktopPet · 角色与设置".encode()), "settings window missing")
        wait(lambda: native.viewable(settings), "tray settings did not map window")
        native.request_close(settings)
        wait(lambda: not native.viewable(settings), "settings close did not hide window")
        assert app.poll() is None and native.viewable(pet), "closing settings stopped application"
        click("角色与设置…")
        wait(lambda: native.viewable(settings), "tray failed to reopen closed settings")

        database = package / "data/care.sqlite3"

        def dnd(value):
            with sqlite3.connect(database) as connection:
                row = connection.execute("SELECT value FROM settings WHERE key='companion'").fetchone()
            return row and json.loads(row[0])["do_not_disturb"] == value

        click("开启免打扰")
        wait(lambda: dnd(True), "tray DND did not persist")
        click("关闭免打扰")
        wait(lambda: dnd(False), "tray DND disable did not persist")
        click("停止当前互动")
        native.screenshot(settings, output / "settings.ppm")
        click("退出 DesktopPet")
        assert app.wait(timeout=10) == 0
        wait(lambda: not Path(f"/proc/{host_pid}").exists(), "native host survived menu quit")
        result = dict(passed=True, scope="real XFCE StatusNotifier and D-BusMenu; owned settings-window lifecycle; isolated test saves; GUI button actions remain manual",
                      menu_labels=expected, checks=["tray registration and icon pixels", "tray show/hide", "settings close and tray reopen", "DND persistence", "menu quit and child cleanup"],
                      workarea=native.workarea(), audio_tested=False)
        (output / "results.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
        print(json.dumps(result, ensure_ascii=False))
    finally:
        if app is not None and app.poll() is None:
            app.terminate()
            app.wait(timeout=10)
        bus.close()
        log.close()
        native.close(restore_pointer=False)


if __name__ == "__main__":
    main()
