# Reverse-Mode AD (`rad`) Implementation Plan

**Date:** 2026-04-27
**Status:** design plan
**Scope:** add propagation support for `rad(expr, seeds)` in `faust-rs`, reusing
the existing box/eval/propagate architecture and the forward AD differentiable
subset where possible.

## 1. Current state

`faust-rs` currently supports forward-mode automatic differentiation through
`fad(expr, seed)`.

Important current properties:

- `fad(expr, seed)` is parsed as a two-child `ForwardAD` box.
- `seed` may have one or more outputs.
- propagated output layout is:

```text
[primal_0, d primal_0 / d seed_0, d primal_0 / d seed_1, ...,
 primal_1, d primal_1 / d seed_0, ...]
```

- the core signal transform is `crates/propagate/src/forward_ad.rs`.
- it operates directly on de Bruijn recursion nodes.
- recursive FAD has two modes in `crates/propagate/src/lib.rs`:
  - expand-after-Rec,
  - augmented-state Rec.
- `rad(expr)` is currently parsed and preserved through eval, but propagation
  always returns `PropagateError::UnsupportedBox { kind: "reversead", ... }`.

This plan keeps the successful FAD architecture:

```text
parse -> boxes -> eval -> propagate -> normalize -> transform -> fir -> backend
```

RAD should expand during `propagate` into ordinary signal nodes, so FIR and
backend codegen do not need a dedicated AD mode in the first implementation.

## 2. Goal

Add a first useful reverse-mode AD primitive:

```faust
rad(expr, seeds)
```

where:

- `expr` is the expression bundle to differentiate; it may have one or more
  outputs,
- `seeds` is a block whose outputs are the independent variables,
- the result is every primal output of `expr`, followed by one gradient per
  seed output.

Semantics:

```text
expr = [e0, e1, ..., eM]

rad(expr, (s0, s1, ..., sN))
  = [e0, e1, ..., eM,
     d sum(e0..eM) / d s0,
     d sum(e0..eM) / d s1,
     ...,
     d sum(e0..eM) / d sN]
```

Example:

```faust
x = hslider("x", 1, 0, 10, 0.01);
y = hslider("y", 2, 0, 10, 0.01);
loss = sin(x * y);

process = rad(loss, (x, y));
```

Expected output layout:

```text
[sin(x*y), cos(x*y)*y, cos(x*y)*x]
```

The first output is intentionally the primal, matching the `fad` mental model.
Users can discard it explicitly:

```faust
grads = rad(loss, (x, y)) : !, _, _;
```

Multi-output example:

```faust
expr = (x * y, sin(x));
process = rad(expr, (x, y));
```

Expected output layout:

```text
[x*y, sin(x), y + cos(x), x]
```

The gradients are those of `x*y + sin(x)`. This is equivalent to reverse-mode
with an implicit cotangent seed of `1` for every primal output. A later VJP API
can expose custom output cotangents when users need a weighted combination.

## 3. Non-goals for the first implementation

- No full reverse-through-time / BPTT for IIR state.
- No mutable table adjoints.
- No soundfile content adjoints.
- No custom vector-output cotangent API yet. Multi-output `expr` is supported
  with the implicit all-ones cotangent described above.
- No backend-level Enzyme/LLVM integration.
- No attempt to match a C++ RAD implementation; upstream C++ does not provide
  an equivalent production RAD path for the current explicit-seed Rust design.
- No automatic discovery of differentiable UI controls. `seeds` is explicit.

## 4. Phase 0 validation items

Before deep implementation, confirm the following for the RAD scope:

1. **Production path:** RAD is implemented at the signal-propagation boundary,
   not in a backend.
2. **Baseline corpus:** add a small RAD corpus and compare against:
   - FAD for small seed counts,
   - central finite differences for runtime checks.
3. **Global state:** no new global AD state. RAD state lives in one transform
   object and local maps keyed by `SigId`.
4. **TreeArena performance:** the reverse pass must preserve DAG sharing and
   remain linear in the visited active subgraph, except for explicit adjoint
   sum nodes.
5. **API lifecycle:** `rad(expr, seeds)` is the supported surface. Legacy
   `rad(expr)` must become a clear arity error or remain parse-only only during
   a short migration window.

## 5. Surface syntax and box layer

### 5.1 Parser

Change the grammar from:

```text
rad(expr)
```

to:

```text
rad(expr, seeds)
```

Implementation:

- update `crates/parser/src/grammar/faustparser.y`;
- keep the existing `RAUTODIFF` token;
- parse both arguments as `Argument`, mirroring the current `fad` rule shape.

Recommended rule shape:

```text
| RAUTODIFF LPAR Argument PAR Argument RPAR {
      crate::with_state(state, |state| state.node_builder().reverse_ad($3, $5))
  }
```

### 5.2 Boxes

Update `ReverseAD` from one child to two children:

```rust
ReverseAD(BoxId, BoxId) // expr, seeds
```

Files:

- `crates/boxes/src/builder.rs`
- `crates/boxes/src/matcher.rs`
- `crates/boxes/src/dump.rs` if reverse AD shape is dumped explicitly
- `crates/boxes/tests/core_api.rs`

Builder:

```rust
pub fn reverse_ad(&mut self, expr: BoxId, seeds: BoxId) -> BoxId {
    intern_tag(self.arena, BOX_REVERSE_AD_TAG, &[expr, seeds])
}
```

Compatibility:

- a one-child `BOXRAUTODIFF` should decode to `Unknown`, or to a dedicated
  legacy error if diagnostics need to be friendlier.

### 5.3 Eval

Update the eval arm:

```rust
BoxMatch::ReverseAD(expr, seeds) => {
    let expr_val = eval_value(arena, expr, env, loop_detector)?;
    let seeds_val = eval_value(arena, seeds, env, loop_detector)?;
    let expr_box = force_value_to_box(arena, expr_val, loop_detector)?;
    let seeds_box = force_value_to_box(arena, seeds_val, loop_detector)?;
    Ok(EvalValue::Box(BoxBuilder::new(arena).reverse_ad(expr_box, seeds_box)))
}
```

Both arguments are evaluated in the same lexical environment, like `fad`.

## 6. Arity contract

For `rad(expr, seeds)`:

- `expr.outputs` must be at least `1`.
- `seeds.outputs` must be at least `1`.
- `inputs = max(expr.inputs, seeds.inputs)`.
- `outputs = expr.outputs + seeds.outputs`.

Add propagate errors:

```rust
PropagateError::RadBodyArity {
    node: TreeId,
    outputs: usize,
}

PropagateError::RadSeedArity {
    node: TreeId,
    outputs: usize,
}
```

Rationale:

- the common reverse-mode use case is still a scalar loss to many parameters;
- supporting multi-output bodies now costs little if we define the initial
  cotangent as all ones;
- custom vector-output reverse mode needs an explicit cotangent input and
  should be a separate later primitive, e.g. `vjp(expr, cotangent, seeds)`.

## 7. Propagation integration

Add:

```rust
FlatNodeKind::ReverseAD {
    body: FlatBoxId,
    seeds: FlatBoxId,
}
```

Update:

- `validate_flat_box_recursive`
- `flat_node_kind`
- `contains_forward_ad` only needs to traverse RAD children; RAD itself should
  not influence FAD recursion classification.
- `box_arity_typed`
- `box_arity_wiring` if needed for future RAD-in-Rec handling
- `collect_ui_nodes` so UI controls inside both `body` and `seeds` are
  registered.

In `propagate_in_slot_env`:

```rust
FlatNodeKind::ReverseAD { body, seeds } => {
    let body_arity = box_arity_typed(arena, body, ctx.cache)?;
    let seed_arity = box_arity_typed(arena, seeds, ctx.cache)?;

    // enforce non-empty body and at least one seed output
    // propagate seeds on the shared input bus
    // propagate body on the shared input bus
    // call reverse_ad::generate_rad_signals(...)
}
```

Input wiring mirrors `fad`: `body` and `seeds` observe the same upstream bus;
the seed expression is not sequenced after the body.

## 8. Reverse AD algorithm

Add a new module:

```text
crates/propagate/src/reverse_ad.rs
```

Core entry point:

```rust
pub(super) fn generate_rad_signals(
    arena: &mut TreeArena,
    primals: &[SigId],
    seeds: &[SigId],
) -> Result<Vec<SigId>, PropagateError>
```

Output:

```text
[primal_0, primal_1, ..., adjoint(seed_0), adjoint(seed_1), ...]
```

### 8.1 Active subgraph collection

Build a reachable active DAG from every primal output, stopping at seed nodes:

- if `sig` is one of the seed `SigId`s, record it as a leaf and do not descend;
- otherwise traverse differentiable children according to the RAD rule table;
- preserve deterministic postorder.

Data structure:

```rust
struct ReverseADGraph {
    postorder: Vec<SigId>,
    visited: AHashSet<SigId>,
    seed_index: AHashMap<SigId, SmallVec<[usize; 2]>>,
}
```

Repeated seed lanes must be preserved exactly like FAD:

```faust
rad(loss, (x, x)) -> [loss, dloss/dx, dloss/dx]
```

### 8.2 Adjoint accumulation

Maintain:

```rust
adjoints: AHashMap<SigId, SigId>
```

Initialize:

```rust
for primal in primals {
    adjoints[primal] += 1.0
}
```

This is the implicit all-ones cotangent for a multi-output expression. Then
traverse `postorder` in reverse. For each node `y` with adjoint `y_bar`, emit
local transpose contributions:

```text
child_bar += y_bar * d y / d child
```

Use a helper:

```rust
fn add_adjoint(
    arena: &mut TreeArena,
    adjoints: &mut AHashMap<SigId, SigId>,
    target: SigId,
    contribution: SigId,
)
```

`add_adjoint` should build `old + contribution` when an adjoint already exists.
Later simplification can fold zeros and shared terms.

### 8.3 Seed extraction

After the reverse sweep:

```rust
result = primals.to_vec();
for seed in seeds {
    result.push(adjoints.get(seed).copied().unwrap_or(zero));
}
```

For repeated seed lanes, push the same computed adjoint once per lane.

## 9. Initial RAD rule table

The first RAD slice should match the current reliable FAD subset as closely as
possible.

### 9.1 Leaves

| Node | Reverse behavior |
|---|---|
| seed `s` | stop descent; final adjoint is output gradient |
| `int`, `real`, `input`, UI controls not selected as seeds | no children |
| `button`, `checkbox` | no children |

### 9.2 Binary arithmetic

For `y = x + z`:

```text
x_bar += y_bar
z_bar += y_bar
```

For `y = x - z`:

```text
x_bar += y_bar
z_bar += -y_bar
```

For `y = x * z`:

```text
x_bar += y_bar * z
z_bar += y_bar * x
```

For `y = x / z`:

```text
x_bar += y_bar / z
z_bar += y_bar * (0 - x / (z*z))
```

For `rem`, `fmod`, `remainder`, reuse the same derivative model as FAD:

```text
x_bar += y_bar
z_bar += y_bar * (0 - floor_or_round(x/z))
```

Comparisons, shifts, bitwise ops:

```text
no adjoint contribution
```

### 9.3 Unary operations

For `y = f(x)`:

```text
x_bar += y_bar * f_prime(x)
```

Use the same derivative formulas already documented in `forward_ad.rs` for:

- `sin`, `cos`, `tan`
- `exp`, `log`, `log10`
- `sqrt`, `abs`
- `acos`, `asin`, `atan`
- `floor`, `ceil`, `rint`, `round`: zero contribution for the first slice

### 9.4 Binary math

`pow(x, z)`:

```text
x_bar += y_bar * pow(x,z) * z / x
z_bar += y_bar * pow(x,z) * log(x)
```

`atan2(y, x)`:

```text
y_bar += out_bar * x / (x*x + y*y)
x_bar += out_bar * (0 - y) / (x*x + y*y)
```

`min` / `max`:

- route adjoint to the selected primal branch with `select2`;
- equality keeps the existing branch convention, not a mathematical
  subgradient promise.

### 9.5 Control-flow and structural nodes

`select2(cond, a, b)`:

```text
a_bar += select2(cond, 0, out_bar) // depending on select2 argument order
b_bar += select2(cond, out_bar, 0)
cond receives no adjoint
```

The exact argument order must match `SigBuilder::select2` and existing FAD
behavior before implementation.

`prefix(init, x)`:

- first slice: forward adjoint to both `init` and `x` using the same structural
  model as FAD;
- add runtime tests before considering this stable for recursive optimizers.

`attach`, `enable`, `control`, `Output`:

- propagate adjoint only through the signal-carrying operand, mirroring FAD.

Bargraphs:

- no adjoint contribution through the meter sink.

### 9.6 Tables

For read-only `rdtbl(T, idx)`:

```text
idx_bar += out_bar * ((rdtbl(T, idx + 1) - rdtbl(T, idx - 1)) / 2)
```

Table payload receives no adjoint in the first slice.

Mutable tables and soundfiles:

- no adjoint contribution;
- preserve primal.

### 9.7 Foreign functions

Support the same unary `FFun` names as FAD:

- `tanh`
- `sinh`
- `cosh`
- `atanh`
- `asinh`
- `acosh`

Unrecognized or non-unary `FFun`:

- preserve primal;
- no adjoint contribution.

## 10. Recursion and time

This is the main design risk.

### 10.1 First slice: instantaneous symbolic reverse

The first implementation should allow RAD through recursive signal *structure*
only when the local transpose remains causal in the generated signal graph.

Recommended first policy:

- support seeds that are ordinary controls or feed-forward expressions;
- support recursive primals only where existing FAD-equivalent checks validate
  the result against FAD or finite differences;
- reject or zero-adjoint cases where a transpose would require future samples.

### 10.2 Delay and recursion policy

Forward delay rule is causal:

```text
d delay1(x) / dp = delay1(x')
```

Reverse transpose of a delay is non-causal in an infinite stream:

```text
adj_x[n] += adj_y[n+1]
```

That requires either:

1. a finite block tape and backward scan, or
2. a causal approximation that is explicitly not exact reverse mode.

For phase 1:

- `delay1` and recursive feedback should be handled conservatively;
- if exact transpose would be non-causal, emit a clear unsupported diagnostic
  for RAD rather than silently producing a misleading gradient;
- allow an experimental local approximation only behind a named mode later.

For the first exact LTI-recursive activation (phase E1), the finite window is
defined by Faust's current block size rather than by a new source-level
argument:

- horizon = current `compute(count, ...)` block length, i.e. the runtime value
  exposed in Faust libraries as `ma.BS`;
- terminal adjoint state at the end of the block = zero;
- no persistent adjoint state crosses block boundaries;
- the gradient is therefore exact for the block-local objective and truncated
  at the block boundary for longer stream objectives.

This keeps `rad(expr, seeds)` usable for the LTI-recursive subset without an
implicit fixed latency such as 1024 samples. If a future user needs a longer
or model-specific horizon, that remains a separate BPTT/block extension rather
than the default E1 semantics.

### 10.3 Future BPTT mode

A later `rad` extension may support:

```faust
rad(expr, seeds, horizon)
```

or a compiler option:

```text
-rad-horizon N
```

That mode would require:

- runtime tape storage for primal intermediates,
- reverse-time compute over a finite buffer,
- backend support for block-local backward sweeps,
- explicit latency and memory documentation.

This is out of scope for the first signal-level implementation.

## 11. Interaction with FAD recursion modes

RAD should not reuse `RecFadMode` blindly.

Add a separate classifier only if RAD is allowed inside `Rec`:

```rust
enum RecRadMode {
    None,
    UnsupportedTemporal,
    LocalAcyclic,
}
```

Initial recommendation:

- do not support local consumption of `rad(...)` inside `~` until feed-forward
  RAD is stable;
- add explicit tests that `rad` inside recursion returns a clear diagnostic;
- then add carefully scoped support for local acyclic seeds.

## 12. Diagnostics

Add structured diagnostics for:

- legacy one-argument `rad(expr)`,
- zero-output RAD body,
- zero-output seeds,
- unsupported temporal transpose,
- unsupported signal family in strict RAD mode.

Default policy should be stricter than FAD:

- FAD's zero-tangent fallback is useful because it preserves compilation.
- RAD silently dropping a path can hide a missing gradient and break training.

Recommendation:

- phase 1 has a `strict` internal policy by default for unimplemented
  differentiable-looking nodes;
- known discrete nodes still contribute zero with documentation.

## 13. Tests

### 13.1 Parser / boxes / eval

- parse `rad(sin(x), x)`;
- reject or diagnose legacy `rad(sin(x))`;
- box matcher round-trip for two-child `ReverseAD`;
- eval preserves both evaluated children.

### 13.2 Propagate structural tests

Add tests in `crates/propagate/tests/core_api.rs`:

1. `rad(x*x, x)` -> 2 outputs.
2. `rad(x*y, (x,y))` -> 3 outputs.
3. repeated seeds: `rad(x*y, (x,x))` -> duplicate gradient lanes.
4. absent seed: `rad(sin(x), y)` -> gradient zero.
5. multi-output body: `rad((x*y, sin(x)), (x,y))` -> primals first, then
   gradients of the sum.
6. zero-output body: `rad(environment, x)` -> `RadBodyArity`.
7. zero-output seed: `rad(x, environment)` -> `RadSeedArity`.

### 13.3 Runtime numeric tests

Compare RAD to FAD for small seed counts:

```faust
rad(expr, (a,b,c))
```

must match:

```faust
fad(expr, (a,b,c))
```

for scalar `expr`, lane by lane. For multi-output `expr`, RAD gradients must
match the sum of FAD tangent lanes across all primal outputs.

Representative cases:

- polynomial: `a*b + c`
- trig: `sin(a*b)`
- `pow`, `atan2`
- `min/max` away from equality
- read-only table index

Compare RAD to central finite differences through the interpreter fast lane:

- scalar UI controls,
- audio-input seed where valid,
- derived seed expression.

### 13.4 Corpus

Add initial corpus:

```text
tests/corpus/rad_basic.dsp
tests/corpus/rad_product_multi_seed.dsp
tests/corpus/rad_trig_composition.dsp
tests/corpus/rad_absent_seed.dsp
tests/corpus/rad_repeated_seed.dsp
tests/corpus/rad_multi_output_sum_cotangent.dsp
tests/corpus/rad_rdtbl_index_basic.dsp
tests/corpus/err_rad_zero_body.dsp
tests/corpus/err_rad_zero_seed.dsp
tests/corpus/err_rad_delay_temporal_unsupported.dsp
```

Keep `tests/corpus/rad_parse_only.dsp` only until the grammar migration is
complete; then replace it with a real two-argument RAD fixture.

## 14. Implementation phases

### Phase A - Surface and arity

Deliverables:

- two-child `ReverseAD` box shape;
- parser accepts `rad(expr, seeds)`;
- eval preserves both children;
- arity contract implemented;
- propagation still returns unsupported after arity checks.

Pass criteria:

- parser/boxes/eval tests pass;
- `rad(expr, seeds)` has deterministic arity;
- old `rad(expr)` produces a clear failure.

### Phase B - Feed-forward RAD core

Deliverables:

- `reverse_ad.rs`;
- active DAG collection;
- adjoint accumulation;
- rules for constants, seeds, arithmetic, unary math, `pow`, `atan2`,
  `min/max`, `select2`, float casts;
- output layout `[primals..., gradients...]`.

Pass criteria:

- RAD vs FAD parity tests pass for feed-forward scalar cases and for
  multi-output all-ones cotangent cases;
- no recursion or delay support claimed.

### Phase C - Extended primitive parity with FAD

Deliverables:

- read-only table index adjoint;
- unary `FFun` adjoints;
- pass-through structural nodes;
- documented zero/unsupported boundary.

Pass criteria:

- RAD vs FAD parity on all matching non-temporal FAD rules;
- central finite-difference runtime tests for table index cases.

### Phase D - Conservative temporal boundary

Deliverables:

- explicit diagnostics for unsupported temporal transpose;
- tests for delay/recursion rejection;
- documentation explaining why reverse delay is non-causal without a tape.

Pass criteria:

- no silent wrong gradient for delay/recursion cases;
- FAD recursive tests remain unchanged.

### Phase E - First recursive/local support, optional

Deliverables:

- scoped `RecRadMode` if a safe local acyclic subset is identified;
- tests against FAD/finite differences for that subset.

Pass criteria:

- no regression in existing recursive FAD corpus;
- RAD recursion support is documented as limited.

### Phase F - Future BPTT prototype, separate phase gate

Deliverables:

- design document for finite-horizon reverse-through-time;
- backend/runtime tape model;
- memory and latency estimates;
- representative adaptive DSP benchmark.

Pass criteria:

- not part of initial RAD merge.

## 15. File summary

Expected touched files:

```text
crates/boxes/src/builder.rs
crates/boxes/src/matcher.rs
crates/boxes/tests/core_api.rs

crates/parser/src/grammar/faustparser.y
crates/parser/tests/api_bridge.rs
crates/parser/tests/structural_cpp_differential.rs

crates/eval/src/lib.rs
crates/eval/tests/core_eval.rs

crates/propagate/src/lib.rs
crates/propagate/src/reverse_ad.rs
crates/propagate/tests/core_api.rs

crates/compiler/tests/signal_pipeline.rs
crates/compiler/tests/fad_recursive_runtime.rs

tests/corpus/rad_*.dsp
tests/corpus/err_rad_*.dsp

docs/fad-note-en.md
porting/faust-rs-supported-faust-subset-en.md
porting/journal/YYYY-MM-DD.md
```

## 16. Compatibility and mapping status

Mapping status: **adapted**.

Rationale:

- Rust already moved FAD to explicit seeds.
- RAD should follow the same explicit-seed model.
- C++ parity is not the primary constraint here because current C++ reference
  material only documents older signal-level symbolic differentiation and does
  not provide the planned `rad(expr, seeds)` semantics.

Compatibility impact:

- `rad(expr)` must change to `rad(expr, seeds)`.
- output arity changes from transparent/unsupported to
  `body_outputs + seed_outputs`.
- users get a stable primal-plus-gradient layout matching `fad`.

## 17. Key risks

1. **Silent missing gradients.**
   - Mitigation: strict diagnostics for unimplemented differentiable-looking
     families.

2. **Expression growth from adjoint sums.**
   - Mitigation: DAG sharing, CSE, and targeted simplification of zero/one
     adjoint terms.

3. **Temporal non-causality.**
   - Mitigation: reject exact reverse of delay/recursion in phase 1; document
     BPTT as a later backend/runtime feature.

4. **Seed identity surprises.**
   - Mitigation: same `SigId` equality contract as FAD; document that algebraic
     equivalents are not automatically matched.

5. **Branch/subgradient behavior.**
   - Mitigation: inherit FAD branch conventions and add tests away from
     discontinuities.

## 18. Success criteria

Initial RAD is ready when:

1. `rad(expr, seeds)` parses, evaluates, propagates, and reaches FIR/backend for
   feed-forward non-empty `expr`.
2. Output order is documented and tested:

```text
[expr_0, expr_1, ..., d sum(expr_i) / d seed_0, d sum(expr_i) / d seed_1, ...]
```

3. RAD gradients match FAD lanes for representative scalar cases, and match
   summed FAD lanes for representative multi-output cases.
4. RAD gradients match central finite differences on runtime smoke tests.
5. Delay/recursion cases either work in a documented subset or fail with a
   precise unsupported-temporal diagnostic.
6. Existing FAD tests and corpus remain green.

---

## 19. Feasibility analysis for stateful RAD

Phase 1 RAD refuses any signal family whose reverse transpose would be
non-causal: `delay1`, `delay`, `prefix`, recursion, projection over a
recursion. This section evaluates two complementary routes for lifting
that restriction, both of which are well established in the literature
but rest on different assumptions and pay different costs.

### 19.1 The two routes at a glance

| Route | Idea | Causal in time? | Memory | Restrictions |
|-------|------|-----------------|--------|--------------|
| **System transposition** (flow-graph reversal) | Replace the recursive subgraph with its adjoint network: arrows reverse, summers and branch points swap roles. | Single forward pass over a *time-reversed* signal block | None beyond the input block | Subgraph must be LTI. Time-varying or nonlinear feedback breaks the transposition identity. |
| **(T)BPTT** (back-propagation through time) | Materialize a finite tape of primal intermediates over a horizon `K`, run a backward sweep over the unrolled graph. | No — anti-causal sweep over the tape | `O(K · |state|)` per recursive node | Bias proportional to `K`. Truncation can be unstable on long-memory filters. |
| **Hybrid** | Transpose the LTI part, BPTT only the nonlinear part of the feedback. | Mixed | `O(K · |nonlinear state|)` | Worth the engineering only when both parts coexist. |

The two routes are not mutually exclusive. They are the two
mathematically clean ways to define an adjoint for a stateful node, and
the best DSP autodiff papers (Yu & Fazekas 2024 on all-pole filters,
Frostig et al. 2021 on linearize-then-transpose) exploit one or the
other depending on the operator type.

### 19.2 Route A — system transposition

#### 19.2.1 Why it works

For any LTI signal flow graph `G`, **Tellegen's theorem** guarantees
that `G` is *interreciprocal* with its transpose `G^T` ([dsprelated:
Transposed Direct Forms](https://www.dsprelated.com/freebooks/filters/Transposed_Direct_Forms.html)).
Concretely:

- the SISO transfer function `H(z)` is preserved by the transformation,
- the *adjoint operator* mapping output cotangent to input adjoint is
  exactly `G^T` evaluated on a time-reversed input.

The transformation rules are local and structural ([Wikipedia:
Tellegen's theorem](https://en.wikipedia.org/wiki/Tellegen's_theorem)):

| Original construct | Transposed |
|--------------------|-----------|
| signal arrow `a → b` | reversed arrow `b → a` |
| branch point fanning out to `n` consumers | summing junction with `n` inputs |
| summing junction with `n` inputs | branch point with `n` consumers |
| `delay1(z⁻¹)` | `delay1(z⁻¹)` (the same — but "consumed" in reverse time) |
| input port | output port |
| output port | input port |
| LTI primitive `f` (gain, delay) | same `f` |
| nonlinear primitive `g` | **not preserved** — see §19.2.4 |

This is exactly the operational definition of reverse-mode AD on a
linear program. Frostig et al. ([arXiv:2105.09469](https://arxiv.org/abs/2105.09469))
show that "reverse-mode AD = forward-mode linearization followed by
transposition," and that the transposition rule is purely linear.
Faust's existing FAD pass already provides the linearization; the
remaining work is the transpose.

#### 19.2.2 Mapping to Faust's `~` operator

In the signal IR, `+ ~ *(p)` lowers to a `DEBRUIJNREC([+(in, *(p, ref(1)))])`
group with one back-edge (`DEBRUIJNREF(1)` = "the previous output"). The
LTI structure of the loop is fully exposed: the `+`, `*(p)`, and the
back-edge are all linear in the signal lane (they are also linear in
`p` only when `p` is a constant; see §19.2.4 below for the time-varying
case).

A transposition pass would rewrite the recursion as a new
`DEBRUIJNREC` whose body is the adjoint network: the back-edge becomes
a *forward* feed (read at frame n−1 from a tape of primal values), the
output adjoint enters where the primal output exited, and the input
adjoint emerges where the primal input entered.

The good news: the existing de-Bruijn rebuilder in
`crates/propagate/src/forward_ad.rs` already shows that a recursive
group can be replaced by another structurally compatible one without
breaking the rest of the pipeline. The transposed group has the same
arity contract.

The hard news: Faust does not, today, separate "primal lane" from
"primal-difference lane" inside a recursion the way Yu & Fazekas
([arXiv:2404.07970](https://arxiv.org/abs/2404.07970)) do for all-pole
filters. Their analytical gradient depends on rewriting the recurrence
as a non-recursive summation that the runtime evaluates in 30× less
time than naive BPTT — but the rewrite is filter-shape specific
(all-pole, biquad, …). A general transposition pass would need to
either:

- restrict to a recognized shape (`+ ~ *(linear_state_update)`), or
- accept the cost of running the transposed graph on a *time-reversed
  block*, which forces buffering anyway (see §19.2.5).

#### 19.2.3 What is needed in the Rust codebase

A transposition route would touch the following:

1. **A linearity classifier on signal subgraphs.** Walk the recursive
   body and confirm every operator is linear in the recursive variable.
   Coefficients can depend on UI controls (constants over a block),
   but cannot depend on the recursive output itself. This excludes
   `+ ~ tanh(...)`, common in nonlinear filters.
2. **A new module `transpose_ad.rs`** that mirrors the structure of
   `reverse_ad.rs` but runs the dual rule table:
   - branch ↔ summer,
   - input port ↔ output port,
   - `delay1` stays `delay1` but the lane direction is swapped,
   - linear coefficients pass through unchanged,
   - `LTI primitive` stays the same (`+`, `*` by constant).
3. **A new `RecRadMode::LinearTranspose`** alongside the strict
   refusal, classifying which recursive groups are eligible.
4. **A block-buffering convention** for the time-reversed evaluation.
   Phase E1 uses the current Faust compute block as that finite window:
   horizon = `count`/`ma.BS`, terminal adjoint state = zero, and no adjoint
   state persists across blocks. This is exact for a block-local loss and
   explicitly truncated at block boundaries for stream-long objectives.
   Longer horizons or persistent adjoint state are reserved for later
   BPTT/block modes with a documented user surface.

#### 19.2.4 Boundaries of the LTI assumption

The transposition identity collapses outside three guarantees:

- **Time-invariance.** A coefficient that varies sample-to-sample
  (e.g., a slider scanned at audio rate) breaks the convolution
  identity. The transpose only works for "frozen-coefficient" blocks.
  Faust's `[autodiff]` annotation approach could mark such coefficients
  as "treat as block-constant" — at the cost of a model bias.
- **Nonlinearity.** A `tanh`, a clipper, a saturating multiplier inside
  the feedback path is not linear in the recursive lane. Transposition
  fails. Yu & Fazekas's all-pole approach is purely linear; the more
  general DDSP literature ([Hayes et al. 2023, Frontiers in Signal Processing](https://www.frontiersin.org/journals/signal-processing/articles/10.3389/frsip.2023.1284100/full))
  resorts to TBPTT precisely because the loop is nonlinear.
- **Multi-output recursion** with non-trivial cross-coupling. The
  transpose of a MIMO LTI block is well-defined ([dsprelated:
  Transposed Direct Forms](https://www.dsprelated.com/freebooks/filters/Transposed_Direct_Forms.html)
  generalizes through Mason's gain formula), but the implementation is
  more invasive — every recursion slot becomes a new adjoint port.

#### 19.2.5 What this route enables

If `R = + ~ *(p)` (constant `p`):

```text
y[n]  = p · y[n-1] + x[n]                  // primal recurrence
y_bar[n] += x_bar[n]                        // local input adjoint feeds y_bar
x_bar[n] += y_bar[n]                        // chain rule via the adder
... but y_bar must propagate "backwards in time" through the *(p) loop:
y_bar[n-1] += p · y_bar[n]                  // exactly the transpose of *(p)
```

This is a causal forward pass on the *time-reversed* `y_bar` block —
which is non-causal in real time but causal when running over a finite
tape that is read in reverse. The cost is one extra pass over the
block; no per-frame state explosion.

For `p` time-varying, the same recurrence becomes
`y_bar[n-1] += p[n] · y_bar[n]`, which is still linear in `y_bar` but
no longer time-invariant. The transposition is still mathematically
correct (it is exactly the "linearize, then transpose" rule), but the
adjoint loop now reads `p[n]` from the same tape used for the primal
intermediates. This recovers the analytical gradient that Yu & Fazekas
exploit for time-varying all-pole filters.

### 19.3 Route B — back-propagation through time (BPTT)

#### 19.3.1 Principle

BPTT unrolls the recurrence over a finite horizon `K`, builds the
explicit DAG `[y[0], y[1], …, y[K−1]]`, and runs ordinary feed-forward
RAD on that DAG. The horizon is the only parameter.

For RNN training in machine learning ([Wikipedia: Backpropagation through time](https://en.wikipedia.org/wiki/Backpropagation_through_time)),
this is the standard approach. Truncated BPTT (TBPTT) processes the
sequence in windows of `k₂` steps, updating parameters every `k₁`
steps; bias is bounded by `k₂` and decays geometrically for stable
recurrences ([Aicher et al. 2020](https://proceedings.mlr.press/v115/aicher20a.html)).

#### 19.3.2 Cost at audio rates

For a Faust filter at 48 kHz:

| Horizon | Time covered | Memory per state slot (`f32`) | Notes |
|---------|--------------|-------------------------------|-------|
| 64 | 1.3 ms | 256 B | adequate for very-fast adaptive filters |
| 512 | 10.7 ms | 2 KiB | typical for adaptive equalisers |
| 4096 | 85.3 ms | 16 KiB | typical for adaptive feedback cancellation |
| 48000 | 1 s | 192 KiB | upper bound for short-form impulse response training |

Memory scales with `K · |state|`. For a 4-pole filter (4 state slots)
at K = 4096, that is 64 KiB per filter instance — affordable in batch
training, prohibitive for real-time inference. Truncation introduces
gradient bias but not instability if the filter itself is stable.

#### 19.3.3 What is needed in the Rust codebase

1. **A horizon parameter** on the RAD surface:

   ```faust
   process = rad(expr, seeds, 4096);  // explicit horizon
   ```

   or a compiler flag `-rad-horizon N` for whole-program defaults. The
   parser already accepts a 3-argument variant pattern for FAD-like
   wrappers; the box-layer change mirrors the existing 2-child shape.

2. **A tape allocation pass** in `transform/` or `fir/`: every primal
   intermediate that contributes to the adjoint must be retained for
   the next `K` frames. The simplest implementation is a circular
   buffer per intermediate; a smarter one identifies *checkpoints*
   (a la gradient checkpointing in deep learning) to trade compute for
   memory.

3. **A backward-sweep code-generation path** in the FIR layer: for
   every output frame, after the primal computes, run the unrolled
   adjoint over the last `K` frames. The control flow is a
   for-loop with index running backwards, which the FIR already
   supports for delay-line indexing.

4. **A new `RecRadMode::BPTT { horizon }`** classifier that triggers
   the transformation when a recursion is reachable from a `rad(...)`
   body. The classifier already exists in the FAD path
   (`RecFadMode`); the symmetric structure for RAD is mechanical.

5. **A documented stability story.** Truncation bias must be exposed
   in the user-facing diagnostic if the gradient norm at `n − K`
   relative to the gradient at `n` exceeds some threshold; otherwise
   users will silently train against a bad gradient.

#### 19.3.4 Backend implications

Unlike the transposition route, BPTT requires backend cooperation:

- the interp backend needs a tape opcode (read/write a sample at
  index `n − k`),
- the C/C++ backend needs to allocate the tape statically,
- the Cranelift JIT needs to lower the backward sweep — the existing
  `ARsh`/index-loading opcodes likely cover it.

This is the heaviest engineering investment of the three routes.

### 19.4 Hybrid route — linearize, transpose the linear part, BPTT the rest

For mixed nonlinear feedback (`+ ~ tanh(*(p))`), neither route stands
alone. The principled solution is the same as Frostig et al.
(linearize-then-transpose) applied locally:

1. Run the FAD linearization over the recursive body. The result is a
   pair `(primal, tangent)` per signal lane; the tangent recurrence
   is linear by construction.
2. Apply system transposition to the tangent recurrence (it is now
   guaranteed LTI in the recursive lane).
3. The nonlinear primal evaluation must still be unrolled and
   buffered — but only for `K` frames, and only for the *operands* of
   the nonlinearity, not its full state.

This recovers the right asymptotic memory cost for typical
adaptive-filter training: `O(K)` for the nonlinear taps and `O(1)`
extra for the linear part. For phase 1 we will not implement this —
but the architecture should leave room for it: the `RecRadMode` enum
needs a third variant beyond `LinearTranspose` and `BPTT`.

### 19.5 What other AD systems do

- **JAX / Dex** (Frostig et al. 2021) explicitly decompose RAD as
  linearize-then-transpose, which is the conceptual basis for §19.4.
- **PyTorch / TensorFlow DDSP** ([Hayes et al. 2023, review](https://www.frontiersin.org/journals/signal-processing/articles/10.3389/frsip.2023.1284100/full))
  default to TBPTT with horizons in the 512–4096 range; performance
  bottlenecks and instability with naive TBPTT are a recurring theme.
- **Yu & Fazekas 2024** ([arXiv:2404.07970](https://arxiv.org/abs/2404.07970))
  achieve up to 30× speedup over TBPTT on time-varying all-pole filters
  by deriving an analytic gradient via "unwound" recursion — i.e., the
  transposed system, evaluated over a finite block.
- **Faust upstream (PR #939)** ([grame-cncm/faust#939](https://github.com/grame-cncm/faust/pull/939))
  hit a wall on recursive AD even in forward mode: the author derived a
  symbolic expression for the derivative of the general recursive
  algorithm but did not land an implementation. This confirms that
  recursion is the well-known crux of any DSP AD system.

### 19.6 Recommended phasing for `faust-rs`

The existing `Phase E` and `Phase F` placeholders in §14 are correct
in spirit but underspecified. Concretely:

1. **Phase E0 — linearity classifier.** Land a pure read-only pass on
   recursive groups that classifies them as
   {`LinearLTI`, `LinearTimeVarying`, `Nonlinear`}. No new AD
   capability yet — this is the gating predicate for both transposition
   and BPTT and it is independently useful for the FAD recursion mode
   classifier.

   **Status 2026-04-28:** implemented as
   `crates/propagate/src/stateful_rad.rs` with Rustdoc and structural
   unit tests. The pass classifies `DEBRUIJNREC` groups only and keeps
   current `rad(...)` recursion/delay rejection unchanged. The same
   module now exposes the `RecRadMode` strategy gate:
   `LinearTranspose` for E1, `BlockLinearTimeVarying` for E2, and
   `BpttRequired` for phase F.
2. **Phase E1 — transposition for the LTI recursive subset.** Implement
   `RecRadMode::LinearTranspose` for groups classified as `LinearLTI`.
   The implementation lives in a new `transpose_ad.rs` module. Test
   surface: parity with FAD on the same recursive shapes (FAD already
   handles them via the de Bruijn rebuilder). Runtime convention:
   `rad(expr, seeds)` over an accepted LTI recursion uses the current Faust
   block size (`compute` count / `ma.BS`) as its reverse window, initializes
   the terminal adjoint state to zero at the end of that block, and does not
   carry adjoint state between blocks.
3. **Phase E2 — block-mode transposition for `LinearTimeVarying`.**
   Same structural pass as E1 but with the time-reversed read of the
   coefficient lane. Requires a documented latency budget (block
   horizon).
4. **Phase F — BPTT for `Nonlinear` recursions.** Heaviest phase.
   Requires:
   - surface change `rad(expr, seeds, horizon)` or
     `-rad-horizon N`,
   - tape allocation in `transform/` and `fir/`,
   - backend support in interp, C/C++, and Cranelift,
   - stability diagnostic for truncation bias.
5. **Phase G — hybrid.** Once E and F coexist, layer the
   linearize-then-transpose decomposition: linearize the nonlinearity,
   transpose the LTI tangent recurrence, BPTT only the nonlinear
   taps. Documented memory advantage `O(K · |nonlinear state|)`
   instead of `O(K · |total state|)`.

Phase E1 is the cheapest and the most rentable: the LTI feedback case
(`+ ~ *(constant)` and IIR biquads) already accounts for a large
fraction of meaningful adaptive-filter targets. Phase F is the
heaviest investment and the only path to differentiating nonlinear
feedback (Moog ladder, hyperbolic-tangent saturator in a feedback
loop, etc.).

### 19.7 Sources

- [Reverse-Mode Autodiff = Linearize + Transpose (Frostig et al. 2021)](https://arxiv.org/abs/2105.09469)
- [Transposed Direct Forms — Smith, *Introduction to Digital Filters*](https://www.dsprelated.com/freebooks/filters/Transposed_Direct_Forms.html)
- [Tellegen's theorem](https://en.wikipedia.org/wiki/Tellegen's_theorem)
- [Differentiable All-Pole Filters for Time-varying Audio Systems — Yu & Fazekas 2024](https://arxiv.org/abs/2404.07970)
- [A review of differentiable digital signal processing — Hayes et al. 2023](https://www.frontiersin.org/journals/signal-processing/articles/10.3389/frsip.2023.1284100/full)
- [Backpropagation through time](https://en.wikipedia.org/wiki/Backpropagation_through_time)
- [Adaptively Truncating Backpropagation Through Time — Aicher et al. 2020](https://proceedings.mlr.press/v115/aicher20a.html)
- [GSoC PR #939 — Automatic Differentiation in the Faust Compiler](https://github.com/grame-cncm/faust/pull/939)

## 20. Engineering plan for the LTI transposition path (route B), independent of BPTT

This section turns §19.2 into a concrete plan that activates LTI
recursive RAD through exact system transposition **without** committing
to BPTT. It is the "route B" of the user-facing recap: precise reverse
mode for the LTI subset, decoupled from the heavier nonlinear-feedback
work of phase F.

The architectural decision that makes B independent of F is that LTI
transposition needs only **two block-local capabilities** at runtime:

1. evaluate a recursive group with the iteration order reversed across
   one block,
2. snapshot the primal recurrence's intermediate values for that block
   so the host (or an aggregation pass) can build the seed adjoint
   `Σ y_bar[n+1] · y[n]` etc.

Both are strictly less than the requirements of BPTT (which additionally
needs a tape of *every* differentiable intermediate of the nonlinear
path). They are also strictly more than the existing forward-only
recursive evaluation in the FIR layer. §20.1–§20.4 below detail what to
build, in roughly increasing scope.

### 20.1 Already in place

- `crates/propagate/src/stateful_rad.rs`
  classifies `DEBRUIJNREC` groups as
  `LinearLti / LinearTimeVarying / Nonlinear` and maps them to the
  matching `RecRadMode` (phase E0).
- `crates/propagate/src/transpose_ad.rs`
  builds the **structurally correct** transposed `DEBRUIJNREC` group
  for a `LinearLti` body. The unit-test interpreter validates the
  numeric identity `p_bar = Σ y_bar[n+1] · y[n]` against the
  closed-form derivative on the canonical first-order recurrence.
- The `RadUnsupportedNode { kind: "recursive-linear-transpose" }`
  diagnostic already points users at this path explicitly, so the
  user-visible surface only needs to flip from "rejected" to
  "lowered".

The remaining work is the wiring that turns the scaffold into running
code.

### 20.2 Block convention (the surface contract)

Phase E1 must publish a fixed contract for **what is differentiated**:

- The compute block of size `count` (the standard Faust compute
  argument) is the reverse-mode horizon. The terminal adjoint at
  frame `count` is zero — there is no carry-over of adjoint state
  between blocks. This matches the journal entry of 2026-04-28 and the
  numeric oracle's boundary.
- The user-visible gradient is therefore an exact gradient of the
  block-local objective `J = Σ_{n=0..count-1} cotangent[n] · y[n]`
  (with `cotangent ≡ 1` for the implicit all-ones case). The
  conventional choice in DSP training pipelines is to call
  `compute(count, …)` in a loop where each call is its own training
  batch.
- A future flag `-rad-block-stride N` (or `rad(expr, seeds, stride=N)`)
  may decouple the gradient horizon from the audio block size; that
  extension is **not** part of E1 and is documented as a phase-E2
  follow-up.

This contract is independent of BPTT: nothing requires inter-block
state, no horizon parameter is exposed in phase E1, and the host
controls the effective horizon by choosing the compute block size.

### 20.3 Signal-IR: a `ReverseTimeRec` node

Add one new signal-IR node:

```rust
/// Recursive group whose body must be evaluated in reverse iteration
/// order across the current compute block. Body and projection
/// semantics are otherwise identical to `DEBRUIJNREC` /
/// `Proj(slot, group)`. The terminal adjoint state for the last
/// frame of the block is implicitly zero.
ReverseTimeRec(body: SigId)
```

Properties:

- **Same arity contract** as `DEBRUIJNREC`. The body lists `k` branches
  and is projected via the existing `Proj(slot, group)` syntax.
- **Same de-Bruijn back-edge semantics** (`DEBRUIJNREF(1)` reads the
  *next* frame's adjoint state, not the previous one — i.e., what
  `DEBRUIJNREF(1)` already means inside the transposed body when read
  in reverse time).
- **Single-block scope.** No state is preserved across `compute(count, …)`
  calls. This is what keeps the runtime requirements minimal.

The propagation arm becomes:

```rust
RecRadMode::LinearTranspose => {
    let transposed = transpose_lti_de_bruijn_rec_scaffold(arena, group)?;
    let reverse_rec = signals::reverse_time_rec(arena, transposed_body);
    // … wire the projections so seed adjoints land on the right slots
}
```

Everything downstream — `signal_prepare`, `signal_fir`, the backends —
needs one new lowering path for `ReverseTimeRec`.

### 20.4 FIR lowering

The FIR layer already lowers `DEBRUIJNREC` to a forward-time loop
backed by a recursion array. The minimal change for `ReverseTimeRec`
is **iteration-order inversion**:

1. **Block extraction.** The compute kernel is already split into a
   per-frame loop with index `i ∈ [0, count)`. For a
   `ReverseTimeRec`, that loop runs `i` from `count - 1` down to `0`.
2. **Recursion-array indexing.** The existing rotating-IOTA scheme
   (`fIOTA - 1 & mask` reads "previous frame") is replaced by the
   symmetric `fIOTA + 1 & mask` for the reverse loop, so
   `DEBRUIJNREF(1)` reads the *next-in-reverse-time* frame's adjoint.
3. **Boundary.** Before the reverse loop, the recursion array slots
   for frame `count` are zeroed. This realises the terminal adjoint
   convention from §20.2.
4. **Sample I/O.** A `ReverseTimeRec` group consumes its `input(i)`
   lanes and produces its outputs at the same per-frame index as the
   surrounding compute. The host therefore sees the adjoint at frame
   `n` aligned with the primal sample at frame `n` — no host-side
   reversal is required.

Implementation site:
- `crates/transform/src/signal_fir/module.rs` (top-level recursion
  lowering and IOTA wiring),
- `crates/transform/src/signal_fir/delay.rs` (the previous-frame
  read for `DEBRUIJNREF`),
- `crates/transform/src/signal_fir/planner.rs` (loop direction).

The change is local: every existing `DEBRUIJNREC` lowering already
reads through the `recursion_array_index(group, frame_index)` helper.
The reverse loop only flips one sign and zeros the terminal slots.

### 20.5 Mixed forward/reverse compute kernels

A `rad(...)` over an LTI recursive primal compiles to **both** a
forward primal computation and a reverse adjoint computation that
share the per-frame state of the original recursion. Two viable
schedules:

- **Schedule A — interleaved.** The compute kernel runs the forward
  loop first (filling the recursion array with primal samples), then
  the reverse loop on the transposed group reading those primals as
  block-local data. This requires the FIR lowering to coalesce
  forward and reverse loops on the same compute block. Memory cost:
  one rotating buffer of size `block_len` per state slot.

- **Schedule B — split.** The compute kernel exposes two entry
  points: `compute_primal(count, …)` and
  `compute_adjoint(count, …)`. The host orchestrates them. Lower
  runtime memory because the rotating buffer can be the host's,
  but breaks the "single `compute` call" assumption of every existing
  Faust integration.

Recommendation: **Schedule A** for E1, with a documented memory
overhead linear in the block length. Schedule B can be added later
behind a flag if the memory cost matters for embedded targets.

### 20.6 Backend coverage

The interp backend is the right first target because:

- it already has a structured opcode set with explicit indexed
  reads/writes on the real heap,
- adding a "reverse iteration" intrinsic to `compute_block` is a
  ~50-line change in `crates/codegen/src/backends/interp/executor.rs`,
- the existing FIR-to-interp serializer in `serial.rs` only needs
  one new opcode emission for `ReverseTimeRec` body bodies.

Cranelift and C/C++ backends follow once the interp path passes
parity tests:

- **Cranelift** (`crates/codegen/src/backends/cranelift/mod.rs`) needs
  one extra `compute_loop` lowering branch that emits a backward
  index counter. The existing IR already supports negative-stride
  loops via `iadd_imm(-1)`.
- **C/C++** (`crates/codegen/src/backends/c/`,
  `crates/codegen/src/backends/cpp/`) emit a `for (int i = count - 1;
  i >= 0; --i)` instead of the standard ascending loop for
  `ReverseTimeRec` bodies. The static structure is generated from the
  same FIR module, so the change is one new code path in the loop
  emitter.

Each backend gets a parity test against the interp E1 result.

### 20.7 Test surface

The existing E0/E1 unit tests
(`crates/propagate/src/transpose_ad.rs::tests`) validate the
numerical correctness of the structural transposition and need no
change. New tests:

- **End-to-end RAD parity for canonical LTI recursions.** A new
  `crates/compiler/tests/rad_lti_recursive_runtime.rs` compiles
  fixtures like `rad_lti_first_order.dsp` (`+ ~ *(p)`) and
  `rad_lti_biquad.dsp` (cross-coupled second-order), runs both the
  rad-compiled DSP and a hand-rolled FAD reference at matching seed
  values, and asserts gradient parity sample-by-sample within the
  block, modulo the boundary effect at the last frame.
- **Block-boundary effect tests.** Fixtures parametrised by block
  size confirm that the adjoint at frame `count - 1` is the
  cotangent itself (no future contribution) and that the adjoint at
  frame `0` matches the closed-form `Σ_{k=0..count-1} p^k`.
- **RAD-vs-FAD perf bench extension.** Add LTI recursive shapes to
  `examples/rad_vs_fad_perf.rs`. Expectation: RAD beats FAD when
  the seed count exceeds a small threshold (the asymptotic
  `O(M·N)` vs. `O(M+N)` advantage finally becomes visible because
  recursion shadows are not duplicated).
- **Backend parity goldens.** `xtask golden-gen-rust` regeneration
  for the new fixtures, plus the existing
  `cpp_signal_differential.rs` harness extended with a
  `ReverseTimeRec`-aware comparison.

### 20.8 Diagnostics and migration

When E1 lands:

- `RadUnsupportedNode { kind: "recursive-linear-transpose" }` becomes
  a successful lowering path — no diagnostic is emitted on the LTI
  recursive subset.
- `recursive-block-linear-time-varying` and `recursive-bptt-required`
  remain diagnostic-only and continue to refer to the future phase E2
  / phase F work.
- The supported-subset doc and the RAD usage guide must mention that
  recursive LTI primals now compile, with the block-local horizon
  contract called out explicitly so users do not assume cross-block
  gradient flow.
- A migration note in `docs/rad-note-en.md` documents the
  block-local boundary semantics. Existing
  `err_rad_delay_temporal_unsupported.dsp` stays — that fixture
  exercises a *non-recursive* delay, which is still phase-F territory.

### 20.9 Phase boundary with F

Phase E1 in this scope **does not depend on** phase F:

- E1 needs reverse iteration on a single block; F needs a tape of
  primal intermediates whose size is independent of any signal node.
- E1 only handles bodies whose state-transition is exactly affine in
  the recursive variables (the `LinearLti` classification).
- A nonlinear recursion (`+ ~ tanh(*(p))`) keeps raising
  `recursive-bptt-required` after E1 lands. The user-visible
  diagnostic and the codepath that produces it are unchanged.

The design is forward-compatible with the §19.4 hybrid: when phase F
adds a tape mechanism, the linearised tangent recurrence still
benefits from the E1 transposition lowering, and the only new wiring
is reading primal-intermediate samples from the F tape instead of the
block-local rotating buffer that E1 uses.

### 20.10 Cost estimate

- **Propagation glue** (replace the strict-failure path with a
  `transpose_lti_de_bruijn_rec_scaffold` call + `ReverseTimeRec`
  emission): half a day.
- **Signal-IR `ReverseTimeRec` node** (signals crate, matchers,
  printers, validators): half a day.
- **FIR lowering** (single backward-loop path, recursion-array
  indexing flip, terminal-zero pre-loop): one day.
- **Interp opcode + serializer**: one day.
- **Cranelift backend support**: one day.
- **C/C++ backend support**: one day.
- **End-to-end runtime tests + parity goldens**: one day.

Total: ~6 working days for an interp-only landing, ~9 working days
to cover all three backends. Phase F is not on the critical path.

### 20.10.1 Implementation status on 2026-05-05

Committed phase-E1 scaffolding now covers:

- `signals::SigBuilder::reverse_time_rec` plus `SigMatch::ReverseTimeRec`,
  with Rustdoc documenting block-local reverse iteration and terminal-zero
  adjoint semantics;
- signal preparation, `sigtype`, reduced promotion, and recursion carrier
  decoding for `Proj(slot, ReverseTimeRec(SYMREC(...)))`;
- interp `SimpleForLoop` lowering for reverse loops;
- a FIR path for pure `ReverseTimeRec` output bundles, selecting a reverse
  compute sample loop;
- compute-preamble resets for reverse recursion carriers, enforcing the
  terminal-zero boundary at every `compute()` call;
- split forward/reverse sample-loop scheduling for mixed bundles such as
  `[primals..., gradients...]`;
- `transpose_lti_de_bruijn_rec_with_cotangents`, which replaces the transposed
  scaffold's `input(i)` cotangent placeholders with explicit caller-supplied
  cotangent signals while preserving a plain `DEBRUIJNREC` for the later
  `ReverseTimeRec` wrapper.

Still deferred:

- `reverse_ad.rs` does not yet replace the `recursive-linear-transpose`
  diagnostic with transposed recursive adjoint emission;
- recursive seed-gradient routing for active LTI coefficients is still needed
  before user-visible `rad(LTI_recursive_primal, seeds)` can produce useful
  parameter gradients.

### 20.11 What this delivers

- `rad(LTI_recursive_primal, seeds)` compiles end-to-end in the
  feed-forward backends. No host orchestration is required: the
  compute kernel runs both the forward primal and the reverse
  adjoint on the same compute block.
- The user can train recursive linear filter coefficients (biquads,
  cross-coupled state-space, integrators) by gradient descent with
  the same loop pattern as `rad_gradient_descent`. The
  `rad_adaptive_notch` example becomes redundant in its host-fed
  delay form; a new example can show a true biquad notch with the
  delay line *inside* `rad(...)`.
- The phase-1 RAD efficiency claim (`O(M+N)` cost for `M` outputs
  and `N` seeds) extends to the LTI recursive subset, recovering
  the asymptotic advantage over FAD on shapes where it matters
  (high-order IIR with many adjustable coefficients).
- The plan §17 risks are unchanged: simplification of adjoint sums
  remains the long-tail concern, and nonlinear feedback remains
  refused with a precise diagnostic.
