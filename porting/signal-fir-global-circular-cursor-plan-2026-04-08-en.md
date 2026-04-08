# Plan: Introduce a Shared Global Circular Cursor Abstraction in `signal_fir`

**Date**: 2026-04-08
**Scope**: `crates/transform/src/signal_fir/module.rs`, `delay.rs`
**Status**: Design
**Goal**: replace ad hoc `fIOTA` ownership with an explicit shared abstraction
for the global circular sample cursor used by both delay lines and upsized
recursion carriers.

---

## 1. Problem

The recent `signal_fir` refactors made the `delay.rs` / `module.rs` boundary
much cleaner:

- delay strategy analysis and emission are mostly owned by `delay.rs`
- recursion carrier resolution is now more explicit in `module.rs`
- sample-loop emission phases are named explicitly

However, one important runtime concept still leaks awkwardly across that
boundary: the global circular cursor currently materialized as `fIOTA`.

Today, `fIOTA` is:

- declared/reset through `ensure_iota_state` in `module.rs`
- used by `delay.rs` for `CircularPow2` delay lines
- used by `module.rs` for recursion carriers with
  `RecursionStorageStrategy::Circular`
- advanced once per sample through delay-owned sample-end logic

So `fIOTA` is no longer purely a delay implementation detail, but it is also
not yet modeled as an explicit shared runtime service.

This leaves several design issues:

1. ownership is split across files without a named abstraction
2. `module.rs` still exposes low-level cursor setup details
3. delay and recursion both depend on the same runtime state, but through
   different local helpers
4. future changes to circular indexing rules would have to touch multiple
   code paths directly

---

## 2. Design Goal

Make the global circular cursor a first-class `signal_fir` abstraction.

The target is not to change semantics or generated code shape.

The target is to introduce an explicit shared API so that:

- delay and recursion use the same runtime cursor contract
- `module.rs` no longer manually owns `fIOTA` declaration details
- the sample-end increment remains centralized
- cursor-dependent indexing logic becomes easier to reason about and test

---

## 3. Current Semantic Role of `fIOTA`

`fIOTA` currently acts as the global sample cursor for all structures that use
masked circular indexing.

### 3.1 Delay side

For `DelayStrategy::CircularPow2`:

- write index: `fIOTA & (size - 1)`
- read index: `(fIOTA - amount) & (size - 1)`
- end-of-sample update: `fIOTA = fIOTA + 1`

### 3.2 Recursion side

For `RecursionStorageStrategy::Circular`:

- current sample writes use the masked current cursor position
- delayed feedback reads use masked offsets from the same cursor
- simple 2-slot recursion does **not** use `fIOTA`

So the real concept is not “delay counter”, but rather:

- one shared **global circular sample cursor**
- consumed by all circular storage strategies

---

## 4. Proposed Abstraction

Introduce an explicit runtime abstraction around the global circular cursor.

Suggested name:

```rust
struct GlobalCircularCursor {
    declared: bool,
}
```

or, if a data-less helper API is sufficient:

```rust
struct GlobalCircularCursor;
```

The exact representation matters less than the API boundary.

### 4.1 Required responsibilities

The abstraction should own:

- declaration of the persistent struct field (`fIOTA`)
- `instanceClear` reset to zero
- loading the current cursor value
- building masked current/read indices from that cursor
- end-of-sample increment emission

### 4.2 Non-responsibilities

It should **not** own:

- choice of delay strategy
- recursion carrier planning
- phase orchestration of the full sample loop
- per-line `IfWrapping` counters

Those remain owned by the existing delay / recursion / module layers.

---

## 5. Proposed API Shape

One plausible direction:

```rust
struct GlobalCircularCursor;

impl GlobalCircularCursor {
    fn ensure_state(
        &self,
        ctx: &mut DelayManager,
    ) -> Result<(), SignalFirError>;

    fn load(
        &self,
        store: &mut FirStore,
    ) -> FirId;

    fn current_index(
        &self,
        store: &mut FirStore,
        size: usize,
    ) -> FirId;

    fn delayed_index(
        &self,
        store: &mut FirStore,
        amount: FirId,
        size: usize,
    ) -> FirId;

    fn emit_advance(
        &self,
        store: &mut FirStore,
    ) -> FirId;
}
```

This exact signature is not mandatory; the important part is that delay and
recursion callers stop talking directly in terms of raw `fIOTA` ownership.

---

## 6. Ownership Boundary

### 6.1 `delay.rs` should own

- the concrete `fIOTA` declaration/reset/advance implementation
- cursor helper functions and masked-index construction
- the fact that `CircularPow2` delay lines rely on the shared cursor

This matches the existing role of `DelayManager::emit_sample_end_updates(...)`
and the earlier migration of delay-specific helpers into `delay.rs`.

### 6.2 `module.rs` should keep

- recursion orchestration
- the decision that a recursion carrier uses
  `RecursionStorageStrategy::Circular`
- calls into the shared cursor API when lowering circular recursion carriers

So `module.rs` should depend on the cursor abstraction, but should not own the
low-level `fIOTA` state protocol itself.

---

## 7. Integration Options

There are two reasonable designs.

### 7.1 Option A: cursor owned by `DelayManager`

Treat the global circular cursor as a delay-owned runtime service that is also
exported to recursion users.

Example direction:

```rust
impl DelayManager {
    fn ensure_global_circular_cursor(&mut self) -> Result<(), SignalFirError>;
    fn global_circular_cursor(&self) -> GlobalCircularCursor;
}
```

Pros:

- minimal structural change
- aligns with current delay ownership of sample-end cursor advance
- likely the least invasive refactor

Cons:

- recursion still depends on a delay-owned subsystem for a now-shared concept

### 7.2 Option B: cursor factored as a neutral helper module/type

Move the cursor abstraction into a neutral `signal_fir` helper layer consumed by
both `delay.rs` and `module.rs`.

Pros:

- most semantically honest design
- makes shared ownership explicit

Cons:

- larger refactor
- risks over-abstracting before there is more shared runtime state

### 7.3 Recommended path

Start with **Option A**, because it preserves momentum and cleans up the API
without forcing a wider module split.

If more shared runtime services appear later, promote the cursor into a neutral
helper abstraction then.

---

## 8. Refactor Sequence

### Step 1: introduce the explicit cursor API

- add a `GlobalCircularCursor` abstraction in `delay.rs`
- move existing raw helpers behind it:
  - `ensure_iota_state` equivalent
  - `current_iota_index`
  - `delayed_iota_index`
  - `bump_iota`
- keep generated FIR unchanged

Pass criteria:

- no behavior change
- `module.rs` no longer emits raw `fIOTA` declaration logic directly

### Step 2: route delay code through the new API

- migrate `CircularPow2Model` / ring emitters to use the cursor abstraction
- keep `IfWrapping` untouched

Pass criteria:

- no direct raw `fIOTA` manipulation remains in circular delay emission paths

### Step 3: route circular recursion through the same API

- migrate circular recursion carrier reads/writes in `module.rs`
- keep two-slot recursion on its dedicated non-circular path

Pass criteria:

- circular recursion no longer builds masked indices via ad hoc local helpers
- cursor reads/writes for delay and recursion share one contract

### Step 4: centralize sample-end cursor maintenance naming

- rename/update any remaining delay-specific wording so that sample-end
  `fIOTA` advance is documented as global cursor maintenance, not only delay
  maintenance

Pass criteria:

- docs and comments consistently describe the cursor as shared state

---

## 9. Validation Strategy

Structural tests should verify that the refactor preserves the existing
contracts.

### 9.1 Delay coverage

- fixed delay below `max_copy_delay` still does **not** allocate `fIOTA`
- circular-pow2 delay still allocates/resets/advances `fIOTA`
- if-wrapping delay still does **not** allocate `fIOTA`

### 9.2 Recursion coverage

- simple feedback recursion still does **not** allocate `fIOTA`
- upsized circular recursion carrier still does allocate/use `fIOTA`

### 9.3 Integration coverage

- mixed recursion + delay cases still emit only one shared `fIOTA`
- sample-end update order remains unchanged

---

## 10. Expected Outcome

After this refactor:

- `fIOTA` is no longer an unnamed cross-cutting implementation detail
- `delay.rs` owns the low-level cursor protocol
- `module.rs` depends on an explicit shared cursor service instead of direct
  state plumbing
- the delay/recursion boundary becomes easier to explain and safer to evolve

This should be treated as an API-cleanup and ownership-clarification step, not
as a semantic parity change.
