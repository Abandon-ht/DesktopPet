# Building and distributing for Windows

The `windows` branch includes the basic companion, other-window snapping, and local Ollama voice, validated by automated checks and local user acceptance. A manual GitHub Actions workflow publishes an Authenticode development-signed Windows x64 Setup.exe with bundled resources, a GUI wizard, optional login startup, and no terminal during normal use. Acceptance across devices remains pending. See [Windows CI and signing](docs/windows-ci-signing.md) and the [implementation handoff](docs/windows-build-progress.md).

## Target and prerequisites

Use Windows 10/11 x64, CPU speech, and a redistributable demo character as the first baseline. Have an interactive Windows machine available to verify transparent composition, click-through behavior, focus, mixed DPI, multiple monitors, and audio devices. A virtual machine's graphics result alone is not sufficient for final validation.

Install the following on Windows:

1. [Microsoft C++ Build Tools](https://v2.tauri.app/start/prerequisites/) with **Desktop development with C++** and the Windows SDK.
2. [Microsoft Edge WebView2 Runtime](https://v2.tauri.app/start/prerequisites/). It may already be present; confirm it on the build machine.
3. [rustup](https://www.rust-lang.org/tools/install) with the pinned Rust **1.95.0** version. Select `x86_64-pc-windows-msvc` as the default host toolchain in the rustup installer. See [`rust-toolchain.toml`](rust-toolchain.toml) and `Cargo.lock`.
4. Git. Python 3 is needed only to prepare or check resource ZIPs locally. The current frontend is static HTML/CSS/JavaScript, so Node.js is not needed for the Cargo compilation probe below.

From the repository root in PowerShell:

```powershell
rustup toolchain install 1.95.0-x86_64-pc-windows-msvc --profile minimal
rustc +1.95.0-x86_64-pc-windows-msvc -vV
cargo +1.95.0-x86_64-pc-windows-msvc build --locked --release -p desktop-pet -p avatar-host-2d
```

The outputs are `release\desktop-pet.exe` and `release\avatar-host-2d.exe` inside the build directory. The pinned Rust, Tauri, Mocari/wgpu, and sherpa-onnx native dependencies have compiled and linked on Windows 11 x64. `tools/windows/build.ps1 -RunTests` loads the MSVC environment and runs the application unit tests before building. `tools/release/package_macos.py` cannot make a Windows package.

## Porting work

| Stage | Current state and completion criterion |
| --- | --- |
| Compilation probe | Both executables and native dependencies build and link with the lockfile on the local Windows machine. |
| Basic companion | Win32 pointer, work-area, movement, placement, dynamic input, and screen snapping are implemented. The user confirmed local functionality. Mixed DPI, multiple monitors, sleep, and other devices need separate coverage. |
| Optional other-window snapping | Foreground discovery, DWM visible bounds, WinEvent, filtering, and retention of the target during pet focus are implemented. Native tests and local user acceptance passed; mixed DPI and elevated windows need separate validation. |
| Voice and resources | Ollama Qwen3.5 9B, CPU ZipVoice, and CPAL smoke checks passed, followed by local user voice acceptance. Other devices, long sessions, and optional backends need separate validation. |
| Packaging | Movable layout, verified assets, an NSIS GUI installer, optional login startup, quiet application startup, signed application/uninstaller/setup executables, and automatic Release publication are implemented. The self-signed certificate has no default system trust; trusted CA signing and clean-machine acceptance remain pending. |

Proceed through compilation, visible character and input, screen snapping and persistence, CPU speech, other-window snapping, and installer validation. The rendering GPU backend and speech inference acceleration are separate; validate CUDA or DirectML combinations independently.

## Assets and validation

- The character pack, interaction audio, and speech models are delivered as separate resource ZIPs, outside Git. See the [macOS resource archive list](BUILDING.md#resource-archives). The local Windows packager verifies size and SHA-256 and safely extracts archives. Use demo assets with clear redistribution rights for releases; see [asset provenance](docs/ASSETS.md) and [use/copyright policy](POLICY.md).
- A valid demo character is enough for the no-voice baseline. Full speech needs the relevant VAD, ASR, TTS/KWS models and reference audio, plus an available Ollama, LM Studio, or other supported LLM service. Ollama requests use `reasoning_effort: none` for spoken replies; the local W2 model alias uses a 4096-token context. The optional ncnn path additionally needs a Windows `sherpa-ncnn-offline` executable.
- Validate clicking through transparent areas into another app, character click and drag, focus and exit, single and mixed-DPI dual monitors, display disconnect, sleep recovery, helper crashes, tray and persistence, microphone/speaker operation, and launch from an installed directory. Record the Windows build, GPU/driver, display scale, model versions, and results.

The [P7 roadmap](docs/05-roadmap.md) gives an early **10–20 person-days and up** for Windows and Linux work; it is not a claim that Windows already builds. Based on the current code, allow **1–3 person-days** for the first Windows compilation and dependency fixes, **8–15 person-days** for a usable basic Windows companion, or **15–25 person-days** including speech validation, resource packaging, and installer testing. These are planning ranges for one developer familiar with Rust and desktop APIs; native dependency or device findings may change them.
