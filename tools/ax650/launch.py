#!/usr/bin/env python3
"""Prepare a private AX650 package, preflight libraries, then replace with Rust."""
import argparse
from datetime import datetime
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent


def arguments(argv):
    parser = argparse.ArgumentParser(description="DesktopPet AX650 端侧启动器")
    parser.add_argument("--backend", choices=("gl", "vulkan"), default="gl")
    parser.add_argument("--model", type=Path, default=ROOT / "resources/avatar/manifest.json")
    parser.add_argument("--data-dir", type=Path, default=ROOT / "data")
    parser.add_argument("--log-dir", type=Path, help="本次日志目录；默认在包内 logs/ 创建新目录")
    parser.add_argument("--scale", type=int)
    parser.add_argument("--fps", type=int)
    parser.add_argument("--seconds", type=int)
    parser.add_argument("--check-only", action="store_true", help="只加载运行库，不连接桌面或启动角色")
    args = parser.parse_args(argv)
    if args.scale is not None and not 50 <= args.scale <= 150:
        parser.error("--scale 必须为 50–150")
    if args.fps is not None and not 1 <= args.fps <= 30:
        parser.error("--fps 必须为 1–30")
    if args.seconds is not None and args.seconds < 0:
        parser.error("--seconds 必须非负，0 表示持续运行")
    return args


def environment(root, backend, inherited):
    env = inherited.copy()
    env["LD_LIBRARY_PATH"] = str(root / "lib") + (
        ":" + env["LD_LIBRARY_PATH"] if env.get("LD_LIBRARY_PATH") else "")
    env.setdefault("DISPLAY", ":0")
    if not env.get("XAUTHORITY"):
        auth = Path("/var/run/lxdm/lxdm-:0.auth")
        if auth.is_file() and os.access(auth, os.R_OK):
            env["XAUTHORITY"] = str(auth)
    # Software rendering is the AX650 baseline. Rust selects its backend and
    # enables the guarded GL-alpha path; this launcher does not broaden it.
    env["LIBGL_ALWAYS_SOFTWARE"] = "1"
    env["WGPU_BACKEND"] = backend
    if backend == "vulkan" and not env.get("VK_ICD_FILENAMES") and not env.get("VK_DRIVER_FILES"):
        lavapipe = Path("/usr/share/vulkan/icd.d/lvp_icd.aarch64.json")
        if lavapipe.is_file():
            env["VK_ICD_FILENAMES"] = str(lavapipe)
    return env


def main(argv=None):
    args = arguments(argv)
    run_dir = (args.log_dir or ROOT / "logs" /
               (datetime.now().strftime("%Y%m%d-%H%M%S") + "-" + str(os.getpid()))).resolve()
    run_dir.mkdir(parents=True, exist_ok=args.log_dir is not None)
    env = environment(ROOT, args.backend, os.environ)
    try:
        checked = subprocess.run(
            [sys.executable, str(ROOT / "runtime-deps.py"), "--backend", args.backend],
            env=env, capture_output=True, text=True, timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        (run_dir / "result.json").write_text(json.dumps({
            "completed": False, "host_started": False, "error": str(error),
            "scope": "launcher preflight",
        }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        raise
    (run_dir / "runtime-deps.json").write_text(checked.stdout, encoding="utf-8")
    if checked.stderr:
        (run_dir / "runtime-deps.stderr.log").write_text(checked.stderr, encoding="utf-8")
    print("日志目录：", run_dir, flush=True)
    if checked.returncode:
        (run_dir / "result.json").write_text(json.dumps({
            "completed": False, "host_started": False, "error": "runtime dependency check failed",
            "scope": "launcher preflight", "dependency_exit_code": checked.returncode,
        }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print("运行依赖检查失败：\n" + checked.stdout + checked.stderr, file=sys.stderr, flush=True)
        return 1
    if args.check_only:
        (run_dir / "result.json").write_text(json.dumps({
            "completed": True, "host_started": False, "scope": "library check only",
        }, indent=2) + "\n", encoding="utf-8")
        print("动态运行依赖检查通过；未连接桌面、未启动角色。", flush=True)
        return 0
    model = args.model.resolve(strict=True)
    args.data_dir.mkdir(parents=True, exist_ok=True)
    command = [str(ROOT / "desktop-pet-ax650"), "--model", str(model),
               "--data-dir", str(args.data_dir.resolve()), "--log-dir", str(run_dir),
               "--backend", args.backend]
    for option in ("scale", "fps", "seconds"):
        value = getattr(args, option)
        if value is not None:
            command.extend(["--" + option, str(value)])
    os.execve(command[0], command, env)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, subprocess.TimeoutExpired) as error:
        print("启动准备失败：", error, file=sys.stderr)
        sys.exit(1)
