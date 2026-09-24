#!/usr/bin/env python3
"""Real native host lifecycle checks. Never sends a show command.
Usage: python3 tools/p1/verify-hidden-host.py HOST MODEL3_JSON
"""
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import time

host, model = map(lambda p: str(Path(p).resolve()), sys.argv[1:])
root = Path(__file__).resolve().parents[2]
out = root / 'artifacts/local/p1'
out.mkdir(parents=True, exist_ok=True)
results = []

def start(name):
    log = (out / f'{name}.log').open('w')
    process = subprocess.Popen([host, model], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=log, text=True)
    return process, log

def exchange(process, sequence, kind, payload=None, request_id=None):
    row = dict(protocol_version=1, session_id='hidden-check', sequence=sequence,
               request_id=request_id or f'r{sequence}', type=kind, payload=payload or {})
    process.stdin.write(json.dumps(row) + '\n')
    process.stdin.flush()
    assert select.select([process.stdout], [], [], 7)[0], 'reply timeout'
    response = json.loads(process.stdout.readline())
    for key in ('protocol_version', 'session_id', 'sequence', 'request_id'):
        assert response[key] == row[key], response
    return response

def case(name, check):
    process, log = start(name)
    try:
        ready = exchange(process, 1, 'hello')
        assert ready['type'] == 'ready'
        if Path(model).name == 'manifest.json':
            actions = json.loads(Path(model).read_text())['actions']
            assert ready['payload']['avatar'] == {name: actions.get(name) is not None for name in ('head_pat', 'body_tap')}
        check(process)
        results.append(dict(case=name, passed=True))
    finally:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=5)
        log.close()

def normal(p):
    command = dict(type='set_visible', payload=False)
    assert exchange(p, 2, 'desktop', command, 'hide')['payload']['accepted']
    assert exchange(p, 3, 'desktop', command, 'hide')['payload']['accepted']
    assert isinstance(exchange(p, 4, 'desktop', dict(type='set_external_snap_enabled', payload=True))['payload']['accepted'], bool)
    assert exchange(p, 5, 'ping')['type'] == 'pong'
    assert exchange(p, 6, 'desktop', dict(type='set_scale', payload=50))['payload']['accepted']
    assert exchange(p, 7, 'desktop', dict(type='set_scale', payload=150))['payload']['accepted']
    assert exchange(p, 8, 'desktop', dict(type='set_scale', payload=10))['payload']['accepted'] is False
    assert exchange(p, 9, 'poll')['payload']['events'] == []
    assert exchange(p, 10, 'avatar', dict(type='cancel_feedback'))['payload']['accepted']
    assert exchange(p, 11, 'shutdown')['type'] == 'stopped'
    assert p.wait(timeout=5) == 0

def eof(p):
    p.stdin.close()
    assert p.wait(timeout=5) == 0

def lease(p):
    # Pipe remains open, simulating a parent that no longer sends heartbeats.
    assert p.wait(timeout=11) != 0

def wrong_version(p):
    p.stdin.write(json.dumps(dict(protocol_version=2, session_id='hidden-check', sequence=2,
                                 request_id='bad', type='ping', payload={})) + '\n')
    p.stdin.flush()
    assert p.wait(timeout=5) != 0

case('normal-hide-toggle-shutdown', normal)
case('pipe-eof', eof)
case('heartbeat-loss', lease)
case('wrong-version', wrong_version)
# Kill a real parent process, leaving the native child to detect its pipe EOF.
parent_code = '''
import subprocess, sys, json, time
p=subprocess.Popen(sys.argv[1:3],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
p.stdin.write(json.dumps(dict(protocol_version=1,session_id='parent-death',sequence=1,request_id='hello',type='hello',payload={}))+'\\n');p.stdin.flush()
assert json.loads(p.stdout.readline())['type']=='ready'
print(p.pid,flush=True)
time.sleep(60)
'''
with (out / 'parent-death.log').open('w') as log:
    parent = subprocess.Popen([sys.executable, '-c', parent_code, host, model], stdout=subprocess.PIPE, stderr=log, text=True)
    child_pid = None
    try:
        assert select.select([parent.stdout], [], [], 8)[0], 'parent startup timeout'
        child_pid = int(parent.stdout.readline())
        parent.kill(); parent.wait(timeout=3)
        deadline = time.monotonic() + 6
        while time.monotonic() < deadline:
            state = subprocess.run(['ps','-p',str(child_pid),'-o','stat='],capture_output=True,text=True).stdout.strip()
            if not state or state.startswith('Z'):
                break
            time.sleep(.1)
        else:
            raise AssertionError('orphan native host')
        results.append(dict(case='parent-killed',passed=True))
    finally:
        if parent.poll() is None: parent.kill(); parent.wait()
        if child_pid:
            try: os.kill(child_pid,signal.SIGKILL)
            except ProcessLookupError: pass
report = dict(scope='native host, hidden windows only; no tray or visible interaction acceptance',results=results)
(out / 'hidden-host-results.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(report,ensure_ascii=False))
