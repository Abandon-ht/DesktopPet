#!/usr/bin/env python3
"""Hidden native window placement: missing monitor falls back to primary screen."""
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile
root = Path(__file__).resolve().parents[2]
host, model = map(lambda p: str(Path(p).resolve()), sys.argv[1:])
out = root / 'artifacts/local/p1'
out.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='desktop-pet-placement-') as directory:
    layout = Path(directory) / 'placement.json'
    layout.write_text(json.dumps(dict(version=1, monitor='absent-monitor', x=.5, y=.5, floor=False)))
    with (out / 'placement-hidden.log').open('w') as log:
        process = subprocess.Popen([host, model], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log,
                                   text=True, env=dict(os.environ, DESKTOPPET_LAYOUT=str(layout)))
        try:
            for sequence, kind, payload in [(1, 'hello', {}), (2, 'desktop', dict(type='set_visible', payload=False)),
                                             (3, 'desktop', dict(type='set_scale', payload=75)), (4, 'ping', {}), (5, 'shutdown', {})]:
                process.stdin.write(json.dumps(dict(protocol_version=1, session_id='placement', sequence=sequence,
                                                   request_id=f'r{sequence}', type=kind, payload=payload))+'\n')
                process.stdin.flush()
                assert select.select([process.stdout], [], [], 7)[0], 'response timeout'
                reply = json.loads(process.stdout.readline())
                assert reply['sequence'] == sequence
                if kind == 'desktop': assert reply['payload']['accepted']
            assert process.wait(timeout=5) == 0
        finally:
            if process.poll() is None: process.kill()
            process.wait()
    records=[]
    for line in (out/'placement-hidden.log').read_text().splitlines():
        try: record=json.loads(line)
        except json.JSONDecodeError: continue
        if record.get('event') == 'position_restored': records.append(record)
    assert records, 'no native placement applied'
    for record in records:
        assert record['monitor'] != 'absent-monitor'
        assert all(abs(a-b)<2 for a,b in zip(record['requested'],record['actual'])), record
    result=dict(passed=True, scope='hidden window native restore and absent-monitor fallback only; no physical unplug', records=records)
    (out/'placement-hidden-results.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps(result,ensure_ascii=False))
