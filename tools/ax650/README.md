# AX650 端侧工具

仅面向当前 Ubuntu 22.04 ARM64 / X11 软件渲染基线。程序实现和人工验收见 [AX650 设计与测试文档](../../docs/26-ax650-edge.md)。角色素材、测试包、存档与日志保留在 Git 忽略目录中。

## 原生 ARM64 编译

在 Apple Silicon 的 OrbStack 上启动原生 ARM64 Ubuntu 22.04 构建环境。构建上下文只包含此工具目录，避免将私有角色资源发送给镜像构建器：

```sh
docker build --platform linux/arm64 \
  -f tools/ax650/Dockerfile \
  -t desktoppet-ax650-build:20261010 tools/ax650

mkdir -p artifacts/local/ax650-build/target artifacts/local/ax650-build/cargo
docker run --rm --platform linux/arm64 \
  -v "$PWD:/workspace:ro" \
  -v "$PWD/artifacts/local/ax650-build/target:/target" \
  -v "$PWD/artifacts/local/ax650-build/cargo:/cargo" \
  desktoppet-ax650-build:20261010
```

镜像固定 Ubuntu 22.04 基础镜像摘要，从官方站点下载 Rust 1.95.0 ARM64 独立工具链并验证 SHA-256。可传 `RUST_ARCHIVE_URL` 和 `RUST_ARCHIVE_SHA256` 使用预先下载的官方归档；指定摘要时不另行获取校验文件。镜像只准备 Rust CLI 和角色宿主的依赖，没有 WebKit、Tauri、语音运行时或桌面服务。

已有匹配环境可以从源码根目录运行 `bash tools/ax650/build.sh /absolute/target`；省略参数时使用 `CARGO_TARGET_DIR`，再缺省时使用源码根目录的 `target/`。脚本检查 Ubuntu / 架构 / Rust 版本与 host，执行锁定依赖的 release 构建，并检查两文件的 AArch64 ELF、所需 GLIBC 不高于 2.35、直接动态依赖可解析。构建过程不启动角色或连接显示器。

## 私有打包

在 Mac 或 Linux 的 Python 3.10 及以上与 Git 环境运行。输出目录、同名归档与归档摘要必须均为新路径；仓库内的输出路径必须被 Git 忽略。

```sh
python3 tools/ax650/package.py \
  --binaries artifacts/local/ax650-build/target/release \
  --avatar /absolute/private-avatar/manifest.json \
  --runtime-libs /absolute/private-runtime/lib \
  --output artifacts/local/ax650-build/DesktopPet-AX650
```

`--runtime-libs` 可省略。提供时必须包含以下四个常规文件：

```text
libxkbcommon-x11.so.0
libxcb-xkb.so.1
licenses/libxkbcommon-x11-0.copyright
licenses/libxcb-xkb1.copyright
```

这两项应来自与板端相匹配的 Ubuntu 22.04 ARM64 软件包；构建容器中的 `/usr/lib/aarch64-linux-gnu/` 和 `/usr/share/doc/` 提供原文件。准备私有 `lib/` 时将软件包软链接指向的真实库内容复制为上述常规文件，不能将软链接直接交给打包器。其他库不会复制，尤其不会附带 GL/EGL/Mesa 驱动。省略私有库时由板端系统提供它们。

脚本只复制模型、纹理、物理 / Pose / DisplayInfo / UserData、登记的动作与表情引用；未引用的文件不进入包。它拒绝软链接、绝对 / 越界引用、缺失资产、未支持类型、超出预算的资源和音频动作，并检查模型 JSON、MOC3 / PNG 头和 PNG 尺寸。完整的角色契约、参数和 Mocari 模型加载仍由 Rust 端侧主程序与宿主检查，静态打包检查不能代替实际模型加载。

输出包括 `desktop-pet-ax650`、`avatar-host-2d`、角色资源、启动与预检脚本、`README.txt`、`package-manifest.json`、`SHA256SUMS.txt`，以及目录名加 `.tar.gz` 的归档和 `.tar.gz.sha256`。元数据记录源码提交、分支和工作区是否有未提交变更，不记录准备机的私有路径或用户身份；生成包和角色均不得公开上传。

## 板端启动与检查

将包放到新的私有路径，保留先前成功的包。运行前可在包目录核验 `sha256sum --check SHA256SUMS.txt`。

```sh
./run.sh --check-only
./run.sh --scale 64 --fps 20 --seconds 120
```

默认 GL 软件渲染；`--backend vulkan` 仅为诊断入口。普通启动也先做同样的动态依赖检查。预检不打开 X display，只加载 X11 / XKB 和当前后端的库、检查 XKB 符号；GL 预检不要求 Vulkan，Vulkan 预检不要求 GL。预检通过不代表驱动、桌面授权、合成器或像素正确。

启动器默认 `DISPLAY=:0`，保留用户已有的 `DISPLAY` 与 `XAUTHORITY`；只在缺少授权变量时引用已有、可读的 LXDM `:0` 授权文件，不复制凭证。`DISPLAY` 是 X 会话编号，不是 HDMI 接口编号。私有 `lib/` 加入子进程搜索路径，软件渲染限定在该运行环境中。

支持 `--model`、`--data-dir`、`--log-dir`、`--backend`、`--scale`、`--fps`、`--seconds`。默认资源与存档在包内；未指定 `--log-dir` 时每次在 `logs/` 创建新目录。启动器预检后替换为 Rust 主进程；心跳、交互、存档、角色日志和正常退出由 Rust 管理。`--check-only` 失败或成功也写入结果，明确标记未启动角色。

## 工具回归检查

```sh
python3 -m unittest discover -s tools/ax650 -p 'test_*.py' -v
bash -n tools/ax650/build.sh
sh -n tools/ax650/run.sh
```

回归使用临时 ELF / JSON / PNG 样本与模拟库加载，只验证架构、GLIBC、打包边界、运行库白名单、后端分支和预检行为，不启动角色、桌面或设备。真实角色、透明、点击、拖拽和退出由用户在板端验收；稳定 30 fps 不是通过条件。
