#!/usr/bin/env bash
# Run in an extracted source tree on Ubuntu x86_64; no network is used.
set -euo pipefail
if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
    echo 'Usage: build-offline.sh TOOLCHAIN VENDOR SHERPA_ARCHIVES [TARGET_DIR]' >&2
    exit 2
fi
toolchain_dir=$(realpath "$1")
vendor_dir=$(realpath "$2")
export SHERPA_ONNX_ARCHIVE_DIR
SHERPA_ONNX_ARCHIVE_DIR=$(realpath "$3")
export PATH="$toolchain_dir/bin:$PATH"
export CARGO_TARGET_DIR="${4:-$PWD/target}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-8}"
rustc --version | grep -q '^rustc 1\.95\.0 '
pkg-config --exists gtk+-3.0 webkit2gtk-4.1 alsa ayatana-appindicator3-0.1
config_file=$(mktemp)
trap 'rm -f "$config_file"' EXIT
python3 - "$vendor_dir" > "$config_file" <<'PY'
import json, sys
print('[source.crates-io]\nreplace-with = "vendored-sources"')
print('[source.vendored-sources]\ndirectory = ' + json.dumps(sys.argv[1]))
PY
cargo --config "$config_file" build --frozen --release -p desktop-pet -p avatar-host-2d
for binary in desktop-pet avatar-host-2d; do
    readelf --file-header "$CARGO_TARGET_DIR/release/$binary"
    libraries=$(ldd "$CARGO_TARGET_DIR/release/$binary")
    if [[ "$libraries" == *'not found'* ]]; then
        echo "$libraries" >&2
        exit 1
    fi
done
