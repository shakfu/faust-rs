# faust-ffi

Unified C/C++ FFI distribution crate — owns the canonical `libfaust-rs` artifacts.

This crate links `interp-ffi`, `cranelift-ffi`, `box-ffi`, and `signal-ffi` as
Rust libraries and distributes their exported `extern "C"` symbols through a
single top-level `staticlib` + `cdylib`.

## Public API

| Re-export | Source crate | Description |
|---|---|---|
| `box_api` | `box_ffi` (`box-ffi`) | Box manipulation C and C++ API |
| `signal_api` | `signal_ffi` (`signal-ffi`) | Signal manipulation C and C++ API |
| `cranelift` | `cranelift_ffi` (`cranelift-ffi`) | Cranelift JIT backend C and C++ API |
| `interp` | `interp_ffi` (`interp-ffi`) | Interpreter backend C and C++ API |

For per-backend API details, see each backend crate's README.

## Build

```bash
cargo run -p xtask -- build-libfaust --release
```

The packaging command produces `libfaust-rs.a` plus the platform dynamic
library (`libfaust-rs.dylib`, `libfaust-rs.so`, or `faust-rs.dll`) under
`target/release/`. The maintained C and C++ headers remain in the source FFI
crates:

- `../interp-ffi/include/interpreter-dsp-c.h` and `interpreter-dsp.h`
- `../cranelift-ffi/include/cranelift-dsp-c.h` and `cranelift-dsp.h`
- `../box-ffi/include/libfaust-box-c.h` and `libfaust-box.h`
- `../signal-ffi/include/libfaust-signal-c.h` and `libfaust-signal.h`

See the workspace [C and C++ usage guide](../../README.md#use-libfaust-rs-from-c-and-c)
for complete Interpreter and Cranelift lifecycle examples.
