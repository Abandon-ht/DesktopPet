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
cargo build --locked --release -p desktop-pet-ax650 -p avatar-host-2d
python3 tools/ax650/package.py --verify-binaries "$CARGO_TARGET_DIR/release"
for binary in desktop-pet-ax650 avatar-host-2d; do
    readelf --file-header "$CARGO_TARGET_DIR/release/$binary"
    libraries=$(ldd "$CARGO_TARGET_DIR/release/$binary")
    if [[ "$libraries" == *'not found'* ]]; then
        echo "$libraries" >&2
        exit 1
    fi
done
echo "Build and ELF/GLIBC/direct dependency checks passed: $CARGO_TARGET_DIR/release"
echo 'Dynamic XKB/graphics dependency preflight is a separate target-machine check.'
