# codegen

Backend code-generation from FIR (Faust Intermediate Representation).

This crate consumes a `FirStore` produced by `transform::signal_fir` and emits
target-language source text.  All backend option structs and signature-validation
helpers live here.

## C++ provenance

| C++ path | Role |
|---|---|
| `compiler/generator/*` | Generator base infrastructure |
| `compiler/generator/fir/*` | FIR visitor and helpers |
| `compiler/generator/<backend>/` | Backend-specific emitters |

## Public API

| Item | Description |
|---|---|
| `backends::cpp` | C++ emitter — `CppOptions`, `generate_cpp_module` |
| `backends::c` | C emitter — `COptions`, `generate_c_module` |
| `backends::interp` | Interpreter bytecode emitter — `InterpOptions`, `generate_interp_module`, `CompilerParts` |
| `fixtures` | Shared FIR fixtures used by backend tests and parity checks |

### Interpreter backend (`backends::interp`)

Emits Faust Bytecode (`.fbc`) for the built-in interpreter.  The pipeline is:

1. `FirToFbcCompiler<R>` compiles each FIR function body into a shared `FbcBlockArena`.
2. `generate_interp_module` maps the 6 known DSP function names (`staticInit`, `instanceConstants`, `instanceResetUserInterface`, `instanceClear`, `compute`, `computeThread`) to the 6 `FbcDspFactory` block slots.
3. `FbcDspFactory::optimize(level)` runs bytecode optimizer passes (levels 1–6).
4. `write_fbc` / `read_fbc` serialize/deserialize the factory to/from `.fbc` text.
5. `FbcDspInstance` provides the DSP lifecycle (`init`, `compute`) for in-process execution.

## Status

- **C and C++ backends** are fully implemented for the active module-first slice.
- **Interpreter backend** is fully implemented: bytecode compiler, optimizer (6 levels), factory, instance, and `.fbc` serialization round-trip.
- Other backend modules are scaffolded with stable identifiers and placeholders.

## Position in the pipeline

```
transform  →  [codegen]  →  emitted source text (C / C++)
                         →  interpreter bytecode text (.fbc)
```
