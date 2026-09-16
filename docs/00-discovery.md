# 本地资源与选型依据

## 核查范围

2026-09-16 对项目目录、指定角色目录及 `/Users/ncy/Projects/ASR`、`TTS`、`kaldi` 做只读检查；读取模型清单和使用说明，以 Blender 后台模式禁用自动脚本执行后读取现有 Blend。未修改原始模型、安装依赖或运行语音推理。当前仓库检查时没有应用代码。

## 开发设备

| 项目 | 已观测信息 | 对设计的影响 |
| --- | --- | --- |
| 机器 | MacBook Pro / Apple M4 Max | 第一性能基线采用 Apple Silicon |
| CPU / GPU | 16 核 CPU / 40 核 GPU | 轻量常驻音频先走 CPU；重模型与渲染竞争 GPU |
| 内存 | 48 GB 统一内存 | 不能把图形显存与模型内存视为独立预算 |
| 显示 | 内建 Retina，物理 3456 × 2234 | 必须验证逻辑坐标、物理像素、缩放转换 |
| Blender | 5.0.0 | PMX 插件需验证此版本；不默认旧插件兼容 |

本轮未记录具体 macOS 版本、LM Studio 版本及其已加载模型，P0/P3 建立基线时补充。未测试麦克风、扬声器和外接屏。

## 角色资产

根目录：[GenshinImpact](/Users/ncy/Downloads/GenshinImpact)。表中路径相对此目录。

| 资源 | 实际格式与检查结果 | 推荐用途 |
| --- | --- | --- |
| `Nahida_1080/` | `.model3.json`、`.moc3`、物理配置、参数显示信息、4 张 1024² PNG；13 个 `.exp3.json` | 首个 Mocari 技术验证角色 |
| `可莉/可莉2.0.pmx` | PMX 与贴图；包含使用规则 | 三维阶段候选，骨骼/表情质量尚未检查 |
| `可莉_琪花星烛/可莉.pmx` | PMX、toon/sph/贴图；包含使用规则 | 第二套三维候选，转换需验证材质 |
| `原神少女_哥伦比亚模型/` | PMX、FBX、Blend、贴图 | 先作为静态三维试件，随后绑定 |
| 两个 ZIP | 可莉相关压缩包 | 本轮未解压，不作为额外已验证模型 |

纳西妲检查明细：

- `FileReferences` 中 Moc、Textures、Physics、DisplayInfo 所指文件均存在。
- `Groups` 登记 `LipSync -> ParamMouthOpenY` 和 `EyeBlink -> ParamEyeLOpen/ParamEyeROpen`。
- `FileReferences` 没有 `Expressions`、`Motions`；顶层没有 `HitAreas`。不能仅把目录交给加载器就假定会自动播放表情或响应点击。
- 递归查找未发现 `.motion3.json`。首版呼吸、视线、眨眼可采用参数程序动画；身体行走或进食动作是否有足够变形能力仍须视觉验证。
- `.moc3` 文件头存在 `MOC3` 标识；文件存在和头部检查不等于 Mocari 兼容性验收。

哥伦比亚 Blend 的实际数据：5 个对象、5 个网格、31,375 个顶点、0 个 Armature、0 个 Action、0 个形态键块。与 `RM.txt` 的未绑定说明一致。文件读取成功，但未进行材质渲染或完整视觉检查；不能称为可直接动画驱动的角色。

## 本地语音工程

| 路径 | 本轮发现 | 接入判断 |
| --- | --- | --- |
| `/Users/ncy/Projects/kaldi/sherpa-onnx` | 本地提交 `bc5157a5`；README 包含 Rust、Tauri、ASR/TTS/KWS/VAD 支持说明 | 第一优先；为各功能单独准备兼容模型 |
| `/Users/ncy/Projects/kaldi/sherpa-mlx` | 本地提交 `0d2eae7`；README 展示 MLX VAD 流程 | 作为 Apple Silicon 实验候选，不视为完整 SenseVoice/CosyVoice 替代 |
| `/Users/ncy/Projects/kaldi/sherpa-ncnn` | 目录存在 | CPU 或特定模型候选；不从目录存在推断任意模型受支持 |
| `/Users/ncy/Projects/ASR/SenseVoiceSmall-onnx` | 本地提交 `fe8f35d`；有 5/10/15/20/25/30 秒命名的模型目录、AX 构建脚本 | 可能是特定设备/输入形状导出；先核查张量签名、tokenizer、前处理、量化，不能直接当作 sherpa 标准包 |
| `/Users/ncy/Projects/ASR` | 另有 FireRedASR2S、Qwen3-ASR 相关项目 | 后续 ASR 适配与质量对比 |
| `/Users/ncy/Projects/TTS` | kokoro、Breeze-TTS-2、MOSS-TTS-Nano、Qwen3-TTS、qwentts.cpp、supertonic 等 | 先选 CPU 可达标的 TTS 基线，再比较质量与设备占用 |
| LM Studio | 用户说明已安装；未检查服务健康 | P3 接入兼容 HTTP API，启动时发现真实模型 ID |

本轮在指定 TTS 根目录列表中未发现 CosyVoice 独立目录；这不表示机器其他位置没有安装。设计保留 CosyVoice 服务适配器，不假定其环境和权重已准备完成。

## Mocari 版本核实

用户指定 [Mocari 0.3.1](https://crates.io/crates/mocari/0.3.1)。网页抓取未成功，改为直接读取 [0.3.1 发布包](https://static.crates.io/crates/mocari/mocari-0.3.1.crate) 内的 `Cargo.toml.orig`、`src/lib.rs`、示例和渲染代码。

- 发布包：Rust edition 2024；默认 feature 为空；内置 wgpu 渲染需开启 `wgpu` feature。
- 声明依赖 wgpu `30.0.0`；示例开发依赖 winit `0.30.13`。它们是包内版本声明，不等于本项目已经编译通过。
- `.cargo_vcs_info.json` 对应源码提交 `d08fdbc05e798716403b2cb63bb2a83546082d1d`。
- 包内包含 `assets::load_model_runtime`、`ModelRuntime`、`MotionPlayer`、`ExpressionManager` 和 `render::wgpu::WgpuLive2dRenderer`。
- 核查时主分支 Cargo 版本为 0.4.0；后续不要用 `latest` 文档直接编写 0.3.1 调用代码。

P0 建议声明 `mocari = { version = "=0.3.1", features = ["wgpu"] }`，生成并提交 Cargo.lock，再确定完整 Rust 工具链版本。edition 2024 本身不能证明所有传递依赖的最低 Rust 版本。

## 资产发布边界

本地可莉两份说明明确限制商业使用和二次配布。纳西妲目录未见明确授权说明；哥伦比亚说明提到游戏提取和支持二改二配，但该文字不能独立确认完整发行权利。

工程采用“运行时与角色包分离”：本地开发从用户选择的目录导入；公共仓库和发行包使用原创或已明确获准分发的测试角色。记录每个角色包的来源、作者和许可状态。原始资产及其转换产物均不默认进入发布包。此处是对本地说明的工程处理，不是法律结论。
