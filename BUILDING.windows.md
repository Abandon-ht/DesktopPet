# Windows build preparation and port status

This repository does **not yet provide a usable Windows release**. The existing GitHub Actions workflow builds a macOS app and DMG only. There is no Windows installer, release workflow, or Windows device validation record. The commands below are a compilation probe for the two executables; producing `.exe` files does not mean the desktop companion works on Windows. See [Building for macOS](BUILDING.md) for the current release path.

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

If compilation succeeds, the outputs should be `target\release\desktop-pet.exe` and `target\release\avatar-host-2d.exe`. Only macOS builds have been recorded so far. The pinned Rust, Tauri, Mocari/wgpu, and sherpa-onnx native dependency combination has not been verified on Windows; record and resolve first-build errors on the target machine. `tools/release/package_macos.py` cannot make a Windows package.

## Porting work

| Stage | Current state and completion criterion |
| --- | --- |
| Compilation probe | Compile both executables with the lockfile and verify that native dependencies, including sherpa-onnx static libraries, link for MSVC. |
| Basic companion | Non-macOS pointer, work-area, move, and snap functions in `apps/avatar-host-2d/src/platform.rs` are stubs or errors. Dynamic hit testing in `window.rs` is macOS-only. Implement and test transparent rendering, click-through recovery, click/drag, focus, DPI coordinates, placement restoration, and screen snapping against real underlying windows. |
| Optional other-window snapping | The non-macOS observer in `apps/avatar-host-2d/src/ax.rs` is a stub. Implement WinEvent observation, DWM window geometry, and filtering from the [platform design](docs/03-desktop-platform.md). The basic companion should work when this capability is unavailable. |
| Voice and resources | The code uses CPAL and sherpa-onnx, but speech validation so far is on macOS. Verify Windows microphone/speaker behavior, the WASAPI path, model loading, and CPU performance. LLM replies still require the user's own LM Studio service. |
| Packaging | `apps/desktop/src/main.rs` currently resolves bundled files and the helper using a macOS `.app/Contents/Resources` layout. `apps/desktop/tauri.conf.json` has no enabled Windows bundling, Windows icon, resource layout, or workflow. Make both executables and optional assets resolve after installation, then choose a [Tauri NSIS or MSI installer](https://v2.tauri.app/distribute/windows-installer/) and validate on a clean machine. |

Proceed through compilation, visible character and input, screen snapping and persistence, CPU speech, other-window snapping, and installer validation. The rendering GPU backend and speech inference acceleration are separate; validate CUDA or DirectML combinations independently.

## Assets and validation

- The character pack, interaction audio, and speech models are delivered as separate resource ZIPs, outside Git. See the [macOS resource archive list](BUILDING.md#resource-archives). A Windows packager must implement checksum verification and safe extraction without assuming an `.app` layout. Use demo assets with clear redistribution rights; see [asset provenance](docs/ASSETS.md) and [use/copyright policy](POLICY.md).
- A valid demo character is enough for the no-voice baseline. Full speech needs the relevant VAD, ASR, TTS/KWS models and reference audio. LLM replies need LM Studio and a model supplied by the user. The optional ncnn path additionally needs a Windows `sherpa-ncnn-offline` executable.
- Validate clicking through transparent areas into another app, character click and drag, focus and exit, single and mixed-DPI dual monitors, display disconnect, sleep recovery, helper crashes, tray and persistence, microphone/speaker operation, and launch from an installed directory. Record the Windows build, GPU/driver, display scale, model versions, and results.

The [P7 roadmap](docs/05-roadmap.md) gives an early **10–20 person-days and up** for Windows and Linux work; it is not a claim that Windows already builds. Based on the current code, allow **1–3 person-days** for the first Windows compilation and dependency fixes, **8–15 person-days** for a usable basic Windows companion, or **15–25 person-days** including speech validation, resource packaging, and installer testing. These are planning ranges for one developer familiar with Rust and desktop APIs; native dependency or device findings may change them.
