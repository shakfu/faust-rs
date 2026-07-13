# Session Handoff

Date: 2026-07-13

## Repo State

- Branch: `ondemand-vec-fad-synthesis`
- HEAD: the commit containing this handoff
- Latest task commit: P4.1/P4.2 unified signal-use analysis
- P4.1 and P4.2 changes are validated and committed together.

## Working Tree

- Committed changes: `signal_fir::vector_analysis`, Hgraph/loop/PV dependency-walk
  adapters, the vector and certified porting plans, today's journal/index, and
  this handoff.
- Many pre-existing untracked scratch files remain at the repository root and
  were left untouched.

## What Changed

- Added one typed signal decoder with separate scheduling and occurrence views,
  shared by Hgraph, LoopGraph/PV, and `SignalUseTable`.
- Ported P4.2 delay, prefix, projection, FIR, IIR, table, wrapper, generator,
  recursive-variability, and `-1 * y` rules from the pinned C++ sources.
- Aggregated `multi` through the four C++ extended-variability buckets and
  retained exact first-visit occurrence expansion.
- Added explicit Rust adaptations for symbolic recursion back-edges, AD tuple
  carriers, wrapper clock boundaries, and the existing permissive variable-
  delay interval contract.
- Made clock inference total for waveform elements after the workspace suite
  exposed a clocked-waveform regression.
- Expanded the focused vector-analysis suite from six to twelve tests.

## Decisions And Constraints

- `Proj(i, SYMREC)` selects definition `i` immediately;
  `Proj(i, SYMREF)` selects it through an explicit one-sample delayed edge.
- BlockReverseAD and ReverseTimeRec projections depend on their Rust-only tuple
  carrier rather than being misclassified as malformed recursion.
- faust-rs keeps accepting bounded variable-delay intervals with negative
  lower bounds when `hi >= 0`; narrowing that scalar contract is out of P4.2.
- Effects and the real execution-condition producer are deferred; no invented
  empty effect certificate is accepted as evidence.
- The current table is additive and has no production placement or VectorPlan
  consumer.
- The C++ compiler has no stable machine-readable occurrence exporter. P4.2
  tests pin rules directly from source; the signal-by-signal differential gate
  remains open.

## Validation

- `cargo fmt --all` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `cargo test --workspace --all-targets` passed.
- `cargo run -p xtask -- golden-check` passed all 190 snapshots.

## Next Steps

1. Add the production execution-condition and conservative effect producers
   (P4.3), including stable resource identities and conflict edges.
2. Export and independently verify `DecorationCertificate` before allowing the
   table to feed production `VectorPlan` construction.
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
