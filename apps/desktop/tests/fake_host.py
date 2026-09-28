#!/usr/bin/env python3
"""Headless fixture for app supervision tests; not a distributable character."""
import json
import pathlib
import sys
source = pathlib.Path(sys.argv[1]).read_text().strip()
try:
    manifest = json.loads(source)
except json.JSONDecodeError:
    manifest = None
mode = 'touch_once' if manifest and manifest.get('touch_reactions') else source
if mode == 'reject':
    sys.exit(12)
hit_sent = False
for line in sys.stdin:
    message = json.loads(line)
    kind = message['type']
    if mode == 'crash' and kind in ('ping', 'poll'):
        sys.exit(17)
    message['type'] = {'hello': 'ready', 'ping': 'pong', 'desktop': 'result', 'shutdown': 'stopped', 'avatar': 'result', 'poll': 'events'}[kind]
    denied = mode == 'deny_external' and kind == 'desktop' and message['payload'] == {'type': 'set_external_snap_enabled', 'payload': True}
    if kind == 'hello':
        message['payload'] = {'capabilities': ['ping', 'shutdown', 'desktop'], 'max_frame_bytes': 256 * 1024}
        if mode == 'touch_once':
            message['payload']['avatar'] = dict.fromkeys(
                ('head_pat', 'body_tap', 'feed', 'play', 'rest', 'greet', 'peek', 'invite', 'celebrate', 'baseline'), False)
            message['payload']['avatar']['touch_reactions'] = True
    elif kind == 'poll':
        events = []
        if mode == 'touch_once' and not hit_sent:
            events.append({'type': 'hit', 'payload': {'region': 'left_arm', 'point': [400, 426], 'event_id': 1}})
            hit_sent = True
        message['payload'] = {'events': events}
    else:
        message['payload'] = {'accepted': not denied}
    print(json.dumps(message), flush=True)
    if kind == 'shutdown':
        break
