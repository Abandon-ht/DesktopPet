# AX650 完整桌宠工具

目标是 Ubuntu 22.04 ARM64 / XFCE X11 上的完整 `desktop-pet`：Tauri 设置和托盘、独立二维角色、养成存档及 CPU 语音识别。`desktop-pet-ax650` 仅作为可选文字诊断程序。设计与人工验收见 [AX650 文档](../../docs/26-ax650-edge.md)；角色、音频、模型、私有包和运行数据保持 Git 忽略。

## ARM64 构建

Apple Silicon 的 OrbStack 使用原生 ARM64 Ubuntu 22.04。构建上下文只包含工具目录，避免将私有资源发送给镜像构建器：

```sh
docker build --platform linux/arm64 -f tools/ax650/Dockerfile \
  -t desktoppet-ax650-desktop-build:20261010 tools/ax650

mkdir -p artifacts/local/ax650-desktop/{target,cargo,native}
bash tools/ax650/fetch-sherpa.sh artifacts/local/ax650-desktop/native
docker run --rm --platform linux/arm64 \
  -v "$PWD:/workspace" \
  -v "$PWD/artifacts/local/ax650-desktop/target:/target" \
  -v "$PWD/artifacts/local/ax650-desktop/cargo:/cargo" \
  -v "$PWD/artifacts/local/ax650-desktop/native:/native:ro" \
  -e SHERPA_ONNX_LIB_DIR=/native/sherpa-onnx-v1.13.8-linux-aarch64-static-lib/lib \
  -e AX650_INCLUDE_CLI=1 \
  desktoppet-ax650-desktop-build:20261010
```

工具链固定 Rust 1.95.0，基础系统固定 Ubuntu 22.04 镜像摘要；`fetch-sherpa.sh` 获取并校验官方 ARM64 1.13.8 静态归档。已有原生环境从源码根目录执行 `bash tools/ax650/build.sh /absolute/target`，目标目录也可使用 `CARGO_TARGET_DIR`。脚本检查 Ubuntu、ARM64、Rust 版本 / host 和 GTK / WebKit / ALSA / AppIndicator 开发依赖，执行 `cargo build --locked --release -p desktop-pet -p avatar-host-2d`。`AX650_INCLUDE_CLI=1` 额外构建诊断程序。

Tauri 构建脚本会在源码树生成已忽略的 `apps/desktop/gen/schemas/`，因此源码挂载需要可写；模型与原生库挂载保持只读。编译后检查 AArch64 ELF、GLIBC 需求不高于 2.35 和直接动态依赖。构建与这些检查均不启动应用、角色或录音。

## 私有完整包

打包需要 Python 3.10 及以上和 Git。显式选择资源，不扫描其他用户目录；输出及归档必须是新路径，仓库内输出必须被 Git 忽略。

```sh
python3 tools/ax650/package.py \
  --binaries artifacts/local/ax650-desktop/target/release \
  --avatar /absolute/private-resources/avatar/manifest.json \
  --models /absolute/private-resources/models \
  --full-voice-models \
  --voice /absolute/private-resources/voice \
  --notices /absolute/private-resources/notices \
  --runtime-libs /absolute/private-runtime/lib \
  --include-cli \
  --output artifacts/local/ax650-desktop/DesktopPet-AX650-Desktop
```

`--models` 必须包含当前 Silero VAD 与 SenseVoice int8 模型及 tokens。`--full-voice-models` 额外包含当前 ZipVoice、Vocos、espeak-ng-data 和 Zipformer KWS；完整语音资源交付使用此选项。固定文件名沿用 [现有语音资源契约](../../docs/19-p3-voice-pipeline.md)。不复制可选 ncnn / MLX 运行时或 LLM 权重。

`--voice` 可选，只复制受支持语言 `zh-CN`、`en-US`、`ja-JP`、`ko-KR` 下 `greetings`、`care`、`intimacy` 的 WAV / TXT，并校验 WAV 头。提供时需有中文 `greetings/noon.wav` 和匹配文字，用作包内可移动的 TTS 参考；不导入用户旧语音设置或凭证。`--notices` 可选，保留原资源归档的六份来源说明；项目 `docs/ASSETS.md` 始终附带。

`--runtime-libs` 可省略，由目标系统提供运行库。提供时必须包含匹配 Ubuntu 22.04 ARM64 的以下六个常规文件，不能直接传软件包的软链接：

```text
libxkbcommon-x11.so.0
libxcb-xkb.so.1
libxdo.so.3
licenses/libxkbcommon-x11-0.copyright
licenses/libxcb-xkb1.copyright
licenses/libxdo3.copyright
```

其他库不会复制，GL / EGL / Mesa 驱动、GTK 和 WebKit 由目标系统提供。打包器只复制角色引用闭包和显式模型 / 音频白名单，拒绝越界路径、软链接、缺失文件、凭证及存档污染，并检查类型与资源预算。角色完整契约和模型实际加载由 Rust 主程序与宿主执行，静态检查不能代替加载或真实画面验收。

包内使用现有 Linux 布局：两个完整应用程序、可选诊断程序、`resources/avatar`、`resources/models`、`resources/voice`、来源与许可、启动器及预检。`resources/voice-settings.json` 使用可移动路径，默认转写模式、CPU ASR 1 / KWS 1 / TTS 2 线程；语音、关键词、问候和联网默认关闭，麦克风接入后由用户启用。`data/` 保留此包独立存档。

生成 `package-manifest.json`、`SHA256SUMS.txt`、目录名加 `.tar.gz` 的归档和 `.tar.gz.sha256`。元数据记录源码提交 / 分支 / 是否有未提交变更以及所有文件摘要，不记录准备机私有路径或身份。资源与生成包只供私有测试，不能公开上传。

## 启动与预检

将包放在新的板端私有目录，保留旧成功包。在包目录执行：

```sh
sha256sum --check SHA256SUMS.txt
./run.sh --check-only
./run.sh --scale 64 --fps 20
```

默认启动完整桌宠与设置窗口，关闭设置后通过真实托盘重开或退出。`--cli` 才启动诊断程序，`--seconds` 仅对该模式有效。支持 `--model`、`--data-dir`、`--log-dir`、`--backend gl|vulkan`、`--scale 50..150` 和 `--fps 1..30`；完整程序通过环境接收这些设置，已保存的角色大小优先于启动默认值。稳定 30 fps 不作为验收要求。

启动器默认 `DISPLAY=:0`，保留用户已有显示与授权变量；缺省时引用已有、可读的 LXDM `:0` 授权文件，不复制凭证。完整模式缺少会话总线时，仅从同用户、同 DISPLAY 的既有 `xfce4-session` / `xfce4-panel` / `xfwm4` 进程环境提取总线；没有匹配会话就退出，不创建新 D-Bus。`DISPLAY` 是 X 会话编号，不是 HDMI 口编号。

角色默认 GL / llvmpipe 软件渲染、320×384、调度上限 20 fps。显式 GL alpha 兼容继续由宿主核验 ARGB visual 与合成器。设置页使用 `WEBKIT_DISABLE_COMPOSITING_MODE=1` 关闭加速合成，这是 [WebKit 官方环境变量](https://trac.webkit.org/wiki/EnvironmentVariables)；保留 WebKit sandbox。Vulkan 仅作诊断，历史空白问题尚未定位。

普通启动也先做依赖预检。完整预检加载 X11 / XKB、当前图形后端以及 GTK / WebKit / AppIndicator / ALSA / xdo；`--cli --check-only` 只检查诊断程序的运行库。GL 不要求 Vulkan，Vulkan 不要求 GL。预检不连接桌面、不创建托盘、不加载模型、不录音；通过仅说明库可加载。每次生成独立日志目录，保存预检结果与启动配置，完整程序和宿主另写运行日志。

## 工具回归

```sh
python3 -m unittest discover -s tools/ax650 -p 'test_*.py' -v
bash -n tools/ax650/build.sh
sh -n tools/ax650/run.sh
```

工具回归使用临时样本和模拟库 / 进程调用，检查架构、GLIBC、私有打包边界、模型选择、后端与应用模式隔离、既有桌面总线查找和启动配置；不启动应用。真实托盘、设置、角色、转写和麦克风仍由用户在板端验收，不安装 VNC 或用 Remote Desktop 代替。
