#!/usr/bin/env python3
"""Generate an unfilled 100-interaction manual checklist; does not drive the mouse."""
import csv
from pathlib import Path
root = Path(__file__).resolve().parents[2]
path = root / 'artifacts/local/p1/p1-04-interactions.csv'
if path.exists():
    raise SystemExit(f'Existing results preserved: {path}')
path.parent.mkdir(parents=True, exist_ok=True)
actions = ['短按头部，表情结束恢复', '短按身体，表情结束恢复', '普通拖拽，松手不触发表情',
           '透明区域点击下面应用', '快速移入角色再拖拽']
with path.open('w', newline='') as stream:
    writer = csv.writer(stream)
    writer.writerow(['编号', '大小', '操作', '结果', '备注'])
    for index in range(100):
        writer.writerow([index + 1, [50, 100, 150, 100][index // 25], actions[index % 5], '待测', ''])
print(path)
