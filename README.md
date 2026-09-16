# DesktopPet：架构与分阶段开发设计

设计日期：2026-09-16。目标：macOS 首发，Rust 为主要开发语言，后续支持 Windows、Linux。当前交付是设计文档，尚未实现或验证桌宠运行程序。

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

关键结论：

- Mocari 是 Live2D/Cubism 兼容运行时，不能直接加载 PMX、FBX、Blend。用户指定版本为 0.3.1，已读取发布包源码核实；不以主分支替代版本依据。
- 本地纳西妲资源引用完整，有 13 个表情文件，但未登记表情，且没有动作文件及命中区配置。适合先补参数映射和交互区域。
- 哥伦比亚 Blend 已通过 Blender 后台只读检查：5 个网格、31,375 个顶点，没有骨架、动作和形态键，需要先完成美术准备。
- 图形 Metal 与推理 Metal 是不同子系统；每个语音模型分别选择运行时、模型变体和设备。AMD ONNX 路径以 MIGraphX 为候选，旧 ROCm EP 已被移除。
- 完成基础桌宠、养成和语音对话后，再实现关键词唤醒与全双工；三维角色和其他平台不阻塞 macOS 首个版本。

首个开发入口：[P0 技术验证](docs/05-roadmap.md)。建议将 P0 的实际测量结果作为正式排期依据。文档中的延迟、内存、帧率均为初始验收目标，非现有实现的实测成绩。

## 视频封面

- [16:9 横版封面](assets/covers/desktoppet-cover-16x9.png)
- [4:3 横版封面](assets/covers/desktoppet-cover-4x3.png)
