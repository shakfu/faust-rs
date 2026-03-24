# Complete Port of C++ `patternSimplification` / `isBoxNumeric` to Rust

**Date**: 2026-03-24
**Status**: Ready for implementation
**Crate**: `crates/eval/src/lib.rs`

---

## Problem statement

The Rust `pattern_simplification` function (called from `eval_pattern` during
automaton construction) is a **partial port** of the C++ `patternSimplification`.
It only folds literal integer/real arithmetic (`1+2 → 3`), but cannot reduce
complex constant expressions involving `max`, `min`, `sin`, `float(ma.SR)/...`,
etc.  The full power already exists in `simplify_pattern` (which uses
`propagate_box_and_simplify`) but is only wired into the pattern *matcher*, not
into the pattern *pre-processor* (`eval_pattern`).

This means patterns that the C++ compiler accepts silently may fail to match in
Rust with `FRS-EVAL-0099`, depending on whether the mismatch is caught at
construction time or at matching time.

Additionally, three private helpers (`simplify_numeric_pattern`,
`eval_numeric_pattern_value`, `eval_numeric_binary_op`) implement a weaker
subset of functionality that is entirely superseded by `simplify_pattern` and
can be deleted.

---

## C++ reference implementation

### `isBoxNumeric` — `compiler/evaluate/eval.cpp` line 742

```cpp
static bool isBoxNumeric(Tree in, Tree& out)
{
    int    numInputs, numOutputs;
    double x;  int i;
    // 1. Already a literal
    if (isBoxInt(in, &i) || isBoxReal(in, &x)) { out = in; return true; }
    // 2. Closure abstraction — cannot evaluate
    if (isClosure(in, ...) && isBoxAbstr(...)) return false;
    // 3. Any constant expression of type (0→1)
    v = a2sb(in);
    if (getBoxType(v, &numInputs, &numOutputs) && numInputs==0 && numOutputs==1) {
        Tree lsignals = boxPropagateSig(nil, v, []);
        Tree res      = simplify(hd(lsignals));
        if (isSigReal(res, &x)) { out = boxReal(x); return true; }
        if (isSigInt(res, &i))  { out = boxInt(i);  return true; }
    }
    return false;
}
```

### `patternSimplification` — `compiler/evaluate/eval.cpp` line 773

Called from `evalPattern` (automaton construction).

```cpp
static Tree patternSimplification(Tree pattern)
{
    Node n(0); Tree v, t1, t2;
    if (isBoxNumeric(pattern, v)) {
        return v;                              // (a) fold whole expression first
    } else if (isBoxPatternOp(pattern, n, t1, t2)) {
        return tree(n,                         // (b) recurse into PatternOp children only
                    patternSimplification(t1),
                    patternSimplification(t2));
    } else {
        return pattern;                        // (c) leave anything else unchanged
    }
}
```

Where `isBoxPatternOp` matches **only**: `Par`, `Seq`, `Split`, `Merge`, `Rec`.

### `simplifyPattern` — `compiler/evaluate/eval.cpp` line 131

Public function, called from the pattern matcher during argument matching.

```cpp
Tree simplifyPattern(Tree value)
{
    Tree num;
    if (!getNumericProperty(value, num)) {   // memoisation cache
        if (!isBoxNumeric(value, num)) { num = value; }
        setNumericProperty(value, num);
    }
    return num;
}
```

Both `patternSimplification` and `simplifyPattern` call the same `isBoxNumeric`
core — the only difference is memoisation in `simplifyPattern`.

---

## Current Rust state

### What already exists and is correct

| Rust function | Line | C++ equivalent | Status |
|---|---|---|---|
| `simplify_pattern` | 1841 | `simplifyPattern` | ✅ correct full port |
| `propagate_box_and_simplify` | 1816 | `isBoxNumeric` non-trivial path | ✅ correct |

`simplify_pattern` is called from `crates/eval/src/pattern_matcher.rs` when the
automaton state has `match_num = true`.  That path is correct.

### What is wrong

`pattern_simplification` (line 4061), called from `eval_pattern` during automaton
construction, does **not** match the C++ `patternSimplification`:

| Aspect | C++ `patternSimplification` | Rust `pattern_simplification` |
|---|---|---|
| Order | Try `isBoxNumeric` on whole expr **first** | Recurse into children **first** |
| Recursion scope | `isBoxPatternOp` → Par/Seq/Split/Merge/Rec only | Also recurses into HGroup/VGroup/TGroup/Route |
| Simplification power | Full propagation + `simplify()` | Literal arithmetic only |

### Dead code to remove

These three functions implement a strict subset of what `simplify_pattern`
already does.  Once `pattern_simplification` delegates to `simplify_pattern`
they become unreachable:

| Function | Lines | Can be deleted |
|---|---|---|
| `simplify_numeric_pattern` | 4110–4117 | ✅ after refactor |
| `eval_numeric_pattern_value` | 4119–4133 | ✅ after refactor |
| `eval_numeric_binary_op` | 4135–4175 | ✅ after refactor |
| `numeric_add/sub/mul/div/rem` helpers | 4177–4230 | ✅ after refactor |

---

## Fix plan

All changes are in `crates/eval/src/lib.rs`.

### Step 1 — Rewrite `pattern_simplification`

Replace the current implementation with the direct C++ port:

```rust
/// Simplifies a pattern after evaluation, mirroring C++ `patternSimplification`.
///
/// Algorithm (matches C++ exactly):
/// 1. Try to reduce the whole expression to a numeric literal via full
///    propagation + simplify (`simplify_pattern` = C++ `isBoxNumeric`).
/// 2. If that fails AND the pattern is a PatternOp (Par/Seq/Split/Merge/Rec),
///    recurse into its two children.
/// 3. Otherwise return the pattern unchanged.
///
/// Note: HGroup / VGroup / TGroup / Route are NOT PatternOps in C++ and are
/// returned unchanged (not recursed into).
fn pattern_simplification(arena: &mut TreeArena, pattern: TreeId) -> TreeId {
    // (a) Try full constant folding first
    let folded = simplify_pattern(arena, pattern);
    if folded != pattern {
        return folded;
    }
    // (b) Recurse into PatternOp children (Par/Seq/Split/Merge/Rec only)
    match match_box(arena, pattern) {
        BoxMatch::Par(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            BoxBuilder::new(arena).par(sa, sb)
        }
        BoxMatch::Seq(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            BoxBuilder::new(arena).seq(sa, sb)
        }
        BoxMatch::Split(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            BoxBuilder::new(arena).split(sa, sb)
        }
        BoxMatch::Merge(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            BoxBuilder::new(arena).merge(sa, sb)
        }
        BoxMatch::Rec(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            BoxBuilder::new(arena).rec(sa, sb)
        }
        // (c) Everything else (including HGroup/VGroup/TGroup/Route) unchanged
        _ => pattern,
    }
}
```

### Step 2 — Remove dead helpers

Delete the following functions and all their subordinate helpers (they are
entirely superseded by `simplify_pattern`):

- `simplify_numeric_pattern` (line ~4110)
- `eval_numeric_pattern_value` (line ~4119)
- `eval_numeric_binary_op` (line ~4135)
- `numeric_add`, `numeric_sub`, `numeric_mul`, `numeric_div`, `numeric_rem` (line ~4177)
- `numeric_int_binop`, `numeric_as_f64` (line ~4215)
- `NumericValue` enum (line ~4099)

Also remove the `rebuild2` / `rebuild3` helpers if they are no longer needed
after the refactor.

### Step 3 — Check test coverage

Verify existing tests still pass:

```bash
cargo test -p eval
cargo test -p compiler
```

Add a corpus entry for a pattern involving `max`/`min` evaluated at
construction time (not just at match time), for example:

```faust
// tests/corpus/rep_73_pattern_max_min_fold.dsp
f(1) = 10;
f(2) = 20;
f(4) = 40;
// max(1, min(6, 4)) = 4, evaluated at construction time via patternSimplification
process = f(max(1, min(6, 4)));
```

Expected: `process` returns `40` (not FRS-EVAL-0099).

---

## Impact assessment

| Category | Impact |
|---|---|
| Correctness | Fixes construction-time pattern folding for `max`/`min`/xtended ops |
| Parity | `eval_pattern` now calls same codepath as `simplifyPattern` (matcher) |
| Dead code | ~120 lines deleted |
| Scope change | HGroup/VGroup/TGroup/Route no longer recursed — matches C++ exactly |
| Risk | Low — `simplify_pattern` already tested; only `pattern_simplification` logic changes |

---

## Files to modify

| File | Change |
|---|---|
| `crates/eval/src/lib.rs` | Rewrite `pattern_simplification`; delete 5 dead helpers + `NumericValue` |
| `tests/corpus/rep_73_pattern_max_min_fold.dsp` | New regression fixture |
| `crates/compiler/tests/signal_pipeline.rs` | New corpus test for `rep_73` |
