#!/usr/bin/env python3
"""Create separate, reproducible local resource archives outside Git."""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile

ROOT = Path(__file__).resolve().parents[2]
MODEL_NAMES = {
    "vad": ["silero_vad.onnx"],
    "asr": ["sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09/model.int8.onnx",
            "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09/tokens.txt",
            "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09/README.md"],
    "asr-ncnn": ["sherpa-ncnn-sense-voice-zh-en-ja-ko-yue-2025-09-09/model.ncnn.bin",
                 "sherpa-ncnn-sense-voice-zh-en-ja-ko-yue-2025-09-09/model.ncnn.param",
                 "sherpa-ncnn-sense-voice-zh-en-ja-ko-yue-2025-09-09/tokens.txt",
                 "sherpa-ncnn-sense-voice-zh-en-ja-ko-yue-2025-09-09/README.md"],
    "tts": ["vocos_24khz.onnx", "sherpa-onnx-zipvoice-distill-int8-zh-en-emilia/encoder.int8.onnx",
            "sherpa-onnx-zipvoice-distill-int8-zh-en-emilia/decoder.int8.onnx",
            "sherpa-onnx-zipvoice-distill-int8-zh-en-emilia/tokens.txt",
            "sherpa-onnx-zipvoice-distill-int8-zh-en-emilia/lexicon.txt"],
    "kws": ["sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20/encoder-epoch-13-avg-2-chunk-16-left-64.onnx",
            "sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20/decoder-epoch-13-avg-2-chunk-16-left-64.onnx",
            "sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20/joiner-epoch-13-avg-2-chunk-16-left-64.onnx",
            "sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20/tokens.txt",
            "sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20/en.phone"],
}


def add_file(archive, path, name):
    if not path.is_file() or path.is_symlink():
        raise SystemExit(f"Missing or unsafe asset: {path}")
    info = zipfile.ZipInfo(name, (2020, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED
    info.external_attr = 0o644 << 16
    archive.writestr(info, path.read_bytes())


def make_archive(destination, entries):
    with zipfile.ZipFile(destination, "w", compresslevel=6) as archive:
        for path, name in sorted(entries, key=lambda entry: entry[1]):
            add_file(archive, path, name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "artifacts/local/release-assets")
    parser.add_argument("--avatar", type=Path, help="validated avatar pack directory (private by default)")
    parser.add_argument("--voice", type=Path, help="localized WAV/TXT directory (private by default)")
    parser.add_argument("--models", type=Path, default=ROOT / "models/local")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    archives = {}
    icons = ROOT / "apps/desktop/icons"
    archives["icons.zip"] = [(path, f"icons/{path.name}") for path in icons.iterdir() if path.is_file()]
    if args.avatar:
        manifest = args.avatar / "manifest.json"
        data = json.loads(manifest.read_text())
        if data.get("renderer") != "live2d_mocari":
            raise SystemExit("Avatar archive requires a validated Live2D pack")
        archives["avatar.zip"] = [(path, "avatar/" + path.relative_to(args.avatar).as_posix())
                                  for path in args.avatar.rglob("*") if path.is_file()]
    if args.voice:
        archives["voice.zip"] = [(path, "voice/" + path.relative_to(args.voice).as_posix())
                                 for path in args.voice.rglob("*") if path.suffix in (".wav", ".txt")]
    for category, names in MODEL_NAMES.items():
        entries = [(args.models / name, "models/" + name) for name in names]
        if category == "tts":
            data_dir = args.models / "sherpa-onnx-zipvoice-distill-int8-zh-en-emilia/espeak-ng-data"
            entries.extend((path, "models/" + path.relative_to(args.models).as_posix())
                           for path in data_dir.rglob("*") if path.is_file())
        archives[f"models-{category}.zip"] = entries
    hashes = {}
    for name, entries in archives.items():
        dest = args.output / name
        make_archive(dest, entries + [(ROOT / "docs/ASSETS.md", f"notices/{name}.sources.md")])
        hashes[name] = {"sha256": hashlib.sha256(dest.read_bytes()).hexdigest(), "bytes": dest.stat().st_size}
    (args.output / "SHA256SUMS.json").write_text(json.dumps(hashes, indent=2) + "\n")
    print(args.output)


if __name__ == "__main__":
    main()
