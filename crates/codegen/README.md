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
| `fixtures` | Shared FIR fixtures used by backend tests and parity checks |

## Status

- **C and C++ backends** are fully implemented for the active module-first slice.
- Other backend modules are scaffolded with stable identifiers and placeholders.

## Position in the pipeline

```
transform  →  [codegen]  →  emitted source text (C / C++)
```
