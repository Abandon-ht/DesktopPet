# Build asset provenance / 构建资源来源

This inventory describes the resource archives used by the manual macOS build. The ZIP checksums in `SHA256SUMS.json` identify exact published bytes. Keep all notices supplied by upstream sources when repackaging. A project's software license does not automatically cover its character art, voice recordings, or model weights.

本清单记录手动 macOS 构建使用的资源包。`SHA256SUMS.json` 固定各发布文件的实际字节。重新打包时应保留上游提供的全部许可与署名说明。项目的软件许可证不自动覆盖角色美术、语音录音或模型权重。

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

No music file is required by the current application or included in these archives. The user's other downloaded 3D character files are not part of the current 2D runtime. / 当前程序不需要音乐文件，本批资源包也不包含音乐；用户另行下载的三维人物文件不属于现有二维运行时。

These provenance notes disclose uncertainty; they are not a license grant. Copyright reports are handled under [POLICY.md](../POLICY.md). / 本清单披露尚未核实之处，不构成授权。版权投诉见[使用与版权说明](../POLICY.md)。
