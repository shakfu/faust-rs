# Session Handoff

Date: 2026-07-13

## Repo State

- Branch: `ondemand-vec-fad-synthesis`
- HEAD: the commit containing this handoff
- Current task: P4.4 production `VectorPlan` construction
- P4.4 is implemented additively and included in this commit.

## Working Tree

- Committed changes: production vector-plan construction, strengthened plan
  verification, plan/certification documentation, today's journal/index, and
  this handoff.
- Many pre-existing untracked scratch files remain at the repository root and
  were left untouched.

## What Changed

- Added `signal_fir::vector_plan::build_vector_plan`, accepting only an opaque
  `VerifiedDecorationCertificate` and positive `vec_size`.
- Consolidated vector placement against certified occurrence/max-delay facts;
  the pass does not inspect the signal arena or fused FIR.
- Allocated deterministic root, recursion-group, and separated loops before
  scheduling; no `SchedulingStrategy` enters plan construction.
- Materialized nonduplicable values once, pre-planned typed cross-loop
  transports, and oriented conflicting effects deterministically.
- Returned only an opaque `VerifiedVectorPlan` after independent verification.
- Strengthened `verify_vector_plan` to derive duplicability and local
  `VecSafe`, check loop/epoch agreement and canonical witnesses, and reject
  unordered conflicting effects.

## Decisions And Constraints

- Empty effects and declared-pure foreign effects are the only duplicable sets.
- State reads/writes block local pointwise `VecSafe`; other effects are safe
  only when the loop graph orders every conflicting pair.
- All projections of one symbolic recursion group share one serial loop.
- Non-recursive unsafe loops conservatively use `Island`; precise clock epochs
  and state transitions remain P6.
- The current P4.4 plan has one forward epoch. AD and full clock-domain epoch
  construction remain P6.
- Table carriers are control values and fail closed if a numeric chunk
  transport is requested.
- P4.4 is not yet connected to FIR routing, compiler options, or backends.

## Validation

- `cargo fmt --all` passes.
- Focused vector analysis/plan/schedule/verifier tests pass (52 tests).
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace --all-targets` passes, including all 270 transform
  tests; pre-existing explicitly ignored tests remain ignored.
- `cargo run -p xtask -- golden-check` passes all 190 snapshots unchanged.

## Next Steps

1. P5: route signal-to-FIR lowering through `VerifiedVectorPlan` with the
   three-scope value cache and fail-closed transport resolution.
2. Add independent routed-FIR checks for region visibility, transport
   store/load pairing, effects, and value typing before backend activation.
3. P6: refine delay storage, recursive transitions, clock epochs, and AD
   execution semantics.
4. Complete canonical JSON/hash plus the Lean R3 checker and add the stable C++
   occurrence oracle/vectorization-retention corpus.

## Useful Commands

- `cargo test -p transform signal_fir::vector_plan --lib`
- `cargo test -p transform signal_fir::vector_verify --lib`
- `cargo test -p transform signal_fir::vector_schedule --lib`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`
