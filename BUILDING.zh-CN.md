# macOS 构建说明

`Build macOS app (manual)` GitHub Actions **只支持手动触发**。它在 `macos-15` Apple Silicon 环境编译程序，从指定 Release 下载分包资源，做临时签名，成功后**自动创建应用预发布 Release**，附上可拖拽安装的 `DesktopPet.dmg` 和 `SHA256SUMS.txt`。同一个 DMG 也会作为 Actions 构建产物保留 14 天。应用尚未经过 Apple 公证，在干净的 Mac 上直接双击启动也尚未验证。LLM 仍依赖用户自己启动的 LM Studio 服务，发行包不含 LLM 权重。

## 资源包

各 ZIP 的内部目录固定；同一 Release 必须包含 `SHA256SUMS.json`。构建脚本会核对校验值，并拒绝危险路径及符号链接。

| 文件 | 内容 |
| --- | --- |
| `icons.zip` | 当前 UI 与应用图标；应用图标也在 Git 中并编译进程序 |
| `avatar.zip` | 最终 Live2D 角色包，含 `avatar/manifest.json` |
| `voice.zip` | 多语言互动 WAV/TXT，位于 `voice/` |
| `models-vad.zip` | Silero VAD |
| `models-asr.zip` | SenseVoice ONNX 与词表 |
| `models-asr-ncnn.zip` | 可选 ncnn SenseVoice，不放进默认应用包 |
| `models-tts.zip` | ZipVoice、Vocos、词典和 espeak 数据 |
| `models-kws.zip` | 中英文关键词模型 |

在本机从现有资源重建分包：

```sh
python3 tools/release/resources.py \
  --avatar artifacts/local/p2/nahida-touch-v4 \
  --voice artifacts/voice
```

输出在 Git 忽略的 `artifacts/local/release-assets/`。不要把资源原件或 ZIP 提交到仓库。将各 ZIP 和 `SHA256SUMS.json` 放入同一个 Release；工作流默认读取 `resources-2026-09-29` 标签。

## 在 GitHub 构建

1. 打开 **Actions → Build macOS app (manual) → Run workflow**。
2. 选择分支和资源 Release 标签，然后运行。
3. 两个任务都成功后，在 **Releases** 打开新建的 `v0.1.0-alpha.<运行序号>` 预发布，下载并打开 `DesktopPet.dmg`，将 `DesktopPet.app` 拖到同一窗口里的“Applications / 应用程序”快捷方式，再从“应用程序”首次启动。同一页面附有 SHA-256 校验文件；Actions 运行页也保留一份临时构建产物。

工作流不会因 push 或 PR 自动运行。构建任务只读取资源 Release；只有发布任务持有创建应用预发布所需的 `contents: write` 权限。

本机需要 Rust 1.95 与 Xcode 命令行工具：

```sh
cargo +1.95.0 build --locked --release -p desktop-pet -p avatar-host-2d
python3 tools/release/package_macos.py \
  --assets-dir artifacts/local/release-assets \
  --require-assets \
  --output artifacts/local/release/DesktopPet.app
```

程序将内置人物、模型和语音路径按 App 的 Resources 目录相对保存；移动 App 后会从新位置重新解析。用户自己选择的外部文件仍保存原路径，移动这些外部文件后须重新指定。

**已有开发版存档**可能保存了旧的绝对资源路径。升级前先退出 DesktopPet，并备份 `~/Library/Application Support/dev.desktoppet.alpha/care.sqlite3` 与同目录的 `preferences.json`。然后删除数据库 `settings` 表中键为 `voice` 的一项，重新启动并配置语音；其他养成数据不受影响。若曾直接选中 App 内角色包，还需删除 `preferences.json` 让人物设置重建。不要删除整个应用数据目录。ncnn 可选后端还需要单独编译 `sherpa-ncnn-offline` 并手动指定其路径。

退出应用后，可只清理旧语音设置：

```sh
cd "$HOME/Library/Application Support/dev.desktoppet.alpha"
sqlite3 care.sqlite3 ".backup 'care-before-path-fix.sqlite3'"
sqlite3 care.sqlite3 "DELETE FROM settings WHERE key = 'voice';"
```

再分发前请阅读[资源来源](docs/ASSETS.md)和[使用与版权说明](POLICY.md)。
