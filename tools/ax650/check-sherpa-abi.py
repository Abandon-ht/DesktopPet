#!/usr/bin/env python3
"""Compare the pinned C header with Rust FFI layouts without loading a model."""
import argparse
import hashlib
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
HEADER_SHA = "4645835a245bf5c7907544f64a367747700171d9929fe24490ea8d8b6e7f25cc"


def check(header):
    if hashlib.sha256(header.read_bytes()).hexdigest() != HEADER_SHA:
        raise ValueError("C API header is not the pinned ZipVoice AXERA revision")
    raw = ROOT / "vendor/zipvoice-axera/sherpa-onnx-sys/src"
    names = {
        "offline_asr.rs": ("OfflineCtcFstDecoderConfig", "OfflineRecognizerConfig", "OfflineModelConfig", "OfflineSenseVoiceModelConfig"),
        "tts.rs": ("OfflineTtsConfig", "OfflineTtsModelConfig", "OfflineTtsZipvoiceModelConfig", "SherpaOnnxGenerationConfig"),
        "kws.rs": ("KeywordSpotterConfig",),
        "vad.rs": ("VadModelConfig",),
        "online_asr.rs": ("OnlineModelConfig", "FeatureConfig"),
    }
    c = ['#include <stddef.h>', '#include <stdio.h>', '#include "c-api.h"', 'int main(void) {']
    rust = ['extern crate sherpa_sys;', 'use sherpa_sys::*;', 'fn main() {']
    for filename, structs in names.items():
        text = (raw / filename).read_text()
        for name in structs:
            c_name = name if name.startswith("SherpaOnnx") else "SherpaOnnx" + name
            body = re.search(r"pub struct " + name + r" \{(.*?)\n\}", text, re.S).group(1)
            fields = re.findall(r"pub (\w+):", body)
            c.append(f'printf("{name} %zu %zu", sizeof({c_name}), _Alignof({c_name}));')
            rust.append(f'print!("{name} {{}} {{}}", std::mem::size_of::<{name}>(), std::mem::align_of::<{name}>());')
            for field in fields:
                c.append(f'printf(" %zu", offsetof({c_name}, {field}));')
                rust.append(f'print!(" {{}}", std::mem::offset_of!({name}, {field}));')
            c.append('printf("\\n");')
            rust.append('println!();')
    c.append('return 0; }')
    rust.append('}')
    with tempfile.TemporaryDirectory(prefix="sherpa-abi-") as directory:
        work = Path(directory)
        (work / "probe.c").write_text("\n".join(c))
        (work / "probe.rs").write_text("\n".join(rust))
        subprocess.run(["cc", "-std=c11", "-I", str(header.parent), str(work / "probe.c"), "-o", str(work / "c")], check=True)
        subprocess.run(["rustc", "--edition=2021", "--crate-type=rlib", "--crate-name=sherpa_sys", str(raw / "lib.rs"), "-o", str(work / "libsherpa_sys.rlib")], check=True)
        subprocess.run(["rustc", "--edition=2021", str(work / "probe.rs"), "--extern", f"sherpa_sys={work / 'libsherpa_sys.rlib'}", "-o", str(work / "rust")], check=True)
        native = subprocess.check_output([str(work / "c")], text=True)
        binding = subprocess.check_output([str(work / "rust")], text=True)
        if native != binding:
            raise ValueError(f"C / Rust ABI mismatch:\nC:\n{native}\nRust:\n{binding}")
        print(native, end="")
        print("C / Rust sizes, alignments and all config field offsets match (12 structs)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("header", type=Path)
    check(parser.parse_args().header)
