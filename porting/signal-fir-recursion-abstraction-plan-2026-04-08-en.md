# Plan: Introduce Explicit Recursion Abstractions in `signal_fir`

**Date**: 2026-04-08
**Scope**: `crates/transform/src/signal_fir/module.rs`
**Status**: Partially implemented
**Goal**: make recursion lowering easier to reason about by introducing
explicit abstractions for:

- recursion carrier resolution
- recursion storage strategy
- recursion reads/writes across sample phases

without changing the current semantic parity with the Faust C++ compiler.

## Implementation Status Snapshot

Implemented on 2026-04-08:

- `RecursionStorageStrategy`
- `RecursionCarrierRef`
- `RecursionDelayRef`
- `resolve_recursion_carrier(...)`
- `resolve_active_recursion_carrier(...)`
- `resolve_recursion_delay_ref(...)`
- migration of key call sites away from ad hoc `size == 2` checks

Still open:

- factor recursion read/write/finalize helpers into explicit local APIs
- decide whether that factoring should remain in `module.rs` or move into a
  dedicated recursion-focused module later

---

## 1. Problem

The recent delay work made the `delay.rs` / `module.rs` boundary much cleaner,
but the recursion side of `signal_fir` is still organized mostly as a set of
correct local mechanisms rather than a clearly named design.

The pre-refactor logic was spread across:

- `recursion_delay_chain_info`
- `recursion_carrier_info`
- `active_recursion_info`
- `lower_proj`
- `ensure_recursion_array_for_group`
- the size-2 special case in `lower_delay_state`

This is workable, but it leaves several important concepts implicit:

1. **which carrier** a recursive projection resolves to
2. **which storage strategy** that carrier uses
3. **how current/previous/delayed reads are expressed**
4. **which phase** the recursion finalization write belongs to

This is the recursion-side equivalent of the older delay situation before
`DelayStrategyEmitter` and explicit sample phases.

---

## 2. Design Goal

Keep `module.rs` as the orchestration layer, but make recursion handling more
explicit and locally composable.

The target is not to move recursion into a new file immediately.

The target is to make the current recursion model visible through stable
abstractions so that:

- code paths stop recomputing the same logic in multiple places,
- delay/recursion interaction is easier to explain,
- future parity work on APF/biquad-like structures has a cleaner home.

---

## 3. Current Implicit Concepts

### 3.1 Carrier resolution

Today, a `Proj(i, group)` may resolve to:

- an active carrier on `recursion_stack` (`SYMREF` path)
- a materialized top-level recursion group carrier (`SYMREC` path)
- no carrier at all (not a recursion projection)

This was previously encoded procedurally across:

- `active_recursion_info`
- `recursion_carrier_info`
- `lower_proj`

### 3.2 Carrier storage strategy

Today there are really two recursion storage strategies:

- **two-slot shift carrier**
  - current sample in slot `0`
  - previous sample in slot `1`
  - post-output copy `slot1 = slot0`
- **circular carrier**
  - size > 2
  - indexed by `fIOTA`
  - used when recursion-delay analysis upsizes the carrier

That distinction is semantically important, and is now modeled explicitly by
`RecursionStorageStrategy`.

### 3.3 Delay-chain reuse over recursion carriers

`Delay1^k(Proj(...))` chains are now supported through accumulated delay
analysis and carrier reuse.

Before the refactor, this abstraction was still procedural:

- `recursion_delay_chain_info` stripped nested `Delay1`
- `lower_delay_state` special-cased reads
- `lower_fixed_delay` special-cased reads

This is now represented explicitly by `RecursionDelayRef`.

---

## 4. Proposed Abstractions

### 4.1 `RecursionCarrierRef`

Implemented:

```rust
struct RecursionCarrierRef {
    info: RecArrayInfo,
    strategy: RecursionStorageStrategy,
}
```

This object represents:

- the storage name/type/size
- whether the carrier is two-slot or circular

It is returned by the canonical carrier lookup path so callers no longer need
to inspect `size == 2` themselves.

### 4.2 `RecursionStorageStrategy`

Implemented:

```rust
enum RecursionStorageStrategy {
    TwoSlotShift,
    Circular,
}
```

Mapping rule:

- `size == 2` -> `TwoSlotShift`
- `size > 2` -> `Circular`

This keeps the current implementation model but makes it explicit.

### 4.3 `RecursionDelayRef`

Implemented:

```rust
struct RecursionDelayRef {
    carrier: RecursionCarrierRef,
    implicit_delay: usize,
}
```

This is the recursion analogue of the older tuple-based accumulated delay-chain
information that used to come from `recursion_delay_chain_info`.

It should represent:

- “which recursion carrier am I reading from?”
- “how many implicit `Delay1` steps were wrapped around it?”

`lower_delay_state` and `lower_fixed_delay` now consume one object instead of
recomputing carrier + offset logic separately.

---

## 5. Proposed API Shape

### 5.1 Carrier resolution API

Implemented:

```rust
fn resolve_recursion_carrier(
    &mut self,
    proj_node: SigId,
    proj_index: i32,
    group: SigId,
) -> Result<Option<RecursionCarrierRef>, SignalFirError>;
```

This subsumes the earlier procedural split between:

- `active_recursion_info`
- `recursion_carrier_info`

The old helpers have now been replaced at call sites by the canonical resolver.

### 5.2 Delay-chain resolution API

Implemented:

```rust
fn resolve_recursion_delay_ref(
    &mut self,
    value: SigId,
) -> Result<Option<RecursionDelayRef>, SignalFirError>;
```

This replaces `recursion_delay_chain_info`.

### 5.3 Strategy-local read/write helpers

Add explicit helpers for recursion carriers:

```rust
fn emit_recursion_current_read(...);
fn emit_recursion_previous_read(...);
fn emit_recursion_delayed_read(...);
fn emit_recursion_current_write(...);
fn emit_recursion_post_output_finalize(...);
```

These do not need to be traits immediately. Simple helper functions or methods
on `RecursionCarrierRef` are enough as a first step.

---

## 6. Division of Responsibility

### 6.1 `module.rs` should keep

- ownership of recursion orchestration
- group decoding and body-lowering order
- interaction with `lower_signal(...)`
- interaction with explicit sample phases

### 6.2 New recursion abstractions should own

- carrier resolution
- carrier strategy classification
- read/write/finalize policy for each strategy
- delay-chain offset interpretation over recursion carriers

---

## 7. Refactor Sequence

### Step 1: make recursion strategy explicit

Status: implemented

- add `RecursionStorageStrategy`
- add `RecursionCarrierRef`
- replace direct `size == 2` tests at key call sites with explicit strategy
  matching

Pass criterion:

- behavior unchanged
- code becomes easier to scan

### Step 2: unify carrier resolution

Status: implemented

- introduce `resolve_recursion_carrier`
- keep old helpers temporarily as thin adapters if needed
- ensure both active `SYMREF` and top-level `SYMREC` paths return the same
  abstraction

Pass criterion:

- `lower_proj`, `lower_delay_state`, and delay-chain handling use one carrier
  lookup surface

### Step 3: unify delay-chain resolution over recursion

Status: implemented

- introduce `RecursionDelayRef`
- replace `recursion_delay_chain_info`
- update `lower_delay_state` and `lower_fixed_delay`

Pass criterion:

- one explicit object carries both carrier identity and implicit delay

### Step 4: factor recursion read/write/finalize helpers

Status: open

- introduce helpers per strategy:
  - current read
  - previous read
  - delayed read
  - current write
  - post-output finalize

Pass criterion:

- `lower_proj` no longer spells out raw slot math directly in multiple places

### Step 5: align recursion with sample phases

Status: partially implemented

- ensure two-slot recursion finalization is clearly classified as `PostOutput`
- ensure circular recursion reads/writes are clearly classified as `Immediate`
- document why no recursion-specific `SampleEnd` phase is currently needed

Pass criterion:

- recursion ordering is described in the same vocabulary as delay ordering

---

## 8. Risks

### 8.1 Parity risk

Recursion is parity-sensitive. Small structural refactors can change:

- whether the current or previous slot is read
- whether a write occurs before or after output observation
- whether a delayed recursion chain shares the intended carrier

So each step must be validated with structural tests and at least one backend
integration test.

### 8.2 Over-generalization risk

It would be easy to over-abstract recursion into traits too early.

That is not the goal.

The goal is first to expose the current two real strategies explicitly:

- `TwoSlotShift`
- `Circular`

Only after that should we evaluate whether a trait or a dedicated
`recursion.rs` module is justified.

---

## 9. Validation Matrix

At minimum, validate:

- `recursive_feedback_delay1_reuses_two_slot_recursion_array`
- `nested_feedback_delay1_chain_reuses_one_recursion_carrier`
- `fixed_delay_over_feedback_chain_reuses_one_recursion_carrier`
- `top_level_recursion_projection_delay_chain_reuses_one_recursion_carrier`
- `rec_proj_lowers_without_placeholder_nodes`
- `recursive_feedback_stays_in_sample_loop`

And selected compiler integration tests that exercise:

- delay + recursion interaction
- APF / phasor / feedback fixtures

Also run:

- `cargo fmt --all`
- `cargo clippy -p transform --lib -- -D warnings`
- `cargo test -p transform --lib signal_fir`

---

## 10. Success Criteria

This plan is complete when:

- recursion carrier resolution has one canonical API
- recursion storage strategy is explicit instead of inferred ad hoc from
  `size == 2`
- recursion-delay reuse is represented as an explicit resolved object
- recursion read/write/finalize rules are easier to explain than the current
  procedural spread across multiple functions
