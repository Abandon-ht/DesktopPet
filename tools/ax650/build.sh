#!/usr/bin/env bash
# Native Ubuntu ARM64 build; does not start the renderer or alter a desktop.
set -euo pipefail
if [ "$#" -gt 1 ]; then
    echo 'Usage: build.sh [TARGET_DIRECTORY] (or CARGO_TARGET_DIR)' >&2
    exit 2
fi
source /etc/os-release
if [ "${ID:-}" != ubuntu ] || [ "${VERSION_ID:-}" != 22.04 ]; then
    echo 'AX650 baseline requires Ubuntu 22.04' >&2
    exit 1
fi
if [ "$(uname -m)" != aarch64 ] || [ "$(dpkg --print-architecture)" != arm64 ]; then
    echo 'Use a native aarch64 Ubuntu environment (OrbStack ARM64 on Apple Silicon)' >&2
    exit 1
fi
rust_version=$(rustc --version)
if [[ "$rust_version" != 'rustc 1.95.0 '* ]] ||
   ! rustc -vV | python3 -c 'import sys; sys.exit("host: aarch64-unknown-linux-gnu\n" not in sys.stdin.read())'; then
    echo 'Rust 1.95.0 aarch64-unknown-linux-gnu is required' >&2
    exit 1
fi
source_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$source_root"
export CARGO_TARGET_DIR="${1:-${CARGO_TARGET_DIR:-$source_root/target}}"
mkdir -p "$CARGO_TARGET_DIR"
export CARGO_TARGET_DIR=$(realpath "$CARGO_TARGET_DIR")
pkg-config --exists gtk+-3.0 webkit2gtk-4.1 alsa ayatana-appindicator3-0.1
if [ -z "${SHERPA_ONNX_LIB_DIR:-}" ] || [ "$(basename "$SHERPA_ONNX_LIB_DIR")" != lib ]; then
    echo 'Set SHERPA_ONNX_LIB_DIR to the verified AXERA native cache lib/ directory' >&2
    exit 1
fi
native_root=$(dirname "$SHERPA_ONNX_LIB_DIR")
python3 tools/ax650/native-runtime.py "$native_root" --verify
python3 tools/ax650/check-sherpa-abi.py "$native_root/include/c-api.h"
export LD_LIBRARY_PATH="$SHERPA_ONNX_LIB_DIR:$native_root/sdk${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LIBRARY_PATH="$SHERPA_ONNX_LIB_DIR:$native_root/sdk${LIBRARY_PATH:+:$LIBRARY_PATH}"
build_packages=(-p desktop-pet -p avatar-host-2d)
verify_options=()
binaries=(desktop-pet avatar-host-2d)
if [ "${AX650_INCLUDE_CLI:-0}" = 1 ]; then
    build_packages+=(-p desktop-pet-ax650)
    verify_options+=(--include-cli)
    binaries+=(desktop-pet-ax650)
fi
cargo build --locked --release "${build_packages[@]}"
python3 tools/ax650/package.py --verify-binaries "$CARGO_TARGET_DIR/release" "${verify_options[@]}"
for binary in "${binaries[@]}"; do
    readelf --file-header "$CARGO_TARGET_DIR/release/$binary"
    libraries=$(ldd "$CARGO_TARGET_DIR/release/$binary")
    if [[ "$libraries" == *'not found'* ]]; then
        echo "$libraries" >&2
        exit 1
    fi
done
echo "Build and ELF/GLIBC/direct dependency checks passed: $CARGO_TARGET_DIR/release"
echo 'Dynamic XKB/graphics dependency preflight is a separate target-machine check.'
