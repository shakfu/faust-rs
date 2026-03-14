# Eval × Simplify — C++ Parity Plan

**Date**: 2026-03-14
**Source**: `compiler/evaluate/eval.cpp`
**Target**: `crates/eval/src/lib.rs` + `crates/normalize/src/`
**Prerequisite**: `normalize` crate complete (Steps 1–5 done, `simplify` available)
**Status**: planning

---

## 1. Context

The C++ evaluator (`eval.cpp`) uses two signal-level helpers inside the box-level
evaluation loop:

| C++ function | Where defined | Role |
|---|---|---|
| `boxPropagateSig(env, box, inputs)` | `propagate/propagate.cpp` | Propagate a signal list through a box; returns output signal list |
| `simplify(sig)` | `normalize/simplify.cpp` | Algebraic simplification of one signal (sigMap + rewrite rules) |

These are called together in a recurring pattern:

```cpp
Tree lsignals = boxPropagateSig(gGlobal->nil, box, makeSigInputList(n));
Tree s        = simplify(hd(lsignals));
```

In Rust, the `propagate` crate already provides `propagate_typed` (the Rust
equivalent of `boxPropagateSig`). The `normalize` crate now provides `simplify`.
**What is missing is wiring them together inside the eval crate, at the exact
call sites that the C++ evaluator uses.**

---

## 2. C++ Call Sites

Six call sites in `eval.cpp` require porting (line numbers are stable as of
2026-03-14):

### CS-1 · `realeval` → `isBoxSeq` numeric folding (line 397–399)

```cpp
// inside isBoxSeq branch, after evaluating a1, a2
Tree lres = boxPropagateSig(gGlobal->nil, a2, lsig);   // lsig = constant outputs of a1
if (isList(lres) && isNil(tl(lres))) {
    Tree r = simplify(hd(lres));
    if (isNum(r)) { return r; }
}
```

**Purpose**: fold `(2, 3) : +` → `5`, or `(0.5, 2.0) : *` → `1.0`.
Guard: `a1` must be a *numerical tuple* (all outputs are numeric constants)
and `a2` must be a primitive or xtended function.

**Rust gap**: `map_children` rebuilds `Seq(a1, a2)` without attempting folding.

---

### CS-2 · `isBoxRoute` constant propagation (lines 666–668)

```cpp
Tree ls1 = boxPropagateSig(gGlobal->nil, v1, makeSigInputList(0));
Tree ls2 = boxPropagateSig(gGlobal->nil, v2, makeSigInputList(0));
Tree lsr = boxPropagateSig(gGlobal->nil, vr, makeSigInputList(0));
// no simplify call; uses sigList2vecInt to extract integers
if (sigList2vecInt(ls1, w1) && sigList2vecInt(ls2, w2) && sigList2vecInt(lsr, wr))
    return boxRoute(boxInt(w1[0]), boxInt(w2[0]), b_from_wr);
```

**Purpose**: normalise route descriptions so that `ins`, `outs`, and the
routing vector are always literal `boxInt(n)` nodes, even when written as
arithmetic expressions (e.g., `route(2*2, 4, ...)`).

**Rust gap**: `infer_box_arity` (used for route arity) pattern-matches
`BoxMatch::Int(n)` directly; non-literal route arguments silently fail.
Route normalization at eval time is not implemented.

---

### CS-3 · `isBoxNumeric` (line 830–831)

```cpp
static bool isBoxNumeric(Tree in, Tree& out)
{
    // fast path: already a literal
    if (isBoxInt(in, &i) || isBoxReal(in, &x)) { out = in; return true; }
    // general path: type-check then propagate+simplify
    v = a2sb(in);
    if (getBoxType(v, &numInputs, &numOutputs) && numInputs==0 && numOutputs==1) {
        Tree lsignals = boxPropagateSig(gGlobal->nil, v, makeSigInputList(0));
        Tree res      = simplify(hd(lsignals));
        if (isSigReal(res, &x)) { out = boxReal(x); return true; }
        if (isSigInt(res, &i))  { out = boxInt(i);  return true; }
    }
    return false;
}
```

**Purpose**: test whether a box expression denotes a compile-time numeric
constant; returns the canonical `boxInt` / `boxReal` if so.

**Rust gap**: no equivalent function. The existing
`eval_numeric_pattern_value` only handles structural patterns (`Int`, `Real`,
`Seq(Par(lhs, rhs), op)`) and cannot evaluate arbitrary propagate-reducible
expressions.

---

### CS-4 · `eval2double` (line 885–886)

```cpp
static double eval2double(Tree exp, Tree visited, Tree localValEnv)
{
    Tree diagram  = a2sb(eval(exp, visited, localValEnv));
    // ... getBoxType checks (0->1) ...
    Tree lsignals = boxPropagateSig(gGlobal->nil, diagram, makeSigInputList(0));
    Tree val      = simplify(hd(lsignals));
    return tree2double(val);
}
```

**Purpose**: convert a 0→1 box expression to a `double` at compile time.
Used for slider/bargraph `min`, `max`, `step`, `init` values.

**Rust gap**: `eval_slider_like` passes widget params through `eval_box`
only — it does **not** reduce them to numeric constants. Slider parameters
are left as unevaluated box expressions if they are not already literals.

---

### CS-5 · `eval2int` (line 914–915)

```cpp
static int eval2int(Tree exp, Tree visited, Tree localValEnv)
{
    Tree diagram  = a2sb(eval(exp, visited, localValEnv));
    // ... getBoxType checks (0->1) ...
    Tree lsignals = boxPropagateSig(gGlobal->nil, diagram, makeSigInputList(0));
    Tree val      = simplify(hd(lsignals));
    return tree2int(val);
}
```

**Purpose**: same as `eval2double` but extracts an `int`. Used for
`soundfile` channel index, `rdtable` / `rwtable` table-size expressions, and
integer-typed UI params.

**Rust gap**: same as CS-4.

---

### CS-6 · `numericBoxSimplification` / `boxSimplification` (lines 1604, 1644–1645)

```cpp
// boxSimplification: memoised entry point
static Tree boxSimplification(Tree box) {
    Tree simplified;
    if (gGlobal->gSimplifiedBoxProperty->get(box, simplified)) return simplified;
    simplified = numericBoxSimplification(box);
    // copy name property, memoize
    gGlobal->gSimplifiedBoxProperty->set(box, simplified);
    return simplified;
}

// numericBoxSimplification: try propagate+simplify; recurse if non-numeric
static Tree numericBoxSimplification(Tree box) {
    // ... getBoxType ...
    if (ins == 0 && outs == 1) {
        if (isBoxInt(box,&i) || isBoxReal(box,&x)) return box; // already literal
        Tree lsignals = boxPropagateSig(gGlobal->nil, box, makeSigInputList(0));
        Tree s        = simplify(hd(lsignals));
        if (isSigReal(s,&x1)) return boxReal(x1);
        if (isSigInt(s,&i1))  return boxInt(i1);
        return insideBoxSimplification(box);  // recurse into children
    }
    return insideBoxSimplification(box);
}
```

**Purpose**: memoised box-level simplifier called on **every evaluated slider
parameter** in the C++. Reduces constant sub-expressions to literals and
recurses structurally otherwise. This is C++'s main compile-time constant
propagation hook inside the evaluator.

**Rust gap**: not implemented at all.

---

## 3. Current Rust Approximation

The Rust eval already has one hybrid: `eval_box_to_scalar_signal` (line 2843)
used for label interpolation:

```rust
// propagate_typed exists, but simplify is NOT called
let signals = propagate_typed(arena, flat, &[], &mut cache)?;
match match_sig(arena, signals[0]) {
    SigMatch::Int(_) | SigMatch::Real(_) => Ok(signals[0]),
    _ => Err(...)  // fails on non-literal constants like sin(0)
}
```

This already handles most cases (propagation reduces simple arithmetic), but
without a `simplify` call it misses algebraically non-trivial expressions.

---

## 4. Dependency Graph

```
crates/normalize  (has `simplify`, currently pub(crate))
    ↑ new dep
crates/eval       (needs simplify for CS-1..CS-6)
    ↑ already dep
crates/propagate  (has propagate_typed, try_build_flat_box — already used)
```

No circular dependency: `normalize` depends on `signals`, `tlib`, `sigtype`,
`interval` — none of which depend on `eval`.

---

## 5. Implementation Plan (7 steps)

---

### Step 1 — Expose `simplify` in normalize's public API

**File**: `crates/normalize/src/simplify.rs`

Add a thin public wrapper that uses an empty type map (correct for
compile-time constant folding, which is all the eval needs):

```rust
/// Simplifies a signal algebraically using built-in rewrite rules only,
/// without type-context information.  Suitable for constant-folding at
/// evaluation time, before type annotation has run.
///
/// C++ equivalent: `simplify(sig)` in `normalize/simplify.cpp`.
pub fn simplify_const(arena: &mut TreeArena, sig: SigId) -> SigId {
    let types = HashMap::new();
    simplify(arena, &types, sig)
}
```

**File**: `crates/normalize/src/lib.rs`

```rust
pub use simplify::simplify_const;
```

**Test**: add one unit test in `simplify.rs` that calls `simplify_const` on a
`SigAdd(SigInt(2), SigInt(3))` and asserts the result is `SigInt(5)`.

---

### Step 2 — Add `normalize` as a dependency of `eval`

**File**: `crates/eval/Cargo.toml`

```toml
[dependencies]
normalize = { path = "../normalize" }
```

Add to the `use` block in `lib.rs`:

```rust
use normalize::simplify_const;
```

---

### Step 3 — Add `propagate_box_and_simplify` helper

**File**: `crates/eval/src/lib.rs`

A private helper that combines the two-step C++ pattern into a single
reusable function:

```rust
/// Propagates a 0→1 box with no inputs, then algebraically simplifies the
/// resulting signal.
///
/// Returns `None` if the box has the wrong arity or cannot be flattened.
///
/// C++ equivalent:
/// ```cpp
/// Tree lsignals = boxPropagateSig(gGlobal->nil, box, makeSigInputList(0));
/// Tree s        = simplify(hd(lsignals));
/// ```
fn propagate_box_and_simplify(
    arena: &mut TreeArena,
    box_id: TreeId,
) -> Option<SigId> {
    let flat = try_build_flat_box(arena, box_id).ok()?;
    let mut cache = ArityCache::default();
    let signals = propagate_typed(arena, flat, &[], &mut cache).ok()?;
    let sig = *signals.first()?;
    Some(simplify_const(arena, sig))
}
```

This helper is the building block for all subsequent steps.

---

### Step 4 — Port `is_box_numeric` and `box_to_f64` / `box_to_i32` (CS-3, CS-4, CS-5)

**File**: `crates/eval/src/lib.rs`

```rust
/// Returns `Some(boxInt(n))` or `Some(boxReal(x))` if `box_id` is a 0→1 box
/// that reduces to a compile-time numeric constant.
///
/// C++ equivalent: `isBoxNumeric(in, out)` in `eval.cpp`.
fn is_box_numeric(arena: &mut TreeArena, box_id: TreeId) -> Option<TreeId> {
    // Fast path: already a literal
    match match_box(arena, box_id) {
        BoxMatch::Int(_) | BoxMatch::Real(_) => return Some(box_id),
        _ => {}
    }
    // General path: propagate + simplify
    let sig = propagate_box_and_simplify(arena, box_id)?;
    match match_sig(arena, sig) {
        SigMatch::Int(i)  => Some(BoxBuilder::new(arena).int(i)),
        SigMatch::Real(x) => Some(BoxBuilder::new(arena).real(x)),
        _ => None,
    }
}

/// Converts a 0→1 box to `f64`.  Errors if not a constant scalar.
///
/// C++ equivalent: `eval2double(...)` in `eval.cpp`.
fn eval_box_to_f64(
    arena: &mut TreeArena,
    expr: TreeId,
    context_node: TreeId,
) -> Result<f64, EvalError> {
    let sig = propagate_box_and_simplify(arena, expr)
        .ok_or(EvalError::NotAConstantExpression { node: context_node })?;
    match match_sig(arena, sig) {
        SigMatch::Real(x) => Ok(x),
        SigMatch::Int(i)  => Ok(f64::from(i)),
        _ => Err(EvalError::NotAConstantExpression { node: context_node }),
    }
}

/// Converts a 0→1 box to `i32`.  Errors if not a constant scalar.
///
/// C++ equivalent: `eval2int(...)` in `eval.cpp`.
fn eval_box_to_i32(
    arena: &mut TreeArena,
    expr: TreeId,
    context_node: TreeId,
) -> Result<i32, EvalError> {
    let sig = propagate_box_and_simplify(arena, expr)
        .ok_or(EvalError::NotAConstantExpression { node: context_node })?;
    match match_sig(arena, sig) {
        SigMatch::Int(i)  => Ok(i),
        SigMatch::Real(x) => Ok(x as i32),
        _ => Err(EvalError::NotAConstantExpression { node: context_node }),
    }
}
```

Add `NotAConstantExpression { node: TreeId }` variant to `EvalError`.

---

### Step 5 — Port `box_simplification` (CS-6)

**File**: `crates/eval/src/lib.rs`

```rust
/// Attempts to reduce a box expression to a numeric literal constant.
/// Recurses structurally for non-scalar boxes.
/// Results are memoised in `cache` for the duration of one evaluation run.
///
/// C++ equivalent: `boxSimplification(box)` in `eval.cpp`.
fn box_simplification(
    arena: &mut TreeArena,
    cache: &mut HashMap<TreeId, TreeId>,
    box_id: TreeId,
) -> TreeId {
    if let Some(&cached) = cache.get(&box_id) {
        return cached;
    }
    let result = numeric_box_simplification(arena, cache, box_id);
    cache.insert(box_id, result);
    result
}

/// Inner (non-memoised) worker for `box_simplification`.
///
/// C++ equivalent: `numericBoxSimplification(box)` in `eval.cpp`.
fn numeric_box_simplification(
    arena: &mut TreeArena,
    cache: &mut HashMap<TreeId, TreeId>,
    box_id: TreeId,
) -> TreeId {
    // Fast path: already a literal
    match match_box(arena, box_id) {
        BoxMatch::Int(_) | BoxMatch::Real(_) => return box_id,
        _ => {}
    }
    // Only 0→1 boxes can be numerically simplified
    if infer_box_arity(arena, box_id) != Some((0, 1)) {
        return inside_box_simplification(arena, cache, box_id);
    }
    // Propagate + simplify
    if let Some(sig) = propagate_box_and_simplify(arena, box_id) {
        match match_sig(arena, sig) {
            SigMatch::Real(x) => return BoxBuilder::new(arena).real(x),
            SigMatch::Int(i)  => return BoxBuilder::new(arena).int(i),
            _ => {}
        }
    }
    inside_box_simplification(arena, cache, box_id)
}

/// Recurse into children of a box without attempting numeric folding.
///
/// C++ equivalent: `insideBoxSimplification(box)` in `eval.cpp`.
fn inside_box_simplification(
    arena: &mut TreeArena,
    cache: &mut HashMap<TreeId, TreeId>,
    box_id: TreeId,
) -> TreeId {
    let Some(node) = arena.node(box_id).cloned() else {
        return box_id;
    };
    let new_children: Vec<_> = node
        .children
        .as_slice()
        .iter()
        .map(|&c| box_simplification(arena, cache, c))
        .collect();
    arena.intern(node.kind, &new_children)
}
```

---

### Step 6 — Apply `box_simplification` on UI widget parameters (CS-4, CS-5, CS-6)

This is the primary call site of `box_simplification` in C++.

**File**: `crates/eval/src/lib.rs`

Modify `eval_slider_like`, `eval_hslider`, `eval_vslider`, `eval_num_entry`,
`eval_bargraph` and similar:

**Before** (current Rust):
```rust
fn eval_slider_like(arena, kind, label, [cur, min, max, step], env, ldet) -> Result<TreeId> {
    let label = evaluated_label_node(...)?;
    let cur  = eval_box(arena, cur,  env, ldet)?;
    let min  = eval_box(arena, min,  env, ldet)?;
    let max  = eval_box(arena, max,  env, ldet)?;
    let step = eval_box(arena, step, env, ldet)?;
    Ok(BoxBuilder::new(arena).hslider(label, cur, min, max, step))
}
```

**After** (with box_simplification):
```rust
fn eval_slider_like(arena, kind, label, [cur, min, max, step], env, ldet) -> Result<TreeId> {
    let label = evaluated_label_node(...)?;
    let cur  = eval_box(arena, cur,  env, ldet)?;
    let min  = eval_box(arena, min,  env, ldet)?;
    let max  = eval_box(arena, max,  env, ldet)?;
    let step = eval_box(arena, step, env, ldet)?;
    let mut simp_cache = HashMap::new();
    let cur  = box_simplification(arena, &mut simp_cache, cur);
    let min  = box_simplification(arena, &mut simp_cache, min);
    let max  = box_simplification(arena, &mut simp_cache, max);
    let step = box_simplification(arena, &mut simp_cache, step);
    Ok(BoxBuilder::new(arena).hslider(label, cur, min, max, step))
}
```

Apply the same pattern to:
- `eval_bargraph` (min, max)
- `eval_soundfile` (channel count — use `eval_box_to_i32` for validation)
- `eval_rdtable` / `eval_rwtable` table-size (use `eval_box_to_i32`)

---

### Step 7 — Port BoxSeq numeric folding (CS-1)

**File**: `crates/eval/src/lib.rs`

Currently `BoxSeq` is handled by the `_ => map_children(...)` fallback in
`eval_value`. Add a dedicated arm:

```rust
/// Evaluates `a1 : a2` with opportunistic numeric folding.
///
/// If `a1` is a numerical tuple (all outputs are constant signals) and `a2`
/// is a primitive or xtended function, attempt to propagate `a2` through the
/// constant signals of `a1` and simplify the result.  If the result is a
/// numeric literal, return it directly, bypassing the `Seq` node.
///
/// C++ equivalent: `realeval` → `isBoxSeq` branch (lines 383–406 of eval.cpp).
fn eval_seq_with_folding(
    arena: &mut TreeArena,
    a1: TreeId,
    a2: TreeId,
) -> Option<TreeId> {
    // Propagate a1 with no inputs to get its output signals
    let flat1 = try_build_flat_box(arena, a1).ok()?;
    let mut cache = ArityCache::default();
    let signals1 = propagate_typed(arena, flat1, &[], &mut cache).ok()?;

    // Guard: all signals must be numeric constants
    let all_numeric = signals1.iter().all(|&s| {
        matches!(match_sig(arena, s), SigMatch::Int(_) | SigMatch::Real(_))
    });
    if !all_numeric { return None; }

    // Guard: a2 must be a primitive or xtended (not a composition)
    match match_box(arena, a2) {
        BoxMatch::Add | BoxMatch::Sub | BoxMatch::Mul | BoxMatch::Div
        | BoxMatch::Rem | BoxMatch::Pow | BoxMatch::Lt  | BoxMatch::Le
        | BoxMatch::Gt  | BoxMatch::Ge  | BoxMatch::Eq  | BoxMatch::Ne
        | BoxMatch::And | BoxMatch::Or  | BoxMatch::Xor
        | BoxMatch::Shl | BoxMatch::Shr | BoxMatch::Abs
        | BoxMatch::Floor | BoxMatch::Ceil | BoxMatch::Rint
        | BoxMatch::Sin | BoxMatch::Cos | BoxMatch::Tan | BoxMatch::Asin
        | BoxMatch::Acos | BoxMatch::Atan | BoxMatch::Atan2
        | BoxMatch::Exp | BoxMatch::Log | BoxMatch::Log10
        | BoxMatch::Sqrt | BoxMatch::Pow | BoxMatch::Min | BoxMatch::Max
        | BoxMatch::FMod | BoxMatch::Remainder | BoxMatch::Wire => {}
        _ => return None,
    }

    // Propagate a2 through the constant signal list
    let flat2 = try_build_flat_box(arena, a2).ok()?;
    let signals2 = propagate_typed(arena, flat2, &signals1, &mut cache).ok()?;
    if signals2.len() != 1 { return None; }

    // Simplify and check if numeric
    let simplified = simplify_const(arena, signals2[0]);
    match match_sig(arena, simplified) {
        SigMatch::Int(i)  => Some(BoxBuilder::new(arena).int(i)),
        SigMatch::Real(x) => Some(BoxBuilder::new(arena).real(x)),
        _ => None,
    }
}
```

Wire into `eval_value`:

```rust
// In the eval_value match dispatch, BEFORE the _ => map_children fallback:
BoxMatch::Seq(raw_a1, raw_a2) => {
    let a1 = force_value_to_box(arena, eval_value(arena, raw_a1, env, ldet)?, ldet)?;
    let a2 = force_value_to_box(arena, eval_value(arena, raw_a2, env, ldet)?, ldet)?;
    if let Some(folded) = eval_seq_with_folding(arena, a1, a2) {
        return Ok(EvalValue::Box(folded));
    }
    Ok(EvalValue::Box(BoxBuilder::new(arena).seq(a1, a2)))
}
```

---

### Step 8 — Fix `eval_box_to_scalar_signal` with `simplify_const` (CS-3)

**File**: `crates/eval/src/lib.rs`
**Lines**: ~2886–2892

After `propagate_typed`, add a `simplify_const` call so that
non-trivially-reduced constant expressions (e.g., `sin(0)`) also pass:

```rust
// Before:
match match_sig(arena, signals[0]) {
    SigMatch::Int(_) | SigMatch::Real(_) => Ok(signals[0]),
    _ => Err(...)
}

// After:
let simplified = simplify_const(arena, signals[0]);
match match_sig(arena, simplified) {
    SigMatch::Int(_) | SigMatch::Real(_) => Ok(simplified),
    _ => Err(...)
}
```

---

## 6. Route Box Parameter Resolution (CS-2)

### The gap — demonstrated by a concrete test

The C++ evaluator normalises the three sub-expressions of a `route(ins, outs,
spec)` box into canonical `boxInt(n)` literals **at evaluation time**:

```cpp
// eval.cpp lines 652–668 (isBoxRoute branch)
Tree v1 = a2sb(eval(ins, ...));   // may be arithmetic: e.g. 1+1
Tree v2 = a2sb(eval(outs, ...));
Tree vr = a2sb(eval(routes, ...));
// ...
Tree ls1 = boxPropagateSig(gGlobal->nil, v1, makeSigInputList(0));
Tree ls2 = boxPropagateSig(gGlobal->nil, v2, makeSigInputList(0));
Tree lsr = boxPropagateSig(gGlobal->nil, vr, makeSigInputList(0));
if (sigList2vecInt(ls1, w1) && sigList2vecInt(ls2, w2) && sigList2vecInt(lsr, wr))
    return boxRoute(boxInt(w1[0]), boxInt(w2[0]), par_from_wr);
```

The result is always a canonical `boxRoute(boxInt(n_in), boxInt(n_out),
par_of_ints)` with every sub-node a literal integer.

**Why the Rust propagate crate cannot compensate**: both
`usize_from_int_node` (used by `box_arity_flat_inner`) and
`flatten_route_ints_into` (used by route propagation) call `tree_to_int`,
which expects a **literal** `Int` node.  When `ins` or `outs` is an
arithmetic expression (`boxAdd(boxInt(1), boxInt(1))`), both functions return
a `PropagateError::InvalidIntegerValue` or `PropagateError::UnsupportedBox`.
The eval crate's current `map_children` fallback evaluates children but does
**not** reduce them to literals.

### Regression test that exposes the gap

Add to `tests/corpus/rep_70_route_arithmetic_params.dsp`:

```faust
// route whose ins/outs/spec are computed via arithmetic — must be resolved
// to literals during evaluation, as C++ eval.cpp does via boxPropagateSig.
// C++ compiles cleanly; Rust propagate crate currently fails with
// PropagateError when the route node's Int children are not literals.
process = route(1+1, 1+1, 1,1, 2,2);
```

The corresponding golden file (`compiler_stdout.txt`) should be generated
**after** the fix is applied (so the file captures the correct output).

**Confirmed current failure** (`cargo run -p compiler -- rep_70.dsp`):

```
error [FRS-PROP-0099] invalid integer value for `route inputs` at node 6
  9 | process = route(1+1, 1+1, 1,1, 2,2);
    |                            ^
  = note: expr=(1 + 1)
```

`usize_from_int_node` in `crates/propagate/src/lib.rs` calls `tree_to_int`
which requires a **literal** `Int` node. `(1+1)` is a `BoxAdd` — not a
literal — so the call returns `None` and propagation errors.

### Implementation — Step 6b: `eval_route_normalize` in eval

Add a dedicated `BoxMatch::Route` arm in `eval_value` (replacing the
`map_children` fallback for this node kind).

**Two helpers needed**:

#### `eval_box_to_int_node`

Reduces a box to a `boxInt(n)` literal.  Thin wrapper over `eval_box_to_i32`
(defined in Step 4) that rebuilds a `boxInt` from the extracted value:

```rust
fn eval_box_to_int_node(
    arena: &mut TreeArena,
    box_id: TreeId,
) -> Option<TreeId> {
    // Fast path
    if let BoxMatch::Int(_) = match_box(arena, box_id) {
        return Some(box_id);
    }
    // Propagate + simplify + extract i32
    let sig = propagate_box_and_simplify(arena, box_id)?;
    match match_sig(arena, sig) {
        SigMatch::Int(i)  => Some(BoxBuilder::new(arena).int(i)),
        SigMatch::Real(x) => Some(BoxBuilder::new(arena).int(x as i32)),
        _ => None,
    }
}
```

#### `normalize_route_spec`

Walks the nested `par(...)` structure of a route specification and reduces
every leaf to a `boxInt(n)` literal.  Mirrors C++'s combined
`boxPropagateSig` + `sigList2vecInt` + route-rebuilding logic:

```rust
/// Reduces every leaf of a route specification to a `boxInt(n)` literal.
///
/// A route spec is a nested `boxPar(boxInt(a), boxPar(boxInt(b), ...))`.
/// If any leaf cannot be reduced to an integer, returns `None` (falling
/// back to leaving the spec as-is, as C++ does for pattern variables).
///
/// C++ equivalent: `sigList2vecInt` + route-rebuilding in `realeval`
/// (eval.cpp lines 674–679).
fn normalize_route_spec(
    arena: &mut TreeArena,
    spec: TreeId,
) -> Option<TreeId> {
    match match_box(arena, spec) {
        BoxMatch::Par(left, right) => {
            let l = normalize_route_spec(arena, left)?;
            let r = normalize_route_spec(arena, right)?;
            Some(BoxBuilder::new(arena).par(l, r))
        }
        BoxMatch::Int(_) => Some(spec),  // already a literal
        _ => eval_box_to_int_node(arena, spec),
    }
}
```

#### Dedicated `BoxMatch::Route` arm in `eval_value`

```rust
// Insert BEFORE the `_ => map_children(...)` fallback:
BoxMatch::Route(raw_ins, raw_outs, raw_routes) => {
    // Evaluate all three children in the current environment
    let ins    = eval_box(arena, raw_ins,    env, loop_detector)?;
    let outs   = eval_box(arena, raw_outs,   env, loop_detector)?;
    let routes = eval_box(arena, raw_routes, env, loop_detector)?;

    // Attempt canonical normalisation: reduce ins, outs, and every
    // route-spec leaf to boxInt(n) literals.
    // Falls back to the unevaluated sub-expressions when a parameter
    // is a pattern variable, slot, or wire (matching C++ fallback at
    // eval.cpp line 683–686).
    let norm_ins    = eval_box_to_int_node(arena, ins).unwrap_or(ins);
    let norm_outs   = eval_box_to_int_node(arena, outs).unwrap_or(outs);
    let norm_routes = normalize_route_spec(arena, routes).unwrap_or(routes);

    Ok(EvalValue::Box(BoxBuilder::new(arena).route(norm_ins, norm_outs, norm_routes)))
}
```

The graceful `unwrap_or` fallback ensures that pattern-variable route
descriptions (used in `case` rules) are preserved unchanged, matching the C++
`isBoxPatternVar` / `isBoxWire` / `isBoxSlot` fallback at eval.cpp line 683.

---

## 7. `EvalError` additions

Add these new variants to `EvalError` in `lib.rs`:

```rust
/// A box expression was expected to evaluate to a compile-time numeric
/// constant (type 0→1 with a numeric value), but did not.
///
/// Occurs in slider parameter evaluation, table-size expressions, and
/// similar contexts where the C++ compiler calls `eval2int` / `eval2double`.
NotAConstantExpression {
    node: TreeId,
},
```

Implement `Display` and add a test.

---

## 8. Testing Strategy

### Unit tests (in `eval/src/lib.rs`)

| Test name | What it checks |
|---|---|
| `propagate_box_and_simplify_int_add` | `box(2+3)` → `SigInt(5)` |
| `propagate_box_and_simplify_float_mul` | `box(0.5*2.0)` → `SigReal(1.0)` |
| `is_box_numeric_literal` | `boxInt(7)` → `Some(boxInt(7))` |
| `is_box_numeric_expression` | `box(2+3)` → `Some(boxInt(5))` |
| `is_box_numeric_wire` | `boxWire` → `None` (has input) |
| `eval_box_to_f64_literal` | `boxReal(3.14)` → `Ok(3.14)` |
| `eval_box_to_i32_arithmetic` | `boxAdd(boxInt(2), boxInt(3))` → `Ok(5)` |
| `box_simplification_literal` | `boxInt(7)` → `boxInt(7)` (identity) |
| `box_simplification_constant_expr` | `box(sin(0.0))` → `boxReal(0.0)` |
| `eval_seq_numeric_fold` | `Seq(Par(Int(2),Int(3)), Add)` → `Int(5)` |
| `eval_seq_no_fold_with_input` | `Seq(Wire, Add)` → `Seq(Wire, Add)` |
| `eval_box_to_int_node_literal` | `boxInt(4)` → `Some(boxInt(4))` (fast path) |
| `eval_box_to_int_node_arithmetic` | `boxAdd(boxInt(1), boxInt(1))` → `Some(boxInt(2))` |
| `normalize_route_spec_literals` | `Par(Int(1), Par(Int(1), Par(Int(2), Int(2))))` → identity |
| `normalize_route_spec_arithmetic` | `Par(Add(1,1), Par(Int(1), Int(2)))` → `Par(Int(2), Par(Int(1), Int(2)))` |
| `eval_route_arithmetic_params` | `route(1+1, 1+1, 1,1, 2,2)` eval → `route(2, 2, par(1,par(1,par(2,2))))` |
| `eval_route_pattern_var_preserved` | route with `SlotId` child → left unchanged (pattern fallback) |

### Golden tests

Run `cargo run -p xtask -- golden-check` after implementation.

Expected impact: zero regressions on the 86 existing golden cases.

New corpus test added to expose and then lock in the CS-2 fix:

| File | Purpose |
|---|---|
| `tests/corpus/rep_70_route_arithmetic_params.dsp` | Route with `1+1` for ins/outs — currently fails in propagate; must compile after fix |

Additional corpus tests may be added if needed to capture CS-1 / CS-6
behaviour (e.g., `(0.5, 2.0) : *` as a slider parameter, `sin(0.0)` as a
slider default value).

---

## 9. File Changeset Summary

| File | Change |
|---|---|
| `crates/normalize/src/simplify.rs` | Add `pub fn simplify_const(arena, sig) -> SigId` |
| `crates/normalize/src/lib.rs` | `pub use simplify::simplify_const` |
| `crates/eval/Cargo.toml` | Add `normalize = { path = "../normalize" }` |
| `crates/eval/src/lib.rs` | Add `use normalize::simplify_const` |
| `crates/eval/src/lib.rs` | Add `propagate_box_and_simplify` (Step 3) |
| `crates/eval/src/lib.rs` | Add `is_box_numeric`, `eval_box_to_f64`, `eval_box_to_i32` (Step 4) |
| `crates/eval/src/lib.rs` | Add `eval_box_to_int_node`, `normalize_route_spec` (Step 6b) |
| `crates/eval/src/lib.rs` | Add dedicated `BoxMatch::Route` arm in `eval_value` (Step 6b) |
| `crates/eval/src/lib.rs` | Add `box_simplification`, `numeric_box_simplification`, `inside_box_simplification` (Step 5) |
| `crates/eval/src/lib.rs` | Update `eval_slider_like`, `eval_bargraph`, `eval_soundfile` (Step 6a) |
| `crates/eval/src/lib.rs` | Add `eval_seq_with_folding`; add `BoxMatch::Seq` arm in `eval_value` (Step 7) |
| `crates/eval/src/lib.rs` | Add `simplify_const` call in `eval_box_to_scalar_signal` (Step 8) |
| `crates/eval/src/lib.rs` | Add `EvalError::NotAConstantExpression` variant |
| `tests/corpus/rep_70_route_arithmetic_params.dsp` | New corpus test exposing CS-2 gap |
| `tests/golden/rust/rep_70_route_arithmetic_params/compiler_stdout.txt` | Golden generated after fix |

---

## 10. Recommended Implementation Order

```
Step 1    normalize: expose simplify_const (public API)
Step 2    eval: add normalize dep
Step 3    eval: propagate_box_and_simplify helper
Step 4    eval: is_box_numeric, eval_box_to_f64, eval_box_to_i32
Step 8    eval: fix eval_box_to_scalar_signal (cheapest win, affects label interpolation)
Step 6b   eval: eval_box_to_int_node + normalize_route_spec + BoxMatch::Route arm
            + add tests/corpus/rep_70 to expose and lock in the gap
Step 5    eval: box_simplification + inside_box_simplification
Step 6a   eval: slider/widget param simplification (highest correctness impact)
Step 7    eval: BoxSeq numeric folding (most complex, least risk of regression)
```

Steps 1–4 and Step 8 form one preparatory commit ("wire simplify_const into eval").
Step 6b is one self-contained commit ("normalize route parameters at eval time").
Steps 5–7 are one commit each (or one combined commit with careful golden
verification).  Each commit must leave `cargo clippy --workspace --all-targets
-- -D warnings` and `cargo run -p xtask -- golden-check` clean.
