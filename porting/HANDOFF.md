# Session Handoff

Date: 2026-07-13

## Repo State

- Branch: `ondemand-vec-fad-synthesis`
- HEAD: the commit containing this handoff
- Current task: P4.3b verified decoration-certificate boundary
- P4.3b is implemented for compute-time facts and included in this commit.

## Working Tree

- Committed changes: P4.3b `signal_fir::decoration_verify`, the vector porting
  plan, today's journal/index, and this handoff.
- Many pre-existing untracked scratch files remain at the repository root and
  were left untouched.

## What Changed

- Added a canonical in-memory `DecorationCertificate` over the real prepared
  `SigId` forest and P4.3a facts.
- Preserved roots, explicit `Gen` lifecycle boundaries, condition expressions,
  exact type/clock/fact records, and both labelled dependency projections.
- Added an exact `CanonicalSigType` boundary including precision and aggregate
  fields deliberately ignored by C++-compatible `SigType` equality.
- Added `verify_decorations`, which reruns analysis and checks authoritative
  prepared types/clocks, exact coverage, ordering, facts, labels, and endpoints.
- Added the opaque `VerifiedDecorationCertificate` accepted-evidence type.
- Added nine focused acceptance/mutation tests, including recursive projections
  and compute-versus-full-lifecycle scope rejection.

## Decisions And Constraints

- `Proj(i, SYMREC)` selects definition `i` immediately;
  `Proj(i, SYMREF)` selects it through an explicit one-sample delayed edge.
- BlockReverseAD and ReverseTimeRec projections depend on their Rust-only tuple
  carrier rather than being misclassified as malformed recursion.
- faust-rs keeps accepting bounded variable-delay intervals with negative
  lower bounds when `hi >= 0`; narrowing that scalar contract is out of P4.2.
- Accepted decorations remain additive and have no production placement,
  scheduling, FIR, or VectorPlan consumer.
- Final loop-level effect edges are deliberately deferred until placement is
  known; only resource conflicts are produced in P4.3a.
- Effects are compute-scoped; `Gen` table-initialization effects require a
  separate lifecycle decoration before certification.
- Compute effects are now accepted only through `VerifiedDecorationCertificate`.
- `FullLifecycle` is rejected; `Gen` initializer effects remain unproved.
- JSON/hash stabilization and executable Lean checking remain R2/RV work.
- The C++ compiler has no stable machine-readable occurrence exporter. P4.2
  tests pin rules directly from source; the signal-by-signal differential gate
  remains open.

## Validation

- `cargo fmt --all -- --check` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace --all-targets` passes, including all 259 transform
  tests, the 16 P4.3a vector-analysis tests, and the nine P4.3b certificate
  tests.
- `cargo run -p xtask -- golden-check` passes all 190 snapshots unchanged.

## Next Steps

1. Make production `VectorPlan` construction require a
   `VerifiedDecorationCertificate`, then consolidate placement/delay consumers.
2. Orient effect edges only after loop placement, preserving semantic order by
   serialization or co-location when commutation is not proved.
3. Add lifecycle decoration for `Gen` initialization before accepting
   `FullLifecycle` scope.
4. Add a stable C++ occurrence oracle or a purpose-built debug exporter before
   claiming the signal-by-signal differential exit criterion.

## Useful Commands

- `cargo test -p transform signal_fir::decoration_verify --lib`
- `cargo test -p transform hgraph --lib`
- `cargo test -p transform signal_fir::loop_graph --lib`
- `cargo test -p compiler --test pv_vector_slice`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`
