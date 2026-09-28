#!/usr/bin/env python3
"""Render a self-contained SVG for reviewing a local avatar pack's hit polygons.

Usage: render-hit-overlay.py MANIFEST CAPTURE_PNG OUTPUT_SVG
The capture is embedded only in the requested output. Keep private outputs in
artifacts/local/, which is ignored by Git.
"""

import base64
import html
import json
from pathlib import Path
import sys

if len(sys.argv) != 4:
    raise SystemExit('usage: render-hit-overlay.py MANIFEST CAPTURE_PNG OUTPUT_SVG')

manifest = json.loads(Path(sys.argv[1]).read_text())
capture = Path(sys.argv[2]).resolve(strict=True)
output = Path(sys.argv[3]).resolve()
if capture.suffix.lower() != '.png' or output.suffix.lower() != '.svg':
    raise SystemExit('expected a PNG capture and SVG output')

interaction = manifest['interaction']
regions = interaction.get('regions', {})
region_parts = interaction.get('region_parts', {})
names = {
    'face': '脸', 'left_hand': '左手', 'right_hand': '右手',
    'left_arm': '左臂', 'right_arm': '右臂', 'abdomen': '腹部',
    'left_leg': '左腿', 'right_leg': '右腿',
    'left_foot': '左脚', 'right_foot': '右脚',
    'upper_body': '上身剩余', 'lower_body': '下身剩余',
}
colors = {
    'face': '#ffcf33', 'left_hand': '#4edbd2', 'right_hand': '#4edbd2',
    'left_arm': '#22add1', 'right_arm': '#22add1', 'abdomen': '#85d460',
    'left_leg': '#a688ff', 'right_leg': '#a688ff',
    'left_foot': '#e595ff', 'right_foot': '#e595ff',
    'upper_body': '#ff7575', 'lower_body': '#ff7575',
}
order = [
    'upper_body', 'lower_body', 'abdomen', 'left_leg', 'right_leg',
    'left_arm', 'right_arm', 'left_hand', 'right_hand',
    'left_foot', 'right_foot', 'face',
]

def points(polygon):
    return ' '.join(f'{x*1000:.1f},{y*1200:.1f}' for x, y in polygon)

elements = [
    '<svg xmlns="http://www.w3.org/2000/svg" width="1440" height="1200" viewBox="0 0 1440 1200">',
    '<rect width="1440" height="1200" fill="#15211d"/>',
    f'<image x="0" y="0" width="1000" height="1200" href="data:image/png;base64,{base64.b64encode(capture.read_bytes()).decode()}"/>',
]
for key in ('head', 'body'):
    elements.append(
        f'<polygon points="{points(interaction[key])}" fill="none" '
        'stroke="#ffffff" stroke-width="3" stroke-dasharray="10 7" opacity="0.9"/>'
    )
for index, key in enumerate(order, start=1):
    polygon = regions.get(key)
    if not polygon:
        continue
    color = colors[key]
    for part in region_parts.get(key, []):
        elements.append(
            f'<polygon points="{points(part)}" fill="{color}" fill-opacity="0.22" '
            f'stroke="{color}" stroke-width="3"/>'
        )
    x = sum(point[0] for point in polygon) / len(polygon) * 1000
    y = sum(point[1] for point in polygon) / len(polygon) * 1200
    elements.append(
        f'<polygon points="{points(polygon)}" fill="{color}" fill-opacity="0.22" '
        f'stroke="{color}" stroke-width="3"/>'
    )
    elements.append(
        f'<text x="{x:.1f}" y="{y:.1f}" text-anchor="middle" dominant-baseline="middle" '
        f'font-family="sans-serif" font-size="24" font-weight="700" fill="#ffffff">'
        f'{index}</text>'
    )

elements.extend([
    '<rect x="1000" width="440" height="1200" fill="#15211d"/>',
    '<text x="1030" y="55" font-family="sans-serif" font-size="27" font-weight="700" fill="#f4f8ef">区域静态初标</text>',
    '<text x="1030" y="88" font-family="sans-serif" font-size="16" fill="#b8c9bd">白色虚线：旧头部／身体回退区</text>',
    '<text x="1030" y="115" font-family="sans-serif" font-size="16" fill="#b8c9bd">彩色区域：需用户桌面点击校准</text>',
])
for index, key in enumerate(order, start=1):
    if key not in regions:
        continue
    y = 160 + (index - 1) * 48
    label = html.escape(names[key])
    elements.append(f'<circle cx="1045" cy="{y}" r="12" fill="{colors[key]}"/>')
    elements.append(
        f'<text x="1070" y="{y+6}" font-family="sans-serif" font-size="21" fill="#f4f8ef">'
        f'{index:02d}  {label}</text>'
    )
elements.extend([
    '<text x="1030" y="795" font-family="sans-serif" font-size="17" fill="#f4f8ef">检查顺序</text>',
    '<text x="1030" y="832" font-family="sans-serif" font-size="15" fill="#b8c9bd">1. 区域是否覆盖正确部位</text>',
    '<text x="1030" y="861" font-family="sans-serif" font-size="15" fill="#b8c9bd">2. 手臂／手、腿／脚边界</text>',
    '<text x="1030" y="890" font-family="sans-serif" font-size="15" fill="#b8c9bd">3. 周围透明区不应命中</text>',
    '<text x="1030" y="1050" font-family="sans-serif" font-size="15" fill="#ffc4a9">静态截图仅作初标，尚非桌面验收。</text>',
    '</svg>',
])
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text('\n'.join(elements))
print(output)
