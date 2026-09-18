# P0 原生验证工具

这是技术试件，不是完整桌宠；包含实验性自动命中切换，没有 Tauri、IPC、屏幕吸附和养成功能。
从仓库根目录执行。模型必须指向本地资源，不能把模型或生成的截图提交到 Git。

## 构建与检查

```sh
cargo build --release --locked
cargo test --release --locked
cargo clippy --release --locked --all-targets -- -D warnings
```

`rust-toolchain.toml` 固定 Rust 1.95.0，首次构建由 rustup 安装工具链。
macOS 需可用的 Xcode/Command Line Tools 和 SDK。

## 资源审计

```sh
mkdir -p artifacts/local/p0
target/release/p0-probe audit "/path/to/Nahida_1080.model3.json" \
  > artifacts/local/p0/nahida-audit.json
```

检查引用、路径越界和符号链接逃逸，读取 Mocari 真实参数范围，验证眼口上下限、旁置/已登记的表情及有限网格坐标。直接从外部目录读取；不复制、不修改模型。表情映射保留原始参数和 Blend，尚未赋予 `happy` 等业务语义。

这是可信本地资产的开发审计工具，尚不是可接收任意不可信角色包的正式导入器；资源大小预算、完整文件格式模糊测试和解压限制后续再加。

## 窗口与截图

由 Tauri 监督的真实窗口使用 `p0-probe host MODEL`，通过 stdin/stdout 接收协议，不启用 standalone 的定时退出。完整启动和生命周期验证入口见 [桥接试件](../p0-tauri-probe/README.md)。

```sh
# 默认 60 秒自动退出，也可指定 1–3600 秒。
target/release/p0-probe window "/path/to/Nahida_1080.model3.json" 60

# 整窗穿透模式：包括角色本身，不可拖拽。
P0_PASSTHROUGH=1 target/release/p0-probe window "/path/to/Nahida_1080.model3.json" 60

# 单独的截图运行；不可把这次运行当性能基线。
P0_CAPTURE_DIR=artifacts/local/p0/captures \
  target/release/p0-probe window "/path/to/Nahida_1080.model3.json" 54
target/release/p0-probe gallery artifacts/local/p0/captures
```

窗口为无边框透明置顶试件，启动不主动获取焦点。默认整窗接收鼠标，左键调用系统拖拽；**透明区域也会接收鼠标**。目前没有角色轮廓命中，不能据此验收透明区穿透。按用户确认的规则，点击和拖拽允许角色窗口取得键盘焦点；启动与悬停不应改变焦点。`P0_PASSTHROUGH=1` 只用于隔离整窗穿透能力，需在真实其他 App 上观察点击结果。

每 3 秒一组姿态，51 秒覆盖纳西妲的 17 组：默认、左闭眼、右闭眼、张嘴、按文件名排序的 13 表情。切换前重置参数，不运行物理。`stderr` 输出 ready/case/focus/summary JSON，`stdout` 预留；尚不是 IPC 协议。

Surface 优先选 `PreMultiplied`，否则选择 `PostMultiplied`，两者均不支持则明确失败。Mocari 先画入预乘场景纹理，再用独立 pass 输出；macOS/Metal 根据本机 LIVE/REF 人工校准保留预乘颜色（模式 2），即使 Surface 报告 PostMultiplied 也不执行反预乘。其他后端保留原有按 Surface 模式选择的逻辑。ready 前的 alpha_output 日志记录实际输出策略。深浅背景图集是 **GPU 输出再离线合成**，不等同于 macOS 桌面原生透明效果的验收，也不代替半透明校准色块测试。

GPU 截图每个 case 回读一次，输出含本地角色，默认被 `.gitignore` 排除。`gallery-index.json` 给出图集由左到右、由上到下的顺序，每组左浅右深；空白尾格无含义。截图只包含本程序渲染内容，不抓取桌面或其他应用。

## 性能测量

```sh
python3 tools/p0-probe/measure.py "/path/to/Nahida_1080.model3.json" --seconds 60

# P0-06 30 分钟稳定性；2026-09-18 已完成一轮，复测请使用新输出目录。
python3 tools/p0-probe/measure.py "/path/to/Nahida_1080.model3.json" \
  --seconds 1800 --output artifacts/local/p0/stability-30m
```

2026-09-18 已完整运行 1800.01 秒并以退出码 0 结束：CPU 中位数 8.2%，RSS 中位数/采样最大值 110.08/110.39 MiB，平均 28.71 次 present/s。完整证据见 [P0 实测记录](../../docs/07-p0-validation.md)。基线运行期间保持解锁和固定显示配置，避免睡眠；可用 `caffeinate -di python3 ...` 将自动防睡眠限制到采样进程生命周期。

脚本每 2 秒读取本次子进程的 `ps %cpu,rss`，前 5 秒不进入稳态中位数。macOS `ps` 的 CPU 数值不是 GPU 占用，也不是瞬时整机占用。RSS 是采样值，不是完整加载期间的峰值；单位转换为 MiB。

窗口通过 `WaitUntil` 以约 30 fps 唤醒，不使用忙轮询。帧指标 `cpu_submit_ms_p95` 是 CPU 构建/提交/调用 present 的耗时，**不是 GPU 时间或帧间隔 p95**。当前每 3 秒变姿态，其余帧重画静态网格；不能当作连续动画/物理的性能指标。测量脚本强制禁用截图并使用整窗穿透；不代表 Tauri 进程管理已经实现。

## 透明合成校准（P0 步骤 1）

校准试件只使用程序生成的色块、柔边圆盘和 alpha 渐变，不包含角色资源：

```sh
cargo build --release --locked
python3 tools/p0-probe/package-calibration.py
P0_CALIBRATION_OUTPUT=artifacts/local/p0/calibration \
  open -n "artifacts/local/p0/Alpha Calibration.app"
```

校准应用包含不透明参考窗口和透明子窗口。窗口标题显示当前模式：`1 STRAIGHT (unpremultiply)` 对 Mocari 预乘颜色做反预乘；`2 PREMULTIPLIED (preserve)` 保留预乘颜色；`S` 切换 macOS 原生窗口阴影，`Esc` 退出。两种模式的正确性需根据实际 LIVE/REF 对照判定，不预设结果。

每个 `LIVE / REF` 色块都应匹配；柔边和渐变用于观察半透明像素。实际本机 Metal 只报告 `Opaque` 与 `PostMultiplied`，代码当前默认选后者，macOS/Metal 最终输出保留预乘颜色，校准程序默认显示模式 2。此前模式优劣的文字记录与截图观察存在冲突，已撤回该验收依据；需重新在相同背景、阴影状态下切换两种模式并记录结果，不能仅凭 Surface 枚举判定。

该校准仍不能证明完整 P0-03：窗口阴影与子窗口层级可能影响边缘；透明区穿透和普通拖拽已由用户在文档场景验证；点击/拖拽允许激活角色。实际角色输出、快速边缘操作及其他应用仍有回归项目。`artifacts/local/p0/calibration/` 中的程序生成 PNG 只作本地证据，已被 Git 忽略。

## 剩余验证

完整状态和顺序见 [P0 验证记录](../../docs/07-p0-validation.md)。P0 Gate 已按本机技术试件条件通过，可进入 P1；专项限制见实测记录。

## P0-05 底边吸附实验

用户已允许在当前桌面测试；运行前确认桌面适合手动拖拽。

```sh
P0_INPUT=dynamic P0_SNAP=1 target/release/p0-probe window "/absolute/path/model.model3.json" 120
```

先把角色拖到工作区底边附近，松开鼠标。中性网格下缘锚点距底边不超过 12 logical points 时，试件尝试吸附，并输出 `screen_snap` 日志，含工作区、缩放、移动前后位置和 AX 信任状态。请核对角色下缘与可用区域底边，而非透明窗口矩形底部。远处松键应不移动，重新拖动应立即可用。

目前已有纯几何测试、构建与用户三屏显示/吸附/插拔人工验证；混合 DPI 和 Dock 自动隐藏专项仍待验收；AX 窗口移动观察已有本机通过证据。该功能默认关闭，不影响已有输入/透明验证路径。当前不设置“所有桌面可见”，也不自动切换或识别 Space 名称。

## P0-05 AX 窗口观察实验

本机完整 120 秒人工复验已收到 511 条移动和 359 条大小通知，最终 summary 确认移动检查通过；关闭/撤权路径仍待验证。详见 [实测记录](../../docs/07-p0-validation.md)。

这是独立 Swift 测试工具，验证 AXObserver 通知和窗口几何读取，尚未接入角色跟随。只观察指定应用开始测试时的焦点窗口；不读取标题或文本、不移动目标窗口，也不请求权限弹窗。无需启动角色。

在 macOS 自带“终端”应用中新开一个方便拖拽的窗口，运行：

```sh
cd /Users/ncy/Projects/DesktopPet
mkdir -p artifacts/local/p0
xcrun swiftc -warnings-as-errors tools/p0-probe/ax-observer.swift -o artifacts/local/p0/ax-observer
artifacts/local/p0/ax-observer com.apple.Terminal 120 | tee artifacts/local/p0/ax-manual.jsonl
```

看到 `observing` 后，拖动执行命令的终端窗口到几个明显不同的位置，再调整大小。保持屏幕解锁，先不最小化或关闭目标窗口，等 120 秒结束。不要使用 IDE 内置终端执行此命令，否则目标可能不是执行命令的窗口。切换窗口不会重新选择观察目标。

如果只有 `AX_permission_required`，本次没有开始观察。此功能需要你自行决定是否授予系统“辅助功能”权限：系统设置 → 隐私与安全性 → 辅助功能，允许实际运行试件的“终端”；必要时完全退出并重开终端后重试。TCC 归属可能因启动方式而异，以重跑后的 `ax_trusted=true` 为准；若仍为 false，反馈日志以便检查，不要批量授予其他应用权限。无需屏幕录制或输入监控权限。试件不会更改系统权限。

通过依据：至少收到一次 `AXMoved`，其 `frame` 的 x/y 相对初始位置有变化，最后 `summary.movement_check_passed=true`。大小变化通常产生 `AXResized`；`subscription.error=0` 表示对应订阅成功。仅 `ax_trusted=true` 或订阅成功不能代替移动验收。

主测试完成后可另开一轮，先移动，再最小化/关闭目标，观察是否记录 `detached` 并结束。关闭运行命令的终端可能同时终止试件；要单测目标关闭通知，应改用另一个应用的 bundle ID 作为目标，从终端运行。权限撤销也应单独测试。退出码：0=移动检查通过，1=参数/目标/API 不可用，2=无权限，3=未取得移动证据或权限撤销；使用 `tee` 时管道退出码不一定代表试件，请查看 summary。日志位于 Git 忽略目录，重新运行会覆盖，请按测试场景换文件名保留结果。

# 动态输入实验（P0 本机验收已通过）

```sh
P0_INPUT=dynamic cargo run --release --bin p0-probe -- window /absolute/path/model.model3.json 120
python3 tools/p0-probe/package-input.py /absolute/path/model.model3.json
```

第一条启用全局指针采样、粗略头/身体区域、6 logical points 滞回及拖拽锁定。第二条在忽略的 `artifacts/local/p0/` 下生成 180 秒自动退出的 Input Probe.app，仅保存本地模型路径，不复制角色资产；先构建 release 后打包。应用启动环境通过 Info.plist 启用动态模式。

当前区域不是精确角色轮廓，用户已接受适量命中余量；文档场景的基本穿透与拖拽已通过人工验证，不再要求点击或拖拽后保持其他应用焦点。详见 [P0 记录](../../docs/07-p0-validation.md)。
