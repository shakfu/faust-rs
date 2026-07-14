# Session Handoff

Date: 2026-07-14

## Repo State

- Branch: `ondemand-vec-fad-synthesis`
- Base HEAD: `1ada5b26d318` (`Build production VectorPlan from verified decorations`)
- Current task: P5.1 region-aware routed-FIR construction and verification
- P5.1 is implemented in the working tree and remains additive.

## Working Tree

- Tracked changes cover P5.1 routing, a P4.4 root-promotion fix, planning and
  certification status, today's journal/index, and this handoff.
- `crates/transform/src/signal_fir/vector_route.rs` is the new untracked source
  file that belongs to this change.
- Many unrelated pre-existing untracked scratch files remain at the repository
  root and were left untouched.

## What Changed

- Added `VectorRouteSession`, consuming only `VerifiedVectorPlan` and the common
  `SchedulingStrategy` to materialize strategy-dependent loop-region order.
- Added distinct control, owned-loop, loop-local inline, and transport-load
  caches with fail-closed visibility rules.
- Declared every canonical typed transport up front; emitted producer stores
  and consumer loads only for exact P4.4 routes at `i0 - vindex`.
- Added independent `verify_routed_fir` checks for definitions, types, scopes,
  transport shapes, and producer/consumer linkage.
- Fixed P4.4 construction when a later sample root was visited inline while
  traversing an earlier root; it is now promoted to the shared root loop.

## Decisions And Constraints

- Routing cannot allocate storage or names. P4.4 remains the sole transport
  allocation authority.
- `Control` values are ancestor-visible; `Inline` values exist per exact loop;
  `Owned` values are direct only in their owner and transported elsewhere.
- This is an adapted internal C++ mapping with no external CLI/ABI change.
- The new route is not connected to `build_module`. P6 must define complete
  state transitions before stateful lowering can safely use these regions.
- Full R4 is not claimed: effect traces, epoch-body order, per-region CSE,
  actual signal-expression routing, and backend activation remain open.

## Validation

- `cargo fmt --all` passes.
- `cargo test -p transform signal_fir::vector_route --lib` passes (7 tests).
- `cargo test -p transform signal_fir::vector_plan --lib` passes (6 tests).
- `cargo clippy -p transform --all-targets -- -D warnings` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace --all-targets` passes.
- `cargo run -p xtask -- golden-check` passes all 190 snapshots unchanged.

## Next Steps

1. P5.2: lower actual pure signal closures into `VectorRouteSession` regions,
   then run CSE independently per routed region.
2. Add the bounded event-order/FissionSafe checker and complete routed effect
   and epoch-order evidence.
3. P6: implement delay storage, recursion transitions, clock epochs, and AD
   execution semantics before stateful route activation.
4. Connect the verified routed module to `build_module`, compiler options, and
   backends only after those gates pass.
5. Complete canonical JSON/hash, Lean R3/R4 checking, and the stable C++
   occurrence/vectorization-retention corpus.

## Useful Commands

- `cargo test -p transform signal_fir::vector_route --lib`
- `cargo test -p transform signal_fir::vector_plan --lib`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`
