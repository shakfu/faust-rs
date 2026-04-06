# Design: `DelayManager` — Standalone Delay-Line Component

**Date**: 2026-04-06
**Scope**: `crates/transform/src/signal_fir/delay.rs`, `module.rs`
**Status**: Design — not yet implemented
**Goal**: Extract all delay-line state and scanning logic into a self-contained
`DelayManager` component, introduce a `DelayLineModel` trait for buffer-geometry
extensibility, and connect the manager to `SignalToFirLower` via an explicit
`DelayFirCtx` borrow bundle.

---

## 1. Current State and Constraints

### 1.1 What was extracted so far (2026-04-06)

`delay.rs` currently holds the **pure sizing layer**: `DelayLineInfo`,
`pow2limit_for_delay`, `constant_delay_amount`, `variable_delay_max_bound`,
`min_const_upper_bound`, `delay_size_for_amount`.  These are stateless free
functions with no FIR side-effects.

All stateful delay methods remain as `impl SignalToFirLower` in `module.rs`
because they need mutable access to `SignalToFirLower`'s private fields.

### 1.2 Delay-specific state in `SignalToFirLower`

Four fields are exclusively owned by delay logic:

| Field | Type | Role |
|-------|------|------|
| `delay_lines` | `HashMap<SigId, DelayLineInfo>` | Allocated ring buffers, keyed by carried signal |
| `rec_group_max_delay` | `HashMap<(u32, usize), i32>` | Max delay per recursion output (recursion+delay merge) |
| `scheduled_delay_writes` | `HashSet<SigId>` | Dedup guard for per-sample delay writes |

(`uses_iota: bool` cannot be moved — it is also read/written by the recursion
lowering path via `ensure_iota_state` calls from `lower_proj`.)

### 1.3 Why full extraction was blocked

The FIR-emitting delay methods also read/write fields shared with recursion,
table, and general lowering:

| Shared field | Used by delay | Also used by |
|---|---|---|
| `struct_declarations` | `ensure_delay_line_decl` | recursion, table, waveform |
| `clear_statements` | `register_clear_table`, `ensure_iota_state` | recursion, waveform |
| `clear_init_seen` | same | recursion, scalar state |
| `sample_statements` | `lower_fixed_delay` | all per-sample lowering |
| `uses_iota` | `ensure_iota_state`, `lower_fixed_delay` | `lower_proj` (recursion) |
| `scheduled_state_updates` | `lower_delay_state` | `lower_proj` |

Additionally, `lower_fixed_delay` and `lower_delay_state` call `lower_signal()`,
the central recursive dispatcher that must remain on `SignalToFirLower`.

---

## 2. Proposed Architecture

The design separates delay concerns into three pieces: a state-owning manager,
a borrowed FIR-context bundle, and an extensible buffer-geometry trait.

### 2.1 `DelayManager` — delay-exclusive state and scan methods

```rust
// delay.rs
pub(super) struct DelayManager {
    delay_lines:            HashMap<SigId, DelayLineInfo>,
    rec_group_max_delay:    HashMap<(u32, usize), i32>,
    scheduled_delay_writes: HashSet<SigId>,
}
```

`SignalToFirLower` replaces its three individual delay fields with one field:

```rust
// module.rs
struct SignalToFirLower<'a> {
    // ... all other fields ...
    delay: DelayManager,
    // delay_lines            ← removed, now delay.delay_lines
    // rec_group_max_delay    ← removed, now delay.rec_group_max_delay
    // scheduled_delay_writes ← removed, now delay.scheduled_delay_writes
}
```

Methods that only need `arena` + `sig_types` and write to `DelayManager`'s
own fields become methods on `DelayManager`:

```rust
impl DelayManager {
    pub(super) fn new() -> Self;

    /// Pre-scan: discover all SIGDELAY nodes, record max delays
    /// and recursion+delay merge patterns.
    pub(super) fn scan_signals(
        &mut self,
        arena: &TreeArena,
        sig_types: &HashMap<SigId, SigType>,
        signals: &[SigId],
    ) -> Result<(), SignalFirError>;

    /// Query: was a write already scheduled for this carried signal?
    pub(super) fn take_delay_write(&mut self, carried: SigId) -> bool;

    /// Query: cached delay line for this carried signal, if allocated.
    pub(super) fn get_delay_line(&self, carried: SigId) -> Option<&DelayLineInfo>;

    /// Query: max total delay recorded for a recursion output.
    pub(super) fn rec_max_delay(&self, var_id: u32, index: usize) -> Option<i32>;

    /// FIR: allocate a delay line (idempotent).
    pub(super) fn ensure_delay_line(
        &mut self,
        carried: SigId,
        delay: i32,
        ctx: &mut DelayFirCtx<'_>,
    ) -> Result<DelayLineInfo, SignalFirError>;
}
```

The scan methods (`scan_signals`, `scan_delay_lines`, `scan_delay_child`,
`try_record_rec_delay`) become private methods of `DelayManager`.  Because
they only read `arena` + `sig_types` and write to `self.delay_lines` /
`self.rec_group_max_delay`, they have no dependency on `SignalToFirLower`
fields and can be unit-tested in isolation.

### 2.2 `DelayFirCtx<'a>` — borrowed context bundle

Methods that emit FIR nodes receive a `DelayFirCtx` built from disjoint
borrows of `SignalToFirLower` fields.  Rust allows splitting borrows across
struct fields, so there is no aliasing issue as long as `delay` (owning the
`DelayManager`) and the other fields are separate.

```rust
// delay.rs
pub(super) struct DelayFirCtx<'a> {
    pub store:               &'a mut FirStore,
    pub real_ty:             FirType,
    pub types:               &'a HashMap<SigId, SimpleSigType>,
    pub struct_declarations: &'a mut Vec<FirId>,
    pub clear_statements:    &'a mut Vec<FirId>,
    pub clear_init_seen:     &'a mut HashSet<String>,
    pub next_loop_var_id:    &'a mut usize,
    pub uses_iota:           &'a mut bool,  // shared delay + recursion
}
```

A helper method on `SignalToFirLower` constructs the context without
borrowing `self.delay`:

```rust
// module.rs
impl<'a> SignalToFirLower<'a> {
    fn delay_fir_ctx(&mut self) -> DelayFirCtx<'_> {
        DelayFirCtx {
            store:               &mut self.store,
            real_ty:             self.real_ty.clone(),
            types:               self.types,
            struct_declarations: &mut self.struct_declarations,
            clear_statements:    &mut self.clear_statements,
            clear_init_seen:     &mut self.clear_init_seen,
            next_loop_var_id:    &mut self.next_loop_var_id,
            uses_iota:           &mut self.uses_iota,
        }
    }
}
```

Call sites in `module.rs` then read as:

```rust
// ensure_delay_line_decl becomes a delegate
fn ensure_delay_line_decl(&mut self, carried: SigId, delay: i32)
    -> Result<DelayLineInfo, SignalFirError>
{
    let mut ctx = self.delay_fir_ctx();
    self.delay.ensure_delay_line(carried, delay, &mut ctx)
}
```

`lower_fixed_delay` and `lower_delay_state` cannot move (they call
`lower_signal()`), but they become thin wrappers:

```rust
fn lower_fixed_delay(&mut self, node, value, amount, delay) -> Result<FirId, _> {
    // Merged-recursion path (reads self.delay internally via recursion_feedback_info)
    // ...
    let mut ctx = self.delay_fir_ctx();
    let line = self.delay.ensure_delay_line(value, delay, &mut ctx)?;
    let current = self.lower_signal(value)?;            // stays here
    if self.delay.take_delay_write(value) {
        let write_idx = delay_write_index(&line, &mut ctx);
        // ...
    }
    // ...
}
```

### 2.3 `DelayLineModel` trait — buffer geometry extensibility

The current power-of-two masking strategy is one choice among several.  A
trait makes the geometry swappable:

```rust
// delay.rs
pub(super) trait DelayLineModel {
    /// Minimum buffer size in elements for a max delay of `max_delay` samples.
    fn buffer_size(&self, max_delay: i32) -> Result<usize, SignalFirError>;

    /// FIR expression: index of the current write slot.
    /// `iota` is a loaded `FirId` for `fIOTA`.
    fn write_index(&self, store: &mut FirStore, iota: FirId, size: usize)
        -> FirId;

    /// FIR expression: index of the slot that is `amount` samples behind.
    fn read_index(&self, store: &mut FirStore, iota: FirId,
                  amount: FirId, size: usize) -> FirId;

    /// FIR statement: advance the write pointer by one step.
    fn bump(&self, store: &mut FirStore, iota: FirId) -> FirId;
}
```

**`CircularPow2Model`** — the current implementation:

```rust
pub(super) struct CircularPow2Model;

impl DelayLineModel for CircularPow2Model {
    fn buffer_size(&self, max_delay: i32) -> Result<usize, SignalFirError> {
        pow2limit_for_delay(max_delay)  // already in delay.rs
    }

    fn write_index(&self, store, iota, size) -> FirId {
        // iota & (size - 1)
        let mask = FirBuilder::new(store).int32((size - 1) as i32);
        FirBuilder::new(store).binop(FirBinOp::And, iota, mask, FirType::Int32)
    }

    fn read_index(&self, store, iota, amount, size) -> FirId {
        // (iota - amount) & (size - 1)
        let raw = FirBuilder::new(store).binop(FirBinOp::Sub, iota, amount, FirType::Int32);
        let mask = FirBuilder::new(store).int32((size - 1) as i32);
        FirBuilder::new(store).binop(FirBinOp::And, raw, mask, FirType::Int32)
    }

    fn bump(&self, store, iota) -> FirId {
        let one = FirBuilder::new(store).int32(1);
        FirBuilder::new(store).binop(FirBinOp::Add, iota, one, FirType::Int32)
    }
}
```

**Possible future models**:

| Model | Buffer size | Index arithmetic | When appropriate |
|-------|-------------|-----------------|-----------------|
| `CircularPow2Model` | `next_power_of_two(N+1)` | `(pos - n) & mask` | Default — all cases |
| `ModuloModel` | `N + 1` (exact) | `(pos - n) % size` | Backends with fast modulo, or when waste from rounding is significant |
| `LinearModel` | `N + 1` | Read `buf[write_pos - n]`, no wrapping | Very short fixed delays where the extra offset can be proven safe |
| `SegmentedModel` | Multiple banks | Bank select + offset | Delays > several seconds where a single 32-bit IOTA would overflow |

`DelayManager` holds a `Box<dyn DelayLineModel>` (or a generic parameter if
monomorphisation is preferred), defaulting to `CircularPow2Model`.

---

## 3. What this changes in `delay.rs`

After the refactor, `delay.rs` holds:

| Item | Kind | Notes |
|------|------|-------|
| `DelayLineInfo` | struct | Unchanged |
| `DelayManager` | struct | New — owns 3 delay-exclusive fields |
| `DelayFirCtx<'a>` | struct | New — borrowed context for FIR emission |
| `DelayLineModel` | trait | New — buffer geometry abstraction |
| `CircularPow2Model` | struct | New — current implementation |
| `pow2limit_for_delay` | free fn | Unchanged |
| `constant_delay_amount` | free fn | Unchanged |
| `variable_delay_max_bound` | free fn | Unchanged |
| `min_const_upper_bound` | free fn | Unchanged |
| `delay_size_for_amount` | free fn | Unchanged |

---

## 4. What stays in `module.rs`

| Method | Why it stays |
|--------|-------------|
| `lower_fixed_delay` | Calls `lower_signal()` |
| `lower_delay_state` | Calls `lower_signal()`, `ensure_state_slot()` |
| `recursion_feedback_info`, `active_recursion_info` | Recursion stack access; also used by `lower_proj` |
| `bump_iota` (as thin wrapper) | Delegates to model, but emits into `sample_statements` via lowerer |
| `ensure_iota_state` | Writes `uses_iota` (shared with recursion) and `struct_declarations` |

The key insight: **nothing that calls `lower_signal()` can leave `module.rs`**,
because `lower_signal` is the recursive core of `SignalToFirLower` and depends
on its full state.

---

## 5. Testability gains

`DelayManager::scan_signals` will have no dependency on FIR or `SignalToFirLower`.
It can be unit-tested directly:

```rust
#[test]
fn test_scan_detects_rec_delay_merge() {
    let mut arena = build_test_arena(/* SIGDELAY(Delay1(Proj(0, rec)), 10) */);
    let sig_types = infer_types(&arena, &signals);
    let mut dm = DelayManager::new();
    dm.scan_signals(&arena, &sig_types, &signals).unwrap();
    assert_eq!(dm.rec_max_delay(var_id, 0), Some(11)); // 10 + 1 for Delay1
}
```

Currently this pattern is only exercised through full end-to-end golden tests.

---

## 6. Migration steps

1. **Add `DelayManager`, `DelayFirCtx`, `DelayLineModel`, `CircularPow2Model`**
   to `delay.rs`.

2. **Replace 3 fields** in `SignalToFirLower` with `delay: DelayManager`.
   Update `SignalToFirLower::new()` accordingly.

3. **Add `delay_fir_ctx()`** helper on `SignalToFirLower`.

4. **Move scan methods** (`scan_delay_lines`, `scan_delay_child`,
   `try_record_rec_delay`) from `module.rs` to `impl DelayManager` in `delay.rs`.
   Update `prepare_delay_lines` to call `self.delay.scan_signals(...)`.

5. **Move `ensure_delay_line_decl`** body to `impl DelayManager::ensure_delay_line`,
   leaving a one-line delegate in `module.rs`.

6. **Thread `CircularPow2Model`** through `ensure_delay_line` and the index
   expression helpers — replace the inline `(x - n) & mask` arithmetic with
   `model.read_index(...)` calls.

7. **Update `lower_fixed_delay`** to use `self.delay.get_delay_line()` and
   `self.delay.take_delay_write()`.

8. Run `cargo test --workspace`. No behaviour change expected.

---

## 7. Deferred questions

- **`uses_iota` ownership**: currently shared between delay and recursion.  A
  cleaner model would be `IotaState` — a small struct that both `DelayManager`
  and recursion lowering borrow as part of their respective context bundles.
  Deferred to avoid scope creep.

- **`rec_group_max_delay` ownership**: populated by delay scanning, consumed by
  `ensure_recursion_array_for_group` (recursion).  This cross-concern dependency
  could be resolved by having `DelayManager` expose a `rec_max_delay()` getter,
  which `module.rs` calls when allocating recursion arrays.  Already reflected
  in the design above.

- **`SegmentedModel` for very long delays**: needed if Faust patches with
  `> 1 second @ 48 kHz` (> 48 000 samples) are to be compiled without a
  4× buffer size overhead from the next power of two.  Not a current blocker.
