#!/usr/bin/env python3
"""Protocol fixture only. No X11, renderer, model loading, or desktop input."""
import json
import os
from pathlib import Path
import sys
import time

mode = os.environ.get("AX650_TEST_MODE", "normal")
record = Path(os.environ["AX650_TEST_RECORD"])
print("AX650 fake host initialized", file=sys.stderr, flush=True)

def save(value):
    with record.open("a") as output:
        output.write(json.dumps(value) + "\n")

save({"started": os.getpid(), "sid": os.getsid(0), "pgrp": os.getpgrp(),
      "environment": {key: os.environ.get(key) for key in (
          "WGPU_BACKEND", "LIBGL_ALWAYS_SOFTWARE", "DESKTOPPET_X11_GL_ALPHA",
          "DESKTOPPET_X11_OPAQUE_ALPHA", "DESKTOPPET_MAX_FPS", "DESKTOPPET_LAYOUT",
          "DESKTOPPET_METRICS", "DISPLAY", "XAUTHORITY")}})
polled = False
for line in sys.stdin:
    request = json.loads(line)
    save({"request": request})
    kind = request["type"]
    if mode == "crash_poll" and kind == "poll":
        sys.exit(17)
    if (mode == "crash_feedback" and kind == "avatar"
            and request["payload"]["type"] == "play_feedback"):
        sys.exit(18)
    response = dict(request)
    response["type"] = {"hello": "ready", "ping": "pong", "poll": "events",
                        "desktop": "result", "avatar": "result", "shutdown": "stopped"}[kind]
    if kind == "hello":
        response["payload"] = {"capabilities": ["ping", "shutdown", "desktop", "avatar", "poll"],
                               "max_frame_bytes": 256 * 1024,
                               "avatar": {"head_pat": True, "body_tap": True, "feed": True,
                                          "play": True, "rest": True, "greet": False,
                                          "peek": False, "invite": False, "baseline": True,
                                          "touch_reactions": True}}
    elif kind == "poll":
        event = {"type": "hit", "payload": {"region": "upper_body", "point": [500, 500], "event_id": 1}}
        response["payload"] = {"events": [event, event] if mode == "duplicate_touch" and not polled else []}
        polled = True
    elif kind in ("desktop", "avatar"):
        response["payload"] = {"accepted": True}
    else:
        response["payload"] = {}
    if mode == "wrong_version":
        response["protocol_version"] = 1
    print(json.dumps(response), flush=True)
    if kind == "shutdown":
        if mode == "stall_exit":
            time.sleep(30)
        break

