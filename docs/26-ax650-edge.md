# AX650 完整桌宠实现与测试计划

2026-10-10 从 `codex/linux-x11-baseline` 创建 `ax650` 分支。目标是在 AX650N / Ubuntu 22.04 ARM64 / XFCE X11 上运行完整桌宠：透明角色、托盘、角色与设置窗口、养成存档，以及板端麦克风语音识别。复用现有 Tauri 主程序和独立角色宿主，默认使用 CPU 软件 OpenGL。稳定 30 fps 不作为验收条件。构建、资源和用户测试见 [AX650 构建说明](../BUILDING.ax650.zh-CN.md)。

## 环境与已有证据

| 项目 | 2026-10-08 至 2026-10-09 实机记录 |
| --- | --- |
| 板卡 / CPU | AX650N，8 核 Cortex-A55，1.7 GHz，aarch64 / NEON |
| 系统 | Ubuntu 22.04、厂商 Linux 5.15.73，系统可见内存约 3.8 GiB |
| 桌面 | XFCE / xfwm4，Xorg / fbdev，单屏 1920×1080 |
| 图形执行 | 没有发现 `/dev/dri/renderD*` 或通用着色器 GPU；Mesa llvmpipe 在 CPU 上执行图形工作 |
| 用户已确认 | 私有诊断包的软件 OpenGL 路径可见角色；实际输出窗口 320×384 |
| 日志佐证 | 两次 30 秒运行正常退出；最新一次初始化约 201 ms，599 次 present 调用，调用间隔 p95 约 51.39 ms；离屏角色和自身窗口 pixmap 均有有效像素及部分透明像素 |
| 麦克风 | 当前尚未接入，由用户稍后连接并在板端验收 |

2026-10-10 的只读会话检查确认当前 XFCE `:0` 已有真实 StatusNotifierWatcher，`IsStatusNotifierHostRegistered=true`。完整包复用该托盘宿主，不安装或创建面板；此检查不证明新应用图标和菜单已通过。

约每秒 20 次 present 调用是历史角色包和尺寸下的调用统计，不是显示扫描帧率，也不代表完整桌宠在语音负载下的性能。旧私有包的目视确认只证明其软件 GL 显示路径；托盘、设置、养成、麦克风采集和识别必须按当前完整包分别验收。

早期失败有三个独立原因或范围：

1. 缺少动态加载的 `libxkbcommon-x11` 及其 `libxcb-xkb` 依赖，宿主在 X11 初始化时退出。仅使用 `ldd` 检查直接链接依赖会遗漏此类库。
2. Vulkan 能初始化设备和模型、调用 submit/present，但用户看不到角色；当时没有有效像素回读，具体绘制、呈现或合成原因仍未定位。已确认窗口在正确的 X11 会话与屏幕范围内。`DISPLAY` 是 X 会话编号，不能从物理 HDMI 端口编号推导。
3. 首次 GL 运行被透明策略拒绝：锁定的 wgpu-hal 30.0.1 GLES 后端只报告 `Opaque`，原策略没有此后端的透明兼容入口。私有补丁在确认真实 32 位 ARGB visual 和合成器 owner 后允许 GL 兼容路径，并保留预乘 alpha；用户随后确认显示成功。

DRM card 和 framebuffer 的存在说明显示输出，不足以说明可执行 wgpu 着色器。AXERA NPU、IVPS 和 VO 各有专门用途，也不自动替代 Live2D 网格、遮罩和桌面合成。

## 当前交付范围与架构

完整入口复用 `apps/desktop` 的 `desktop-pet`。Tauri 主程序持有托盘、设置 UI、角色管理、PetCore、SQLite 和语音控制器；`avatar-host-2d` 独立负责渲染和原生窗口。它们使用已有语义 IPC 与心跳，不传整帧像素。`apps/ax650` 的终端程序保留为无 UI 的渲染和生命周期诊断工具。

| 组成 | AX650 交付职责 |
| --- | --- |
| `desktop-pet` | XFCE 托盘、角色与设置窗口、导入与切换、显示 / 隐藏、养成、存档、主程序与宿主生命周期 |
| `avatar-host-2d` | Mocari 模型与表情、软件 GL、透明置顶窗口、X11 点击穿透与拖拽、屏幕位置、渲染统计 |
| `pet-voice-session` / CPAL / sherpa-onnx | 输入设备选择、麦克风采集、Silero VAD、SenseVoice CPU / NPU、识别结果显示，以及可选会话与朗读 |
| `PetCore` / `CareStore` | 原有养成与细分点击规则、行为仲裁、SQLite 事务和请求去重 |
| `desktop-pet-ax650` | 独立诊断；定位渲染、养成 IPC 和异常清理问题，不提供托盘或麦克风识别 |
| `tools/ax650` | Ubuntu 22.04 原生 ARM64 构建、完整私有包、运行依赖预检、独立数据与日志 |

AX650 设备配置通过 `DESKTOPPET_DEVICE_PROFILE=ax650` 选择。新存档初始使用较小角色、20 fps 调度预算、软件 GL；语音初始关闭，模式为“仅识别并显示文字”，ASR 2 / KWS 1 线程、TTS 4 线程，NPU、预留 GPU、AEC 和 KWS 关闭。已保存的用户语音设置优先，不反复覆盖用户配置。

GL 透明兼容只在显式开启 `DESKTOPPET_X11_GL_ALPHA=1`、实际 GL 后端、surface 支持 `Opaque`、窗口深度为 32、XRender alpha 通道有效且合成器 selection 有 owner 时成立。普通 PreMultiplied / PostMultiplied 路径仍优先，NVIDIA Vulkan 例外独立保留。日志记录 surface 和原生透明证据。Vulkan 是显式诊断入口，历史空白尚未定位。

完整设置窗口需要 GTK / WebKitGTK，托盘需要 Ayatana AppIndicator 及真实 XFCE 托盘宿主。其动态依赖和桌面会话 D-Bus 与角色宿主分别检查；角色显示成功不能证明设置页和托盘通过。Linux x86_64 的相关验收仍见 [Linux 开发记录](25-linux-x11-implementation.md)，不能扩大为 ARM64 板端结果。

## 板端语音识别

在 Audio 卡片中选择“仅识别并显示文字”时，只加载 Silero VAD 和 SenseVoice。它不要求 LLM 连接、ZipVoice、参考 WAV 或扬声器；用户可以先接麦克风完成独立 ASR 验收。点击角色头部或设置中的“开始聆听”开启采集，VAD 端点后生成完整识别文字并显示到现有识别结果区域。识别文字不写入长期记忆。

输入设备留空时使用 CPAL 系统默认麦克风。设置页刷新只枚举输入设备名称，不打开采集；ASR 和可选 KWS 共用保存的精确名称。同名设备无法唯一匹配时明确报错，用户可在系统设置默认输入并清空应用内名称。未接设备时报告输入故障，角色、托盘和养成继续可用；用户接入麦克风后刷新并保存选择，再开始聆听。每次会话最多处理 10 段，或在静默超时后结束；识别文字在下一段聆听时仍可查看。

| 资源 | 默认 CPU 路径 |
| --- | --- |
| VAD | `resources/models/silero_vad.onnx`，16 kHz、1 线程 |
| ASR | `resources/models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09/` 中的 `model.int8.onnx` 与 `tokens.txt`，2 线程 |
| KWS，可选 | 中英 Zipformer 模型与词表；关闭时不要求这些文件，不作为 ASR 首次验收前提 |
| LLM，可选 | 用户配置的可达 LM Studio 或兼容 Chat Completions 服务及真实模型 ID；板端的 `127.0.0.1` 指板卡自身 |
| TTS，可选 | ZipVoice int8、Vocos、参考 WAV 与准确文本；AX650 初始线程预算为 4，实际速度与音质待测 |

“识别、对话与朗读”模式沿用现有半双工管线：VAD → SenseVoice → LLM → ZipVoice → CPAL 输出。输出使用系统默认音频设备；播放时停止采集，头部点击或停止按钮可打断。LM Studio 等外部服务由用户提供，不随桌宠启动或关闭。模型、服务和真实音频设备分别报告状态，缺少可选资源不阻塞仅识别模式。

当前不开放未实现的 GPU / AEC 组合；SenseVoice / ZipVoice 已接入可选 AXERA NPU。勾选“启用 NPU 加速”自动选择包内匹配 axmodel；VAD / KWS 保留 CPU。模型来自另一会话已经转换并测试的集合。KWS、麦克风拔插恢复、识别准确率、TTS 延迟以及渲染和推理并发时的 CPU / 温度需实际测量。语音整体设计见 [流水线](19-p3-voice-pipeline.md) 与 [设置说明](20-p3-voice-settings-and-greetings.md)。

## 运行、数据与验收边界

完整包的启动器初始化本包资源、独立数据目录与日志，并选择 AX650 软件渲染配置。角色、模型权重、存档、用户声音、日志和发布产物保留在 Git 忽略位置；公共记录不收录用户路径、主机地址或凭证。更新包时保留此前成功包，以新目录验收。

完整桌宠通过真实托盘和设置页操作。CLI 的 `status`、`feed`、`play`、`rest`、`show`、`hide` 等命令只用于独立诊断；不能拿诊断程序运行成功替代完整 UI 与录音测试。

2026-10-10 此前完成的诊断路径已通过 Ubuntu 22.04 ARM64 release 构建：Rust 1.95.0、锁定依赖，CLI 最高 GLIBC 为 2.34、角色宿主为 2.35，直接依赖解析通过。81 项 Rust 回归、16 项 Python 3.10 工具回归与严格 Clippy / 格式 / shell 检查通过。这些结果只覆盖当时的诊断程序和宿主，不证明随后加入的完整 Tauri / ASR 构建或板端可用性。

同日最终完整 release 构建通过，包含 `desktop-pet`、角色宿主和可选 CLI；环境为 Ubuntu 22.04 ARM64、Rust 1.95.0 和官方 sherpa-onnx 1.13.8 CPU 静态库。完整主程序 / CLI 最高 GLIBC 为 2.34，角色宿主为 2.35，直接依赖解析通过。14 项桌面回归和语音模块 11 项纯回归通过，完整主程序与语音模块 all-targets 的严格 Clippy 通过；语音模块另 3 项既有实机 / 网络测试保持忽略，没有连接真实桌面或麦克风。25 项工具回归通过 Ubuntu Python 3.10 最终复核。完整资源静态检查覆盖 VAD / ASR / TTS / KWS、互动 WAV 及来源说明，不代表模型已在板端加载或识别准确率通过。最终二进制与源码状态以包内元数据和 SHA-256 清单为准。

| 检查 | 结果边界 |
| --- | --- |
| 历史私有 GL 角色显示 | 2026-10-09 用户确认通过；仅对应历史包 |
| 诊断程序 ARM64 构建 / 97 项回归 | 2026-10-10 通过；不代表完整桌宠 UI / ASR |
| 既有 XFCE 托盘宿主 | 2026-10-10 只读检查已存在且注册有效；新应用图标 / 菜单待用户测试 |
| 完整桌宠、宿主与可选 CLI 的 ARM64 release 构建 | 2026-10-10 通过；主程序 / CLI 最高 GLIBC 2.34，宿主 2.35，直接依赖解析通过 |
| 完整应用 / 语音自动回归与严格 Clippy | 14 项桌面、11 项语音通过，另 3 项既有实机 / 网络测试忽略；all-targets 严格 Clippy 通过 |
| 工具自动回归 | 25 项通过 Ubuntu Python 3.10 最终复核；不运行真实 UI / 录音 |
| 完整模型、互动音频与来源说明 | 静态打包检查通过；板端真实模型加载、播放与质量仍待测 |
| 真实托盘、设置、透明、点击、拖拽、养成与重启 | 由用户在板端 HDMI / XFCE 桌面测试 |
| 麦克风选择、VAD、ASR 文字与停止 | 用户接入麦克风后测试；当前没有录音或准确率结论 |
| LLM / TTS / KWS、长期运行 CPU / RSS / 温度 | 可选功能分别验收；不要求稳定 30 fps |
| Vulkan、原生 Wayland | 无当前可用性结论 |
| NPU 应用接入 | 配置、ABI、编译与纯回归通过；真实麦克风 / 应用内朗读待用户测试 |

不安装 VNC，不使用 Remote Desktop 验证。编译和不启动窗口的检查完成后，交付完整包、摘要和手动步骤；真实 UI / 麦克风测试由用户进行，不因为麦克风尚未接入而把 ASR 实现延期。

## 后续计划

| 阶段 | 工作与完成条件 |
| --- | --- |
| 完整 AX650 开发包，本轮 | 复用 Tauri 托盘 / 设置 / 养成，接入 AX650 软件 GL 配置和独立 ASR 模式，完成 ARM64 构建与私有资源打包 |
| 用户板端验收 | 托盘、设置、透明、输入、存档；麦克风接入后验证设备选择、VAD、ASR 文字、停止与重试 |
| 运行质量与可选语音 | 按实测调整帧调度和线程预算，验证 KWS / LLM / TTS、持续运行与音频设备变化；不以 30 fps 为门槛 |
| AXERA 加速适配，本轮已实现 | 复用已测试的 SenseVoice / ZipVoice / Vocos axmodel 与 zipvoice-axera 分支；独立保存 CPU / NPU 目录，编译和配置回归通过；当前应用真实音频与并发由用户测试 |
| 后续交付 | 根据实际需求实现配置远程控制、通用安装包、多屏 / 混合 DPI 与平台扩展；分别验收 |

NPU 不会默认直接执行现有 ONNX，也不是通用图形 GPU。若采用 IVPS / VO 或序列帧优化，需要另测格式、同步、透明与 XFCE 共存，不影响本轮完整桌宠与 CPU ASR 的实现范围。


## SenseVoice / ZipVoice NPU 接入（2026-10-10）

- sherpa-onnx 固定 `codex/zipvoice-axera` revision `5473e19db9c08e63aa41e4c273d9b3f743f1cd25`。Linux ARM64 使用 AXERA-enabled shared C API 与 ONNX Runtime；其余目标保留 static。版本字符串仍为 1.13.8，原生 artifact 由 SHA-256 固定。Rust overlay 仅补齐该分支 C API 已追加的 offline CTC 字段；12 个 C / Rust 配置结构的大小、对齐、每个字段偏移一致。
- SenseVoice 默认采用已独立测试的 AX650 10 秒模型及对应 tokens，来源 revision `7cf51476f6236f5dd81f08d9df533483e6845c2f`；ZipVoice 默认采用 distill encoder、四段 decoder 和 version 1 JSON 清单，以及 Vocos axmodel，来源 revision `aa48d14426f528a63ccb0720edb79651597e5316`。保留 HF 来源、前端和许可证。
- 两个高级设置里的 GPU 预留项改为“启用 NPU 加速”。勾选后 provider 为 `axera`，默认解析 `models/axera/sensevoice` / `models/axera/zipvoice`；不勾选时沿用原 CPU ONNX 路径。可分别设置 NPU 模型目录，CPU 自定义路径不被覆盖，资源路径可随包移动。默认 ASR 2、TTS 4 线程，旧存档保留用户已保存的值，VAD / KWS 仍为 CPU。
- NPU ZipVoice distill 使用 4 steps / guidance 3.0；自定义 standard version 2 清单使用 10 steps / guidance 1.0。加载前验证 decoder 清单和四段文件；语音取消沿用已有 generation 与回调检查。
- 本轮 Ubuntu 22.04 ARM64 release 编译通过；主程序最高 GLIBC 2.34，宿主 2.35，直接依赖解析通过。14 项桌面、15 项语音纯回归与 all-targets 严格 Clippy 通过；3 项既有设备 / 网络测试仍忽略。26 项工具回归通过 Ubuntu Python 3.10。测试覆盖模型切换、CPU 路径恢复、完整清单、缺失 / 越界分段、旧配置迁移和包移动，不打开真实麦克风或 NPU 推理模型。

另一会话的独立 C++ / C API 模型测试只是当前模型选择依据。本轮桌宠的真实 NPU 识别、克隆朗读、取消延迟、长发言及渲染并发仍由用户在 HDMI / XFCE 与接入的麦克风验收。当前实现不增加 VNC / Remote Desktop，不要求稳定 30 fps。来源、原生 artifact、ABI 和用户步骤见 [构建文档](../BUILDING.ax650.zh-CN.md)。

## AppImage 内置 NPU 资源与离线构建（2026-10-11）

首次手动 AppImage 构建在最终提取时报错：重打包使用 xz，而 Tauri 的 AppImage runtime 只支持 zlib / zstd。此前编译、纯回归、资源下载和 Tauri 初次打包已经通过；修正最终 SquashFS 为 zstd，并保留使用相同 runtime 提取最终产物的审核步骤。

此前套用公共 CPU 资源 Release 导致 NPU 模型未进入 AppImage，打包范围不完整。补充独立资源 Release `resources-ax650-2026-10-11`：`models-axera.zip` 包含已经交付板端的 SenseVoice AX650 10 秒 axmodel、ZipVoice distill encoder / 四段 decoder、Vocos，以及匹配 tokens、version 1 清单、lexicon 和 espeak 数据。勾选 NPU 后默认使用包内目录。原公共资源 Release 与其他平台构建继续使用现有内容。

373 个本地已有模型 / 前端文件与此前板端完整交付包逐一比对；本次仅增加上游许可证及补充 NOTICE，不更换模型与前端字节。原 HF 文件按固定 revision 校验，最终 ZIP、全部 374 个文件和来源记录锁定于 `tools/ax650/model-resources.lock.json`。CI 必须验证并内置该集合；资源缺失、被替换或多出参考声音时失败。权重上传资源 Release，Git 仅保存工具、来源和摘要。

板卡离线时仍可完成模型归档、纯回归、ARM64 编译、AppImage 重新提取审核和 GitHub 发布；不尝试连接或更改板端。当前构建流程仍仅手动触发，真实角色、托盘、麦克风、TTS 播放和应用内 NPU 推理由用户上线后验收，不要求稳定 30 fps。
