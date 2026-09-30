# Windows 构建与分发

`windows` 分支的基础桌宠、他应用窗口吸附和 Ollama 本地语音已通过本机自动检查及用户功能验收。手动 GitHub Actions 构建并发布带 Authenticode 开发签名的 Windows x64 Setup.exe，包含全部资源及图形向导，支持可选开机自启和静默启动。跨设备发行验收仍需另行覆盖。构建／签名操作见 [Windows CI 与签名](docs/windows-ci-signing.md)，实现与证据见 [Windows 开发交接](docs/windows-build-progress.md)。

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

两个文件为构建目录中的 `release\desktop-pet.exe` 和 `release\avatar-host-2d.exe`。Windows 11 x64 的 Rust、Tauri、Mocari/wgpu、sherpa-onnx 原生依赖已完成本机编译及链接验证。可使用 `tools/windows/build.ps1 -RunTests` 加载 MSVC 环境并运行测试；`-BuildSmokeTools` 另编译无麦克风的检查工具。手动 CI 和便携包签名见 [Windows CI 与签名](docs/windows-ci-signing.md)。

## 当前阻碍与实现顺序

| 阶段 | 当前代码状态与完成条件 |
| --- | --- |
| 编译探针 | 本机两个程序的锁定版本 Release 编译及原生依赖链接通过。 |
| 基础桌宠 | Win32 指针、工作区、移动、位置恢复、点击穿透和屏幕吸附已接入，用户确认本机功能正常。混合 DPI、多屏、休眠和其他设备仍需专项覆盖。 |
| 可选的他应用吸附 | 前台窗口、DWM 可见边框、WinEvent、过滤及宠物获焦时保留目标已实现，单元测试和用户本机验收通过。混合 DPI／高权限窗口待专项验证。 |
| 语音与资源 | Ollama Qwen3.5 9B、CPU ZipVoice 和 CPAL 链路检查通过，用户确认本机语音功能正常；其他设备、长期运行及可选后端单独验收。 |
| 打包与发行 | 可移动布局、资源校验、NSIS 图形安装向导、可选开机自启、静默启动、程序／卸载器／Setup.exe 签名和 Release 自动发布已接入。自签名证书无默认系统信任；受信任 CA 签名与干净机器验收仍未完成。 |

建议按“编译 → 可见角色与输入 → 屏幕吸附和存档 → CPU 语音 → 他应用吸附 → 安装包”的顺序推进。不要将图形渲染使用的 GPU 后端等同于语音推理加速；CUDA、DirectML 等每种组合均须单独验证。

## 资源和验收准备

- 角色包、互动语音和语音模型目前由独立资源 ZIP 提供，不在 Git 中。格式与清单见 [macOS 构建说明](BUILDING.zh-CN.md#资源包)；Windows 本地打包器已接入大小／SHA-256 校验和安全解包。发行时使用有明确再分发权限的演示资产；来源和限制见 [资源来源](docs/ASSETS.md)与[使用及版权说明](POLICY.md)。
- 无语音基线只需要有效的演示角色包。完整语音还需要 VAD、ASR、TTS／KWS 对应模型及参考音频；LLM 需要可用的 Ollama、LM Studio 等服务及模型。Ollama 请求使用 `reasoning_effort: none`，避免把思考过程引入语音；W2 的本机模型别名使用 4096 上下文。可选 ncnn 后端还需单独提供 Windows 版 `sherpa-ncnn-offline`。
- 验收至少覆盖：透明区域点击下层应用、角色点击和拖拽、失焦／退出、单屏与混合 DPI 双屏、显示器拔插、休眠恢复、宿主崩溃、托盘与存档、麦克风／扬声器以及安装后从非源码目录启动。记录机器、Windows 版本、GPU／驱动、显示缩放、模型版本和结果。

[路线图 P7](docs/05-roadmap.md)中的 10–20 人日起是 Windows 与 Linux 的早期阶段估算，不是 Windows 已可构建的承诺。基于当前代码，Windows 首次编译排障约需 **1–3 人日**；可用的基础 Windows 桌宠约需 **8–15 人日**；包含语音实测、资源打包与安装验收约需 **15–25 人日**。这些是单名熟悉 Rust 和桌面 API 的开发者的规划量级，原生依赖和目标设备问题可能使其变化。
