# boxClosure — First-class Closure Node in Tree Arena

**Date**: 2026-03-15
**Status**: Ready for implementation
**Prerequisite**: boxSlot+substitute_tree workaround (current — to be replaced)
**Blocks**: Full parity with C++ on aanl.lib ADAA functions

## Context

In C++ Faust, closures are **tree nodes** (`closure(expr, genv, visited, lenv)`)
that carry their lexical environment. When `revEvalList` evaluates a function
argument like `F1(x_F1) = ...` from a `with` block, `eval()` returns a closure
**tree** — the environment travels with the node through application chains.

In Rust faust-rs, closures are `EvalValue::Closure { expr, env }` — they only
exist in the evaluator domain. When `rev_eval_list` needs to produce a `TreeId`
list, it calls `force_value_to_box`, which must **flatten** the closure back
to a plain tree, **losing the environment**.

The current workaround (boxSlot placeholder + `substitute_tree`) solves the
immediate scoping bug but adds complexity and a full tree traversal per
abstraction. Adding `boxClosure` as a first-class tree node eliminates this
workaround and aligns with the C++ architecture.

## C++ Reference

```cpp
// boxes.cpp — closure IS a tree node
Tree closure(Tree abstr, Tree genv, Tree visited, Tree lenv) {
    return tree(CLOSURE, abstr, genv, visited, lenv);
}
```

Key lifecycle:
| Site | Behaviour |
|------|-----------|
| `eval()` | When ident resolves to closure → returns closure tree as-is |
| `revEvalList()` | Calls `eval()` per arg → closures remain as closure trees |
| `applyList()` | `isClosure(fun, ...)` → extracts env, applies abstraction in captured env |
| `a2sb()` | Detects residual closures → lowers to `symbolic(slot, ...)` |

## Design

### Approach: side-table keyed by `boxInt` (same as `boxPatternMatcher`)

Environments (`Environment`) use `Arc<Mutex<EnvStore>>` and cannot be
hash-consed into the tree arena. Follow the proven `boxPatternMatcher`
pattern: store `ClosureValue` in a dense `Vec` inside `LoopDetector`,
reference it from the tree via `boxClosure(boxInt(key))`.

```
boxClosure(boxInt(key))
    │
    └─► closure_store[key] = ClosureValue { expr: TreeId, env: Environment }
```

Single child (the key), like `boxPatternMatcher(boxInt(key))`.

### Why not 2-child `boxClosure(expr, boxInt(env_key))`

Splitting `expr` and `env_key` into two children adds no value — the stored
`ClosureValue` already holds both. A single-child node is simpler and
follows the `boxPatternMatcher` precedent exactly.

## Implementation Steps

### Step 1 — Add `boxClosure` to box system

**File**: `crates/boxes/src/lib.rs`

1. Add tag constant:
   ```rust
   const BOX_CLOSURE_TAG: &str = "BOXCLOSURE";
   ```

2. Add `BoxMatch` variant:
   ```rust
   Closure(BoxId),  // child = boxInt(key) into closure_store
   ```

3. Add `match_box` dispatch in the 1-child arm:
   ```rust
   BOX_CLOSURE_TAG => BoxMatch::Closure(c0),
   ```

4. Add `BoxBuilder` method + helper:
   ```rust
   pub fn closure_node(&mut self, key: BoxId) -> BoxId {
       node_closure(self.arena, key)
   }
   // ...
   fn node_closure(arena: &mut TreeArena, key: BoxId) -> BoxId {
       intern_tag(arena, BOX_CLOSURE_TAG, &[key])
   }
   ```

5. Add predicate if needed:
   ```rust
   pub fn is_box_closure(arena: &TreeArena, b: BoxId) -> bool { ... }
   ```

### Step 2 — Add closure store to `LoopDetector`

**File**: `crates/eval/src/lib.rs`

1. Add field to `LoopDetector` (next to `pm_store`):
   ```rust
   closure_store: Vec<ClosureValue>,
   ```

2. Add store/get methods (mirror `store_pm`/`get_pm`):
   ```rust
   fn store_closure(&mut self, cv: ClosureValue) -> i32 {
       let key = self.closure_store.len() as i32;
       self.closure_store.push(cv);
       key
   }

   fn get_closure(&self, key: i32) -> Option<ClosureValue> {
       self.closure_store.get(key as usize).cloned()
   }
   ```

3. Initialize in constructors (`new`, `with_cancel`, etc.):
   ```rust
   closure_store: Vec::new(),
   ```

### Step 3 — Replace workaround in `force_value_to_box`

**File**: `crates/eval/src/lib.rs`, function `force_value_to_box`

Replace the current Abstr arm (slot + substitute_tree) with:
```rust
BoxMatch::Abstr(_, _) => {
    let key = loop_detector.store_closure(closure);
    let mut b = BoxBuilder::new(arena);
    let key_node = b.int(key);
    Ok(b.closure_node(key_node))
}
```

This is the same pattern as `PatternMatcher` forcing (lines 2558-2572).

### Step 4 — Handle `boxClosure` in `eval_value`

**File**: `crates/eval/src/lib.rs`, function `eval_value`

Add arm in the main match (alongside `BoxMatch::PatternMatcher`):
```rust
BoxMatch::Closure(key_node) => {
    let key = match match_box(arena, key_node) {
        BoxMatch::Int(k) => k,
        _ => return Err(EvalError::InternalError { ... }),
    };
    let cv = loop_detector.get_closure(key)
        .ok_or_else(|| EvalError::InternalError { ... })?;
    Ok(EvalValue::Closure(cv))
}
```

When eval encounters a `boxClosure` tree node, it extracts the stored
`ClosureValue` and returns it — restoring the full closure with its
captured environment.

### Step 5 — Handle `boxClosure` in `apply_list`

**File**: `crates/eval/src/lib.rs`, function `apply_list`

Add arm to match `boxClosure` and delegate to `apply_value_list_value`:
```rust
BoxMatch::Closure(key_node) => {
    let key = match match_box(arena, key_node) {
        BoxMatch::Int(k) => k,
        _ => return Err(...),
    };
    let cv = loop_detector.get_closure(key).ok_or_else(|| ...)?;
    let result = apply_value_list_value(
        arena,
        EvalValue::Closure(cv),
        larg, env, loop_detector, call_site,
    )?;
    force_value_to_box(arena, result, loop_detector)
}
```

### Step 6 — Handle `boxClosure` in `a2sb`

**File**: `crates/eval/src/lib.rs`, function `a2sb`

Add arm (similar to `BoxMatch::PatternMatcher` handling):
```rust
BoxMatch::Closure(key_node) => {
    let key = match match_box(arena, key_node) {
        BoxMatch::Int(k) => k,
        _ => return Err(...),
    };
    let cv = loop_detector.get_closure(key).ok_or_else(|| ...)?;
    a2sb_value(arena, EvalValue::Closure(cv), loop_detector)
}
```

This dispatches to the existing `lower_abstraction_to_symbolic_value` /
`lower_pattern_matcher_to_symbolic` paths via `a2sb_value`.

### Step 7 — Handle `boxClosure` in `infer_box_arity`

**File**: `crates/eval/src/lib.rs`, function `infer_box_arity`

Add arm returning the abstraction's arity from the stored closure:
```rust
BoxMatch::Closure(key_node) => {
    let key = match match_box(arena, key_node) {
        BoxMatch::Int(k) => k,
        _ => return None,
    };
    let cv = loop_detector.get_closure(key)?;
    // Delegate to the stored expr's arity
    infer_box_arity(arena, cv.expr)
}
```

Note: `infer_box_arity` currently doesn't take `loop_detector`. If needed,
add it to the signature or handle closure nodes in the caller instead.

### Step 8 — Remove `substitute_tree` and slot workaround

**File**: `crates/eval/src/lib.rs`

1. Remove function `substitute_tree` (lines 2134-2162)
2. In `force_value_to_box`, the Abstr arm is already replaced in Step 3
3. `fresh_slot` remains — it's still used by `a2sb` / `lower_abstraction_to_symbolic_value`

### Step 9 — Handle `boxClosure` in `box_simplification` (if needed)

**File**: `crates/eval/src/lib.rs`, function `box_simplification` (or wherever
the simplification pass runs)

`boxClosure` nodes should pass through simplification unchanged (like
`boxPatternMatcher`). Add to the normal-form arm:
```rust
| BoxMatch::Closure(_)
```

## Files Modified

| File | Changes |
|------|---------|
| `crates/boxes/src/lib.rs` | Tag, BoxMatch variant, match_box arm, BoxBuilder method |
| `crates/eval/src/lib.rs` | closure_store, force_value_to_box, eval_value, apply_list, a2sb, infer_box_arity, remove substitute_tree |

## Verification

1. `cargo test` — all existing tests pass (no regression)
2. `cargo test -p eval` — 48 eval tests pass
3. `faust-rs -pn arccos_test tests/aanl_tests.dsp` — no `x_F1` error, passes eval
4. `faust-rs -pn ADAA1_test tests/aanl_tests.dsp` — passes eval
5. `faust-rs -pn hardclip_test tests/aanl_tests.dsp` — passes eval
6. Grep for `substitute_tree` — no remaining references
