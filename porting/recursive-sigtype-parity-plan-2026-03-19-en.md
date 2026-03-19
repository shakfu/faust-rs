# Plan — Recursive `sigtype` Parity Refactor vs C++ `typeAnnotation/updateRecTypes`

Date: 2026-03-19

## Goal

Replace the current adapted Rust recursive typing mechanism with a structure
that follows the C++ `sigtyperules.cpp` algorithm closely enough to remove the
current class of regressions:
- incomplete `SigId -> SigType` maps after recursive retyping,
- non-terminating or over-widening recursive interval updates,
- promotion failures caused by relying on a canonical type map that is not
  stable under recursive invalidation,
- repeated fast-lane regressions on real corpus cases (`goertzelOpt`,
  `rep_38_sine_phasor`, `zita_min`, `cubic_distortion`, and similar).

This is a parity-first refactor, not a local bugfix pass.

## C++ Reference

Primary file:
- `/Users/letz/Developpements/RUST/faust/compiler/signals/sigtyperules.cpp`

Relevant entry points and helpers:
- `typeAnnotation(Tree sig, bool causality)`
- `updateRecTypes(...)`
- `initialRecType(...)`
- `maximalRecType(...)`
- `inferRecType(...)`
- `checkRecType(...)`

Important globals/defaults used by the algorithm:
- `gGlobal->TREC`
- `gGlobal->TRECMAX`
- `gGlobal->gNarrowingLimit`
- `gGlobal->gWideningLimit`

## Problem Statement

The current Rust implementation in `crates/sigtype/src/rules.rs` is still
structurally different from C++:

- Rust drives recursion from a memoized per-node `infer(sig)` function.
- Recursive refinement is implemented by local subtree invalidation and
  re-inference inside `infer_sym_rec(...)`.
- `TypeAnnotator::annotate(...)` returns a clone of this mutable memo table and
  downstream passes assume that map is complete and stable.

That design is weaker than the C++ algorithm because the C++ compiler does not
solve recursion by mutating a per-node memo table in place. Instead it:

1. discovers all recursive groups globally,
2. keeps explicit vectors for recursive group definitions and current types,
3. updates those vectors via repeated `updateRecTypes(...)` passes,
4. only treats ordinary subtree typing as a consumer of the current group
   approximation.

The result is that Rust currently mixes two models:
- global recursive reasoning is approximated,
- per-node memoization remains the main control structure.

This mismatch explains the recurring regressions.

## Current Rust Weaknesses

### 1. Recursive convergence is encoded in the wrong place

`infer_sym_rec(...)` currently owns convergence, widening, and invalidation.
That is too local relative to C++, where recursive groups are solved by a
top-level driver (`typeAnnotation`).

### 2. Canonical type maps are not guaranteed to be closed

Because the memo table is mutated during recursive retyping, the final
`HashMap<SigId, SigType>` can miss reachable subtrees. Promotion then fails on
perfectly valid nodes such as:
- `SIGBINOP(mul, SIGHSLIDER(...), float_bits(...))`
- `SYMREF(Wn)`

### 3. Widening is still adapted rather than orchestrated

Rust now imitates some of the interval behaviour (`TREC`, `TRECMAX`,
immediate widening), but it still does so inside local node inference instead
of the vector-based recursive-group driver used by C++.

### 4. Promotion now depends on an unstable contract

Moving the fast-lane promotion into `normalize` was the right architectural
step, but it exposed the fact that recursive typing is not yet robust enough to
serve as the single canonical source of truth.

## Target Architecture

Port the recursive part of `typeAnnotation(...)` as a dedicated global driver.

### Rust target shape

Add a recursive typing phase in `crates/sigtype` with explicit state similar to:

```rust
struct RecGroup {
    rec_sig: SigId,
    body_list: SigId,
    arity: usize,
}

struct RecTypingState {
    groups: Vec<RecGroup>,
    current: Vec<SigType>,
    upper: Vec<SigType>,
    age_min: Vec<Vec<i32>>,
    age_max: Vec<Vec<i32>>,
}
```

Then split typing into two layers:

1. `annotate(outputs)`:
   - discover recursive groups globally,
   - run the recursive-group fixpoint/widening driver,
   - expose the final group approximations to subtree inference.

2. `infer(sig, env)`:
   - ordinary structural typing using the already-computed recursive-group
     approximations,
   - no local recursive invalidation loop.

## Deliverables

### D1. Global recursive-group discovery

Create a Rust equivalent of the C++ recursive-group collection used by
`typeAnnotation(...)`.

Requirements:
- deterministic ordering,
- one record per `SYMREC` group,
- arity extracted from the body list,
- no duplicate groups when the same recursive group is shared in the DAG.

Likely file:
- `crates/sigtype/src/rules.rs`

Optional extraction if it grows:
- `crates/sigtype/src/recursion.rs`

### D2. Explicit `updateRecTypes` port

Implement a Rust function that mirrors the C++ structure:

```rust
fn update_rec_types(
    state: &mut RecTypingState,
    inter: bool,
    arena: &TreeArena,
    ui: &UiProgram,
) -> Result<(), TypeError>
```

Semantics to preserve:
- set current recursive approximations before typing recursive bodies,
- infer each recursive body list using those approximations,
- update intervals by intersection (`inter = true`) or reunion (`inter = false`),
- preserve non-interval type coordinates from the newly inferred body.

### D3. Narrowing + widening policy parity

Port the C++ control flow, not just the final interval effect:
- initialize `upper` with `maximalRecType`,
- run `gNarrowingLimit` passes with `inter = true`,
- run the widening loop with `inter = false`,
- maintain `age_min` / `age_max`,
- widen to the `upper` bounds only when the configured limit is exceeded.

Note: workspace defaults currently match C++ defaults (`0`), but the Rust
implementation should still carry the full mechanism explicitly.

### D4. Final canonical annotation closure

After recursive groups have converged, ensure the returned map contains types
for all reachable nodes in the prepared forest.

This should be guaranteed structurally, not repaired later by promotion.

Success criterion:
- no promotion fallback should be needed for missing canonical types on valid
  reachable signals.

### D5. Remove the local recursive invalidation strategy

Delete the current local mechanism from `infer_sym_rec(...)`:
- subtree invalidation,
- local fixpoint loop,
- local widening helper used only because the algorithm is node-driven.

Replace it with:
- lookup into solved recursive-group approximations,
- or a small scoped helper used only by the global recursive driver.

## Migration Plan

### Phase A — Introduce the new recursive driver alongside the current one

1. Add recursive-group discovery helpers.
2. Add `RecTypingState`.
3. Implement `initial_rec_type` / `maximal_rec_type` in the new driver.
4. Implement `update_rec_types(..., inter)` without removing the old path yet.
5. Add tests that compare the old and new driver on simple recursion groups.

Exit condition:
- new recursive driver compiles and matches current behaviour on existing unit
  tests for simple integer/real recursion.

### Phase B — Switch `annotate(...)` to the global recursive driver

1. `annotate(outputs)` discovers all recursive groups first.
2. It computes final recursive approximations with the new driver.
3. Ordinary subtree typing reads those solved approximations.
4. `infer_sym_rec(...)` becomes a thin access path instead of a local solver.

Exit condition:
- `annotate(...)` produces a complete canonical type map without local
  invalidation.

### Phase C — Remove promotion-side fallbacks introduced by incompleteness

Once the canonical map is stable:
- simplify `normalize::SignalPromoter` again,
- remove fallback shape typing added only to survive missing canonical entries,
- keep only trivial literal fast paths if still useful.

Exit condition:
- `promote_signals_fastlane(...)` depends only on canonical `SigType` results,
  not on ad hoc shape reconstruction.

## Validation Matrix

### Unit tests in `sigtype`

Add dedicated recursive tests for:
- `x = x + c` style widening (`rep_38` class),
- integer-preserving recursion (`min`, `abs`),
- multi-output recursion groups,
- projection typing over recursive groups,
- stability of returned type maps (all reachable children typed).

### `transform` tests

Keep or expand:
- `prepare_signals_for_fir_keeps_integer_recursive_min_feedback_int`
- `prepare_signals_for_fir_keeps_integer_recursive_abs_feedback_int`
- `recursive_fixpoint_recomputes_body_types_after_real_widening`
- `prepare_signals_for_fir_uses_foreign_function_return_type`

Add:
- a regression test that asserts no missing canonical type during promotion for
  a slider-times-constant binop,
- a regression test that asserts no missing canonical type for `SYMREF` inside
  recursive preparation.

### `compiler` tests

Mandatory cases:
- `cargo test -p compiler --test signal_fir_lane -- --nocapture`
- `cargo test -p compiler --test zita_pipeline -- --nocapture`

Corpus-level smoke cases:
- `tests/corpus/rep_38_sine_phasor.dsp`
- `tests/corpus/rep_55_sine_phasor_echo_feedback.dsp`
- `/Users/letz/Developpements/faust/tests/impulse-tests/dsp/cubic_distortion.dsp`
- `/Users/letz/Developpements/faustlibraries/tests/analyzers_tests.dsp` with `-pn goertzelOpt_test`

Final gate:
- `cargo fmt --all`
- `cargo test`

## Non-Goals

This refactor does **not** attempt in the same step to:
- redesign the entire `SigType` public API,
- merge all of `normalize` and `sigtype`,
- port unrelated C++ analyses from `sigtyperules.cpp` (IIR gain, extra
  diagnostics, causality branches) unless they block recursive parity.

## Risks

### Risk 1 — ordering drift of recursive groups

If Rust group discovery order differs from C++, tests may pass structurally but
 still drift on naming, diagnostics, or subtle projection mapping.

Mitigation:
- deterministic traversal,
- dedicated structural tests on multi-group recursion fixtures.

### Risk 2 — partial dual-path period

Keeping old and new recursive logic alive simultaneously for too long would
increase confusion.

Mitigation:
- short overlap period,
- delete the old local solver as soon as the new driver is green.

### Risk 3 — promotion fallback hides remaining typing bugs

The temporary promotion fallback for missing canonical types can mask progress.

Mitigation:
- track it explicitly as temporary,
- remove it in Phase C,
- add a test that the canonical map is complete for targeted regression
  fixtures.

## Success Criteria

The refactor is complete when all of the following are true:

1. `TypeAnnotator::annotate(...)` no longer relies on local recursive subtree
   invalidation.
2. Recursive typing is driven by an explicit global recursive-group state in
   the style of C++ `typeAnnotation/updateRecTypes`.
3. Promotion no longer needs shape-based fallback for missing canonical types.
4. `signal_fir_lane`, `zita_pipeline`, and the known corpus regressions pass.
5. The remaining limitation documented in Rustdoc can be reduced from
   “not a structural port of `updateRecTypes`” to a narrower statement, or
   removed if parity is reached.
