# P0 技术验证记录

记录始于 2026-09-16；最终验收更新于 2026-09-18。**P0 Gate 按本机技术试件条件通过，可进入 P1。** 下文保留各轮历史结论，当前状态以本表及末尾最终验收为准。

## 验收状态

| 项目 | 当前状态 | 本轮证据 / 剩余工作 |
| --- | --- | --- |
| P0-01 锁定版本 | 通过 | Rust 1.95.0、Mocari =0.3.1、wgpu =30.0.0、winit =0.30.13；Cargo.lock；本机 release 构建成功 |
| P0-02 角色加载 | 通过（自动检查及用户目视验收） | 眼口上下限、13 表情网格及损坏资源报错通过；用户确认完整三轮目视测试及深浅背景正常。参考运行时对照、物理保留后续专项 |
| P0-03 原生窗口 | 通过（本机、用户确认范围） | 用户确认修改后的角色在深浅背景显示正常；文档透明区穿透、普通拖拽和悬停不干扰输入符合要求，接受点击/拖拽取得焦点与粗略命中余量 |
| P0-04 进程桥接 | 通过（本机试件自动验收） | Tauri 2.11.5 接入真实 winit/Mocari 宿主；正常退出、父进程死亡清理、崩溃恢复、三次失败停止重启及错版本拒绝通过；发行包和特殊 UI 场景未覆盖 |
| P0-05 平台试验 | 通过（本机核心试件条件） | 无 AX 三屏显示/吸附及插拔获用户确认；授权后收到终端窗口移动和大小通知并读取到几何变化。AX 120 秒复验已有通过 summary；关闭/撤权、混合 DPI 和正式跟随集成仍待验证 |
| P0-06 性能记录 | 通过（单宿主 30 分钟基线） | 1800.01 秒正常退出，51,678 次 present；CPU 中位数 8.2%，RSS 中位数 110.08 MiB，CPU 提交耗时 p95 1.812 ms。平均 28.71 次 present/s；未测 GPU 完成时间或实际帧间隔，未证明严格 30 fps 目标 |

## 环境与版本

- Apple M4 Max，48 GB；`aarch64-apple-darwin`。
- macOS 27.0，build 26A428；SDK 27.0。
- 用户完成 Xcode 许可接受后，`xcrun --show-sdk-version` 成功；使用 `/Applications/Xcode.app/Contents/Developer`，没有修改全局开发目录。
- `rustc 1.95.0 (59807616e 2026-04-14)`；`cargo 1.95.0 (f2d3ce0bd 2026-03-21)`。
- 顶层固定 wgpu 30.0.0；本次 Cargo.lock 内 wgpu-core、wgpu-hal、wgpu-types 等传递依赖为 30.0.1，不能把整个依赖树描述为全是 30.0.0。
- GPU 实际后端：Metal / Apple M4 Max；不是 noop renderer。
- 显示器：物理 3456 × 2234、scale factor 2、物理原点 (0, 0)。尚未验证工作区边界和屏幕坐标翻转。

完整版本由仓库根的 `rust-toolchain.toml`、`Cargo.toml` 和 `Cargo.lock` 记录。命令及 24 个输入文件的大小/SHA-256 保存在本地 `artifacts/local/p0/environment-and-inputs.json`。

## 角色与表情

输入为外部本地目录 `Nahida_1080`，直接只读加载。没有修改源 manifest、复制角色到 Git 或下载其他角色。

| 检查 | 观测 |
| --- | --- |
| Mocari runtime | 成功加载 83 个参数、153 个 Drawable |
| 贴图 | 4 张 1024 × 1024；CPU 解码 RGBA 合计 16 MiB，不等于程序总内存 |
| 遮罩 | 26 个 Drawable 带遮罩，GPU clipping plan 生成 5 个 context |
| 眼口 | ParamEyeLOpen、ParamEyeROpen、ParamMouthOpenY 的 min/max 共 6 次设置、网格更新和有限坐标检查成功 |
| 表情 | 13 个文件均由 Mocari 解析并应用，全部参数 ID 存在；每次先重置为默认值，无累积混合 |
| 原清单缺项 | Expressions 登记数 0、HitAreas 数 0；试件通过目录扫描显式发现表情 |
| 动画范围 | 本次为静态参数/表情切换，未运行物理、呼吸、连续动作或业务语义映射 |

GPU 图集共 17 组，每组左浅右深。顺序如下；不能只靠文件名确定业务情绪。

| Case | 内容 | 检查结果 |
| --- | --- | --- |
| 00 | 默认姿态 | 角色完整可见 |
| 01 | 左眼闭合 | 姿态输出可见 |
| 02 | 右眼闭合 | 姿态输出可见 |
| 03 | 嘴部张开 | 姿态输出可见 |
| 04 | Angry.exp3.json | 参数存在、网格更新、渲染输出成功 |
| 05 | Halfeyes.exp3.json | 同上 |
| 06 | HandChange.exp3.json | 同上 |
| 07 | Happy1.exp3.json | 同上 |
| 08 | Sad1.exp3.json | 同上 |
| 09 | Sad2.exp3.json | 同上 |
| 10 | Shy.exp3.json | 同上 |
| 11 | StarEye.exp3.json | 同上 |
| 12 | Wink.exp3.json | 同上 |
| 13 | black.exp3.json | 同上 |
| 14 | kusa.exp3.json | 同上 |
| 15 | mouthchange.exp3.json | 同上 |
| 16 | shy_normal.exp3.json | 同上 |

已检查整张深浅图集与默认姿态大图，未观察到明显几何破损。**这只是本次目视结果，没有 Cubism/VTube Studio 同姿态参考图，因此不宣布完整外观一致性通过。**细小表情差异、半透明发丝、物理摆动仍需对照检查。

渲染图不是桌面截图：它们从本程序 GPU 输出回读，再离线合成深浅背景，不能作为原生桌面透明合成或跨应用点击的证明。本机 UI 自动化无法读取试件应用窗口，最终两项仍需独立桌面实测，不能用图集替代。

## 实际发现：alpha 合成模式

首次窗口启动返回错误：`premultiplied alpha unavailable: [Opaque, PostMultiplied]`。

Mocari 0.3.1 shader 输出预乘 RGB；本机 wgpu Metal Surface 却只报告 `Opaque` / `PostMultiplied`。因此调整试件：先渲染到预乘场景纹理，在最终 pass 中按 Surface 契约转换；alpha 为零时输出零，避免除零。不修改 Mocari 源码、不切换主分支、不静默用不透明 Surface。

调整后，实际选择 `PostMultiplied` / `Bgra8Unorm`，窗口完成了全部姿态绘制，无 wgpu validation 错误。仍须用已知 alpha 的简单色块，在原生深浅背景上校准边缘，再决定最终宿主方案。当前没有依据宣称透明输入 Gate 已通过。

实现依据为本机发布包中的 `mocari-0.3.1/src/render/shaders/live2d.wgsl`、`wgpu-types-30.0.1/src/surface.rs` 与 `wgpu-hal-30.0.1/src/metal/{adapter,surface}.rs`；版本由 Cargo.lock 固定。

## 短时性能基线

以下为 release 单宿主数据，不包含 Tauri/WebView、语音、LLM，也不是完整桌宠内存。其他桌面负载没有隔离。

| 指标 | 实测 | 范围 |
| --- | --- | --- |
| CPU 模型加载 | 8.89 ms | 一次观测，含解析/PNG 解码/初始网格；不含预检和 GPU；未清空文件缓存，不称磁盘冷启动 |
| CPU 网格更新 p50 / p95 / max | 0.142 / 0.190 / 0.216 ms | 300 次默认参数 update_meshes；不是动画帧耗时 |
| 首次观测 GPU 初始化 | 355.96 ms | 首次成功窗口运行；未控制 shader/驱动缓存 |
| 后续窗口初始化 | 13.80 ms | 独立 60 秒采样运行；不当作首次启动成绩 |
| 基线运行时长 / 提交帧 | 60.03 s / 1,750 | 约 29.15 次 present/s；不是显示器实际呈现帧统计 |
| CPU 提交耗时 p95 | 2.001 ms | CPU render/submit/present 调用；不含 GPU 完成等待，不是帧间隔 p95 |
| CPU 中位数 | 7.8% | macOS ps 进程 %cpu，每 2 秒采样，排除前 5 秒 |
| RSS 中位数 / 采样最大值 | 100.91 / 100.94 MiB | 进程 RSS；不是完整加载期的高水位，也不是独立显存统计 |
| 较长试跑 | 120.02 s / 3,489 帧，正常退出 | 仍不足 30 分钟 |

基线每 3 秒切换一个姿态，其余时间重画静态网格，默认通过 WaitUntil 约 30 Hz 唤醒。截图运行单独进行，其回读/PNG 写入不进入上述 60 秒基线。当前静态场景仍逐帧绘制；降低待机重绘频率是后续优化候选，不能据现有数据直接称为瓶颈定论。

测量结束后确认无 `p0-probe` 残留进程。这只证明本次定时退出，不涵盖主进程异常退出和 IPC EOF 清理。

## 可复现检查

已通过：

- `cargo build --release --locked`
- `cargo fmt --all -- --check`
- `cargo test --release --locked`：6 项异常输入测试
- `cargo clippy --release --locked --all-targets -- -D warnings`

异常输入覆盖：JSON 损坏、引用缺失、绝对路径、父目录越界、符号链接逃逸、损坏 moc3；均非零退出并输出可读错误，没有测试用例触发 panic。这不代表完成任意损坏文件的全面模糊测试。

复现命令见 [工具说明](../tools/p0-probe/README.md)。本地证据位于 `artifacts/local/p0/`，被 Git 忽略：

- `nahida-audit.json`：完整参数范围、表情参数映射与 CPU 数据。
- `environment-and-inputs.json`：环境和输入摘要。
- `captures/case-00.png` 到 `case-16.png`、`gallery.png`、`gallery-index.json`。
- `window.stderr.log`：120 秒试跑；`capture.stderr.log`：截图运行。
- `measurement/report.json` / `host.stderr.log`：60 秒 CPU/RSS 采样及事件。

这些文件可能含本地角色派生图和绝对路径，不进入公开仓库；只提交工具、锁文件和本报告的汇总结果。

## 下一轮 P0 顺序

1. 透明合成校准：用户确认模式 2 在深浅背景均与 REF 目视一致；macOS/Metal 角色输出已改为保留预乘颜色。剩余是修改后的真实角色边缘与外部深浅背景视觉回归。
2. 输入方案：按用户最新要求，启动/悬停不抢焦点，点击/拖拽允许角色取得焦点，并接受粗略命中余量。文档场景普通拖拽和透明区穿透已获人工验证；不再安排非激活窗口改造。快速跨边缘操作和其他应用场景作为补充回归。
3. Tauri + 独立宿主：版本握手与大小限制、错版本拒绝、正常退出、EOF、崩溃恢复、重启预算。
4. 单屏工作区及脚底锚点吸附；无 AX 继续可用。用户授权后观察一个目标窗口移动，不自动申请或更改授权。
5. 固定上述配置后跑满 30 分钟，增加实际帧间隔和 GPU 耗时；双屏/混合 DPI、负坐标、拔插、Spaces、睡眠唤醒保留待测清单。

## P0 步骤 1：透明合成校准明细

校准程序为 `tools/p0-probe/src/bin/alpha-calibration.rs`，参考图案在 `calibration_fixture.rs` 中程序生成。它创建不透明背景窗口与透明子窗口，子窗口渲染同一图案的 LIVE 版本，背景窗口绘制按 straight-alpha 公式计算的 REF 版本；窗口位置、尺寸和缩放会输出 JSON。该程序没有读取角色、屏幕截图、网络或系统权限。

本机实测：

- 背景 Surface：Metal / `Bgra8Unorm` / `Opaque`。
- 透明 Surface：Metal / `Bgra8Unorm` / `PostMultiplied`；`PreMultiplied` 不在 capabilities 中。
- Retina scale factor 2；背景 2000×1300 physical，透明子窗口 1760×1000 physical。
- 校准窗口能正常显示在浅色和深色参考背景上；`1`/`2` 模式可通过键盘切换；`S` 可切换阴影，`Esc` 可退出。
- 之前记录的 STRAIGHT/PREMULTIPLIED 优劣判断撤回：截图观察与文字记录存在冲突，需重新保存两种模式的同条件对照证据。不能将这个记录作为 alpha 验收通过依据。
- 子窗口与父窗口位置对齐，截图中内容和参考区域同时可见；由于窗口截图经过缩放，未做像素级差异统计。后续需要原生窗口截图或屏幕采样来量化误差，并检查窗口阴影外扩。

当前证据不足以确认“当前 Surface 必须做 unpremultiply”，P0-03 Gate 保持未通过。Mocari 0.4.0 已下载做版本对照，Cargo.lock 仍固定 0.3.1；其 shader 与 0.3.1 没有发现可解决该 Surface alpha 差异的改动，因此本轮没有升级版本。

## 2026-09-17：首选输入方案第一轮

新增 `P0_INPUT=dynamic` 实验入口，固定整窗模式保留用于对照。AppKit `mouseLocationOutsideOfEventStream` 在穿透状态下采样窗口局部指针位置，转换为左上原点的 logical points；`pressedMouseButtons` 检测松键。轮询约 60 Hz，渲染仍约 30 Hz，使用 WaitUntil 而非忙循环。当前没有实现远近自适应采样。

输入区域是窗口比例定义的头部椭圆与身体矩形，退出边界增加 6 logical points 滞回。它们只是粗略验证范围，不代表纳西妲完整轮廓；尚未完成与姿态、缩放的视觉对齐。窗口收到左键后锁定输入并请求系统拖拽，松键后解除；从其他应用开始的拖拽不在中途开启接收。不使用 GPU alpha 回读，也不请求 AX 权限。

验证结果：

- 3 个输入状态测试通过：穿透后重入/拖拽锁定/松键恢复、外部拖拽不接管、边界滞回。另 7 个现有测试通过；修复并行资源测试临时目录可能重名的问题。
- release 构建、Clippy 通过。
- 本机无 AX 授权，动态模式运行 120.01 秒，3523 帧，CPU render/submit/present 调用耗时 p95 1.44 ms。日志：本地忽略目录 `artifacts/local/p0/input-dynamic.jsonl`。这不是 GPU 帧耗时或输入延迟测量。
- UI 工具无法识别直接运行的 CLI；随后打包为 Input Probe.app，进程启动成功，但按路径和 bundle ID 连接均超时。Finder 可正常连接。未完成真实鼠标操作，不能据此认定点击穿透、拖拽或快速边缘点击通过。
- winit 0.30.13 的 `WinitWindow::canBecomeMainWindow` 和 `canBecomeKeyWindow` 均固定返回 true。`with_active(false)` 不足以保证点击不抢焦点；本轮未实现原生非激活窗口适配。这是代码层面的缺口，与 UI 工具连接超时分别记录。

下一步仍在 P0：实现可验证的原生非激活窗口承载方式，先用可分发几何试件取得跨应用点击、拖拽和键盘焦点证据，再校准角色命中区域。保留当前输入状态机；不因此改动 Mocari 或提前进入 P1。轮询期间快速跨边缘点击存在竞争窗口，须实测，不能由单元测试推定通过。

## 用户手动验证回报与决策

本节更新此前待复核结论，以用户手测及 `manual-input.log`、`manual-alpha.log` 为依据；本轮仅记录判断，未修改渲染或窗口实现。

- 输入：用户报告角色中央拖拽正常、透明区点击可操作文档，鼠标箭头/文本光标切换正常。基本穿透和拖拽在本次文档场景取得人工验证，不代表精确 alpha 轮廓、多应用或快速边缘竞争已验收。
- 焦点：用户报告点击贴图覆盖区域后文字不再进入编辑器。日志有 5 次 `focused=true`，可见命中接收开启后取得焦点、随后发出拖拽请求的事件序列。这证明当前窗口确实能够取得焦点，不能把“透明区域不妨碍编辑”视为“点击角色不抢焦点”。日志含 8 次拖拽请求，不等于 8 次成功拖拽。
- Alpha：用户报告模式 1 在深浅背景均与 REF 不同，模式 2 在两种背景均目视同色同亮度。日志记录两种模式各 46 条、阴影均关闭，支持测试条件一致；颜色匹配依据来自人工观察，日志本身不测颜色。当前机器与试件应选择模式 2（保留预乘颜色）。此前偏向模式 1 的判断错误，不再作为依据。
- 当前角色窗口仍按 PostMultiplied 枚举执行反预乘，因此试件模式 2 通过不等于角色窗口默认路径已经修复。下一步应为已验证的 macOS/Metal 路径采用保留预乘输出，保留 A/B 试件，再在真实角色和深浅背景上回归；不将此结果泛化到其他图形后端。
- 决策：继续当前输入状态机和 Mocari 0.3.1，不因本次结果更换渲染后端。剩余主要工作是局部 alpha 输出修正与非激活窗口适配，然后补角色点击/拖拽后键盘焦点、快速边缘点击和另一应用的验收。P0-03 Gate 仍未通过。锁屏是否导致此前工具超时仍无法从这些证据确定，但人工操作已取得不依赖该工具的有效结果。

## 用户确认交互规则后的实现更新

本节取代前述要求点击/拖拽后维持其他应用焦点的建议：用户明确接受角色被点击或拖拽后取得键盘焦点，后续可用于动作或文字输入；鼠标仅悬停时不得抢焦点。接受角色周围适量的命中余量，保留当前几何命中与输入状态机，不新增非激活原生窗口适配。具体键盘动作/文本 UI 尚未实现。

`Composite::new` 现在接收真实 adapter backend：仅 macOS + Metal 保留预乘颜色（模式 2），其他平台/后端仍按 Surface alpha 模式选择输出。窗口 resize 重建同样使用该策略。校准程序的初始画面、标题及 resize 状态同步使用实际输出策略，仍可按 1/2 做对照。角色启动日志新增 `alpha_output`，避免只记录 Surface 枚举而不记录实际像素输出。Mocari 版本未变。

10 个测试、fmt、Clippy 与 release 构建通过；本地 Input Probe.app 和 Alpha Calibration.app 已重新打包。P0-03 的基本输入在用户确认的范围内符合要求；角色默认输出修正后的原生视觉回归仍需完成，因此不将整个 P0 Gate 标记通过。

修正后原生 smoke test：动态输入模式运行 60.02 秒正常退出，1761 帧，CPU 提交耗时 p95 1.36 ms；日志 `artifacts/local/p0/premultiplied-regression.log` 确认 `backend=Metal, straight_alpha=false`。没有在本轮完成原生画面的人工视觉判定，运行成功不替代视觉回归。

## P0-03 关闭与下一步

用户最终确认：模式 2 修正后的真实角色拖到浅色、深色窗口背景均显示正常。结合前述人工输入验证，P0-03 按最新交互要求标记通过。不将此结论扩展到多屏、其他操作系统或所有快速边缘操作。整个 P0 仍未通过：P0-04 进程桥接、P0-05 平台试验和 P0-06 长时稳定性尚有工作。下一步进入 P0-04。

## P0-04 第一轮：独立管道试件

新增 `tools/p0-ipc-probe`，实现版本 1 的 hello/ready、ping/pong、shutdown/stopped；逐行 JSON 在解析前限制 256 KiB（含换行），校验会话、连续序号、请求 ID 与 payload 类型。异常输入明确输出 stderr 原因并非零退出。暂不定义业务动作或宣称有 request_id 去重能力。

release 构建和 Clippy 通过。`verify.py` 的 12 个协议/EOF 场景以及额外的父进程强制终止测试通过；每个进程测试设有 5 秒超时。父进程死亡测试验证写端关闭后子进程释放继承输出管道；接入真实 Tauri/winit 后还需验证窗口销毁、超时与句柄继承。下一步是 Supervisor 的握手/心跳超时和有界重启预算，再将生命周期协议接到渲染事件循环。

## P0-04 第二轮：Supervisor

用户授权删除的根目录手测脚本 test.sh、test2.sh 已删除（原为未跟踪文件）。新增可复用 Rust Supervisor 库与 p0-supervisor 命令：容量为 1 的请求/响应通道、后台阻塞管道读写、响应身份校验、超时 kill/wait、正常退出确认与进程退出期限、Drop 兜底清理。重启创建新会话，退避 250/500 ms，60 秒滚动窗口累计 3 次失败后终止。

9 项新增测试通过，包含真实宿主与 Python 故障子进程；原有 10 项 Rust 测试也通过。Clippy 和 release 构建通过；原有 12 个协议/EOF 场景与父进程死亡退出验证通过。release Supervisor 对真实管道试件完成 3 次每秒心跳并正常退出，attempts=1。

这些结论只覆盖直接子进程和有限监督试跑，不代表 Tauri 应用崩溃或 winit 窗口恢复已经验证。控制管道不能继续继承给宿主后代，当前没有进程树管理。下一步接入真实渲染宿主与 Tauri 后台生命周期，处理可取消监控、主线程关闭投递，再验证 UI 退出、宿主恢复及父进程死亡无残留窗口。

## P0-04 第三轮：真实渲染宿主与 Tauri 试件验证

本轮只执行测试，不扩展开发功能。`p0-probe host MODEL` 现在作为测试目标运行真实 winit/Mocari 渲染宿主；`tools/p0-tauri-probe/verify.py` 使用本地模型执行三个场景：直接宿主正常 handshake/ping/shutdown、EOF、错误版本拒绝；Tauri 试件正常自动退出、强杀渲染宿主后重新连接、强杀 Tauri 父进程后检查没有残留宿主。

结果：release workspace 构建通过；协议脚本 12 个场景通过；真实渲染宿主三种场景通过；Tauri normal/recover/parent-death 三种场景通过。每次 Tauri 场景均检查 `bridge_ready` 的宿主 PID，恢复场景确认 PID 改变，父进程死亡场景确认宿主退出。测试过程使用本地模型，只把日志写入被忽略的 `artifacts/local/p0/bridge/`。

Clippy 本轮未通过：`tools/p0-tauri-probe/src/main.rs` 的 `ExitRequested` 嵌套 if 被 `clippy::collapsible_if`（`-D warnings`）拒绝。该项属于上一轮开发代码的静态检查问题，本轮按用户要求不修改开发代码；在修复前不能宣称全量质量门通过。工作区保留上一轮尚未提交的桥接实现变更，未在本轮提交或推送。

这些测试证明直接渲染宿主和当前 Tauri 试件的生命周期路径可工作，不证明正式 Tauri 产品配置、签名、托盘退出、进程树继承句柄或 30 分钟稳定性。P0-04 的核心试件场景已取得证据；全量 P0 仍受 Clippy 修复、正式宿主集成、P0-05 和 P0-06 约束。

## P0-04 收尾：修复与完整桥接复验

恢复开发后修复 Clippy 嵌套 if 问题，并补测完整 Tauri 进程的三次失败重启上限。该测试揭示两处此前未覆盖的问题：父子进程共享 stderr 时 JSON 分段写入会交错，导致测试漏读 ready；重启预算耗尽的失败日志未可靠反映为进程失败退出码。

修复：父子结构化日志先完整格式化，再整行写入；验证脚本不再忽略完整但损坏的 JSON 行。Tauri 使用 run_return 并额外保留后台 Supervisor 的最终结果，避免平台事件循环正常返回覆盖业务失败。第三次失败后现已验证为退出码 1，并且没有残留已知宿主 PID。

最终验证：fmt、19 项 Rust 测试、Clippy（-D warnings）、release workspace 构建通过；12 个独立协议场景及父进程死亡管道测试通过；真实渲染宿主正常协议/EOF/错版本三项与 Tauri 正常退出/崩溃恢复/父进程死亡/重启预算四项通过。正常退出由自动计时器调用 Tauri exit，经过 ExitRequested 防止提前退出，再由后台清理宿主；不是仅测试 kill。日志位于 `artifacts/local/p0/bridge/`。

P0-04 按路线图的本机试件条件标记通过，下一步为 P0-05 工作区/坐标/屏幕吸附。此结论不要求提前实现正式 P1 应用，但不覆盖签名 sidecar、托盘、点击关闭按钮/Cmd-Q 的人工操作、长时间拖拽对心跳的影响、暂停父进程或任意进程树。此次日志已枚举到 3 个显示器，尚未据此认定多屏坐标和混合 DPI 通过。整个 P0 仍未完成。

## P0-05 第一轮：工作区与底边吸附

用户限定原生测试必须在 Desktop 2 或新建桌面进行，禁止在 Desktop/Desktop 1 测试。工具未能确认 Mission Control 桌面名称，因此本轮没有启动角色、桥接或吸附窗口；已请求用户确认测试桌面，确认前只进行代码和无窗口测试。

新增 opt-in `P0_SNAP=1`，需同时开启 `P0_INPUT=dynamic`。在本窗口开始的拖拽松键后读取 NSWindow.screen 与 NSScreen.visibleFrame，以 AppKit 逻辑点做坐标翻转、距离计算和 setFrameOrigin，不混用跨屏 physical pixels。工作区每次重读，不缓存 Dock/菜单栏预留区域；模型下缘距底边最多 12 logical points 时吸附，并将窗口水平方向限制到当前工作区。

锚点采用中性网格最低边界的拟合位置，固定比例避免表情切换导致窗口跳动；这是 P0 几何锚点，不是模型作者标注的语义脚底。当前仅实现松键后的底边吸附，不包含左右边缘吸附、持续跟随工作区变化、外屏拔插恢复或他应用吸附。读取工作区和移动自身窗口不请求 AX 权限，实际无 AX 行为仍需桌面实测。

4 项纯几何测试通过：负坐标显示器的 Y 翻转往返、以模型锚点而非透明窗口底部吸附、远距离与无效输入不移动、右边界限制。原有 3 项输入测试、Clippy 和 release workspace 构建通过。未将编译/单元测试视为 P0-05 原生验收通过。

平台依据：[Apple visibleFrame](https://developer.apple.com/documentation/appkit/nsscreen/visibleframe)（工作区随 UI 设置变化）、[setFrameOrigin](https://developer.apple.com/documentation/appkit/nswindow/setframeorigin(_:))。原生验证需核对实际位置是否受系统窗口约束、锚点是否需要角色标注校准，以及不同显示器上的坐标与缩放。


## P0-05 当前桌面实测记录（第一次尝试，已被后续复验取代）

首次当前桌面尝试曾启动 `P0_INPUT=dynamic P0_SNAP=1` 的本地 Input Probe，并执行了一次角色拖拽；该记录只保留为早期诊断，已被下面的用户人工复验取代。

观测到：

- `ready` 日志枚举到 3 个显示器；本次宿主报告 Metal、scale factor 2，并显示 `ax_trusted=false`。
- UI 工具可连接角色窗口并完成一次拖拽动作；日志包含 `drag_requested` 和后续 `hit_region`，说明窗口仍在运行并继续采样。
- macOS 输出 `Window move completed without beginning`，当时日志没有 `screen_snap` 事件，也没有可比较的 before/after 窗口坐标。
- 该结果暴露了当前测试方法的限制：`drag_window()` 的系统拖拽开始状态与 UI 自动化拖拽的鼠标序列没有形成可验收的窗口移动证据。没有修改代码来掩盖或推断这一点；过程日志已保存在被忽略的 `artifacts/local/p0/snap-manual.log`。

## P0-05 用户人工复验结果

用户随后在当前桌面按 `P0_INPUT=dynamic P0_SNAP=1` 运行试件，并将纳西妲窗口与终端窗口的行为进行对照。用户报告：左侧、右侧和上侧屏幕边缘均可正常吸附；屏幕底部会穿过，且与终端窗口表现一致。这是系统行为对照结果，不等于所有位置都触发自定义底边吸附，也不改变脚底锚点的设计要求。

日志复核结果：

- 当时共 33 条 `screen_snap`，涉及 `Built-in Retina Display`、`AG324UWS4R4B` 和 `Sidecar Display (AirPlay)` 三个屏幕名称。该路径会被后续测试覆盖；此处保留上一轮读取结果。
- 读取到的工作区包括 `(0, 33, 1728, 999)`、`(-97, -1050, 1920, 1050)` 和 `(-1164, 345, 1164, 772)`，记录了负坐标与不同显示器工作区。
- 其中 1 条记录 `snapped=true`，before `[-845, 528]`，after `[-845, 536]`，说明底边吸附入口实际发生过坐标调整；其余记录 `snapped=false` 与用户观察到的边缘停靠/穿透并不矛盾，系统拖拽本身也会把窗口带到屏幕边界。
- `ax_trusted=false`，说明屏幕边缘吸附不依赖 AX 授权。日志中仍有 2 条 macOS `Window move completed without beginning`，但未阻止后续 screen_snap 记录或用户观察到的边缘行为；该 warning 作为拖拽实现的低优先级诊断保留，不将本次边缘验收判失败。

基于用户对照测试，P0-05 的“无 AX 屏幕边缘吸附”在当前多屏桌面环境取得人工通过证据。左右与上侧表现来自系统拖拽约束，当前试件只实现自定义底边吸附。后续插拔复验见下文；混合 DPI 的完整回归以及工作区变化后的持续跟随尚未验证。纯几何测试和本次人工结果不代表 P0-06 长时稳定性通过。

## P0-05 三屏插拔反馈与 AX 试件（2026-09-18）

用户补充确认：MacBook Pro 内置屏、外接显示器和 iPad Pro 扩展屏均已测试，插拔、窗口显示及吸附正常。将这些场景记为当前设备配置下的人工通过；没有实现或据此认定持久化显示器恢复策略已完成。

重新读取 `artifacts/local/p0/snap-manual.log`，文件已被新一轮测试覆盖：运行 300.01 秒、呈现 9120 帧，CPU render/submit/present 调用耗时 p95 为 1.47 ms；不是 GPU 完成时间或帧间隔。20 条 `screen_snap` 分别来自内置屏 11 条、外接屏 8 条、Sidecar 1 条，均 `ax_trusted=false`、scale=2。工作区分别为 `(0,33,1728,994)`、`(-97,-1050,1920,1050)`、`(-1164,345,1164,772)`。本轮没有非 JSON 警告行，全部 `snapped=false`，因此不能用本轮日志新增证明脚底吸附发生过。插拔成功以用户实际观察为依据，日志没有专门的热插拔事件；三个 scale 均为 2，不覆盖不同 scale 的混合 DPI。

下一步为路线图要求的“获准后观察一个目标窗口移动”。新增独立 Swift 测试源 `tools/p0-probe/ax-observer.swift`，直接使用 SDK 的 AXObserver，不改角色运行代码。只读取指定应用当前焦点窗口的几何并固定观察该窗口，不读取标题/文本、不移动目标、不自动申请权限。记录移动、大小、销毁和最小化通知；目标销毁、最小化、退出或检测到权限撤销时停止观察。该试件验证 API 可行性，尚未集成 Rust 平台层或角色跟随/回落动作。

`xcrun swiftc -warnings-as-errors` 编译通过；当前执行环境返回 `ax_trusted=false`，以退出码 2 正常降级退出，无授权弹窗。参数错误以退出码 1 拒绝。授权后的通知、几何变化以及关闭/撤权路径仍待人工执行，不提前标为通过。操作见 [AX 测试说明](../tools/p0-probe/README.md#p0-05-ax-窗口观察实验)。

## P0-05 AX 授权后人工验证（2026-09-18）

用户确认完成授权并操作窗口。读取 `artifacts/local/p0/ax-manual.jsonl`：`ax_trusted=true`、`prompt_requested=false`，目标为 `com.apple.Terminal`，初始窗口几何为 `[960,566,580,385]`。移动、大小、销毁、最小化四项通知订阅均返回 0。

共收到 391 条通知，其中 `AXMoved` 171 条、`AXResized` 220 条；包含初始几何在内，共有 242 个不同位置和 127 组不同大小。最后记录的窗口几何为 `[402,-1050,582,387]`。通知携带了实际变化的坐标和尺寸，满足路线图“获准后观察一个目标窗口移动”的要求，不仅是授权或订阅成功。负坐标已被读取，不据此推断所有显示器坐标转换均正确。

本次日志无 `geometry_unavailable` 或订阅错误，但也没有 `summary` 或 `detached`，读取时试件进程已不存在。最后一条通知距开始观察约 50 秒；这不是进程总运行时间。无法从现有证据确认正常跑满 120 秒、退出码或结束原因，不推断为崩溃或正常退出。关闭、最小化与权限撤销均未取得对应事件证据。

结合此前无 AX 的屏幕吸附及用户三屏插拔结果，P0-05 按本机核心试件条件标记通过。该结论不覆盖正式 Rust AX 集成、角色跟随、权限归属在发行包中的表现以及上述生命周期专项。下一阶段为 P0-06 性能与 30 分钟稳定性验证；P0-02 的参考对照/物理等缺项仍保留，整个 P0 Gate 尚未通过。本次只更新记录，没有修改运行或测试代码。

## P0-05 AX 完整时长复验（2026-09-18）

用户再次完整执行 120 秒。新 `ax-manual.jsonl` 中授权为 true，观察至 summary 间隔 120.09 秒；收到 511 条 `AXMoved`、359 条 `AXResized`，最终 `position_changed=true`、`movement_check_passed=true`。已取得正常完成路径的 summary，补齐上一轮缺失的结束证据；未单独保存 shell 退出码。目标关闭/最小化与撤权专项仍未执行，不重复要求已通过的移动测试。

下一步使用现有 `measure.py` 和 release 原生宿主运行 1800 秒基线，整窗穿透、禁用截图与动态输入/吸附，采样 CPU/RSS 并核验宿主自动退出。测试不包含 Tauri、连续动画、物理、语音或 LLM；CPU 提交耗时不能当作 GPU 完成时间或实际帧间隔。运行结果完成后补记。

## P0-06 30 分钟稳定性基线（2026-09-18）

AX 完整复验通过后，在当前桌面执行 `cargo build --release --locked -p p0-probe --bin p0-probe`，构建通过；使用现有 `measure.py` 启动 1800 秒原生宿主。未修改开发或测试代码。关闭截图、动态输入及吸附，启用整窗穿透；`caffeinate -di` 随采样进程临时阻止自动息屏和睡眠，结束后无对应进程残留。

本次为 Apple M4 Max / Metal，启动枚举到内置屏和外接屏两块显示器，scale 均为 2；不把上一轮 iPad 三屏测试配置套用于此次基线。宿主 AX 为 false，与用户通过终端启动的 AX 观察试件授权归属不同，不影响本次屏幕渲染。其他桌面负载未隔离，且未测锁屏/睡眠专项。

| 指标 | 本次结果 | 解释 |
| --- | --- | --- |
| 运行时长 / 退出码 | 1800.0087 秒 / 0 | 自动退出，`stability_30_minutes_observed=true`，无宿主或采样进程残留 |
| 初始化 | 15.36 ms | 当前缓存状态的一次窗口初始化，不是磁盘冷启动 |
| present 调用数 / 平均速率 | 51,678 / 28.71 次每秒 | 不是显示器扫描输出统计，未达到严格 30 次每秒 |
| CPU render/submit/present 耗时 p95 | 1.812 ms | 不含 GPU 完成等待，也不是帧间隔 p95 |
| 进程 CPU 中位数 / p95 | 8.2% / 9.8% | macOS `ps` 采样，排除前 5 秒；不是整机或 GPU 占用 |
| RSS 中位数 / 采样最大值 | 110.08 / 110.39 MiB | 单渲染进程，不含 Tauri、WebView、语音和 LLM |
| 首五分钟 / 末五分钟 RSS 中位数 | 109.72 / 110.33 MiB | 首段排除前 5 秒；差值 +0.61 MiB，不能据此证明无内存泄漏 |
| 采样数 / 最大间隔 | 893 / 2.022 秒 | CPU/RSS 采样连续，无长间断 |

结束日志含完整 summary，没有非 JSON 警告/错误行。测试每 3 秒切换姿态，其余帧重绘静态模型，不包含连续动画或物理。日志中的 `visual_verdict` 仍要求人工查看；此次运行没有新增逐帧视觉正确性证据。

平均提交速率低于 30：现有时间统计无法区分事件循环调度、系统合成节奏、GPU 工作或其他桌面负载的影响，不将某一项断言为瓶颈。后续需要实际帧间隔分布和 GPU 时间才能定位；静态模型仍逐帧重绘是待验证的节能优化候选。本试件将每帧 CPU 耗时保存到数组，测量自身也有随帧数增加的内存开销，因此小幅 RSS 增量不能直接归因于渲染泄漏。

证据保存在 Git 忽略目录 `artifacts/local/p0/stability-30m-20260918/`：`host.stderr.log`、`report.json`、`analysis.json`、`provenance.json`。后者记录二进制 SHA-256、Git HEAD 和工作区未提交状态，避免将本次二进制等同于已提交版本。`report.json` SHA-256 为 `f98b114d6663e097d58e121c31afb73e82a9e857964c3ae45a65793e7bb2b2c9`。

P0-06 按路线图的单宿主长时稳定性与性能基线记录条件通过；不声称完整产品、30 fps/p95 帧间隔目标或 GPU 性能通过。下一步核对 P0-02 剩余角色验收并汇总 P0 Gate，整个 P0 暂不标记通过，不进入 P1。

## P0-02 剩余验收复核（2026-09-18）

严格按 `05-roadmap.md` 的条件核对：模型可见；眼口参数和 13 个表情逐个检查；损坏资源可报错。模型可见已有多次人工证据。本次重新执行 release `audit`，83 个参数、153 个 Drawable、4 张贴图加载成功；眼口参数上下限 6 项及 13 个表情全部网格更新成功，无未知参数。新结果保存在 `artifacts/local/p0/p0-02-reaudit.json`。

`cargo test --release --locked -p p0-probe --test invalid_assets` 的 6 项全部通过：损坏 JSON、引用缺失、绝对路径、父目录越界、符号链接越界、损坏 MOC。另在临时目录复制本地模型，分别破坏一张 PNG 和一个 exp3 JSON，两项均以退出码 1 给出对应解析错误，无 panic；临时目录自动清理，原资源未修改。结果见本地忽略目录 `p0-02-corruption-checks.json`。未改开发或测试源码。

剩余阻塞项收敛为用户对眼口与全部表情的逐项外观确认。参考运行时完整一致性、物理摆动与连续动画未被路线图列为 P0-02 明确条件，保留后续兼容性专项，不据这些未实现功能无限延长本项验收；不宣称这些能力已经通过。手工测试说明见 [P0-02 目视验收](08-p0-02-manual-check.md)。

源 exp3 参数表确认 `Sad2.exp3.json` 与 `black.exp3.json` 完全相同（`ParamPale` Add 30），两者外观一致是预期。其他自定义参数的检查提示结合源 cdi3 显示名称，不仅靠表情文件名猜测；显示名称是线索，仍需用户目视确认。当前维持 P0-02 待人工验收，整个 P0 Gate 尚未通过。

## P0-02 最终目视验收及 P0 Gate 结论（2026-09-18）

用户完成检查后确认“最终测试显示没有问题，浅色或深色背景都正常”，并提供最终 summary。重新读取 `artifacts/local/p0/p0-02-visual-manual.log`：默认、左右单侧闭眼、张嘴以及 13 个表情共 17 项，每项均出现 3 次，合计 51 条 case，完整覆盖三轮；运行 153.0109 秒、提交 4444 帧，CPU render/submit/present 耗时 p95 为 1.289291 ms，与用户提供的 summary 一致。

程序固定输出的 `visual_verdict=manual review required` 不会随用户反馈改变；此处以用户实际目视结论补充外观证据。结合参数与异常资源自动检查，P0-02 按路线图条件通过。

日志另有一条系统输入法相关消息 `error messaging the mach port for IMKCFRunLoopWakeUpReliable`；其后试件继续运行并输出完整 summary，用户未报告视觉或交互异常。本次未据此判为验收失败，也未定位该消息根因，作为诊断记录保留；本轮未单独保存 shell 退出码。

P0-01 至 P0-06 均已达到各自的本机技术试件通过条件，P0 Gate 通过，可进入 P1 基础 macOS 桌宠开发。本结论不等同于完整产品可发布，不扩大已有测试范围：正式 Rust AX 跟随及关闭/撤权处理、混合 DPI/Spaces/睡眠专项、发行包签名与权限归属、连续动画/物理及参考运行时一致性、GPU 时间和实际帧间隔均保留后续工作。30 分钟基线平均 28.71 次 present/s，仍不宣称严格 30 fps 或帧间隔 p95 目标达成。

下一阶段按路线图先建立 PetCore、AvatarPort、DesktopPort 最小契约与常驻应用生命周期，再接入托盘、显示/隐藏、大小位置和基础交互。本轮只更新验收文档，未启动 P1 实现。
