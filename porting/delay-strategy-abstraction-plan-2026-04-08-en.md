# Plan: Normalize Delay Strategy Abstractions in `signal_fir`

**Date**: 2026-04-08
**Scope**: `crates/transform/src/signal_fir/delay.rs`, `module.rs`
**Status**: Design — not yet implemented
**Goal**: split the current delay abstraction into:

- a **ring-buffer model layer** for pointer/index driven delay lines
- a **full lowering-strategy layer** for complete delay emission behavior

This avoids forcing the `Shift` strategy into an index-based trait that does
not match its semantics.

---

## 1. Problem

The current `DelayLineModel` / `CircularPow2Model` abstraction is a good fit for
power-of-two circular buffers, but it does not generalize cleanly to all
existing delay strategies.

Current strategies in the fast lane:

- `Shift`
- `CircularPow2`
- `IfWrapping`

These strategies do not all have the same semantic shape.

### 1.1 Why `CircularPow2` fits the current trait

`CircularPow2` is naturally described by:

- one buffer size rule
- one write index
- one read index
- one pointer advance

So the current trait shape makes sense there.

### 1.2 Why `IfWrapping` also fits that family

`IfWrapping` differs from `CircularPow2` mainly by:

- exact-size buffer instead of power-of-two size
- per-line counter instead of global `fIOTA`
- `if`-based wrap logic instead of bitmask wrap

But conceptually it is still:

- one pointer-based write position
- one pointer-based delayed read
- one pointer advance

So it belongs in the same family as `CircularPow2`.

### 1.3 Why `Shift` does not fit that family

`Shift` is not naturally a pointer/index model.

Its semantics are:

- immediate write at `buf[0]`
- direct read at `buf[amount]`
- deferred copy updates:
  - unrolled for small delays
  - loop-based for larger delays

It does not fundamentally have:

- a write pointer
- a read index derived from that pointer
- a pointer-advance step

Forcing `Shift` into an index-based trait would make the abstraction less
truthful and less stable.

---

## 2. Proposed Split

Introduce **two abstraction levels**.

### 2.1 Low-level: ring-buffer models

Keep a trait only for strategies that are genuinely pointer/index driven.

Suggested shape:

```rust
trait RingDelayModel {
    fn buffer_size(&self, max_delay: i32) -> Result<usize, SignalFirError>;

    fn write_index(
        &self,
        store: &mut FirStore,
        state: &DelayRuntimeState,
        size: usize,
    ) -> FirId;

    fn read_index(
        &self,
        store: &mut FirStore,
        state: &DelayRuntimeState,
        amount: FirId,
        size: usize,
    ) -> FirId;

    fn emit_advance(
        &self,
        store: &mut FirStore,
        state: &DelayRuntimeState,
        size: usize,
    ) -> FirId;
}
```

Supporting runtime-state descriptor:

```rust
enum DelayRuntimeState {
    GlobalIota,
    Counter(String),
}
```

Implementations:

- `CircularPow2Model`
- `IfWrappingModel`

This layer should own:

- buffer-size rules
- pointer/read/write index computation
- state advance logic

It should **not** own higher-level lowering orchestration.

### 2.2 High-level: full delay-lowering strategies

Add a separate abstraction for what `module.rs` actually needs when lowering
`Delay` and `Delay1`.

Suggested shape:

```rust
trait DelayStrategyEmitter {
    fn emit_fixed_delay(
        &self,
        ctx: &mut DelayLoweringCtx<'_>,
        line: &DelayLineInfo,
        current: FirId,
        amount: FirId,
        read_ty: FirType,
        carried: SigId,
    ) -> FirId;

    fn emit_delay1(
        &self,
        ctx: &mut DelayLoweringCtx<'_>,
        line: &DelayLineInfo,
        current: FirId,
        read_ty: FirType,
        carried: SigId,
    ) -> FirId;
}
```

Suggested lowering context:

```rust
struct DelayLoweringCtx<'a> {
    store: &'a mut FirStore,
    sample_statements: &'a mut Vec<FirId>,
    deferred_shift_writes: &'a mut Vec<FirId>,
    next_loop_var_id: &'a mut usize,
    uses_iota: &'a mut bool,
    scheduled_delay_writes: &'a mut HashSet<SigId>,
}
```

Implementations:

- `ShiftEmitter`
- `RingEmitter<CircularPow2Model>`
- `RingEmitter<IfWrappingModel>`

This layer should own:

- strategy-specific write scheduling
- immediate write emission
- delayed read emission
- deferred shift-copy emission
- per-sample advance emission when applicable

---

## 3. Division of Responsibility

### 3.1 `delay.rs` should own

- delay analysis
- delay planning
- ring-buffer geometry models
- strategy-specific FIR emission primitives
- `ShiftEmitter`
- `RingEmitter`
- `IfWrappingModel`

### 3.2 `module.rs` should keep

- `lower_fixed_delay`
- `lower_delay_state`
- `lower_shift_delay1`
- recursion carrier lookup / resolution
- `lower_signal(...)` integration
- module assembly orchestration

The high-level lowering entry points must stay in `module.rs` because they are
coupled to:

- recursion ownership
- `lower_signal(...)`
- `sample_statements` / `compute_updates`
- general fast-lane scheduling

So the goal is **cleaner separation**, not complete extraction.

---

## 4. Expected Benefits

### 4.1 More truthful abstraction

`Shift` stops pretending to be a pointer/index model.

### 4.2 Better reuse between ring strategies

`CircularPow2` and `IfWrapping` can share one consistent model interface.

### 4.3 Thinner `module.rs`

The strategy-specific `match DelayStrategy` bodies in `lower_fixed_delay` and
`lower_shift_delay1` can become smaller and more declarative.

### 4.4 Easier future extension

If a new pointer-based delay model is added later, it should fit naturally as a
new `RingDelayModel` implementation without affecting `Shift`.

---

## 5. Non-Goals

This plan does **not** aim to:

- move recursion carrier ownership logic out of `module.rs`
- eliminate `DelayStrategy` as the top-level planning enum
- force one universal trait over all delay strategies

The design explicitly rejects a single trait of the form:

- `buffer_size`
- `write_index`
- `read_index`
- `advance`

for all strategies, because that abstraction is not a good fit for `Shift`.

---

## 6. Suggested Refactor Sequence

### Step 1. Introduce `IfWrappingModel`

Add an explicit model type alongside `CircularPow2Model`.

Pass criteria:

- no behavior change
- the current `IfWrapping` helper logic is expressed through one model type

### Step 2. Rename/generalize the ring trait

Replace or refactor the current `DelayLineModel` into `RingDelayModel`.

Pass criteria:

- the trait clearly describes only pointer/index driven models
- `CircularPow2Model` and `IfWrappingModel` both implement it

### Step 3. Introduce `DelayLoweringCtx`

Add a smaller context dedicated to strategy-specific FIR emission.

Pass criteria:

- delay emission helpers stop depending directly on `SignalToFirLower`
- shared mutable state required by strategy emitters is explicit

### Step 4. Add `ShiftEmitter`

Move shift-specific write/read/deferred-copy logic behind one emitter type.

Pass criteria:

- no change in generated FIR/C++ shape
- `Shift` no longer depends on fake index-model methods

### Step 5. Add `RingEmitter<M: RingDelayModel>`

Implement strategy emission for pointer/index-driven models.

Pass criteria:

- `CircularPow2` and `IfWrapping` share more strategy emission code
- differences are localized to the ring model implementation

### Step 6. Simplify `module.rs`

Rewrite `lower_fixed_delay` and `lower_shift_delay1` to delegate strategy
details to the emitters while keeping orchestration in place.

Pass criteria:

- `module.rs` still owns recursion integration and `lower_signal(...)`
- `delay.rs` owns the strategy-specific FIR details

---

## 7. Validation

Validation should preserve the current delay-strategy and recursion-carrier
behavior on the existing structural tests:

- `fixed_delay_two_uses_unrolled_shift_copies`
- `fixed_delay_three_uses_shift_loop`
- `fixed_delay_at_mcd_boundary_uses_circular_pow2`
- `fixed_delay_at_dlt_boundary_uses_if_wrapping`
- `nested_feedback_delay1_chain_reuses_one_recursion_carrier`
- `fixed_delay_over_feedback_chain_reuses_one_recursion_carrier`
- `top_level_recursion_projection_delay_chain_reuses_one_recursion_carrier`

Also rerun:

- `cargo clippy -p transform --lib -- -D warnings`
- targeted compiler-level structural tests when strategy code shape is touched

---

## 8. Success Criteria

This design is considered successful when:

- `delay.rs` owns the strategy-specific FIR emission details
- `module.rs` keeps only orchestration and recursion integration
- `Shift` is no longer forced into an index-based abstraction
- `CircularPow2` and `IfWrapping` are normalized under one ring-model layer
- existing structural behavior remains unchanged
