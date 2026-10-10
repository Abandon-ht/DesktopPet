# AX650 完整桌宠构建与用户测试

`ax650` 分支面向 Ubuntu 22.04 / aarch64 / XFCE X11 的 AX650N 板卡，交付完整 `desktop-pet`：透明角色、托盘、角色与设置窗口、养成存档和麦克风语音识别。主程序复用 Tauri，角色默认由 CPU 软件 OpenGL 渲染。独立 `desktop-pet-ax650` CLI 只作为诊断工具。稳定 30 fps 不作要求；范围与历史证据见 [AX650 实现记录](docs/26-ax650-edge.md)。

构建和不启动窗口的检查由开发侧完成，用户在板端真实 HDMI / XFCE 桌面验收 UI；麦克风稍后连接后再验收录音与识别。此前私有包已显示角色，不能据此声称当前完整包的托盘、设置或 ASR 已通过。不要使用 x86_64 AppImage 或 macOS 可执行文件替代 ARM64 Linux 程序。

## Ubuntu 22.04 ARM64 完整构建

在 Mac 的 OrbStack 中使用同版本原生 ARM64 Ubuntu，Rust 固定为仓库 `rust-toolchain.toml` 的 1.95.0，Cargo 依赖使用 `Cargo.lock`。完整构建环境包括 GTK / WebKitGTK、Ayatana AppIndicator、XDO、ALSA，以及角色宿主的 X11 / GL 依赖。

先下载并校验官方 sherpa-onnx 1.13.8 ARM64 CPU 静态库。归档保存在忽略目录，不提交到 Git：

```sh
bash tools/ax650/fetch-sherpa.sh artifacts/local/ax650-native
```

脚本固定 `sherpa-onnx-v1.13.8-linux-aarch64-static-lib.tar.bz2`，SHA-256 为 `77983e3cf29aa60f2e531d249dbd01d15596530550c8db2e9e02fc6a655da6bb`，解包后检查需要的静态库。这个原生运行时不包含 ASR / TTS 模型权重。

从仓库根目录构建并运行完整构建镜像；镜像上下文仅为工具目录：

```sh
ax650_target="$PWD/artifacts/local/ax650-full-build/target"
ax650_cargo="$PWD/artifacts/local/ax650-full-build/cargo"
ax650_native="$PWD/artifacts/local/ax650-native"
mkdir -p "$ax650_target" "$ax650_cargo"
docker --context orbstack build --platform linux/arm64 \
  -f tools/ax650/Dockerfile \
  -t desktoppet-ax650-desktop-build:20261010 tools/ax650
docker --context orbstack run --rm --platform linux/arm64 \
  -v "$PWD:/workspace" -v "$ax650_target:/target" \
  -v "$ax650_cargo:/cargo" -v "$ax650_native:/native:ro" \
  -e SHERPA_ONNX_LIB_DIR=/native/sherpa-onnx-v1.13.8-linux-aarch64-static-lib/lib \
  desktoppet-ax650-desktop-build:20261010
```

`build.sh` 默认执行 `cargo build --locked --release -p desktop-pet -p avatar-host-2d`，输出在 `artifacts/local/ax650-full-build/target/release/`。需要同时保留 CLI 诊断程序时给容器增加 `-e AX650_INCLUDE_CLI=1`。已有匹配 Linux 环境也可设置 `SHERPA_ONNX_LIB_DIR` 后运行 `bash tools/ax650/build.sh /absolute/target`。

源码挂载允许写入，因为 Tauri 构建会生成 Git 忽略的 `gen/schemas/`；首次完整构建使用只读源码挂载会失败。原生库目录继续只读挂载，Cargo 缓存与输出单独保留。

构建检查 Ubuntu / 架构 / Rust 版本、AArch64 ELF、最高 glibc 要求不超过 2.35，并在构建环境解析直接链接依赖。动态加载的 X11 / GL / WebKit 等库仍需目标端预检。构建不启动角色或音频设备；成功编译不是 UI、麦克风或识别质量验收。

## 私有资源与完整打包

角色、互动声音、用户参考声音和语音模型权重不进入 Git。使用已经准备好的 `manifest.json` 角色包，以及私有模型根目录；模型根目录沿用现有资源布局：

| 功能 | 模型根目录中的文件 |
| --- | --- |
| VAD | `silero_vad.onnx` |
| ASR | `sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09/model.int8.onnx`、`tokens.txt` |
| TTS，可选启用 | `sherpa-onnx-zipvoice-distill-int8-zh-en-emilia/` 下的 encoder / decoder int8、tokens、lexicon、espeak-ng-data，以及 `vocos_24khz.onnx` |
| KWS，可选启用 | `sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20/` 下的 encoder / decoder / joiner 模型与 tokens |

本次完整资源包可携带 VAD / ASR / TTS / KWS；初始只配置“仅识别并显示文字”，不自动打开麦克风或可选服务。仅识别模式不加载 TTS，也不要求 LLM 或参考 WAV。模型来源与下载入口见 [语音流水线](docs/19-p3-voice-pipeline.md) 和 [KWS 配置](docs/20-p3-voice-settings-and-greetings.md)。

此前板端缺少 XKB 运行库，完整应用检查又发现需要补齐 `libxdo.so.3`。从同版本构建镜像准备三个私有库与 Ubuntu 包版权文件：

```sh
ax650_runtime="$PWD/artifacts/local/ax650-full-build/runtime-lib"
mkdir -p "$ax650_runtime/licenses"
ax650_container=$(docker --context orbstack create desktoppet-ax650-desktop-build:20261010)
for ax650_library in libxkbcommon-x11.so.0 libxcb-xkb.so.1 libxdo.so.3; do
  docker --context orbstack cp -L \
    "$ax650_container:/usr/lib/aarch64-linux-gnu/$ax650_library" \
    "$ax650_runtime/$ax650_library"
done
for ax650_package in libxkbcommon-x11-0 libxcb-xkb1 libxdo3; do
  docker --context orbstack cp \
    "$ax650_container:/usr/share/doc/$ax650_package/copyright" \
    "$ax650_runtime/licenses/$ax650_package.copyright"
done
docker --context orbstack rm "$ax650_container"
```

`docker cp -L` 把软件包库的符号链接解引用为普通文件。私有库不替换系统 Mesa / GL / EGL 驱动；GTK / WebKit / AppIndicator / ALSA 等仍由板端匹配系统提供。`--runtime-libs` 可省略，但省略时板端必须已具备相应库。

制作新目录中的完整包：

```sh
python3 tools/ax650/package.py \
  --binaries artifacts/local/ax650-full-build/target/release \
  --avatar /absolute/private-avatar/manifest.json \
  --models /absolute/private-models \
  --full-voice-models \
  --voice /absolute/private-interaction-voice \
  --notices /absolute/private-resource-notices \
  --runtime-libs artifacts/local/ax650-full-build/runtime-lib \
  --output artifacts/local/DesktopPet-AX650-full
```

根据实际私有资源目录替换路径。`--full-voice-models` 携带完整语音模型集合，`--voice` 指互动 WAV 目录，`--notices` 保留资源来源与版权说明；如果编译了 CLI，可再加 `--include-cli`。打包器只复制显式支持的资源和运行库，检查架构、glibc、引用及摘要，不扫描用户目录。输出目录、同名 `.tar.gz` 和 `.tar.gz.sha256` 必须均为新路径；仓库内输出必须被 Git 忽略。

```text
DesktopPet-AX650-full/
  desktop-pet                  # 完整 Tauri 主程序
  avatar-host-2d
  desktop-pet-ax650             # 可选诊断程序
  run.sh
  resources/
    avatar/
    models/
    voice/
  lib/                         # 白名单私有库及 licenses/
  data/                        # 本包设置、角色库与 SQLite 存档
  logs/                        # 每次运行独立目录
```

静态打包检查不代替 Rust 角色契约、Mocari 模型加载、语音模型加载或真实设备测试。包和私有资源不公开上传；再分发前需另行核实权利。

## 上传、预检与完整启动

把归档和摘要传到板端新目录，保留此前成功的测试包。`<board>` 为用户配置的 SSH 主机名：

```sh
scp artifacts/local/DesktopPet-AX650-full.tar.gz <board>:/tmp/
scp artifacts/local/DesktopPet-AX650-full.tar.gz.sha256 <board>:/tmp/
ssh <board>
cd /tmp
sha256sum --check DesktopPet-AX650-full.tar.gz.sha256
mkdir -p ~/DesktopPet-AX650-full-test
tar -xzf DesktopPet-AX650-full.tar.gz -C ~/DesktopPet-AX650-full-test
cd ~/DesktopPet-AX650-full-test/DesktopPet-AX650-full
sha256sum --check SHA256SUMS.txt
./run.sh --check-only
```

完整预检加载 X11 / XKB、所选图形后端、GTK / WebKitGTK、AppIndicator、ALSA 和 XDO 相关库与必要符号；不连接显示、不创建窗口、不打开麦克风。通过只代表库可加载，不证明会话授权、托盘宿主、软件驱动、角色或 ASR 成功。

在板端现有 XFCE 终端启动完整桌宠：

```sh
./run.sh --scale 64 --fps 20
```

默认完整模式常驻，通过托盘退出；`--seconds` 仅适用于 CLI 诊断模式。首次启动打开角色与设置窗口；关闭设置后桌宠继续运行，可从托盘“角色与设置…”重新打开。

当前板端只读检查已确认真实 XFCE StatusNotifier 托盘宿主存在。测试包复用它，不安装或新建桌面面板；新应用托盘图标和菜单仍需用户确认。

启动器使用 `DESKTOPPET_DEVICE_PROFILE=ax650`，选择软件 GL 和 WebKit 软件合成，不关闭 WebKit 沙箱。完整主程序的 `DESKTOPPET_DATA_DIR` 和 `DESKTOPPET_LOG_DIR` 使用绝对路径指向本包独立目录。`--scale` 范围 50–150，是新存档的初始大小；已有角色大小设置优先。`--fps` 范围 1–30，是角色调度预算，实际呈现可能更低。

SSH 启动仍连接既有 X11 / XFCE 会话：保留正确的 `DISPLAY`、`XAUTHORITY` 和会话 D-Bus。启动器可以从同用户、同 `DISPLAY` 的现有 XFCE 进程提取会话总线，不创建替代总线。物理 HDMI 端口号不是 `DISPLAY` 编号。若无法访问会话，先回到真实 XFCE 终端试运行并保留错误记录；不安装 VNC 或用 Remote Desktop 验证。

完整包每次运行的日志保存在 `logs/` 独立目录，包括预检、主程序日志、每次宿主启动的 `host-0001.stderr.log` 和 `host-0001.metrics.json` 等。宿主重试使用不同编号。数据目录保存设置、角色选择和养成状态；升级前在退出后备份 `data/`，先在新包目录验收。

## 用户手动验收：角色、托盘与设置

先保持语音关闭，不需要麦克风：

1. 确认角色可见、深浅背景透明、表情与眨眼正常。点击透明区可操作下方应用；点击角色有反馈，拖动与释放正常。改变大小后复测穿透边缘。
2. 确认 XFCE 托盘图标存在，菜单显示 / 隐藏角色有效。关闭设置窗口后，角色仍在；从“角色与设置…”可再次打开。缺少真实托盘宿主时记录为环境问题，不能以程序进程仍在判通过。
3. 在设置中测试角色大小、导入 / 切换、养成的喂食 / 玩耍 / 休息及停止互动。收益和冷却沿用现有核心规则；重复操作不会无限增加收益。
4. 从托盘退出，再启动同一包，确认养成、大小、角色选择与屏幕位置恢复。确认退出后角色子进程消失；关闭设置本身不等于退出应用。
5. 保留 CPU、RSS、温度及渲染调用间隔；分别比较只显示角色和打开设置的负载。通过标准是角色与设置可用、操作正常且无异常积累，不要求稳定 30 fps。

## 用户接入麦克风后的 ASR 验收

Audio 卡片新增语音模式与麦克风选择。先验证独立识别，无需启动 LLM 或准备 TTS 参考声音：

1. 接入麦克风，在 Audio 点击“刷新麦克风”，选择列出的精确设备名称；刷新只枚举名称，不开始采集。留空表示系统默认输入。若有多个同名设备，名称选择会明确报错，应在系统选择默认输入并清空应用设备名。保存后，ASR 与 KWS 使用同一选择。列表为空或采集失败时保留设备错误，不把角色渲染判为失败。
2. 选择“仅识别并显示文字”，启用语音；ASR 使用 sherpa-onnx SenseVoice，CPU 1 线程。共享模型根目录应指向包内 `resources/models/`，高级模型路径可留空使用默认目录。VAD 使用包内 Silero，默认阈值 0.5、句末静音 600 ms。
3. 保存配置，点击“开始聆听”或角色头部。说一句简短中文，停顿后确认状态经过聆听 / 识别，识别结果区域出现文字。它是完整语句识别，不承诺实时逐字输出。
4. 说不同句子和一段静音，检查文字是否对应本次录音、静默超时是否正常、停止按钮是否立即停止当前会话。每次会话最多处理 10 段；下一段聆听时上一段文字仍可查看。再次启动或点击头部不应显示迟到的旧轮次结果。
5. 重启同一包，确认模式、设备名和模型配置保存；重新开始识别。再记录设备断开 / 重接、识别耗时、CPU / RSS / 温度，以及角色同时显示时的操作体验。

目前麦克风尚未接入，这些项由用户稍后测试；编译及模型文件存在不能替代录音、VAD 端点或识别准确率验收。仅识别文字不写入长期记忆。

## 可选对话、朗读与关键词

需要对话时在 Audio 切换“识别、对话与朗读”：

- LLM 填写板端可访问的 LM Studio 或兼容服务 URL 及真实模型 ID。若服务在 Mac 上，板端的 `127.0.0.1` 不会指向 Mac，应使用实际可达地址或用户配置的转发。外部 LLM 服务与模型由用户提供。
- TTS 选择 sherpa-onnx ZipVoice，填写包内模型目录、用户有权使用的参考 WAV 和准确文本，先以 2 CPU 线程验收。朗读使用系统默认输出设备；先检查扬声器或耳机。参考采样率由音频文件真实读取，不通过修改标签伪装重采样。
- KWS 默认关闭。需要唤醒词时单独开启并配置中英 Zipformer 与词表；先确认头部点击 ASR 可用，再测试真实发音和误唤醒。历史 macOS 合成音频的结果不能代表板端真实关键词质量。

对话沿用半双工：播放期间不采集麦克风，头部点击或停止按钮可打断。GPU / AEC 等未实现选项不开放。分别记录识别、回复、合成、首音、播放和并发负载，不把 TTS 已生成当作扬声器已播放。仅识别模式不会因未配置这些可选项而失败。

## 独立 CLI 诊断

只在打包了 `desktop-pet-ax650` 时可使用；不要与完整应用共用正在使用的数据目录：

```sh
./run.sh --cli --data-dir ./data-cli --seconds 120 --scale 64 --fps 20
```

CLI 接受 `status`、`feed [request-id]`、`play [request-id]`、`rest [request-id]`、`stop`、`show`、`hide`、`scale 64`、`expr <name>`、`reset`、`quit`。重复同一养成请求 ID 用于验证幂等；`reset` 只结束表情预览。CLI 不提供托盘、设置或录音，诊断通过不能替代完整桌宠验收。Vulkan 通过 `--backend vulkan` 显式诊断，其历史空白原因未定。

## 结果记录与支持范围

| 包版本 / 日期 | 配置 | 角色 / 透明 / 输入 | 托盘 / 设置 / 存档 | 麦克风 / VAD / ASR | 可选 LLM / TTS / KWS | CPU / RSS / 温度 | 问题与日志目录 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 待用户填写 | GL，64%，20 fps | 待测 | 待测 | 接入后测试 | 按需测试 | 待记录 | 待记录 |

当前支持目标是已知 AX650N / Ubuntu 22.04 / ARM64 / XFCE X11。完整程序的 ARM64 构建与回归结果单独记录在 [开发记录](docs/26-ax650-edge.md)，与板端 UI / 录音验收分开。无 NPU 推理、原生 Wayland、他应用吸附、多屏 / 混合 DPI 或通用安装包的通过结论。后续加速需核实 SDK、模型转换、算子与量化，不能默认现有 ONNX 或图形着色器可直接交给 NPU。
