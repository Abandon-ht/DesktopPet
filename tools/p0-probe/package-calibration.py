#!/usr/bin/env python3
"""Create a local unsigned development .app using a real executable (no shell wrapper)."""
from pathlib import Path
import plistlib
import shutil

root = Path(__file__).resolve().parents[2]
app = root / "artifacts/local/p0/Alpha Calibration.app"
contents = app / "Contents"
(contents / "MacOS").mkdir(parents=True, exist_ok=True)
shutil.copy2(root / "target/release/alpha-calibration", contents / "MacOS/alpha-calibration")
with (contents / "Info.plist").open("wb") as stream:
    plistlib.dump({
        "CFBundleIdentifier": "dev.desktoppet.alpha-calibration",
        "CFBundleName": "Alpha Calibration",
        "CFBundleDisplayName": "DesktopPet Alpha Calibration",
        "CFBundleExecutable": "alpha-calibration",
        "CFBundlePackageType": "APPL",
        "CFBundleVersion": "1",
        "CFBundleShortVersionString": "0.1.0",
        "NSHighResolutionCapable": True,
    }, stream)
print(app)
