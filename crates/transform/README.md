# transform

Mid-level transform passes between signal propagation and backend emission:
staging/verification of the propagated signal forest, clock-domain analysis,
dependency scheduling, and signal-to-FIR lowering — in production for both the
scalar path and the independently checked vector (`-vec`) path.

## Position in the pipeline

```
propagate → [signal_prepare] → [clk_env / hgraph / schedule] → [signal_fir] → fir → codegen
```

## Public modules

| Module | Role |
|---|---|
| `signal_prepare` | Arena-owning staging boundary: clone, normalize, type, and verify the propagated forest before lowering |
| `clk_env` | Clock-environment inference for `ondemand`/`upsampling`/`downsampling` domains |
| `hgraph` | Hierarchical dependency graph, effect-conflict orientation, and audits over the prepared forest |
| `schedule` | Dependency scheduling shared by the scalar and vector paths (`SchedulingStrategy`, `-ss 0..3`) |
| `signal_fir` | Signal→FIR lowering: scalar lowerer, checked vector pipeline, selection and fail-closed fallback |

## Scalar and vector paths

```text
propagated signal forest + UiProgram
              |
              v
signal_prepare -> VerifiedPreparedSignals
              |
              +---------------- scalar -----------------+
              |                                          |
              |             clk_env / hgraph / schedule  |
              |                         |                |
              |                         v                |
              |                 scalar SignalToFirLower  |
              |                                          |
              +---------------- vector -----------------+
                                        |
        analysis -> decorations -> VectorPlan -> state/clock policy
                                        |
                route -> lower -> event certificate -> FIR assembly
                                        |
                             final module verification
                                        |
                                        v
                                  SignalFirOutput
```

With `ComputeMode::Vector` (`-vec`), the checked vector pipeline runs first;
every stage produces an artifact that an **independent checker** must accept
(see `signal_fir/vector/mod.rs` for the authoritative stage map). Any named
unsupported shape fails **closed** to scalar lowering, reported through
`VectorPipelineStatus::Fallback(reason)` with a stable `FRS-VEC-FALLBACK-*`
code and `VectorEffectiveMode::Scalar`. A fallback is never silently counted
as vector coverage.

Lifecycle ownership follows the C++ Faust contract: persistent fields belong
to the DSP struct, compiled constants to `instanceConstants`, resettable
signal state to `instanceClear`, and UI zone resets to
`instanceResetUserInterface`.

## API classification

| Tier | Items |
|---|---|
| Stable compiler contract | `signal_fir::{compile_signals_to_fir_fastlane_with_ui, compile_signals_to_fir_fastlane_clocked, compile_signals_to_fir_fastlane_clocked_with_timing}`, `SignalFirOptions`, `SignalFirOutput`, `SignalFirError`/`SignalFirErrorCode`, `RealType`, `ComputeMode`, `VectorPipelineStatus`, `VectorFallbackReason`, `VectorEffectiveMode`, `schedule::SchedulingStrategy`, `signal_prepare::{prepare_signals_for_fir, prepare_signals_for_fir_verified, PreparedSignals, VerifiedPreparedSignals}` |
| Diagnostic / testing surface | `clk_env::annotate`, `hgraph::{build_hgraph, audit_hgraph, audit_control_variability, schedule}`, `signal_fir::decoration_verify`, `signal_fir::shadow`, `signal_fir::pv_slice`, the vector artifact producers/checkers under `signal_fir::vector::*` |
| Compatibility facade | `signal_fir::vector_*` aliases of the grouped `signal_fir::vector::{...}` modules (retained during the 2026-07 cleanup; do not remove without an explicit API decision) |

### `signal_fir` key items

| Item | Description |
|---|---|
| `compile_signals_to_fir_fastlane_with_ui(arena, sigs, n_in, n_out, ui, opts)` | Canonical grouped-UI-aware entry point |
| `compile_signals_to_fir_fastlane_clocked(..., clock_domains, opts)` | Clock-domain-aware variant (`ondemand`/`upsampling`/`downsampling`) |
| `SignalFirOptions` | `module_name`, `real_type`, delay thresholds (`-mcd`/`-dlt`), `compute_mode` (`-vec -vs -lv`), `scheduling_strategy` (`-ss`) |
| `SignalFirOutput` | `FirStore` + module root + vector status/effective mode + diagnostics |
| `SignalFirError` / `SignalFirErrorCode` | Typed errors with stable `FRS-SFIR-*` codes |

### `signal_prepare` key items

| Item | Description |
|---|---|
| `prepare_signals_for_fir(arena, sigs, ui)` | Clone into a private staging arena, normalize, type, and verify fast-lane invariants |
| `prepare_signals_for_fir_verified(arena, sigs, ui)` | Same, returned as the `VerifiedPreparedSignals` wrapper consumed by lowering |
| `PreparedSignals` | Encapsulated staging result with read-only accessors |
| `SimpleSigType` | Reduced type domain (`Int` / `Real` / `Sound`) |

## Validation

```bash
cargo test -p transform --lib                                  # unit tests
cargo run -p xtask -- golden-check                             # generated-output parity
cargo run -p xtask -- vector-coverage-check                    # certified vector retention
cargo run --release -p xtask -- vector-compile-budget-check    # compile-cost budget
cargo test -p compiler --test vector_mode                      # scalar/vector bit-exactness oracle
```

## Active plans

- [`porting/transform-cleanup-documentation-factorization-plan-2026-07-19-en.md`](../../porting/transform-cleanup-documentation-factorization-plan-2026-07-19-en.md)
- [`porting/vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md`](../../porting/vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md)
- [`porting/scheduling-vectorization-implementation-review-2026-07-16-en.md`](../../porting/scheduling-vectorization-implementation-review-2026-07-16-en.md)

## C++ provenance

| C++ path | Role |
|---|---|
| `compiler/transform/*` | Transform pass infrastructure |
| `compiler/generator/dag_instructions_compiler.cpp`, `compile_vect.cpp` | Vector loop DAG, delay-line words, placement rules |
| `compiler/generator/compile_scal.cpp` | Scalar lowering and `ondemand` guard generation |
| `compiler/Dependencies/*`, `compiler/generator/occurrences.cpp` | Dependency and occurrence rules |
