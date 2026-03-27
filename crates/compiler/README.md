# compiler

Top-level compiler facade.  Wires all pipeline stages together behind a single
`Compiler` struct and a unified `CompilerError` surface.

## C++ provenance

| C++ file | Role |
|---|---|
| `compiler/libcode.cpp` | Compile entry points and orchestration |
| `compiler/global.cpp` | Session lifecycle |

## Public API

| Item | Description |
|---|---|
| `Compiler` | Session handle; all compile entry points are methods |
| `CompilerError` | Aggregated error type covering every pipeline stage |
| `SignalCompileOutput` | Parse + eval + propagate result package |
| `enrobage` | Architecture-file wrapping (Step E) |

### `Compiler` entry points

| Method group | Output |
|---|---|
| `compile_source_to_cpp[_with_lane]` | C++ source string |
| `compile_file_to_cpp[_with_lane]` | C++ source string |
| `compile_source_to_c[_with_lane]` | C source string |
| `compile_file_to_c[_with_lane]` | C source string |
| `compile_source_to_interp[_with_lane]` | `.fbc` bytecode string |
| `compile_file_to_interp[_with_lane]` | `.fbc` bytecode string |
| `compile_file_default_to_interp[_with_lane]` | `.fbc` bytecode string |
| `compile_source_to_wasm[_with_lane]` | `WasmModule` (`.wasm` + companion JSON) |
| `compile_file_to_wasm[_with_lane]` | `WasmModule` (`.wasm` + companion JSON) |
| `compile_file_default_to_wasm[_with_lane]` | `WasmModule` (`.wasm` + companion JSON) |
| `compile_wasm_artifact` | Owned `WasmArtifactBundle` |
| `compile_file_to_wasm_artifact[_with_lane]` | Owned `WasmArtifactBundle` |
| `compile_file_default_to_wasm_artifact` | Owned `WasmArtifactBundle` |
| `compile_source_to_json[_with_lane]` | Strict Faust JSON string |

## Pipeline

```
parse → eval → propagate → [optional signal→FIR] → codegen (C / C++ / .fbc / WASM / JSON)
```

Two lanes coexist to de-risk migration:

| Lane | Description |
|---|---|
| `SignalFirLane::LegacyBridge` | Original signal→FIR path |
| `SignalFirLane::TransformFastLane` | New `transform::signal_fir` path |

## Facade responsibilities

- Provide one orchestrator type (`Compiler`) for file-based compilation.
- Aggregate typed stage errors into one top-level `CompilerError`.
- Provide test/golden-oriented helper outputs (box dump, signal dump, FIR dump).
- Route backend generation to C, C++, interpreter bytecode, WASM/JSON artifacts, and strict JSON emitters with consistent options.
