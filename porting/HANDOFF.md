# Session Handoff

Date: 2026-07-14

## Repo State

- Branch: `ondemand-vec-fad-synthesis`
- HEAD: the commit containing this handoff
- Current task: P6.2 clock-island and AD execution policy, completed in this
  commit.
- P6.1 is committed at `58b6d330`; P6.2 remains additive and does not activate
  production vector lowering.

## Working Tree

- This commit adds the P6.2 producer/checker, clock and reverse-window reference
  simulations, focused compiler integration tests, and planning updates.
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

## Decisions And Constraints

- Only exact delay/recursion resources covered by the P6.1 artifact replace
  conservative P5.3 effects; all other state remains rejected or serial.
- Exhaustive bounded simulation is executable evidence, not an unbounded proof.
- `VectorRouteSession` still rejects tuple-valued FIR definitions, so real
  recursive tuple assembly remains open even though signal-level P6.1 planning
  is tested on a real multi-projection graph.
- No CLI, ABI, `build_module`, or backend behavior changes in either P6 slice.
- Full R4/R5 still requires FIR phase/island emission, tuple routing,
  output/module assembly, serialization/Lean acceptance, and backend gating.

## Validation

- `cargo fmt --all` passes.
- `cargo test -p transform signal_fir::vector_state --lib` passes (5 tests).
- `cargo test -p transform signal_fir::vector_events --lib` passes (8 tests).
- `cargo test -p transform signal_fir::vector_clock_ad --lib` passes (7 tests).
- `cargo test -p compiler --test vector_clock_ad` passes (2 tests).
- `cargo test -p transform signal_fir::vector --lib` passes (77 tests).
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace --all-targets` passes.
- `cargo run -p xtask -- golden-check` passes all 190 snapshots unchanged.

## Next Steps

1. Add tuple-valued FIR routing and materialize accepted P6.1 phases plus P6.2
   islands and island-local transports in final region bodies.
2. Add output/module assembly and connect `build_module` only after P5.3/P6
   acceptance; then activate compiler options and backends.
3. Complete canonical JSON/hash, Lean R3/R4 checking, and the stable C++
   vectorization-retention corpus.

## Useful Commands

- `cargo test -p transform signal_fir::vector_events --lib`
- `cargo test -p transform signal_fir::vector_state --lib`
- `cargo test -p transform signal_fir::vector_clock_ad --lib`
- `cargo test -p compiler --test vector_clock_ad`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`
