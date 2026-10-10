#!/usr/bin/env bash
# Verified official CPU library archive; never adds native libraries to Git.
set -euo pipefail
if [ "$#" -ne 1 ]; then
    echo 'Usage: fetch-sherpa.sh NATIVE_CACHE_DIRECTORY' >&2
    exit 2
fi
cache_dir=$1
mkdir -p "$cache_dir"
cache_dir=$(cd "$cache_dir" && pwd)
archive_name=sherpa-onnx-v1.13.8-linux-aarch64-static-lib.tar.bz2
archive_path="$cache_dir/$archive_name"
archive_sha256=77983e3cf29aa60f2e531d249dbd01d15596530550c8db2e9e02fc6a655da6bb
# Digest published with this asset on the official v1.13.8 release page:
# https://github.com/k2-fsa/sherpa-onnx/releases/expanded_assets/v1.13.8
if [ ! -f "$archive_path" ]; then
    temporary="$archive_path.part.$$"
    trap 'rm -f "$temporary"' EXIT
    curl --fail --location --retry 3 \
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/$archive_name" \
        --output "$temporary"
    mv "$temporary" "$archive_path"
fi
python3 - "$archive_path" "$archive_sha256" <<'PY'
import hashlib
import sys
from pathlib import Path
path = Path(sys.argv[1])
digest = hashlib.sha256()
with path.open("rb") as stream:
    for block in iter(lambda: stream.read(1024 * 1024), b""):
        digest.update(block)
if digest.hexdigest() != sys.argv[2]:
    raise SystemExit(f"SHA256 mismatch: {path}")
print(f"Verified {path.name}: {digest.hexdigest()}")
PY
tar -xjf "$archive_path" -C "$cache_dir"
library_dir="$cache_dir/${archive_name%.tar.bz2}/lib"
for library in sherpa-onnx-c-api sherpa-onnx-core kaldi-decoder-core \
    sherpa-onnx-kaldifst-core sherpa-onnx-fstfar sherpa-onnx-fst \
    kaldi-native-fbank-core kissfft-float piper_phonemize espeak-ng ucd \
    onnxruntime ssentencepiece_core; do
    test -s "$library_dir/lib${library}.a"
done
echo "SHERPA_ONNX_LIB_DIR=$library_dir"
