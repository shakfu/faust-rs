# faust-ffi

Unified C/C++ FFI distribution crate — owns the canonical `libfaust` artifacts.

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
cargo build -p faust-ffi --release
```

The crate produces the platform `libfaust` static and dynamic libraries under
`target/release/`. The maintained C and C++ headers remain in the source FFI
crates:

- `../interp-ffi/include/interpreter-dsp-c.h` and `interpreter-dsp.h`
- `../cranelift-ffi/include/cranelift-dsp-c.h` and `cranelift-dsp.h`
- `../box-ffi/include/libfaust-box-c.h` and `libfaust-box.h`
- `../signal-ffi/include/libfaust-signal-c.h` and `libfaust-signal.h`

See the workspace [C and C++ usage guide](../../README.md#use-libfaust-from-c-and-c)
for complete Interpreter and Cranelift lifecycle examples.
