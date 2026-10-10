#!/usr/bin/env bash
# Reuse the other chat's tested shared AXERA build. No inference or installation.
set -euo pipefail
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo 'Usage: fetch-sherpa.sh NATIVE_CACHE_DIRECTORY [SSH_HOST]' >&2
    exit 2
fi
python3 "$(dirname -- "$0")/native-runtime.py" "$1" --host "${2:-ax650}" \
    --build-root "${AX650_SHERPA_BUILD_ROOT:-/root/Sherpa-ZipVoice-AXERA-PR}"
