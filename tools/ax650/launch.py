#!/usr/bin/env python3
"""Prepare a private AX650 package, preflight libraries, then replace with Rust."""
import argparse
from datetime import datetime
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent


def arguments(argv):
    parser = argparse.ArgumentParser(description="DesktopPet AX650 端侧启动器")
    parser.add_argument("--backend", choices=("gl", "vulkan"), default="gl")
    parser.add_argument("--cli", action="store_true", help="启动可选文字诊断程序，默认启动完整桌宠")
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
    if args.seconds is not None and not args.cli:
        parser.error("--seconds 仅适用于 --cli；完整桌宠通过托盘退出")
    return args


def display_session(display):
    match = re.fullmatch(r":(\d+)(?:\.\d+)?", display)
    return match.group(1) if match else display


def desktop_environment(display, proc=Path("/proc"), uid=None):
    """Read only session variables from the same user's matching XFCE desktop."""
    uid = os.geteuid() if uid is None else uid
    preferred = {"xfce4-session": 0, "xfce4-panel": 1, "xfwm4": 2}
    candidates = []
    for process in proc.glob("[0-9]*"):
        try:
            if process.stat().st_uid != uid:
                continue
            name = (process / "comm").read_text().strip()
            if name not in preferred:
                continue
            values = dict(item.split("=", 1) for item in
                          (process / "environ").read_bytes().decode("utf-8", errors="replace").split("\0")
                          if "=" in item)
            if display_session(values.get("DISPLAY", "")) != display_session(display):
                continue
            if values.get("DBUS_SESSION_BUS_ADDRESS"):
                candidates.append((preferred[name], int(process.name), {
                    key: values[key] for key in ("DBUS_SESSION_BUS_ADDRESS", "XAUTHORITY", "XDG_RUNTIME_DIR")
                    if values.get(key)
                }))
        except (OSError, ValueError):
            continue
    return min(candidates, default=(None, None, {}))[-1]


def environment(root, backend, inherited, full_app=True):
    env = inherited.copy()
    env["LD_LIBRARY_PATH"] = str(root / "lib") + (
        ":" + env["LD_LIBRARY_PATH"] if env.get("LD_LIBRARY_PATH") else "")
    env.setdefault("DISPLAY", ":0")
    if full_app and not env.get("DBUS_SESSION_BUS_ADDRESS"):
        for key, value in desktop_environment(env["DISPLAY"]).items():
            if not env.get(key):
                env[key] = value
    if not env.get("XAUTHORITY"):
        auth = Path("/var/run/lxdm/lxdm-:0.auth")
        if auth.is_file() and os.access(auth, os.R_OK):
            env["XAUTHORITY"] = str(auth)
    # Software rendering is the AX650 baseline. Rust selects its backend and
    # enables the guarded GL-alpha path; this launcher does not broaden it.
    env["LIBGL_ALWAYS_SOFTWARE"] = "1"
    env["WGPU_BACKEND"] = backend
    env.pop("DESKTOPPET_X11_OPAQUE_ALPHA", None)
    if backend == "gl":
        env["DESKTOPPET_X11_GL_ALPHA"] = "1"
    else:
        env.pop("DESKTOPPET_X11_GL_ALPHA", None)
    if full_app:
        env["GDK_BACKEND"] = "x11"
        # WebKit's documented GTK switch disables accelerated compositing;
        # it does not disable the WebKit sandbox.
        env["WEBKIT_DISABLE_COMPOSITING_MODE"] = "1"
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
    env = environment(ROOT, args.backend, os.environ, full_app=not args.cli)
    try:
        preflight = [sys.executable, str(ROOT / "runtime-deps.py"), "--backend", args.backend]
        if args.cli:
            preflight.append("--cli")
        checked = subprocess.run(
            preflight,
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
    if not args.cli:
        if not env.get("DBUS_SESSION_BUS_ADDRESS"):
            error = "未找到当前 XFCE 会话总线；从桌面终端运行，或传入该会话的 DBUS_SESSION_BUS_ADDRESS"
            (run_dir / "result.json").write_text(json.dumps({
                "completed": False, "host_started": False, "error": error, "scope": "desktop session lookup",
            }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            print(error, file=sys.stderr)
            return 1
        runtime = ROOT / ".runtime"
        if not env.get("XDG_RUNTIME_DIR"):
            runtime.mkdir(mode=0o700, exist_ok=True)
            runtime.chmod(0o700)
            env["XDG_RUNTIME_DIR"] = str(runtime)
        env["DESKTOPPET_RESOURCE_DIR"] = str(ROOT / "resources")
        env["DESKTOPPET_MODEL"] = str(model)
        env["DESKTOPPET_DATA_DIR"] = str(args.data_dir.resolve())
        env["DESKTOPPET_LOG_DIR"] = str(run_dir)
        env["DESKTOPPET_INITIAL_SCALE"] = str(args.scale or 64)
        env["DESKTOPPET_MAX_FPS"] = str(args.fps or 20)
        env["DESKTOPPET_DEVICE_PROFILE"] = "ax650"
        (run_dir / "launcher.json").write_text(json.dumps({
            "application": "desktop-pet", "backend": args.backend, "display": env["DISPLAY"],
            "session_bus_found": True, "software_rendering": True, "webkit_accelerated_compositing": False,
            "scope": "Launch configuration; no desktop, tray, audio or rendering validation",
        }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        os.chdir(ROOT)
        stderr = (run_dir / "desktop.stderr.log").open("a", encoding="utf-8")
        os.dup2(stderr.fileno(), sys.stderr.fileno())
        stderr.close()
        executable = str(ROOT / "desktop-pet")
        return os.execve(executable, [executable], env)
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
