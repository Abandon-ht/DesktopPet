#!/usr/bin/env python3
"""Prepare and audit Linux AppImages with selected, verified Release resources."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
RESOURCE_NAMES = {"show-settings-on-launch", "voice-settings.json", "release.json"}
FORBIDDEN_SUFFIXES = {".moc3", ".onnx", ".gguf", ".safetensors", ".pfx", ".pem"}
ARCHIVES = ("avatar.zip", "voice.zip", "models-vad.zip", "models-asr.zip", "models-tts.zip", "models-kws.zip")


def digest(path):
    with path.open("rb") as stream:
        value = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
        return value.hexdigest()


def unpack_resources(assets, resources):
    checks = json.loads((assets / "SHA256SUMS.json").read_text())
    # Verify every selected archive before extracting any content.
    for name in ARCHIVES:
        path = assets / name
        if path.is_symlink() or digest(path) != checks[name]["sha256"] or path.stat().st_size != checks[name]["bytes"]:
            raise ValueError(f"resource checksum mismatch: {name}")
    inventory = {}
    for name in ARCHIVES:
        category = "models" if name.startswith("models-") else name.removesuffix(".zip")
        with zipfile.ZipFile(assets / name) as archive:
            for entry in archive.infolist():
                path = Path(entry.filename)
                if not path.parts or path.is_absolute() or ".." in path.parts or "\\" in entry.filename or path.parts[0] not in (category, "notices"):
                    raise ValueError(f"unsafe resource entry: {entry.filename}")
                if (entry.external_attr >> 16) & 0o170000 == 0o120000:
                    raise ValueError(f"resource symlink: {entry.filename}")
                if entry.is_dir():
                    continue
                if path.suffix.lower() in (".pem", ".pfx") or path.name.startswith((".env", "id_rsa", "id_ed25519")) or path.name in ("care.sqlite3", "preferences.json", "placement.json", "dev-data-dir.txt"):
                    raise ValueError(f"private resource entry: {entry.filename}")
                target = resources / path
                if target.exists():
                    raise ValueError(f"duplicate resource entry: {entry.filename}")
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(entry) as source, target.open("wb") as output:
                    shutil.copyfileobj(source, output)
                inventory[path.as_posix()] = digest(target)
    manifest = json.loads((resources / "avatar/manifest.json").read_text())
    if manifest.get("renderer") != "live2d_mocari":
        raise ValueError("Release avatar is not a prepared Live2D pack")
    (resources / "model-path.txt").write_text("avatar/manifest.json\n")
    inventory["model-path.txt"] = digest(resources / "model-path.txt")
    return inventory, {name: checks[name] for name in ARCHIVES}


def elf_x64(path):
    with path.open("rb") as stream:
        header = stream.read(20)
    if len(header) < 20 or header[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", header, 18)[0] != 62:
        raise ValueError(f"expected Linux x86_64 ELF: {path}")


def prepare(binaries, output, vulkan_loader, assets=None, resource_tag=None):
    output = output.resolve()
    if output.exists():
        raise ValueError("staging output must be a new directory")
    if output.is_relative_to(ROOT) and subprocess.run(
        ["git", "check-ignore", "-q", str(output)], cwd=ROOT
    ).returncode:
        raise ValueError("staging inside the checkout must be ignored by Git")
    for name in ("desktop-pet", "avatar-host-2d"):
        elf_x64(binaries / name)
    elf_x64(vulkan_loader)
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    output.mkdir(parents=True)
    resources = output / "resources"
    resources.mkdir()
    # Only selected Release ZIPs, never an arbitrary local character/model tree.
    inventory, archives = unpack_resources(assets, resources) if assets else ({}, {})
    (resources / "show-settings-on-launch").touch()
    (resources / "voice-settings.json").write_text(json.dumps(dict(
        enabled=False, kws_enabled=False, wake_greeting_enabled=False,
        timed_greetings_enabled=False, first_greeting_enabled=False,
        web_enabled=False, output_volume_percent=70,
        model_dir="@desktop-pet-resources/models" if assets else "",
        greeting_dir="@desktop-pet-resources/voice" if assets else ""
    ), indent=2) + "\n")
    (resources / "release.json").write_text(json.dumps(dict(
        source_commit=commit, architecture="x86_64", base_system="Ubuntu 22.04",
        desktop="X11 experimental; native Wayland unsupported; XWayland unvalidated",
        bundled_character=bool(assets), bundled_voice_models=bool(assets),
        resource_tag=resource_tag, resource_archives=archives, resource_files=inventory
    ), indent=2) + "\n")
    sidecar = output / "avatar-host-2d-x86_64-unknown-linux-gnu"
    shutil.copy2(binaries / "avatar-host-2d", sidecar)
    sidecar.chmod(0o755)
    notices = output / "notices"
    notices.mkdir()
    for name in ("POLICY.md", "docs/ASSETS.md"):
        shutil.copy2(ROOT / name, notices / Path(name).name)
    (notices / "README.txt").write_text("""DesktopPet Linux x86_64 alpha / AppImage

Requires glibc 2.35+ and a compatible GPU/Vulkan driver, X11 compositor and
AppIndicator/StatusNotifier tray host. Native Wayland is not implemented and
XWayland is unvalidated. CentOS and other desktop environments need separate QA.

Make the AppImage executable and launch it. The bundled character and settings
open on startup. Selected Release character, interaction
voices and CPU voice models are included in resource-enabled builds.
Voice, KWS, greetings and web access default to disabled; enable in settings
when audio devices are available. Linux audio has not been validated.
User saves use the normal per-user application data directory outside the image.
Close settings to hide it; use the tray to reopen settings or quit the app.
Other-application window snapping is not implemented on Linux.

For the previously verified NVIDIA Vulkan / XFCE X11 alpha combination only:
GDK_BACKEND=x11 DESKTOPPET_X11_OPAQUE_ALPHA=1 ./DesktopPet-linux-x86_64.AppImage
Do not assume that compatibility switch has been validated on every desktop.

If FUSE is unavailable, run with --appimage-extract-and-run, or extract with
--appimage-extract and launch squashfs-root/AppRun from the extracted directory.
This alpha is not a promise of compatibility with every Linux distribution.
""")
    files = {f"/usr/bin/resources/{path.relative_to(resources).as_posix()}": str(path)
             for path in resources.rglob("*") if path.is_file()}
    files.update({f"/usr/share/doc/desktoppet/{path.name}": str(path) for path in notices.iterdir()})
    # wgpu loads Vulkan dynamically, so ldd alone will not discover the loader.
    # GPU driver libraries and ICD manifests always come from the host.
    files["/usr/lib/libvulkan.so.1"] = str(vulkan_loader.resolve(strict=True))
    config = dict(bundle=dict(active=True, targets=["appimage"],
        externalBin=[str(output / "avatar-host-2d")], resources={},
        linux=dict(appimage=dict(files=files, bundleMediaFramework=False))))
    config_path = output / "tauri-appimage.conf.json"
    config_path.write_text(json.dumps(config, indent=2) + "\n")
    print(config_path)


def audit(appdir, expected_commit):
    resources = appdir / "usr/bin/resources"
    metadata = json.loads((resources / "release.json").read_text())
    inventory = metadata["resource_files"]
    actual = {p.relative_to(resources).as_posix() for p in resources.rglob("*") if p.is_file()}
    expected = RESOURCE_NAMES | inventory.keys()
    if actual != expected:
        raise ValueError(f"unexpected public resources: {actual ^ expected}")
    if metadata["source_commit"] != expected_commit:
        raise ValueError("release provenance mismatch")
    if metadata["bundled_character"]:
        if set(metadata["resource_archives"]) != set(ARCHIVES) or not metadata["bundled_voice_models"] or "avatar/manifest.json" not in inventory:
            raise ValueError("incomplete Release resources")
    elif inventory or metadata["bundled_voice_models"]:
        raise ValueError("resource-free metadata mismatch")
    for name, checksum in inventory.items():
        if digest(resources / name) != checksum:
            raise ValueError(f"resource changed after staging: {name}")
    settings = json.loads((resources / "voice-settings.json").read_text())
    if any(settings[key] for key in ("enabled", "kws_enabled", "web_enabled", "first_greeting_enabled", "timed_greetings_enabled", "wake_greeting_enabled")):
        raise ValueError("public interaction build must start without voice or networking")
    for path in appdir.rglob("*"):
        relative = path.relative_to(appdir)
        if path.is_symlink() and (os.path.isabs(os.readlink(path)) or not path.resolve().is_relative_to(appdir.resolve())):
            raise ValueError(f"unsafe AppDir symlink: {relative}")
        approved_asset = path.is_relative_to(resources) and path.relative_to(resources).as_posix() in inventory
        if (path.suffix.lower() in FORBIDDEN_SUFFIXES and not approved_asset) or path.name.startswith((".env", "id_ed25519", "id_rsa", "libnvidia", "libcuda")) or path.name in ("dev-data-dir.txt", "care.sqlite3", "preferences.json", "placement.json", "nvidia_icd.json"):
            raise ValueError(f"private data, assets or GPU driver in public AppDir: {relative}")
    for name in ("desktop-pet", "avatar-host-2d"):
        elf_x64(appdir / "usr/bin" / name)
    if not (appdir / "AppRun").is_file() or not os.access(appdir / "AppRun", os.X_OK):
        raise ValueError("AppRun is missing or not executable")
    if not list(appdir.rglob("libvulkan.so.1")) or not list(appdir.rglob("WebKitWebProcess")) or not list(appdir.rglob("WebKitNetworkProcess")):
        raise ValueError("Vulkan loader or WebKit helpers missing")
    return metadata


def verify(image, expected_commit, report):
    elf_x64(image)
    with image.open("rb") as stream:
        stream.seek(8)
        if stream.read(3) != b"AI\x02":
            raise ValueError("expected a type-2 AppImage")
    with tempfile.TemporaryDirectory(prefix="desktoppet-appimage-audit-") as temp:
        subprocess.run([str(image.resolve()), "--appimage-extract"], cwd=temp,
                       stdout=subprocess.DEVNULL, check=True)
        appdir = Path(temp) / "squashfs-root"
        metadata = audit(appdir, expected_commit)
        abi = {}
        for name in ("desktop-pet", "avatar-host-2d"):
            output = subprocess.check_output(["readelf", "--version-info", str(appdir / "usr/bin" / name)], text=True)
            versions = {tuple(map(int, match.split("."))) for match in re.findall(r"GLIBC_(\d+\.\d+)", output)}
            maximum = max(versions)
            if maximum > (2, 35):
                raise ValueError(f"{name} exceeds the Ubuntu 22.04 glibc baseline: {maximum}")
            abi[name] = ".".join(map(str, maximum))
        result = dict(passed=True, source=metadata, maximum_glibc_symbols=abi,
                      sha256=digest(image),
                      scope="extracted AppImage architecture, helpers, ABI, resources and privacy; no GPU/Wayland acceptance")
        report.write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps(result))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    stage = sub.add_parser("prepare")
    stage.add_argument("--binaries", required=True, type=Path)
    stage.add_argument("--output", required=True, type=Path)
    stage.add_argument("--assets-dir", type=Path)
    stage.add_argument("--resource-tag")
    stage.add_argument("--vulkan-loader", default=Path("/usr/lib/x86_64-linux-gnu/libvulkan.so.1"), type=Path)
    check = sub.add_parser("verify")
    check.add_argument("--image", required=True, type=Path)
    check.add_argument("--expected-commit", required=True)
    check.add_argument("--report", required=True, type=Path)
    args = parser.parse_args()
    if args.command == "prepare":
        if bool(args.assets_dir) != bool(args.resource_tag):
            parser.error("--assets-dir and --resource-tag must be provided together")
        prepare(args.binaries.resolve(), args.output, args.vulkan_loader, args.assets_dir, args.resource_tag)
    else:
        verify(args.image, args.expected_commit, args.report)


if __name__ == "__main__":
    main()
