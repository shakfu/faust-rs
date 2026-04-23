# FAD N-Lanes Unified Recursion — Refactor Plan

**Date:** 2026-04-23
**Scope:** Eliminate redundant primal recursions in the output of
`generate_fad_signals_multi`. A single `ForwardADTransform` instance
produces one interleaved `DEBRUIJNREC` carrying the primal and every
per-seed tangent in `1 + N` lanes, instead of one `DEBRUIJNREC` per seed
(each duplicating the primal).

**Motivation:**

For `process = fad((2 :+ ~ *(param1)), param1)` the current compiler
emits two recursion accumulators computing the same `y[n] = 2 +
param1·y[n-1]`:

```cpp
// primal (standalone)
float fRecCur90 = (fSlow0 * fRec90) + 2.0f;
output0[i0] = fRecCur90;
// FAD_REC interleaved — fRec98 re-computes the same y[n], never read externally
fRec98[0]   = (fSlow0 * fRec98[1]) + 2.0f;
fRec98_1[0] = fRec98[1] + fSlow0 * fRec98_1[1];
output1[i0] = fRec98_1[0];
```

The generated code is numerically correct — the two recursions compute
identical values from identical initial state — but wastes one DSP
state variable and one MAC per recursive node per seed. With `N` seeds
and `k` recursions in the primal, `k·N` primal copies are emitted where
`k` would suffice (`k` lanes, `N` tangent lanes per primal).

The redundancy comes from [`generate_fad_signals_multi`](../crates/propagate/src/forward_ad.rs)
pushing the *original* primal `p` alongside the tangent rows, instead of
reusing the primal slot already present inside the interleaved
`FAD_REC`. One transformer per seed builds its own `FAD_REC` with its
own private primal slot, and none of them share.

**Non-goals:**

- No change to the seed model (`sig == diff_seed` equality, DeBruijn
  lifting on REC entry, projection index rules).
- No change to the output ordering `[p, t_s0, t_s1, …, p2, t2_s0, …]`
  seen by downstream passes.
- No change to non-recursive primitive derivatives (arithmetic, trig,
  delays, select2, FFun table).
- No semantic deduplication of repeated seed outputs unless the result
  is re-expanded to preserve the observable output lane layout.

**Reference documents:**

- `porting/fad-debruijn-native-transform-plan-2026-04-21-en.md` — the
  current single-transformer, single-seed architecture.
- `porting/autodiff-forward-ad-port-plan-2026-04-13-en.md` — original
  FAD port plan.
- `crates/propagate/src/forward_ad.rs` — module docstring, §
  *Projection and recursive groups (de Bruijn form)*.

**C++ source (parity anchors):** none. The Faust C++ compiler runs one
FAD pass per seed and accepts the same duplication. This plan is a
Rust-side optimisation and does not have a C++ counterpart.

---

## Problem restated

Given `seeds = [s_0, s_1, … s_{N-1}]` and `outputs = [p_0, p_1, …]`,
the module today computes:

```
for each seed s_j:
    T_j = ForwardADTransform::new(s_j)
    tangent_rows[j] = [T_j.transform(p_i).tangent for p_i in outputs]

result = flatten( [p_i, tangent_rows[0][i], …, tangent_rows[N-1][i]]
                  for i in 0..outputs.len() )
```

Each `T_j` rewrites every enclosing `DEBRUIJNREC` into a fresh
interleaved `FAD_REC_j` with body `[primal, tangent_j]`. The
`result` list references the *original* `DEBRUIJNREC` through `p_i`
and N further `FAD_REC_j` through the tangent rows. Post
`de_bruijn_to_sym`, that is `1 + N` distinct `SYMREC` names per
original recursion.

We want `1` `SYMREC` per original recursion, with `1 + N` slots.

---

## Target layout

Single transformer, single pass. For each original `DEBRUIJNREC` with
body `[e_0, e_1, …, e_{k-1}]` the transform emits one new
`DEBRUIJNREC` whose body has length `k · (1 + N)`:

```
[ primal(e_0), tangent_{s_0}(e_0), …, tangent_{s_{N-1}}(e_0),
  primal(e_1), tangent_{s_0}(e_1), …, tangent_{s_{N-1}}(e_1),
  …
  primal(e_{k-1}), tangent_{s_0}(e_{k-1}), …, tangent_{s_{N-1}}(e_{k-1}) ]
```

A `Proj(i, group)` in the source maps to:

| Result lane | New slot |
|-------------|----------|
| primal of `e_i` | `i · (1 + N)` |
| tangent of `e_i` w.r.t. `s_j` | `i · (1 + N) + 1 + j` |

For `N = 1` this degenerates to the current `[p_0, t_0, p_1, t_1, …]`
interleaving.

`generate_fad_signals_multi` then pushes, for each original output
`p_i`:

```
result.push( fad.transform(p_i).primal );     // lane 0 of FAD_REC
for j in 0..N: result.push( fad.transform(p_i).tangents[j] );
```

No original `p_i` is pushed — all outputs are `Proj(…, FAD_REC)` onto
the unified recursion.

---

## Design

### Dual carrier with N tangents

```rust
struct DualN {
    primal: SigId,
    tangents: smallvec::SmallVec<[SigId; 2]>,   // one per seed, same order as seeds
}
```

Using `SmallVec` keeps the common single-seed case allocation-free. An
invariant check asserts `tangents.len() == self.seeds.len()` at every
carrier construction.

### Seed indexing

The single-seed transform currently uses one `SigId` equality check:

```rust
if sig == self.diff_seed { … }
```

The unified transformer must keep the same semantic rule, but should
avoid an accidental `O(N)` scan at every node visit when `N > 1`.

Recommended shape:

```rust
diff_seeds: Vec<SigId>
diff_seed_index: AHashMap<SigId, usize>
```

where `diff_seed_index` is rebuilt whenever the seeds are lifted on
`DEBRUIJNREC` entry. The expected workload still has small `N`, but the
lookup contract should remain explicit and not depend on that fact for
correctness or acceptable asymptotic behavior.

### Transformer

`ForwardADTransform` carries `seeds: &'a [SigId]` and
`cache: AHashMap<SigId, DualN>`. Every per-node rule that today emits
`Dual { primal, tangent }` is generalised:

- Constants / non-differentiable leaves: `tangents = [zero; N]`.
- Seed equality: the seed *index* `j` sets `tangents[j] = 1.0`, the
  others stay `0.0`. Seed recognition becomes `self.seed_index(sig)`.
- Linear rules (`add`, `sub`, `delay1`, `float_cast`, `prefix`,
  `select2`, `attach`, `enable`, `control`, `Output`, `Proj` without
  interleaving): map over `tangents` element-wise.
- Bilinear rules (`mul`, `div`, `pow`, `min`, `max`, `atan2`, `fmod`,
  `remainder`, `Rem`): the formulas stay the same but build one
  tangent per seed; intermediate primals are hoisted above the loop
  over seeds so the tangent loop is pure composition.
- Unary chain rules (`sin`, `cos`, `tan`, `exp`, `log`, `log10`,
  `sqrt`, `abs`, `acos`, `asin`, `atan`, and every `FFun` entry):
  factor the derivative scalar `f'(x)` once and multiply by each
  `tangents[j]`.

### DeBruijn seed lifting

`diff_seed` becomes `diff_seeds: Vec<SigId>`. On `DEBRUIJNREC` entry,
every seed is lifted:

```rust
let old_seeds = self.diff_seeds.clone();
self.diff_seeds = old_seeds.iter()
    .map(|&s| lift_de_bruijn(self.arena, s))
    .collect();
… recurse …
self.diff_seeds = old_seeds;
```

Allocation cost is `O(N)` per REC entry, identical to the current
single-seed cost times `N`.

If `diff_seed_index` is added, it must be rebuilt from the lifted seed
vector before the recursive descent starts, then restored with the
previous seed vector on exit.

### Projection rule

`BoundRec`:
- primal lane: `Proj(index * (1 + N), dual_group.primal)`
- tangent lane `j`: `Proj(index * (1 + N) + 1 + j, dual_group.tangents[j])`

`UnboundRef` (outer REC not entered by FAD):
- primal: `Proj(index, dual_group.primal)`, unchanged
- every tangent lane: zero

`Other`: identity on every lane.

### DEBRUIJNREC body rebuild

`transform_list(body)` returns `Vec<DualN>` of length `k`. The rebuilt
body flattens to length `k · (1 + N)` as specified in *Target layout*.
The cache placeholder installed before recursion becomes
`DualN { primal: sig, tangents: [sig; N] }` (same `sig` in every
tangent slot, identical to the current single-seed placeholder which
uses `sig` for the tangent field).

This placeholder invariant is subtle and must stay documented:

- it is only visible during the recursive descent that rebuilds the REC
  body;
- it exists solely to break the cycle while back-edges are being
  interned;
- no public result lane may observe it after `transform_list(body)`
  completes and the real interleaved `DEBRUIJNREC` node has replaced the
  temporary cache entry.

Any implementation should keep a focused regression test where a
back-edge reaches the recursive group before the body reconstruction is
complete, to prove that the final projections land on the rebuilt slots
rather than the placeholder shape.

### Duplicate seeds

The seed list supplied by `fad(expr, seed_box)` is observable through
output arity and lane order. If the seed box lowers to repeated `SigId`
values, the unified transform must preserve the same public layout as
today:

- either keep duplicate tangent lanes internally,
- or deduplicate for optimization but re-expand them in
  `generate_fad_signals_multi`.

The default recommendation is to keep duplicates in the internal lane
order for the first implementation, because it avoids introducing a new
equivalence/re-expansion layer in a parity-sensitive pass.

### Public entry point

```rust
pub(super) fn generate_fad_signals_multi(
    arena: &mut TreeArena,
    outputs: &[SigId],
    seeds: &[SigId],
) -> Result<Vec<SigId>, PropagateError> {
    if seeds.is_empty() { return Ok(outputs.to_vec()); }
    let mut fad = ForwardADTransform::new(arena, seeds);
    let duals: Vec<DualN> = outputs.iter().map(|&s| fad.transform(s)).collect();
    let mut result = Vec::with_capacity(outputs.len() * (1 + seeds.len()));
    for dual in duals {
        result.push(dual.primal);
        for t in &dual.tangents { result.push(*t); }
    }
    Ok(result)
}
```

---

## Steps

### Step 1 — Carrier + transformer skeleton

- Introduce `DualN` with `SmallVec`.
- Replace `diff_seed: SigId` by `diff_seeds: Vec<SigId>` on
  `ForwardADTransform`. Keep the struct private; the public boundary
  is still `generate_fad_signals_multi`.
- Rewrite the per-kind arms to build `tangents` as a vector (one per
  seed). Factor helper functions so each derivative rule is written
  once and the per-seed loop is driven by that helper, not duplicated.

**Exit criterion:** code compiles with `N = 1` behaviour preserved
bit-for-bit. Golden stdout hashes for single-seed tests unchanged.

### Step 2 — Unified DEBRUIJNREC body

- Interleave body to length `k · (1 + N)` (see *Target layout*).
- Update the `Proj` classification to the new slot arithmetic.
- Seed-lifting loop over every entry of `diff_seeds` on REC entry.

**Exit criterion:** `fad_recursive.dsp` (single seed) compiles to one
semantic recursion group, not one original process recursion plus one
AD-shadow recursion. Output values unchanged (runtime trace
comparison).

Backend note: "one recursion accumulator" here means one recursion
group per source recursion at the signal/FIR lowering boundary. A
backend is still free to materialize that group as several local arrays
or state slots during code emission; the structural win is that those
arrays all belong to the same unified group rather than duplicated
primal/tangent groups.

### Step 3 — Multi-seed collapse

- `generate_fad_signals_multi` instantiates one transformer with the
  full seed list and pushes primal + N tangents from a single
  `DualN`.
- Delete the per-seed transformer loop.

**Exit criterion:** `fad_multi_seed.dsp` and the four new fixtures
(Step 5) compile to one recursion accumulator per original primal
recursion, regardless of seed count.

### Step 4 — Non-regression sweep

- `cargo test -p propagate` and `cargo test -p compiler` green.
- Golden stdout hashes for every `fad_*.dsp` in `tests/corpus/`
  unchanged *except* for the fixtures where we intentionally reduced
  recursion count — those goldens need a one-shot refresh.
- Numeric FAD checks still match either:
  - closed-form recurrence expectations, or
  - central finite differences on the interpreter fast lane.
- `tests/runtime_corpus` FAD runtime comparisons still match reference.

### Step 5 — New corpus fixtures

Add, with expected-values comments matching the simplified layout:

- `fad_recursive_shared_primal.dsp` — minimal single-recursion,
  single-seed: asserts one `REC` in output.
- `fad_recursive_multi_seed_shared.dsp` — single recursion, two seeds:
  asserts one `REC` with three lanes.
- `fad_recursive_nested_shared.dsp` — two nested recursions, one seed:
  asserts two `REC`s (one per source recursion), not four.
- `fad_recursive_product_shared.dsp` — `a * b` with `a`, `b` two
  independent recursions: asserts two `REC`s, not four.

Each fixture carries the same mathematical check already used by
`fad_nested_on_recursive_seed.dsp`: every REC slot count in the
generated C++ is asserted by a structural test in `signal_pipeline`
tests.

This step should also add numeric checks, not just structural ones:

- self-recursive case: closed-form recurrence comparison;
- nested recursion: central finite differences;
- multi-output recursion: central finite differences per output;
- mutual recursion: central finite differences per output.

### Step 6 — Documentation

- Update the module docstring in `crates/propagate/src/forward_ad.rs`:
  rewrite the *Projection and recursive groups* section with the
  `1 + N` slot arithmetic and retire references to per-seed
  transformers.
- `JOURNAL.md` entry for the refactor with before/after examples from
  `fad_bug1.dsp`.
- Archive this plan ("SUPERSEDED" banner if a later decision reverts
  the refactor).

---

## Risks

- **Multi-recursion interaction.** A `DEBRUIJNREF` inside one REC body
  that targets an outer REC (level > `debruijn_depth`) already falls
  under `UnboundRef` → tangent zero. With `N` seeds the same rule
  applies for every lane. Covered by a dedicated fixture (outer loop
  enclosing an inner `fad`-entered REC).
- **Placeholder during back-edge resolution.** The cache placeholder
  uses `sig` as every tangent. For a back-edge hitting the REC itself,
  the real tangent slot replaces the placeholder after `transform_list`
  returns. The invariant is the same as in the single-seed case: no
  caller observes the placeholder outside the recursive descent.
- **Repeated seeds.** If the seed box lowers to `[s, s]`, the unified
  transform must still preserve the observable two-tangent layout unless
  a deliberate compatibility decision is taken and documented.
- **Success criterion drift.** Counting `fRec*` names in emitted C++ is
  not, by itself, a robust acceptance test. The authoritative criterion
  is one unified recursion group at the signal/FIR level plus unchanged
  runtime values.
- **Hash-consing collisions.** The new body list is longer (`k·(1+N)`
  vs `k`). Interning cost grows linearly with `N`. Benchmark on
  `auto_chorus_stereo_fad_host.dsp` to verify propagate latency stays
  within noise.
- **SmallVec inline capacity.** Chosen inline size `2` covers the
  current test corpus; seeds > 2 spill to the heap. This trades one
  allocation per `DualN` for large-seed workloads; acceptable because
  the carrier lives only during the propagate pass.

---

## Validation

- `cargo test -p propagate -p compiler` — green.
- `cargo run --bin faust-rs -- --dump-sig tests/corpus/fad_recursive.dsp`:
  one unified recursion group with `1 + N` slot arithmetic.
- `cargo run --bin faust-rs -- --dump-cpp tests/corpus/fad_recursive.dsp`:
  no duplicated AD-local primal shadow recursion for the same source REC.
- `cargo run --bin faust-rs -- --dump-cpp tests/corpus/fad_multi_seed.dsp`:
  one unified recursion group, three observable output lanes.
- Numeric comparison against finite differences or closed-form expected
  values on the dedicated recursive FAD fixtures.
- Runtime parity: existing `fad_gradient_host.dsp` numerical sweep
  unchanged to `1e-6` tolerance.
