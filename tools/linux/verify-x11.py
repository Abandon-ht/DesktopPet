#!/usr/bin/env python3
"""Real X11 avatar-host checks with an owned background and XTest input.

Usage: verify-x11.py HOST PACK_MANIFEST OUTPUT_DIRECTORY
Requires DISPLAY, libX11, libXtst and libXss (and python3-dbus on XFCE).
Runs on the desktop, temporarily places a
background below the pet, then removes both windows and restores the pointer.
No input is sent to other applications. Use a private pack only on your machine.
"""
import ctypes as C
import json
import os
from pathlib import Path
import re
import select
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
VERSION = int(re.search(r"PROTOCOL_VERSION: u32 = (\d+)", (ROOT / "crates/pet-protocol/src/lib.rs").read_text())[1])
U = C.c_ulong
I = C.c_int
P = C.c_void_p


class Button(C.Structure):
    _fields_ = [("type", I), ("serial", U), ("send_event", I), ("display", P),
                ("window", U), ("root", U), ("subwindow", U), ("time", U),
                ("x", I), ("y", I), ("x_root", I), ("y_root", I),
                ("state", C.c_uint), ("button", C.c_uint), ("same_screen", I)]


class Event(C.Union):
    _fields_ = [("button", Button), ("pad", C.c_long * 24)]


class Rectangle(C.Structure):
    _fields_ = [("x", C.c_short), ("y", C.c_short), ("width", C.c_ushort), ("height", C.c_ushort)]


class ScreenSaverInfo(C.Structure):
    _fields_ = [("window", U), ("state", I), ("kind", I),
                ("since", U), ("idle", U), ("event_mask", U)]


class X11:
    def __init__(self):
        self.lib = C.CDLL("libX11.so.6")
        self.test = C.CDLL("libXtst.so.6")
        signatures = {
            "XOpenDisplay": (P, [C.c_char_p]), "XDefaultRootWindow": (U, [P]),
            "XCreateSimpleWindow": (U, [P, U, I, I, C.c_uint, C.c_uint, C.c_uint, U, U]),
            "XStoreName": (I, [P, U, C.c_char_p]), "XSelectInput": (I, [P, U, C.c_long]),
            "XMapRaised": (I, [P, U]), "XSetWindowBackground": (I, [P, U, U]),
            "XClearWindow": (I, [P, U]), "XSync": (I, [P, I]), "XPending": (I, [P]),
            "XNextEvent": (I, [P, C.POINTER(Event)]), "XDestroyWindow": (I, [P, U]),
            "XCloseDisplay": (I, [P]), "XGetInputFocus": (I, [P, C.POINTER(U), C.POINTER(I)]),
            "XResetScreenSaver": (I, [P]),
            "XQueryPointer": (I, [P, U, C.POINTER(U), C.POINTER(U), C.POINTER(I), C.POINTER(I), C.POINTER(I), C.POINTER(I), C.POINTER(C.c_uint)]),
            "XQueryTree": (I, [P, U, C.POINTER(U), C.POINTER(U), C.POINTER(C.POINTER(U)), C.POINTER(C.c_uint)]),
            "XFetchName": (I, [P, U, C.POINTER(P)]), "XFree": (I, [P]),
            "XGetGeometry": (I, [P, U, C.POINTER(U), C.POINTER(I), C.POINTER(I), C.POINTER(C.c_uint), C.POINTER(C.c_uint), C.POINTER(C.c_uint), C.POINTER(C.c_uint)]),
            "XTranslateCoordinates": (I, [P, U, U, I, I, C.POINTER(I), C.POINTER(I), C.POINTER(U)]),
            "XGetImage": (P, [P, U, I, I, C.c_uint, C.c_uint, U, I]),
            "XGetPixel": (U, [P, I, I]), "XDestroyImage": (I, [P]),
        }
        for name, (result, args) in signatures.items():
            function = getattr(self.lib, name)
            function.restype, function.argtypes = result, args
        self.test.XTestFakeMotionEvent.argtypes = [P, I, I, I, U]
        self.test.XTestFakeButtonEvent.argtypes = [P, C.c_uint, I, U]
        self.display = self.lib.XOpenDisplay(None)
        assert self.display, "cannot connect to DISPLAY"
        self.root = self.lib.XDefaultRootWindow(self.display)
        # A screensaver sits above the compositor. Check before sending input;
        # never mistake its black pixels for a transparency failure or unlock it.
        self.screensaver = C.CDLL("libXss.so.1")
        self.screensaver.XScreenSaverQueryInfo.argtypes = [P, U, C.POINTER(ScreenSaverInfo)]
        self.screensaver.XScreenSaverSuspend.argtypes = [P, I]
        info = ScreenSaverInfo()
        available = self.screensaver.XScreenSaverQueryInfo(self.display, self.root, C.byref(info))
        if not available or info.state != 0:
            self.lib.XCloseDisplay(self.display)
            raise RuntimeError("desktop screensaver is active or its status is unavailable; resume the test desktop before validation")
        self.session_bus = None
        if shutil.which("xfce4-screensaver-command"):
            # XFCE can blank independently while core XScreenSaver says Off.
            # Query only; do not change XFCE settings or attempt an unlock.
            try:
                import dbus
                self.session_bus = dbus.SessionBus(private=True)
                if self.session_bus.name_has_owner("org.xfce.ScreenSaver"):
                    proxy = self.session_bus.get_object("org.xfce.ScreenSaver", "/org/xfce/ScreenSaver")
                    saver = dbus.Interface(proxy, "org.xfce.ScreenSaver")
                    if saver.GetActive():
                        raise RuntimeError("XFCE screensaver is active; resume the test desktop before validation")
            except BaseException:
                if self.session_bus:
                    self.session_bus.close()
                self.lib.XCloseDisplay(self.display)
                raise
        # This inhibition ends with this X connection, including abnormal exit.
        self.screensaver.XScreenSaverSuspend(self.display, 1)
        self.lib.XResetScreenSaver(self.display)
        self.lib.XSync(self.display, 0)
        self.background = 0
        self.presses = 0
        self.original_pointer = self.pointer()

    def pointer(self):
        root, child, rx, ry, x, y, mask = U(), U(), I(), I(), I(), I(), C.c_uint()
        assert self.lib.XQueryPointer(self.display, self.root, C.byref(root), C.byref(child), C.byref(rx), C.byref(ry), C.byref(x), C.byref(y), C.byref(mask))
        return rx.value, ry.value

    def focus(self):
        focus, revert = U(), I()
        self.lib.XGetInputFocus(self.display, C.byref(focus), C.byref(revert))
        return focus.value

    def drain(self):
        self.lib.XSync(self.display, 0)
        while self.lib.XPending(self.display):
            event = Event()
            self.lib.XNextEvent(self.display, C.byref(event))
            if event.button.type == 4 and event.button.window == self.background:
                self.presses += 1
        return self.presses

    def create_background(self):
        _, _, width, height, _ = self.geometry(self.root)
        self.background = self.lib.XCreateSimpleWindow(self.display, self.root, 0, 0, width, height, 0, 0, 0xffffff)
        self.lib.XStoreName(self.display, self.background, b"DesktopPet X11 validation background")
        self.lib.XSelectInput(self.display, self.background, (1 << 2) | (1 << 3))
        self.lib.XMapRaised(self.display, self.background)
        self.lib.XSync(self.display, 0)
        time.sleep(.4)

    def move(self, x, y):
        self.test.XTestFakeMotionEvent(self.display, -1, round(x), round(y), 0)
        self.lib.XSync(self.display, 0)
        time.sleep(.15)

    def button(self, down):
        self.test.XTestFakeButtonEvent(self.display, 1, int(down), 0)
        self.lib.XSync(self.display, 0)
        time.sleep(.15)

    def click(self):
        self.button(True)
        self.button(False)

    def find_pet(self, parent=None):
        parent = self.root if parent is None else parent
        root, ancestor, children, count = U(), U(), C.POINTER(U)(), C.c_uint()
        assert self.lib.XQueryTree(self.display, parent, C.byref(root), C.byref(ancestor), C.byref(children), C.byref(count))
        ids = list(children[:count.value]) if children else []
        if children:
            self.lib.XFree(children)
        for window in ids:
            name = P()
            self.lib.XFetchName(self.display, window, C.byref(name))
            title = C.string_at(name) if name else b""
            if name:
                self.lib.XFree(name)
            if title == b"DesktopPet":
                return window
            found = self.find_pet(window)
            if found:
                return found
        return None

    def geometry(self, window):
        root, x, y, w, h, border, depth = U(), I(), I(), C.c_uint(), C.c_uint(), C.c_uint(), C.c_uint()
        assert self.lib.XGetGeometry(self.display, window, C.byref(root), C.byref(x), C.byref(y), C.byref(w), C.byref(h), C.byref(border), C.byref(depth))
        child = U()
        self.lib.XTranslateCoordinates(self.display, window, self.root, 0, 0, C.byref(x), C.byref(y), C.byref(child))
        return x.value, y.value, w.value, h.value, depth.value

    def image(self, window, x, y, w, h):
        image = self.lib.XGetImage(self.display, window, x, y, w, h, U(-1).value, 2)
        assert image, "X11 image unavailable"
        return image

    def pixel(self, window, x, y):
        image = self.image(window, x, y, 1, 1)
        value = self.lib.XGetPixel(image, 0, 0)
        self.lib.XDestroyImage(image)
        return value

    def input_diagnostic(self, window):
        extension = C.CDLL("libXext.so.6")
        extension.XShapeGetRectangles.restype = C.POINTER(Rectangle)
        extension.XShapeGetRectangles.argtypes = [P,U,I,C.POINTER(I),C.POINTER(I)]
        details = []
        while window != self.root:
            count, ordering = I(), I()
            rectangles = extension.XShapeGetRectangles(self.display, window, 2, C.byref(count), C.byref(ordering))
            details.append(dict(window=window, input_rectangles=[[r.x,r.y,r.width,r.height] for r in rectangles[:count.value]]))
            if rectangles:
                self.lib.XFree(rectangles)
            root, parent, children, count = U(), U(), C.POINTER(U)(), C.c_uint()
            self.lib.XQueryTree(self.display, window, C.byref(root), C.byref(parent), C.byref(children), C.byref(count))
            if children:
                self.lib.XFree(children)
            window = parent.value
        return details

    def capture(self, window, directory):
        x, y, w, h, depth = self.geometry(window)
        assert depth == 32
        raw = self.image(window, 0, 0, w, h)
        samples = [(px, py, self.lib.XGetPixel(raw, px, py)) for py in range(0, h, 4) for px in range(0, w, 4)]
        alphas = [value >> 24 & 255 for _, _, value in samples]
        self.lib.XDestroyImage(raw)
        assert alphas.count(0) > 100 and sum(a > 0 for a in alphas) > 100, "missing model or transparent pixels"
        assert sum(0 < a < 255 for a in alphas) > 0, "no partially transparent model edges"
        for label, color in [("light", 0xffffff), ("dark", 0x202030)]:
            self.lib.XSetWindowBackground(self.display, self.background, color)
            self.lib.XClearWindow(self.display, self.background)
            self.lib.XSync(self.display, 0)
            time.sleep(.3)
            assert self.pixel(window, 5, 5) >> 24 == 0, "corner is not transparent"
            observed = self.pixel(self.root, x + 5, y + 5) & 0xffffff
            assert observed == color, ("compositor did not reveal background", "pet", self.geometry(window), "background", self.geometry(self.background), "expected", hex(color), "actual", hex(observed))
            image = self.image(self.root, x, y, w, h)
            pixels = bytearray()
            colored_pixels = 0
            for py in range(h):
                for px in range(w):
                    value = self.lib.XGetPixel(image, px, py)
                    colored_pixels += (value & 0xffffff) != color
                    pixels.extend(((value >> 16) & 255, (value >> 8) & 255, value & 255))
            self.lib.XDestroyImage(image)
            (directory / f"avatar-{label}.ppm").write_bytes(f"P6\n{w} {h}\n255\n".encode() + pixels)
            assert colored_pixels > 100, ("model not actually visible above background", (x,y,w,h), colored_pixels)
        return dict(zero_alpha=alphas.count(0), visible_alpha=sum(a > 0 for a in alphas), partial_alpha=sum(0 < a < 255 for a in alphas))

    def close(self):
        # Release even when a check failed while a test drag was held.
        self.test.XTestFakeButtonEvent(self.display, 1, 0, 0)
        if self.background:
            self.lib.XDestroyWindow(self.display, self.background)
        self.test.XTestFakeMotionEvent(self.display, -1, *self.original_pointer, 0)
        self.lib.XSync(self.display, 0)
        self.lib.XCloseDisplay(self.display)
        if self.session_bus:
            self.session_bus.close()


class Host:
    def __init__(self, host, model, output, name):
        self.log = (output / f"{name}.log").open("w")
        self.process = subprocess.Popen([str(host), str(model)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log, text=True,
                                        env=dict(os.environ, DESKTOPPET_LAYOUT=str(output / "layout.json")))
        self.sequence = 0
        try:
            assert self.exchange("hello")["type"] == "ready"
        except BaseException:
            self.process.kill()
            self.process.wait()
            self.log.close()
            raise

    def exchange(self, kind, payload=None):
        self.sequence += 1
        row = dict(protocol_version=VERSION, session_id="linux-x11", sequence=self.sequence, request_id=f"r{self.sequence}", type=kind, payload=payload or {})
        self.process.stdin.write(json.dumps(row) + "\n")
        self.process.stdin.flush()
        assert select.select([self.process.stdout], [], [], 7)[0], "host response timeout"
        line = self.process.stdout.readline()
        assert line, "host exited; see runtime log"
        reply = json.loads(line)
        assert reply["sequence"] == self.sequence and reply["request_id"] == row["request_id"]
        return reply

    def command(self, kind, payload):
        reply = self.exchange("desktop", dict(type=kind, payload=payload))
        assert reply["payload"]["accepted"], reply

    def close(self):
        if self.process.poll() is None:
            try:
                self.exchange("shutdown")
                assert self.process.wait(timeout=5) == 0
            finally:
                if self.process.poll() is None:
                    self.process.kill()
        self.process.wait()
        self.log.close()


def main():
    host_path, model, output = map(lambda p: Path(p).resolve(), sys.argv[1:])
    output.mkdir(parents=True, exist_ok=True)
    # A missing monitor deliberately exercises fallback; normalized coordinates
    # keep this baseline independent of the desktop's current resolution.
    (output / "layout.json").write_text(json.dumps(dict(version=1, monitor="absent-test-monitor", x=.5, y=.2, floor=False)))
    native = X11()
    host = None
    try:
        native.create_background()
        focus = native.focus()
        host = Host(host_path, model, output, "visible")
        host.command("set_scale", 50)
        host.command("set_visible", True)
        time.sleep(.5)
        window = native.find_pet()
        assert window, "pet window missing"
        assert native.focus() == focus, "show stole keyboard focus"
        alpha = native.capture(window, output)
        x, y, w, h, _ = native.geometry(window)
        native.move(x + 5, y + 5)
        before = native.drain()
        native.click()
        assert native.drain() == before + 1, ("transparent area swallowed background click", native.input_diagnostic(window))
        native.move(x + w * .5, y + h * .27)
        before = native.drain()
        native.click()
        assert native.drain() == before, ("character click reached background", native.input_diagnostic(window))
        events = host.exchange("poll")["payload"]["events"]
        assert any(e["type"] == "hit" for e in events), ("no semantic hit", events)
        native.move(x + w * .5, y + h * .27)
        native.button(True)
        native.move(x + w * .5 + 70, y + h * .27 + 40)
        native.button(False)
        time.sleep(.3)
        after_drag = native.geometry(window)
        assert abs(after_drag[0] - x - 70) <= 3 and abs(after_drag[1] - y - 40) <= 3, ("drag failed", (x,y), after_drag)
        assert host.exchange("poll")["payload"]["events"] == [], "drag emitted click"
        native.move(after_drag[0] + 5, after_drag[1] + 5)
        before = native.drain()
        native.button(True)
        native.move(after_drag[0] + w * .5, after_drag[1] + h * .27)
        assert native.geometry(window)[:2] == after_drag[:2], "background drag captured by pet"
        native.button(False)
        assert native.drain() == before + 1
        assert host.exchange("poll")["payload"]["events"] == []
        saved = json.loads((output / "layout.json").read_text())
        assert saved["monitor"] != "absent-test-monitor", saved
        host.close()
        host = Host(host_path, model, output, "restored")
        host.command("set_scale", 50)
        host.command("set_visible", True)
        time.sleep(.5)
        restored_window = native.find_pet()
        restored = native.geometry(restored_window)
        assert all(abs(a-b) <= 3 for a,b in zip(restored[:2], after_drag[:2])), ("restore failed", restored, after_drag)
        anchor = json.loads(model.read_text())["interaction"]["anchor"][1]
        floor = native.geometry(native.root)[3]
        target_y = floor - h * anchor - 5
        native.move(restored[0] + w * .5, restored[1] + h * .27)
        native.button(True)
        native.move(restored[0] + w * .5, target_y + h * .27)
        native.button(False)
        time.sleep(.3)
        snapped = native.geometry(restored_window)
        assert abs(snapped[1] + h * anchor - floor) <= 2, ("floor snap failed", snapped, anchor, floor)
        floor_saved = json.loads((output / "layout.json").read_text())
        assert floor_saved["floor"], ("floor placement not saved", floor_saved)
        host.command("set_visible", False)
        host.command("set_visible", True)
        host.close()
        host = None
        result = dict(passed=True, scope="single-monitor native X11; real model/alpha/focus/passthrough/click/owned drag/external drag/save/restore/floor snap/hide/shutdown", alpha=alpha, semantic_events=events, after_drag=after_drag, restored=restored, saved=saved, snapped=snapped, floor_saved=floor_saved)
        (output / "results.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
        print(json.dumps(result, ensure_ascii=False))
    finally:
        try:
            if host:
                host.close()
        finally:
            native.close()


if __name__ == "__main__":
    main()
