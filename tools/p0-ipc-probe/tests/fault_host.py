"""Fault injection only; never installed as the application host."""
import json
import sys
import time

mode = sys.argv[1]
if mode == "stall_handshake":
    time.sleep(30)
for line in sys.stdin:
    message = json.loads(line)
    kind = message["type"]
    if kind == "ping" and mode == "crash_ping":
        sys.exit(17)
    if kind == "ping" and mode == "stall_ping":
        time.sleep(30)
    if mode == "oversized":
        print("x" * (256 * 1024 + 1), flush=True)
        continue
    message["type"] = {"hello": "ready", "ping": "pong", "shutdown": "stopped"}[kind]
    message["payload"] = {"capabilities": ["ping", "shutdown"], "max_frame_bytes": 256 * 1024}
    if mode == "wrong_version":
        message["protocol_version"] = 2
    if mode == "wrong_request":
        message["request_id"] = "unrelated"
    print(json.dumps(message), flush=True)
    if kind == "shutdown":
        if mode == "stall_exit":
            time.sleep(30)
        break
