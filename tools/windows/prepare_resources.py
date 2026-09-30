"""Verify and unpack the default Release resources for the local Windows build."""
import hashlib
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import shutil
import zipfile

ROOT = Path(__file__).resolve().parents[2]
ARCHIVES = ROOT / "artifacts/local/release-assets"
DESTINATION = ROOT / "artifacts/local/release-resources"
NAMES = ("icons.zip", "avatar.zip", "voice.zip", "models-vad.zip",
         "models-asr.zip", "models-tts.zip", "models-kws.zip")


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def main():
    checks = json.loads((ARCHIVES / "SHA256SUMS.json").read_text(encoding="utf-8"))
    for name in NAMES:
        archive = ARCHIVES / name
        if archive.stat().st_size != checks[name]["bytes"] or sha256(archive) != checks[name]["sha256"]:
            raise SystemExit("Resource checksum mismatch: " + name)

    DESTINATION.mkdir(parents=True, exist_ok=True)
    destination = DESTINATION.resolve()
    for name in NAMES:
        with zipfile.ZipFile(ARCHIVES / name) as archive:
            for entry in archive.infolist():
                path = PurePosixPath(entry.filename)
                windows_path = PureWindowsPath(entry.filename)
                if (not path.parts or path.is_absolute() or windows_path.drive
                        or windows_path.is_absolute() or windows_path.is_reserved()
                        or ".." in path.parts or "\\" in entry.filename or ":" in entry.filename
                        or (entry.external_attr >> 16) & 0o170000 == 0o120000):
                    raise SystemExit("Unsafe archive entry: " + entry.filename)
                target = destination.joinpath(*path.parts)
                target.resolve().relative_to(destination)
                if entry.is_dir():
                    target.mkdir(parents=True, exist_ok=True)
                else:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with archive.open(entry) as source, target.open("wb") as output:
                        shutil.copyfileobj(source, output)
        print("Verified and unpacked " + name)
    print(destination)


if __name__ == "__main__":
    main()
