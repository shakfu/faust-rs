# Plan: Introduce Explicit Emission Phases in `signal_fir`

**Date**: 2026-04-08
**Scope**: `crates/transform/src/signal_fir/module.rs`, `delay.rs`
**Status**: Design
**Goal**: make sample-loop ordering explicit by introducing named emission
phases instead of exposing raw staging details such as
`deferred_shift_writes` in `module.rs`.

---

## 1. Problem

The current `signal_fir` split between `module.rs` and `delay.rs` is much
cleaner than before, but one important concept still leaks across the boundary:
**when** emitted statements must run inside the sample loop.

Today that timing is represented indirectly through several storage buckets in
`SignalToFirLower`:

- `sample_statements`
- `deferred_shift_writes`
- `compute_updates`
- delay-specific sample-end updates emitted separately

This works, but it has two drawbacks:

1. the boundary is expressed in terms of implementation buckets rather than
   semantic phases
2. delay-specific ordering constraints are still partly visible from
   `module.rs`

The clearest example is the `Shift` strategy:

- write `buf[0]` immediately
- read delayed values before output stores are finalized
- perform the shift-copy only after outputs are stored

That is not just “some deferred writes”; it is a specific **post-output**
phase.

---

## 2. Design Goal

Replace ad hoc statement buckets with an explicit phase model for sample-loop
emission.

The target is not to move all orchestration out of `module.rs`.

The target is to let `module.rs` assemble the sample loop using a small,
explicit sequence of phases, while subsystems such as `delay.rs` contribute
statements to those phases without exposing low-level timing details.

---

## 3. Proposed Phase Model

Introduce three explicit sample phases:

### 3.1 Immediate phase

Statements that must occur during ordinary sample evaluation, before output
stores are finalized.

Examples:

- immediate `Shift` write `buf[0] = current`
- circular / if-wrapping delay writes
- ordinary per-sample state writes
- inline arithmetic and value materialization

### 3.2 Post-output phase

Statements that must run after outputs are stored, but before the sample is
fully finalized.

Examples:

- `Shift` copy loops / unrolled shift copies
- recursion copy updates that must observe the current-sample output first

This is the phase that `deferred_shift_writes` is approximating today.

### 3.3 Sample-end phase

Statements that advance runtime counters or perform generic subsystem-finalize
maintenance after the sample’s observable reads/writes are complete.

Examples:

- `fIOTA` increment
- per-line `IfWrapping` counter advances

This phase is already conceptually present in
`DelayManager::emit_sample_end_updates`.

---

## 4. Proposed Rust Shape

There are two viable representations. The recommended one is phase buckets.

### 4.1 Recommended: explicit phase buckets

Add a dedicated container:

```rust
struct SamplePhases {
    immediate: Vec<FirId>,
    post_output: Vec<FirId>,
    sample_end: Vec<FirId>,
}
```

`SignalToFirLower` would then replace:

- `sample_statements`
- `deferred_shift_writes`

with:

- `sample_phases: SamplePhases`

`compute_updates` should then be reviewed and either:

- folded into `post_output`, if semantically equivalent
- or kept separate only if it represents a genuinely different phase

Current evidence suggests that `compute_updates` and `post_output` likely
belong to the same semantic stage, but this must be checked carefully against
recursion ordering.

### 4.2 Alternative: phase-tagged append API

Instead of exposing the buckets directly, add append helpers:

```rust
enum EmissionPhase {
    Immediate,
    PostOutput,
    SampleEnd,
}

impl SamplePhases {
    fn push(&mut self, phase: EmissionPhase, stmt: FirId);
    fn extend(&mut self, phase: EmissionPhase, stmts: impl IntoIterator<Item = FirId>);
}
```

This is more explicit at call sites and avoids accidental writes to the wrong
bucket.

If the refactor is done in stages, this API is the safer intermediate form.

---

## 5. Ownership Boundary

### 5.1 `module.rs` should keep

- ownership of the global sample-loop assembly order
- the final flattening order:
  1. immediate
  2. output stores
  3. post-output
  4. sample-end
- recursion orchestration
- all logic that depends on `lower_signal(...)`

### 5.2 `delay.rs` should own

- the fact that `Shift` emits both immediate and post-output work
- the fact that circular / if-wrapping lines emit immediate writes
- the fact that delay strategies may emit sample-end maintenance

This means `delay.rs` should contribute statements **by phase**, not by leaking
implementation details such as “deferred shift writes”.

---

## 6. Proposed API Direction

Extend the delay-lowering interface so strategy emitters append into explicit
phases.

Suggested shape:

```rust
struct DelayLoweringCtx<'a> {
    store: &'a mut FirStore,
    phases: &'a mut SamplePhases,
    next_loop_var_id: &'a mut usize,
}
```

Then `DelayStrategyEmitter` implementations would do:

- `Immediate` for direct writes
- `PostOutput` for `Shift` copy loops

Delay sample-finalize logic should stay behind:

```rust
DelayManager::emit_sample_end_updates(...)
```

or evolve to:

```rust
DelayManager::append_sample_end_updates(...)
```

so that `module.rs` never handles individual delay-maintenance statements.

---

## 7. Refactor Sequence

### Step 1: introduce phase names without changing behavior

- add `EmissionPhase` and `SamplePhases`
- keep current behavior identical
- route existing `sample_statements` writes to `Immediate`
- route `deferred_shift_writes` to `PostOutput`
- route `emit_sample_end_updates` to `SampleEnd`

Pass criterion:

- no FIR output change except possible harmless statement ordering labels or
  temporary naming diffs

### Step 2: migrate delay lowering to phase-aware append helpers

- change `DelayLoweringCtx` to target phases rather than raw vectors
- update `ShiftDelayStrategyEmitter`
- update `RingDelayStrategyEmitter`

Pass criterion:

- no change in golden behavior for delay tests
- no new direct writes to `deferred_shift_writes`

### Step 3: review recursion updates

- inspect `compute_updates`
- determine whether recursion writes belong to `Immediate` or `PostOutput`
- eliminate ambiguous naming if possible

Pass criterion:

- recursion ordering remains parity-correct
- comments explain why each recursion update phase is chosen

### Step 4: collapse obsolete storage details

- remove `deferred_shift_writes`
- rename or remove `compute_updates` if phase-equivalent
- ensure sample-loop assembly reads like a phase scheduler, not a bucket merge

Pass criterion:

- `SignalToFirLower` state becomes simpler, not more fragmented

---

## 8. Risks

### 8.1 Behavioral risk

Any mistake in phase reassignment can break Faust parity for:

- `Shift` delay ordering
- recursion feedback timing
- merged recursion-delay carriers

So each phase move must be validated with structural tests, not just compile
success.

### 8.2 Over-abstraction risk

If too many tiny phase-specific helper methods are introduced, the code may
become less readable than the current explicit vectors.

The abstraction should therefore stay small:

- a small fixed enum of phases
- one central `SamplePhases` container
- subsystem append helpers where useful

---

## 9. Validation Matrix

At minimum, validate:

- `fixed_delay_two_uses_unrolled_shift_copies`
- `fixed_delay_three_uses_shift_loop`
- `fixed_delay_at_mcd_boundary_uses_circular_pow2`
- `fixed_delay_at_dlt_boundary_uses_if_wrapping`
- `recursive_feedback_delay1_reuses_two_slot_recursion_array`
- `fixed_delay_over_feedback_chain_reuses_one_recursion_carrier`
- `top_level_recursion_projection_delay_chain_reuses_one_recursion_carrier`
- relevant compiler integration tests that lock emitted C/C++ ordering

Also run:

- `cargo fmt --all`
- `cargo clippy -p transform --lib -- -D warnings`
- `cargo test -p transform --lib signal_fir`

---

## 10. Success Criteria

This plan is complete when:

- sample-loop ordering is represented explicitly as named phases
- delay lowering contributes statements by phase instead of leaking timing
  details
- `deferred_shift_writes` disappears as a public structural concept in
  `module.rs`
- the resulting boundary between `module.rs` and `delay.rs` is easier to read
  and explain than the current one
