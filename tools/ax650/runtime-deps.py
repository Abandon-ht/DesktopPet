#!/usr/bin/env python3
"""Check the selected renderer's dynamic libraries without opening an X display."""
import argparse
import ctypes
import json
import sys

X11_LIBRARIES = (
    "libX11.so.6", "libX11-xcb.so.1", "libXcursor.so.1", "libXi.so.6",
    "libXrandr.so.2", "libXrender.so.1", "libXfixes.so.3", "libxcb.so.1",
    "libxcb-xkb.so.1", "libxkbcommon.so.0", "libxkbcommon-x11.so.0",
)
BACKEND_LIBRARIES = {
    "gl": ("libEGL.so.1", "libGL.so.1", "libGLESv2.so.2"),
    "vulkan": ("libvulkan.so.1",),
}
XKB_SYMBOLS = (
    "xkb_x11_setup_xkb_extension", "xkb_x11_get_core_keyboard_device_id",
    "xkb_x11_keymap_new_from_device", "xkb_x11_state_new_from_device",
)


def check(backend, loader=ctypes.CDLL):
    """Keep handles alive through the check, including XKB symbols loaded by winit."""
    loaded, failures, handles = [], [], []
    for name in X11_LIBRARIES + BACKEND_LIBRARIES[backend]:
        try:
            library = loader(name)
            handles.append(library)
            if name == "libxkbcommon-x11.so.0":
                for symbol in XKB_SYMBOLS:
                    getattr(library, symbol)
            loaded.append(name)
        except (OSError, AttributeError) as error:
            failures.append({"library": name, "error": str(error)})
    return {
        "backend": backend, "loaded": loaded, "failures": failures,
        "scope": "Library loading and XKB symbols only; no display, device or rendering validation",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", choices=BACKEND_LIBRARIES, default="gl")
    args = parser.parse_args()
    result = check(args.backend)
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return bool(result["failures"])


if __name__ == "__main__":
    sys.exit(main())
