# Session Handoff

Date: 2026-07-15

## Repo State

- Branch: `ondemand-vec-fad-synthesis`
- HEAD: the commit containing this handoff.

Recent commits (most recent first):

- `this commit` Certify fused recursive delay vector loops
- `37ab6a58` Document certified recursive vector fusion
- `b5a0a8b3` Guard recursive delay vector transports

## Working Tree

- Tracked changes implement certified fused serial recursive-delay reads across
  the vector-plan DTO/checker, producer, routing, lowering/assembly verification,
  schema, compiler tests, journal, and this handoff.
- New task-owned files:
  `tests/corpus/vector_recursive_delay_fusion_pulse_countup_loop.dsp` and its
  Rust golden directory.
- Many unrelated pre-existing untracked scratch files remain untouched. In
  particular, the untracked `rad_lti_recursive_multi_output1` corpus/golden
  files are not part of this task.

## Current Goal

- Execute `porting/vector-fused-recursive-delay-plan-2026-07-15-en.md` for the
  direct top-rate `pulse_countup_loop` pattern while retaining fail-closed
  fallback for unsupported shapes.

## What Changed This Session

- Added `FusedSerialGroupRecord` and `fused_serial_groups` to `VectorPlan`, plus
  JSON shape constraints and an independent decoration-backed L2 checker.
- Added mutation rejection for empty/unknown/duplicate membership,
  non-recursive carriers, missing delayed edges, uncovered dangerous
  transports, and incompatible clocks.
- The producer now certifies minimal direct top-rate groups and leaves longer
  chains, ambiguous carriers, overlap, and clocked groups unsupported.
- Internal transports retain stable plan identities but use
  `ClockTransportMode::FusedScalar`, lowering to stack `StoreVar`/`LoadVar`
  rather than `OuterChunk` arrays.
- Assembly preserves logical loop identities while emitting all group members,
  in scheduled order, inside one physical serial `for i0` loop. The FIR checker
  validates copy-in/read/write/copy-out placement and internal transport shape.
- The conservative guard accepts only fully covered certified groups; all other
  dangerous delayed-recursive transports still fall back to scalar.
- Added explicit `Certified` and bit-exact coverage for count-up/count-down,
  `lv0/lv1`, `ss0..ss3`, and a non-divisible tail chunk.

## Decisions / Constraints

- Confirmed with the user: member `LoopRecord`s remain in the epoch and routed
  layout; fusion is a quotient of physical emission owned by
  `owner_loop_id`.
- The first producer slice is top-rate and direct only. It intentionally does
  not fuse clock islands, longer pure chains, overlapping groups, or multiple
  carriers owned by one loop.
- JSON Schema checks finite shape only. Cross-artifact joins involving
  `max_delay`, recursion facts, and `DepKind::Delayed` remain Rust L2
  obligations.
- This is an adapted internal Rust API extension with no C/C++ ABI change.

## Validation Run

- `cargo fmt --all -- --check` -> pass.
- `cargo clippy --workspace --all-targets -- -D warnings` -> pass.
- `cargo test --workspace --all-targets` -> pass.
- `cargo run -p xtask -- golden-check` -> pass, including the new corpus.
- Generated C++ for `-vec -lv 1 -ss 3` contains scalar
  `transport_s23_l2_l1` and one loop with delayed read, recurrence, and state
  write before the separate pure tail loop.
- faustlibraries `check-rs-cpp` and `check-rse-cpp`, each over scalar
  `ss0..ss3` and vector `lv0/lv1 x ss0..ss3`, pass with no differences within
  `0.0001`.

## Open Issues / Blockers

- None for the scoped direct top-rate pattern.
- Generalized fusion across longer pure chains and clock islands remains a
  later extension and currently falls back by design.

## Next Steps

1. Review the working-tree diff and create a coherent commit if desired.
2. Extend the producer/checker only when a new characterized unsupported shape
   justifies longer-chain or clock-aware fusion.
3. Keep the faustlibraries pulse tests in the parity matrix as a regression
   gate.

## Useful Commands to Resume

- `cargo test -p transform signal_fir::vector_verify::tests --lib`
- `cargo test -p transform signal_fir::vector_assemble::tests --lib`
- `cargo test -p compiler --test vector_mode`
- `cargo run -p xtask -- golden-check`
- `target/debug/faust-rs --dump-cpp --vec --lv 1 --scheduling-strategy 3 tests/corpus/vector_recursive_delay_fusion_pulse_countup_loop.dsp`
