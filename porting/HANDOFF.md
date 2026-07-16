# Session Handoff

Date: 2026-07-16

## Repo State

- Branch: `ondemand-vec-fad-synthesis`
- HEAD: the commit containing this handoff.

Recent commits (most recent first):

- `this commit` Verify lockstep C++ lowering produces SIMD
- `b8a50658` Add complex partial lockstep corpus coverage
- `eb8414ba` Add lockstep vectorization corpus coverage
- `13474bfe` Fuse lockstep lanes into one FIR sample loop
- `f9543530` Detect and schedule lockstep recursion bundles
- `fe1b30fe` Add lockstep vector plan trust boundary
- `effd2104` Freeze lockstep vectorization implementation gate

## Working Tree

- Tracked changes: the native SIMD evidence gate, its documentation, journal,
  and this handoff update belong to the final commit.
- Many unrelated pre-existing untracked scratch files remain untouched. The
  untracked `rad_lti_recursive_multi_output1` corpus/golden files are not part
  of this task.

## Current Goal

- Complete section 8, lockstep instance vectorization, with checked producer,
  physical FIR lowering, corpus evidence, and maintained impulse validation.

## What Changed This Session

- Added vector-plan schema v2 lockstep bundles, lane isomorphism witnesses,
  transport layout, and independent rejecting checks.
- Added automatic exact-instance detection under `-vec`, one scheduler unit,
  stable logical lane/state ownership, and sample-interleaved event ordering.
- Added one physical FIR sample loop per bundle while preserving lane IEEE
  operation order, contraction policy, and planar external I/O.
- Added accepted width-two/width-four corpus cases, a rejected near-isomorphic
  case, Rust goldens, bit-exact integration tests, and optimizer parity.
- Added three profitability-oriented recursive cases, including two where the
  bundle is only one DSP subgraph, plus a Clang optimized-LLVM SIMD gate.

## Decisions / Constraints

- Lockstep is automatic under `-vec`; there is no new CLI flag.
- Logical loop and recursion-group ids remain stable. One bundle is one
  scheduler node and one physical sample loop.
- Unsupported shapes and existing fused-delay slices fail closed. External
  `compute` buffers and chunk transports remain planar.
- The C++ compiler has no corresponding pass; Rustdoc records the adapted
  source provenance and invariants instead of claiming a 1:1 source port.

## Validation Run

- `cargo test -p transform --lib` -> 363 passed.
- `cargo test -p compiler --test vector_mode` -> 25 passed.
- `cargo run -p xtask -- vector-interp-opt-check` -> 40 traces matched.
- `cargo run -p xtask -- golden-check` -> complete Rust golden corpus passed.
- `cargo run -p xtask -- lockstep-simd-check` -> 14/17/14 four-wide LLVM FP
  operations for the three complex cases.
- `make build` in `tests/impulse-tests` -> release harness built.
- `make -j8 interp-vec0 interp-vec1` in `tests/impulse-tests` -> 92/93 expected
  DSPs passed for each variant; documented `subcontainer1` exclusion only.

## Open Issues / Blockers

- The repository-wide test gate currently stops in the unrelated existing
  `wasm_compute_lowers_control_flow_statements` assertion, which expects a
  `Drop` operator. The isolated test reproduces the same failure; lockstep does
  not touch Wasm lowering.
- Wider recognition of recursive loops already owned by a fused-delay slice is
  a conservative future optimization, not a correctness gap.

## Next Steps

1. Resolve the unrelated Wasm `Drop` assertion before claiming the complete
   workspace test gate is green.
2. Consider composing fused-delay slices into lane-level lockstep units only
   with an extended certificate and equivalent mutation tests.

## Useful Commands to Resume

- `cargo test -p compiler --test vector_mode lockstep -- --nocapture`
- `cargo run -p xtask -- vector-interp-opt-check`
- `cargo run -p xtask -- lockstep-simd-check`
- `make -j8 interp-vec0 interp-vec1 -C tests/impulse-tests`
- `cargo run -p xtask -- golden-check`
