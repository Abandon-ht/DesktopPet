#!/usr/bin/env python3
"""Headless fixture for app supervision tests; not a distributable character."""
import json
import pathlib
import sys
mode = pathlib.Path(sys.argv[1]).read_text().strip()
if mode == 'reject':
    sys.exit(12)
for line in sys.stdin:
    message = json.loads(line)
    kind = message['type']
    if mode == 'crash' and kind in ('ping', 'poll'):
        sys.exit(17)
    message['type'] = {'hello': 'ready', 'ping': 'pong', 'desktop': 'result', 'shutdown': 'stopped', 'avatar': 'result', 'poll': 'events'}[kind]
    denied = mode == 'deny_external' and kind == 'desktop' and message['payload'] == {'type': 'set_external_snap_enabled', 'payload': True}
    message['payload'] = ({'capabilities': ['ping', 'shutdown', 'desktop'], 'max_frame_bytes': 256 * 1024}
                          if kind == 'hello' else {'events': []} if kind == 'poll' else {'accepted': not denied})
    print(json.dumps(message), flush=True)
    if kind == 'shutdown':
        break
