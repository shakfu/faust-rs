# Session Handoff

Date: 2026-07-14

## Repo State

- Branch: `ondemand-vec-fad-synthesis`
- HEAD: the commit containing this handoff
- Current task: P5.2 actual pure signal closure lowering and local CSE
- P5.2 is implemented additively in this commit.

## Working Tree

- Committed changes add the P5.2 lowerer, expose the shared scalar binary-op
  mapping and P5.1 plan accessor, and update planning/certification documents.
- Many unrelated pre-existing untracked scratch files remain untouched.

## What Changed

- Added `lower_pure_vector_program`, consuming `VerifiedPreparedSignals` and
  `VerifiedVectorPlan` without re-running placement or allocating routes.
- Lowered actual effect-free constants, inputs, casts, selects, typed binary
  operators, min/max/abs, output wrappers, and math nodes into scheduled loops.
- Kept provisional caches per control/loop closure and resolved sibling values
  only through `VectorRouteSession` transports.
- Ran CSE independently in each region before sealing owned definitions and
  emitted loop-id-derived temporary names.
- Added `verify_pure_vector_bodies` to reconnect final CSE bodies to P5.1 route
  definitions, stores, loads, and uses.

## Decisions And Constraints

- The internal API is adapted from C++ `DAGInstructionsCompiler`: Rust separates
  verified allocation/routing from pure expression lowering. No CLI/ABI changes.
- P5.2 supports only effect-free pointwise closures. State, tables, UI, foreign
  calls, clocks, recursion, and AD fail closed until P6.
- The artifact records required math/integer helpers but is not assembled by
  `build_module`; no backend behavior changes in this slice.
- Full R4 still requires event/effect evidence, state transitions, complete
  epoch bodies, output/module assembly, and Lean-side acceptance.

## Validation

- `cargo fmt --all` passes.
- `cargo test -p transform signal_fir::vector_lower --lib` passes (4 tests).
- `cargo test -p transform signal_fir::vector --lib` passes (64 tests).
- `cargo clippy -p transform --all-targets -- -D warnings` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace --all-targets` passes.
- `cargo run -p xtask -- golden-check` passes all 190 snapshots unchanged.

## Next Steps

1. P5.3: implement the bounded event-order/FissionSafe checker and attach
   complete routed effect/epoch-order evidence to the final-body gate.
2. P6: route delay storage, recursion transitions, clock epochs, and AD
   execution semantics through the same region artifact.
3. Add output/module assembly and connect `build_module` only after P5.3/P6
   acceptance; then activate compiler options and backends.
4. Complete canonical JSON/hash, Lean R3/R4 checking, and the stable C++
   vectorization-retention corpus.

## Useful Commands

- `cargo test -p transform signal_fir::vector_lower --lib`
- `cargo test -p transform signal_fir::vector_route --lib`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`
