# Windows 手动 CI 与签名

工作流：`.github/workflows/windows-manual.yml`。只允许手动触发，不随 push／PR 自动运行。

在 GitHub Actions 选择 **Build Windows app (manual)**、触发分支和源码 `source_ref`（默认 `windows`），资源标签默认 `resources-2026-09-29`。源码分支与工作流触发分支可分别选择；实际源码提交写入产物 `BUILD-INFO.json`。新增手动工作流需先在默认分支登记，之后可在 Windows 分支触发。[GitHub 手动工作流说明](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow)

也可使用已认证的 GitHub CLI：

```sh
gh workflow run windows-manual.yml --ref windows -f source_ref=windows -f resource_tag=resources-2026-09-29
```

## 签名配置

添加三个 Repository Actions Secrets：

| 名称 | 内容 |
| --- | --- |
| `WINDOWS_SIGNING_CERT_PFX` | 包含代码签名证书和私钥的 PFX 文件的 Base64 文本 |
| `WINDOWS_SIGNING_CERT_PASSWORD` | 该 PFX 的导出密码 |
| `WINDOWS_SIGNING_CERT_THUMBPRINT` | 预期签名证书的 SHA-1 thumbprint，40 位十六进制，用于核对身份 |

`tools/windows/sign_package.ps1` 为主程序、图形宿主、NSIS 内嵌卸载器及最终 Setup.exe 签名；SHA-256 文件摘要、RFC 3161 时间戳、证书身份和 Authenticode 校验都必须通过。Secrets 缺失或签名失败时工作流停止，不上传未签名应用。

当前采用自签名开发证书，公共 `.cer` 随包提供。它可标识签名身份、验证文件完整性，但不会自动获得 Windows／SmartScreen 信任，也不等于 Microsoft 认证。用户无需为了运行测试包安装根证书。[Microsoft SignTool 说明](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool)

CI 可暂时把公共证书加入该次临时 runner 的 LocalMachine Root store，用于验证自签名证书，结束后移除。CurrentUser Root store 可能弹出确认窗口，不适用于无交互 CI。此选项仅在一次性的 GitHub 托管 Actions runner 允许，不能用于自托管 runner 或用户设备；私钥仍只导入当前用户 My store。签名步骤有 10 分钟超时并输出各阶段进度。未来换用受信任代码签名证书时更新 Secrets；正式发行仍需针对干净机器验证。

GitHub SSH 密钥用于开发机推送，和 Windows 代码签名证书互不相干。SSH 私钥只保留在开发机，公钥添加到账户 Authentication keys；不放入 Actions 或应用资源。密码、PFX、私钥及其 Base64 文本不进入文档、Git、日志或 ZIP。

## 产物

`DesktopPet-windows-x64` artifact 保留 14 天，内含 **DesktopPet-windows-x64-Setup.exe** 和外部 `SHA256SUMS.txt`。成功后工作流以独立的 `contents: write` 发布任务创建 `windows-v0.1.0-alpha.<run_number>` prerelease，标签指向实际源码提交，和 macOS 标签区分。用户从 [Releases](https://github.com/Abandon-ht/DesktopPet/releases) 下载单个 Setup.exe 即可。

安装向导使用固定版本 NSIS 3.12，默认安装到当前用户的程序目录，创建开始菜单及可选桌面快捷方式。开机自启默认关闭，向导和应用设置均可修改当前用户的 `Run` 项。升级保留已有自启选择；升级前从系统托盘退出旧程序。卸载只删除已打包的程序文件，保留额外用户文件、应用设置和数据库。

安装包内包含两个已签名 EXE、公共证书、全部角色及语音资源、构建信息和版权／资产说明。缺少 WebView2 时运行微软官方 bootstrapper，首次安装需联网；CI 下载时核验其微软 Authenticode 签名。Ollama 服务和 LLM 权重另行配置。Release 主程序使用 Windows GUI subsystem，宿主和可选 ncnn ASR 子进程使用 `CREATE_NO_WINDOW`，正常启动不显示终端；需要诊断日志时可运行安装目录内的 `start-test.ps1`。

便携 ZIP 作为安装器构建的中间产物，不额外发布。安装包校验和在最终签名后计算。ZIP 和安装器共用隐私检查，拒绝 QA 标记、用户语音设置、数据库、日志、符号链接或私钥文件。`test_installer.py` 实际编译并静默安装隔离的测试程序，验证中文／空格路径、自启选择、升级和卸载保留用户文件。
