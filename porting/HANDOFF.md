# Session Handoff

Date: 2026-07-13

## Repo State

- Branch: `ondemand-vec-fad-synthesis`
- HEAD: the commit containing this handoff
- Current task: P4.3a execution-condition and effect analysis
- P4.3a is implemented, fully validated, and included in this commit.

## Working Tree

- Committed changes: P4.3a `signal_fir::vector_analysis`, the vector porting
  plan, today's journal/index, and this handoff.
- Many pre-existing untracked scratch files remain at the repository root and
  were left untouched.

## What Changed

- Added deterministic positive-DNF execution conditions matching C++
  `conditionAnnotation`, including `SigControl` refinement and shared-node OR.
- Added a condition-child projection to the shared signal decoder and a typed
  `Control` guard edge that remains same-tick for Hgraph.
- Added sorted transitive effect decoration with stable state, recursion,
  table, UI, output, and raw foreign-signature resource identities.
- Added conservative effect conflict predicates; unknown/impure foreign
  effects are global barriers.
- Added the canonical `analyze_vector_signals` entry point so production
  clients cannot accidentally retain P4.2's constant test provider.
- Expanded the focused vector-analysis suite from twelve to sixteen tests.

## Decisions And Constraints

- `Proj(i, SYMREC)` selects definition `i` immediately;
  `Proj(i, SYMREF)` selects it through an explicit one-sample delayed edge.
- BlockReverseAD and ReverseTimeRec projections depend on their Rust-only tuple
  carrier rather than being misclassified as malformed recursion.
- faust-rs keeps accepting bounded variable-delay intervals with negative
  lower bounds when `hi >= 0`; narrowing that scalar contract is out of P4.2.
- Conditions/effects remain additive and have no production placement,
  scheduling, FIR, or VectorPlan consumer.
- Final loop-level effect edges are deliberately deferred until placement is
  known; only resource conflicts are produced in P4.3a.
- Effects are compute-scoped; `Gen` table-initialization effects require a
  separate lifecycle decoration before certification.
- No effect set is accepted as a certificate yet: independent decoration
  verification remains mandatory.
- The C++ compiler has no stable machine-readable occurrence exporter. P4.2
  tests pin rules directly from source; the signal-by-signal differential gate
  remains open.

## Validation

- `cargo fmt --all -- --check` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace --all-targets` passes, including all 250 transform
  tests and the 16 focused vector-analysis tests.
- `cargo run -p xtask -- golden-check` passes all 190 snapshots unchanged.

## Next Steps

1. Export and independently verify `DecorationCertificate` (P4.3b) before allowing the
   table to feed production `VectorPlan` construction.
2. Orient effect edges only after loop placement, preserving semantic order by
   serialization or co-location when commutation is not proved.
3. Consolidate placement and delay consumers onto accepted decoration facts.
4. Add a stable C++ occurrence oracle or a purpose-built debug exporter before
   claiming the signal-by-signal differential exit criterion.

## Useful Commands

- `cargo test -p transform signal_fir::vector_analysis --lib`
- `cargo test -p transform hgraph --lib`
- `cargo test -p transform signal_fir::loop_graph --lib`
- `cargo test -p compiler --test pv_vector_slice`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`
