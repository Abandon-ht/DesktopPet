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
expressions = ('Angry', 'Halfeyes', 'HandChange', 'Happy1', 'Sad1', 'Sad2',
               'Shy', 'StarEye', 'Wink', 'black', 'kusa', 'mouthchange', 'shy_normal')
for stem in expressions:
    name = stem + '.exp3.json'
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
    def stage(expression, duration_ms):
        return dict(expression=expression, duration_ms=duration_ms)
    catalog = {stem: dict(path=f'model/{stem}.exp3.json', label=stem) for stem in expressions}
    profile = dict(catalog=catalog,
                   baseline=dict(neutral='shy_normal',cheerful='Happy1',sad='Sad1',irritable='Angry',
                                 tired='Halfeyes',depleted='Sad2',starving='black',affectionate='Shy'),
                   reactions=dict(head_pat=[stage('Shy',1200)],
                                  feed=[stage('mouthchange',1100),stage('Happy1',1400)],
                                  play=[stage('StarEye',1800)],
                                  celebrate=[stage('StarEye',1800),stage('kusa',1200)],
                                  rest=[stage('Halfeyes',8000)],greet=[stage('HandChange',1800)],
                                  peek=[stage('Wink',1600)],invite=[stage('Shy',2000)]))
    touch_reactions = dict(
        head_wary=[stage('Shy', 1200)],
        head_warm=[stage('Shy', 1200)],
        head_close=[stage('Shy', 1200)],
        face_wary=[stage('shy_normal', 1200)],
        face_warm=[stage('Wink', 1200)],
        face_close=[stage('shy_normal', 1200)],
        hand_wary=[stage('Shy', 1200)],
        hand_warm=[stage('Happy1', 1200)],
        hand_close=[stage('Shy', 1200)],
        arm=[stage('HandChange', 1200)],
        uneasy=[stage('Halfeyes', 1100)],
        discomfort=[stage('Angry', 1200)],
        boundary_first=[stage('Angry', 1200)],
        boundary_second=[stage('Angry', 1200)],
        boundary_third=[stage('Sad2', 1600)],
    )
    manifest = dict(schema_version=4,id='nahida-p2-touch-local',display_name='纳西妲 · P2 部位反馈版',renderer='live2d_mocari',
        entry='model/'+source.name,
        interaction=dict(head=[[.38,.08],[.62,.08],[.72,.20],[.72,.36],[.62,.46],[.38,.46],[.28,.36],[.28,.20]],
                         body=[[.34,.43],[.66,.43],[.66,.9],[.34,.9]],
                         regions=dict(face=[[.40,.235],[.60,.235],[.60,.345],[.40,.345]],
                                      left_hand=[[.265,.52],[.345,.52],[.345,.585],[.265,.585]],
                                      right_hand=[[.625,.53],[.705,.53],[.705,.595],[.625,.595]],
                                      left_arm=[[.395,.345],[.455,.345],[.36,.535],[.305,.535]],
                                      right_arm=[[.545,.345],[.605,.345],[.695,.535],[.64,.535]],
                                      abdomen=[[.425,.425],[.575,.425],[.575,.575],[.425,.575]],
                                      left_leg=[[.42,.62],[.50,.62],[.50,.93],[.42,.93]],
                                      right_leg=[[.51,.62],[.59,.62],[.59,.93],[.51,.93]],
                                      left_foot=[[.42,.90],[.50,.90],[.50,.965],[.42,.965]],
                                      right_foot=[[.51,.90],[.59,.90],[.59,.965],[.51,.965]],
                                      upper_body=[[.405,.35],[.595,.35],[.595,.50],[.405,.50]],
                                      lower_body=[[.39,.51],[.61,.51],[.61,.64],[.39,.64]]),
                         region_parts=dict(lower_body=[
                             [[.38,.52],[.405,.52],[.405,.665],[.34,.665]],
                             [[.595,.52],[.62,.52],[.66,.665],[.595,.665]],
                         ]),
                         anchor=[.5,.9625],window_perch_y=.5),
        actions=dict(head_pat=dict(expression='model/Shy.exp3.json',duration_ms=1200),
                     body_tap=None,
                     feed=dict(expression='model/Happy1.exp3.json',duration_ms=1600),
                     play=dict(expression='model/StarEye.exp3.json',duration_ms=1800),
                     rest=dict(expression='model/Halfeyes.exp3.json',duration_ms=3000),
                     greet=dict(expression='model/HandChange.exp3.json',duration_ms=1800),
                     peek=dict(expression='model/Wink.exp3.json',duration_ms=1600),
                     invite=dict(expression='model/Shy.exp3.json',duration_ms=2000)),
        parameter_map=dict(mouth_open='ParamMouthOpenY',blink_left='ParamEyeLOpen',blink_right='ParamEyeROpen',gaze_x='ParamEyeBallX',gaze_y='ParamEyeBallY',head_x='ParamAngleX',head_y='ParamAngleY',body_sway='ParamBodyAngleZ'),
        expression_profile=profile,
        touch_reactions=touch_reactions,
        touch_variants=dict(face=['face_wary', 'face_warm'],
                            left_hand=['hand_wary', 'hand_warm'],
                            right_hand=['hand_wary', 'hand_warm']),
        license=dict(status='unverified',redistributable=False))
    (output/'manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
except Exception:
    shutil.rmtree(output)
    raise
print(output/'manifest.json')
