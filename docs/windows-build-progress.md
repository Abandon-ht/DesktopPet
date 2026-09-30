# Windows 开发与验收交接

更新日期：2026-09-30。Windows W1／W2 本地功能已由用户测试确认正常，进入手动 CI 构建和签名分发阶段。本文仅记录公开实现与验收结论；机器路径、代理、凭据、用户设置和原始运行日志保留在本地忽略目录。

## 已完成

- Windows x64／MSVC 锁定 Rust 1.95.0 构建；主程序和独立图形宿主均能编译、链接及运行。
- EXE 相对宿主和资源发现、内置资源路径迁移、托盘、角色透明窗口、动态点击穿透、互动、拖拽、大小和位置存档、屏幕工作区吸附。
- Win32 指针／显示器工作区／不激活移动；所有桌面坐标按当前宠物窗口的统一 DPI 比例换算。取不到鼠标时暂停采样，不退出宿主。
- 前台目标窗口、DWM 可见边框和 WinEvent 通知；过滤自身、隐藏、最小化、cloaked、桌面／Shell 与工具窗口。拖动宠物获得焦点时保留此前外部目标；目标失效或前台切换时解除。仅观察几何，不读取窗口内容或移动目标应用。
- Ollama 兼容流式对话及 `reasoning_effort: none`；同样覆盖工具对话和后台记忆请求。其他 LLM 后端保持原有行为。
- 资源 ZIP 大小与 SHA-256 核验、安全解包、可移动测试目录和隐藏控制台日志启动脚本。
- Windows 签名分发脚本：两个 EXE 使用 SHA-256 Authenticode 签名及 RFC 3161 时间戳；校验签名和证书身份后生成 ZIP、公共证书与校验清单。私钥与密码不进入 Git 或产物。

## 验证证据与边界

本机 31 项测试通过：宿主 26 项、主程序 2 项、HTTP 推理 3 项。真实宿主握手、初始隐藏、大小、显示不抢前台焦点、心跳、位置保存及退出检查通过。完整目录移动后从无关工作目录启动，主程序进入 ready；测试主进程终止后宿主退出。

实际 HWND 的 DWM 几何、吸附策略、移动跟随、隐藏拒绝及解除测试通过。Ollama Qwen3.5 9B 的真实流式中文回复、CPU ZipVoice 合成、CPAL 播放与恢复 idle 通过。固定文本 smoke 绕过麦克风／识别；其后用户确认桌面和语音功能测试正常。

该结论限于已测试的本地环境，不代表混合 DPI、多屏热插拔、高权限窗口、所有音频设备、长期运行及干净机器安装均已专项覆盖。Windows 凭据保存、可选 ncnn 外部程序、正式安装器及受信任 CA 代码签名仍为后续任务。

## 本地重建

从仓库根目录执行：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\tools\windows\build.ps1 -RunTests -BuildSmokeTools
python tools/windows/package_local.py --binaries-dir "$env:LOCALAPPDATA/DesktopPet/target/release" --output artifacts/local/windows-test/DesktopPet --with-voice --data-dir artifacts/local/windows-test/data --show-settings
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\artifacts\local\windows-test\DesktopPet\start-test.ps1
```

打包前将独立资源 Release 中的七个 ZIP 及 `SHA256SUMS.json` 放入 `artifacts/local/release-assets`。文件和模型不纳入源码；来源见 [ASSETS.md](ASSETS.md)。首次角色发现失败的历史问题已由 EXE 相对布局修复，直接双击宿主仍不是完整启动方式。

## 手动构建与签名

入口为 `.github/workflows/windows-manual.yml`，仅 `workflow_dispatch`；可选择触发分支及 `source_ref`，默认构建 `windows`。默认分支只登记工作流入口，Windows 实现留在独立分支。

流程使用 Windows Server 2022 runner，下载所选资源 Release、检查格式、执行不依赖交互桌面的 30 项测试、构建、校验资源并打包、签名和验证、上传 `DesktopPet-windows-x64.zip` 与 `SHA256SUMS.txt`。原生可见 HWND 的一项测试已在本机通过，在无交互桌面的 CI 中明确跳过；图形、真实音频和桌面输入不在 runner 上验收。

签名 Secrets、证书信任边界和操作见 [Windows CI 与签名](windows-ci-signing.md)。测试步骤见 [W1](windows-stage1-manual-check.md) 与 [W2](windows-stage2-manual-check.md)。发布包不包含 QA 标记、用户语音配置、SQLite、私钥或 PFX。
