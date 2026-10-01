#!/usr/bin/env python3
"""Prepare and audit public, resource-free Linux x86_64 AppImages."""
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

ROOT = Path(__file__).resolve().parents[2]
RESOURCE_NAMES = {"show-settings-on-launch", "voice-settings.json", "release.json"}
FORBIDDEN_SUFFIXES = {".moc3", ".onnx", ".gguf", ".safetensors", ".pfx", ".pem"}


def elf_x64(path):
    with path.open("rb") as stream:
        header = stream.read(20)
    if len(header) < 20 or header[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", header, 18)[0] != 62:
        raise ValueError(f"expected Linux x86_64 ELF: {path}")


def prepare(binaries, output, vulkan_loader):
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
    # Deliberately no asset input, directory glob, saved data, or credentials.
    # Published builds use Tauri's normal per-user data directory.
    (resources / "show-settings-on-launch").touch()
    (resources / "voice-settings.json").write_text(json.dumps(dict(
        enabled=False, kws_enabled=False, wake_greeting_enabled=False,
        timed_greetings_enabled=False, first_greeting_enabled=False,
        web_enabled=False, output_volume_percent=0
    ), indent=2) + "\n")
    (resources / "release.json").write_text(json.dumps(dict(
        source_commit=commit, architecture="x86_64", base_system="Ubuntu 22.04",
        desktop="X11 experimental; native Wayland unsupported; XWayland unvalidated",
        bundled_character=False, bundled_voice_models=False
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

Make the AppImage executable and launch it. Settings opens on startup; import
your own prepared avatar manifest.json. Character assets and voice models are
not included. Voice, KWS, greetings and web access default to disabled.
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
    files = {f"/usr/bin/resources/{name}": str(resources / name) for name in sorted(RESOURCE_NAMES)}
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
    actual = {p.relative_to(resources).as_posix() for p in resources.rglob("*")}
    if actual != RESOURCE_NAMES:
        raise ValueError(f"unexpected public resources: {actual ^ RESOURCE_NAMES}")
    metadata = json.loads((resources / "release.json").read_text())
    if metadata["source_commit"] != expected_commit or metadata["bundled_character"] or metadata["bundled_voice_models"]:
        raise ValueError("release provenance or resource privacy mismatch")
    settings = json.loads((resources / "voice-settings.json").read_text())
    if any(settings[key] for key in ("enabled", "kws_enabled", "web_enabled", "first_greeting_enabled", "timed_greetings_enabled", "wake_greeting_enabled")):
        raise ValueError("public interaction build must start without voice or networking")
    for path in appdir.rglob("*"):
        relative = path.relative_to(appdir)
        if path.is_symlink() and (os.path.isabs(os.readlink(path)) or not path.resolve().is_relative_to(appdir.resolve())):
            raise ValueError(f"unsafe AppDir symlink: {relative}")
        if path.suffix.lower() in FORBIDDEN_SUFFIXES or path.name.startswith((".env", "id_ed25519", "id_rsa", "libnvidia", "libcuda")) or path.name in ("dev-data-dir.txt", "care.sqlite3", "preferences.json", "placement.json", "nvidia_icd.json"):
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
                      sha256=hashlib.sha256(image.read_bytes()).hexdigest(),
                      scope="extracted AppImage architecture, helpers, ABI, resources and privacy; no GPU/Wayland acceptance")
        report.write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps(result))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    stage = sub.add_parser("prepare")
    stage.add_argument("--binaries", required=True, type=Path)
    stage.add_argument("--output", required=True, type=Path)
    stage.add_argument("--vulkan-loader", default=Path("/usr/lib/x86_64-linux-gnu/libvulkan.so.1"), type=Path)
    check = sub.add_parser("verify")
    check.add_argument("--image", required=True, type=Path)
    check.add_argument("--expected-commit", required=True)
    check.add_argument("--report", required=True, type=Path)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.binaries.resolve(), args.output, args.vulkan_loader)
    else:
        verify(args.image, args.expected_commit, args.report)


if __name__ == "__main__":
    main()
