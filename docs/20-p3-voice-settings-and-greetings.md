# P3 语音配置与互动语音：分步实施

日期：2026-09-29。本文接续 [语音流水线](19-p3-voice-pipeline.md)。当前代码已接入设置面板、部分可运行后端、头部点击与本地互动语音；真实麦克风和桌面试听仍需人工验收。

## 已接入的首步

| 配置 | 当前行为 |
| --- | --- |
| ASR | `sherpa-onnx` SenseVoice 或 `sherpa-ncnn` SenseVoice 2025-09-09。ncnn 适配器调用官方 `sherpa-ncnn-offline`，模型目录、程序路径和 CPU 线程数可配；sherpa-mlx 暂从设置中移除。 |
| LLM | LM Studio 默认使用原生 `/api/v1/chat`；Ollama、llama.cpp server、OpenAI API 使用可配置的 `/v1/chat/completions` 兼容接口和可选 Bearer key。兼容后端在本轮会话内最多保留最近 6 组问答。Anthropic 与 Gemini 原生适配尚未接入。 |
| TTS | `sherpa-onnx` ZipVoice；可改模型目录、参考 WAV 与文字、1–16 CPU 线程，默认 8。qwentts.cpp 的服务地址和 Kokoro 说话人字段先预留，后端不可启用。 |
| VAD | Silero 模型文件、阈值、句末静音时长与等待说话超时可配，默认阈值 0.5、静音 600 ms、首音 8 秒。 |
| KWS | 可选 sherpa-onnx 中英 Zipformer 常驻待机监听，默认“你好纳西妲”和“Hello Nahida”；唤醒后复用头部点击入口。自定义词须提供 `phone+ppinyin` 转换后的关键词文件。 |
| GPU / AEC | 可见但不可启用；设置校验会拒绝已启用却未实现的组合。 |
| 无语音互动后休息 | 0 为关闭；设置分钟数后，待机超过该时长触发一次现有养成 `Rest` 行为，下一次语音互动重新计时。 |

## 设置面板排版

面板按一次对话的使用顺序纵向排列：**Audio → ASR → VAD → KWS → LLM → TTS**。480 像素设置窗口内每块独立成卡片，常用项直接显示，高级项默认折叠；底部统一保存、开始、停止和显示状态。Audio 集中总开关、0–100% 播放音量、超时、AEC 与互动提示音。音量在同一扬声器输出端同时作用于 ZipVoice 回复和本地互动 WAV。

ASR、TTS 的后端选择会切换其下方的配置面板，并保留各后端已经填过的值。sherpa-onnx 的模型文件与 CPU/GPU 设置折叠在 SenseVoice、ZipVoice 各自下方；sherpa-ncnn 展示模型目录、离线程序路径、CPU 数量及预留的 Vulkan 选项，qwentts.cpp 展示服务 URL，Kokoro 展示说话人、模型、CPU/GPU。KWS 有自己的 sherpa-onnx 模型、中英文关键词、可选词表文件、阈值和 CPU 设置；VAD 的模型与阈值也单独折叠。共享模型根目录留在 Audio，因为当前 ASR、VAD 和 TTS 都从那里解析默认文件。

LLM 卡片新增可编辑的 **System Prompt**，默认保持原先的简短中文角色提示。保存后，LM Studio 原生请求和兼容 Chat Completions 请求都会使用它；旧存档自动获得默认值。规划中的后端可切换查看和填写参数，但启用语音时设置校验仍会拒绝未实现的后端。

接口继续用 `InputPort`、`AsrPort`、`LlmPort`、`TtsPort`、`PlaybackPort` 隔离提供方。`TtsPort` 通过 PCM 块回调交付音频；未来的 qwentts.cpp 服务、云端 ASR/TTS、Anthropic/Gemini 等分别实现端口，不需要更改角色点击逻辑。设置采用向后兼容默认值读取旧的 `voice` 存档。

Ollama、llama.cpp server 与 OpenAI API 的兼容路径已接线，但本轮未连接这些服务做实际生成验证；具体模型 ID、鉴权和响应格式仍须分别试跑。[Ollama 兼容接口](https://ollama.com/blog/openai-compatibility)、[llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) 均提供对应的 Chat Completions 路径。

## 互动语音规则

**头部点击或关键词**启动对话：若“播放唤醒语音”启用，先顺序播放 `心事.wav`，再打开麦克风。其他部位点击仍只触发现有表情与养成反馈。再次点击头部会取消当前生成或播放，重新播放提示音并开启新会话。KWS 只在待机时占用麦克风；语音播放和整段对话期间暂停监听，以免扬声器自声误唤醒。因此当前版本仍以头部点击作为播放期间的显式打断方式。

### sherpa-ncnn 与 KWS 本地安装

从[官方 2025-09-09 SenseVoice 模型页](https://k2-fsa.github.io/sherpa/ncnn/sense-voice/pretrained.html#sherpa-ncnn-sense-voice-zh-en-ja-ko-yue-2025-09-09-chinese-english-japanese-korean-cantonese)下载模型，解压到被 Git 忽略的 `models/local/sherpa-ncnn-sense-voice-zh-en-ja-ko-yue-2025-09-09/`。其中应有 `model.ncnn.param`、`model.ncnn.bin` 和 `tokens.txt`。自行构建官方 `sherpa-ncnn-offline`，在 ASR 高级设置填写程序完整路径，或将程序放入 `PATH`；模型目录留空时使用上述默认路径。适配器把 VAD 端点后的 16 kHz PCM 写入系统临时 WAV，传给官方离线程序，从其输出中读取 JSON `text`；当前官方构建把结果写到 stderr，适配器兼容 stdout 和 stderr。取消会话会结束正在运行的子进程。模型与程序不随仓库提交。

KWS 模型为[官方中英 Zipformer 3M](https://k2-fsa.github.io/sherpa/onnx/kws/pretrained_models/index.html)，默认放在 `models/local/sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20/`。内置词表按官方 `phone+ppinyin` 格式为 `n ǐ h ǎo n à x ī d á @你好纳西妲` 与 `HH AH0 L OW1 N AA0 HH IY1 D AH0 @Hello_Nahida`。后者的专名发音按 *Na-hee-da* 近似，仍需真实语音验证漏检率。修改文本时，应使用 `sherpa-onnx-cli text2token --tokens .../tokens.txt --tokens-type phone+ppinyin --lexicon .../en.phone keywords_raw.txt keywords.txt` 生成词表，并把 `keywords.txt` 路径填进 KWS 高级设置。关键词是可听音频门控；阈值默认 0.25，可根据误触发再调。

本机功能试跑：从官方源码构建 `sherpa-ncnn-offline` 后，其 2025-09-09 模型的 `zh.wav` 输出“饭时间早上九点至下午五点”，Rust 子进程适配器成功解析该结果（仅证明链路能运行，不代表识别准确率）。KWS Rust 接口成功加载默认双语词表，对系统语音合成的“你好纳西妲”在追加静音后命中。系统及 ZipVoice 合成的英文“Hello Nahida”均未命中；SenseVoice 对其中一条只转写出“HELLO NAHI”，说明测试合成音频至少有一条词尾不完整。当前不能宣称英文唤醒已通过实际质量验收。测试音频、模型与构建目录均留在 Git 忽略的 `models/local/`。

“初次见面”在语音配置启用、角色显示、互动语音文件可用后排队播放一次；标记写入本地存档。定时问候在本地时间 8、12、19、22 点对应的小时内、角色显示且语音待机时各播放一次，同一小时的日期键保存在存档中，避免重启后重复。若程序在该小时内没有运行，就不补播错过的问候。检查间隔为 15 秒。

本地素材映射：`初次见面` → 首次见面；`心事` → 点击头部；`早上好` → 8 点；`午休时间到` → 12 点；`太阳落山` → 19 点；`快去睡吧` → 22 点。源目录的 `.mp3` 文件实际为 **Ogg Vorbis** 编码；开发机已转换为 24 kHz 单声道 WAV，按语言和用途放在被 Git 忽略的 `artifacts/voice/`。源文件留在 `artifacts/wav/`，两处都不提交。ZipVoice 当前默认参考音频为 `artifacts/voice/zh-CN/greetings/noon.wav`，参考文字来自同名 `.txt`；用户可在面板更换为自己的 WAV。

## 下一步

1. **真实桌面验收**：逐项试听首次、头部、四时段提示音，以及提示音结束后麦克风权限、VAD 端点、识别、回复和扬声器；同时检查角色点击与既有表情反馈不冲突。记录首次播报失败时的重试策略。本机已通过 `voice-clip-smoke` 将 `心事.wav` 经实际扬声器播放，也曾通过文本注入的 LM Studio → ZipVoice → 扬声器试跑；整条桌面链路尚未人工验收。2026-09-29 再试 LM Studio 时服务未启动，无法复测该段。
2. **后端适配**：接 qwentts.cpp 的 URL、鉴权、说话人和 PCM 流；再按需接 sherpa-mlx、Kokoro、云 ASR/TTS 与 Anthropic/Gemini 原生协议。ncnn 若成为长期依赖，可从子进程转为原生 Rust/C 边界。启用 GPU 前测实际 provider 和回退效果。发布前把云端 API key 移至系统凭据库，当前开发存档将 key 保存在用户本机 SQLite。
3. **KWS 质量验收**：以实际说出的中英文唤醒词、长时间背景录音和扬声器播放测漏检与误触发；调节阈值与英文专名发音。待 AEC 验证后，再考虑播放期间的语音打断。
4. **AEC 与打断**：在支持的平台验证麦克风与扬声器设备、时钟和播放参考帧，分别评估 macOS、Windows、Linux。当前不提供说话打断；即使 AEC 不可用，头部点击仍可显式打断。只有通过真实设备测试后才开放 AEC 开关。

开发版可通过设置面板修改这些配置；本机示例配置与模型留在 `artifacts/local/`、`models/local/`。仓库只保留代码和文档，不提交角色素材、用户声音、模型权重或密钥。

2026-09-29 后续更新：生日、喂食、主动招呼、四档亲密度语音与中英日韩目录、界面切换已接入；KWS 勾选即保存并显示监听状态。详见[多语言互动语音](22-p3-localized-interactions.md)。
