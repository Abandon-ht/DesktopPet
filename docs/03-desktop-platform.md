# 桌面交互与跨平台设计

## 能力抽象

`DesktopCapabilities` 是平台实测后的能力集合，至少包含 `global_pointer`、`absolute_position`、`external_window_observation`、`input_passthrough`、`workspace_tracking`、`global_shortcut`。不以操作系统名称直接判断全部能力可用。

| 能力 | macOS 首发 | Windows 后续 | Linux X11 | Linux Wayland |
| --- | --- | --- | --- | --- |
| 透明置顶窗口 | AppKit/winit 验证 | Win32 合成验证 | 依赖窗口管理器和合成器 | 依赖 compositor/协议 |
| 点击穿透 | 窗口忽略事件 + 命中状态切换 | 原生输入命中/窗口区域策略 | 输入区域策略 | surface input region；全局恢复需专门设计 |
| 屏幕边缘吸附 | 工作区和屏幕坐标 | 工作区与 DPI | 工作区协议 | 普通窗口不保证可任意定位 |
| 他应用窗口吸附 | AX 授权后观察 | WinEvent + 窗口几何 | EWMH/X11 | 无通用跨桌面保证 |
| 主动占屏 | 非激活 overlay | 非激活 overlay | 按 WM 验证 | 可选 compositor 扩展；普通窗口降级 |
| 语音/养成 | 支持目标 | 可复用 | 可复用 | 可复用 |

Wayland 的输入由 compositor 路由到 surface，普通应用不能假定知道全局鼠标或所有其他窗口几何。XWayland 也不自动赋予观察所有原生 Wayland 窗口的能力。Linux 应拆成 X11、Wayland 基础窗口模式、可选 compositor 扩展三个支持等级。[Wayland 协议模型](https://wayland.freedesktop.org/docs/book/Protocol.html)

## macOS 原生适配

宠物宿主负责自己的 NSWindow/NSView，Rust 通过受限的 AppKit 绑定完成原生调用。窗口创建、焦点、层级、移动和输入模式更新按 API 要求回到主线程；GPU 工作不阻塞窗口事件处理。

首版：无边框、透明、不投影或可控投影、浮动层级、点击宠物不激活设置窗口。启动与鼠标悬停不改变其他应用的键盘焦点；主动点击或拖拽角色允许角色取得并保留焦点，用户点击其他窗口恢复输入。角色可在后续绑定动作快捷键或文本输入，托盘/设置窗口仍可独立取得焦点。全屏 App、Spaces、Mission Control、Stage Manager 分别测试，不用一个“所有工作区可见”标志承诺完全一致行为。

wgpu Surface 查询 alpha modes，选择实际支持的透明合成方式；清屏 alpha=0，确保输出与预乘 alpha 规则匹配。只设置 clear color 并不足以让原生窗口透明。[wgpu 合成模式](https://docs.rs/wgpu/latest/wgpu/enum.CompositeAlphaMode.html)

Tauri 透明 WebView 配置在 macOS 涉及 `macOSPrivateApi` 限制。推荐拓扑中设置 WebView 不需要透明，宠物窗口由独立原生宿主实现；不得从这一配置推导本应用已满足商店审核要求。首个发行目标为 Developer ID 签名及公证的独立应用，签名验证包含所有嵌套可执行文件。[Tauri 配置](https://v2.tauri.app/reference/config/)

## 点击、拖拽与穿透

必须区分“窗口看起来透明”和“鼠标事件穿透”。Tauri 的 `set_ignore_cursor_events` 与 AppKit 的 `ignoresMouseEvents` 都是窗口级控制，不自动根据角色像素 alpha 工作。[Tauri Window](https://docs.rs/tauri/latest/tauri/window/struct.Window.html)、[AppKit 鼠标事件](https://developer.apple.com/documentation/appkit/nswindow/ignoresmouseevents)

首版接受角色周围适量的命中余量，P0 可使用粗略几何区域；后续采用角色包多边形 + 当前变换矩阵，必要时用降采样 alpha 遮罩增强精度。不要每帧同步 GPU readback 计算命中。动画显著改变轮廓时更新 CPU 侧区域或异步遮罩。

1. 宿主采样可用的全局鼠标位置；窗口已穿透时仍需能发现鼠标重新进入角色。不能只依赖该窗口的鼠标移动回调。
2. 鼠标进入可交互区域时恢复接收事件；离开时穿透。近边界设置小幅迟滞，避免频繁抖动。
3. 鼠标按下后锁定此次拖拽交互，拖动时暂停穿透切换；松开再恢复命中检查。
4. 低速移动可 30 Hz 采样、近角色时提高到 60 Hz；P0 必须测试快速点击切换的漏接/误吞，失败时改为稳定原生命中策略或明确的交互模式。

原生视图 `hitTest` 返回空不必然将事件转交给下面另一个应用；Windows 特定 hit-test 返回值也需跨进程实际测试。P0 以桌面和浏览器的真实点击成功作为验收，不仅测本窗口日志。

## 吸附：先屏幕，后其他窗口

“屏幕吸附”只需要显示器与工作区；“其他应用窗口吸附”需要观察目标窗口。二者分别设开关。

统一内部使用顶部为原点的逻辑桌面坐标，携带 monitor_id 和 scale_factor；适配层处理 AppKit 坐标翻转、多屏负坐标和物理像素转换。屏幕底部吸附使用脚底锚点；他应用窗口顶边吸附使用独立的角色接触高度，使角色下半身叠入目标窗口，具体高度按角色配置，不能复用脚底锚点。换屏时重新计算比例；持久化显示器标识和归一化位置，外屏拔掉时回到主屏工作区。

macOS 通过 Accessibility 查询目标窗口并用 AXObserver 订阅位置、大小、销毁等变化，必要时低频补采样。AX 权限未授予时继续屏幕吸附；不循环提示授权。AX observer 支持情况取决于目标应用，必须处理不发事件或不可访问窗口。[AX 信任检查](https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions)、[AXObserver](https://developer.apple.com/documentation/applicationservices/axobserver)

目标记录：进程身份、窗口标识/引用、几何、可见状态、更新时间和短时有效期；不要只存窗口标题。鼠标释放后搜索附近合法边缘，当前实验吸附阈值 24 逻辑像素，以覆盖小幅视觉间隙；脱离时按目标失效、焦点切换或用户主动拖离处理。动画缓动追随及频繁几何更新合并仍属后续优化。

窗口最小化、关闭、跨 Space 或超时不可观察时解除吸附并落回可用屏幕边缘。遮挡关系不明确时允许“只吸附前台窗口”的保守模式。只移动宠物，不移动被吸附的应用窗口。

Windows 路线采用 WinEvent 通知与窗口几何读取；DWM 可见边框和 DPI-aware 坐标需统一。必须过滤自身、不可见、最小化、cloaked 和不合适的系统窗口，并验证高权限窗口限制。[SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)、[DwmGetWindowAttribute](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmgetwindowattribute)

## 主动交互与“占据显示屏”

默认实现为短时视觉表演：角色放大、靠近屏幕中央、伸手、探头、抛出虚拟食物或邀请休息。无需截图、理解屏幕内容或模拟键鼠。屏幕视觉理解可以在未来作为独立可选能力添加。

行为调度检查精力、亲密度、距上次打扰时间、当前语音/拖拽、安静时段与用户设置。初始策略：主动行为间隔至少 15 分钟，占屏最长 8 秒，每小时最多 2 次；全部可配置，属于设计默认值。

- 普通状态使用紧凑宠物窗口，避免常驻全屏透明大 Surface。
- 占屏时切换到当前显示器的临时 overlay；透明区保持穿透，不抢键盘焦点，不强制切 Space。
- 退出采用可见关闭按钮、托盘停止与可配置全局快捷键。只有能收到键盘事件时 Esc 才有效，因此不能把 Esc 作为非激活窗口唯一退出方式。
- 宿主自身执行最长时限；即使主进程或 LLM 卡住，也恢复到紧凑模式。父进程丢失后宿主退出。
- 用户手动免打扰优先；若系统没有可靠的会议/全屏状态能力，则显示能力限制，不假定已识别会议软件。

## 权限与验收

| 功能 | 按需获取的权限或能力 | 拒绝/撤销后的行为 |
| --- | --- | --- |
| 麦克风 | macOS 麦克风用途说明与授权，由实际采集进程处理 | 保留文字与非语音互动 |
| 他应用吸附 | Accessibility | 仅屏幕边缘吸附 |
| 广域输入观察/快捷键 | 依所选 API 验证 Input Monitoring 等要求 | 按钮/托盘控制；停用依赖能力 |
| 屏幕视觉理解（未来） | Screen Recording | 不启动捕获，基础桌宠仍工作 |

从签名后的应用包验证授权归属：主应用负责音频采集；图形辅助进程负责桌面观察时需确认 TCC 对辅助程序的识别。不能假定主应用获准就代表所有子进程获准。

手工测试至少覆盖：单屏/双屏、混合 DPI、外屏拔插、Dock 自动隐藏、全屏 App、切 Space、Stage Manager、快速拖拽、睡眠唤醒、权限撤销、主/子进程异常退出。窗口集成以真实桌面录屏和结果清单验收，纯单元测试不能代替。
