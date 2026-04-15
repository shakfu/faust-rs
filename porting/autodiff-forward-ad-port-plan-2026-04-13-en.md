# Automatic Differentiation — Forward-Mode Port Plan

> **SUPERSEDED (2026-04-15)**: The `fad(exp)` single-argument form documented
> here has been replaced by `fad(exp, x)`. See
> `fad-explicit-diff-variable-plan-2026-04-15-en.md`.

**Date:** 2026-04-13
**Scope:** Port `fad(expr)` and `rad(expr)` primitives from C++ Faust compiler to faust-rs. Implement forward-mode AD signal transformation. Reverse-mode (`rad`) is parsed and evaluated but **not** propagated in this phase.

**Reference documents:**
- `Faust_Autodifferentiation_report.pdf` — synthesis document
- `Automatic_Differentiation_in_the_Faust_Compiler_(v1).pdf` — detailed architecture

**C++ source:**
- Parser: `compiler/parser/faustlexer.l`, `compiler/parser/faustparser.y`
- Boxes: `compiler/boxes/boxes.cpp` (`boxForwardAD`, `boxReverseAD`)
- Eval: `compiler/evaluate/eval.cpp` (lines 673–677)
- Propagate: `compiler/propagate/propagate.cpp` (lines 562–566)
- Forward AD transform: `compiler/transform/forwardADSignalTransform.{hh,cpp}`
- Control collector: `compiler/transform/ADSignalTransform.hh` (`SignalADControlsCollector`)

---

## Architecture Overview

The AD system follows the existing **box → signal → transform → box** pipeline:

```
fad(expr)           rad(expr)
    ↓                   ↓
 Parser          Parser
    ↓                   ↓
 boxForwardAD     boxReverseAD      ← new box primitives
    ↓                   ↓
 Eval (recurse)   Eval (recurse)    ← evaluate inner expr
    ↓                   ↓
 Propagate        Propagate
    ↓                   ↓
 generateFAD      (error/stub)      ← signal-to-signal transform
    ↓
 expanded signal list: [primal_1, tangent_1_c1, tangent_1_c2, ..., primal_2, ...]
```

Forward-mode AD transforms each signal `s` into a **dual number** `(primal, tangent)` where:
- `primal` = original signal value
- `tangent` = derivative with respect to the current differentiation variable (a UI control)

The transformation runs once per differentiable UI control found in the expression. Output is the original outputs interleaved with their derivatives for each control.

---

## Step 1 — Box Primitives

**Goal:** Add `ForwardAD(BoxId)` and `ReverseAD(BoxId)` box node types.

### 1.1 `crates/boxes/src/tags.rs`

Add two new tag constants:

```rust
pub(crate) const BOX_FORWARD_AD_TAG: &str = "BOXFAUTODIFF";
pub(crate) const BOX_REVERSE_AD_TAG: &str = "BOXRAUTODIFF";
```

### 1.2 `crates/boxes/src/matcher.rs`

Add two new `BoxMatch` variants:

```rust
ForwardAD(BoxId),   // fad(expr) — wraps inner expression
ReverseAD(BoxId),   // rad(expr) — wraps inner expression
```

Add matching arms in `match_box()` for the new tags. These are unary (1 child):

```rust
BOX_FORWARD_AD_TAG => BoxMatch::ForwardAD(children[0]),
BOX_REVERSE_AD_TAG => BoxMatch::ReverseAD(children[0]),
```

### 1.3 `crates/boxes/src/builder.rs`

Add two builder methods:

```rust
pub fn forward_ad(&mut self, expr: BoxId) -> BoxId {
    self.arena.intern_tag(BOX_FORWARD_AD_TAG, &[expr])
}

pub fn reverse_ad(&mut self, expr: BoxId) -> BoxId {
    self.arena.intern_tag(BOX_REVERSE_AD_TAG, &[expr])
}
```

### 1.4 `crates/boxes/src/arity.rs` (if exists) or arity handling in propagate

AD boxes are transparent wrappers — their arity is the arity of the inner expression. This must be handled in `box_arity_typed()`.

### 1.5 Tests

- Unit tests: construct `forward_ad(par(hslider(...), sin()))`, verify `match_box` round-trips correctly.
- Arity test: `box_arity_typed(forward_ad(expr))` == `box_arity_typed(expr)`.

---

## Step 2 — Parser

**Goal:** Recognize `fad(expr)` and `rad(expr)` syntax.

### 2.1 `crates/parser/src/grammar/faustlexer.l`

Add two new lexer tokens:

```
"fad"    return FAUTODIFF;
"rad"    return RAUTODIFF;
```

### 2.2 `crates/parser/src/grammar/faustparser.y`

Add token declarations and grammar rules:

```
%token FAUTODIFF RAUTODIFF

ffad : FAUTODIFF LPAR expression RPAR { $$ = BoxBuilder::forward_ad($3); }
frad : RAUTODIFF LPAR expression RPAR { $$ = BoxBuilder::reverse_ad($3); }
```

Integrate `ffad` and `frad` into the `primary` production rule (same position as C++).

### 2.3 Tests

- Parse `fad(hslider("freq", 440, 50, 2000, 0.01) : sin)` → verify box tree structure.
- Parse `rad(process)` → verify box tree structure.
- Parse error: `fad()` (missing expression), `fad(1, 2)` (too many arguments).

---

## Step 3 — Evaluator

**Goal:** Handle `ForwardAD` and `ReverseAD` in `eval_value_uncached`.

### 3.1 `crates/eval/src/lib.rs`

Add two new arms in `eval_value_uncached` match:

```rust
BoxMatch::ForwardAD(inner) => {
    let evaluated = eval_value(arena, inner, env, loop_detector)?;
    let inner_box = force_value_to_box(arena, evaluated, loop_detector)?;
    let mut b = BoxBuilder::new(arena);
    Ok(EvalValue::Box(b.forward_ad(inner_box)))
}
BoxMatch::ReverseAD(inner) => {
    let evaluated = eval_value(arena, inner, env, loop_detector)?;
    let inner_box = force_value_to_box(arena, evaluated, loop_detector)?;
    let mut b = BoxBuilder::new(arena);
    Ok(EvalValue::Box(b.reverse_ad(inner_box)))
}
```

This mirrors the C++ `eval.cpp` pattern: recursively evaluate the inner expression, then re-wrap in the AD box.

### 3.2 Pattern matcher support

Add `ForwardAD` and `ReverseAD` to any pattern-matching infrastructure that enumerates box kinds (e.g., `is_binary_box_tag` in `crates/eval/src/pattern_matcher.rs`). These are unary structural nodes and should be treated like `Metadata` or `Component` in pattern contexts.

### 3.3 Tests

- `eval_process` on `process = fad(hslider("f", 440, 50, 2000, 1) : sin);` → verify the evaluated box is `ForwardAD(...)` wrapping a flat expression.

---

## Step 4 — Flat-Box Validation and Propagation Bridge

**Goal:** Allow `ForwardAD` and `ReverseAD` to pass the flat-box validation gate and be lowered in propagation.

### 4.1 `crates/propagate/src/lib.rs` — `FlatNodeKind`

Add two new variants:

```rust
enum FlatNodeKind {
    // ... existing variants ...
    ForwardAD { body: FlatBoxId },
    ReverseAD { body: FlatBoxId },
}
```

### 4.2 `try_build_flat_box` / flat-box validation

Add `BoxMatch::ForwardAD(inner)` and `BoxMatch::ReverseAD(inner)` to the accepted set. Recursively validate the inner expression.

### 4.3 `box_arity_typed`

```rust
FlatNodeKind::ForwardAD { body } | FlatNodeKind::ReverseAD { body } => {
    // AD boxes are transparent for arity — their arity comes from the
    // inner expression. The actual output count changes after propagation
    // (augmented with derivatives), but at the box level the user sees
    // the wrapped expression's arity.
    box_arity_inner(arena, body)
}
```

**Note:** The output arity of `fad(expr)` is actually `n_outputs * (1 + n_controls)`. This expansion happens during `propagate_inner`, not at the box arity level. The C++ compiler also handles this transparently — box arity returns the inner arity, and the signal list expansion happens in `generateFADSignals`.

### 4.4 `propagate_inner`

```rust
FlatNodeKind::ForwardAD { body } => {
    let inner_sigs = propagate_inner(arena, body, inputs, ctx)?;
    generate_fad_signals(arena, &inner_sigs)
}
FlatNodeKind::ReverseAD { body } => {
    // Phase 1: rad is not yet implemented in propagation
    Err(PropagateError::Unsupported {
        message: "'rad' primitive not yet implemented".to_owned(),
    })
}
```

### 4.5 Tests

- Flat-box validation accepts `fad(sin)` and `rad(sin)`.
- Propagation of `fad(hslider(...) : sin)` produces expanded signal list.
- Propagation of `rad(...)` returns a clear error.

---

## Step 5 — AD Control Collector

**Goal:** Implement a signal visitor that collects all differentiable UI controls in a signal graph.

### 5.1 New module: `crates/propagate/src/ad_controls.rs` (or in `crates/transform/src/ad_controls.rs`)

```rust
use ahash::AHashSet;
use signals::{SigId, SigMatch, match_sig, ControlId};
use tlib::TreeArena;

/// Collects all differentiable UI controls (hslider, vslider, numentry)
/// reachable from a set of signal roots.
///
/// Controls annotated with `[autodiff:false]` metadata are excluded.
pub struct ADControlCollector {
    pub controls: Vec<SigId>,           // ordered, deduplicated
    visited: AHashSet<SigId>,
    seen_controls: AHashSet<SigId>,     // for deduplication
}

impl ADControlCollector {
    pub fn new() -> Self { ... }

    pub fn collect(&mut self, arena: &TreeArena, sig: SigId) {
        if !self.visited.insert(sig) { return; }
        match match_sig(arena, sig) {
            SigMatch::HSlider(ctrl) | SigMatch::VSlider(ctrl) | SigMatch::NumEntry(ctrl) => {
                if is_autodiff_enabled(arena, ctrl) && self.seen_controls.insert(sig) {
                    self.controls.push(sig);
                }
            }
            // Recurse into children for all other nodes
            _ => {
                for child in sig_children(arena, sig) {
                    self.collect(arena, child);
                }
            }
        }
    }
}
```

### 5.2 `[autodiff:false]` metadata check

Implement `is_autodiff_enabled(arena, ctrl) -> bool` that parses the label metadata of a control and checks for the `[autodiff:false]` tag. Default is `true` (enabled).

This reuses the existing metadata extraction infrastructure (if available) or implements a simple string search on the control label for `[autodiff:false]`.

### 5.3 Tests

- Collect controls from `hslider("f", ...) * hslider("g", ...)` → 2 controls.
- Collect controls from `hslider("f [autodiff:false]", ...) * hslider("g", ...)` → 1 control.
- Collect controls from `sin(button("b"))` → 0 controls (buttons are non-differentiable).

---

## Step 6 — Forward AD Signal Transformation

**Goal:** Implement the core `ForwardADSignalTransform` that converts signals to dual numbers `(primal, tangent)`.

### 6.1 New module: `crates/propagate/src/forward_ad.rs`

This is the heart of the implementation. It mirrors `compiler/transform/forwardADSignalTransform.cpp`.

#### Core data structure

```rust
/// Dual number: (primal_signal, tangent_signal)
struct Dual {
    primal: SigId,
    tangent: SigId,
}

struct ForwardADTransform<'a> {
    arena: &'a mut TreeArena,
    diff_control: SigId,               // The UI control we differentiate w.r.t.
    cache: AHashMap<SigId, Dual>,      // Memoization (DAG preservation)
}
```

#### Transformation method

```rust
impl<'a> ForwardADTransform<'a> {
    fn transform(&mut self, sig: SigId) -> Dual {
        // Check memoization cache first (critical for DAG preservation)
        if let Some(cached) = self.cache.get(&sig) {
            return cached.clone();
        }
        let result = self.transform_uncached(sig);
        self.cache.insert(sig, result.clone());
        result
    }
}
```

#### Differentiation rules

| Signal | Primal | Tangent |
|--------|--------|---------|
| `Real(c)` / `Int(n)` | `c` | `0.0` |
| `Input(i)` | `input(i)` | `0.0` |
| `HSlider(ctrl)` / `VSlider(ctrl)` / `NumEntry(ctrl)` | `sig` | `1.0` if `sig == diff_control`, else `0.0` |
| `Button` / `Checkbox` | `sig` | `0.0` |
| `BinOp(Add, x, y)` | `x + y` | `x' + y'` |
| `BinOp(Sub, x, y)` | `x - y` | `x' - y'` |
| `BinOp(Mul, x, y)` | `x * y` | `x' * y + x * y'` (product rule) |
| `BinOp(Div, x, y)` | `x / y` | `(x' * y - x * y') / y^2` (quotient rule) |
| `Sin(u)` | `sin(u)` | `cos(u) * u'` |
| `Cos(u)` | `cos(u)` | `-sin(u) * u'` |
| `Tan(u)` | `tan(u)` | `(1/cos(u)^2) * u'` |
| `Exp(u)` | `exp(u)` | `exp(u) * u'` |
| `Log(u)` | `log(u)` | `(1/u) * u'` |
| `Log10(u)` | `log10(u)` | `(1/(u * log(10))) * u'` |
| `Sqrt(u)` | `sqrt(u)` | `(1/(2*sqrt(u))) * u'` |
| `Abs(u)` | `abs(u)` | `sign(u) * u'` where `sign(u) = u/abs(u)` |
| `Asin(u)` | `asin(u)` | `(1/sqrt(1-u^2)) * u'` |
| `Acos(u)` | `acos(u)` | `(-1/sqrt(1-u^2)) * u'` |
| `Atan(u)` | `atan(u)` | `(1/(1+u^2)) * u'` |
| `Pow(u, v)` | `pow(u,v)` | `pow(u,v) * (v' * log(u) + v * u' / u)` |
| `Min(u, v)` | `min(u,v)` | `select2(u < v, u', v')` |
| `Max(u, v)` | `max(u,v)` | `select2(u > v, u', v')` |
| `Delay1(u)` | `delay1(u)` | `delay1(u')` |
| `Delay(u, d)` | `delay(u, d)` | `delay(u', d) - d' * delay(u - delay1(u), d)` |
| `Rec(body)` | rec of primals | rec of tangents (separate recursive systems) |
| `Select2(c, x, y)` | `select2(c, x, y)` | `select2(c, x', y')` |
| `RdTbl(T, i)` | `rdtbl(T, i)` | `gradient(T, i) * i'` (finite differences) |
| `WrTbl(...)` | `wrtbl(...)` | `0.0` |
| `Attach(x, y)` | `attach(x, y)` | `x'` (pass through primary path) |
| `Enable(x, y)` | `enable(x, y)` | `x'` |
| `Control(x, y)` | `control(x, y)` | `x'` |
| Comparison/Bitwise/IntCast | `sig` | `0.0` (non-differentiable) |
| `VBargraph`/`HBargraph` | `sig` | `0.0` (side effects) |

### 6.2 Top-level entry point: `generate_fad_signals`

```rust
/// Applies forward-mode AD to a list of output signals.
///
/// For each output signal, collects all differentiable UI controls it depends on,
/// then runs one ForwardADTransform per control. The result is an expanded signal
/// list: [out1, d_out1/d_ctrl1, d_out1/d_ctrl2, ..., out2, d_out2/d_ctrl1, ...]
pub fn generate_fad_signals(
    arena: &mut TreeArena,
    outputs: &[SigId],
) -> Result<Vec<SigId>, PropagateError> {
    let mut result = Vec::new();

    for &out_sig in outputs {
        // Add original output
        result.push(out_sig);

        // Collect all differentiable controls
        let mut collector = ADControlCollector::new();
        collector.collect(arena, out_sig);

        // For each control, compute the derivative
        for &control in &collector.controls {
            let mut fad = ForwardADTransform::new(arena, control);
            let dual = fad.transform(out_sig);
            // Optional: simplify the tangent signal
            result.push(dual.tangent);
        }
    }

    Ok(result)
}
```

### 6.3 DAG Preservation (Expression Swell Prevention)

The memoization cache in `ForwardADTransform` is **critical**. When two signal paths share a sub-expression, the cache ensures the derivative is computed only once for that shared node. This makes the derivative graph size `O(N)` (linear in the original graph), not exponential.

The Rust `TreeArena` hash-consing already provides structural sharing for identical signal nodes. The `AHashMap<SigId, Dual>` cache provides semantic sharing for the AD transform itself.

### 6.4 Tests

**Unit tests for each differentiation rule:**
- `d/dx(x) = 1` where `x` is an hslider
- `d/dx(x * x) = 2 * x` (product rule)
- `d/dx(sin(x)) = cos(x)`
- `d/dx(a * x + b) = a` (linearity)
- `d/dx(delay1(x)) = delay1(d/dx(x))`

**Integration tests:**
- `fad(hslider("f", 440, 50, 2000, 1) : sin)` → 2 outputs (primal + tangent)
- `fad(hslider("f", 440, 50, 2000, 1) * hslider("g", 1, 0, 1, 0.01))` → 3 outputs (primal + 2 tangents)
- `fad(hslider("f [autodiff:false]", 440, 50, 2000, 1) * hslider("g", 1, 0, 1, 0.01))` → 2 outputs (1 control excluded)

---

## Step 7 — End-to-End Integration

### 7.1 Compiler pipeline

Ensure `FaustCompiler::compile_source_to_signals` correctly handles `fad(expr)` through the full pipeline: parse → eval → propagate (with FAD transform).

### 7.2 Signal preparation

The expanded signal list from `generate_fad_signals` feeds into the existing `prepare_signals_for_fir` pipeline. The derivative signals are standard signal trees (add, mul, sin, cos, delay1, etc.) and require no special handling in FIR compilation.

### 7.3 FAD inside a recursive branch (`fad` inside `~`)

When `fad(expr)` appears as the feedback branch of a `~` combinator (e.g. `+~(fad(*(g)))`),
the right branch produces FAD-expanded outputs (primal + tangents) during propagation. The
`Rec` combinator must handle this:

1. After propagating the right branch, split the output into:
   - **primal feedback** = first `right_arity.outputs` signals
   - **tangent extras** = remaining signals (FAD tangents)
2. Only feed the primal signals back to the left branch (preserving the recursion contract).
3. Include the tangent signals in the de Bruijn recursive group body, so they properly
   reference the recursive projections.
4. Expose the tangent signals as additional outputs of the `Rec`.

This ensures the recursion arity contract is preserved while FAD tangent information
propagates through the recursive structure.

**Implementation:** `crates/propagate/src/lib.rs` — `FlatNodeKind::Rec(left, right)` arm
in `propagate_inner`.

### 7.4 Corpus tests

Add test DSP files to `tests/corpus/`:

```
tests/corpus/fad_basic.dsp             — fad(hslider("f",440,50,2000,1) : sin)
tests/corpus/fad_product.dsp           — fad(hslider("f",1,0,10,0.1) * hslider("g",1,0,10,0.1))
tests/corpus/fad_delay.dsp             — fad(hslider("f",1,0,10,0.1) : @(128))
tests/corpus/fad_recursive.dsp         — fad(hslider("fb",0.5,0,1,0.01) : +~*(hslider("g",0.5,0,1,0.01)))
tests/corpus/fad_recursive_branch.dsp  — +~(fad(*(hslider("g",0.5,0,1,0.01)))) — fad inside feedback branch
tests/corpus/fad_autodiff_false.dsp    — fad(hslider("f [autodiff:false]",1,0,10,0.1) * hslider("g",1,0,10,0.1))
tests/corpus/rad_parse_only.dsp        — rad(hslider("f",1,0,10,0.1) : sin) — parse OK, propagate error
```

---

## Step 8 — Future Work (Out of Scope)

These items are deferred to later phases:

1. **Reverse-mode AD (`rad`)**: Implement `generate_rad_signals` using adjoint accumulation and tape-based backward pass. Requires `ReverseADSignalTransform` (adjoint builder) and `ADDependencyVisitor` (parent map).

2. **Algebraic simplification of derivatives**: Apply simplification rules to tangent signals (e.g., `0 * x → 0`, `1 * x → x`, `x + 0 → x`). The existing `simplify_signals_fastlane` may partially cover this.

3. **`boxSignal` bridge**: The C++ compiler uses `boxSignal(signal_tree)` to wrap individual signals back into box form. This is used by the eval stage to return AD results as a `boxPar` composition. In Rust, the propagate-level integration avoids this bridge — the signal expansion happens directly in `propagate_inner`, so `boxSignal` is **not needed** for the initial port.

4. **`[autodiff:false]` metadata**: Full metadata extraction from control labels. Can be initially stubbed to always return `true` (all controls differentiable).

---

## Implementation Order

| Phase | Deliverable | Crates touched | Dependencies |
|-------|------------|----------------|-------------|
| A | Box primitives + matcher | `boxes` | None |
| B | Parser tokens + grammar | `parser` | Phase A |
| C | Evaluator arms | `eval` | Phase A |
| D | Flat-box + propagation bridge | `propagate` | Phase A, C |
| E | AD control collector | `propagate` (or `transform`) | Signals crate |
| F | Forward AD transform | `propagate` (or new `autodiff` crate) | Phase D, E |
| G | Integration tests | `compiler`, `tests/` | All above |

Phases A, B, C can be done in parallel. D depends on A. E is independent. F depends on D and E. G is last.

---

## File Summary

| File | Action |
|------|--------|
| `crates/boxes/src/tags.rs` | Add 2 tag constants |
| `crates/boxes/src/matcher.rs` | Add 2 `BoxMatch` variants + match arms |
| `crates/boxes/src/builder.rs` | Add 2 builder methods |
| `crates/parser/src/grammar/faustlexer.l` | Add 2 lexer tokens |
| `crates/parser/src/grammar/faustparser.y` | Add 2 grammar rules + integrate into `primary` |
| `crates/eval/src/lib.rs` | Add 2 match arms in `eval_value_uncached` |
| `crates/propagate/src/lib.rs` | Add `FlatNodeKind::ForwardAD`/`ReverseAD`, flat-box validation, arity, propagation |
| `crates/propagate/src/ad_controls.rs` | **New file** — AD control collector |
| `crates/propagate/src/forward_ad.rs` | **New file** — Forward AD signal transformation |
| `tests/corpus/fad_*.dsp` | **New files** — Test programs |
