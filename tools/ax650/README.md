# AX650 完整桌宠工具

目标是 Ubuntu 22.04 ARM64 / XFCE X11 上的完整 `desktop-pet`：Tauri 设置和托盘、独立二维角色、养成存档及 CPU / NPU 语音。`desktop-pet-ax650` 仅作为可选文字诊断程序。设计与人工验收见 [AX650 文档](../../docs/26-ax650-edge.md)；角色、音频、模型、私有包和运行数据保持 Git 忽略。

## GitHub Actions 编译

[AX650 工作流](../../.github/workflows/ax650-build.yml) 仅通过 `workflow_dispatch` 手动触发；推送、PR 或定时器都不触发。使用 GitHub 原生 `ubuntu-22.04-arm` 和 Rust 1.95.0；从固定 sherpa revision `5473e19db9c08e63aa41e4c273d9b3f743f1cd25` 编译 AXERA shared C API，SDK 使用公开 `AXERA-TECH/ax650n_bsp_sdk` revision `71ac84128ebc96219a9bd4348a49c43527bc605d`（`v1.45.0_p39`），只用于链接。不需要访问板卡或配置 SSH 密钥。

流程检查 C / Rust ABI、工具回归、纯应用与语音回归，编译完整桌宠、角色宿主和诊断 CLI，并检查 ARM64、GLIBC ≤ 2.35 和直接动态依赖。保留代码产物 `DesktopPet-AX650-arm64-binaries`，同时生成 `DesktopPet-AX650-AppImage`，二者保留 14 天。勾选 `publish_release`（默认开启）后，把审核通过的 AppImage、SHA-256 和审计记录发布为 `ax650-v0.1.0-alpha.<run-number>` 预发布版。

AppImage 使用公共资源 Release（默认 `resources-2026-09-29`）中的角色、互动语音及 CPU VAD / ASR / TTS / KWS，同时必须下载 AX650 资源 Release `resources-ax650-2026-10-11` 的 `models-axera.zip`。后者内置此前交付板端的 SenseVoice 10 秒 axmodel、ZipVoice distill encoder / 四段 decoder、Vocos，以及匹配词表、version 1 清单、lexicon 和 espeak 数据。勾选 ASR / TTS 的 NPU 开关后自动选择包内目录；取消后选择 CPU 模型。

`model-resources.lock.json` 固定 ZIP 摘要和全部 374 个文件的 SHA-256 / 大小；373 个已有文件在发布前与此前板端交付包逐一比对，新增 ZipVoice 上游许可证，并补充 Vocos / espeak 许可说明。`model-resources.py` 只选择已测试模型与前端，拒绝替换的 Release 摘要、缺失分段、额外文件及用户参考声音。权重留在 Release，不进入 Git。

`appimage.py` 复用 Linux 资源审计，加入 ARM64 sidecar / ELF 检查、AX650 软件 GL 启动器和 shared sherpa / ONNX Runtime。发布前移除 linuxdeploy 带入的 AXERA SDK 库，保留 `/soc/lib` 链接入口，分别记录编译输入和打包后原生库摘要。最终重打包使用启动器支持的 zstd 压缩，再由同一启动器提取最终 AppImage，复查资源、模型、架构、GLIBC、启动器及隐私规则。用户参考声音和存档不进入发布包。

CI 从源码生成的原生库摘要不同于 `fetch-sherpa.sh` 锁定的板端测试库，构建记录明确区分两者；它不是现有私有完整包的替换包，不自动覆盖板端文件。现有板端导入与私有打包仍使用下面的流程，真实 UI、音频和 NPU 推理由用户验收。

手动构建并发布：

GitHub 的手动入口已登记在默认分支 `master`；应用代码和构建工具由 `source_ref` 选择，默认仍为 `ax650`。

```sh
gh workflow run ax650-build.yml --ref ax650 \
  -f source_ref=ax650 -f resource_tag=resources-2026-09-29 \
  -f axera_resource_tag=resources-ax650-2026-10-11 -f publish_release=true
gh run list --workflow ax650-build.yml --branch ax650 --limit 1
```

仅需要 Actions 产物时使用 `-f publish_release=false`。首次代码编译记录已通过；后续编译和发布都由用户手动调度，不因推送自动启动。

离线重新制作锁定模型 ZIP（需完整已校验的模型根目录；输出为新目录）：

```sh
python3 tools/ax650/model-resources.py \
  --models /absolute/tested-axera-models \
  --output artifacts/local/ax650-model-release
```

输出 `models-axera.zip` 与 `SHA256SUMS.json`；文件清单和最终 ZIP 都必须与锁文件一致。更新模型版本时应重新核对来源与板端配套前端，再更新锁文件和独立资源 Release；不能只替换远端 ZIP。

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
  -e SHERPA_ONNX_LIB_DIR=/native/lib \
  -e AX650_INCLUDE_CLI=1 \
  desktoppet-ax650-desktop-build:20261010
```

工具链固定 Rust 1.95.0，基础系统固定 Ubuntu 22.04 镜像摘要；`fetch-sherpa.sh` 从已有板端构建导入锁定的 ZipVoice AXERA shared C API、ONNX Runtime、头文件和许可证，并校验 SHA-256；链接 SDK 只用于构建，板端仍使用 `/soc/lib`。不使用官方 CPU 静态库替代 NPU 运行库。构建前检查 12 个 C / Rust 配置结构的 ABI；Rust 绑定尾部字段修正见 [说明](../../vendor/zipvoice-axera/README.md)。已有原生环境从源码根目录执行 `bash tools/ax650/build.sh /absolute/target`，目标目录也可使用 `CARGO_TARGET_DIR`。脚本检查 Ubuntu、ARM64、Rust 版本 / host 和 GTK / WebKit / ALSA / AppIndicator 开发依赖，执行 `cargo build --locked --release -p desktop-pet -p avatar-host-2d`。`AX650_INCLUDE_CLI=1` 额外构建诊断程序。

Tauri 构建脚本会在源码树生成已忽略的 `apps/desktop/gen/schemas/`，因此源码挂载需要可写；模型与原生库挂载保持只读。编译后检查 AArch64 ELF、GLIBC 需求不高于 2.35 和直接动态依赖。构建与这些检查均不启动应用、角色或录音。

## 私有完整包

打包需要 Python 3.10 及以上和 Git。显式选择资源，不扫描其他用户目录；输出及归档必须是新路径，仓库内输出必须被 Git 忽略。

```sh
python3 tools/ax650/package.py \
  --binaries artifacts/local/ax650-desktop/target/release \
  --avatar /absolute/private-resources/avatar/manifest.json \
  --models /absolute/private-resources/models \
  --full-voice-models \
  --axera-models /absolute/staged-axera-models \
  --sherpa-native artifacts/local/ax650-desktop/native \
  --voice /absolute/private-resources/voice \
  --notices /absolute/private-resources/notices \
  --runtime-libs /absolute/private-runtime/lib \
  --include-cli \
  --output artifacts/local/ax650-desktop/DesktopPet-AX650-Desktop
```

`--models` 必须包含当前 Silero VAD 与 SenseVoice int8 模型及 tokens。`--full-voice-models` 额外包含当前 ZipVoice、Vocos、espeak-ng-data 和 Zipformer KWS；完整语音资源交付使用此选项。固定文件名沿用 [现有语音资源契约](../../docs/19-p3-voice-pipeline.md)。不复制可选 ncnn / MLX 运行时或 LLM 权重。`--sherpa-native` 必须包含导入后的 lib / include / licenses / runtime.json；打包只复制两个 shared 库、许可证与摘要，排除 SDK。`--axera-models` 额外携带匹配的 SenseVoice 10 秒 axmodel + tokens，以及 ZipVoice distill 四段 decoder / encoder / JSON 清单、Vocos、tokens / lexicon / espeak；文件布局见 [构建文档](../../BUILDING.ax650.zh-CN.md)。

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

包内使用现有 Linux 布局：两个完整应用程序、可选诊断程序、`resources/avatar`、`resources/models`、`resources/voice`、来源与许可、启动器及预检。`resources/voice-settings.json` 使用可移动路径，默认转写模式、ASR 2 / KWS 1 / TTS 4 线程；语音、关键词、问候和联网默认关闭，麦克风接入后由用户启用。SenseVoice / ZipVoice 的 NPU 开关默认关闭，勾选后选择 axera 目录，取消后恢复原 CPU ONNX 路径。`data/` 保留此包独立存档。

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

普通启动也先做依赖预检。完整预检加载 X11 / XKB、当前图形后端以及 GTK / WebKit / AppIndicator / ALSA / xdo、shared sherpa / ONNX Runtime / AXERA SDK；`--cli --check-only` 只检查诊断程序的运行库。GL 不要求 Vulkan，Vulkan 不要求 GL。预检不连接桌面、不创建托盘、不加载模型、不录音；通过仅说明库可加载。每次生成独立日志目录，保存预检结果与启动配置，完整程序和宿主另写运行日志。

## 工具回归

```sh
python3 -m unittest discover -s tools/ax650 -p 'test_*.py' -v
bash -n tools/ax650/build.sh
sh -n tools/ax650/run.sh
```

工具回归使用临时样本和模拟库 / 进程调用，检查架构、GLIBC、私有打包边界、模型选择、后端与应用模式隔离、既有桌面总线查找和启动配置；不启动应用。真实托盘、设置、角色、转写和麦克风仍由用户在板端验收，不安装 VNC 或用 Remote Desktop 代替。
