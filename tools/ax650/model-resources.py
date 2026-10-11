#!/usr/bin/env python3
"""Publish and consume the exact AX650 model set previously delivered to the board."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import stat
import zipfile

HERE = Path(__file__).resolve().parent
LOCK = HERE / "model-resources.lock.json"
spec = importlib.util.spec_from_file_location("ax650_model_package", HERE / "package.py")
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


def digest(path):
    with path.open("rb") as stream:
        checksum = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            checksum.update(chunk)
        return checksum.hexdigest()


def read_lock():
    lock = json.loads(LOCK.read_text())
    if lock["schema_version"] != 1 or len(lock["files"]) > 2048:
        raise ValueError("unsupported AX650 model lock")
    for name in lock["files"]:
        path = Path(name)
        if path.is_absolute() or ".." in path.parts or "\\" in name or path.parts[:2] != ("models", "axera"):
            raise ValueError("unsafe AX650 model lock entry")
    return lock


def selected_files(models):
    return {target.relative_to("resources").as_posix(): source
            for source, target in packaging.axera_model_files(models)}


def verify_files(files, lock):
    if files.keys() != lock["files"].keys():
        raise ValueError("incomplete or unexpected AX650 model files")
    for name, source in files.items():
        expected = lock["files"][name]
        if source.is_symlink() or source.stat().st_size != expected["bytes"] or digest(source) != expected["sha256"]:
            raise ValueError(f"AX650 model checksum mismatch: {name}")


def write_archive(files, archive):
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as package:
        for name, source in sorted(files.items()):
            entry = zipfile.ZipInfo(name, (2026, 10, 10, 0, 0, 0))
            entry.create_system = 3
            entry.external_attr = (stat.S_IFREG | 0o644) << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            with source.open("rb") as stream, package.open(entry, "w") as destination:
                shutil.copyfileobj(stream, destination)
    return dict(name=archive.name, sha256=digest(archive), bytes=archive.stat().st_size)


def create(models, output):
    lock = read_lock()
    files = selected_files(models)
    verify_files(files, lock)
    output.mkdir(parents=True, exist_ok=False)
    archive = output / lock["archive"]["name"]
    actual = write_archive(files, archive)
    if actual != lock["archive"]:
        raise ValueError("generated AX650 archive differs from the published lock")
    (output / "SHA256SUMS.json").write_text(json.dumps({archive.name: {key: actual[key] for key in ("sha256", "bytes")}}, indent=2) + "\n")
    print(json.dumps(actual))


def unpack(assets, resources, resource_tag):
    lock = read_lock()
    if resource_tag != lock["resource_tag"]:
        raise ValueError("AX650 model Release is not the locked revision")
    expected = lock["archive"]
    archive_path = assets / expected["name"]
    checks = json.loads((assets / "SHA256SUMS.json").read_text())
    if checks != {expected["name"]: {key: expected[key] for key in ("sha256", "bytes")}}:
        raise ValueError("AX650 Release checksum manifest differs from the source lock")
    if archive_path.is_symlink() or archive_path.stat().st_size != expected["bytes"] or digest(archive_path) != expected["sha256"]:
        raise ValueError("AX650 model archive checksum mismatch")
    with zipfile.ZipFile(archive_path) as archive:
        entries = archive.infolist()
        if len(entries) != len(lock["files"]) or {entry.filename for entry in entries} != lock["files"].keys():
            raise ValueError("unexpected or missing AX650 ZIP entries")
        for entry in entries:
            if entry.is_dir() or stat.S_IFMT(entry.external_attr >> 16) != stat.S_IFREG or entry.file_size != lock["files"][entry.filename]["bytes"]:
                raise ValueError(f"unsafe AX650 model entry: {entry.filename}")
            target = resources / entry.filename
            if target.exists() or target.is_symlink():
                raise ValueError(f"duplicate AX650 resource: {entry.filename}")
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(entry) as source, target.open("wb") as destination:
                shutil.copyfileobj(source, destination)
    files = selected_files(resources / "models/axera")
    verify_files(files, lock)
    return {name: item["sha256"] for name, item in lock["files"].items()}, dict(
        resource_tag=resource_tag, archive=expected, sources=lock["sources"],
        file_count=len(files), total_bytes=sum(item["bytes"] for item in lock["files"].values()))


def audit(resources, metadata):
    lock = read_lock()
    receipt = metadata.get("axera_models", {})
    if metadata.get("bundled_axera_models") is not True or receipt.get("archive") != lock["archive"] or receipt.get("resource_tag") != lock["resource_tag"] or receipt.get("sources") != lock["sources"]:
        raise ValueError("bundled AX650 model provenance mismatch")
    verify_files(selected_files(resources / "models/axera"), lock)
    expected = {name: item["sha256"] for name, item in lock["files"].items()}
    actual = {name: checksum for name, checksum in metadata["resource_files"].items() if name.startswith("models/axera/")}
    if actual != expected:
        raise ValueError("AX650 model inventory mismatch")
    return dict(file_count=len(expected), archive=lock["archive"], resource_tag=lock["resource_tag"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--models", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    create(args.models.resolve(), args.output.resolve())


if __name__ == "__main__":
    main()
