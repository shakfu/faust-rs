# `fad(exp, x)` — Explicit Differentiation Variable Port Plan

**Date:** 2026-04-15
**Scope:** Replace the current `fad(exp)` primitive (differentiate `exp` against every
reachable differentiable UI control) by a new two-argument form `fad(exp, x)` where
`x` is a block with a single output that *names* the differentiation variable.
UI control discovery inside `exp` is no longer required. Only one tangent output is
produced per primal output.

**Supersedes (partially):**
- `porting/autodiff-forward-ad-port-plan-2026-04-13-en.md` — box/parser/eval/propagate
  pipeline is kept, but Steps 5 (control collector) and 6 (per-control loop) are
  reworked around an explicit variable.
- `porting/fad-ddsp-library-plan-2026-04-14-en.md` — training library still consumes
  the primal / tangent pair; the only change is that the user now picks which signal
  acts as the independent variable.

**Reference files in faust-rs:**
- `crates/boxes/src/{tags.rs,matcher.rs,builder.rs}` — `ForwardAD` box node
- `crates/parser/src/grammar/{faustlexer.l,faustparser.y}` — `FAUTODIFF` token
- `crates/eval/src/lib.rs` — `BoxMatch::ForwardAD` arm in `eval_value_uncached`
- `crates/propagate/src/lib.rs` — `FlatNodeKind::ForwardAD`, arity, `Rec` interaction
- `crates/propagate/src/forward_ad.rs` — `ADControlCollector`, `ForwardADTransform`,
  `generate_fad_signals`

---

## 1. Motivation

The existing `fad(exp)` implementation computes `d(exp)/d(ctrl)` for every UI control
reachable from `exp` (filtered by `[autodiff:false]`). This has several drawbacks:

- **Ambiguous output layout.** Output arity is `outputs * (1 + n_controls)` and
  depends on a recursive walk of the signal DAG; the user must sort controls by label
  to get a stable ordering.
- **Coupling to UI metadata.** The transform depends on the `ControlSpec` registry
  and on `[autodiff:false]` label parsing. That coupling forces `fad` to run *after*
  UI collection and makes it impossible to differentiate against a derived signal
  (e.g. `sin(hslider(...))`, a `bus`, an audio input).
- **Exponential seed enumeration.** Each extra control adds an independent tangent
  output even when the user only cares about one of them. Inside a `~` branch this
  compounds with the `suppress_fad` plumbing in `propagate_inner`.
- **Not aligned with standard forward-mode AD.** `jvp`-style APIs pick one tangent
  direction; `fad(exp)` is closer to a full Jacobian column enumeration.

The new form `fad(exp, x)` aligns with classic forward-mode AD: the user supplies a
block `x` that produces the single signal `v` to differentiate against. The transform
then computes `d(exp)/dv` treating `v` as the independent variable (seed 1) and every
other leaf as constant (seed 0). Output arity is simply `outputs * 2` — one primal
followed by one tangent per output.

---

## 2. Surface syntax and semantics

### 2.1 Syntax

```faust
fad(exp, x)
```

- `exp` — any Faust expression (same as today).
- `x` — a block whose **output arity is exactly 1**. There is *no* constraint on
  its input arity: `x` may be a pure 0-input block (`hslider(...)`, a literal,
  a derived constant), or a 1-input block (`_`, `abs`, `sin`, `*(k)`), or
  generally any `k → 1` transformer. `x` acts as the seed: we propagate `x` with
  the same upstream input bus as `exp` and use the resulting single signal as
  the "independent variable".

Parse / arity errors:
- `fad(exp)` — single-argument form is rejected (hard break with the old API; see
  §9 for the migration note).
- `fad(exp, x)` where `x` has output arity ≠ 1 after propagation is rejected in
  `box_arity_typed` with a dedicated
  `PropagateError::FadSeedArity { node, inputs, outputs }`.
- `x` is free to have any number of inputs; §6.4 describes how they are wired
  from the shared input bus.

### 2.2 Semantics

Let `[s_1, ..., s_n]` be the propagated signal list of `exp` (its `n = outputs`
primal signals) and `v` be the single propagated signal of `x`. The new output list
is the interleaved dual bundle:

```
[s_1, d s_1 / d v, s_2, d s_2 / d v, ..., s_n, d s_n / d v]
```

Differentiation rules are the same as today (see
`autodiff-forward-ad-port-plan-2026-04-13-en.md` §6.1 table). The only thing that
changes is the **seed rule**:

| Signal node | Old tangent rule | New tangent rule |
|---|---|---|
| `HSlider(c)` / `VSlider(c)` / `NumEntry(c)` | `1.0` if `c == diff_control`, else `0.0` | `0.0` (unless the node hash-cons-equals `v`, see below) |
| Any signal equal to `v` | — | `1.0` |
| Any other leaf | `0.0` | `0.0` |
| Structural nodes | chain rule | chain rule |

Because the `TreeArena` is hash-consing signals, "equal to `v`" is implemented as a
pointer comparison `sig == v`. The transform therefore *does not* need to inspect
what kind of node `v` is — it can be a slider, a constant, or a derived
signal like `sin(...)`, `_ : abs`, or `*(k)`. Since `x` is propagated with the
same upstream bus as `exp` (§6.4), any `Input(i)` the seed reads hash-cons-shares
its `SigId` with the identical `Input(i)` inside `exp`; the transform picks up
the seed automatically at the point where the two subgraphs meet.

### 2.3 Mathematical contract

- `fad(exp, x)` computes a **directional derivative** along the direction encoded by
  `v`: for every reachable signal `s` in `exp`, the tangent is `∂s/∂v`.
- If `v` does not appear anywhere inside `exp`'s signal graph, every tangent is
  `0.0`. This is a valid, non-error result (same as writing `fad(sin, hslider g)`
  today where `g` is not used by `sin`).
- `v` is treated as a *free variable* of the differentiation. Sub-expressions of
  `v` itself are **not** differentiated: if `v = a * b`, the rule is still
  `dv/dv = 1`, not `da/dv * b + a * db/dv`. This matches how `jvp` chooses a tangent
  direction at the leaves.
- Inside `~`, the existing `suppress_fad` mechanism is still needed (one tangent is
  enough to blow up a recursive group if expansion happens mid-wiring). See §6.

---

## 3. Box layer

### 3.1 `crates/boxes/src/tags.rs`

`BOX_FORWARD_AD_TAG` stays the same (`"BOXFAUTODIFF"`) but the arity of the tagged
node changes from 1 child to 2 children. Bump any compatibility constant that
carries the expected child count.

### 3.2 `crates/boxes/src/matcher.rs`

```rust
/// - `boxForwardAD(Tree exp, Tree x)`
/// - `isBoxForwardAD(Tree t, Tree& exp, Tree& x)`
ForwardAD(BoxId, BoxId),
```

The match arm reads two children:

```rust
BOX_FORWARD_AD_TAG => {
    let [exp, seed] = children.try_into().map_err(…)?;
    BoxMatch::ForwardAD(exp, seed)
}
```

Update all existing match-sites (arity inference, dump, tests) to destructure the
new pair. `BoxMatch::ReverseAD` stays single-argument for now; it is still a
parse-only stub.

### 3.3 `crates/boxes/src/builder.rs`

```rust
pub fn forward_ad(&mut self, expr: BoxId, seed: BoxId) -> BoxId {
    self.debug_assert_node_exists("boxForwardAD exp", expr);
    self.debug_assert_node_exists("boxForwardAD x",   seed);
    intern_tag(self.arena, BOX_FORWARD_AD_TAG, &[expr, seed])
}
```

### 3.4 Tests

- `crates/boxes/tests/core_api.rs`:
  - Update the `ForwardAD` round-trip test to build `forward_ad(wire, hslider_box)`
    and verify both children are recovered.
  - Add a negative test asserting that the old single-child shape no longer decodes.

---

## 4. Parser layer

### 4.1 `crates/parser/src/grammar/faustparser.y`

Replace the current rule:

```
    | FAUTODIFF LPAR Expression RPAR {
          crate::with_state(state, |state| state.node_builder().forward_ad($3))
      }
```

with a two-argument form:

```
    | FAUTODIFF LPAR Expression COMMA Expression RPAR {
          crate::with_state(state, |state| state.node_builder().forward_ad($3, $5))
      }
```

No change to `faustlexer.l` (the `FAUTODIFF` token and `"fad"` keyword stay).

### 4.2 Parser tests

- `crates/parser/tests/api_bridge.rs`:
  - Update `parses_fad_wraps_expression` (or equivalent) to parse
    `fad(hslider("f", 440, 50, 2000, 0.01) : sin, hslider("f", 440, 50, 2000, 0.01))`
    and assert both children round-trip through `BoxMatch::ForwardAD`.
  - Add a new test `parses_fad_requires_two_arguments` that asserts `fad(sin)`
    produces a parser error with a message mentioning the new arity.
- `crates/parser/tests/structural_cpp_differential.rs`:
  - The current C++ differential fixture must be regenerated or the assertion
    relaxed: upstream C++ Faust still uses `fad(exp)`, so the structural comparison
    for `fad` nodes is intentionally skipped behind a feature gate
    `skip_fad_shape_parity` until C++ adopts the same form. Document the skip.

---

## 5. Evaluator layer

### 5.1 `crates/eval/src/lib.rs`

Update the `BoxMatch::ForwardAD` arm in `eval_value_uncached`:

```rust
BoxMatch::ForwardAD(exp, seed) => {
    let exp_val  = eval_value(arena, exp,  env, loop_detector)?;
    let seed_val = eval_value(arena, seed, env, loop_detector)?;
    let exp_box  = force_value_to_box(arena, exp_val,  loop_detector)?;
    let seed_box = force_value_to_box(arena, seed_val, loop_detector)?;
    let mut bld = BoxBuilder::new(arena);
    Ok(EvalValue::Box(bld.forward_ad(exp_box, seed_box)))
}
```

Both children are evaluated independently in the current environment. The seed
`x` is *not* evaluated inside `exp`'s scope — it is a sibling block, so variables
of `x` resolve against the same enclosing `env` `exp` sees. This matches the user
expectation that writing `fad(exp, x)` where `x` is a named definition in scope
differentiates w.r.t. the same `x` that `exp` is free to reference.

### 5.2 Arity inference

`infer_box_arity` currently says:

```rust
BoxMatch::ForwardAD(inner) | BoxMatch::ReverseAD(inner) => infer_box_arity(arena, inner),
```

Split it:

```rust
BoxMatch::ForwardAD(exp, seed) => {
    // Sanity-check the seed so errors surface at the eval layer with a
    // clear message; the authoritative check runs in propagate.
    let seed_arity = infer_box_arity(arena, seed)?;
    if seed_arity.outputs != 1 {
        return Err(EvalError::FadSeedShape { seed_arity });
    }
    let exp_arity = infer_box_arity(arena, exp)?;
    Ok(BoxArity {
        // `exp` and `x` share the same upstream bus (§6.4), so the outer
        // `fad` exposes as many inputs as the most demanding child.
        inputs:  exp_arity.inputs.max(seed_arity.inputs),
        outputs: exp_arity.outputs * 2,
    })
}
BoxMatch::ReverseAD(inner) => infer_box_arity(arena, inner),
```

Adding `EvalError::FadSeedShape { seed_arity: BoxArity }` requires touching the
eval error enum and its diagnostic lowering. Doing the check here as well as in
propagate costs very little and lets `fad`-based macros in the standard library
fail early with a better error message.

### 5.3 Tests

- `crates/eval/tests/core_eval.rs::eval_process_preserves_forward_ad_wrapper_around_evaluated_body`:
  rename + rewrite to assert that both children of `ForwardAD` are re-wrapped
  around their evaluated bodies.
- New test: `eval_process_rejects_fad_seed_with_wrong_output_arity` — builds
  `fad(sin, par(hslider, hslider))` and expects `FadSeedShape { outputs: 2, .. }`.
- New test: `eval_process_accepts_fad_seed_with_inputs` — builds
  `fad(_ : sin : *(_), _ : abs)` (seed = `_ : abs`, i.e. 1 input, 1 output) and
  verifies no `FadSeedShape` error is raised.

---

## 6. Propagate layer

### 6.1 `FlatNodeKind::ForwardAD`

Replace the single-body variant:

```rust
ForwardAD { body: FlatBoxId },
```

with a pair:

```rust
ForwardAD { body: FlatBoxId, seed: FlatBoxId },
```

`try_build_flat_box` now recurses into both children:

```rust
BoxMatch::ForwardAD(body, seed) => Ok(FlatNodeKind::ForwardAD {
    body: try_build_flat_box(arena, body)?,
    seed: try_build_flat_box(arena, seed)?,
}),
```

`contains_forward_ad` walks both branches (the seed is unlikely to itself contain a
`fad`, but we do not short-circuit — nested `fad` must remain explicit and
detectable so recursive-branch handling stays correct).

### 6.2 Arity (`box_arity_typed`, `box_arity_wiring`)

```rust
FlatNodeKind::ForwardAD { body, seed } => {
    let seed_arity = box_arity_typed(arena, seed, cache)?;
    if seed_arity.outputs != 1 {
        return Err(PropagateError::FadSeedArity {
            node: box_tree.as_tree_id(),
            inputs: seed_arity.inputs,
            outputs: seed_arity.outputs,
        });
    }
    let inner = box_arity_typed(arena, body, cache)?;
    Ok(BoxArity {
        // `body` and `seed` are driven by the same shared bus; see §6.4.
        inputs:  inner.inputs.max(seed_arity.inputs),
        outputs: inner.outputs * 2,
    })
}
```

`box_arity_wiring` stays transparent: it still returns the inner `body` arity so
that `Rec` sees the untangled output count (the dual expansion happens after the
recursive group is built, as before).

**Rec interaction.** The `contains_forward_ad` helper used inside
`FlatNodeKind::Rec` must now walk *both* `body` and `seed`, but the tangent count
in the `has_fad` branch collapses from `n_controls` to just `1`. The extra-outputs
formula becomes:

```rust
let outputs = if has_fad {
    // One tangent per primal output, regardless of how many `fad(...)` nodes
    // the Rec contains, because each fad fixes a single direction.
    //
    // Multiple distinct fad() nodes inside the same Rec with different seeds
    // still expand to disjoint dual bundles, but count_ad_tangents below
    // sums them.
    core_outputs + count_ad_tangents(arena, left)?
                 + count_ad_tangents(arena, right)?
} else {
    core_outputs
};
```

Where `count_ad_tangents` replaces `count_ad_controls`: for each `ForwardAD { body, .. }`
reachable from the branch, add `box_arity_wiring(body).outputs` (one tangent per
primal output of that inner body). The old `count_ad_controls` is dead and can be
removed.

### 6.3 UI collection (`collect_ui_nodes`)

`ForwardAD { body, seed }` still recurses into both: UI controls defined inside
the seed (e.g. `fad(..., hslider("lr",0,0,1,0.01))`) must be registered as regular
controls in the DSP's UI. The current single-branch recursion merely becomes a
two-branch recursion.

### 6.4 `propagate_inner`

```rust
FlatNodeKind::ForwardAD { body, seed } => {
    // `body` and `seed` share the caller's upstream input bus.  Each child
    // reads as many leading signals as its own input arity requires; extra
    // inputs beyond the longer of the two are routed to `body` only.  This
    // mirrors the way `metadata(...)` and similar transparent wrappers
    // consume the shared bus: the seed is *not* a sequenced branch but a
    // side observation of the same signals `body` sees.
    let seed_arity = box_arity_typed(arena, seed, ctx.cache)?;
    let seed_inputs: Vec<SigId> = inputs
        .iter()
        .copied()
        .take(seed_arity.inputs)
        .collect();
    let seed_sigs = propagate_in_slot_env(arena, seed, &seed_inputs, ctx)?;
    let [seed_sig]: [SigId; 1] = seed_sigs.try_into().map_err(|got: Vec<_>| {
        PropagateError::FadSeedArity {
            node: box_tree.as_tree_id(),
            inputs: seed_arity.inputs,
            outputs: got.len(),
        }
    })?;

    // Propagate the body with the caller's inputs, exactly like today.
    let body_sigs = propagate_in_slot_env(arena, body, inputs, ctx)?;

    if ctx.suppress_fad {
        // Inside a Rec branch we defer the expansion.  The Rec arm will call
        // generate_fad_signals after the recursive group is built.  We still
        // have to surface `seed_sig` to that future call: store it on a
        // per-node side table keyed by the FlatBoxId so the Rec arm can look
        // it up when it iterates its pending FAD sites.
        ctx.pending_fad_seeds.push((box_tree, seed_sig));
        return Ok(body_sigs);
    }

    generate_fad_signals(arena, &body_sigs, seed_sig)
}
```

`ctx.pending_fad_seeds` is a new `Vec<(FlatBoxId, SigId)>` field on
`PropagateContext`. The Rec arm drains it after building the recursive group and
calls `generate_fad_signals` once per (body, seed) pair; because each `fad(...)` is
lexically unique inside a branch, the draining order is deterministic. If a Rec
branch contains zero `fad` nodes the vector stays empty and this path is inert.

### 6.5 New error variants

- `PropagateError::FadSeedArity { node, inputs, outputs }` — triggered by the
  static arity check (`box_arity_typed`) and the dynamic check after
  `propagate_in_slot_env`.
- Corresponding `DiagnosticCode::FadSeedArity` for the error report layer.
- Update `tests/fixtures/` (or whatever holds expected error snapshots) to cover
  both cases.

---

## 7. Forward AD transform

`crates/propagate/src/forward_ad.rs` is the core of the plan's simplification.

### 7.1 Delete the control collector

- Remove `ADControlCollector`, `is_autodiff_enabled`, `sort_controls_by_label`.
- Remove the dependency on `ControlSpec` and `ctx.ui_controls` from this module.
- `[autodiff:false]` metadata becomes irrelevant for this form: if the user does
  not want a control differentiated, they do not pass it as `x`. Leave the
  metadata parsing elsewhere (UI layer) untouched, but stop consuming it here.
  Document the semantic shift in the crate-level module doc.

### 7.2 Rework `ForwardADTransform`

Replace `diff_control: ControlId` with `diff_seed: SigId`:

```rust
struct ForwardADTransform<'a> {
    arena: &'a mut TreeArena,
    diff_seed: SigId,
    cache: AHashMap<SigId, Dual>,
}
```

The seeding rule at the top of `transform_uncached` short-circuits on pointer
equality *before* pattern-matching:

```rust
fn transform_uncached(&mut self, sig: SigId) -> Dual {
    if sig == self.diff_seed {
        return Dual {
            primal: sig,
            tangent: SigBuilder::new(self.arena).real(1.0),
        };
    }
    // ... existing rules, with one change:
    //     HSlider/VSlider/NumEntry/Input/Const now all return tangent = 0
    //     unconditionally.  The equality check above is the single source
    //     of seed truth.
    // ...
}
```

Implications:

- `Input(_)`, `Real(_)`, `Int(_)`, `HSlider`, `VSlider`, `NumEntry`, `Button`,
  `Checkbox`, `FConst`, `FVar`, `Waveform`, bargraphs all return
  `tangent = 0.0` unconditionally. The seed check runs first, so a seed that
  happens to be a slider still gets `1.0`.
- Chain-rule arms (BinOp, Sin, Cos, …) are unchanged: they call `self.transform`
  on children and compose the tangent with `SigBuilder`.
- `Rec` / `sym_rec` handling is unchanged: we still build a parallel recursive
  group for tangents so that feedback through the seed works correctly. The
  only difference is that we now build **one** parallel group instead of
  `n_controls` of them, which simplifies the `fad_var` naming (the
  fresh recursion variable can be a single suffix `FAD_tan` on top of the
  body's recursion variable).
- Memoization cache semantics unchanged.

### 7.3 Rework `generate_fad_signals`

```rust
pub(super) fn generate_fad_signals(
    arena: &mut TreeArena,
    outputs: &[SigId],
    diff_seed: SigId,
) -> Result<Vec<SigId>, PropagateError> {
    let converted_outputs: Vec<SigId> = outputs
        .iter()
        .copied()
        .map(|sig| de_bruijn_to_sym(arena, sig).unwrap_or(sig))
        .collect();

    let converted_seed = de_bruijn_to_sym(arena, diff_seed).unwrap_or(diff_seed);

    let mut fad = ForwardADTransform::new(arena, converted_seed);
    let mut result = Vec::with_capacity(converted_outputs.len() * 2);
    for out_sig in converted_outputs {
        let dual = fad.transform(out_sig);
        result.push(dual.primal);
        result.push(dual.tangent);
    }
    Ok(result)
}
```

Notable points:

- **One transform per `fad(..., x)` node** (not one per control). The cache
  amortizes shared sub-expressions across all primal outputs of the same body.
- **No sort, no label-based ordering.** The output layout is
  `[primal_1, tangent_1, primal_2, tangent_2, ...]` which makes downstream
  post-processing (e.g. the `ddsp.lib` training skill) trivially stable.
- **De Bruijn conversion of the seed** mirrors the conversion of each output:
  when `fad` is used inside a `Rec`, the seed signal must be expressed in the
  same symbolic recursion form as the body signals so that pointer equality
  still holds after canonicalization. If the seed references the recursion
  itself we effectively end up differentiating against a recursive variable,
  which is still mathematically well-defined (directional derivative along the
  steady-state direction of that projection).

### 7.4 Tests

Add `crates/propagate/tests/forward_ad_explicit_seed.rs`:

1. **Seed is a slider used inside the body.**
   `fad(x*x, x)` with `x = hslider("x",0,-1,1,0.01)` → two outputs:
   - primal: `x*x`
   - tangent: `2*x` (after simplification; raw form is `x*1 + 1*x`).
2. **Seed is a derived signal.**
   `fad(sin(y), y)` with `y = a * b` where `a, b` are sliders. Expected tangent:
   `cos(a*b)`, i.e. the subtree `a*b` inside `exp` matches `y` and receives
   seed = 1 *without* recursing into `a` and `b`.
3. **Seed absent from body.**
   `fad(sin(u), v)` where `u` and `v` are unrelated sliders → tangent = 0 for
   every output.
4. **Seed with one input (audio input wire).**
   `fad(_ : sin, _)` — seed is `_` (1 → 1, identity on the audio bus). Expected
   tangent = `cos(input_0)` (directional derivative along the audio input).
   This is the clearest evidence that the new API no longer depends on UI
   controls *and* that seeds with inputs are first-class.
4b. **Seed with one input (transformer).**
   `fad(_ : abs : sin, _ : abs)` — seed is `_ : abs`. Expected tangent =
   `cos(abs(input_0))` because the `abs(input_0)` subtree inside `exp` hash-cons-
   matches the propagated seed signal and receives seed = 1 without further
   recursion into `abs` or its input.
5. **Seed = constant.**
   `fad(x*x, 1.0)` — pathological but legal: tangent collapses to zero because
   the seed `1.0` does not equal the slider `x` and the chain rule terminates
   at every leaf. Document this as the "identity of tangent" case.
6. **Multiple `fad` inside `Rec`.**
   `+~( fad(_ * g, g) )` with `g` a slider: check that `suppress_fad` still
   defers the expansion until after the recursive group is built and that the
   final arity is `1 (primal) + 1 (tangent) = 2` outputs.
7. **Nested `fad`.**
   `fad(fad(x*x, x), x)` — the inner `fad` expands first, producing a 2-output
   body that the outer `fad` differentiates again. Expected outer result: 4
   signals `[x*x, 2x, 2x, 2]` (primal, tangent, tangent-of-primal,
   tangent-of-tangent). This confirms the layered semantics.

---

## 8. Corpus and integration tests

Rename / rewrite the existing corpus files:

```
tests/corpus/fad_basic.dsp             → fad(hslider("f",440,50,2000,1):sin, hslider("f",440,50,2000,1))
tests/corpus/fad_product.dsp           → fad(hslider("f",1,0,10,0.1) * hslider("g",1,0,10,0.1), hslider("f",1,0,10,0.1))
tests/corpus/fad_delay.dsp             → fad(hslider("f",1,0,10,0.1):@(128), hslider("f",1,0,10,0.1))
tests/corpus/fad_recursive.dsp         → fad(hslider("fb",0.5,0,1,0.01) : +~*(hslider("g",0.5,0,1,0.01)), hslider("fb",0.5,0,1,0.01))
tests/corpus/fad_recursive_branch.dsp  → +~(fad(*(hslider("g",0.5,0,1,0.01)), hslider("g",0.5,0,1,0.01)))
tests/corpus/fad_derived_seed.dsp      → new: fad(sin(x+y), x+y) where x,y are sliders
tests/corpus/fad_audio_seed.dsp        → new: fad(_ : sin, _)
tests/corpus/fad_autodiff_false.dsp    → deleted (the metadata is no longer consulted)
tests/corpus/rad_parse_only.dsp        → unchanged (rad still single-argument, stub)
```

The FFI / codegen crates (`crates/codegen/src/backends/interp/compiler.rs`,
`crates/codegen/src/backends/cranelift/mod.rs`) do not need to change: they already
consume the expanded signal list returned by propagation. The only effect on them is
that the `outputs` count they see is `body_outputs * 2` instead of
`body_outputs * (1 + n_controls)`.

---

## 9. Migration & breaking-change policy

This is a breaking change for any existing `.dsp` file that uses `fad(exp)`.
Because the feature is new (merged in 2026-04 only) and not yet shipped in a
tagged release, we follow a **hard cutover**:

1. Land this plan as one PR that updates parser, box layer, eval, propagate,
   tests, and corpus in lockstep. No transitional feature flag.
2. Add a JOURNAL entry (`porting/JOURNAL.md`, English) dated 2026-04-15 describing
   the cutover and pointing at this plan.
3. Update `porting/autodiff-forward-ad-port-plan-2026-04-13-en.md` with a banner
   at the top stating that the `fad(exp)` form documented there is superseded by
   the two-argument form in this plan.
4. Update `porting/fad-ddsp-library-plan-2026-04-14-en.md` §2 to reflect that
   `fad(expr)` becomes `fad(expr, x)` and that "automatic detection of
   hslider/vslider/numentry" is removed from the table row.
5. If a C++ parity test fails because upstream Faust still uses `fad(exp)`, gate
   the assertion behind `skip_fad_shape_parity` (see §4.2) rather than
   reintroducing a single-argument fallback.

No grace period, no `#[deprecated]` shim, no `fad_v2` alias. The old form is
simply removed.

---

## 10. Work breakdown

| Phase | Deliverable | Crates touched | Depends on |
|-------|-------------|----------------|------------|
| A | Box layer: 2-child `ForwardAD`, builder, matcher, tests | `boxes` | — |
| B | Parser: grammar rule + tests + error cases | `parser` | A |
| C | Evaluator: new arm, `EvalError::FadSeedShape`, tests | `eval` | A |
| D | Propagate flat layer: `FlatNodeKind` change, arity, UI collection, Rec extras | `propagate` | A |
| E | Forward AD transform rewrite (seed = SigId, no collector) | `propagate` | D |
| F | `generate_fad_signals` signature change + call sites | `propagate` | E |
| G | Corpus rewrite + integration tests | `propagate`, `tests/` | D, E, F |
| H | JOURNAL entry + plan banners + doc updates | `porting/` | G |

Phases A, B, C can land in parallel once A is reviewed. D depends on A. E and F are
two commits in the same crate but can be reviewed as one PR to avoid a broken
intermediate state (the `ADControlCollector` deletion must coincide with the
`generate_fad_signals` signature change). G and H close the cutover.

---

## 11. Non-goals

- **Reverse-mode `rad`.** Still parse-only, still stubbed in propagate. The
  single-argument form is unchanged.
- **Higher-order derivatives via library macros.** The nested-`fad` test (§7.4
  test 6) covers the compiler side; macros like `fad2(exp, x) = fad(fad(exp, x), x)`
  can live in `ddsp.lib` and are out of scope here.
- **Mixed seeds.** `fad(exp, (x, y))` with a two-output seed block is **not**
  supported. The seed arity is strictly 1. Users who want Jacobians can wrap
  several `fad` calls in `par(...)`. This keeps the semantics of "directional
  derivative" unambiguous.
- **UI metadata `[autodiff:false]`.** No longer consulted. If a library wants to
  re-introduce it as a *policy* (e.g. forbid passing a `[autodiff:false]`-tagged
  slider as the seed), that is a separate, lint-style plan.
