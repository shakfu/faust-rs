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

## Pipeline

```
parse → eval → propagate → [optional signal→FIR] → codegen (C / C++)
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
- Route backend generation to C/C++ emitters with consistent options.
