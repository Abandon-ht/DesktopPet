#!/usr/bin/env python3
"""Package a local input experiment; resources stay in ignored artifacts/."""
from pathlib import Path
import plistlib
import shutil
import sys

root = Path(__file__).resolve().parents[2]
model = Path(sys.argv[1]).resolve(strict=True)
app = root / "artifacts/local/p0/Input Probe.app"
contents = app / "Contents"
(contents / "MacOS").mkdir(parents=True, exist_ok=True)
(contents / "Resources").mkdir(exist_ok=True)
shutil.copy2(root / "target/release/p0-probe", contents / "MacOS/p0-probe")
(contents / "Resources/model-path.txt").write_text(str(model))
with (contents / "Info.plist").open("wb") as stream:
    plistlib.dump({
        "CFBundleIdentifier": "dev.desktoppet.input-probe",
        "CFBundleName": "Input Probe",
        "CFBundleExecutable": "p0-probe",
        "CFBundlePackageType": "APPL",
        "CFBundleVersion": "1",
        "NSHighResolutionCapable": True,
        "LSUIElement": True,
        "LSEnvironment": {"P0_INPUT": "dynamic"},
    }, stream)
print(app)
