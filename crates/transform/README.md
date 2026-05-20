# transform

Mid-level transform passes over signals and FIR.

This crate hosts transformations that sit between propagation and backend
emission: neither parser/eval/propagate concerns nor final code-generation, but
the lowering layer that prepares a propagated signal forest and turns it into a
structured FIR module.

## C++ provenance

| C++ path | Role |
|---|---|
| `compiler/transform/*` | Transform pass infrastructure |
| `compiler/generator/*` (selected) | FIR-oriented lowering helpers |

## Public API

| Module | Description |
|---|---|
| `signal_fir` | Signal → FIR lowering fast-lane |
| `signal_prepare` | Pre-FIR staging/preparation boundary for the fast-lane |

### `signal_fir` key items

| Item | Description |
|---|---|
| `compile_signals_to_fir_fastlane_with_ui(arena, sigs, num_inputs, num_outputs, ui, opts)` | Canonical grouped-UI-aware fast-lane entrypoint |
| `SignalFirOptions` | Lowering options (`module_name`, `real_type`, delay strategy thresholds, …) |
| `SignalFirOutput` | Output bundle: `FirStore` + module root `FirId` |
| `RealType` | Internal DSP precision (`Float32` / `Float64`) |
| `SignalFirError` / `SignalFirErrorCode` | Typed errors with stable diagnostic codes |

### `signal_prepare` key items

| Item | Description |
|---|---|
| `prepare_signals_for_fir(arena, sigs, ui)` | Prepare propagated signals into a private staging arena and verify fast-lane invariants |
| `prepare_signals_for_fir_verified(arena, sigs, ui)` | Same preparation step, returned as a verified wrapper for downstream lowering |
| `PreparedSignals` | Encapsulated staging result with read-only accessors for arena, outputs, and type maps |
| `VerifiedPreparedSignals` | Checked prepared forest that passed explicit postcondition verification |
| `SimpleSigType` | Reduced type domain (`Int` / `Real` / `Sound`) |
| `SignalPrepareError` | Typed errors from the preparation pass |

## Status

- `signal_prepare` owns the pre-FIR staging boundary used by the active
  fast-lane:
  - clone the output forest into a private arena,
  - run forest-wide `de_bruijn_to_sym`,
  - canonicalize degenerate symbolic recursion projections,
  - infer canonical `SigType` information,
  - derive the reduced `SimpleSigType` view used by FIR lowering,
  - insert the current promotion subset, simplify, and canonicalize one-sample
    delays back to `Delay1`,
  - explicitly verify the resulting staging contract before FIR lowering.
- `signal_fir` is implemented for the active fast-lane slice and covered by
  integration tests and golden checks.
- `signal_fir` owns concrete `BlockReverseAD` execution for `rad(...)`
  temporal/recursive fallback:
  - forward tape allocation and stores,
  - reverse sweep scheduling,
  - adjoint carry reset/storage,
  - FIR type selection for tape values and local math helpers.
  Local pointwise RAD formulas are shared with `propagate` through
  `signals::ad_rules`; tape loading and loop placement remain local here.
- The current fast-lane covers the executable bootstrap path plus the active
  delay/recursion/table lowering slices. Delay lowering is controlled by:
  - `max_copy_delay` for shift/copy vs circular buffering,
  - `delay_line_threshold` for circular-pow2 vs exact-size if-wrapping buffers.
- Additional transform families (scheduling, vectorisation, algebraic rewrites) are
  planned but not yet exposed as stable public APIs.

## Position in the pipeline

```
signals  →  [transform::signal_prepare]  →  [transform::signal_fir]  →  fir  →  codegen
```
