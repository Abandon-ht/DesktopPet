# sherpa-onnx Rust ABI overlay

Source: https://github.com/Abandon-ht/sherpa-onnx/tree/5473e19db9c08e63aa41e4c273d9b3f743f1cd25/sherpa-onnx/rust
Branch: `codex/zipvoice-axera`; package version remains 1.13.8. Apache-2.0.

These are the fork's Rust crates, copied without examples or model files. The only
code changes add `OfflineCtcFstDecoderConfig` to the raw offline recognizer config
and initialize it with null graph / zero max_active in the safe wrapper. The C API
at the pinned revision appended that field but its Rust bindings had not followed;
the native constructor reads it unconditionally. Keeping the overlay in this
repository makes the build reproducible without changing the other chat's branch.

AX650 links the same revision's AXERA-enabled shared C API and ONNX Runtime.
Other targets keep the upstream 1.13.8 static library; the appended zero field is
ignored by that older C API. No new speaker embedding APIs are used here.
`tools/ax650/check-sherpa-abi.py` compares Rust and C config sizes/offsets before
building. Update this overlay together with the pinned native runtime.
