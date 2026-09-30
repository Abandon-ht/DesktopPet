# Windows 构建准备与移植状态

当前仓库**尚未提供可用的 Windows 版本**。现有 GitHub Actions 只在 macOS 构建 `.app`／DMG；Windows 没有安装包、发布工作流或实机验收记录。下面的命令仅用于在 Windows 上尝试编译两个程序，成功生成 `.exe` 不代表桌宠功能已可用。macOS 的现行构建步骤见 [macOS 构建说明](BUILDING.zh-CN.md)。

`windows` 分支已完成首次 Windows x64 Release 编译。环境、图标修复、资源准备和“未配置角色”的启动反馈见 [Windows 编译与开发交接](docs/windows-build-progress.md)；桌宠运行验收仍未完成。

## 先确定目标与环境

首轮建议以 Windows 10/11 x64、CPU 语音和可分发演示角色为基线。至少准备一台可交互的 Windows 实机或虚拟机；透明合成、点击穿透、焦点、混合 DPI、多显示器和音频设备仍需在目标硬件上验证。虚拟机的图形结果不能单独作为最终验收。

在 Windows 上安装：

1. [Microsoft C++ Build Tools](https://v2.tauri.app/start/prerequisites/) 的 **Desktop development with C++** 工作负载及 Windows SDK。
2. [Microsoft Edge WebView2 Runtime](https://v2.tauri.app/start/prerequisites/)。部分 Windows 安装已带有它，构建机仍应确认可用。
3. [rustup](https://www.rust-lang.org/tools/install) 和仓库锁定的 Rust **1.95.0**；安装 rustup 时选择 `x86_64-pc-windows-msvc` 作为默认主机工具链。版本见 [`rust-toolchain.toml`](rust-toolchain.toml)，依赖版本见 `Cargo.lock`。
4. Git。只有在本机准备或核验资源 ZIP 时才需要 Python 3；当前前端是仓库内静态 HTML/CSS/JavaScript，不需要安装 Node.js 来执行下方 Cargo 编译探针。

在仓库根目录的 PowerShell 中确认工具链并尝试编译：

```powershell
rustup toolchain install 1.95.0-x86_64-pc-windows-msvc --profile minimal
rustc +1.95.0-x86_64-pc-windows-msvc -vV
cargo +1.95.0-x86_64-pc-windows-msvc build --locked --release -p desktop-pet -p avatar-host-2d
```

如编译成功，两个文件应为 `target\release\desktop-pet.exe` 和 `target\release\avatar-host-2d.exe`。仓库目前只记录了 macOS 的构建结果；Windows 上的 Rust、Tauri、Mocari/wgpu、sherpa-onnx 原生依赖组合尚未验证，应记录首次编译错误并按目标环境修复。不要运行 `tools/release/package_macos.py` 来制作 Windows 包。

## 当前阻碍与实现顺序

| 阶段 | 当前代码状态与完成条件 |
| --- | --- |
| 编译探针 | 先让主程序与角色宿主在锁定版本下都编译通过，确认 sherpa-onnx 静态库等原生依赖能够在 MSVC 目标上链接。 |
| 基础桌宠 | `apps/avatar-host-2d/src/platform.rs` 的非 macOS 指针、显示器工作区、移动和吸附接口仍为空实现或报错；`window.rs` 仅在 macOS 启用动态命中。需在 Windows 实现透明窗口、穿透与恢复、点击/拖拽、焦点、DPI 坐标、位置恢复和屏幕吸附，并用真实桌面下层窗口验证点击。 |
| 可选的他应用吸附 | `apps/avatar-host-2d/src/ax.rs` 的非 macOS 观察器仍为空实现。按 [跨平台设计](docs/03-desktop-platform.md) 接入 WinEvent、DWM 窗口几何及过滤规则；缺少此能力时基础桌宠仍应可用。 |
| 语音与资源 | CPAL 与 sherpa-onnx 已在代码中使用，但当前语音实测来自 macOS。须在 Windows 验证默认麦克风／扬声器、WASAPI 路径、模型加载与 CPU 性能；LLM 仍由用户运行的 LM Studio 提供。 |
| 打包与发行 | `apps/desktop/src/main.rs` 当前按 macOS `.app/Contents/Resources` 查找内置文件和辅助进程；`apps/desktop/tauri.conf.json` 未启用 Windows 打包，也没有 Windows 图标、资源布局或工作流。先使两个 `.exe`、角色包和可选语音资源在移动后的安装目录中正确解析，再选择 [Tauri 的 NSIS 或 MSI 安装包](https://v2.tauri.app/distribute/windows-installer/)并做干净机器验收。 |

建议按“编译 → 可见角色与输入 → 屏幕吸附和存档 → CPU 语音 → 他应用吸附 → 安装包”的顺序推进。不要将图形渲染使用的 GPU 后端等同于语音推理加速；CUDA、DirectML 等每种组合均须单独验证。

## 资源和验收准备

- 角色包、互动语音和语音模型目前由独立资源 ZIP 提供，不在 Git 中。格式与清单见 [macOS 构建说明](BUILDING.zh-CN.md#资源包)；Windows 打包器仍需实现相同的校验和安全解包，不能直接复用 `.app` 目录结构。先使用有明确再分发权限的演示资产；来源和限制见 [资源来源](docs/ASSETS.md)与[使用及版权说明](POLICY.md)。
- 无语音基线只需要有效的演示角色包。完整语音还需要 VAD、ASR、TTS／KWS 对应模型及参考音频；LLM 需要用户自行启动 LM Studio 并加载模型。可选 ncnn 后端还需单独提供 Windows 版 `sherpa-ncnn-offline`。
- 验收至少覆盖：透明区域点击下层应用、角色点击和拖拽、失焦／退出、单屏与混合 DPI 双屏、显示器拔插、休眠恢复、宿主崩溃、托盘与存档、麦克风／扬声器以及安装后从非源码目录启动。记录机器、Windows 版本、GPU／驱动、显示缩放、模型版本和结果。

[路线图 P7](docs/05-roadmap.md)中的 10–20 人日起是 Windows 与 Linux 的早期阶段估算，不是 Windows 已可构建的承诺。基于当前代码，Windows 首次编译排障约需 **1–3 人日**；可用的基础 Windows 桌宠约需 **8–15 人日**；包含语音实测、资源打包与安装验收约需 **15–25 人日**。这些是单名熟悉 Rust 和桌面 API 的开发者的规划量级，原生依赖和目标设备问题可能使其变化。
