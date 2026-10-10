#!/usr/bin/env python3
"""Import the tested AXERA shared build; validate its identity without inference."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

REVISION = "5473e19db9c08e63aa41e4c273d9b3f743f1cd25"
FILES = {
    "lib/libsherpa-onnx-c-api.so": "4ab8adf8f619fac437fa0eb2b10342528b39afc02a704650d209aac668661fe5",
    "lib/libonnxruntime.so": "7b31d696ba5020ae72c967e499e802af8123778f1540237bff155db15fe0d7d7",
    "include/c-api.h": "4645835a245bf5c7907544f64a367747700171d9929fe24490ea8d8b6e7f25cc",
}
SOURCE_FILES = {
    "sherpa-onnx/csrc/axera/offline-tts-zipvoice-model-axera.cc": "b5ece4b451fb3c094ee482b1cb6dcdf926ef68fc922a1190c87b7e40be879c11",
}
LICENSES = ("sherpa-onnx.LICENSE", "onnxruntime.LICENSE", "onnxruntime.ThirdPartyNotices.txt")
SDK = ("libax_engine.so", "libax_sys.so", "libax_interpreter.so")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate(directory):
    if directory.is_symlink() or any((directory / name).is_symlink() for name in ("lib", "include", "licenses")):
        raise ValueError("Native cache directory symlink rejected")
    for relative, expected in FILES.items():
        path = directory / relative
        if path.is_symlink() or not path.is_file() or digest(path) != expected:
            raise ValueError(f"Pinned native runtime mismatch: {relative}")
    for name in LICENSES:
        path = directory / "licenses" / name
        if path.is_symlink() or not path.is_file() or not path.stat().st_size:
            raise ValueError(f"Missing native license: {name}")
    receipt = json.loads((directory / "runtime.json").read_text())
    if receipt.get("revision") != REVISION or receipt.get("files") != FILES:
        raise ValueError("Native runtime receipt does not match the pinned fork")
    return receipt


def fetch(directory, host, build_root):
    if not host or host.startswith("-") or any(char.isspace() for char in host):
        raise ValueError("Use an existing SSH host alias")
    if not build_root.startswith("/") or any(char not in "/-_.abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789" for char in build_root):
        raise ValueError("Native build path must be an absolute plain POSIX path")
    sources = {
        "lib/libsherpa-onnx-c-api.so": f"{build_root}/build/lib/libsherpa-onnx-c-api.so",
        "lib/libonnxruntime.so": f"{build_root}/build/lib/libonnxruntime.so",
        "include/c-api.h": f"{build_root}/source/sherpa-onnx/c-api/c-api.h",
        "licenses/sherpa-onnx.LICENSE": f"{build_root}/source/LICENSE",
        "licenses/onnxruntime.LICENSE": f"{build_root}/deps/onnxruntime-src/LICENSE",
        "licenses/onnxruntime.ThirdPartyNotices.txt": f"{build_root}/deps/onnxruntime-src/ThirdPartyNotices.txt",
        **{f"sdk/{name}": f"/soc/lib/{name}" for name in SDK},
    }
    # Never silently turn a different board build into the pinned native artifact.
    for relative, source in sources.items():
        destination = directory / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        if destination.is_symlink():
            raise ValueError(f"Destination symlink rejected: {relative}")
        if not destination.is_file():
            subprocess.run(["scp", "-q", "-o", "BatchMode=yes", f"{host}:{source}", str(destination)], check=True)
    script = "import pathlib,hashlib,json\nroot=pathlib.Path(" + repr(build_root + "/source") + ")\nfiles=" + repr(SOURCE_FILES) + "\nprint(json.dumps({p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in files}))\n"
    actual = json.loads(subprocess.check_output(["ssh", "-o", "BatchMode=yes", host, "python3 -"], input=script, text=True))
    if actual != SOURCE_FILES:
        raise ValueError("Board AXERA implementation differs from the pinned branch")
    receipt = {"revision": REVISION, "branch": "codex/zipvoice-axera", "files": FILES,
               "source_files": SOURCE_FILES, "provider": "axera", "sdk": "board /soc/lib (build-only; not redistributed)"}
    (directory / "runtime.json").write_text(json.dumps(receipt, indent=2) + "\n")
    validate(directory)
    print(f"Verified AXERA shared runtime at {REVISION}: {directory}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--host", default="ax650")
    parser.add_argument("--build-root", default="/root/Sherpa-ZipVoice-AXERA-PR")
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    if args.verify:
        validate(args.directory)
        print("Pinned AXERA shared runtime verified")
    else:
        fetch(args.directory, args.host, args.build_root)
