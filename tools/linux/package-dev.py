#!/usr/bin/env python3
"""Make a local Linux x86_64 interaction test tree; never publish private assets."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binaries", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--avatar", type=Path, required=True,
                        help="private local manifest.json; copied only into this test package")
    parser.add_argument("--nvidia-x11-compat", action="store_true")
    args = parser.parse_args()
    output = args.output.resolve()
    archive_path = output.with_suffix(".tar.gz")
    if any(path.exists() for path in (output, archive_path, archive_path.with_name(archive_path.name + ".sha256"))):
        raise SystemExit("output and archive must be new paths")
    if output.is_relative_to(ROOT) and subprocess.run(
        ["git", "check-ignore", "-q", str(output)], cwd=ROOT
    ).returncode != 0:
        raise SystemExit("test output inside the checkout must be ignored by Git")
    manifest = args.avatar.resolve(strict=True)
    asset = json.loads(manifest.read_text())
    if manifest.name != "manifest.json" or asset.get("renderer") != "live2d_mocari":
        raise SystemExit("expected a prepared Live2D avatar manifest")
    for name in ("desktop-pet", "avatar-host-2d"):
        with (args.binaries / name).open("rb") as stream:
            header = stream.read(20)
        if len(header) < 20 or header[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", header, 18)[0] != 62:
            raise SystemExit(f"{name} must be Linux x86_64 ELF, not a preparation-host binary")
    output.mkdir(parents=True)
    resources = output / "resources"
    resources.mkdir()
    for name in ("desktop-pet", "avatar-host-2d"):
        shutil.copy2(args.binaries / name, output / name)
        (output / name).chmod(0o755)
    for source in manifest.parent.rglob("*"):
        if source.is_symlink():
            raise SystemExit(f"avatar symlinks are unsupported: {source}")
        if source.is_file() and source.suffix.lower() in (".json", ".moc3", ".png"):
            target = resources / "avatar" / source.relative_to(manifest.parent)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
    (resources / "model-path.txt").write_text("avatar/manifest.json\n")
    (resources / "show-settings-on-launch").touch()
    # The launcher sets its working directory. Saves move with this test tree;
    # production user data and the previous probe's data remain independent.
    (resources / "dev-data-dir.txt").write_text("data\n")
    (resources / "voice-settings.json").write_text(json.dumps(dict(
        enabled=False, kws_enabled=False, wake_greeting_enabled=False,
        timed_greetings_enabled=False, first_greeting_enabled=False,
        web_enabled=False, output_volume_percent=0
    ), indent=2) + "\n")
    shutil.copy2(ROOT / "apps/desktop/icons/icon.png", resources / "icon.png")
    compatibility = "export DESKTOPPET_X11_OPAQUE_ALPHA=1\n" if args.nvidia_x11_compat else ""
    (output / "run.sh").write_text('''#!/bin/sh
set -eu
app_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$app_dir"
: "${DISPLAY:?Run this launcher inside the X11 desktop session}"
export GDK_BACKEND=x11
export WGPU_BACKEND="${WGPU_BACKEND:-vulkan}"
if [ -z "${XDG_RUNTIME_DIR:-}" ]; then
    mkdir -p "$app_dir/.runtime"
    chmod 700 "$app_dir/.runtime"
    export XDG_RUNTIME_DIR="$app_dir/.runtime"
fi
''' + compatibility + '''exec "$app_dir/desktop-pet" "$@"
''')
    (output / "run.sh").chmod(0o755)
    (output / "README.txt").write_text("""DesktopPet Linux 交互测试版

目标：当前 Ubuntu 22.04 / XFCE / X11 / NVIDIA Vulkan x86_64 环境。
在桌面终端中运行 ./run.sh；首次启动显示角色和设置窗口。
托盘菜单支持显示、隐藏、设置、重试、停止互动、免打扰和退出。
关闭设置窗口会隐藏窗口，角色仍运行；通过托盘重新打开或退出。

本包已关闭语音、关键词监听、提示音和联网，未附语音模型。
可测试：部位点击与表情、透明区穿透、拖拽、底部吸附、角色大小、
视线跟随、喂食/玩耍/休息、存档恢复、托盘显示/隐藏/退出。
主动陪伴和短时占屏需在设置中显式开启，可随时停止。
他应用窗口吸附尚未实现；多屏、混合 DPI 和 Wayland 未实测。

data/ 是本测试包的独立存档，.runtime/ 为临时运行文件。
角色为用户已有的私有测试资源，版权/再分发许可未核实。
仅供当前用户本地验证，不得作为公开发行包上传。
系统 GTK/WebKit/AppIndicator/Vulkan 依赖由目标机器提供。
本包不是可跨所有 Linux 发行版使用的 AppImage 或安装包。
""")
    info = dict(git_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip(),
                scope="Ubuntu 22.04 x86_64 X11 interaction test; private assets; no audio",
                avatar_license=asset.get("license"), nvidia_x11_compat=args.nvidia_x11_compat,
                files={})
    for path in sorted(output.rglob("*")):
        if path.is_file():
            info["files"][path.relative_to(output).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    (output / "package-manifest.json").write_text(json.dumps(info, ensure_ascii=False, indent=2) + "\n")
    archive = output.with_suffix(".tar.gz")
    with tarfile.open(archive, "w:gz") as tar:
        tar.add(output, arcname=output.name)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n")
    print(output)
    print(archive)


if __name__ == "__main__":
    main()
