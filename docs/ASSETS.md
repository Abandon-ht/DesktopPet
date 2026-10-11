# Build asset provenance / 构建资源来源

This inventory describes the resource archives used by manual macOS, Linux and AX650 builds. The ZIP checksums in each resource Release's `SHA256SUMS.json` identify exact published bytes. AX650 NPU resources additionally have a source-controlled per-file lock. Keep all notices supplied by upstream sources when repackaging. A project's software license does not automatically cover its character art, voice recordings, or model weights.

本清单记录手动 macOS、Linux 和 AX650 构建使用的资源包。各资源 Release 的 `SHA256SUMS.json` 固定发布文件实际字节，AX650 NPU 资源另外使用仓库内逐文件锁定清单。重新打包时应保留上游提供的全部许可与署名说明。项目的软件许可证不自动覆盖角色美术、语音录音或模型权重。

| Archive | Source / 来源 | Current rights status / 当前权利状态 |
| --- | --- | --- |
| `icons.zip` | Existing repository icons, based on the project's Nahida-themed artwork / 仓库现有图标 | Character art permission not verified / 角色美术授权未核实 |
| `avatar.zip` | Local `nahida-touch-v4` pack derived from the public Nahida Live2D files / 基于网络公开纳西妲 Live2D 文件制作的本地 v4 包 | Pack manifest says `unverified`, `redistributable: false`; no independent grant found / 清单标记授权未核实且不可再分发，尚未找到独立授权 |
| `voice.zip` | Project's generated localized interaction WAV/TXT files / 项目生成的多语言互动语音 | Voice/character likeness redistribution status unverified / 音色与角色形象的再分发状态未核实 |
| `models-asr.zip` | [sherpa-onnx SenseVoice conversion](https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09.tar.bz2), based on [WSYue-ASR](https://huggingface.co/ASLP-lab/WSYue-ASR) | Upstream WSYue-ASR displays Apache-2.0; preserve that attribution. Confirm conversion/checkpoint terms separately / 上游 WSYue-ASR 标注 Apache-2.0，保留该署名；转换权重条款仍需单独核对 |
| `models-asr-ncnn.zip` | [sherpa-ncnn SenseVoice conversion](https://k2-fsa.github.io/sherpa/ncnn/sense-voice/pretrained.html) | Same upstream model family; ncnn runtime itself is not shipped / 同源模型系列，默认未附带 ncnn 运行程序 |
| `models-vad.zip` | [Silero VAD via sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx) | Check upstream model terms before redistribution / 再分发前核对上游权重条款 |
| `models-tts.zip` | [ZipVoice conversion](https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/sherpa-onnx-zipvoice-distill-int8-zh-en-emilia.tar.bz2), [Vocos](https://github.com/k2-fsa/sherpa-onnx/releases/download/vocoder-models/vocos_24khz.onnx), espeak data in the upstream archive | Check checkpoint, vocoder and espeak notices individually; ZipVoice checkpoint permission is not confirmed here / 需分别核对权重、声码器和 espeak 说明；此处未确认 ZipVoice 权重许可 |
| `models-kws.zip` | [sherpa-onnx bilingual Zipformer](https://k2-fsa.github.io/sherpa/onnx/kws/pretrained_models/index.html) | Check upstream model terms before redistribution / 再分发前核对上游权重条款 |
| `models-axera.zip` | [SenseVoiceSmall AX650](https://huggingface.co/Abandon-ht/SenseVoiceSmall-axmodel/tree/7cf51476f6236f5dd81f08d9df533483e6845c2f), [ZipVoice distill / Vocos AXERA](https://huggingface.co/AXERA-TECH/ZipVoice.AXERA/tree/aa48d14426f528a63ccb0720edb79651597e5316), matched frontend from the tested sherpa build / 已测试模型与配套前端 | Preserve SenseVoice's supplied license reference and ZipVoice README; include upstream ZipVoice, Vocos and espeak notices. Repository software terms do not establish checkpoint terms / 保留 SenseVoice 原始许可指向、ZipVoice README，附上 ZipVoice、Vocos、espeak 上游说明；不把源码许可当作权重授权 |

AX650's [resource Release](https://github.com/Abandon-ht/DesktopPet/releases/tag/resources-ax650-2026-10-11) adds `models-axera.zip`; existing common resources remain at `resources-2026-09-29`. The AX650 AppImage embeds both resource sets. The [model lock](../tools/ax650/model-resources.lock.json) records the archive hash, 374 files, source revisions and frontend origin. Seven converted AX650 weights are included: SenseVoice 10 seconds, ZipVoice distill encoder and four decoder parts, and Vocos. Tokens, version-1 decoder manifest, lexicon and espeak data are matched to the previous board delivery. No AX630C / AX637 / AXCL weights, standard ZipVoice variant, sample recordings or user reference WAV are selected.

AX650 独立资源 Release 包含 `models-axera.zip`；AppImage 同时内置公共 CPU 资源和这些 NPU 资源。373 个既有模型 / 前端文件在发布前逐一核对旧板端交付包，模型与前端字节保持一致；新增 ZipVoice 上游 LICENSE，扩充 NOTICE 中的 Vocos / espeak 说明。原始 HF 文件通过固定 revision 校验。前端来自已测试 sherpa 构建，逐文件 SHA-256 固定，避免另取词表或 espeak 版本导致不匹配。NPU ZIP 的 SHA-256 为 `19557e8c6b7f981f2b9f352e25775163f5ddc4a08ea50b174eeb1c8c48f2d5ed`。

No music file is required by the current application or included in these archives. The user's other downloaded 3D character files are not part of the current 2D runtime. / 当前程序不需要音乐文件，本批资源包也不包含音乐；用户另行下载的三维人物文件不属于现有二维运行时。

These provenance notes disclose uncertainty; they are not a license grant. Copyright reports are handled under [POLICY.md](../POLICY.md). / 本清单披露尚未核实之处，不构成授权。版权投诉见[使用与版权说明](../POLICY.md)。
