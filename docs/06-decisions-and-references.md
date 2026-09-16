# 架构决策与参考项目

## 决策记录

| 编号 | 推荐决策 | 依据与代价 | 重新评估条件 |
| --- | --- | --- | --- |
| ADR-001 | macOS / Apple Silicon 先发 | 本机具备验证条件；其他平台不能直接视为已支持 | P6 稳定、取得其他目标设备 |
| ADR-002 | 首个角色用 Live2D 纳西妲，Mocari =0.3.1 | 与指定技术和现有资源匹配；需要补表情/区域映射 | P0 出现无法修复的兼容问题，或用户改为三维优先 |
| ADR-003 | Tauri 主程序 + 独立 Rust Avatar Host | 每进程一个原生事件循环；可分别锁定渲染依赖 | IPC/内存实测成为主要瓶颈，且同进程宿主已验证 |
| ADR-004 | PetCore 管理全部养成状态 | 无模型也可运行；便于重放和一致性测试 | 多角色共同世界需要更复杂状态模型 |
| ADR-005 | LLM 只建议动作，核心仲裁 | 不依赖模型输出正确性来维持窗口/库存状态 | 未来增加受限工具时扩展 schema 与权限 |
| ADR-006 | 模型适配器 + 可选推理进程 | 复用 SenseVoice/CosyVoice 经验；接受 Python/C++ 边界 | 某引擎成为长期稳定发行依赖时再考虑更紧集成 |
| ADR-007 | KWS/VAD/ASR/LLM/TTS 各自选设备 | 实际模型和 runtime 不统一；增加能力与变体管理 | 同类任务验证可共用 runtime 时合并实现 |
| ADR-008 | 三维采用 GLB 与独立 Bevy 宿主候选 | 复用骨骼、动画、morph；toon/VRM 扩展需额外开发 | P5 发现材质或性能阻碍，再评估其他原生引擎 |
| ADR-009 | 按键 → 唤醒半双工 → 全双工 | 先建立可测闭环，再解决 AEC 和自声问题 | AEC 实测通过后默认开放扬声器打断 |
| ADR-010 | Wayland 按能力降级 | 无通用跨应用几何和任意全局定位保证 | 特定 compositor 协议与环境验证完成 |
| ADR-011 | 应用与角色/模型包独立版本 | 本地素材存在明确再分发限制；重模型体积大 | 获得清晰分发条件后选择随包资产 |

## 开源项目如何借鉴

这里记录可借鉴的功能和代码边界，不依据 stars 或 README 功能列表承诺实现质量。除 Mocari 发布包外，本轮主要查看仓库介绍、官方接口与部分许可文件，未对下列工程完成代码审计或本机运行测试。

| 项目 | 借鉴内容 | 本项目使用方式 |
| --- | --- | --- |
| [Mocari](https://github.com/Eatgrapes/Mocari) | Rust Live2D 解析、参数/动作/表情、wgpu 渲染 | 指定 0.3.1 直接依赖候选；读取发布包，P0 验证纳西妲 |
| [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) | 原生 ASR/TTS/KWS/VAD，Rust/C API 与 Tauri 示例 | 第一语音引擎；核查本地提交和选定模型兼容性 |
| [Open-LLM-VTuber](https://github.com/Open-LLM-VTuber/Open-LLM-VTuber) | 语音对话、打断、Live2D 表现、provider 拆分 | 借鉴会话和同步设计；不整体搬入 Python 应用架构 |
| [AIRI](https://github.com/moeru-ai/airi) | Live2D/VRM 角色、语音陪伴和模块化 stage | 借鉴角色能力和音画同步；当前项目含 Electron 路线，不能当成现成 Tauri/Rust 框架 |
| [Desktop Pet Engine for macOS](https://github.com/InsiderX-Pro/desktop-pet-engine) | AppKit 透明窗口、alpha 命中、拖拽、主动行为 | 研究 macOS 桌面细节；原生 Swift/帧资产实现与 Mocari 不同 |
| [VPet](https://github.com/LorisYounger/VPet) | 养成、喂食、行为与桌宠交互 | 借鉴玩法和状态组织；WPF 工程不作为跨平台基础 |
| [Lively Wallpaper](https://github.com/rocksdanister/lively) | Windows 桌面集成、显示器与性能情境 | 参考 Windows 实现细节；壁纸层与浮动宠物窗口目标不同 |
| [Bevy](https://github.com/bevyengine/bevy) | Rust 三维场景、glTF、动画与 morph | P5 独立三维宿主候选，不要求 P0 集成完整引擎 |
| [MMD Tools](https://github.com/MMD-Blender/blender_mmd_tools) | PMX/MMD 导入与导出工具链 | 离线资产转换，针对 Blender 版本验证 |
| [CosyVoice](https://github.com/FunAudioLLM/CosyVoice) | 高质量语音及双流推理方案 | 可选服务适配，单独验证平台、延迟和模型版本 |

许可核查必须以实际采用的提交和子目录为单位。Mocari 0.3.1 发布清单标为 MIT；这不覆盖角色资产。Open-LLM-VTuber 的后端、Web 前端与 Live2D 示例资产有各自许可，发布说明也记录许可调整，不能把整个相关生态简单视为同一 MIT 许可。[后端许可](https://github.com/Open-LLM-VTuber/Open-LLM-VTuber/blob/main/LICENSE)、[Web 前端许可](https://github.com/Open-LLM-VTuber/Open-LLM-VTuber-Web/blob/main/LICENSE)、[发行说明](https://github.com/Open-LLM-VTuber/Open-LLM-VTuber/releases)

## 官方资料索引

访问日期均为 2026-09-16。`latest`/主分支链接会变化，实施阶段将选定版本和源码 SHA 固化到依赖记录。下列链接用于支持各文档中的事实；具体架构、阈值、预算和排期属于本项目设计建议。

| 主题 | 一手资料 | 本设计采用的结论 |
| --- | --- | --- |
| Mocari 0.3.1 | [发布包](https://static.crates.io/crates/mocari/mocari-0.3.1.crate) | 直接读取 Cargo、库接口与示例；版本/提交记录见资源审计 |
| Tauri 子进程 | [Embedding External Binaries](https://v2.tauri.app/develop/sidecar/) | 可随应用分发辅助二进制；需按 target 构建 |
| Tauri 窗口 | [Window API](https://docs.rs/tauri/latest/tauri/window/struct.Window.html) | 窗口级穿透和原生窗口操作，不自动提供逐像素命中 |
| Tauri 配置 | [Config](https://v2.tauri.app/reference/config/) | 平台配置和 macOS 透明窗口限制 |
| Rust 前端候选 | [Tauri Leptos 指南](https://v2.tauri.app/start/frontend/leptos/) | 如选全 Rust UI，可使用 CSR/WASM 前端 |
| GPU 透明合成 | [CompositeAlphaMode](https://docs.rs/wgpu/latest/wgpu/enum.CompositeAlphaMode.html) | Surface 支持的 alpha mode 决定可用透明方式 |
| macOS 输入 | [ignoresMouseEvents](https://developer.apple.com/documentation/appkit/nswindow/ignoresmouseevents) | 原生窗口是否接收鼠标事件 |
| macOS AX | [AX 信任检查](https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions)、[AXObserver](https://developer.apple.com/documentation/applicationservices/axobserver) | 跨应用窗口观察有能力/授权边界 |
| Windows 窗口事件 | [SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook) | 监听窗口相关事件，需正确线程和消息循环 |
| Windows 几何属性 | [DwmGetWindowAttribute](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmgetwindowattribute) | 获取 DWM 窗口属性 |
| Wayland | [协议模型](https://wayland.freedesktop.org/docs/book/Protocol.html)、[xdg-shell 协议](https://github.com/wayland-mirror/wayland-protocols/blob/main/stable/xdg-shell/xdg-shell.xml) | compositor 管理 surface 和输入；普通窗口能力不同于 X11 |
| ASR/KWS | [SenseVoice](https://k2-fsa.github.io/sherpa/onnx/sense-voice/index.html)、[KWS](https://k2-fsa.github.io/sherpa/onnx/kws/index.html)、[C API](https://k2-fsa.github.io/sherpa/onnx/c-api/html/index.html) | 区分离线识别、流式识别和关键词检测 |
| LM Studio | [Chat Completions](https://lmstudio.ai/docs/developer/openai-compat/chat-completions) | 可配置的本地兼容接口及流式请求参数 |
| ORT 总览 | [Execution Providers](https://onnxruntime.ai/docs/execution-providers/) | 图由支持的 EP 执行，不保证单一设备覆盖全图 |
| Apple 推理 | [CoreML EP](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html) | 需要核查模型/算子和 EP 配置 |
| AMD 推理 | [ROCm EP](https://onnxruntime.ai/docs/execution-providers/ROCm-ExecutionProvider.html)、[MIGraphX EP](https://onnxruntime.ai/docs/execution-providers/MIGraphX-ExecutionProvider.html) | 旧 ROCm EP 移除；新方案按 MIGraphX 评估 |
| Windows 通用 GPU | [DirectML EP](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html) | 候选执行提供程序，有相应执行约束 |
| 三维形态动画 | [Bevy morph targets 示例](https://github.com/bevyengine/bevy/blob/main/examples/animation/morph_targets.rs) | 三维候选支持 glTF morph 动画，扩展仍需验证 |

## 实施前待补证据

- Mocari 0.3.1 加载纳西妲的实际截图、全部表情、遮罩和透明窗口结果。
- 确定支持的 macOS 最低版本，以及两进程签名、TCC 权限和焦点行为。
- 本地 SenseVoice ONNX 的真实输入/输出契约和可复用程度。
- LM Studio 服务版本、实际模型、上下文长度、资源占用和取消语义。
- 首选 TTS 的中文质量与 CPU 实时率；CosyVoice 的本机或远端部署位置。
- 具体三维模型的绑定工作量，以及可公开分发演示资产的来源。
- Windows/Linux 目标发行版、硬件和真实测试设备。

以上是 P0–P7 内需执行的验证任务，不影响先按当前设计启动 P0。
