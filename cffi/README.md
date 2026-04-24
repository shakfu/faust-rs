# cffi

Legacy documentation landing area for C/C++ API exposure.

The active FFI implementation now lives in workspace crates:

| Crate | Role |
|---|---|
| `crates/box-ffi` | Faust-style box manipulation C/C++ API |
| `crates/interp-ffi` | Interpreter backend C/C++ API |
| `crates/cranelift-ffi` | Experimental Cranelift backend C/C++ API |
| `crates/faust-ffi` | Unified `libfaust` distribution crate linking backend exports |
| `crates/wasm-ffi` | Raw WASM ABI for the Rust-backed `faustwasm` compiler module |

Keep new API notes close to the owning crate README. Use this directory only for
cross-cutting C FFI notes that do not belong to a specific backend crate.
