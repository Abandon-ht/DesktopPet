#!/usr/bin/env bash
# Source-built CI runtime. Separate from the checksum-pinned board runtime.
set -euo pipefail
if [ "$#" -ne 3 ]; then
    echo 'Usage: ci-native.sh SHERPA_SOURCE SDK_SOURCE OUTPUT_DIRECTORY' >&2
    exit 2
fi
source /etc/os-release
if [ "${ID:-}" != ubuntu ] || [ "${VERSION_ID:-}" != 22.04 ] || [ "$(uname -m)" != aarch64 ]; then
    echo 'CI requires native Ubuntu 22.04 ARM64' >&2
    exit 1
fi
sherpa_source=$(realpath "$1")
sdk_source=$(realpath "$2")
mkdir -p "$3"
native_root=$(realpath "$3")
test "$(git -C "$sherpa_source" rev-parse HEAD)" = 5473e19db9c08e63aa41e4c273d9b3f743f1cd25
test "$(git -C "$sdk_source" rev-parse HEAD)" = 71ac84128ebc96219a9bd4348a49c43527bc605d
test -z "$(git -C "$sherpa_source" status --porcelain --untracked-files=no)"
test -z "$(git -C "$sdk_source" status --porcelain --untracked-files=no)"
sdk_root="$sdk_source/msp/out"
export SHERPA_ONNX_AXERA_LIB_DIR="$sdk_root/lib"
export CPLUS_INCLUDE_PATH="$sdk_root/include${CPLUS_INCLUDE_PATH:+:$CPLUS_INCLUDE_PATH}"
export LD_LIBRARY_PATH="$sdk_root/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
python3 tools/ax650/check-sherpa-abi.py "$sherpa_source/sherpa-onnx/c-api/c-api.h"
cmake -S "$sherpa_source" -B "$native_root/build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON \
    -DCMAKE_BUILD_RPATH='$ORIGIN' \
    -DSHERPA_ONNX_ENABLE_AXERA=ON -DSHERPA_ONNX_ENABLE_TTS=ON \
    -DSHERPA_ONNX_ENABLE_C_API=ON -DSHERPA_ONNX_ENABLE_BINARY=OFF \
    -DSHERPA_ONNX_BUILD_C_API_EXAMPLES=OFF -DSHERPA_ONNX_ENABLE_PYTHON=OFF \
    -DSHERPA_ONNX_ENABLE_TESTS=OFF -DSHERPA_ONNX_ENABLE_PORTAUDIO=OFF \
    -DSHERPA_ONNX_ENABLE_WEBSOCKET=OFF -DSHERPA_ONNX_ENABLE_SPEAKER_DIARIZATION=OFF \
    -DSHERPA_ONNX_USE_PRE_INSTALLED_ONNXRUNTIME_IF_AVAILABLE=OFF
cmake --build "$native_root/build" --target sherpa-onnx-c-api --parallel "${CARGO_BUILD_JOBS:-4}"
mkdir -p "$native_root/lib" "$native_root/include" "$native_root/licenses"
cp -L "$native_root/build/lib/libsherpa-onnx-c-api.so" "$native_root/lib/"
# Preserve ONNX Runtime's SONAME symlink as well as the unversioned link name.
cp -a "$native_root/build/_deps/onnxruntime-src/lib/"libonnxruntime.so* "$native_root/lib/"
cp "$sherpa_source/sherpa-onnx/c-api/c-api.h" "$native_root/include/"
cp "$sherpa_source/LICENSE" "$native_root/licenses/sherpa-onnx.LICENSE"
cp "$native_root/build/_deps/onnxruntime-src/LICENSE" "$native_root/licenses/onnxruntime.LICENSE"
cp "$native_root/build/_deps/onnxruntime-src/ThirdPartyNotices.txt" "$native_root/licenses/onnxruntime.ThirdPartyNotices.txt"
echo "Source-built AXERA runtime: $native_root/lib (SDK remains build-only)"
