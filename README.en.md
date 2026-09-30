# DesktopPet

DesktopPet is a macOS-first desktop companion built with Rust, Tauri 2, and a separate Mocari 0.3.1/wgpu Live2D rendering process. The development app supports character-pack import, desktop interactions, care state, localized audio, and optional local speech through sherpa-onnx and a configured LLM service. Windows basic interaction, other-window snapping, and local Ollama voice have passed local user acceptance; three-dimensional characters and Linux remain future work.

The repository includes runnable development code, architecture documents, and validation tools. The macOS workflow builds a DMG; the manual Windows workflow builds an Authenticode development-signed x64 portable ZIP using separately verified resource archives. The self-signed development certificate has no default Windows or SmartScreen trust. See [Windows builds](BUILDING.windows.md), [Windows CI and signing](docs/windows-ci-signing.md), [Linux preparation](BUILDING.linux.md), [macOS builds](BUILDING.md), and [asset provenance](docs/ASSETS.md).

## Architecture

- `apps/desktop`: Tauri tray application, settings, care state, and voice orchestration.
- `apps/avatar-host-2d`: native transparent avatar window and rendering process.
- `crates/`: core state, protocol, character packs, persistence, IPC, and voice modules.
- `tools/`: validation, packaging, and local development utilities.
- `docs/`: design decisions, testing records, and staged implementation notes. The detailed historical notes are currently in Chinese; the build, asset, and project-use documents are available in English.

The Live2D pack and model weights are kept out of Git. The manual build loads them from a resource Release. LLM inference uses the user's configured Ollama, LM Studio, or other supported service; its model is not bundled.

## Use and copyright

Contributors and users must not use this project to create or distribute sexualized content involving minors or minor-presenting characters.

禁止使用本项目制作或传播涉及未成年人或明显幼态角色的色情、性化内容。

Copyright holders can request review and removal of affected material using the process in [POLICY.md](POLICY.md). Public availability and non-commercial use alone do not establish redistribution permission; known sources and unresolved rights are documented in [docs/ASSETS.md](docs/ASSETS.md).
