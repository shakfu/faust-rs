# Plan: Multi-output recursion groups (SIGPROJ index > 0) in FIR lowering

**Date:** 2026-03-15
**Trigger:** `dsp/freeverb.dsp` fails with FRS-SFIR-0004: `SIGPROJ index 1 unsupported in Step 2C.2 (only 0)`

---

## 1. Context

Freeverb uses the `allpass` filter:

```faust
allpass(dt,fb) = (_,_ <: (*(fb),_:+:@(dt)), -) ~ _ : (!,_);
```

The `~` operator creates a recursion group with **2 outputs** in its body cons-list.
Propagation emits `proj(0, group)` and `proj(1, group)`. The current fast-lane FIR
lowerer (`lower_proj` in `signal_fir/module.rs`) hard-rejects `index ≠ 0`.

Signal preparation (`signal_prepare.rs`) and the sigtype annotator already support
multi-output groups via `infer_group` / `infer_proj` with full `list_to_vec`-based
indexed access. Only the FIR lowering layer needs extension.

## 2. Signal structure of multi-output recursion

Propagation of `A ~ B` (file `propagate/src/lib.rs`, lines 2133-2176):

1. `make_mem_sig_proj_list(n)` creates feedback placeholders: `delay1(proj(i, DEBRUIJNREF(1)))` for `i in 0..n`
2. Right side (`B`) is propagated with these placeholders
3. Left side (`A`) is propagated with feedback outputs + external inputs
4. Result `l2` contains ALL outputs of `A`
5. `group_body = vec_to_list(l2)` → cons-list of all output signals
6. `group = debruijn_rec(group_body)` → De Bruijn recursive binder
7. For each output: if `aperture > 0` → `proj(idx, group)`, else raw signal

After `de_bruijn_to_sym`: `SYMREC(var, cons(body0, cons(body1, nil)))`.
Inside bodies, feedback references become `proj(j, SYMREF(var))`.

## 3. Current limitations in `signal_fir/module.rs`

| Function | Limitation |
|----------|-----------|
| `lower_proj` (line 2366) | `if index != 0 { return Err(...) }` |
| `decode_symbolic_group` (line 2492) | `arena.hd(body_list)` — only first element |
| `active_recursion_info` (line 1084) | Returns one `RecArrayInfo` — no index param |
| `recursion_feedback_info` (line 1074) | `if index != 0 { return Ok(None) }` |
| `recursion_stack` (line 536) | `Vec<RecArrayInfo>` — one array per group |

## 4. Design

### 4.1 Data structure: `recursion_stack` → `Vec<Vec<RecArrayInfo>>`

Each entry becomes a group of arrays, one per output body.
Single-output groups store `vec![info]` (backward compatible).

### 4.2 `active_recursion_info(group, proj_index: usize)`

Resolves `SYMREF(var)` to depth in the stack, then returns `stack[depth][proj_index]`.

### 4.3 `recursion_feedback_info` — remove index-0 restriction

Pass projection index through to `active_recursion_info`.

### 4.4 `decode_symbolic_group_bodies(group) → Option<Vec<SigId>>`

Replace the old `decode_symbolic_group` (which returned only `hd`) with a version
that uses `list_to_vec` to extract ALL body signals.

### 4.5 `lower_proj` — full rewrite

```
fn lower_proj(node, index, group):
    // 1. Active reference — body being lowered references a sibling/self
    if let Some(info) = active_recursion_info(group, index as usize):
        return load_table(info.name, slot=0, info.typ)

    // 2. First encounter — extract all bodies
    bodies = decode_symbolic_group_bodies(group)   // Vec<SigId>

    // 3. Create RecArrayInfo for each body
    group_arrays = Vec::new()
    for (i, body) in bodies:
        ty = signal_fir_type(body)
        init = zero(ty)
        info = ensure_recursion_array(keyed by group+i, ty, init)
        group_arrays.push(info)

    // 4. Push group context, lower ALL bodies
    recursion_vars.push(var)
    recursion_stack.push(group_arrays.clone())
    for (i, body) in bodies:
        if scheduled_state_updates.insert((group, i)):
            rhs = lower_signal(body)
            store_table(group_arrays[i].name, slot=0, rhs)   // current sample
            shift_store(group_arrays[i].name, slot=1, load[0]) // prev ← current
    recursion_stack.pop()
    recursion_vars.pop()

    // 5. Return result for requested index
    return load_table(group_arrays[index].name, slot=0, ty)
```

### 4.6 `ensure_recursion_array` keying

Currently keyed by `node: SigId` (the SIGPROJ node). For multi-output groups where
we lower all bodies on the first encounter, the SIGPROJ nodes for other indices may
not be known yet. Options:

- **Option A:** Use the body SigId as the key (each body is unique).
- **Option B:** Introduce a composite key `(group, index)`.

Option A is simpler since `state_name_by_node` already maps `SigId → String`.

### 4.7 `scan_delay_lines` — cons-list traversal

`scan_delay_child` must enter all bodies of a multi-output group, not just `hd`.
The recent cons-list fix for the sigtype annotator likely already handles this via
list-spine traversal in the recursive children walk. Verify.

## 5. File to modify

`crates/transform/src/signal_fir/module.rs`

## 6. Verification

1. `cargo test` — full suite passes
2. `faust-rs dsp/freeverb.dsp` — compiles successfully
3. Re-verify `dsp/karplus.dsp` and `dsp/delays.dsp` still work
