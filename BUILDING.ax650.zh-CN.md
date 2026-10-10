# AX650 构建与用户测试

`ax650` 分支基于已有 Linux X11 分支，目标是 Ubuntu 22.04 / aarch64 / XFCE 的 AX650N 端侧开发包。第一阶段包含原生 Rust 主程序 `desktop-pet-ax650` 和独立 `avatar-host-2d`，复用养成与 SQLite 存档，不需要 WebKit、托盘或语音模型。稳定 30 fps 不作要求。环境、历史成功渲染证据和后续规划见 [AX650 端侧记录](docs/26-ax650-edge.md)。

这份说明面向构建与用户在板端手动验收。此前私有诊断包的软件 GL 已经显示角色；本分支新主程序的养成、退出与存档仍须独立验收。不要把 x86_64 AppImage 复制到 ARM64 板卡。

## 同版本 Ubuntu ARM64 构建

在 Mac 的 OrbStack 中使用 Ubuntu 22.04 原生 ARM64 容器，Rust 版本由仓库 `rust-toolchain.toml` 固定为 1.95.0，Cargo 依赖使用 `Cargo.lock`。同架构容器构建得到 `aarch64-unknown-linux-gnu` ELF，不需要在板卡上安装 Rust。不能用 macOS 二进制替代 Linux 程序；也不要在更新的 Ubuntu 上构建后假定其 glibc 需求兼容板卡。

从仓库根目录准备容器和忽略目录；下例使用 OrbStack Docker context：

```sh
ax650_target="$PWD/artifacts/local/ax650-build/target"
ax650_cargo="$PWD/artifacts/local/ax650-build/cargo"
mkdir -p "$ax650_target" "$ax650_cargo"
docker --context orbstack build --platform linux/arm64 \
  -f tools/ax650/Dockerfile -t desktoppet-ax650-build:20261010 tools/ax650
docker --context orbstack run --rm --platform linux/arm64 \
  -v "$PWD:/workspace:ro" -v "$ax650_target:/target" \
  -v "$ax650_cargo:/cargo" desktoppet-ax650-build:20261010
```

Dockerfile 准备 Ubuntu 22.04 ARM64、所需构建依赖和 Rust；默认调用 `bash tools/ax650/build.sh`。此脚本只能在原生 Ubuntu 22.04 aarch64 环境中运行，支持 `bash tools/ax650/build.sh /target` 或 `CARGO_TARGET_DIR` 指定输出。上例的两程序位于 `artifacts/local/ax650-build/target/release/`，Cargo 缓存单独保留便于重建。构建阶段不启动板端角色、不安装板端软件，也不修改显示配置。手工在同版本 Linux 构建环境中编译时可使用：

```sh
cargo build --locked --release -p desktop-pet-ax650 -p avatar-host-2d
```

两程序输出在所用 Cargo target 目录的 `release/` 中。编译结果应记录源码提交、Ubuntu 镜像、Rust 版本与二进制 SHA-256，并检查 AArch64 架构、glibc 版本需求和直接链接依赖。直接链接检查不能覆盖 X11 / GL 运行时的 `dlopen`，还需要包内的动态依赖预检。

## 私有角色包与打包

编译无需美术或语音资源。运行使用经过现有工具准备的 `manifest.json` 角色包；不将 `.moc3`、纹理、表情、动作、模型权重或用户日志提交到 Git。

此前本板缺少的两个 XKB 运行库可从同一构建镜像复制到私有目录，并附上 Ubuntu 包版权文件；`docker cp -L` 将符号链接解引用为普通文件：

```sh
ax650_runtime="$PWD/artifacts/local/ax650-build/runtime-lib"
mkdir -p "$ax650_runtime/licenses"
ax650_container=$(docker --context orbstack create desktoppet-ax650-build:20261010)
docker --context orbstack cp -L \
  "$ax650_container:/usr/lib/aarch64-linux-gnu/libxkbcommon-x11.so.0" \
  "$ax650_runtime/libxkbcommon-x11.so.0"
docker --context orbstack cp -L \
  "$ax650_container:/usr/lib/aarch64-linux-gnu/libxcb-xkb.so.1" \
  "$ax650_runtime/libxcb-xkb.so.1"
docker --context orbstack cp \
  "$ax650_container:/usr/share/doc/libxkbcommon-x11-0/copyright" \
  "$ax650_runtime/licenses/libxkbcommon-x11-0.copyright"
docker --context orbstack cp \
  "$ax650_container:/usr/share/doc/libxcb-xkb1/copyright" \
  "$ax650_runtime/licenses/libxcb-xkb1.copyright"
docker --context orbstack rm "$ax650_container"
```

制作测试包时显式指定私有角色入口：

```sh
python3 tools/ax650/package.py \
  --binaries artifacts/local/ax650-build/target/release \
  --avatar /absolute/private-avatar/manifest.json \
  --runtime-libs artifacts/local/ax650-build/runtime-lib \
  --output artifacts/local/DesktopPet-AX650
```

根据构建工具实际输出替换 `--binaries`。`--runtime-libs` 是可选项：板端已具备相关库时可省略，否则应显式提供上述两个普通 `.so` 文件和 `licenses/`。此前本板需要补齐 `libxkbcommon-x11.so.0`（Ubuntu 包 1.4.0-1）和 `libxcb-xkb.so.1`（1.14-3ubuntu3）；新构建时记录实际 Ubuntu 包版本。这两项来自 Ubuntu ARM64 构建环境，不从 macOS 系统库复制。使用尚不存在的输出目录；复用旧目录可能覆盖日志或存档。工具校验两程序的 Linux AArch64 架构和 glibc 不高于 2.35 的要求，复制角色包与显式提供的私有运行库，生成启动脚本、文件摘要、构建元数据、版权信息及同名 `.tar.gz` / `.tar.gz.sha256`。资源和包留在本地忽略目录；发布前须另核实素材再分发权利。

目录布局：

```text
DesktopPet-AX650/
  desktop-pet-ax650
  avatar-host-2d
  run.sh
  resources/avatar/
  lib/                       # 此包私有运行库，附来源与版权
  data/                      # 本包独立存档，运行后建立
  logs/                      # 每次运行独立目录
```

不要捆绑板卡 GPU 驱动或替换系统 Mesa。私有库补齐动态运行依赖时，应与 Ubuntu 22.04 ARM64 匹配并保留版权；图形驱动和既有 XFCE / Xorg 环境继续由目标系统提供。

## 上传与启动

将归档和摘要传到用户选定的板端新目录，核验摘要后解包。以下 `<board>` 是用户配置的 SSH 主机名，目录仅为示例：

```sh
scp artifacts/local/DesktopPet-AX650.tar.gz <board>:/tmp/
scp artifacts/local/DesktopPet-AX650.tar.gz.sha256 <board>:/tmp/
ssh <board>
cd /tmp
sha256sum --check DesktopPet-AX650.tar.gz.sha256
mkdir -p ~/DesktopPet-AX650-test
tar -xzf /tmp/DesktopPet-AX650.tar.gz -C ~/DesktopPet-AX650-test
cd ~/DesktopPet-AX650-test/DesktopPet-AX650
sha256sum --check SHA256SUMS.txt
./run.sh --check-only
```

以实际归档文件名和解包布局为准。包内预检涵盖 X11 / XKB 及所选后端的动态加载库和必要符号；GL 检查 GL / EGL / GLES，Vulkan 检查 Vulkan loader。ELF 直接依赖解析在构建阶段检查。`--check-only` 不连接显示、不创建角色窗口，也不验证动画或桌面透明。

在板端现有 XFCE 会话的终端运行：

```sh
./run.sh --seconds 120 --scale 64 --fps 20
```

也可通过 SSH 连接既有桌面运行；`DISPLAY` 和 X 授权必须指向用户当前 X11 会话。物理 HDMI 端口编号不是 `DISPLAY` 编号。以当前已知桌面会话为准，通常是 `:0`；若访问被拒绝，应使用该桌面用户的正确授权，而不是全局放开 X11 访问。

默认使用 CPU 软件 OpenGL；`--scale` 范围为 50–150，64 对应约 320×384；`--fps` 范围为 1–30，20 请求 20 fps 调度。大小增大会提高 CPU 像素处理成本；请求的 fps 与实际显示不是同一指标。不指定 `--scale` 时恢复本包保存的大小，首次使用 64；显式 `--scale` 会更新存档。`--seconds` 默认 0，持续运行直到 `quit` 或退出信号；终端 stdin EOF 本身不会关闭应用，有限试跑应指定 `--seconds`。`--log-dir` 可显式指定本次日志目录，`--data-dir` 可指定存档目录。Vulkan 仅通过 `--backend vulkan` 显式诊断，历史空白原因未定，不建议作为本板默认路径。

Ctrl+C、SIGTERM 和终端挂断 SIGHUP 会请求角色正常关闭并保存日志。当前包不自动注册后台服务，也不承诺断开 SSH 后继续常驻。显式 `--log-dir` 是单次日志目录；已有 `parent.jsonl` 时拒绝复用，避免覆盖先前记录。正常退出码为 0，运行失败为 1，直接执行 Rust 程序的配置错误为 2。

## 用户手动测试

测试由用户在真实 HDMI / XFCE 桌面进行。无需安装 VNC 或使用 Remote Desktop。每次启动记录其日志目录、包版本、backend、scale、fps 和实际桌面环境；不同配置分别运行，避免同时启动多个实例造成结果混淆。

主程序在运行终端接受以下命令；表情名称以当前角色包为准：

| 命令 | 用途 |
| --- | --- |
| `status` | 查看当前养成、活动和宿主状态 |
| `feed [request-id]` / `play [request-id]` / `rest [request-id]` | 请求喂食、玩耍和休息；省略 ID 时生成新请求，相同 ID 用于验证重复请求不会重复获得收益 |
| `stop` | 停止当前互动 |
| `show` / `hide` | 显示或隐藏角色 |
| `scale 64` | 调整角色大小 |
| `expr <name>` | 预览角色包 `expression_profile.catalog` 中的指定表情；不存在时明确拒绝 |
| `reset` | 结束表情预览，恢复正常表现；不会清空 SQLite 养成存档 |
| `quit` | 保存并退出主程序与角色子进程 |

按以下顺序记录结果：

1. **角色与透明。** 在深色、浅色背景前确认模型、表情和眨眼可见，透明区没有整块底色。软件 GL 的日志应记录 llvmpipe、surface 模式及真实 ARGB / 合成器证据；出现错误时保留 `host.stderr.log`。
2. **输入。** 点击角色部位，检查反馈；点击透明空白区，应能操作下方应用。拖动角色后释放，确认没有持续跟随或吞掉下层拖拽；改变大小再复测穿透边缘。
3. **养成。** 先 `status`，再执行喂食、玩耍、休息和停止，比较活动与数值。可连续两次输入 `feed test-feed-1` 验证同一请求只扣一次库存、只获得一次收益；新操作使用不同 ID，例如 `feed test-feed-2`。将已有 ID 用于另一动作应报错，不重复执行。连续新请求仍受现有核心冷却和规则限制，拒绝或无收益不一定是渲染问题。
4. **存档。** 用 `quit` 正常退出，再启动同一包且不传 `--scale`，检查养成状态、大小和屏幕位置恢复。`data/` 中的 `care.sqlite3` 与 `placement.json` 是此包存档；测试新包时不要直接覆盖旧包数据。需要保留数据时在退出后备份。
5. **生命周期。** 分别测试 `quit`、时间到期和 Ctrl+C，确认角色窗口消失、日志保存、子进程退出；异常退出与 EOF / 父进程死亡清理由自动回归和后续手动检查共同覆盖。
6. **性能与温度。** 先完成 120 秒基线，再按实际需要延长运行；保留 CPU、RSS、温度及呈现调用间隔。通过标准是角色可用、输入可操作、无异常累积，不要求稳定 30 fps。日志中的 submit/present 计数不能视为 GPU 完成或屏幕扫描计数。

可使用表格汇总：

| 包版本 / 日期 | 配置 | 角色 / 透明 | 穿透 / 拖拽 | 养成 / 存档 | 关停 | CPU / RSS / 温度 | 问题与日志目录 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 待用户填写 | GL，64%，20 fps，120 秒 | 待测 | 待测 | 待测 | 待测 | 待记录 | 待记录 |

## 常见问题与支持范围

- `libxkbcommon-x11.so` 加载失败：说明动态运行依赖缺失，先运行 `--check-only` 并核对私有 `lib/`，不能以 `ldd` 通过排除。
- 透明合成被拒绝：检查是否真实 X11、32 位 ARGB visual、XRender alpha 与合成器 owner；不要把 NVIDIA Vulkan 兼容变量用于 AX650 GL。
- 有窗口但无角色：保存宿主日志和模型错误，核对 backend 与显示会话；Vulkan 的历史空白尚未定位，帧提交计数不能证明图像正确。
- 字符命令无反馈：确认输入位于启动器终端、宿主仍存活；查看主程序和宿主日志，不向独立宿主手工发送不完整 IPC。

本阶段只支持已知 AX650N / Ubuntu 22.04 / aarch64 / XFCE X11 开发环境。完整 Tauri 设置、托盘、音频和 NPU 推理、他应用吸附、多屏 / 混合 DPI、原生 Wayland及通用安装包均需另行实施和验收。
