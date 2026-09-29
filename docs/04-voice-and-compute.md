# 语音管线与硬件加速

## 第一条可交付链路

macOS 上先用 Rust 音频 I/O + sherpa-onnx 或 sherpa-ncnn 的 SenseVoice ASR + LM Studio HTTP 流式对话 + ZipVoice TTS。当前交互入口是**点击角色头部**或可选的 sherpa-onnx 中英文关键词监听，可先播放本地提示音再开始聆听，由 VAD 确定语句终点。再次点击头部可取消旧轮次并开始新轮次，设置面板提供停止入口。KWS 在对话与播放期间暂停；扬声器全双工需 AEC 验证后加入。

ZipVoice 是首个本地 TTS 适配器；后续可替换为 qwentts.cpp、CosyVoice 或云端服务。第一轮先验证半双工的扬声器播放；在没有验证声学回声消除前，不将扬声器模式标记为可靠的全双工。

```mermaid
flowchart LR
  CLICK[角色点击] --> SESSION[VoiceSession / 多轮会话]
  SESSION --> MIC[麦克风 / 原生采样率]
  MIC --> IO[CPAL / 有界缓冲 / 重采样]
  IO --> VAD[VAD + 端点]
  VAD --> ASR
  SESSION --> ASR[SenseVoice / 完整语句识别]
  ASR --> LLM[LM Studio / 文本增量]
  LLM --> SEG[分句 / 最小长度 / 超时合并]
  SEG --> TTS[TTS Provider / PCM chunks]
  TTS --> OUT[播放缓冲与播放时钟]
  OUT --> SPEAKER[扬声器或耳机]
  OUT --> LIP[口型包络与时间戳]
  OUT -. 后续播放参考信号 .-> AEC[AEC / 后续]
```

[CPAL](https://github.com/RustAudio/cpal) 提供跨平台底层音频 I/O；平台使用 CoreAudio、WASAPI 或 Linux 音频后端的具体支持由锁定版本和打包环境确定。CPAL 本身不提供完整 AEC。先录入设备信息，再按模型要求转换采样率；不能只修改音频头部来“变成 16 kHz”。

## 音频和会话契约

- 采集回调只拷贝到预分配环形缓冲，不做推理、网络请求、文件写入或阻塞锁。
- 初始内部分帧 20 ms；模型输入常用 16 kHz 单声道，实际以模型 manifest 为准。输出保留 TTS 原始采样率，在播放端转换为设备格式。
- 点击后立即开启采集，VAD 使用模型自己的预录机制；后续用真实麦克风样本校准首字保留和语句边界。
- KWS 与 ASR 为不同模型/任务。SenseVoice 在 sherpa 中属于非流式 ASR；“麦克风连续录入 + VAD 分段 + 句末识别”不等于真正流式 partial ASR。需要实时转写时再接支持在线解码的模型。[SenseVoice](https://k2-fsa.github.io/sherpa/onnx/sense-voice/index.html)、[sherpa C API](https://k2-fsa.github.io/sherpa/onnx/c-api/html/index.html)
- 可选 KWS 使用独立模型，仅待机监听“你好纳西妲”和“Hello Nahida”；上线前仍需验证词表、误触发和漏触发。[KWS 文档](https://k2-fsa.github.io/sherpa/onnx/kws/index.html)
- 初始端点参数：静音 600 ms 结束，最长语句 20 秒，无语音等待 8 秒结束；作为可调值，经中文停顿语料测试后更新。
- 每个轮次有 `session_id/turn_id/generation_id`；输入帧、文本片段和音频包均有序号，输出音频附 `sample_rate/channels/format/start_sample`。

当前半双工状态：Idle → 点击头部 → 提示音（若启用）→ Listening → Recognizing → Thinking → Synthesizing → Speaking → Listening。无语音等待超时（默认 8 秒）或 10 轮完成时回到 Idle；任何状态允许用户停止，设备故障进入 Faulted。

## LM Studio 与文本流

Rust HTTP 适配器连接用户配置地址，默认示例为 `http://127.0.0.1:1234` 和 `qwen/qwen3.6-35b-a3b`。启动检查服务可达性和真实模型 ID，地址与模型均可配置。保留兼容 `/v1/chat/completions`；当前会话优先使用原生 `/api/v1/chat` 的 SSE 和上下文续接，以关闭当前模型的推理输出。[LM Studio Chat Completions](https://lmstudio.ai/docs/developer/openai-compat/chat-completions)

保存 endpoint、model_id、上下文预算、超时和认证配置；支持流式文本是基线，工具调用和结构化输出需实际模型支持。LlmPort 把服务差异封装起来；不会为了使用兼容 API 而依赖云端 OpenAI 服务。

提示词包含角色性格、精简的 PetNeeds、最近对话和用户允许保存的偏好。不要将所有数据库历史或桌面内容自动输入模型。模型响应分成用户可听文本和受限动作建议；不读取或播放推理过程字段。未闭合 JSON、非法工具调用和未知动作均不执行。

分句器等待中文句末标点或长度/时间阈值，再提交 TTS；限制并发合成与待播总时长。短回复优先响应，长回复可边生成边播放。基线 TTS 即使只能整句输出，也可通过分句得到较早首段音频，但不要称为该 TTS 已支持文本/音频双流。

## 联网查询与受限 Agent（规划）

**状态：仅设计，尚未接入代码。** 当前 `crates/inference-http` 的兼容接口请求没有 `tools` 字段，SSE 解析只收集 `choices[0].delta.content`；LM Studio 原生接口也只转发 `message.delta`。`VoiceSession` 在一轮中调用一次 `LlmPort::reply`，随后才分句合成语音。新增联网查询必须扩展这些契约，不能只让模型“假装已经搜索”。

### 首版能力与数据流

首版只提供两个只读工具：`web_search(query)` 返回来源 ID、标题、摘要、URL、检索时间；`fetch_page(url)` 返回正文摘录、最终 URL、页面标题与获取时间。搜索由可替换的搜索服务 API 提供，不把单个网站抓取当作全网搜索。`fetch_page` 仅在摘要不足时读取本轮搜索返回的少量 URL。回答引用的来源须匹配本轮工具结果中的来源 ID；没有可靠来源时说明未查到或无法核实，不用模型已有知识冒充实时结果。

1. 联网开关默认关闭，用户开启后，明确的查询请求或明显依赖实时信息的问题才可触发上述工具；普通聊天沿用现有无工具路径。界面说明查询会发送到所选搜索服务，并显示“查询中”及当前来源；语音仅朗读最终回答正文。
2. `AgentRuntime` 校验工具名和参数，执行搜索或网页读取，把有界结果作为 `tool` 消息回传模型；模型可继续请求工具，直到返回最终正文或达到预算。模型输出只是请求，执行权在 Rust 工具注册表。
3. 每轮沿用 `session_id`、`turn_id` 和取消令牌。初始预算建议为最多 2 次模型工具往返、4 次工具调用、3 个网页、30 秒总等待；均应在真实服务和模型上测量后调整。停止、打断或用户关闭联网时取消未完成请求并丢弃迟到结果。
4. 最终输出分为 `answer_text` 与 `sources`。TTS 只接收 `answer_text`；界面显示来源标题、链接和获取时间。工具参数、网页正文与模型推理字段均不得直接播报。

### 模型与网络边界

LM Studio 官方接口对比显示：`/v1/chat/completions` 与 `/v1/responses` 支持自定义工具，原生 `/api/v1/chat` 不支持同类自定义工具。[接口对比](https://lmstudio.ai/docs/developer/rest)、[工具调用说明](https://lmstudio.ai/docs/developer/openai-compat/tools)。因此首版优先为现有兼容 Chat Completions 适配器增加工具消息及完整调用解析；保留无工具对话的原生路径。已有本机记录显示当前配置模型在兼容端点曾耗尽 512 个输出 token 仍无正文，所以先做真实模型 smoke test：工具调用格式、回传后的最终正文、超时与取消都通过后，才对该模型启用 Agent。不能根据服务宣称支持工具就视作模型质量已达标。其他 LLM 后端逐个验证，不默认协议名称相同就有相同工具表现。

搜索凭据应由系统凭据库保存，不写入角色包、日志或仓库。网页读取只接受 HTTP(S)，首版可收紧到 HTTPS；请求和每次重定向都校验解析后的目标地址，拒绝回环、私有、链路本地及其他受保护地址，并限制响应大小、内容类型、连接时间与跳转次数。提取纯文本并截断后再交给模型，页面中的“忽略之前指令”等内容始终按不可信资料处理；工具参数必须由本轮用户问题与已登记搜索结果约束，不能因网页文字扩大查询范围。搜索服务失败、限流或断网时显示错误并回到普通对话能力，不无限重试。

长期记忆只应从用户明确提供的信息产生个人事实候选。搜索结果、网页内容及模型对网页的复述不应进入“关于用户”的自动事实提取；来源数据不作为已确认记忆保存。若保留查询轮次用于会话连续性，要与现有 `memory_turns` 的提取/压缩流程隔离，并验证删除、关闭记忆及取消时不会复活旧数据。现有长期记忆行为与边界见[长期记忆](23-long-term-memory.md)。

本阶段暂不接入任何产生外部副作用的工具；后续若扩展工具集合，需要重新定义逐项权限和确认规则。P3-W 的任务及通过条件见[路线图](05-roadmap.md)。

## TTS、口型和打断

Provider 能力分别报告 `stream_text_in`、`stream_audio_out`、`cancel`、`phoneme_timestamps`、`voice_clone`。CosyVoice 上游提供双流能力，但各版本、部署方式和设备实现要单独验证；不把宣传的延迟数字当成本机指标。[CosyVoice](https://github.com/FunAudioLLM/CosyVoice)

首版口型从即将播放的 PCM 计算 RMS 包络，平滑后驱动 `mouth_open`；用实际已播放 sample count 对齐画面，不从 TTS 请求时间或网络收包时间起算。播放停顿时嘴应合上。后续有音素时间戳和对应 blendshape 时再做多音素口型。

取消顺序：

1. 核心递增 generation_id，音频回调停止读旧 generation 的输出并清空待播队列。
2. 取消 LLM 网络流，通知 TTS 停止，关闭口型并让角色进入聆听姿态。
3. 所有迟到片段按 turn/generation 丢弃，不能再次排队播放。
4. 不支持硬取消的推理在工作线程完成或独立进程内结束；新轮次不等待旧音频播放。关闭 HTTP 连接不保证服务已释放 GPU，另行监控后端健康与资源。

P4 全双工在 VAD 前加入 AEC，参考信号取实际播放流；需要处理重采样、输出设备延迟、时钟漂移和蓝牙切换。播放期间出现语音只作打断候选，结合 AEC 后的近端语音检测确认。效果未通过时自动使用耳机或半双工模式，避免宠物声音唤醒自己。

## 加速抽象：按任务选择模型实现

不能只写一个 `Device = Metal | CUDA | ROCm` 就覆盖整个系统。渲染后端与推理后端各有自己的设备上下文，不共享 wgpu 纹理、CUDA pointer 或假设零拷贝。

每个可运行变体记录：`task/model_id/revision/sha256/format/tokenizer/frontend/runtime/runtime_version/provider/device/dtype/input_shapes/streaming/license`。缓存编译产物时还包括驱动、目标 GPU 和 shape profile。转换精度或后端后重新测质量。

探测顺序：枚举候选 → 检查安装库/驱动/硬件 → 检查模型与 provider 契约 → 加载并 warmup → 验证输出和时延 → 选择 → 运行监控。向 UI 显示“实际使用”的后端和回退原因，而不只展示“机器有 GPU”。

| 平台/硬件 | 渲染路径 | 语音/推理候选 | 验证要点与回退 |
| --- | --- | --- | --- |
| macOS Apple Silicon | wgpu Metal | sherpa/ORT CPU 基线；ORT CoreML、MLX、PyTorch MPS 或服务式 Metal 实现为候选 | CoreML 不是通用 Metal EP；模型/算子/shape/分区要匹配，失败回已验证 CPU 变体 |
| macOS Intel（后置） | wgpu 可用后端实测 | CPU；其他后端按硬件检查 | 不承诺 MLX GPU 能力，需独立打包与性能档 |
| Windows NVIDIA | wgpu DX12 等已验证后端 | ORT CUDA；必要时 TensorRT；外部 LLM/TTS 服务 | CUDA/cuDNN/驱动和导出兼容；CPU 兜底 |
| Linux NVIDIA | wgpu Vulkan | CUDA / TensorRT / PyTorch CUDA | 发行版、库版本、设备权限和显存预算 |
| Linux AMD | wgpu Vulkan | ORT MIGraphX 或模型支持的 PyTorch ROCm | GPU/OS/ROCm 支持矩阵及算子覆盖；不是任意 Radeon 自动可用 |
| Windows AMD/Intel | wgpu DX12 | ORT DirectML 或其他已验证提供程序；CPU | DirectML 有执行配置约束；不默认 ROCm 可用 |
| Linux Intel/CPU | wgpu Vulkan 或可用图形后端 | CPU；OpenVINO 作为按模型验证候选 | 未达性能档时限制模型规模，保留基本互动 |

此表是适配候选矩阵，只有 CPU 基线和后续逐项通过的组合才进入发行支持列表。sherpa 对某个模型和 provider 的封装支持也需检查；ORT 有某个 EP 不表示 sherpa 任意模型能使用它。[ORT EP 总览](https://onnxruntime.ai/docs/execution-providers/)、[CoreML](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html)、[DirectML](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html)

AMD 注意：官方说明 ROCm EP 自 ONNX Runtime 1.23 移除，建议迁移 MIGraphX；不能新建一个笼统的 `ort-rocm` 后端并假设最新版本支持。[ROCm EP 变更](https://onnxruntime.ai/docs/execution-providers/ROCm-ExecutionProvider.html)、[MIGraphX](https://onnxruntime.ai/docs/execution-providers/MIGraphX-ExecutionProvider.html)

## 本机推荐调度

M4 Max / 48 GB 的起始策略：

- 点击或待机关键词命中后启用 VAD；ASR 先测 int8 CPU。KWS 开启时只在待机常驻。
- LM Studio 使用其自行管理的加载和硬件策略；应用仅知道外部服务健康，不能强制获得系统级 GPU 优先权。
- TTS 先选择本地 CPU 可达到实时速度的语音；CosyVoice/高质量服务加入后与 LLM 比较并发和串行两种时延。
- 应用内部用每设备预算与并发 semaphore 控制自己拥有的任务；在外部服务无资源遥测时采用保守并发上限。
- 初始给系统及其他应用保留至少 12 GB 余量；剩余预算包含模型、KV cache、纹理和临时峰值，按实测修订，禁止无限加载多个语音模型。
- 内存压力时先停止预取、限制上下文、卸载自管闲置模型、降低渲染 fps，再回退轻量模型。LM Studio 未开放的卸载能力不强行模拟。

GPU 不可用时切换 CPU 需对应可运行模型变体，不能只改一个配置枚举。重试/切换发生在语句边界；用户收到明确的状态说明。CPU 也无法实时运行的模型退到轻量 TTS 或文字，基础桌宠保持运行。

## 服务协议与封装

原生 sherpa 调用先放 Rust 专用工作线程，通过受支持 Rust 绑定或 C ABI 接入；FFI 对象构造/销毁和线程归属集中封装。可选重模型服务独立 Python 环境，避免把 CUDA、ROCm、PyTorch 和所有语音模型依赖塞进同一个发行包。

自建 TTS 服务控制面可用本机 HTTP，音频用带 metadata 的 WebSocket/二进制流；定义有界队列和最大待播时长（初始 3 秒），慢消费者触发背压，不能无限缓冲。仅监听回环地址，由父进程注入短期访问凭据；模型目录和服务配置不接受 LLM 自由修改。

P3 开发版允许连接用户已有服务；P6 发行版才确定哪些 runtime 随包、哪些作为可选组件。不要把 `/Users/ncy/Projects` 或开发环境绝对路径写死进产品。

## 测量标准

以下均为初始目标，基线为 M4 Max、48 GB、单角色 30 fps、固定短上下文与已 warmup 的指定模型。冷启动另测；每次记录模型 hash、tokenizer、runtime、provider、输入长度和并发场景。

| 指标 | 目标 / 测量定义 |
| --- | --- |
| 点击响应 | 角色点击到 UI 聆听反馈 p95 < 300 ms；模型冷加载另测 |
| 端点等待 | 正常句末静音约 600 ms，可调；不能混入 ASR 运算耗时 |
| ASR | 5 秒中文语音，端点确定后识别 p95 < 500 ms，比较 CER |
| LLM | 请求发出到首个可显示文本增量 p95 < 800 ms；冷启动独列 |
| TTS | 首个完整分句提交到可播首段 p95 < 600 ms，持续合成 RTF < 1 |
| 端到端 | 用户实际说完到回复可听首音 p95 目标 < 3 秒，包含分句等待 |
| 停止播放 | 已检测打断事件到设备输出停止 p95 < 150 ms；蓝牙另列 |
| 口型 | 与实际播放时间偏差 p95 < 100 ms |

分项 p95 不可简单相加当作端到端 p95。测试集至少含 100 条中文命令/闲聊、不同停顿、背景电视和宠物自声、30 次打断、耳机/内建扬声器/蓝牙切换；FAR 测试至少采集多个 8 小时背景段并报告样本量。先固定 CPU 质量基线，再比较加速后的精度、时延、资源与功耗变化。
