# Linux X11 第一阶段开发记录

本阶段按 Linux 构建文档推进“离线构建 → 原生角色窗口与输入 → 屏幕工作区、吸附和存档 → XFCE 托盘及交互测试包”。这是开发基线，尚无 Linux 安装包，不能据此声明所有 Linux 桌面或 Wayland 已受支持。

## 2026-10-01 验证环境

- Ubuntu 22.04、x86_64，云服务器容器。
- TigerVNC 1.12 的原生 X11 `:1`，XFCE / xfwm4 合成器，单屏 1920×1080，缩放 1。
- NVIDIA GeForce RTX 5090，驱动 595.84；角色进程使用 Vulkan。桌面合成器仍使用原有软件渲染，没有更换 X server、驱动或桌面配置。
- Rust 1.95.0，锁定的 winit 0.30.13、wgpu 30.0.0、Mocari 0.3.1、Tauri 2、CPAL 和 sherpa-onnx。
- 角色运行测试使用用户本地的私有角色包；美术资源、语音和模型权重不进入 Git，也不作为可再分发演示包。

## 代码边界

`avatar-host-2d` 在 Linux 明确选择 X11。Wayland 独占会话没有当前阶段所需的定位与指针实现，不能静默当作已支持；Wayland 会话内的 XWayland 也不属于本次实机验证。

新增 X11 边界读取全局指针与左键状态、查询 EWMH 当前工作区，并将窗口和监视器几何换算到同一坐标系。它复用原有角色命中、拖拽归属、位置存档和屏幕底部锚点吸附逻辑。坐标基线采用窗口的 X11 全局缩放，混合 DPI 与多 X screen 尚待实机验证。工作区属性缺失、截断或与监视器无交集时回退到监视器边界；EWMH 全桌面工作区并不表达所有多屏独立面板保留区。

实机测试发现并修正了四项差异：

1. winit 的 `with_active(false)` 在 X11 上不受支持。角色窗口设置 ICCCM `WM_HINTS.input = false`，保持鼠标交互并避免显示时抢走键盘焦点。
2. xfwm4 会给无装饰角色窗口建立父边框。仅清空客户端输入形状，父边框仍会吞掉点击。现在同步角色客户端及其 WM 父窗口的 XFixes 输入形状，在接收鼠标时恢复默认形状，在穿透时清空形状；缓存状态避免反复写入，重建父窗口或缩放后重新同步。不会修改根窗口或其他应用窗口。
3. xfwm4 会忽略初始隐藏窗口发送的置顶请求。显示后重新发送 EWMH 置顶请求，避免透明区点击使普通窗口覆盖角色。
4. X11 的移动请求是异步的。设置明确的用户位置提示以保留首次显示位置；收到窗口移动事件后再更新存档，确保底部吸附后的 `floor` 标记对应实际位置。

Linux 主程序的开发目录为：

```text
DesktopPet/
  desktop-pet
  avatar-host-2d
  resources/
    model-path.txt              # 例如 avatar/manifest.json
    avatar/                    # 本地角色资源，发行时须另核实权利
    models/                    # 后续语音模型，可缺省
    voice/                     # 后续互动音频，可缺省
    show-settings-on-launch    # 可选空文件，启动时显示设置窗口
```

`DESKTOPPET_MODEL` 可覆盖角色入口，`DESKTOPPET_RESOURCE_DIR` 可覆盖资源根目录。设置中的资源相对路径继续复用现有可移动存储逻辑。macOS 保持 `.app` 的 Helpers / Resources 布局。此布局用于开发验证，尚不代表 deb / RPM / AppImage 的安装路径约定。

## NVIDIA X11 透明兼容开关

先前最小探针和当前角色测试均确认：此 NVIDIA Vulkan X11 surface 虽然只报告 `Opaque`，32 位 ARGB 视觉格式中的 alpha 仍能被 xfwm4 正确合成。不能仅凭 `Opaque` 能力声明推广到其他环境。

正常路径优先 `PreMultiplied`，其次 `PostMultiplied`。只有显式设置 `DESKTOPPET_X11_OPAQUE_ALPHA=1`、后端为 NVIDIA Vulkan、窗口拥有 XRender alpha 通道且合成器 selection 存在时，才允许此次验证的兼容路径。输出仍采用预乘 alpha；运行日志记录视觉格式、合成器和兼容模式。其他 GPU 或后端没有相同回退。

```sh
DISPLAY=:1 WGPU_BACKEND=vulkan DESKTOPPET_X11_OPAQUE_ALPHA=1 \
  ./avatar-host-2d /absolute/path/to/manifest.json
```

角色宿主使用父进程 IPC 和心跳；直接在终端执行而不发送协议消息，会在租约到期后退出。应由主程序启动，或使用下述验证脚本。

## 离线构建与验证入口

联网准备机执行 `python3 tools/linux/prepare-offline.py artifacts/local/linux-offline`，生成当前工作区源码、锁定 Cargo vendor 归档和校验文件。只包含 Git 跟踪文件及未忽略的新文件，不包含 Git 历史、私有角色、模型、凭证或本地构建产物。先审阅 `source-manifest.json` 再传输。

构建机的 Ubuntu 软件包索引应先更新，再生成与该机器已安装软件匹配的清单：

```sh
apt-get --no-install-recommends --print-uris --yes --download-only install \
  libwebkit2gtk-4.1-dev build-essential libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev libasound2-dev pkg-config
```

把输出保存并传回准备机，通过 `tools/linux/download-apt.py APT_PLAN OUTPUT_DIRECTORY` 下载 HTTPS 软件包，校验 apt 清单大小和摘要，生成额外 SHA-256 清单。传回构建机后先核验 SHA-256，再放入 `/var/cache/apt/archives/`，用 `apt-get --no-download` 安装上述依赖。不要执行自动清理或额外升级。APT 清单过期时先更新索引；本次 NVIDIA CUDA 第三方镜像曾同步失败，但 Ubuntu 索引更新和本项目依赖安装成功，没有升级 GPU 驱动。

Rust 使用 Linux x86_64 的 1.95.0 独立工具链，sherpa-onnx 使用官方 `v1.13.8` 的 `sherpa-onnx-v1.13.8-linux-x64-static-lib.tar.bz2`。两者在联网准备机下载并保留 SHA-256，再传输到构建机。Rust 和 Cargo vendor 不能包含准备机的 macOS 可执行工具链。构建机从解包的源码根目录运行：

```sh
bash tools/linux/build-offline.sh \
  /absolute/linux-rust-toolchain /absolute/vendor /absolute/sherpa-archives \
  /absolute/build-target
```

脚本核对工具链版本和原生依赖，使用 `cargo build --frozen --release`，并核验 ELF 架构和动态库解析。代码、Cargo 依赖和 sherpa 原生归档均无需由服务器访问境外网站。

```sh
# 以下环境变量只针对上述已验证的 NVIDIA X11 组合。
export DISPLAY=:1 WGPU_BACKEND=vulkan DESKTOPPET_X11_OPAQUE_ALPHA=1
python3 tools/p1/verify-hidden-host.py /absolute/avatar-host-2d /absolute/manifest.json
python3 tools/linux/verify-x11.py /absolute/avatar-host-2d /absolute/manifest.json \
  /absolute/test-output
```

X11 脚本会暂时显示自己的深浅背景，并用 XTest 发送鼠标事件到背景与角色；测试后移除窗口、释放测试按键并恢复鼠标位置。应在无人操作的测试桌面运行，且仅适用于本次单屏基线；底部吸附检查使用当前桌面的 EWMH 工作区；属性不可用时回退到根屏幕。该验证器仍只覆盖单屏基线。脚本保留截图、实际命中事件、拖拽／恢复几何和存档，不需要人工根据“看到窗口”判断通过。

验证器通过 XScreenSaver 检查屏保状态；XFCE 另通过会话 D-Bus 查询自己的屏保（需 `python3-dbus`）。屏保已激活时在发送鼠标事件前终止，不尝试解锁。通过后重置空闲计时，仅对当前 X 连接暂缓核心自动屏保，连接关闭后解除；后续短时输入检查维持测试桌面的活动状态。最终复核曾因 XFCE 屏保盖住桌面而读到黑色截图；恢复测试桌面后重新验证，角色渲染无需修改，不更改全局屏保或合成器设置。

## XFCE 托盘与私有交互测试包

本机原有 XFCE 会话没有安装 `xfce4-panel`，因此应用的 AppIndicator 没有桌面托盘宿主。2026-10-01 使用当前 Ubuntu 索引生成下载计划，在准备机下载并验证 `xfce4-panel 4.16.3-1`，传回后离线安装；只新增一个软件包，没有升级或删除现有包。创建一个仅含 systray 的面板，实测 `org.kde.StatusNotifierWatcher` 已注册真实宿主。原有 XFCE 默认会话已包含 `xfce4-panel` 启动项，无需再增加重复启动项。没有替换为 GNOME，也没有修改 VNC 或 GPU 驱动配置。

应用继续使用已有的 Tauri / Ayatana AppIndicator 托盘，不增加无托盘控制逻辑。实际 D-BusMenu 含显示／隐藏、角色与设置、重试、停止互动、开关免打扰和退出八项。面板保留区会影响 EWMH 工作区，因此实窗验证器也改为按工作区验收底部吸附。

在已经完成原生构建的 Linux 机器上制作测试包：

```sh
python3 tools/linux/package-dev.py \
  --binaries /absolute/linux-target/release \
  --avatar /absolute/private-avatar/manifest.json \
  --output /absolute/new/DesktopPet-Linux-Interaction \
  --nvidia-x11-compat
```

脚本拒绝 macOS 可执行文件、已存在的输出和仓库内未忽略的测试目录。输出包含两个 x86_64 ELF、私有角色副本、X11 启动脚本、说明、文件 SHA-256 清单及同名 `.tar.gz` 和归档摘要。角色素材和测试包保留在私有目录，不上传 Git。`--nvidia-x11-compat` 仅用于上述已经验证的 GPU／合成器组合。

在目标桌面的终端运行 `./run.sh`。首次启动打开设置窗口；关闭设置后通过真实托盘重新打开或退出。语音、关键词唤醒、问候提示音和联网默认关闭，没有打包语音模型。本包存档位于自己的 `data/`，不修改旧测试包或正式应用存档。系统 GTK / WebKit / AppIndicator / Vulkan 依赖由目标机器提供，因此它是当前机器的开发测试包，尚非通用安装包。

托盘与设置验证入口：

```sh
# 从真实 X11 会话执行，须继承该会话的 DBUS_SESSION_BUS_ADDRESS。
python3 tools/linux/verify-tray.py /absolute/DesktopPet-Linux-Interaction \
  /absolute/new-test-output
```

验证器按文件摘要复制包到独立测试目录，只操作自己的进程与窗口。真实托盘宿主必须已经启动；不存在宿主时直接失败，不创建模拟宿主。托盘验证器发送真实 D-BusMenu 事件并检查设置窗口的关闭／重开，不移动鼠标。设置页面各按钮、养成及大小设置仍按下方清单手动验收，不以窗口映射成功代替完整 UI 功能验证。测试执行前应让桌面退出屏保，期间不要操作该临时实例。

手动交互验收顺序：

1. 启动后确认角色、透明背景和托盘图标；关闭设置，再从托盘打开。
2. 点角色部位、拖动、点击透明区及拖到底部；调整角色大小，退出后重启确认位置与大小恢复。
3. 在养成区测试喂食、玩耍和休息；主动陪伴和短时占屏需显式开启，使用停止按钮或托盘结束。
4. 托盘隐藏／显示、开关免打扰，最后从托盘退出，确认角色窗口也消失。

真实托盘验证已通过：图标注册及图像文件、菜单显示／隐藏、关闭设置不退出应用及托盘重新打开、免打扰开关写入独立 SQLite 存档、菜单退出返回 0 且无角色孤儿进程。实际工作区为 `[0, 33, 1920, 1047]`。这是当前 XFCE 会话的结果，不代表 GNOME、Wayland 或没有 StatusNotifier 宿主的桌面也通过。设置页面鼠标操作不在这项托盘测试的通过范围内。

## 完成与后续门槛

已通过的原生角色检查：真实角色的深浅背景透明合成、原始像素中非空与部分 alpha、显示不抢焦点、透明区点击穿透、角色部位事件、角色拖拽、拒绝接管下层开始的拖拽、单屏位置存档／恢复、底部锚点吸附、隐藏与关闭。后台租约、管道 EOF、父进程死亡和协议错误检查继续有效，旧 P1 脚本改为从当前协议定义读取版本。

最终优化构建也通过上述实窗检查。完整主程序从源码目录之外的开发目录读取相对角色资源，设置窗口已映射，主程序进入 `ready`，NVIDIA 进程列表确认真实角色子进程为 `C+G`；终止主程序后未留下角色孤儿进程。首次启动集成检查未覆盖设置页面每个操作或菜单退出路径；后续真实托盘验证已覆盖菜单退出及子进程清理，但完整设置页面仍需手动验收。Linux 和 macOS 分别通过宿主及主程序合计 38 项单元／回归测试；macOS 宿主严格 Clippy 通过。

服务器没有 `/dev/snd`，本轮没有物理麦克风／扬声器验收。启动时也有会话／系统 D-Bus 相关 GTK 警告；真实托盘已补齐，警告仍需记录，不能仅凭警告或图标出现判断完整 UI 功能。没有为测试关闭 WebKit 沙箱或改变桌面启动服务。

尚待后续阶段：多屏／混合 DPI、热插拔与休眠恢复、其他窗口管理器、完整设置操作验收、麦克风／扬声器和 CPU 语音模型、Linux 凭证存储、明确可分发的演示资源、安装包与发行流程。X11 他应用窗口吸附当前明确拒绝启用；设置页面仍保留 macOS 的权限提示文案，Linux 测试时不要启用该实验开关。原生 Wayland 需要另做实现和验收。

本次日志、私有截图和资源清单保存在忽略目录 `artifacts/local/linux-x11-2026-10-01/`；它们不是发行资源。详细实测范围以该目录的报告和 JSON 结果为准。
