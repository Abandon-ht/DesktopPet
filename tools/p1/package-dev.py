#!/usr/bin/env python3
"""Local unsigned development bundle. Copies executables, never character assets.
Usage: package-dev.py MODEL_PATH [OUTPUT_APP]
"""
from pathlib import Path
import os
import plistlib
import re
import shutil
import subprocess
import sys
if len(sys.argv) not in (2, 3):
    raise SystemExit('usage: package-dev.py MODEL_PATH [OUTPUT_APP]')
root = Path(__file__).resolve().parents[2]
model = Path(sys.argv[1]).resolve(strict=True)
if not model.is_file():
    raise SystemExit('model must be a file')
app = Path(sys.argv[2]).resolve() if len(sys.argv) == 3 else root / 'artifacts/local/p1/DesktopPet Dev.app'
if app.suffix != '.app':
    raise SystemExit('output must be an .app directory')
slug = re.sub(r'[^a-z0-9]+', '-', app.stem.lower()).strip('-')
bundle_id = 'dev.desktoppet.alpha' if app.stem == 'DesktopPet Dev' else f'dev.desktoppet.alpha.{slug}'
contents = app / 'Contents'
(contents / 'MacOS').mkdir(parents=True, exist_ok=True)
(contents / 'Resources').mkdir(exist_ok=True)
shutil.copy2(root / 'target/release/desktop-pet', contents / 'MacOS/desktop-pet')
(contents / 'MacOS/avatar-host-2d').unlink(missing_ok=True)
helper = contents / 'Helpers/DesktopPet Avatar Host.app/Contents'
(helper / 'MacOS').mkdir(parents=True, exist_ok=True)
shutil.copy2(root / 'target/release/avatar-host-2d', helper / 'MacOS/avatar-host-2d')
with (helper / 'Info.plist').open('wb') as stream:
    plistlib.dump(dict(CFBundleIdentifier=f'{bundle_id}.avatar-host',
                      CFBundleName=f'{app.stem} Host', CFBundleDisplayName=f'{app.stem} Host',
                      CFBundleExecutable='avatar-host-2d', CFBundlePackageType='APPL',
                      CFBundleVersion='1', LSUIElement=True), stream)
(contents / 'Resources/model-path.txt').write_text(str(model) + '\n')
show_settings = contents / 'Resources/show-settings-on-launch'
if os.environ.get('DESKTOPPET_DEV_SHOW_SETTINGS') == '1':
    show_settings.touch()
else:
    show_settings.unlink(missing_ok=True)
with (contents / 'Info.plist').open('wb') as stream:
    plistlib.dump(dict(CFBundleIdentifier=bundle_id, CFBundleName=app.stem,
                      CFBundleDisplayName=app.stem,
                      CFBundleExecutable='desktop-pet', CFBundlePackageType='APPL',
                      CFBundleVersion='1', NSHighResolutionCapable=True, LSUIElement=True), stream)
subprocess.run(['codesign', '--force', '--sign', '-', str(helper.parent)], check=True)
subprocess.run(['codesign', '--force', '--sign', '-', str(app)], check=True)
print(app)
