# Linux 构建准备与移植状态

当前仓库提供 **Linux X11 实验开发基线**，已加入手动 AppImage 构建与预发布工作流。2026-10-01 已在 Ubuntu 22.04 / TigerVNC / xfwm4 / RTX 5090 上完成首次构建及原生角色透明、输入、屏幕吸附与存档测试；范围及离线构建步骤见 [第一阶段开发记录](docs/25-linux-x11-implementation.md)。GitHub Actions 可手动构建 Linux x86_64 AppImage；打包检查与 Xvfb 启动检查通过后上传 Linux 预发布。生成两个可执行文件不代表其他 Linux 桌面或完整应用功能已经通过验收；现行发行流程见 [macOS 构建说明](BUILDING.zh-CN.md)。

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

输出为 `target/release/desktop-pet` 和 `target/release/avatar-host-2d`。Tauri、Mocari/wgpu、CPAL 和 sherpa-onnx 静态库的锁定版本组合已在上述 Ubuntu 22.04 环境构建通过；其他目标机器仍须验证。编译无需角色或语音模型；运行验证则需要角色包。不要用 `tools/release/package_macos.py` 制作 Linux 包。

## 当前阻碍与建议顺序

| 阶段 | 当前代码状态与完成条件 |
| --- | --- |
| 编译探针 | 用锁定依赖编译两个程序，记录并解决 Linux 原生依赖、链接与目标架构错误。 |
| 基础角色窗口 | 已增加 X11 指针、EWMH 工作区、位置移动和屏幕吸附，并在上述单屏环境实测透明合成、焦点、点击穿透、点击／拖拽和位置恢复。NVIDIA Vulkan 的 `Opaque` alpha 兼容需显式开启且通过视觉格式／合成器检查，见开发记录。多屏、混合 DPI 及其他窗口管理器待验收。 |
| X11 他应用吸附 | `apps/avatar-host-2d/src/ax.rs` 的非 macOS 窗口观察器是空实现。可按 [跨平台设计](docs/03-desktop-platform.md)增加 X11／EWMH 的目标窗口观察和过滤；缺少该能力时基础桌宠仍应可用。 |
| Wayland 基础模式 | 普通客户端无法假定拥有全局指针、任意窗口定位或其他应用窗口几何。需要按 compositor 实测透明窗口、输入区域、置顶与托盘，并为不可用的吸附／占屏功能明确降级；可选扩展需单独列出支持环境。[Wayland 协议模型](https://wayland.freedesktop.org/docs/book/Protocol.html) |
| 语音、资源和发行 | CPAL 麦克风／扬声器、sherpa-onnx CPU 模型加载及外部 LLM 服务待验证。Linux 开发目录已改为同目录辅助进程及 `resources/`，可用 `DESKTOPPET_RESOURCE_DIR` 覆盖；`apps/desktop/tauri.conf.json` 仍关闭打包。当前 XFCE 的真实托盘、关闭设置／重开和退出路径已验证；正式安装路径、凭证存储及通用安装包验收尚未完成。 |

按“编译 → 可见角色与输入 → X11 屏幕吸附及存档 → CPU 语音 → 可选他应用吸附 → Linux 包 → Wayland 基础模式”推进；前三项已建立单屏 X11 基线，后续及其他桌面环境分别验收。

## 资源、打包与验收

- 角色包、互动语音和模型权重不在 Git 中。资源 ZIP 格式见 [macOS 资源清单](BUILDING.zh-CN.md#资源包)；Linux 打包器需保留校验和安全解包逻辑，不能复用 `.app` 目录布局。先用有明确再分发权限的演示资产，参见 [资源来源](docs/ASSETS.md)和[使用及版权说明](POLICY.md)。无语音基线只需有效角色包；完整语音还需 VAD、ASR、TTS／KWS 模型及参考音频，LLM 服务及模型由用户自行提供。
- Linux 安装格式可在验证后选择 [deb、RPM 或 AppImage](https://v2.tauri.app/distribute/)；可用 `tools/linux/package-dev.py` 制作当前 X11 基线的私有交互测试目录和 tar.gz，见开发记录；尚不承诺所有 Linux 桌面的兼容性。制作 AppImage 时需选定最低支持发行版并在相应基线构建，避免新系统的 glibc 依赖使旧系统无法运行。[Tauri AppImage 指南](https://v2.tauri.app/distribute/appimage/)
- 验收至少覆盖：深浅背景透明效果、点击下层应用与角色点击／拖拽、焦点和退出、单屏／混合 DPI 双屏、显示器拔插、睡眠恢复、宿主崩溃、托盘和存档、麦克风／扬声器，以及安装后脱离源码目录启动。X11 与 Wayland 分开记录实际支持能力。

[路线图 P7](docs/05-roadmap.md)的 **10–20 人日起**是早期跨平台阶段估算，不表示 Linux 已可构建。按当前代码，单名熟悉 Rust 与 Linux 桌面 API 的开发者可暂按 **2–4 人日**做首次编译及窗口技术探针，**合计 8–15 人日**做 X11 可用开发版，**合计 15–25 人日**做较完整的 X11 功能、打包和验收；Wayland 基础模式在此基础上**另加约 5–10 人日**。这些是规划量级，窗口管理器差异及原生依赖问题可能改变结果。

## 手动 Actions / AppImage 预发布

工作流为 `.github/workflows/linux-manual.yml`，在默认 `master` 分支登记；`source_ref` 指定要构建的 Linux 分支、标签或提交，默认 `codex/linux-x11-baseline`。为可追溯发布，建议手动触发时填入完整提交 SHA。`publish_release` 默认开启，关闭时仅保留 Actions 构建产物。发布标签为 `linux-v0.1.0-alpha.<run_number>`，不会覆盖其他平台的 Release。

流程固定 Ubuntu 22.04 x86_64、Rust 1.95.0、Cargo.lock 和 Tauri CLI 2.11.4。sherpa-onnx 1.13.8 的原生静态库归档按 SHA-256 校验；不下载角色或语音模型。执行主程序、宿主和存档测试，以及公开包的隐私／架构检查。打包器包含两程序、GTK/WebKit 依赖与动态加载的 Vulkan loader，GPU 驱动及 ICD 由用户系统提供。

AppImage 资源只有启动设置窗口标记、默认关闭语音／KWS／问候／联网的配置及源码提交元数据；不含私有角色、模型、凭证或开发存档目录。辅助进程位于主程序同目录，资源位于同目录的 `resources/`，存档使用 Tauri 正常用户数据目录。解包验收会拒绝私有资源、开发存档标记、宿主 GPU 驱动、越界链接或高于 glibc 2.35 的程序符号要求。Xvfb 检查 FUSE-free 启动、设置窗口与用户存档；不代表真实 GPU、透明交互、音频或 Wayland 已通过。

下载 Release 的 `DesktopPet-linux-x86_64.AppImage` 和 `SHA256SUMS.txt` 后：

```sh
sha256sum --check SHA256SUMS.txt
chmod +x DesktopPet-linux-x86_64.AppImage
./DesktopPet-linux-x86_64.AppImage
```

首次启动打开设置，在角色库导入用户自己的准备好的 `manifest.json`。关闭设置后使用托盘重新打开或退出。机器需有 X11 合成器、StatusNotifier / AppIndicator 宿主及兼容的 Vulkan GPU 驱动。缺少 FUSE 时加 `--appimage-extract-and-run`；也可 `--appimage-extract` 后从解包目录运行 `./AppRun`。

对于已经实测的 NVIDIA Vulkan / XFCE X11 透明组合，显式使用：

```sh
GDK_BACKEND=x11 DESKTOPPET_X11_OPAQUE_ALPHA=1 ./DesktopPet-linux-x86_64.AppImage
```

原生 Wayland 尚未实现，XWayland 未实测。Ubuntu 26.04 的默认 GNOME 会话仅支持 Wayland，但仍可通过 XWayland 运行 X11 应用；其他桌面仍可使用 X11。不能据此声明当前桌宠在 GNOME Wayland 已完整兼容。[Ubuntu 官方说明](https://documentation.ubuntu.com/release-notes/26.04/summary-for-lts-users/)
