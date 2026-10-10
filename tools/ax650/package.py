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
BINARIES = ("desktop-pet", "avatar-host-2d")
CLI_BINARY = "desktop-pet-ax650"
ASR_DIRECTORY = "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"
TTS_DIRECTORY = "sherpa-onnx-zipvoice-distill-int8-zh-en-emilia"
KWS_DIRECTORY = "sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20"
MODEL_FILES = (
    "silero_vad.onnx", f"{ASR_DIRECTORY}/model.int8.onnx", f"{ASR_DIRECTORY}/tokens.txt",
    "vocos_24khz.onnx", *(f"{TTS_DIRECTORY}/{name}" for name in
                           ("encoder.int8.onnx", "decoder.int8.onnx", "tokens.txt", "lexicon.txt")),
    *(f"{KWS_DIRECTORY}/{name}" for name in (
        "encoder-epoch-13-avg-2-chunk-16-left-64.onnx", "decoder-epoch-13-avg-2-chunk-16-left-64.onnx",
        "joiner-epoch-13-avg-2-chunk-16-left-64.onnx", "tokens.txt", "en.phone")),
)
NOTICE_FILES = tuple(f"{category}.zip.sources.md" for category in
                     ("avatar", "voice", "models-vad", "models-asr", "models-tts", "models-kws"))
RUNTIME_LIBRARIES = {
    "libxkbcommon-x11.so.0": "libxkbcommon-x11-0.copyright",
    "libxcb-xkb.so.1": "libxcb-xkb1.copyright",
    "libxdo.so.3": "libxdo3.copyright",
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


def private_resource_file(root, relative, max_bytes=512 * 1024 * 1024):
    require(not root.is_symlink(), "resource directory symlink rejected")
    path = root / relative
    require(path.is_relative_to(root) and ".." not in relative.parts, "escaping resource reference rejected")
    for current in (path, *path.parents):
        if current == root:
            break
        require(not current.is_symlink(), "resource symlinks are unsupported")
    require(path.is_file() and stat.S_ISREG(path.stat().st_mode), f"missing resource: {relative}")
    require(0 < path.stat().st_size <= max_bytes, f"empty or oversized resource: {relative}")
    require(not path.name.startswith((".env", "id_rsa", "id_ed25519"))
            and path.suffix not in (".pem", ".pfx", ".sqlite3", ".sqlite", ".key"), "private config/credential resource rejected")
    return path


def model_files(directory, full_voice_models=False):
    require(directory is not None, "full application requires --models with CPU VAD/ASR resources")
    selected = MODEL_FILES if full_voice_models else MODEL_FILES[:3]
    files = [(private_resource_file(directory, Path(name)), Path("resources/models") / name)
             for name in selected]
    for subdirectory in (ASR_DIRECTORY, TTS_DIRECTORY, KWS_DIRECTORY):
        for name in ("README.md", "LICENSE", "LICENSE.txt", "COPYING", "NOTICE"):
            relative = Path(subdirectory) / name
            if (directory / relative).exists():
                files.append((private_resource_file(directory, relative, MAX_JSON), Path("resources/models") / relative))
    if full_voice_models:
        espeak = directory / TTS_DIRECTORY / "espeak-ng-data"
        require(espeak.is_dir() and not espeak.is_symlink(), "TTS requires regular espeak-ng-data directory")
        data_files = sorted(espeak.rglob("*"))
        require(data_files and not any(path.is_symlink() for path in data_files), "empty or symlink espeak data rejected")
        for path in data_files:
            if path.is_file():
                relative = path.relative_to(directory)
                files.append((private_resource_file(directory, relative, MAX_FILE), Path("resources/models") / relative))
    require(len(files) <= 2048 and sum(source.stat().st_size for source, _ in files) <= 1024 * 1024 * 1024,
            "model resource budget exceeded")
    return files


def voice_files(directory):
    if directory is None:
        return []
    require(not directory.is_symlink(), "voice directory symlink rejected")
    files = []
    for language in ("zh-CN", "en-US", "ja-JP", "ko-KR"):
        for category in ("greetings", "care", "intimacy"):
            base = directory / language / category
            if not base.is_dir():
                continue
            require(not base.is_symlink(), "voice directory symlink rejected")
            for path in sorted(base.rglob("*")):
                require(not path.is_symlink(), "voice symlink rejected")
                if not path.is_file() or path.suffix not in (".wav", ".txt"):
                    continue
                relative = path.relative_to(directory)
                source = private_resource_file(directory, relative, MAX_FILE)
                if path.suffix == ".wav":
                    with source.open("rb") as stream:
                        header = stream.read(12)
                    require(header[:4] == b"RIFF" and header[8:12] == b"WAVE", "invalid WAV header")
                files.append((source, Path("resources/voice") / relative))
    require(files and len(files) <= 1024 and sum(source.stat().st_size for source, _ in files) <= 256 * 1024 * 1024,
            "voice resource budget exceeded or no supported localized audio")
    private_resource_file(directory, Path("zh-CN/greetings/noon.wav"), MAX_FILE)
    private_resource_file(directory, Path("zh-CN/greetings/noon.txt"), MAX_JSON)
    return files


def notice_files(directory):
    files = [(ROOT / "docs/ASSETS.md", Path("resources/notices/resource-sources.md"))]
    if directory is not None:
        files.extend((private_resource_file(directory, Path(name), MAX_JSON), Path("resources/notices") / name)
                     for name in NOTICE_FILES)
    return files


def copy_file(source, destination, executable=False):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    destination.chmod(0o755 if executable else 0o644)


README = """DesktopPet AX650 私有完整桌宠开发包

目标：Ubuntu 22.04 ARM64 / 当前 XFCE X11 / CPU llvmpipe。
默认 OpenGL 软件渲染，320×384（scale 64），目标调度 20 fps。
运行 ./run.sh；或 ./run.sh --scale 64 --fps 20。
包含 Tauri 设置与托盘、角色交互、养成存档与 CPU 语音模型。
首次打开设置；关闭设置后通过真实 XFCE 托盘重开或退出。
先检查依赖：./run.sh --check-only（不连接桌面、不会启动角色）。
--backend vulkan 是诊断入口，此板 Vulkan 空白原因尚未定位。
--model、--data-dir、--log-dir 可设置独立角色、存档和本次日志目录。
从 SSH 启动默认 DISPLAY=:0；它是 X11 会话编号，不是 HDMI 接口编号。
沿用用户 XAUTHORITY；缺省时仅引用已有、可读的 LXDM :0 凭证，不复制凭证。
从 SSH 启动需已有正确的 XFCE 会话总线；启动器可从同用户、同 DISPLAY
的 xfce4-session/panel/xfwm4 环境读取总线，找不到则退出，不创建新总线。
可选 --cli 启动文字诊断程序；--seconds 仅在 --cli 模式可用。

语音、KWS、问候与联网默认关闭。麦克风接入后在设置中启用并测试。
ASR/VAD/TTS/KWS 使用 CPU，外部 LLM 服务由用户配置；NPU 尚未接入。
软件 WebKit 设置关闭加速合成，保留 WebKit sandbox。
data/ 与 logs/ 保留本包的独立存档和运行记录，移动包时可一并保留。
日志和 CPU 提交/present 统计不能替代用户目视的角色/透明/交互验收。
稳定 30 fps 不作为验收门槛。

角色资源仅供用户私有验证，版权和再分发权利未核实，不得公开上传。
可选 lib/ 只含 Ubuntu XKB / xdo 运行库及其许可证；GL/EGL/Mesa 驱动由板上提供。
普通启动也会先做动态加载预检，结果保存在 runtime-deps.json。
检查通过仅代表运行库可加载，不能证明桌面授权、合成器或渲染成功。
本包不安装软件、不改桌面服务，也不包含 VNC/Remote Desktop 测试工具。
"""


def package(binaries, avatar, runtime_libs, output, models=None, voice=None, notices=None, include_cli=False,
            full_voice_models=False):
    output, archive, archive_checksum = output_paths(output)
    names = BINARIES + ((CLI_BINARY,) if include_cli else ())
    binary_info = {name: verify_elf(binaries / name) for name in names}
    avatar_root, assets, manifest = avatar_files(avatar)
    libraries = runtime_files(runtime_libs)
    extra_resources = model_files(models, full_voice_models) + voice_files(voice) + notice_files(notices)
    try:
        output.mkdir(parents=True)
        for name in names:
            copy_file(binaries / name, output / name, executable=True)
        for source in assets:
            copy_file(source, output / "resources/avatar" / source.relative_to(avatar_root))
        for source, relative in libraries:
            copy_file(source, output / relative)
        for source, relative in extra_resources:
            copy_file(source, output / relative)
        resources = output / "resources"
        (resources / "model-path.txt").write_text("avatar/manifest.json\n", encoding="utf-8")
        (resources / "show-settings-on-launch").touch()
        (resources / "dev-data-dir.txt").write_text("data\n", encoding="utf-8")
        settings = dict(
            enabled=False, kws_enabled=False, wake_greeting_enabled=False, timed_greetings_enabled=False,
            first_greeting_enabled=False, web_enabled=False, asr_backend="sherpa_onnx",
            model_dir="@desktop-pet-resources/models", mode="transcription", input_device_name="",
            asr_threads=1, asr_gpu=False,
            tts_threads=2, tts_gpu=False, kws_threads=1, output_volume_percent=100,
        )
        if voice is not None:
            settings.update(
                greeting_dir="@desktop-pet-resources/voice",
                reference_audio="@desktop-pet-resources/voice/zh-CN/greetings/noon.wav",
                reference_text=(voice / "zh-CN/greetings/noon.txt").read_text(encoding="utf-8").strip(),
            )
        (resources / "voice-settings.json").write_text(json.dumps(settings, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        copy_file(ROOT / "apps/desktop/icons/icon.png", resources / "icon.png")
        for name in ("run.sh", "launch.py", "runtime-deps.py"):
            copy_file(TOOLS / name, output / name, executable=True)
        (output / "README.txt").write_text(README, encoding="utf-8")
        info = {
            "schema_version": 1,
            "scope": "Private Ubuntu 22.04 AArch64 AX650 complete Tauri desktop pet with CPU voice resources",
            **source_metadata(),
            "binaries": binary_info, "default_backend": "gl",
            "entrypoint": "desktop-pet", "diagnostic_cli": include_cli,
            "voice_models": ["vad", "asr", "tts", "kws"] if full_voice_models else ["vad", "asr"],
            "localized_voice": voice is not None,
            "avatar_license": {key: manifest["license"].get(key) for key in ("status", "redistributable")},
            "runtime_libraries": [target.name for _, target in libraries if target.parent == Path("lib")],
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
    parser.add_argument("--models", type=Path, help="explicit CPU VAD/ASR/TTS/KWS model root")
    parser.add_argument("--voice", type=Path, help="optional localized WAV/TXT root with Chinese noon reference")
    parser.add_argument("--notices", type=Path, help="optional original resource provenance notices")
    parser.add_argument("--include-cli", action="store_true", help="also include the renderer diagnostic CLI")
    parser.add_argument("--full-voice-models", action="store_true", help="include CPU TTS/KWS/espeak in addition to VAD/ASR")
    parser.add_argument("--verify-binaries", type=Path, help="ELF/GLIBC checks only; no package or execution")
    args = parser.parse_args()
    try:
        if args.verify_binaries:
            require(not any((args.binaries, args.avatar, args.runtime_libs, args.output, args.models, args.voice, args.notices)),
                    "verification mode cannot create a package")
            names = BINARIES + ((CLI_BINARY,) if args.include_cli else ())
            print(json.dumps({name: verify_elf(args.verify_binaries / name) for name in names}, indent=2))
        else:
            require(all((args.binaries, args.avatar, args.output)), "--binaries, --avatar and --output are required")
            for path in package(args.binaries, args.avatar, args.runtime_libs, args.output,
                                args.models, args.voice, args.notices, args.include_cli, args.full_voice_models):
                print(path)
    except (PackageError, OSError, UnicodeError, json.JSONDecodeError, struct.error, TypeError,
            subprocess.CalledProcessError) as error:
        parser.exit(1, f"AX650 packaging failed: {error}\n")


if __name__ == "__main__":
    main()
