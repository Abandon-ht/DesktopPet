# DesktopPet：架构与分阶段开发设计

English: [README.en.md](README.en.md) · macOS 构建：[中文](BUILDING.zh-CN.md) / [English](BUILDING.md) · Windows 构建准备：[中文](BUILDING.windows.zh-CN.md) / [English](BUILDING.windows.md) · Linux 构建准备：[中文](BUILDING.linux.zh-CN.md) / [English](BUILDING.linux.md) · [资源来源 / Asset provenance](docs/ASSETS.md) · [使用与版权说明 / Use and copyright](POLICY.md)

Contributors and users must not use this project to create or distribute sexualized content involving minors or minor-presenting characters.

禁止使用本项目制作或传播涉及未成年人或明显幼态角色的色情、性化内容。

版权投诉及联系删除方式见 [POLICY.md](POLICY.md)。

设计日期：2026-09-16。目标：macOS 首发，Rust 为主要开发语言，后续支持 Windows、Linux。当前包含设计文档、P0 原生验证工具与 P1 开发版应用；P0 Gate 已通过，P1-01 至 P1-06 的本地 alpha 开发和用户桌面验收已完成。窗口跟随仍有少量可感知延迟，用户接受并决定停止本阶段优化；帧间隔 p95 略高于原目标，完整发行包的集成验收仍属后续阶段。

建议先使用本地纳西妲 Live2D 资源，采用 **Tauri 2 + Rust 业务核心 + 独立 Mocari 0.3.1/wgpu 渲染进程**。语音先通过 sherpa-onnx 和 LM Studio 建立本地闭环，再引入 CosyVoice 等可替换推理服务。三维角色走独立 GLB/Bevy 适配路径。

| 文档 | 内容 |
| --- | --- |
| [本地资源与选型依据](docs/00-discovery.md) | 模型、Blender、硬件、语音项目和已核实的限制 |
| [总体架构](docs/01-architecture.md) | 进程、Rust 模块、接口、事件、状态、扩展方式 |
| [角色与美术资产管线](docs/02-avatar-pipeline.md) | 纳西妲 Live2D 首发；PMX/FBX/Blend 到三维运行时 |
| [桌面交互与跨平台设计](docs/03-desktop-platform.md) | 点击、穿透、吸附、主动互动、macOS/Windows/Linux 差异 |
| [语音与硬件加速](docs/04-voice-and-compute.md) | KWS/VAD/ASR/LLM/TTS、打断、设备调度、联网查询与受限 Agent 规划 |
| [分阶段开发计划](docs/05-roadmap.md) | 任务、依赖、交付物、验收条件、风险和估算 |
| [架构决策与参考项目](docs/06-decisions-and-references.md) | ADR、开源项目借鉴范围、官方资料 |
| [P0 实测记录](docs/07-p0-validation.md) | 固定版本构建、纳西妲参数/表情、透明 Surface、短时基线和待测项 |
| [P1 分步实现](docs/09-p1-implementation.md) | 核心契约、托盘与宿主实现进度；角色导入和平台验收清单 |
| [P1-05 他应用吸附](docs/13-p1-05-external-snap.md) | 实验开关、AX 焦点窗口读取、权限和后续验收 |
| [P1-06 alpha 桌面验收](docs/14-p1-06-alpha-validation.md) | 分组测试、日志采集与结果记录 |
| [P2 分步实现](docs/15-p2-implementation.md) | 养成存档与界面、后续行为仲裁和主动陪伴 |
| [P2 表情编排设计](docs/16-p2-expression-design.md) | 13 个表情的状态／交互映射、切换提示修复和历史验收记录 |
| [P2 细分点击区域](docs/17-p2-fine-hit-regions.md) | 角色包 v3／v4、12 个细分区与历史校准记录 |
| [P2 点击反应规划](docs/18-p2-touch-reactions-plan.md) | 各部位表情、关系状态规则、限制区惩罚与测试交接 |
| [P3 语音流水线设计](docs/19-p3-voice-pipeline.md) | SenseVoice、ZipVoice、扬声器播放、LM Studio 后端与分步验收 |
| [本机语音测试包](docs/21-p3-voice-test-app.md) | 编译应用路径、菜单栏新图标和 ASR/KWS/LLM/TTS 测试步骤 |
| [多语言互动语音](docs/22-p3-localized-interactions.md) | 生日、喂食、亲密度、主动招呼、多语言音频目录和 KWS 状态 |
| [长期记忆](docs/23-long-term-memory.md) | 跨会话存储、自动提取与压缩、召回、用户管理和验收边界 |

关键结论：

- Mocari 是 Live2D/Cubism 兼容运行时，不能直接加载 PMX、FBX、Blend。用户指定版本为 0.3.1，已读取发布包源码核实；不以主分支替代版本依据。
- 本地纳西妲资源引用完整，有 13 个表情文件，但未登记表情，且没有动作文件及命中区配置。适合先补参数映射和交互区域。
- 哥伦比亚 Blend 已通过 Blender 后台只读检查：5 个网格、31,375 个顶点，没有骨架、动作和形态键，需要先完成美术准备。
- 图形 Metal 与推理 Metal 是不同子系统；每个语音模型分别选择运行时、模型变体和设备。AMD ONNX 路径以 MIGraphX 为候选，旧 ROCm EP 已被移除。
- 完成基础桌宠、养成和语音对话后，再实现关键词唤醒与全双工；三维角色和其他平台不阻塞 macOS 首个版本。

开发入口：[P0 原生验证工具](tools/p0-probe/README.md)与 [Tauri 进程桥接试件](tools/p0-tauri-probe/README.md)。P0-01 至 P0-06 已按本机技术试件条件通过，包括角色目视验收、原生输入/透明、进程生命周期、三屏及 AX 观察、30 分钟稳定性；P1 基础 macOS 桌宠已进入分步实现与验收。[路线图](docs/05-roadmap.md)中的产品延迟、内存、帧率仍是验收目标；实际成绩及边界见 [P0 实测记录](docs/07-p0-validation.md)，不代表完整应用成绩。

P1 开发入口：`apps/desktop` 为托盘及状态面板，`apps/avatar-host-2d` 为独立渲染宿主。可运行本地开发版，已支持目录角色包导入、切换、大小设置及头/身体点击表情；现已加入自动眨眼、视线跟随与位置恢复。请按 [P1-04 验收说明](docs/12-p1-04-local-interaction.md) 测试；实现边界见 [P1 分步记录](docs/09-p1-implementation.md)。

P2-01 至 P2-06 已接入喂食、玩耍、休息、SQLite 存档、活动仲裁、受频率控制的主动陪伴、短时占屏、数值驱动表情及细分点击诊断。P2-07 的 12 个细分点击区和上／下半身剩余区惩罚已由用户初步目视确认。开发版将前两次限制区反馈绑定为 `Angry`、第三次绑定为 `Sad2` 黑脸；摸头固定 `Shy`，摸脸在 `shy_normal`／`Wink` 间选择，摸手在 `Shy`／`Happy1` 间选择。亲密度变化、收益冷却和退避规则仍按原设计执行。设置面板已移除三个验收用检查区，实际点击与养成数值刷新仍可用。旧角色包沿用旧反馈，更新后的本地角色包需重新导入。[实现与测试说明](docs/15-p2-implementation.md)、[表情编排](docs/16-p2-expression-design.md)、[点击反应](docs/18-p2-touch-reactions-plan.md)。

P3 本地语音开发版已把角色头部点击和可选中英文关键词唤醒接入半双工会话，使用 Silero VAD、sherpa-onnx 或 sherpa-ncnn SenseVoice、LM Studio、ZipVoice 和系统扬声器；模型和参考 WAV 留在本地。文本注入的扬声器链路已试跑，真实麦克风、关键词质量与桌面点击仍待验收。设置见[语音配置与互动语音](docs/20-p3-voice-settings-and-greetings.md)。
语音设置面板已加入后端选择、高级模型与 VAD 参数、头部唤醒语音、首次见面与定时问候；可用范围与后续步骤见[语音配置与互动语音](docs/20-p3-voice-settings-and-greetings.md)。

长期记忆首版已接入 SQLite、语音对话和设置面板，默认关闭。开启后保存文字轮次，后台提出待确认事实并每 6 轮生成滚动摘要；确认过的事实可用于跨会话回复。用户可编辑、固定、忘记或清空，真实模型质量与桌面操作待验收。详见[长期记忆设计与实现](docs/23-long-term-memory.md)。

联网查询与受限 Agent 目前**仅完成文档规划，尚未实现**。首版拟由 Rust 执行只读搜索与网页读取工具，由模型提出请求并综合带来源的结果；所用模型的工具调用质量、网络边界、取消和长期记忆隔离都需单独验收。方案见[总体架构](docs/01-architecture.md)、[语音与计算设计](docs/04-voice-and-compute.md)及[路线图 P3-W](docs/05-roadmap.md)。

若曾保存原始 `.model3.json` 路径，新版会在启动时优先迁移到开发包内有效的 `manifest.json` 并保留大小设置；已选择角色包的用户保持原选择。旧程序运行期间仍可从“角色与设置…”手动导入新包。

## 视频封面

- [宽屏重新构图封面（1672 × 941，约 16:9）](assets/covers/desktoppet-cover-16x9-native.png)
- [16:9 精确比例适配版（1920 × 1080）](assets/covers/desktoppet-cover-16x9.png)
- [4:3 横版封面](assets/covers/desktoppet-cover-4x3.png)
