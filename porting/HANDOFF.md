# Session Handoff

Date: 2026-07-14

## Repo State

- Branch: `ondemand-vec-fad-synthesis`
- HEAD: the commit containing this handoff
- Current task: P6.4 checked final vector module integration, completed in this
  commit.
- P6.4 activates the checked path only for the supported pure subset.

## Working Tree

- This commit adds final output/lifecycle assembly, checked production
  selection, named fallback reporting, integration tests, and planning updates.
- Many unrelated pre-existing untracked scratch files remain untouched.

## What Changed

- Added canonical delay storage and loop phase DTOs derived only from verified
  decoration and vector-plan artifacts.
- Matched C++ copy geometry `R=4*ceil(D/4)`, temporary length `R+V`, and ring
  geometry `N=next_power_of_two(D+V)` with index/save transitions.
- Grouped recursive projection aliases by symbolic index and emitted one
  simultaneous `RecursionStep` per group and sample in its serial loop.
- Added independent coverage/geometry/phase checking and fail-closed clock/AD
  resource diagnostics.
- Refined P5.3 managed effects into loop-pre/sample/loop-post events, with phase
  barriers and recursion-step chains checked under all four `-ss` strategies.
- Added bounded copy/ring simulation models and exhaustive `DelaySim` checks
  against newest-first abstract history.
- Added one checked serial island per clock domain with exact wrapper, parent,
  guard, member-signal, nested-loop, and clock-state facts.
- Partitioned P5 transports into top-rate outer-chunk, domain-rate
  island-scalar, and persistent held-output routes so guarded code cannot reuse
  an outer chunk index or lose `PermVar` lifetime.
- Accepted propagated FAD as an ordinary signal graph and added explicit
  scalar `Forward < Reverse` fallbacks for reverse-time/BRA carriers.
- Added recursively checked FIR tuple constructors with deterministic types.
- Kept tuple transport fail-closed: C++-compatible scalar projections, not a
  new tuple array ABI, cross loop boundaries.
- Closed P5 routed-FIR verification for a real two-projection recursion under
  all four `-ss` strategies.
- Materialized P6.1 copy/ring delay words and simultaneous recursive
  projection captures into explicit loop `pre/exec/post` FIR.
- Rematerialized all P6.2 transport modes: outer chunk arrays, island-local
  stack scalars below guards, and cleared persistent held outputs.
- Nested single-fire P4 loop bodies under OD/US/DS guards and exact parent
  domains, with a checker for action, island, and lifetime coverage.
- Added output stores and both C++-shaped `-lv` chunk drivers around accepted
  P6.3b bodies.
- Assembled and independently checked all lifecycle sections before returning
  a production vector module.
- Activated the full checked P4/P5/P6 chain for pure programs under every
  `-ss`, while retaining named fallbacks for unsupported state/UI shapes.
- Propagated `VectorPipelineStatus` through transform and compiler FIR outputs.

## Decisions And Constraints

- Only exact delay/recursion resources covered by the P6.1 artifact replace
  conservative P5.3 effects; all other state remains rejected or serial.
- Exhaustive bounded simulation is executable evidence, not an unbounded proof.
- `VectorRouteSession` accepts tuple-valued definitions but intentionally
  rejects tuple transports; P6.3b lowers scalar projections into state words.
- No C/C++ ABI changes; the Rust FIR output API gains an adapted diagnostic
  status describing certified selection versus fallback.
- Full R4/R5 still requires stateful/clocked production region lowering,
  differential execution, serialization/Lean acceptance, and fallback removal.

## Validation

- `cargo fmt --all -- --check` passes.
- `cargo test -p transform signal_fir::vector_state --lib` passes (5 tests).
- `cargo test -p transform signal_fir::vector_events --lib` passes (8 tests).
- `cargo test -p transform signal_fir::vector_clock_ad --lib` passes (7 tests).
- `cargo test -p transform signal_fir::vector_route --lib` passes (11 tests).
- `cargo test -p transform signal_fir::vector_assemble --lib` passes (2 tests).
- `cargo test -p transform signal_fir::vector_module --lib` passes (3 tests).
- `cargo test -p compiler --test vector_mode` passes (6 tests).
- `cargo test -p compiler --test vector_clock_ad` passes (2 tests).
- `cargo test -p transform signal_fir::vector --lib` passes (93 tests).
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace --all-targets` passes.
- `cargo run -p xtask -- golden-check` passes all 190 snapshots unchanged.

## Next Steps

1. Replace the pure-only region producer with production stateful
   recursion/delay and clock/FAD lowering so those named fallbacks can enter
   the checked final-module path.
2. Run scalar/vector differential execution for stateful, clocked, and FAD
   fixtures under `-lv 0/1` and every `-ss` strategy, then gate backends.
3. Complete canonical JSON/hash, Lean R3/R4 checking, and the stable C++
   vectorization-retention corpus.

## Useful Commands

- `cargo test -p transform signal_fir::vector_events --lib`
- `cargo test -p transform signal_fir::vector_state --lib`
- `cargo test -p transform signal_fir::vector_clock_ad --lib`
- `cargo test -p transform signal_fir::vector_route --lib`
- `cargo test -p transform signal_fir::vector_assemble --lib`
- `cargo test -p transform signal_fir::vector_module --lib`
- `cargo test -p compiler --test vector_mode`
- `cargo test -p compiler --test vector_clock_ad`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`
