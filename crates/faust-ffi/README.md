# faust-ffi

Unified C/C++ FFI distribution crate — owns the canonical `libfaust` artifacts.

This crate links `interp-ffi`, `cranelift-ffi`, and `box-ffi` as Rust libraries
and distributes their exported `extern "C"` symbols through a single top-level
`staticlib` + `cdylib`.

## Public API

| Re-export | Source crate | Description |
|---|---|---|
| `box_api` | `faust_box` (`box-ffi`) | Box manipulation C API |
| `cranelift` | `faust_cranelift` (`cranelift-ffi`) | Cranelift JIT backend C API |
| `interp` | `interp_ffi` (`interp-ffi`) | Interpreter backend C API |

For per-backend API details, see each backend crate's README.
