#!/usr/bin/env python3
"""Prepare current source and locked Cargo dependencies for an offline Linux build.

Usage: prepare-offline.py OUTPUT_DIRECTORY
Run from the canonical checkout. This includes current tracked edits and new
non-ignored files, excludes Git internals and local assets, and never uploads.
Native system packages, Rust and sherpa-onnx archives are prepared separately.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    output = Path(sys.argv[1]).resolve()
    output.mkdir(parents=True, exist_ok=True)
    if output.is_relative_to(ROOT) and not subprocess.run(
        ["git", "check-ignore", "-q", str(output)], cwd=ROOT
    ).returncode == 0:
        raise ValueError("output inside checkout must be ignored by Git")
    vendor = output / "vendor"
    result = subprocess.run(["cargo", "vendor", "--locked", str(vendor)], cwd=ROOT, check=True, capture_output=True, text=True)
    (output / "vendor-config.toml").write_text(result.stdout)
    names = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=ROOT).decode().split("\0")
    files = sorted({name for name in names if name and (ROOT / name).is_file()})
    with tarfile.open(output / "source.tar.gz", "w:gz") as archive:
        for name in files:
            path = ROOT / name
            if path.is_symlink():
                raise ValueError(f"source symlinks unsupported: {name}")
            archive.add(path, arcname=name, recursive=False)
    with tarfile.open(output / "vendor.tar.gz", "w:gz") as archive:
        archive.add(vendor, arcname="vendor")
    manifest = {"git_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip(),
                "files": files, "archives": {}}
    for name in ("source.tar.gz", "vendor.tar.gz"):
        path = output / name
        hasher = hashlib.sha256()
        with path.open("rb") as stream:
            while chunk := stream.read(1024 * 1024):
                hasher.update(chunk)
        digest = hasher.hexdigest()
        manifest["archives"][name] = dict(bytes=path.stat().st_size, sha256=digest)
    (output / "source-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (output / "SOURCE-SHA256SUMS").write_text("".join(f"{value['sha256']}  {name}\n" for name, value in manifest["archives"].items()))
    print(f"Prepared {len(files)} source files and locked dependencies in {output}")


if __name__ == "__main__":
    main()
