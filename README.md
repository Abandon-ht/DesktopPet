# DesktopPet：架构与分阶段开发设计

设计日期：2026-09-16。目标：macOS 首发，Rust 为主要开发语言，后续支持 Windows、Linux。当前包含设计文档与 P0 原生验证工具，尚无完整桌宠应用；P0 Gate 已按本机技术试件条件通过，可进入 P1。

建议先使用本地纳西妲 Live2D 资源，采用 **Tauri 2 + Rust 业务核心 + 独立 Mocari 0.3.1/wgpu 渲染进程**。语音先通过 sherpa-onnx 和 LM Studio 建立本地闭环，再引入 CosyVoice 等可替换推理服务。三维角色走独立 GLB/Bevy 适配路径。

| 文档 | 内容 |
| --- | --- |
| [本地资源与选型依据](docs/00-discovery.md) | 模型、Blender、硬件、语音项目和已核实的限制 |
| [总体架构](docs/01-architecture.md) | 进程、Rust 模块、接口、事件、状态、扩展方式 |
| [角色与美术资产管线](docs/02-avatar-pipeline.md) | 纳西妲 Live2D 首发；PMX/FBX/Blend 到三维运行时 |
| [桌面交互与跨平台设计](docs/03-desktop-platform.md) | 点击、穿透、吸附、主动互动、macOS/Windows/Linux 差异 |
| [语音与硬件加速](docs/04-voice-and-compute.md) | KWS/VAD/ASR/LLM/TTS、打断、设备调度、兼容矩阵 |
| [分阶段开发计划](docs/05-roadmap.md) | 任务、依赖、交付物、验收条件、风险和估算 |
| [架构决策与参考项目](docs/06-decisions-and-references.md) | ADR、开源项目借鉴范围、官方资料 |
| [P0 实测记录](docs/07-p0-validation.md) | 固定版本构建、纳西妲参数/表情、透明 Surface、短时基线和待测项 |
| [P1 分步实现](docs/09-p1-implementation.md) | 核心契约已实现；应用/宿主、角色导入和平台验收的分步清单 |

关键结论：

- Mocari 是 Live2D/Cubism 兼容运行时，不能直接加载 PMX、FBX、Blend。用户指定版本为 0.3.1，已读取发布包源码核实；不以主分支替代版本依据。
- 本地纳西妲资源引用完整，有 13 个表情文件，但未登记表情，且没有动作文件及命中区配置。适合先补参数映射和交互区域。
- 哥伦比亚 Blend 已通过 Blender 后台只读检查：5 个网格、31,375 个顶点，没有骨架、动作和形态键，需要先完成美术准备。
- 图形 Metal 与推理 Metal 是不同子系统；每个语音模型分别选择运行时、模型变体和设备。AMD ONNX 路径以 MIGraphX 为候选，旧 ROCm EP 已被移除。
- 完成基础桌宠、养成和语音对话后，再实现关键词唤醒与全双工；三维角色和其他平台不阻塞 macOS 首个版本。

开发入口：[P0 原生验证工具](tools/p0-probe/README.md)与 [Tauri 进程桥接试件](tools/p0-tauri-probe/README.md)。P0-01 至 P0-06 已按本机技术试件条件通过，包括角色目视验收、原生输入/透明、进程生命周期、三屏及 AX 观察、30 分钟稳定性；下一步进入 P1 基础 macOS 桌宠开发。[路线图](docs/05-roadmap.md)中的产品延迟、内存、帧率仍是验收目标；实际成绩及边界见 [P0 实测记录](docs/07-p0-validation.md)，不代表完整应用成绩。

P1 已开始：`crates/pet-protocol` 与 `crates/pet-core` 提供类型化事件、最小端口和交互状态逻辑，已通过纯逻辑测试；尚未接入原生窗口。下一步为 P1-02 托盘与独立宿主闭环，范围和验证见 [P1 分步实现](docs/09-p1-implementation.md)。

## 视频封面

- [宽屏重新构图封面（1672 × 941，约 16:9）](assets/covers/desktoppet-cover-16x9-native.png)
- [16:9 精确比例适配版（1920 × 1080）](assets/covers/desktoppet-cover-16x9.png)
- [4:3 横版封面](assets/covers/desktoppet-cover-4x3.png)
