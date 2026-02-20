# transform

Mid-level transform passes over signals and FIR.

This crate hosts transformations that sit between propagation and backend emission:
neither parser/eval/propagate concerns nor final code-generation, but the lowering
layer that turns a signal graph into a structured FIR module.

## C++ provenance

| C++ path | Role |
|---|---|
| `compiler/transform/*` | Transform pass infrastructure |
| `compiler/generator/*` (selected) | FIR-oriented lowering helpers |

## Public API

| Module | Description |
|---|---|
| `signal_fir` | Signal → FIR lowering; main entry point is `compile_signals_to_fir_fastlane` |

### `signal_fir` key items

| Item | Description |
|---|---|
| `compile_signals_to_fir_fastlane` | Full signal list → `FirStore` + roots |
| `SignalFirOptions` | Lowering options (class name, sample rate, …) |
| `SignalFirError` / `SignalFirErrorCode` | Typed errors with stable diagnostic codes |

## Status

- `signal_fir` is fully implemented for the active slice and covered by integration
  tests and golden checks against the C++ reference.
- Additional transform families (scheduling, vectorisation, algebraic rewrites) are
  planned but not yet exposed as stable public APIs.

## Position in the pipeline

```
signals  →  [transform::signal_fir]  →  fir  →  codegen
```
