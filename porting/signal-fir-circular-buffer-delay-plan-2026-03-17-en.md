# Plan: Use circular buffers with fIOTA for delay1/recursion

## Context

The faust-rs `signal_fir` fast lane uses two different delay strategies:
- **SIGDELAY** (multi-sample): circular buffers with `fIOTA` — correct, matches C++ `signalFIRCompiler`
- **delay1/recursion**: scalar state variables + deferred shift in `compute_updates` — creates ordering bugs

The C++ `signalFIRCompiler` uses circular buffers with `fIOTA` for **all** delays uniformly
(`writeReadDelay` at line 803 of `signalFIRCompiler.hh`). This inherently avoids ordering issues.

A topological sort was added as a band-aid but adds unnecessary complexity. The proper fix is to
match the C++ approach: use circular buffers for delay1 and recursion too.

## Changes

### File: `crates/transform/src/signal_fir/module.rs`

1. **Remove topological sort** — delete `sort_compute_updates`, `collect_var_reads`,
   `written_var_name` functions and their call site (line ~222).

2. **`ensure_state_slot`** (~line 1289): change from scalar to 2-element array.
   - Currently creates `FirType::Scalar` → change to `FirType::Array(Box::new(typ), 2)`
   - Update `register_clear_init` call to use a 2-element loop init (like `register_clear_recursion_array`)

3. **`lower_delay_state`** (~line 1195): use circular buffer read/write.
   - Ensure `fIOTA` is declared via `ensure_iota_state()`
   - Write immediately (in `sample_statements`): `store_table(name, fIOTA & 1, value)`
   - Read previous value: `load_table(name, (fIOTA - 1) & 1)`
   - Remove the deferred store from `compute_updates`
   - Remove `scheduled_state_updates` guard (no longer needed for deferred stores)

4. **Recursion feedback fast path** in `lower_delay_state` (~line 1201):
   - Change `load_table(name, 1, ...)` to `load_table(name, (fIOTA - 1) & 1, ...)`

5. **`lower_proj`** (~line 2602): use circular buffer for recursion arrays.
   - Write body value: `store_table(name, fIOTA & 1, rhs)` instead of `store_table(name, 0, rhs)`
   - Current value read: `load_table(name, fIOTA & 1, ...)` instead of `load_table(name, 0, ...)`
   - Remove the deferred shift store from `compute_updates` (no more `store_table(name, 1, load(0))`)
   - Ensure `fIOTA` is declared

6. **`ensure_iota_state`**: may need to be called unconditionally whenever delay1 or recursion
   is present (not just for SIGDELAY). The `uses_iota` flag should be set.

### File: `crates/fir/src/lib.rs`

7. **Revert `child_ids`** from `pub` back to private `fn` (was only made public for the topo sort).

### File: `crates/transform/src/signal_fir/module.rs` (imports)

8. **Remove unused imports**: `FirMatch`, `match_fir`, `child_ids` — no longer needed.

## Helper: masked iota index

Add a small helper (matching the C++ `DelayLine::read`/`write` pattern):
```rust
fn masked_iota_index(&mut self, offset: i32, mask: i32) -> FirId {
    let iota_load = self.load_iota();
    if offset == 0 {
        let mask_val = self.lower_int32_const(mask);
        let mut b = FirBuilder::new(&mut self.store);
        b.binop(FirBinOp::And, iota_load, mask_val)
    } else {
        let offset_val = self.lower_int32_const(offset);
        let mask_val = self.lower_int32_const(mask);
        let mut b = FirBuilder::new(&mut self.store);
        let sub = b.binop(FirBinOp::Sub, iota_load, offset_val);
        b.binop(FirBinOp::And, sub, mask_val)
    }
}
```

For 2-element buffers: mask = 1. The existing `delayed_iota_index` for SIGDELAY already does
something similar — reuse the pattern.

## Verification

1. `cargo test -p transform -p compiler` — all tests pass
2. `cargo run -p compiler -- --lang cpp APF.dsp` — verify no `compute_updates` shifts,
   output uses `fIOTA & 1` indexing, same numerical behavior
3. Compare sample output with reference APF1.cpp
