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
| `signal_fir` | Signal → FIR lowering fast-lane |
| `signal_prepare` | Pre-FIR signal preparation (type inference, de-Bruijn resolution) |

### `signal_fir` key items

| Item | Description |
|---|---|
| `compile_signals_to_fir_fastlane_with_ui(arena, sigs, ui, opts)` | Full signal list + `UiProgram` → `SignalFirOutput` |
| `SignalFirOptions` | Lowering options (module name, real type, …) |
| `SignalFirOutput` | Output bundle: `FirStore` + module root `FirId` |
| `RealType` | Internal DSP precision (`Float32` / `Float64`) |
| `SignalFirError` / `SignalFirErrorCode` | Typed errors with stable diagnostic codes |

### `signal_prepare` key items

| Item | Description |
|---|---|
| `prepare_signals_for_fir(arena, sigs)` | Prepare propagated signals for FIR lowering |
| `PreparedSignals` | Result: staging arena + prepared roots + type annotations |
| `SimpleSigType` | Reduced type domain (`Int` / `Real` / `Sound`) |
| `SignalPrepareError` | Typed errors from the preparation pass |

## Status

- `signal_fir` is fully implemented for the active slice and covered by integration
  tests and golden checks against the C++ reference.
- Additional transform families (scheduling, vectorisation, algebraic rewrites) are
  planned but not yet exposed as stable public APIs.

## Position in the pipeline

```
signals  →  [transform::signal_fir]  →  fir  →  codegen
```
