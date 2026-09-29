# Linux 构建准备与移植状态

当前仓库**尚未提供可用的 Linux 版本**。现有 GitHub Actions 只构建 macOS `.app`／DMG；没有 Linux 安装包、发布工作流或实机验收记录。下方 Cargo 命令只是编译探针：生成两个可执行文件不代表透明桌宠已经可交互。现行发行流程见 [macOS 构建说明](BUILDING.zh-CN.md)。

## 目标环境与构建前提

首轮建议固定一个发行版、`x86_64` 架构和 **X11 桌面会话**，以 CPU 语音及可分发演示角色为基线。准备可交互的 Linux 桌面机；虚拟机可用于编译排障，但透明合成、窗口层级、点击穿透、多屏和音频仍需在目标桌面环境验收。记录发行版、桌面环境、窗口管理器／合成器、`$XDG_SESSION_TYPE`、GPU／驱动和显示缩放。Wayland 是单独的支持目标，不能用 XWayland 的结果代替原生 Wayland 验收。

在 Debian／Ubuntu 构建机上安装 [Tauri 2 的 Linux 系统依赖](https://v2.tauri.app/start/prerequisites/)以及 [CPAL 所需的 ALSA 开发文件](https://github.com/RustAudio/cpal/blob/master/README.md?plain=1)：

```sh
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
  libasound2-dev pkg-config
```

其他发行版应按上述官方说明换用对应包名，并补齐 ALSA 开发包。另需 Git 和 [rustup](https://www.rust-lang.org/tools/install)，使用仓库 [`rust-toolchain.toml`](rust-toolchain.toml) 锁定的 Rust **1.95.0**；依赖版本以 `Cargo.lock` 为准。前端是仓库内静态 HTML/CSS/JavaScript，运行下方 Cargo 编译探针无需 Node.js。仅在本机准备或核验资源 ZIP 时需要 Python 3。

在 Linux 上从仓库根目录尝试编译：

```sh
rustup toolchain install 1.95.0 --profile minimal
rustc +1.95.0 -vV
printf 'session=%s\n' "$XDG_SESSION_TYPE"
cargo +1.95.0 build --locked --release -p desktop-pet -p avatar-host-2d
```

如成功，输出应为 `target/release/desktop-pet` 和 `target/release/avatar-host-2d`。目前没有 Linux 构建通过的记录；Tauri、Mocari/wgpu、CPAL 和 sherpa-onnx 静态库的锁定版本组合仍须在目标机器上验证。编译探针无需角色或语音模型；运行验证则需要角色包。不要用 `tools/release/package_macos.py` 制作 Linux 包。

## 当前阻碍与建议顺序

| 阶段 | 当前代码状态与完成条件 |
| --- | --- |
| 编译探针 | 用锁定依赖编译两个程序，记录并解决 Linux 原生依赖、链接与目标架构错误。 |
| 基础角色窗口 | `apps/avatar-host-2d/src/platform.rs` 的非 macOS 指针、桌面工作区、窗口移动和屏幕吸附仍为空实现或报错。`window.rs` 初始设置点击穿透，却只在 macOS 启用动态命中恢复。先验证透明合成和焦点，再实现 Linux 输入区域、点击／拖拽、位置恢复及屏幕吸附，并实测下层应用是否收到透明区点击。 |
| X11 他应用吸附 | `apps/avatar-host-2d/src/ax.rs` 的非 macOS 窗口观察器是空实现。可按 [跨平台设计](docs/03-desktop-platform.md)增加 X11／EWMH 的目标窗口观察和过滤；缺少该能力时基础桌宠仍应可用。 |
| Wayland 基础模式 | 普通客户端无法假定拥有全局指针、任意窗口定位或其他应用窗口几何。需要按 compositor 实测透明窗口、输入区域、置顶与托盘，并为不可用的吸附／占屏功能明确降级；可选扩展需单独列出支持环境。[Wayland 协议模型](https://wayland.freedesktop.org/docs/book/Protocol.html) |
| 语音、资源和发行 | 在 Linux 验证 CPAL 麦克风／扬声器、sherpa-onnx CPU 模型加载及外部 LLM 服务。`apps/desktop/src/main.rs` 当前按 macOS `.app/Contents/Resources` 寻找辅助进程和资源；`apps/desktop/tauri.conf.json` 关闭了打包。需重新定义 Linux 安装目录、资源相对路径与辅助进程布局，再制作并验证安装包。 |

建议按“编译 → 可见角色与输入 → X11 屏幕吸附及存档 → CPU 语音 → 可选他应用吸附 → Linux 包 → Wayland 基础模式”推进。项目当前并未针对 Linux 实现或验收上述功能。

## 资源、打包与验收

- 角色包、互动语音和模型权重不在 Git 中。资源 ZIP 格式见 [macOS 资源清单](BUILDING.zh-CN.md#资源包)；Linux 打包器需保留校验和安全解包逻辑，不能复用 `.app` 目录布局。先用有明确再分发权限的演示资产，参见 [资源来源](docs/ASSETS.md)和[使用及版权说明](POLICY.md)。无语音基线只需有效角色包；完整语音还需 VAD、ASR、TTS／KWS 模型及参考音频，LLM 服务及模型由用户自行提供。
- Linux 安装格式可在验证后选择 [deb、RPM 或 AppImage](https://v2.tauri.app/distribute/)；目前没有可直接运行的 Linux 打包命令。制作 AppImage 时需选定最低支持发行版并在相应基线构建，避免新系统的 glibc 依赖使旧系统无法运行。[Tauri AppImage 指南](https://v2.tauri.app/distribute/appimage/)
- 验收至少覆盖：深浅背景透明效果、点击下层应用与角色点击／拖拽、焦点和退出、单屏／混合 DPI 双屏、显示器拔插、睡眠恢复、宿主崩溃、托盘和存档、麦克风／扬声器，以及安装后脱离源码目录启动。X11 与 Wayland 分开记录实际支持能力。

[路线图 P7](docs/05-roadmap.md)的 **10–20 人日起**是早期跨平台阶段估算，不表示 Linux 已可构建。按当前代码，单名熟悉 Rust 与 Linux 桌面 API 的开发者可暂按 **2–4 人日**做首次编译及窗口技术探针，**合计 8–15 人日**做 X11 可用开发版，**合计 15–25 人日**做较完整的 X11 功能、打包和验收；Wayland 基础模式在此基础上**另加约 5–10 人日**。这些是规划量级，窗口管理器差异及原生依赖问题可能改变结果。
