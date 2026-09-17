# DesktopPet：架构与分阶段开发设计

设计日期：2026-09-16。目标：macOS 首发，Rust 为主要开发语言，后续支持 Windows、Linux。当前包含设计文档与 P0 原生验证工具，尚无完整桌宠应用；P0 Gate 尚未通过。

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

关键结论：

- Mocari 是 Live2D/Cubism 兼容运行时，不能直接加载 PMX、FBX、Blend。用户指定版本为 0.3.1，已读取发布包源码核实；不以主分支替代版本依据。
- 本地纳西妲资源引用完整，有 13 个表情文件，但未登记表情，且没有动作文件及命中区配置。适合先补参数映射和交互区域。
- 哥伦比亚 Blend 已通过 Blender 后台只读检查：5 个网格、31,375 个顶点，没有骨架、动作和形态键，需要先完成美术准备。
- 图形 Metal 与推理 Metal 是不同子系统；每个语音模型分别选择运行时、模型变体和设备。AMD ONNX 路径以 MIGraphX 为候选，旧 ROCm EP 已被移除。
- 完成基础桌宠、养成和语音对话后，再实现关键词唤醒与全双工；三维角色和其他平台不阻塞 macOS 首个版本。

开发入口：[P0 验证工具与运行命令](tools/p0-probe/README.md)。透明试件模式 2 已获人工验证，基本输入符合用户确认的焦点规则；角色透明输出已修正并获深浅背景人工复验，P0-03 按确认范围通过，下一步验证 P0-04 进程桥接；[路线图](docs/05-roadmap.md)中的延迟、内存、帧率仍是验收目标；本轮实测值和测量边界单独记录在 [P0 实测记录](docs/07-p0-validation.md)，不代表完整应用成绩。

## 视频封面

- [宽屏重新构图封面（1672 × 941，约 16:9）](assets/covers/desktoppet-cover-16x9-native.png)
- [16:9 精确比例适配版（1920 × 1080）](assets/covers/desktoppet-cover-16x9.png)
- [4:3 横版封面](assets/covers/desktoppet-cover-4x3.png)
