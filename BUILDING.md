# Building DesktopPet for macOS

The `Build macOS app (manual)` GitHub Actions workflow is triggered **only** through `workflow_dispatch`. It builds an Apple Silicon (`arm64`) app on `macos-15`, adds resource archives from a selected GitHub Release, applies an ad-hoc signature, and uploads `DesktopPet.zip` as a workflow artifact. The artifact expires after 14 days. It is not notarized; macOS may require the user to explicitly open it in Privacy & Security. The app uses a local LM Studio server for LLM replies; model weights for the LLM are not included.

## Resource archives

Each archive has a fixed top-level directory. Keep `SHA256SUMS.json` with the ZIPs in the same Release. The packaging script checks every downloaded ZIP against this file and rejects unsafe paths and symlinks.

| File | Content |
| --- | --- |
| `icons.zip` | Current UI and app icons; the app icon is also tracked in Git and compiled into the app |
| `avatar.zip` | Final Live2D pack (`avatar/manifest.json` and referenced files) |
| `voice.zip` | Localized interaction WAV and TXT files in `voice/` |
| `models-vad.zip` | Silero VAD |
| `models-asr.zip` | SenseVoice ONNX and tokens |
| `models-asr-ncnn.zip` | Optional alternative SenseVoice ncnn files; not included in the default app |
| `models-tts.zip` | ZipVoice, Vocos, lexicon, and espeak data |
| `models-kws.zip` | Bilingual keyword spotting model |

To reproduce the archives from this machine's local resources:

```sh
python3 tools/release/resources.py \
  --avatar artifacts/local/p2/nahida-touch-v4 \
  --voice artifacts/voice
```

The output is `artifacts/local/release-assets/`, which is ignored by Git. Do not add it or raw assets to the repository. The selected Release tag must contain the ZIPs and `SHA256SUMS.json`. The workflow's default tag is `resources-2026-09-29`.

## Run the workflow

1. Open **Actions → Build macOS app (manual) → Run workflow**.
2. Choose the branch and the resource Release tag. Run it.
3. Download `DesktopPet-macOS-arm64` from the completed run and unzip the artifact, then unzip `DesktopPet.zip` to obtain `DesktopPet.app`.

The workflow does not run on pushes or pull requests. It needs the workflow file on the default branch and read access to the selected Release. For local builds, install Rust 1.95 and Xcode command line tools, then run:

```sh
cargo +1.95.0 build --locked --release -p desktop-pet -p avatar-host-2d
python3 tools/release/package_macos.py \
  --assets-dir artifacts/local/release-assets \
  --require-assets \
  --output artifacts/local/release/DesktopPet.app
```

The app reads the bundled avatar through a relative path and loads bundled voice/model files from its own Resources directory. Existing user settings with paths from an older local test install can override these defaults; use a new macOS user profile or reset those settings when validating a clean install. The ncnn alternative needs a separately built `sherpa-ncnn-offline` executable and manual path selection.

See [asset provenance](docs/ASSETS.md) and [project use/copyright policy](POLICY.md) before redistributing these files.
