#!/usr/bin/env python3
"""Run a fixed local DesktopPet app and retain P1-06 evidence outside Git."""
import argparse
import csv
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_APP = ROOT / 'artifacts/local/p1/gaze-range/DesktopPet Gaze Range.app'
METRICS = Path.home() / 'Library/Application Support/dev.desktoppet.alpha/render-metrics.json'


def output(command):
    return subprocess.run(command, capture_output=True, text=True, check=False).stdout.strip()


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def sample(writer, role, pid):
    values = output(['ps', '-p', str(pid), '-o', '%cpu=,rss=']).split()
    if len(values) == 2:
        writer.writerow([datetime.now(timezone.utc).isoformat(), role, pid, *values])


def children(pid):
    for text in output(['pgrep', '-P', str(pid)]).split():
        child = int(text)
        if 'avatar-host-2d' in output(['ps', '-p', text, '-o', 'comm=']):
            yield child


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, default=DEFAULT_APP,
                        help='Existing local .app to test; default is the latest gaze-range package')
    parser.add_argument('--label', default='general',
                        help='Short label such as general or window-snap')
    args = parser.parse_args()
    app = args.app.resolve(strict=True)
    executable = app / 'Contents/MacOS/desktop-pet'
    host = app / 'Contents/Helpers/DesktopPet Avatar Host.app/Contents/MacOS/avatar-host-2d'
    if app.suffix != '.app' or not executable.is_file() or not host.is_file():
        parser.error('choose a packaged DesktopPet .app with the nested avatar host')
    if not args.label or any(c not in 'abcdefghijklmnopqrstuvwxyz0123456789-_' for c in args.label):
        parser.error('--label must contain only lowercase letters, digits, dash or underscore')
    now = datetime.now(timezone.utc)
    directory = ROOT / 'artifacts/local/p1/p1-06' / f'{now:%Y%m%dT%H%M%SZ}-{args.label}'
    directory.mkdir(parents=True, exist_ok=False)
    previous_metrics = METRICS.stat().st_mtime_ns if METRICS.exists() else None
    info = {
        'started_utc': now.isoformat(),
        'app': str(app),
        'git_head': output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD']),
        'desktop_sha256': sha256(executable),
        'host_sha256': sha256(host),
        'macos_version': output(['sw_vers', '-productVersion']),
        'sample_interval_seconds': 5,
    }
    (directory / 'run-info.json').write_text(json.dumps(info, ensure_ascii=False, indent=2) + '\n')
    print(f'P1-06 日志目录：{directory}', flush=True)
    print('请按测试清单操作，结束时用菜单栏 Pet → 退出；不要同时运行另一份 DesktopPet。', flush=True)
    with (directory / 'app-stderr.log').open('wb') as log, \
            (directory / 'resource-samples.csv').open('w', newline='') as samples:
        writer = csv.writer(samples)
        writer.writerow(['time_utc', 'process', 'pid', 'cpu_percent', 'rss_kb'])
        process = subprocess.Popen([str(executable)], cwd=ROOT,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=log)
        info['desktop_pid'] = process.pid
        try:
            while process.poll() is None:
                sample(writer, 'desktop', process.pid)
                for child in children(process.pid):
                    sample(writer, 'avatar-host', child)
                samples.flush()
                time.sleep(5)
        except KeyboardInterrupt:
            print('已中断采集，正在关闭测试应用…', flush=True)
            process.terminate()
        try:
            info['exit_code'] = process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            info['exit_code'] = process.wait()
        info['ended_utc'] = datetime.now(timezone.utc).isoformat()
    if METRICS.exists() and METRICS.stat().st_mtime_ns != previous_metrics:
        shutil.copy2(METRICS, directory / 'render-metrics.json')
    (directory / 'run-info.json').write_text(json.dumps(info, ensure_ascii=False, indent=2) + '\n')
    print(f'采集完成：{directory}', flush=True)
    return 0 if info['exit_code'] == 0 else 1


if __name__ == '__main__':
    sys.exit(main())
