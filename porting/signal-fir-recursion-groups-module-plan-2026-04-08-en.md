# Plan: Extract Recursive-Group Management from `signal_fir/module.rs`

**Date**: 2026-04-08
**Scope**: `crates/transform/src/signal_fir/module.rs`, new recursion-focused helper module
**Status**: Partially implemented
**Goal**: decide whether recursive-group handling should move out of
`module.rs`, and if so, define a clean API boundary that improves locality
without breaking parity or over-coupling recursion logic to the rest of the
lowerer.

## Implementation Status Snapshot

Implemented on 2026-04-08:

- dedicated `recursion.rs` module
- owned `RecursionState`
- recursion carrier data types moved out of `module.rs`
- canonical pure lookup helpers moved to `recursion.rs`
- `RecursionAllocCtx` introduced for carrier allocation / clear-loop registration
- active recursion stack push/pop centralized through
  `with_active_recursion_group(...)`

Still open:

- decide whether the remaining recursion-group state fields should move behind
  a dedicated `RecursionState` bundle
- decide whether `lower_proj(...)` should stay in `module.rs` long-term or be
  further split into orchestration + helper subroutines
- decide whether `scheduled_state_updates` recursion-group ownership should be
  made more explicit on the recursion side

---

## 1. Short Answer

Yes, extracting more of the recursive-group management out of `module.rs`
looks pertinent now.

Recent work already made the recursion model more explicit:

- `RecursionStorageStrategy`
- `RecursionCarrierRef`
- `RecursionDelayRef`
- canonical carrier / delay-ref resolution helpers
- explicit sample-loop phases
- shared global circular cursor helpers

That means the code is no longer blocked on missing concepts. The next likely
gain is structural: move the mechanics of recursive-group allocation and
resolution behind a narrower API.

The important constraint is that `module.rs` should still own global lowering
orchestration. So the target is not a full “recursion subsystem takeover”, but
an extraction of the **recursive-group management** layer.

---

## 2. Problem

`module.rs` still mixes several different responsibilities:

- generic `lower_signal(...)` dispatch
- sample-loop assembly and section routing
- delay orchestration
- recursion-delay coupling
- recursive-group carrier allocation and resolution
- active recursion-stack bookkeeping
- recursive body lowering scheduling

The recursion-focused pieces are now coherent enough to identify, but they are
still scattered across:

- `resolve_recursion_carrier(...)`
- `resolve_active_recursion_carrier(...)`
- `resolve_recursion_delay_ref(...)`
- `lower_proj(...)`
- `ensure_recursion_array_for_group(...)`
- `decode_symbolic_group_bodies(...)`
- `recursion_stack`
- `recursion_vars`
- `rec_array_by_group_index`

This creates three practical issues:

1. the recursive-group state model is still embedded in the monolithic lowerer
2. carrier allocation/scheduling logic is harder to inspect in isolation
3. future parity work on recursive compaction still has no focused home

---

## 3. Design Goal

Extract the **recursive-group management layer** into its own module with a
clear API, while keeping `module.rs` as the owner of:

- global `lower_signal(...)` orchestration
- sample-loop phase ordering
- integration with delay lowering
- final FIR statement emission

The extracted module should own:

- recursive-group decoding
- recursive carrier allocation bookkeeping
- active-group stack resolution
- canonical carrier lookup
- recursive-group scheduling state

The extracted module should not directly own:

- lowering of arbitrary non-recursive signals
- delay-line planning
- output-store ordering
- generic sample-loop phase assembly

---

## 4. What Is a “Recursive-Group Management” Subsystem?

This subsystem is narrower than “all recursion lowering”.

It is specifically responsible for:

### 4.1 Group structure discovery

- decode `SYMREC` / `SYMREF` group shape
- determine canonical output indexing for single-output vs multi-output groups

### 4.2 Carrier ownership

- allocate or retrieve the canonical carrier for `(group, body_index)`
- record the chosen storage strategy and carrier metadata
- expose canonical carrier references back to `module.rs`

### 4.3 Active recursion context

- push/pop active group contexts while body signals are lowered
- resolve `SYMREF` projections against the active recursion stack

### 4.4 Scheduling bookkeeping

- ensure each recursive group body-lowering pass is scheduled once
- expose whether a group has already been materialized

### 4.5 Delay-coupled recursion lookup

- resolve `Delay1^k(Proj(...))` into a canonical `RecursionDelayRef`

This is already conceptually separate from generic FIR lowering; it is just not
yet isolated as a module.

---

## 5. Why Extraction Makes Sense Now

Earlier in the port, this extraction would have been premature because the
recursion model itself was still implicit.

That is no longer true.

The current state already has stable concepts that a new module could expose:

- `RecursionStorageStrategy`
- `RecursionCarrierRef`
- `RecursionDelayRef`
- explicit global circular cursor helpers

So the remaining question is not conceptual readiness, but boundary quality.

This extraction is now plausible because:

- the recursive-group state is already identifiable as a coherent slice
- carrier resolution no longer depends on anonymous tuples and `size == 2`
  conventions everywhere
- the delay/recursion boundary is already better named than before

---

## 6. Recommended Module Boundary

Recommended new file:

- `crates/transform/src/signal_fir/recursion.rs`

This file should initially stay crate-private / `pub(super)` only.

It should become the home for:

- recursive-group data types
- recursive-group resolution helpers
- active recursion context management
- canonical carrier storage / lookup logic

`module.rs` would keep:

- `lower_signal(...)`
- `lower_delay(...)`
- sample phase assembly
- the final body-lowering calls that ask the recursion manager what to do

---

## 7. Proposed API Direction

There are two plausible API styles.

### 7.1 Option A: stateful `RecursionGroupManager`

```rust
struct RecursionGroupManager {
    rec_array_by_group_index: HashMap<(u32, usize), RecArrayInfo>,
    recursion_stack: Vec<Vec<RecArrayInfo>>,
    recursion_vars: Vec<SigId>,
    scheduled_groups: HashSet<SigId>,
}
```

with methods such as:

```rust
impl RecursionGroupManager {
    fn resolve_recursion_carrier(...)-> Result<Option<RecursionCarrierRef>, SignalFirError>;
    fn resolve_recursion_delay_ref(...)-> Result<Option<RecursionDelayRef>, SignalFirError>;
    fn ensure_group_carrier(...)-> Result<RecArrayInfo, SignalFirError>;
    fn decode_group_bodies(...)-> Option<(SigId, Vec<SigId>)>;
    fn enter_group(...);
    fn exit_group(...);
}
```

Pros:

- the recursive-group state becomes explicit
- the current `SignalToFirLower` field cluster shrinks
- ownership is easy to explain

Cons:

- methods that allocate FIR declarations still need access to broader lowering
  context
- risks a “manager with too many callbacks” design if not carefully scoped

### 7.2 Option B: stateless helper module + small context structs

Keep the state fields on `SignalToFirLower`, but move recursion logic into a
dedicated module with explicit context structs, similar to the delay refactor.

Example direction:

```rust
struct RecursionState<'a> {
    rec_array_by_group_index: &'a mut HashMap<(u32, usize), RecArrayInfo>,
    recursion_stack: &'a mut Vec<Vec<RecArrayInfo>>,
    recursion_vars: &'a mut Vec<SigId>,
    scheduled_state_updates: &'a mut HashSet<SigId>,
}
```

Pros:

- lower migration risk
- mirrors the successful `DelayFirCtx` / `DelayLoweringCtx` pattern
- keeps `SignalToFirLower` as the ultimate owner

Cons:

- less architectural punch than a dedicated owned manager
- recursive-group state is still physically stored on the lowerer

### 7.3 Recommended path

Start with **Option B**.

It is the safer extraction path:

- it separates code before it separates ownership
- it mirrors the delay refactor that already worked well
- it avoids committing too early to a heavyweight recursion manager design

If the resulting boundary still looks stable after a first extraction pass,
Option A can be revisited later.

---

## 8. Proposed API Slice for a First Extraction

The first extracted recursion module should provide APIs for:

### 8.1 Group decoding

```rust
fn decode_symbolic_group_bodies(...)
```

### 8.2 Carrier lookup

```rust
fn resolve_active_recursion_carrier(...)
fn resolve_recursion_carrier(...)
fn resolve_recursion_delay_ref(...)
```

### 8.3 Carrier allocation helpers

```rust
fn ensure_recursion_array_for_group(...)
fn canonical_group_index(...)
```

### 8.4 Active-group scoped execution helper

Instead of raw push/pop in `module.rs`, prefer an API like:

```rust
fn with_active_group<R>(
    state: &mut RecursionState<'_>,
    var: SigId,
    arrays: Vec<RecArrayInfo>,
    f: impl FnOnce(&mut RecursionState<'_>) -> Result<R, SignalFirError>,
) -> Result<R, SignalFirError>;
```

or an equivalent explicit guard type.

This is attractive because the push/pop discipline is important and currently
easy to get subtly wrong.

---

## 9. What Should Stay in `module.rs`

Even after extraction, these should remain in `module.rs`:

- the `SigMatch::Proj(...) => self.lower_proj(...)` dispatch point
- global sample-phase emission
- actual lowering of recursive body signals via `lower_signal(...)`
- integration with delay emission and output stores

`lower_proj(...)` itself may stay in `module.rs` at first, but should delegate:

- group decoding
- group carrier allocation
- active-context setup
- canonical carrier lookup

So the initial goal is not “move `lower_proj` wholesale”, but “make `lower_proj`
thin”.

---

## 10. Refactor Sequence

### Step 1: extract recursion state/context types

Status: implemented

- add `recursion.rs`
- move `RecArrayInfo`, `RecursionStorageStrategy`, `RecursionCarrierRef`,
  `RecursionDelayRef`, and any recursion-local context bundles there
- keep behavior unchanged

Pass criteria:

- no semantic changes
- `module.rs` imports recursion data types from the new module

### Step 2: extract pure/canonical recursion lookup helpers

Status: implemented

- move:
  - `canonical_group_index(...)`
  - `decode_symbolic_group_bodies(...)`
  - `resolve_active_recursion_carrier(...)`
  - `resolve_recursion_carrier(...)`
  - `resolve_recursion_delay_ref(...)`

Pass criteria:

- `module.rs` no longer owns the lookup algorithms
- existing recursion tests stay green

### Step 3: extract active-group stack discipline

Status: implemented (adapted)

Implementation note:

- the push/pop discipline is now centralized through
  `with_active_recursion_group(...)`
- this helper remains in `module.rs` for now because a fully externalized
  guard/context abstraction would fight the current borrow structure of
  `SignalToFirLower`

- replace raw `recursion_vars.push/pop` and `recursion_stack.push/pop`
  sequences with one helper / guard abstraction

Pass criteria:

- push/pop lifetime is centralized
- no open-coded stack balancing remains in `lower_proj(...)`

### Step 4: extract recursive-group carrier allocation helpers

Status: implemented

- move `ensure_recursion_array_for_group(...)`
- keep FIR declaration emission via an explicit context bundle passed from
  `module.rs`

Pass criteria:

- allocation bookkeeping no longer lives directly in `module.rs`

### Step 5: thin `lower_proj(...)`

Status: partially implemented

- keep `lower_proj(...)` in `module.rs`
- make it mostly orchestration that delegates to the recursion module

Pass criteria:

- `lower_proj(...)` stops owning the recursive-group bookkeeping details

---

## 11. Validation Strategy

This refactor is structural, so validation should focus on non-regression.

### 11.1 Existing recursion tests that must stay green

- simple two-slot feedback recursion
- upsized circular recursion carrier reuse
- top-level recursion projection delay-chain reuse
- mixed recursion + fixed-delay reuse

### 11.2 Structural expectations

- simple recursion still avoids `fIOTA`
- upsized circular recursion still uses shared cursor indexing
- recursive groups are still scheduled once per sample

### 11.3 Integration expectation

- no change in sample-phase ordering
- no reintroduction of aliasing between recursion carriers and standalone
  delay-state slots

---

## 12. Risks

### 12.1 Extracting too much too early

If `lower_proj(...)` is moved wholesale before the boundary is tested, the new
module may end up depending on most of `SignalToFirLower` anyway.

Mitigation:

- first extract data types and canonical helpers
- keep lowering orchestration local

### 12.2 Repeating the old monolith in a new file

Simply moving a large block of code without tightening the API would not really
improve the design.

Mitigation:

- require explicit context structs
- define pass criteria for each extraction step

### 12.3 Delay/recursion coupling regressions

`Delay1^k(Proj(...))` reuse depends on the recursion side and delay side staying
aligned on carrier identity and offsets.

Mitigation:

- keep `RecursionDelayRef` as the canonical interface
- run the existing merged-delay recursion tests at every step

---

## 13. Expected Outcome

If this plan succeeds:

- `module.rs` remains the orchestrator, not the owner of every recursion detail
- recursive-group management gains a focused implementation home
- future recursion parity work has a clearer landing zone
- the delay-side and recursion-side abstractions become more symmetric

This would be a structural cleanliness step, not a semantic change.
