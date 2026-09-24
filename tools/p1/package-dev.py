#!/usr/bin/env python3
"""Local unsigned development bundle. Copies executables, never character assets.
Usage: package-dev.py MODEL_PATH [OUTPUT_APP]
"""
from pathlib import Path
import plistlib
import shutil
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
contents = app / 'Contents'
(contents / 'MacOS').mkdir(parents=True, exist_ok=True)
(contents / 'Resources').mkdir(exist_ok=True)
for name in ('desktop-pet', 'avatar-host-2d'):
    shutil.copy2(root / 'target/release' / name, contents / 'MacOS' / name)
(contents / 'Resources/model-path.txt').write_text(str(model) + '\n')
with (contents / 'Info.plist').open('wb') as stream:
    plistlib.dump(dict(CFBundleIdentifier='dev.desktoppet.alpha', CFBundleName='DesktopPet Dev',
                      CFBundleExecutable='desktop-pet', CFBundlePackageType='APPL',
                      CFBundleVersion='1', NSHighResolutionCapable=True, LSUIElement=True), stream)
print(app)
