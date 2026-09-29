# Building DesktopPet for macOS

The `Build macOS app (manual)` GitHub Actions workflow is triggered **only** through `workflow_dispatch`. It builds an Apple Silicon (`arm64`) app on `macos-15`, adds resource archives from a selected GitHub Release, applies an ad-hoc signature, and automatically creates an app pre-release with `DesktopPet.zip` and `SHA256SUMS.txt`. The same ZIP is retained as a workflow artifact for 14 days. The app is not notarized, and direct double-click launch on a clean Mac has not yet been validated. The app uses a local LM Studio server for LLM replies; model weights for the LLM are not included.

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
3. After both jobs succeed, open the new `v0.1.0-alpha.<run number>` pre-release under **Releases**. Download `DesktopPet.zip`, extract `DesktopPet.app`, and move it to Applications before the first launch. The release also contains its SHA-256 checksum. The Actions run keeps a temporary copy of the same ZIP.

The workflow does not run on pushes or pull requests. The build job reads the resource Release; only the publish job has `contents: write` permission to create the app pre-release. For local builds, install Rust 1.95 and Xcode command line tools, then run:

```sh
cargo +1.95.0 build --locked --release -p desktop-pet -p avatar-host-2d
python3 tools/release/package_macos.py \
  --assets-dir artifacts/local/release-assets \
  --require-assets \
  --output artifacts/local/release/DesktopPet.app
```

The app stores bundled avatar, voice, and model paths relative to its Resources directory, so they resolve again after the app moves. User-selected external files keep their original paths and must be reselected if moved.

**Older development profiles** may contain absolute paths saved by earlier builds. Quit DesktopPet and back up `~/Library/Application Support/dev.desktoppet.alpha/care.sqlite3` and `preferences.json` in the same directory. Remove only the `voice` entry from the database's `settings` table, then launch the new app and configure voice again; care data remains intact. If you explicitly selected an avatar inside the old app bundle, remove `preferences.json` as well to rebuild avatar preferences. Do not delete the whole app data directory. The ncnn alternative needs a separately built `sherpa-ncnn-offline` executable and manual path selection.

After quitting the app, you can clear only the old voice setting with:

```sh
cd "$HOME/Library/Application Support/dev.desktoppet.alpha"
sqlite3 care.sqlite3 ".backup 'care-before-path-fix.sqlite3'"
sqlite3 care.sqlite3 "DELETE FROM settings WHERE key = 'voice';"
```

See [asset provenance](docs/ASSETS.md) and [project use/copyright policy](POLICY.md) before redistributing these files.
