# Windows NVIDIA：CUDA 接入设计与验证计划

设计日期：2026-09-30。分支：`windows-nvidia`。基线：`windows` 的 `d096278311fce2b34cd4b22d34c2fe4c157fe84f`（`fix(windows): preserve certificate byte array during signing`）。

**当前状态：分支与验证设计已建立，应用仍使用 CPU；CUDA 原生库接入、模型推理、性能及发行验收尚未完成。** 本文的目标和通过标准是拟定的实验门槛，不是实测成绩。Windows CPU 链路的既有验收见 [Windows 开发交接](windows-build-progress.md)。

## 1. 范围与交付目标

本分支只推进 Windows 10/11 x64、MSVC、NVIDIA GPU 的 sherpa-onnx CUDA 推理。首台验证设备为当前 RTX 5070。后续迁移验证仍限定 Windows NVIDIA；AMD、Intel GPU、macOS、Linux、DirectML、CoreML、MLX 不纳入本轮设计和验收。

沿用 Tauri 主程序、独立 wgpu 角色宿主、CPAL、现有语音端口和半双工会话。工作目标是建立可测量、可诊断的 SenseVoice ASR / ZipVoice TTS CUDA 路径，比较实际收益，再决定默认设备策略。VAD、KWS 和预录互动音频继续沿用 CPU / 播放路径。LLM 继续由 Ollama 或 LM Studio 管理，本分支不修改其推理运行时。

本轮按以下交付物推进：

- 带版本和 SHA-256 的原生运行时及模型清单。
- 不依赖麦克风或 LLM 的设备探测、ASR 文件识别、TTS 文件生成验证工具。
- 每任务的设备选择、加载/warmup、实际执行证据、CPU 回退和用户可见状态。
- 独立的 Windows NVIDIA 便携测试包、完整依赖清单和跨 NVIDIA 机器验收记录。
- CPU/CUDA 的质量、耗时、显存、LLM 共用 GPU 结果，以及默认启用或保持可选的决定。

## 2. 已检查的基线与尚未验证的条件

2026-09-30 的只读检查得到以下结果；硬件和驱动快照只用于本轮测试，不作为其他机器的固定要求。

| 项目 | 当前证据 | 含义 |
| --- | --- | --- |
| GPU | `nvidia-smi` 和 CUDA Driver API 均返回 RTX 5070，计算能力 12.0，约 12 GB 显存 | 驱动可访问设备；不等于模型已运行 |
| CUDA 驱动 | 616.92；`cuInit(0)` 成功；驱动 API 版本 13040 | CUDA 13.4 是驱动能力，不能当作已安装 Toolkit 版本 |
| Windows 设备信息 | WMI 返回 RTX 4070，与 CUDA 枚举不同 | 测试保存两者；推理设备按 CUDA 枚举确认，不推断差异原因 |
| CPU | i7-12700KF，12 核 / 20 逻辑处理器 | 固定 CPU 线程配置建立可比较基线 |
| 运行库 | 未在标准目录及当前 PATH 找到现代 CUDA/cuDNN 运行库，未找到 `nvcc` | 依赖可加载性未确认；不能据此宣称机器所有位置都没有运行库 |
| Rust 依赖 | `sherpa-onnx = 1.13.8`，`default-features = false`，`static` | 默认预编译原生库是当前 CPU 构建路径 |
| 配置 | `VoiceSettings::validate()` 拒绝 GPU 开关；ASR/TTS/VAD/KWS provider 为 `cpu` | 设置和设备选择尚未接线 |
| 模型 | SenseVoice `model.int8.onnx`；ZipVoice `encoder.int8.onnx`、`decoder.int8.onnx`；Vocos | 本轮未检查实际权重图，也未做 CPU/CUDA 对照 |
| TTS 交付 | 生成回调只检查取消，生成返回后才向播放端交付 PCM | 当前仍是分句后整句合成；CUDA 不会自动变为流式音频 |
| 模型生命周期 | 设置 revision 未变化时复用语音 pipeline | 可以复用 CUDA session；冷启动必须单独统计 |
| 打包 | `package_local.py` 只复制两个 EXE，不收集推理 DLL | 现有便携包流程不足以分发共享 CUDA 依赖 |

相关代码：[`Cargo.toml`](../crates/voice-session/Cargo.toml)、[`lib.rs`](../crates/voice-session/src/lib.rs)、[`local.rs`](../crates/voice-session/src/local.rs)、[`voice.rs`](../apps/desktop/src/voice.rs)、[`package_local.py`](../tools/windows/package_local.py)。

## 3. 原生运行时与构建方案

### 3.1 首选固定版本的共享 GPU 库

初始继续使用 sherpa-onnx Rust/C API 1.13.8，候选原生包为官方 `sherpa-onnx-v1.13.8-cuda-12.x-cudnn-9.x-win-x64-cuda.tar.bz2`。构建验证时才下载、校验和解包，保存于 Git 忽略目录；本次文档工作不安装依赖。

本机缓存的 `sherpa-onnx-sys 1.13.8/build.rs` 已确认：

- 没有单独的 Rust `cuda` feature。使用 `shared` 链接，并通过 `SHERPA_ONNX_LIB_DIR` 指向 GPU 包的 import library / DLL 目录。
- `static` 和 `shared` 互斥，不能在现有 `static` 依赖上简单追加 `shared`；只改成 `shared` 仍会自动下载常规共享库，不会自动取得 GPU 包。
- `SHERPA_ONNX_ARCHIVE_DIR` 按默认 archive 文件名查找，不能把 GPU 包改名成普通包来规避选择。
- 构建脚本只复制指定 lib 目录顶层的 DLL。CUDA/cuDNN 的依赖闭包和最终发行复制仍由项目打包器负责。

GPU 构建使用独立 target/cache/package 目录，防止复用 CPU 产物。若保留两种构建，设计互斥的项目构建选项，构建时检查冲突；不修改 Cargo 缓存内的 crate 源码。

官方 1.13.8 标签的 Windows GPU CMake 配置当前指向 ORT 1.28.2 CUDA 12 包。**实际发布 archive 中 ORT 的版本、DLL 布局和依赖仍须在 N1 核验，不以源码 URL 代替二进制证据。** 原生包、Rust 绑定、import library、ORT 与 provider DLL 成套固定；不从不同版本临时拼装。

针对 RTX 5070，首轮选择 CUDA 12.8 或更新的匹配 CUDA 12 运行库，以及支持 Blackwell 的 cuDNN 9 具体版本。最终小版本、驱动下限、MSVC runtime 和 DLL 清单在 N1 测试前冻结。运行预编译程序无需 `nvcc`；源构建时才安装对应开发工具链。驱动支持 CUDA 13.x 不代表应换用 CUDA 13 的 DLL。

### 3.2 依赖加载与 CPU 回退的边界

先实现明确标识的 NVIDIA GPU 包，并保留原 `windows` CPU 包作为独立构建基线。本阶段不设计一个自动兼容所有硬件的平台发行包。

候选依赖至少包括 sherpa C API DLL、ORT DLL、ORT CUDA/shared provider DLL、实际依赖的 CUDA/cuDNN DLL、MSVC runtime；具体名称和版本由 archive 清单与 PE 依赖检查确定。安装或随包分发前核对对应再分发条款。应用使用受控的包内 DLL 路径，不依赖 Python 环境、系统自带 `onnxruntime.dll` 或用户临时修改 PATH。

区分两类失败：

1. **主程序已启动、原生运行时可加载**：CUDA provider 创建、warmup 或推理失败时，可以销毁失败 session，在语句边界用已验证 CPU 模型重建一次，报告原因。
2. **Windows loader 因导入 DLL 缺失而无法启动 EXE**：Rust 错误处理尚未执行，不能宣称能在应用内部回退 CPU。测试启动器须能检查依赖、保存诊断并提示使用独立 CPU 包；若以后要求同一 EXE 无 CUDA 依赖也能启动，再单独设计延迟加载或语音子进程。

也不能仅以 sherpa 构造成功判断 CUDA 生效：上游存在 provider 不可用时记录日志并退回 CPU 的路径。

## 4. 模型、设备与资源设计

### 4.1 模型变体

| 任务 | 对照与候选 | 初始策略 |
| --- | --- | --- |
| ASR SenseVoice | CPU int8；GPU 库 CPU int8；CUDA int8；CUDA FP32，必要时再验证 FP16 | 先维持 CPU 默认，N3 达标后再选择 |
| TTS ZipVoice + Vocos | 同样比较两种 CPU 基线、CUDA int8、CUDA FP32；FP16 单列实验 | 优先验证 TTS CUDA 收益 |
| Silero VAD | 现有 CPU 模型 | 保持 CPU |
| Zipformer KWS | 现有 CPU 模型 | 保持 CPU，检查 GPU 实验未破坏监听 |
| 预录互动音频 | WAV 播放 | 沿用 CPAL；无推理加速 |

先用官方、适配 sherpa 元数据及 tokenizer 的浮点模型；找不到对应版本时明确标记“模型未准备”，再设计导出。不把任意上游 ONNX 文件视为可直接替换。FP16 转换须保留原模型并重新验证质量；不从 int8 模型恢复浮点权重。

现有 ASR 自定义路径不包含配套 tokens 选择；TTS encoder/decoder 文件名写死为 int8。设计独立 manifest，明确每个变体的模型、tokens、lexicon、espeak 数据及 vocoder 路径，不通过字符串替换文件名猜测变体。模型 manifest 至少记录：任务、模型 ID/版本、SHA-256、dtype、opset、输入 shape、采样率、tokenizer/frontend、支持 provider、来源及许可。

上游 ZipVoice 导出使用动态 MatMul 量化；ORT 1.28.2 CUDA 算子表未列出 `DynamicQuantizeLinear`，`MatMulInteger` 列出 int8×int8。现有 int8 图可能在 CPU 执行关键算子并产生设备拷贝。N2 必须检查实际模型图、优化后分区及 profile；文件名和 GPU 利用率不能替代算子证据。

### 4.2 每任务配置与实际状态

ASR、TTS 分别支持 `cpu / cuda / auto`，选择明确的 CUDA device ID。兼容旧设置：原来的 `false` 映射为 CPU；`true` 表示请求 CUDA，而非已验证可用。配置校验按选中的任务后端与能力进行，GPU 校验与尚未接入的 AEC 分开。

每个任务报告请求设备、选定模型变体、provider 初始化结果、CUDA 设备名称、warmup 状态和回退原因。实验 profile 另记录 CPU/CUDA 分区；产品 UI 不把部分图在 CUDA 执行显示为“全部 GPU”。现有 Rust 绑定是否能直接暴露所需信息在 N1 核查；缺失时补充最小探测边界或辅助工具，不伪造实际设备状态。

首轮一次仅执行一个应用自管的 GPU 语音任务，复用 warmup 后的 session。原生调用留在专用工作线程；音频回调不做推理。CUDA 错误不无限重试；失败后释放 session，至多一次 CPU 重建，CPU 也失败则保留文字/基础互动并报告故障。

约 12 GB 显存不能预先承诺与任意 LLM 共存。按模型测量驻留、workspace 和瞬时峰值；初始给系统、角色渲染及其他应用保留至少 2 GiB 的实验余量，实际预算以 N4 峰值修订。ORT 的 arena 限额不等于整个进程的显存上限。应用不强制卸载外部 LLM；遥测缺失时采用已验证的保守设备配置。

初轮保持当前整句 PCM 交付，以隔离 CUDA 收益。音频回调转发/流式播放作为独立后续变量，不能同时修改后再把首音改善全部归因于 CUDA。取消依旧按 generation 丢弃旧结果；当前 native 推理能否及时停止需实测，不能承诺能立即中止 GPU kernel。

## 5. 分阶段验证与通过标准

各阶段必须留下证据后才进入下一阶段；允许记录失败并调整候选，调整依赖或模型后重跑受影响阶段。相对性能门槛为首轮决策标准，测试前冻结，测试后不得为通过而降低。

| 阶段 | 工作及输出 | 通过标准 |
| --- | --- | --- |
| N0 CPU 基线 | 当前分支构建 CPU 包；固定 ASR/TTS、线程和语料；记录质量与耗时 | 文件识别/生成和现有桌面语音能运行；有可重复数据，不能只引用历史通过记录 |
| N1 原生库与设备 | 校验下载 hash、二进制版本/架构、DLL 依赖、CUDA 枚举、provider 注册及真实模型 warmup | Rust 1.95.0 MSVC 链接通过；加载正确包内 DLL；目标设备可执行一次实际推理；版本清单完整 |
| N2 模型兼容 | 检查量化图、tokens/前端；导出 profile、节点/分区、Memcpy 与耗时 | 候选输出合法；主要耗时子图确实落到 CUDA；未通过项明确排除。CUDA int8 可以失败或无收益，不能据此伪报加速 |
| N3 独立对照 | 按下述方法比较质量、ASR 延迟、TTS 首音/生成/RTF、显存 | CUDA ASR p95 至少改善 20%，或 CUDA TTS 生成 p95 至少改善 25%，按任务分别决策；不达标保持该任务 CPU |
| N4 LLM 与桌面闭环 | LLM 空闲、驻留、生成场景；真实输入、播放、停止、再唤醒、角色渲染 | 相对同场景 CPU 包端到端 p95 不退化超过 5%；无 OOM、串轮或 UI 冻结；冷启动另列 |
| N5 故障和打包 | DLL/provider/模型/显存故障；路径迁移；干净 Windows NVIDIA 机器 | 已启动进程的失败按设计回退/报错；缺导入 DLL 由启动器诊断；离开源码目录可运行；依赖和资源清单可核验 |
| N6 稳定性与支持范围 | 60 分钟及至少 50 轮闭环，30 次打断，10 次模型重建；另一台 NVIDIA 设备迁移 | 无崩溃、旧音频复活或持续内存增长；完成支持列表和默认设备决定。仅单机通过时只标记本机实验可用 |

N3 还须满足质量条件：ASR CER 相对相同语料的 CPU 基线绝对恶化不超过 0.5 个百分点，并审阅全部变动转写；TTS 无 NaN、空输出、严重爆音或截断，盲听至少 30 对输出，在发音/内容/克隆音色方面至少 90% 被判为无明显退化。随机合成输出不做逐字节一致性要求。

产品延迟目标继续参考 [语音与硬件加速](04-voice-and-compute.md)：5 秒输入 ASR p95 < 500 ms、TTS 首个分句到可播音频 p95 < 600 ms、RTF < 1、说完到可听首音 p95 < 3 s。它们是独立的体验目标；通过相对加速门槛也可能未达到产品目标，报告中必须分别标明。

## 6. 测量方法与场景矩阵

### 6.1 数据与计时

- ASR 至少 100 条带人工转写的中文语音，覆盖短命令、闲聊、3/5/10/20 秒输入、停顿和噪声；额外保留中英文混合专项。文件验证统一为实际重采样后的 16 kHz 单声道 PCM。
- TTS 至少 60 条文本，短/中/长三组各 20 条；固定参考 WAV、准确参考文字、速度、`num_steps = 4`。任何改变步数或参考音频的实验另列，不能混入设备对照。
- 每个候选独立进程启动，先 warmup 至少 3 次，再对完整数据集测量 3 轮；轮换 CPU/CUDA 顺序控制温度和后台负载。冷启动至少独立启动 10 次，记录依赖加载、session 创建、首次推理，不混入 warm p95。
- CPU 线程固定后再比较；CPU 原生包与 GPU 原生包的 CPU provider 都测，区分 ORT 版本/构建差异和 CUDA 收益。
- ASR 计时从 PCM 已准备到 final 转写；不含 VAD 等待。TTS 同时记录请求到生成完成、首个 PCM 交付、播放开始、真实可听首音；设备缓冲造成的差异单列。
- RTF = 合成耗时 / 输出音频时长。端到端由同一单调时钟测量说完到可听首音；不能把分项 p95 相加。可听首音采用固定有线设备及录音/loopback 对齐方法，注明该方法的声学与设备延迟边界。
- 性能计时关闭 verbose/profile；另跑同一代表样本采集 profile。保存每样本原始数据、样本数、p50/p95/max、模型与运行时 hash、失败数；不能只汇报最快的一次。

### 6.2 必测场景

| 场景 | 固定条件 | 检查重点 |
| --- | --- | --- |
| 语音独立 | 无外部 LLM GPU 负载 | 判断模型本身是否值得加速 |
| LLM 驻留 | 固定 LLM 模型、量化、上下文及 GPU 配置，不生成 | 显存余量、CUDA warmup 与分配失败 |
| LLM 生成 | 相同配置实际生成，记录有效输出 token 和 TTFT | 共享显存/计算、TTS 首音和 LLM 延迟变化 |
| 桌面闭环 | 单角色固定帧率、真实麦克风、有线扬声器/耳机 | 说完到首音、帧间隔 p95、点击响应、停止播放 |
| 冷热切换 | 第一次会话、连续会话、设置保存、模型重建 | 避免每轮重载、资源释放与缓存失效 |
| 压力与失败 | 受控显存压力、缺模型/依赖、provider 初始化失败 | 故障不会造成无限重试、旧轮次结果覆盖或卡死 |

先测 TTS CUDA + ASR CPU，再测 ASR CUDA + TTS CPU，最后才测两者 CUDA。N4 记录 CPU/进程内存、整卡与语音进程显存、GPU 利用率/功耗；WDDM 无法获得精确进程显存时明确记为不可用，不能把整卡使用量当作语音模型使用量。

资源稳定性以 warmup 后驻留为起点，检查 10 次重建及 50 轮后的内存趋势。缓存导致的稳定平台可接受；持续增长或超出已固定预算必须定位，不按结束时总量简单判为泄漏。

## 7. 故障与发行验收

| 注入条件 | 预期行为 / 验证证据 |
| --- | --- |
| 缺 CUDA/cuDNN/provider DLL | 记录准确缺失依赖；若无法启动 EXE，由启动器给出诊断；不得宣称应用内 CPU 回退已执行 |
| GPU/驱动不可访问，provider 创建失败 | 已启动的应用停止该任务 CUDA 尝试，重建已验证 CPU 变体一次或报告不可用 |
| int8 关键计算留在 CPU | profile 标记混合执行及耗时；排除“CUDA 已全量加速”的结论 |
| 模型损坏、tokens/采样率不匹配 | 失败原因明确，不无限重试，不错误复用旧 session |
| 显存不足 | 当前轮次失败/回退可诊断；释放失败对象；不强制改变外部 LLM 设置 |
| 生成中停止并立即开启新轮次 | 旧 PCM 不播放；测播放停止和新轮次等待 native 任务的时间 |
| 修改设置、睡眠恢复、音频设备切换 | 正确重建/报错，没有旧设备句柄和重复 worker；半双工与 KWS 状态可恢复 |
| 包移动到含空格/中文的目录，从其他工作目录启动 | 找到包内 DLL、角色、模型、参考音频；不依赖开发目录或 Python 环境 |

打包新增专用 NVIDIA 输出标识和运行时清单，收集实际依赖闭包；运行时 DLL 与模型原包保持版本关联，避免扫描开发机 PATH 后随意复制。仓库只记录公共来源、版本、hash 和构建步骤，权重、音频、原始日志、用户存档、凭据及机器路径继续留在 `artifacts/local/`、`models/local/` 等忽略目录。

CI 首轮验证格式、构建、普通测试、DLL/resource manifest 和打包；标准 Windows runner 没有 GPU 推理结果，不能代替 N1–N6。沿用手动 workflow 时必须显式选择 `source_ref = windows-nvidia`，当前默认仍为 `windows`；后续新增 GPU 构建选项和独立 cache key 后再形成 NVIDIA 发行流程。暂不发布 CUDA 支持声明。

## 8. 实施顺序与结果记录

1. N0：重建 CPU 基线，准备有来源和 hash 的固定语料，定义结果字段。
2. N1：准备运行库，完成独立设备/模型探针和真实 warmup，冻结依赖清单。
3. N2–N3：准备模型 manifest，优先验证 TTS，再验证 ASR；选择达标变体。
4. N4：接入设置、实际设备状态、模型生命周期和回退，测真实桌面与 LLM 共用 GPU。
5. N5–N6：补齐启动器/打包/CI，做故障、迁移和长期验收，形成支持范围。

以下工具接口属于待实现设计，当前仓库不能直接执行：`voice-device-probe`（版本/DLL/CUDA/warmup 报告）、`voice-asr-bench`（文件语料识别及 CER/计时）、`voice-tts-bench`（固定文本和参考音频生成 WAV/计时）、`voice-bench-report`（汇总指标与模型/provider 证据）。工具通过显式 manifest 和参数运行，默认不访问麦克风、不调用 LLM、不播放声音；桌面验收另外执行。

结果文件按 `artifacts/local/windows-nvidia/<run-id>/` 保存，至少包含：环境及版本 JSON、依赖/模型 SHA-256 清单、原始每样本 CSV、质量对照、ORT profile、资源采样、故障日志和汇总 Markdown。每次修改模型/依赖都使用新 run-id。

每样本 CSV 建议字段：`run_id, commit, task, case_id, model_sha256, runtime_id, requested_device, selected_provider, partition_report_id, device_id, cold, repetition, input_duration_ms, text_chars, cpu_threads, num_steps, elapsed_ms, first_pcm_ms, playback_start_ms, audible_first_ms, output_duration_ms, rtf, process_rss_mib, process_vram_mib, total_gpu_mib, error`。字段不适用或不可测时为空，不能用零代替。环境文件另记录驱动、GPU、Windows、音频设备、LLM 配置和语料版本。

| 阶段 | 当前状态 | 结果/下一项 |
| --- | --- | --- |
| 环境初查 | 已检查 | CUDA Driver API 可访问 RTX 5070；完整运行库及推理未验证 |
| N0 | 待执行 | 重测 CPU 基线及准备固定语料 |
| N1 | 待执行 | 下载/核验固定 GPU 运行库，Rust 链接及实际 warmup |
| N2–N3 | 待执行 | 模型图、质量、独立性能及分区证据 |
| N4 | 待执行 | 桌面闭环与 LLM 共享 GPU |
| N5–N6 | 待执行 | 故障、便携包、迁移及稳定性 |

最终决定按任务分别记录为“本机 CUDA 可选”“支持列表内默认 CUDA”“保持 CPU”或“未通过”。至少另一台 Windows NVIDIA 设备完成迁移验收后，才扩大支持列表；不能从 RTX 5070 的结果推断所有 NVIDIA 架构可用。

## 9. 官方参考

以下资料于 2026-09-30 核查；后续升级须重新核对版本与支持条件。

- [sherpa-onnx Windows CUDA 安装与预编译包](https://k2-fsa.github.io/sherpa/onnx/install/windows/build-cuda.html)
- [sherpa-onnx 1.13.8 Windows GPU ORT 构建配置](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.8/cmake/onnxruntime-win-x64-gpu.cmake)
- [sherpa-onnx 1.13.8 provider 初始化及 CPU 回退](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.8/sherpa-onnx/csrc/session.cc)
- [ONNX Runtime CUDA EP 依赖、配置及性能说明](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html)
- [ONNX Runtime 1.28.2 算子支持表](https://github.com/microsoft/onnxruntime/blob/v1.28.2/docs/OperatorKernels.md)
- [ZipVoice 动态量化导出源码](https://github.com/k2-fsa/ZipVoice/blob/master/zipvoice/bin/onnx_export.py)
- [NVIDIA 驱动与 Toolkit/架构矩阵](https://docs.nvidia.com/datacenter/tesla/drivers/latest/cuda-toolkit-driver-and-architecture-matrix.html)
- [NVIDIA cuDNN 支持矩阵](https://docs.nvidia.com/deeplearning/cudnn/backend/latest/reference/support-matrix.html)
- [NVIDIA Blackwell 二进制兼容说明](https://docs.nvidia.com/cuda/blackwell-compatibility-guide/index.html)
