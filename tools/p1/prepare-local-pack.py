#!/usr/bin/env python3
"""Create a local Nahida development pack without changing source resources.
Usage: prepare-local-pack.py MODEL3_JSON OUTPUT_DIRECTORY
The expression mapping below is specific to the P0-validated Nahida asset.
"""
import json
from pathlib import Path
import shutil
import sys
source = Path(sys.argv[1]).resolve(strict=True)
output = Path(sys.argv[2]).resolve()
if output.exists() or output.is_relative_to(source.parent):
    raise SystemExit('output must be a new directory outside the source model directory')
for name in ('Happy1.exp3.json', 'Shy.exp3.json'):
    if not (source.parent / name).is_file():
        raise SystemExit(f'missing expected local expression: {name}')
output.mkdir(parents=True)
try:
    for path in source.parent.rglob('*'):
        if path.is_symlink():
            raise ValueError('symlink source is not supported')
        if path.is_file() and path.suffix in ('.json', '.moc3', '.png'):
            destination = output / 'model' / path.relative_to(source.parent)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, destination)
    manifest = dict(schema_version=1,id='nahida-local',display_name='纳西妲 · 本地角色',renderer='live2d_mocari',
        entry='model/'+source.name,
        interaction=dict(head=[[.38,.08],[.62,.08],[.72,.20],[.72,.36],[.62,.46],[.38,.46],[.28,.36],[.28,.20]],
                         body=[[.34,.43],[.66,.43],[.66,.9],[.34,.9]],anchor=[.5,.9625],window_perch_y=.5),
        actions=dict(head_pat=dict(expression='model/Happy1.exp3.json',duration_ms=1200),
                     body_tap=dict(expression='model/Shy.exp3.json',duration_ms=1200)),
        parameter_map=dict(mouth_open='ParamMouthOpenY',blink_left='ParamEyeLOpen',blink_right='ParamEyeROpen',gaze_x='ParamEyeBallX',gaze_y='ParamEyeBallY',head_x='ParamAngleX',head_y='ParamAngleY'),
        license=dict(status='unverified',redistributable=False))
    (output/'manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
except Exception:
    shutil.rmtree(output)
    raise
print(output/'manifest.json')
