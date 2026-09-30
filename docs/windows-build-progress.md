# Windows 编译与开发交接（2026-09-30）

## 当前结论

从 `master` 的 `6d1915af73fe40dad96961cd2471e28cb36a5989` 创建 `windows` 分支。
Windows 11 x64 上两个程序已完成锁定版本的 Release 编译及链接，但主程序尚未自动找到角色，桌宠窗口尚未完成运行验收。

分支保存以下进度：

- 使用现有 `apps/desktop/icons/icon.png` 转换出 16/32/48/256 像素 ICO，并加入 Tauri 图标配置，修复首次构建时 `icons/icon.ico not found` 错误。
- `tools/windows/build.ps1`：查找 C++ Build Tools、加载 x64 开发环境并编译两个程序；构建目录与代理均可配置。
- `tools/windows/prepare_resources.py`：核验默认 Release 资源的大小及 SHA-256，拒绝危险路径和符号链接，然后解包。
- 本记录：环境、产物、资源及用户启动反馈，供后续会话接续开发。

没有修改 `Cargo.lock`，没有实现 Windows 资源自动发现或桌面平台适配。角色、模型、语音、ZIP、EXE、日志及凭据均不提交到 Git。

## 已配置的本机环境

- Windows 11 x64，系统版本 10.0.22631。
- Git for Windows `2.55.0.windows.5`。
- rustup `1.29.1`，Rust/Cargo `1.95.0`，`x86_64-pc-windows-msvc`，已安装 rustfmt/clippy。
- Visual Studio 2022 Build Tools `17.14.37710.0`，C++ 桌面开发工作负载及推荐组件，MSVC `14.44.35207`，Windows SDK `10.0.26100.0`。
- 现有 WebView2 Runtime `154.0.4258.37`。
- 下载时使用机器已有代理 `http://127.0.0.1:7897`，仅在命令进程中配置，没有更改系统代理。

仓库：`Z:\Users\neo\Workspace\DesktopPet`。
缓存：`C:\Users\Administrator\.cache\DesktopPet\target`。
原始本机脚本及报告还保存在仓库父目录。

复用本次缓存进行编译（仓库根目录）：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\tools\windows\build.ps1 -TargetDirectory C:\Users\Administrator\.cache\DesktopPet\target -ProxyUrl http://127.0.0.1:7897
```

执行策略选项只作用于该次 PowerShell 进程。脚本默认输出到当前用户的 `%LOCALAPPDATA%\DesktopPet\target`，并在未指定代理时保留当前环境设置。

底层构建命令与仓库 Windows 说明一致：

```powershell
cargo +1.95.0-x86_64-pc-windows-msvc build --locked --release -p desktop-pet -p avatar-host-2d
```

## 本机编译产物和资源

本次成功产物的复制件：

- `artifacts/local/windows-build/desktop-pet.exe`，37,742,080 字节，SHA-256 `76ae4042184523c7ab4a0d33c88b1ec9977442951c7587a8fbf7b911a15b3f60`。
- `artifacts/local/windows-build/avatar-host-2d.exe`，9,104,384 字节，SHA-256 `21a704b3ad56ba6e13927f990fc2528c53096d5c89be6a956736f9c7da72abc2`。
- 两者均已检查为 x64 PE 文件，Tauri、Mocari/wgpu、CPAL、SQLite 和 sherpa-onnx 原生依赖编译及链接通过。
- 同目录的 `windows-build-01.log` 记录首次图标缺失错误，`windows-build-02.log` 记录成功编译。后者开头的 PowerShell `NativeCommandError` 是标准错误流中编译进度的显示标记；实际构建退出码为 0。

已阅读 `.github/workflows/macos-manual.yml`。其默认资源标签是 [resources-2026-09-29](https://github.com/Abandon-ht/DesktopPet/releases/tag/resources-2026-09-29)，编译同样的两个包，然后使用专用 macOS 打包脚本生成 DMG。

已下载并逐个通过 `SHA256SUMS.json` 的大小与哈希检查：`icons.zip`、`avatar.zip`、`voice.zip`、`models-vad.zip`、`models-asr.zip`、`models-tts.zip`、`models-kws.zip`。没有下载默认应用不包含的可选 `models-asr-ncnn.zip`。

- ZIP 和校验清单：`artifacts/local/release-assets/`。
- 已安全解包的资源：`artifacts/local/release-resources/`。
- 角色入口：`artifacts/local/release-resources/avatar/manifest.json`。
- 可重新校验及解包：`python tools/windows/prepare_resources.py`。

这些目录在 Git 忽略范围内，新 checkout 不会自带资源；应从上述 Release 重新下载并校验。没有运行 macOS 打包脚本，也没有制作 Windows 安装包。

## 用户运行反馈与源码线索

用户运行编译的主程序后报告状态依次为 `connecting` → `recovering`（重试）→ `fault`，整个过程中 `host_pid` 为 `null`，错误为：

```text
未配置角色。请设置 DESKTOPPET_MODEL 为本地 model3.json 路径后重新启动应用。
```

用户还报告单独启动 `avatar-host-2d` 没有窗口。没有收到其标准错误输出或完整启动参数，因此不能据此认定图形渲染失败。

已检查以下源码，尚未改动运行逻辑：

- `apps/desktop/src/main.rs` 的 `run()` 仍以可执行文件的父目录推导 macOS `.app/Contents`，读取 `Resources/model-path.txt`，并尝试 macOS helper 布局或同目录无扩展名的 `avatar-host-2d`。
- 在启动 `Host::start()` 之前，未配置模型会直接返回上述错误。因此这次日志首先表明角色发现失败，尚未证明宿主启动或渲染成功。
- `apps/avatar-host-2d/src/main.rs` 要求一个角色入口参数；不传参数会返回 `usage: avatar-host-2d MODEL3_JSON`。
- `apps/avatar-host-2d/src/window.rs` 同时接受原始 model3.json 和名为 `manifest.json` 的角色包入口。宿主窗口初始隐藏，需要主程序经 stdin/stdout IPC 发送显示指令，并依赖父进程心跳；直接双击不是完整的宿主验收方式。

下一会话建议先实现或验证 Windows 下与 EXE 同目录的宿主查找（包含 `.exe`）、资源目录发现，以及角色包入口配置。可以先将 `DESKTOPPET_MODEL` 指向已经解包的 `avatar/manifest.json` 做诊断，但该尝试在本次会话中没有执行，不代表能立即显示角色。

随后再验证透明合成、点击穿透、拖拽、DPI、多屏、屏幕吸附、音频设备和安装目录迁移。有关已知平台缺口见 `BUILDING.windows.zh-CN.md` 与 `docs/03-desktop-platform.md`。本分支完成的是首次编译及交接，不是完整 Windows 移植。
