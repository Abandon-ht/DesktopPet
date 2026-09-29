#!/usr/bin/env python3
"""Assemble an ad-hoc signed macOS app from Rust binaries and resource ZIPs."""
import argparse
import hashlib
import json
import plistlib
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import zipfile

ROOT = Path(__file__).resolve().parents[2]
REQUIRED = ("avatar.zip", "voice.zip", "models-vad.zip", "models-asr.zip", "models-tts.zip", "models-kws.zip")


def unpack(archive_path, destination):
    with zipfile.ZipFile(archive_path) as archive:
        for entry in archive.infolist():
            path = Path(entry.filename)
            if path.is_absolute() or ".." in path.parts or not path.parts:
                raise SystemExit(f"Unsafe archive entry: {entry.filename}")
            if (entry.external_attr >> 16) & 0o170000 == 0o120000:
                raise SystemExit(f"Symlink not allowed: {entry.filename}")
            target = destination / path
            if entry.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(entry) as source, target.open("wb") as output:
                    shutil.copyfileobj(source, output)


def make_drag_install_dmg(app):
    dmg = app.with_suffix(".dmg")
    if dmg.exists():
        dmg.unlink()
    # Staging next to the app makes both moves atomic and avoids copying large models.
    with tempfile.TemporaryDirectory(prefix="desktop-pet-dmg-", dir=app.parent) as temp:
        staged_app = Path(temp) / app.name
        app.rename(staged_app)
        try:
            (Path(temp) / "Applications").symlink_to("/Applications", target_is_directory=True)
            command = [
                "hdiutil", "create", "-volname", "DesktopPet", "-srcfolder", temp,
                "-format", "UDZO", "-ov", str(dmg),
            ]
            for attempt in range(1, 5):
                result = subprocess.run(command, capture_output=True, text=True)
                if result.stdout:
                    print(result.stdout, end="", file=sys.stdout)
                if result.stderr:
                    print(result.stderr, end="", file=sys.stderr)
                if result.returncode == 0:
                    break
                if "Resource busy" not in result.stdout + result.stderr or attempt == 4:
                    result.check_returncode()
                dmg.unlink(missing_ok=True)
                delay = 2 ** attempt
                print(f"hdiutil resource busy; retrying DMG creation in {delay}s ({attempt}/3)", file=sys.stderr)
                time.sleep(delay)
        finally:
            staged_app.rename(app)
    return dmg


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets-dir", type=Path, help="directory with separate resource ZIPs and SHA256SUMS.json")
    parser.add_argument("--require-assets", action="store_true", help="fail unless full character and voice assets exist")
    parser.add_argument("--output", type=Path, default=ROOT / "artifacts/local/release/DesktopPet.app")
    parser.add_argument("--data-dir", type=Path, help="isolated local test data directory; never use for published builds")
    args = parser.parse_args()
    if args.output.suffix != ".app":
        parser.error("--output must end in .app")
    if args.require_assets and not args.assets_dir:
        parser.error("--require-assets needs --assets-dir")
    assets = args.assets_dir
    available = {path.name: path for path in assets.glob("*.zip")} if assets else {}
    if args.require_assets:
        missing = set(REQUIRED) - available.keys()
        if missing:
            parser.error("missing required archives: " + ", ".join(sorted(missing)))
    if assets and (assets / "SHA256SUMS.json").is_file():
        checks = json.loads((assets / "SHA256SUMS.json").read_text())
        for name, path in available.items():
            if name not in checks:
                parser.error(f"no checksum for {name}")
            digest = hashlib.file_digest(path.open("rb"), "sha256").hexdigest()
            if digest != checks[name]["sha256"]:
                parser.error(f"checksum mismatch: {name}")
    app = args.output.resolve()
    if app.exists():
        shutil.rmtree(app)
    contents = app / "Contents"
    macos = contents / "MacOS"
    resources = contents / "Resources"
    helper = contents / "Helpers/DesktopPet Avatar Host.app"
    helper_macos = helper / "Contents/MacOS"
    for directory in (macos, resources, helper_macos):
        directory.mkdir(parents=True)
    shutil.copy2(ROOT / "target/release/desktop-pet", macos / "desktop-pet")
    shutil.copy2(ROOT / "target/release/avatar-host-2d", helper_macos / "avatar-host-2d")
    shutil.copy2(ROOT / "apps/desktop/icons/icon.icns", resources / "icon.icns")
    for name, path in sorted(available.items()):
        if name in ("icons.zip", "models-asr-ncnn.zip"):
            continue  # Icons are compiled in; ncnn is an optional separate backend.
        unpack(path, resources)
    if (resources / "avatar/manifest.json").is_file():
        (resources / "model-path.txt").write_text("avatar/manifest.json\n")
    else:
        (resources / "show-settings-on-launch").touch()
    if args.data_dir:
        (resources / "dev-data-dir.txt").write_text(str(args.data_dir.resolve()) + "\n")
    common = dict(CFBundleVersion="1", CFBundleShortVersionString="0.1.0", CFBundlePackageType="APPL")
    with (helper / "Contents/Info.plist").open("wb") as stream:
        plistlib.dump(dict(common, CFBundleIdentifier="dev.desktoppet.alpha.avatar-host",
                           CFBundleName="DesktopPet Avatar Host", CFBundleExecutable="avatar-host-2d",
                           LSUIElement=True), stream)
    with (contents / "Info.plist").open("wb") as stream:
        plistlib.dump(dict(common, CFBundleIdentifier="dev.desktoppet.alpha",
                           CFBundleName="DesktopPet", CFBundleDisplayName="DesktopPet",
                           CFBundleExecutable="desktop-pet", CFBundleIconFile="icon.icns",
                           NSHighResolutionCapable=True, LSUIElement=True,
                           NSMicrophoneUsageDescription="语音交互需要使用麦克风识别你的话语。"), stream)
    subprocess.run(["codesign", "--force", "--sign", "-", str(helper)], check=True)
    subprocess.run(["codesign", "--force", "--sign", "-", str(app)], check=True)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
    dmg = make_drag_install_dmg(app)
    print(f"{app}\n{dmg}")


if __name__ == "__main__":
    main()
