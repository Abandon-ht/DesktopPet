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

`tools/windows/sign_package.ps1` 为包中的主程序与图形宿主签名；SHA-256 文件摘要、RFC 3161 时间戳、证书身份和 Authenticode 校验都必须通过。Secrets 缺失或签名失败时工作流停止，不上传未签名应用。

当前采用自签名开发证书，公共 `.cer` 随包提供。它可标识签名身份、验证文件完整性，但不会自动获得 Windows／SmartScreen 信任，也不等于 Microsoft 认证。用户无需为了运行测试包安装根证书。[Microsoft SignTool 说明](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool)

CI 可暂时把公共证书加入该次临时 runner 的当前用户 Root store，用于验证自签名证书，结束后移除；此选项仅在 GitHub Actions 环境允许，不能用来改变用户设备的信任设置。未来换用受信任代码签名证书时更新 Secrets；正式发行仍需针对干净机器验证。

GitHub SSH 密钥用于开发机推送，和 Windows 代码签名证书互不相干。SSH 私钥只保留在开发机，公钥添加到账户 Authentication keys；不放入 Actions 或应用资源。密码、PFX、私钥及其 Base64 文本不进入文档、Git、日志或 ZIP。

## 产物

`DesktopPet-windows-x64` artifact 保留 14 天，内含 ZIP 和外部 `SHA256SUMS.txt`。完整解压后运行 `desktop-pet.exe`；需要日志时运行 `start-test.ps1`，从托盘退出。资源随包，Ollama 服务和 LLM 权重另行安装配置。

ZIP 内包含两个已签名 EXE、公共证书、资源、构建信息、版权／资产说明。SHA-256 在签名后计算。`archive_package.py` 拒绝 QA 标记、用户语音设置、数据库、日志或私钥文件。当前提供便携包；NSIS／MSI 和安装升级流程尚未实现。
