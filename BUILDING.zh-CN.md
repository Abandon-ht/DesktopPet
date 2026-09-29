# macOS 构建说明

`Build macOS app (manual)` GitHub Actions **只支持手动触发**。它在 `macos-15` Apple Silicon 环境编译程序，从指定 Release 下载分包资源，做临时签名，并把 `DesktopPet.zip` 上传为保留 14 天的 Actions 构建产物。它没有经过 Apple 公证；首次打开时可能需要在“隐私与安全性”中明确允许。LLM 仍依赖用户自己启动的 LM Studio 服务，发行包不含 LLM 权重。

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
3. 成功后下载 `DesktopPet-macOS-arm64` 构建产物，解开外层压缩包，再解开 `DesktopPet.zip`，得到 `DesktopPet.app`。

工作流不会因 push 或 PR 自动运行。工作流文件必须在默认分支，且它要有权限读取指定 Release。

本机需要 Rust 1.95 与 Xcode 命令行工具：

```sh
cargo +1.95.0 build --locked --release -p desktop-pet -p avatar-host-2d
python3 tools/release/package_macos.py \
  --assets-dir artifacts/local/release-assets \
  --require-assets \
  --output artifacts/local/release/DesktopPet.app
```

程序通过相对路径加载内置人物，并从应用 Resources 目录加载模型与语音。旧版本地测试存档中的绝对路径会覆盖这些默认值；验证全新安装时请用新的 macOS 用户配置或清理旧设置。ncnn 可选后端还需要单独编译 `sherpa-ncnn-offline` 并手动指定其路径。

再分发前请阅读[资源来源](docs/ASSETS.md)和[使用与版权说明](POLICY.md)。
