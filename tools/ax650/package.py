#!/usr/bin/env python3
"""Create an ignored, private Ubuntu 22.04 ARM64 AX650 development package."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import struct
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
TOOLS = Path(__file__).resolve().parent
BINARIES = ("desktop-pet-ax650", "avatar-host-2d")
RUNTIME_LIBRARIES = {
    "libxkbcommon-x11.so.0": "libxkbcommon-x11-0.copyright",
    "libxcb-xkb.so.1": "libxcb-xkb1.copyright",
}
MAX_JSON = 1024 * 1024
MAX_FILE = 32 * 1024 * 1024
MAX_PACK = 128 * 1024 * 1024


class PackageError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise PackageError(message)


def git_command(*arguments):
    # A source mount can be owned by a different UID in an ARM64 container.
    # Trust only this explicitly selected checkout, without global Git config.
    return ["git", "-c", f"safe.directory={ROOT}", *arguments]


def source_metadata():
    def value(*arguments):
        return subprocess.check_output(git_command(*arguments), cwd=ROOT, text=True).strip()
    return {
        "source_revision": value("rev-parse", "HEAD"),
        "source_branch": value("branch", "--show-current"),
        "source_worktree_dirty": bool(value("status", "--porcelain", "--untracked-files=normal")),
    }


def digest(path):
    checksum = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            checksum.update(chunk)
    return checksum.hexdigest()


def verify_elf(path):
    """Read ELF version requirements directly; works on Mac without readelf."""
    require(path.is_file() and not path.is_symlink(), f"missing or symlink binary: {path.name}")
    data = path.read_bytes()
    require(len(data) >= 64 and data[:7] == b"\x7fELF\x02\x01\x01",
            f"{path.name} must be ELF64 little endian")
    kind, machine = struct.unpack_from("<HH", data, 16)
    require(kind in (2, 3) and machine == 183, f"{path.name} must be Linux AArch64 executable ELF")
    section_offset = struct.unpack_from("<Q", data, 40)[0]
    section_size, section_count, string_index = struct.unpack_from("<HHH", data, 58)
    require(section_size == 64 and section_count > 0 and string_index < section_count,
            f"invalid ELF section table: {path.name}")
    require(section_offset + section_size * section_count <= len(data), "truncated ELF sections")
    sections = [struct.unpack_from("<IIQQQQIIQQ", data, section_offset + i * 64)
                for i in range(section_count)]

    def section_bytes(section):
        start, length = section[4:6]
        require(start + length <= len(data), "truncated ELF section data")
        return data[start:start + length]

    def string_at(strings, offset):
        require(offset < len(strings), "invalid ELF string offset")
        end = strings.find(b"\0", offset)
        require(end >= 0, "unterminated ELF string")
        return strings[offset:end].decode("ascii")

    section_names = section_bytes(sections[string_index])
    versions = []
    for section in sections:
        if string_at(section_names, section[0]) != ".gnu.version_r":
            continue
        require(section[6] < len(sections), "invalid ELF version string table")
        strings = section_bytes(sections[section[6]])
        requirements = section_bytes(section)
        position = 0
        visited = set()
        while position < len(requirements):
            require(position not in visited and position + 16 <= len(requirements), "invalid ELF version chain")
            visited.add(position)
            version, count, _, auxiliary_offset, next_offset = struct.unpack_from("<HHIII", requirements, position)
            require(version == 1, "unsupported ELF version requirements")
            auxiliary = position + auxiliary_offset
            for index in range(count):
                require(auxiliary + 16 <= len(requirements), "truncated ELF version requirement")
                _, flags, other, name_offset, auxiliary_next = struct.unpack_from("<IHHII", requirements, auxiliary)
                name = string_at(strings, name_offset)
                if name.startswith("GLIBC_"):
                    match = re.fullmatch(r"GLIBC_(\d+)\.(\d+)(?:\.(\d+))?", name)
                    require(match is not None, f"unsupported GLIBC requirement: {name}")
                    numeric = tuple(int(part or 0) for part in match.groups())
                    require(numeric <= (2, 35, 0), f"{path.name} requires {name}; AX650 limit is GLIBC_2.35")
                    versions.append(numeric)
                if not auxiliary_next:
                    require(index == count - 1, "incomplete ELF version auxiliary chain")
                    break
                auxiliary += auxiliary_next
            if not next_offset:
                break
            position += next_offset
    maximum = max(versions, default=None)
    return {"architecture": "aarch64", "glibc_required":
            ".".join(map(str, maximum[:2])) if maximum else None}


def read_json(path):
    require(path.stat().st_size <= MAX_JSON, "JSON asset exceeds 1 MiB")
    data = json.loads(path.read_text(encoding="utf-8"))
    require(isinstance(data, dict), "JSON asset must contain an object")
    return data


def safe_reference(root, base, name, suffix):
    require(isinstance(name, str) and name and "\\" not in name and "\0" not in name,
            "asset reference must be a nonempty relative POSIX path")
    relative = PurePosixPath(name)
    require(not relative.is_absolute() and all(part not in ("", ".", "..") for part in name.split("/"))
            and not relative.parts[0].endswith(":"), "absolute or escaping asset reference rejected")
    path = base.joinpath(*relative.parts)
    require(path.is_relative_to(root), "asset reference escaped avatar root")
    for current in (path, *path.parents):
        if current == root:
            break
        require(not current.is_symlink(), "avatar symlinks are unsupported")
    require(path.is_file() and path.suffix == suffix, "missing asset or unexpected asset type")
    require(stat.S_ISREG(path.stat().st_mode), "nonregular asset rejected")
    require(path.stat().st_size <= (MAX_JSON if suffix == ".json" else MAX_FILE), "asset exceeds file budget")
    return path


def avatar_files(manifest_path):
    require(manifest_path.name == "manifest.json" and not manifest_path.is_symlink()
            and not manifest_path.parent.is_symlink(), "select a regular manifest.json")
    manifest_path = manifest_path.resolve(strict=True)
    root = manifest_path.parent
    # Reject symlinks anywhere in this selected pack, but copy only references.
    require(not any(path.is_symlink() for path in root.rglob("*")), "avatar symlinks are unsupported")
    manifest = read_json(manifest_path)
    require(manifest.get("renderer") == "live2d_mocari" and manifest.get("schema_version") in (1, 2, 3, 4),
            "expected a supported Live2D avatar manifest")
    require(isinstance(manifest.get("interaction"), dict) and isinstance(manifest.get("actions"), dict)
            and isinstance(manifest.get("license"), dict), "incomplete avatar manifest")
    files = {manifest_path}
    entry = safe_reference(root, root, manifest.get("entry"), ".json")
    files.add(entry)
    model = read_json(entry)
    require(model.get("Version") == 3 and isinstance(model.get("FileReferences"), dict), "expected model3 Version 3")
    refs, base = model["FileReferences"], entry.parent
    require(set(refs).issubset({"Moc", "Textures", "Physics", "Pose", "DisplayInfo", "UserData", "Expressions", "Motions"}),
            "unsupported model FileReferences field")
    moc = safe_reference(root, base, refs.get("Moc"), ".moc3")
    require(moc.read_bytes()[:4] == b"MOC3", "invalid MOC3 asset header")
    files.add(moc)
    textures = refs.get("Textures")
    require(isinstance(textures, list) and 1 <= len(textures) <= 32, "expected 1–32 PNG textures")
    require(all(isinstance(texture, str) for texture in textures), "invalid texture reference")
    require(len(set(textures)) == len(textures), "duplicate texture references")
    decoded_bytes = 0
    for texture in textures:
        path = safe_reference(root, base, texture, ".png")
        with path.open("rb") as stream:
            header = stream.read(24)
        require(len(header) == 24 and header[:8] == b"\x89PNG\r\n\x1a\n" and header[12:16] == b"IHDR",
                "invalid PNG header")
        width, height = struct.unpack_from(">II", header, 16)
        require(0 < width <= 8192 and 0 < height <= 8192, "texture dimensions exceed budget")
        decoded_bytes += width * height * 4
        require(decoded_bytes <= MAX_PACK, "decoded texture budget exceeded")
        files.add(path)
    for key in ("Physics", "Pose", "DisplayInfo", "UserData"):
        if key in refs:
            path = safe_reference(root, base, refs[key], ".json")
            read_json(path)
            files.add(path)
    expressions = refs.get("Expressions", [])
    require(isinstance(expressions, list), "invalid Expressions")
    for expression in expressions:
        require(isinstance(expression, dict), "invalid expression reference")
        files.add(safe_reference(root, base, expression.get("File"), ".json"))
    motions = refs.get("Motions", {})
    require(isinstance(motions, dict), "invalid Motions")
    for group in motions.values():
        require(isinstance(group, list), "invalid motion group")
        for motion in group:
            require(isinstance(motion, dict) and "Sound" not in motion, "audio motions are unsupported")
            files.add(safe_reference(root, base, motion.get("File"), ".json"))
    for action in manifest["actions"].values():
        if action is not None:
            require(isinstance(action, dict), "invalid action")
            files.add(safe_reference(root, root, action.get("expression"), ".json"))
    profile = manifest.get("expression_profile")
    if profile is not None:
        require(isinstance(profile, dict) and isinstance(profile.get("catalog"), dict), "invalid expression profile")
        for expression in profile["catalog"].values():
            require(isinstance(expression, dict), "invalid catalog expression")
            files.add(safe_reference(root, root, expression.get("path"), ".json"))
    require(len(files) <= 1024 and sum(path.stat().st_size for path in files) <= MAX_PACK, "avatar pack budget exceeded")
    for path in files:
        if path.suffix == ".json":
            read_json(path)
    return root, sorted(files), manifest


def output_paths(output):
    require(not output.is_symlink(), "output symlink rejected")
    output = output.resolve()
    archive = output.with_name(output.name + ".tar.gz")
    checksum = archive.with_name(archive.name + ".sha256")
    for path in (output, archive, checksum):
        require(not path.exists() and not path.is_symlink(), "output and archive must be new paths")
        if path.is_relative_to(ROOT):
            require(subprocess.run(git_command("check-ignore", "-q", "--", str(path)), cwd=ROOT).returncode == 0,
                    "private test outputs inside the checkout must be ignored by Git")
    return output, archive, checksum


def runtime_files(directory):
    if directory is None:
        return []
    require(not directory.is_symlink(), "runtime library directory symlink rejected")
    files = []
    for name, notice in RUNTIME_LIBRARIES.items():
        source = directory / name
        license_path = directory / "licenses" / notice
        verify_elf(source)
        require(license_path.is_file() and not license_path.is_symlink()
                and not license_path.parent.is_symlink()
                and license_path.stat().st_size > 0, f"runtime library license missing: {notice}")
        files.extend([(source, Path("lib") / name), (license_path, Path("lib/licenses") / notice)])
    return files


def copy_file(source, destination, executable=False):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    destination.chmod(0o755 if executable else 0o644)


README = """DesktopPet AX650 私有端侧开发包

目标：Ubuntu 22.04 ARM64 / 当前 XFCE X11 / CPU llvmpipe。
默认 OpenGL 软件渲染，320×384（scale 64），目标调度 20 fps。
运行 ./run.sh；或 ./run.sh --scale 64 --fps 20 --seconds 120。
先检查依赖：./run.sh --check-only（不连接桌面、不会启动角色）。
--backend vulkan 是诊断入口，此板 Vulkan 空白原因尚未定位。
--model、--data-dir、--log-dir 可设置独立角色、存档和本次日志目录。
从 SSH 启动默认 DISPLAY=:0；它是 X11 会话编号，不是 HDMI 接口编号。
沿用用户 XAUTHORITY；缺省时仅引用已有、可读的 LXDM :0 凭证，不复制凭证。
Ctrl+C/quit 由 Rust 主进程执行正常关闭；详细终端命令见 --help。

本包包含 Rust 端侧主进程与独立角色宿主，不含 Tauri/WebKit/托盘。
语音模型、NPU 推理、联网和其他应用窗口吸附不属于本阶段。
data/ 与 logs/ 保留本包的独立存档和运行记录，移动包时可一并保留。
日志和 CPU 提交/present 统计不能替代用户目视的角色/透明/交互验收。
稳定 30 fps 不作为验收门槛。

角色资源仅供用户私有验证，版权和再分发权利未核实，不得公开上传。
可选 lib/ 只含 Ubuntu XKB 运行库及其许可证；GL/EGL/Mesa 驱动由板上提供。
普通启动也会先做动态加载预检，结果保存在 runtime-deps.json。
检查通过仅代表运行库可加载，不能证明桌面授权、合成器或渲染成功。
本包不安装软件、不改桌面服务，也不包含 VNC/Remote Desktop 测试工具。
"""


def package(binaries, avatar, runtime_libs, output):
    output, archive, archive_checksum = output_paths(output)
    binary_info = {name: verify_elf(binaries / name) for name in BINARIES}
    avatar_root, assets, manifest = avatar_files(avatar)
    libraries = runtime_files(runtime_libs)
    try:
        output.mkdir(parents=True)
        for name in BINARIES:
            copy_file(binaries / name, output / name, executable=True)
        for source in assets:
            copy_file(source, output / "resources/avatar" / source.relative_to(avatar_root))
        for source, relative in libraries:
            copy_file(source, output / relative)
        for name in ("run.sh", "launch.py", "runtime-deps.py"):
            copy_file(TOOLS / name, output / name, executable=True)
        (output / "README.txt").write_text(README, encoding="utf-8")
        info = {
            "schema_version": 1,
            "scope": "Private Ubuntu 22.04 AArch64 AX650 X11 software-rendering development package",
            **source_metadata(),
            "binaries": binary_info, "default_backend": "gl",
            "avatar_license": {key: manifest["license"].get(key) for key in ("status", "redistributable")},
            "runtime_libraries": list(RUNTIME_LIBRARIES) if libraries else [],
            "files": {path.relative_to(output).as_posix(): digest(path)
                      for path in sorted(output.rglob("*")) if path.is_file()},
        }
        (output / "package-manifest.json").write_text(json.dumps(info, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        sums = [f"{digest(path)}  {path.relative_to(output).as_posix()}\n"
                for path in sorted(output.rglob("*")) if path.is_file()]
        (output / "SHA256SUMS.txt").write_text("".join(sums), encoding="utf-8")
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(output, arcname=output.name)
        archive_checksum.write_text(f"{digest(archive)}  {archive.name}\n", encoding="utf-8")
    except Exception:
        shutil.rmtree(output, ignore_errors=True)
        archive.unlink(missing_ok=True)
        archive_checksum.unlink(missing_ok=True)
        raise
    return output, archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binaries", type=Path)
    parser.add_argument("--avatar", type=Path, help="prepared private manifest.json")
    parser.add_argument("--runtime-libs", type=Path,
                        help="directory with the two whitelisted regular XKB .so files and licenses/")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--verify-binaries", type=Path, help="ELF/GLIBC checks only; no package or execution")
    args = parser.parse_args()
    try:
        if args.verify_binaries:
            require(not any((args.binaries, args.avatar, args.runtime_libs, args.output)), "verification mode cannot create a package")
            print(json.dumps({name: verify_elf(args.verify_binaries / name) for name in BINARIES}, indent=2))
        else:
            require(all((args.binaries, args.avatar, args.output)), "--binaries, --avatar and --output are required")
            for path in package(args.binaries, args.avatar, args.runtime_libs, args.output):
                print(path)
    except (PackageError, OSError, UnicodeError, json.JSONDecodeError, struct.error, TypeError,
            subprocess.CalledProcessError) as error:
        parser.exit(1, f"AX650 packaging failed: {error}\n")


if __name__ == "__main__":
    main()
