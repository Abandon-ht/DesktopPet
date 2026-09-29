# DesktopPet

DesktopPet is a macOS-first desktop companion built with Rust, Tauri 2, and a separate Mocari 0.3.1/wgpu Live2D rendering process. The current development app supports character-pack import, desktop interactions, care state, localized interaction audio, and an optional local speech pipeline using sherpa-onnx and LM Studio. Three-dimensional characters and other desktop platforms are future work.

The repository includes runnable P1–P3 development code, architecture documents, and validation tools. The current macOS release workflow is manual and packages a Live2D character plus voice and speech-model resources from separate ZIP files. See [Building for macOS](BUILDING.md), [asset provenance](docs/ASSETS.md), and the [Chinese README](README.md) for the detailed development record.

## Architecture

- `apps/desktop`: Tauri tray application, settings, care state, and voice orchestration.
- `apps/avatar-host-2d`: native transparent avatar window and rendering process.
- `crates/`: core state, protocol, character packs, persistence, IPC, and voice modules.
- `tools/`: validation, packaging, and local development utilities.
- `docs/`: design decisions, testing records, and staged implementation notes. The detailed historical notes are currently in Chinese; the build, asset, and project-use documents are available in English.

The Live2D pack and model weights are kept out of Git. The app loads them from a resource Release during the manual build. LLM inference uses the user's own LM Studio service; its model is not bundled.

## Use and copyright

Contributors and users must not use this project to create or distribute sexualized content involving minors or minor-presenting characters.

禁止使用本项目制作或传播涉及未成年人或明显幼态角色的色情、性化内容。

Copyright holders can request review and removal of affected material using the process in [POLICY.md](POLICY.md). Public availability and non-commercial use alone do not establish redistribution permission; known sources and unresolved rights are documented in [docs/ASSETS.md](docs/ASSETS.md).
