# Linux build preparation and port status

This repository provides an **experimental Linux X11 development baseline**, without a Linux installer or release workflow. On 2026-10-01, Ubuntu 22.04 / TigerVNC / xfwm4 / RTX 5090 passed the first builds and native character transparency, input, screen snapping and persistence checks. See the [first-stage implementation record](docs/25-linux-x11-implementation.md) for scope and offline preparation. GitHub Actions still builds only macOS `.app` and DMG artifacts. Other Linux desktops and full application behavior require separate validation. See [Building for macOS](BUILDING.md) for the current release path.

## Target and prerequisites

For the first baseline, select one distribution, `x86_64`, and an **X11 desktop session**, with CPU speech and a redistributable demo character. Have an interactive Linux desktop available. A virtual machine can help diagnose builds, but transparent composition, window level, click-through behavior, multiple monitors, and audio still need validation in the target desktop environment. Record the distribution, desktop environment, window manager/compositor, `$XDG_SESSION_TYPE`, GPU/driver, and display scale. Treat Wayland as a separate target; XWayland results do not validate native Wayland behavior.

On Debian/Ubuntu, install [Tauri 2's Linux system dependencies](https://v2.tauri.app/start/prerequisites/) plus the [ALSA development files required by CPAL](https://github.com/RustAudio/cpal/blob/master/README.md?plain=1):

```sh
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
  libasound2-dev pkg-config
```

For other distributions, use the corresponding packages in the official guidance and add the ALSA development package. Install Git and [rustup](https://www.rust-lang.org/tools/install); the repository pins Rust **1.95.0** in [`rust-toolchain.toml`](rust-toolchain.toml) and dependency versions in `Cargo.lock`. The frontend is static HTML/CSS/JavaScript, so Node.js is not needed for this Cargo probe. Python 3 is needed only to prepare or inspect resource ZIPs locally.

From the repository root on Linux, try:

```sh
rustup toolchain install 1.95.0 --profile minimal
rustc +1.95.0 -vV
printf 'session=%s\n' "$XDG_SESSION_TYPE"
cargo +1.95.0 build --locked --release -p desktop-pet -p avatar-host-2d
```

The outputs are `target/release/desktop-pet` and `target/release/avatar-host-2d`. The pinned Tauri, Mocari/wgpu, CPAL, and sherpa-onnx static-library combination builds on the Ubuntu 22.04 baseline above; other target machines still require checks. Compilation needs no character or speech models; runtime validation needs a character pack. `tools/release/package_macos.py` cannot make a Linux package.

## Porting work and sequence

| Stage | Current state and completion criterion |
| --- | --- |
| Compilation probe | Build both executables with locked dependencies; record and resolve Linux native dependency, link, and target architecture errors. |
| Basic character window | Native X11 pointer, EWMH work area, positioning and screen snapping are implemented. The single-monitor baseline passes real transparency, focus, click-through, click/drag and placement checks. NVIDIA Vulkan's `Opaque` alpha compatibility needs explicit opt-in and native visual/compositor checks; see the implementation record. Multiple monitors, mixed DPI and other WMs remain unvalidated. |
| X11 other-window snapping | The non-macOS observer in `apps/avatar-host-2d/src/ax.rs` is a stub. X11/EWMH target-window observation and filtering can follow the [platform design](docs/03-desktop-platform.md). The basic companion should work when this optional capability is unavailable. |
| Wayland basic mode | Ordinary clients cannot assume access to a global pointer, arbitrary window positioning, or other applications' window geometry. Test transparency, input regions, window level, and tray behavior per compositor; explicitly degrade unavailable snapping and screen-play features. List supported environments for any optional extension. [Wayland protocol model](https://wayland.freedesktop.org/docs/book/Protocol.html) |
| Speech, resources, and distribution | CPAL microphone/speaker operation, sherpa-onnx CPU model loading and an external LLM service remain unvalidated. Linux development uses a sibling helper and `resources/`, with an optional `DESKTOPPET_RESOURCE_DIR` override. Bundling remains disabled. The current XFCE tray, settings close/reopen and menu exit pass native checks. Production installation paths, credential storage and general-purpose packages remain unvalidated. |

Proceed through compilation, a visible character and input, X11 screen snapping and persistence, CPU speech, optional other-window snapping, a Linux package, and then native Wayland basic mode. The first three now have a single-monitor X11 baseline; later stages and other desktops require separate acceptance.

## Assets, packaging, and validation

- Character packs, interaction audio, and model weights stay outside Git. See the [macOS resource archive list](BUILDING.md#resource-archives). A Linux packager must preserve checksum verification and safe extraction without assuming an `.app` layout. Start with demo assets that have clear redistribution rights; see [asset provenance](docs/ASSETS.md) and the [use/copyright policy](POLICY.md). A valid character pack is enough for the no-voice baseline. Full speech also needs VAD, ASR, TTS/KWS models, and reference audio; users supply their own LLM service and model.
- After validation, choose a [deb, RPM, or AppImage](https://v2.tauri.app/distribute/) format. `tools/linux/package-dev.py` creates a private interaction test directory and tar.gz for the current X11 baseline; see the implementation record. It does not create a general-purpose Linux installer. For AppImage, select and build on a minimum supported distribution baseline so newer glibc dependencies do not prevent running on older systems. [Tauri AppImage guide](https://v2.tauri.app/distribute/appimage/)
- Validate transparency on light and dark backgrounds; clicks through to another app; character click/drag; focus and exit; single and mixed-DPI dual monitors; display disconnect; sleep recovery; helper crashes; tray and persistence; microphone/speaker operation; and launch from an installed directory outside the source tree. Record X11 and Wayland capabilities separately.

The [P7 roadmap](docs/05-roadmap.md) gives an early **10–20 person-days and up** for cross-platform work; it does not claim Linux builds today. For one developer familiar with Rust and Linux desktop APIs, allow **2–4 person-days** for first compilation and a window probe, **8–15 person-days total** for a usable X11 development app, and **15–25 person-days total** for fuller X11 behavior, packaging, and validation. A native Wayland basic mode may add **about 5–10 person-days**. These planning ranges may change with window-manager differences and native dependency findings.
