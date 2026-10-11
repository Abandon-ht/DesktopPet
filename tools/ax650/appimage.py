#!/usr/bin/env python3
"""Stage verified Release resources and finalize an AX650 software-GL AppImage."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


linux = load("linux_appimage", HERE.parent / "linux/appimage.py")
packaging = load("ax650_package", HERE / "package.py")
native = load("ax650_native", HERE / "native-runtime.py")
models = load("ax650_model_resources", HERE / "model-resources.py")
runtime_deps = load("ax650_runtime_deps", HERE / "runtime-deps.py")

APP_RUN = '''#!/usr/bin/env bash
set -e
export APPDIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
export DESKTOPPET_DEVICE_PROFILE=ax650
export LIBGL_ALWAYS_SOFTWARE=1
export WGPU_BACKEND=gl
export DESKTOPPET_X11_GL_ALPHA=1
unset DESKTOPPET_X11_OPAQUE_ALPHA
export GDK_BACKEND=x11
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export LD_LIBRARY_PATH="$APPDIR/usr/lib:/soc/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$APPDIR/AppRun.tauri" "$@"
'''


def x11_runtime_files(directory=None):
    # winit and the tray load these with dlopen: linuxdeploy/ldd cannot discover
    # them from the executable's DT_NEEDED entries. Use the same library contract
    # as the previously tested private AX650 packages, including their notices.
    if directory is not None:
        return packaging.runtime_files(directory)
    files = []
    for name, notice in packaging.RUNTIME_LIBRARIES.items():
        source = (Path("/usr/lib/aarch64-linux-gnu") / name).resolve(strict=True)
        packaging.verify_elf(source)
        license_path = Path("/usr/share/doc") / notice.removesuffix(".copyright") / "copyright"
        if not license_path.is_file() or not license_path.stat().st_size:
            raise ValueError(f"runtime library license missing: {notice}")
        files.extend([(source, Path("lib") / name), (license_path, Path("lib/licenses") / notice)])
    return files


def prepare(binaries, output, assets, resource_tag, native_root, provenance, loader, axera_assets, axera_resource_tag, runtime_libs=None):
    receipt = json.loads(provenance.read_text())
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=linux.ROOT, text=True).strip()
    if receipt.get("sherpa_revision") != native.REVISION or receipt.get("provider") != "axera" or receipt.get("source_commit") != commit:
        raise ValueError("CI native provenance does not match the source being packaged")
    libraries = {}
    for path in sorted((native_root / "lib").glob("*.so*")):
        if path.name != "libsherpa-onnx-c-api.so" and not re.fullmatch(r"libonnxruntime\.so(?:\.\d+)*", path.name):
            raise ValueError(f"unexpected native library: {path.name}")
        if not path.resolve(strict=True).is_relative_to((native_root / "lib").resolve()):
            raise ValueError("native library escapes the approved directory")
        packaging.verify_elf(path.resolve())
        libraries[path.name] = path.resolve()
    if not {"libsherpa-onnx-c-api.so", "libonnxruntime.so"}.issubset(libraries):
        raise ValueError("incomplete AXERA / ONNX Runtime libraries")
    for name in native.LICENSES:
        if not (native_root / "licenses" / name).is_file() or not (native_root / "licenses" / name).stat().st_size:
            raise ValueError(f"missing native license: {name}")
    dynamic_files = {("usr/lib" if target.parent == Path("lib") else "usr/share/doc/desktoppet") + "/" + target.name: source
                     for source, target in x11_runtime_files(runtime_libs)}
    linux.prepare(binaries, output, loader, assets, resource_tag, architecture="aarch64")
    resources = output / "resources"
    settings_path = resources / "voice-settings.json"
    settings = json.loads(settings_path.read_text())
    settings.update(mode="transcription", asr_threads=2, tts_threads=4, asr_npu=False, tts_npu=False)
    settings_path.write_text(json.dumps(settings, indent=2) + "\n")
    metadata_path = resources / "release.json"
    metadata = json.loads(metadata_path.read_text())
    inventory, model_receipt = models.unpack(axera_assets, resources, axera_resource_tag)
    metadata["resource_files"].update(inventory)
    metadata.update(device_profile="ax650", rendering="software OpenGL", bundled_axera_models=True,
                    axera_models=model_receipt,
                    x11_runtime=dict(files={name: linux.digest(path) for name, path in dynamic_files.items()}),
                    native_runtime=dict(sherpa_revision=native.REVISION, sdk_revision=receipt["sdk_revision"],
                                        origin=receipt["native_origin"], sdk_bundled=False,
                                        files={f"usr/lib/{name}": linux.digest(path) for name, path in libraries.items()}),
                    launcher_sha256=hashlib.sha256(APP_RUN.encode()).hexdigest())
    metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")
    config_path = output / "tauri-appimage.conf.json"
    config = json.loads(config_path.read_text())
    files = config["bundle"]["linux"]["appimage"]["files"]
    # Tauri serializes this config into TAURI_CONFIG for cargo. A per-file map
    # exceeds Linux's 128 KiB single-environment-string limit with NPU frontend
    # data. Its pinned bundler copies directory mappings recursively; keep the
    # full per-file hashes in release.json, outside that environment variable.
    for target in list(files):
        if target.startswith("/usr/bin/resources/"):
            del files[target]
    files["/usr/bin/resources"] = str(resources)
    files.update({f"/usr/lib/{name}": str(path) for name, path in libraries.items()})
    files.update({f"/{name}": str(path) for name, path in dynamic_files.items()})
    files.update({f"/usr/share/doc/desktoppet/{name}": str(native_root / "licenses" / name) for name in native.LICENSES})
    (output / "notices/README.txt").write_text('''DesktopPet AX650 ARM64 AppImage / Ubuntu 22.04 / XFCE X11

Software OpenGL, 64% initial size and a 20 fps scheduling budget; 30 fps is not required.
GTK/WebKit, tray and the avatar helper are packaged with the selected Release
character, interaction voices and CPU VAD/ASR/TTS/KWS models. Voice starts disabled
in transcription mode (ASR 2 threads, TTS 4 threads). For LLM/TTS configuration,
switch the voice mode to recognition, conversation and speech.

AXERA sherpa is included; the board supplies its matching SDK in /soc/lib.
XKB/XCB and xdo runtime libraries and their licenses are explicitly bundled.
The tested AX650 SenseVoice (10 seconds), ZipVoice distill (encoder and four
decoder parts) and Vocos axmodels are included with matched tokens, manifest,
lexicon and espeak data. Enable NPU acceleration in ASR / TTS settings to select
the bundled models automatically. CPU models remain available when unchecked.

Make the image executable and launch it from the existing XFCE session.
Use --appimage-extract-and-run if FUSE is unavailable. Per-user settings and saves
remain outside the image. Real role, tray, audio and NPU inference are user tests.
''')
    config_path.write_text(json.dumps(config, indent=2) + "\n")


def finalize_tree(appdir):
    # linuxdeploy may discover the SDK through the CI link environment. Remove
    # those copies before creating any publishable artifact; the board owns them.
    removed = []
    for path in sorted(appdir.rglob("libax_*")):
        if path.is_file() or path.is_symlink():
            removed.append(path.relative_to(appdir).as_posix())
            path.unlink()
    original = appdir / "AppRun"
    original.rename(appdir / "AppRun.tauri")
    original.write_text(APP_RUN)
    original.chmod(0o755)
    metadata_path = appdir / "usr/bin/resources/release.json"
    metadata = json.loads(metadata_path.read_text())
    runtime = metadata["native_runtime"]
    runtime["build_files"] = dict(runtime["files"])
    # linuxdeploy strips and changes RPATHs. Record its resulting bytes separately
    # from build inputs, and make the native C API resolve board drivers at runtime.
    subprocess.run(["patchelf", "--set-rpath", "$ORIGIN:/soc/lib",
                    str(appdir / "usr/lib/libsherpa-onnx-c-api.so")], check=True)
    for relative in runtime["files"]:
        path = Path(relative)
        if path.parent != Path("usr/lib") or (path.name != "libsherpa-onnx-c-api.so" and not re.fullmatch(r"libonnxruntime\.so(?:\.\d+)*", path.name)):
            raise ValueError("native runtime path outside allowlist")
        runtime["files"][relative] = linux.digest(appdir / path)
    dynamic = metadata["x11_runtime"]
    dynamic["build_files"] = dict(dynamic["files"])
    for relative in dynamic["files"]:
        dynamic["files"][relative] = linux.digest(appdir / relative)
    metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")
    return removed


def check_x11_loads(appdir):
    # A fresh process needs LD_LIBRARY_PATH before startup. Do not inherit the
    # CI native/SDK library search path, which could hide missing packaged libs.
    env = os.environ.copy()
    env["LD_LIBRARY_PATH"] = str(appdir / "usr/lib")
    env.pop("LD_PRELOAD", None)
    libraries = [str(appdir / "usr/lib" / name) for name in packaging.RUNTIME_LIBRARIES]
    code = ("import ctypes\n"
            f"handles = [ctypes.CDLL(p) for p in {libraries!r}]\n"
            f"[getattr(handles[0], s) for s in {runtime_deps.XKB_SYMBOLS!r}]\n")
    subprocess.run([sys.executable, "-c", code], env=env, check=True, text=True)


def audit(appdir, metadata, load_runtime=False):
    if metadata.get("device_profile") != "ax650" or metadata.get("rendering") != "software OpenGL":
        raise ValueError("AX650 software rendering profile missing")
    runtime = metadata["native_runtime"]
    if runtime.get("sherpa_revision") != native.REVISION or runtime.get("sdk_bundled") is not False:
        raise ValueError("unexpected AXERA runtime provenance")
    files = runtime["files"]
    if not {"usr/lib/libsherpa-onnx-c-api.so", "usr/lib/libonnxruntime.so"}.issubset(files):
        raise ValueError("AXERA native runtime is missing")
    checks = {}
    for relative, checksum in files.items():
        path = Path(relative)
        if path.parent != Path("usr/lib") or (path.name != "libsherpa-onnx-c-api.so" and not re.fullmatch(r"libonnxruntime\.so(?:\.\d+)*", path.name)):
            raise ValueError("native runtime path outside allowlist")
        actual = appdir / path
        if linux.digest(actual) != checksum:
            raise ValueError(f"native runtime changed: {relative}")
        checks[relative] = packaging.verify_elf(actual.resolve())
    if (appdir / "AppRun").read_text() != APP_RUN or linux.digest(appdir / "AppRun") != metadata["launcher_sha256"]:
        raise ValueError("AX650 launcher mismatch")
    if not (appdir / "AppRun.tauri").is_file():
        raise ValueError("Tauri launcher missing")
    dynamic = metadata.get("x11_runtime", {}).get("files", {})
    expected = {f"usr/lib/{name}" for name in packaging.RUNTIME_LIBRARIES} | {
        f"usr/share/doc/desktoppet/{notice}" for notice in packaging.RUNTIME_LIBRARIES.values()}
    if set(dynamic) != expected:
        raise ValueError("XKB/XCB/xdo runtime or licenses missing from inventory")
    dynamic_checks = {}
    for relative, checksum in dynamic.items():
        path = appdir / relative
        if not path.is_file() or linux.digest(path) != checksum:
            raise ValueError(f"X11 runtime changed or missing: {relative}")
        if relative.startswith("usr/lib/"):
            dynamic_checks[relative] = packaging.verify_elf(path.resolve())
        elif not path.stat().st_size:
            raise ValueError(f"empty X11 runtime license: {relative}")
    if load_runtime:
        check_x11_loads(appdir)
    model_checks = models.audit(appdir / "usr/bin/resources", metadata)
    return dict(native_elf=checks, x11_elf=dynamic_checks, x11_load_checked=load_runtime,
                axera_models=model_checks, sdk_bundled=False, rendering="software OpenGL",
                scope="packaging and provenance only; no desktop, microphone or NPU inference")


def finish(image, output, expected_commit, report):
    linux.elf_arch(image, "aarch64")
    if output.exists():
        raise ValueError("final AppImage output must be a new file")
    offset = int(subprocess.check_output([str(image.resolve()), "--appimage-offset"], text=True).strip())
    if not 0 < offset < image.stat().st_size:
        raise ValueError("invalid AppImage runtime offset")
    with image.open("rb") as stream:
        prefix = stream.read(offset)
        if stream.read(4) != b"hsqs":
            raise ValueError("AppImage has no SquashFS at the reported offset")
    with tempfile.TemporaryDirectory(prefix="ax650-appimage-") as temp:
        work = Path(temp)
        subprocess.run([str(image.resolve()), "--appimage-extract"], cwd=work,
                       stdout=subprocess.DEVNULL, check=True)
        appdir = work / "squashfs-root"
        removed = finalize_tree(appdir)
        metadata = linux.audit(appdir, expected_commit, "aarch64")
        audit(appdir, metadata)
        squashfs = work / "appdir.squashfs"
        # Tauri's type-2 runtime supports zlib/zstd, not xz. Always re-extract the
        # finished image with that same runtime before publishing it.
        subprocess.run(["mksquashfs", str(appdir), str(squashfs), "-noappend", "-all-root", "-comp", "zstd", "-no-xattrs"], check=True)
        output.parent.mkdir(parents=True, exist_ok=True)
        with output.open("wb") as destination, squashfs.open("rb") as source:
            destination.write(prefix)
            shutil.copyfileobj(source, destination)
        output.chmod(0o755)
    linux.verify(output, expected_commit, report, "aarch64",
                 extra_audit=lambda tree, metadata: audit(tree, metadata, load_runtime=True))
    print(json.dumps(dict(removed_sdk_libraries=removed)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    stage = sub.add_parser("prepare")
    for name in ("binaries", "output", "assets-dir", "native", "native-provenance", "axera-assets-dir"):
        stage.add_argument("--" + name, required=True, type=Path)
    stage.add_argument("--resource-tag", required=True)
    stage.add_argument("--axera-resource-tag", required=True)
    stage.add_argument("--vulkan-loader", type=Path, default=Path("/usr/lib/aarch64-linux-gnu/libvulkan.so.1"))
    stage.add_argument("--runtime-libs", type=Path, help="verified XKB/XCB/xdo lib directory with licenses/; defaults to Ubuntu system packages")
    final = sub.add_parser("finish")
    for name in ("image", "output", "report"):
        final.add_argument("--" + name, required=True, type=Path)
    final.add_argument("--expected-commit", required=True)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.binaries.resolve(), args.output.resolve(), args.assets_dir.resolve(), args.resource_tag,
                args.native.resolve(), args.native_provenance.resolve(), args.vulkan_loader,
                args.axera_assets_dir.resolve(), args.axera_resource_tag, args.runtime_libs)
    else:
        finish(args.image, args.output, args.expected_commit, args.report)


if __name__ == "__main__":
    main()
