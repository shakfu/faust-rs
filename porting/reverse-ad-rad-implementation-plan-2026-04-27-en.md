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
