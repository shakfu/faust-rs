# True Closure Model Port Plan (Rust `eval` vs C++)

> **Date**: 2026-03-06
> **Scope**: `crates/eval`, `crates/boxes`, parser/eval integration points
> **Reference C++ baseline**: `master-dev-ocpp-od-fir-2-FIR19` (`8eebea429`)
> **Reference C++ source roots**:
> - `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.cpp`
> - `/Users/letz/Developpements/RUST/faust/compiler/evaluate/environment.cpp`
> - `/Users/letz/Developpements/RUST/faust/compiler/evaluate/environment.hh`
> - `/Users/letz/Developpements/RUST/faust/compiler/boxes/boxes.cpp`
> **Status**: implemented in `crates/eval` with adapted Rust-side representation and parity guardrails

---

## 1. Goal

The Rust port has already restored several important parity points:

- patterned and grouped parser definitions,
- evaluated `case` patterns before automaton construction,
- barrier semantics for repeated pattern variables,
- adapted `a2sb` lowering for residual `abstr` / `case` forms,
- `slot` / `symbolic` propagation support.

The main structural mismatch that remains is the evaluator environment model.
The current Rust implementation still stores bare `TreeId` definitions in a
scoped environment and reuses the caller environment during identifier forcing.
The C++ compiler, by contrast, stores **captured closures** in each environment
layer and evaluates them later in the environment captured at definition time.

If the objective is full semantic parity, the next major evaluator task should
be to port that closure model instead of extending the current eager
adaptation.

---

## 2. C++ Target Semantics That Must Be Preserved

## 2.1 Definitions are stored as closures, not raw expressions

`pushMultiClosureDefs(...)` in `environment.cpp` does not bind a symbol to its
raw RHS. It binds:

- `closure(expr, genv, visited, lenv)`

where `lenv` is the environment layer being created. This means every
definition carries its own lexical capture, including same-layer references.

This is the semantic anchor for:

- lexical scoping,
- delayed evaluation,
- recursive-definition detection keyed by `(id, env)`,
- later environment rewrites such as `copyEnvReplaceDefs(...)`.

## 2.2 Identifier forcing uses the captured environment

`evalIdDef(...)` in `eval.cpp`:

1. searches the environment chain for the definition of `id`,
2. builds the visited key from `(id, lenv)`,
3. evaluates the found definition with `eval(def, visited', nil)`.

When `def` is a closure, `eval(...)` then switches to the closure's captured
`lenv`, not the caller environment.

The important parity point is not just "lookup a symbol", but "force the
captured closure bound to that symbol".

## 2.3 Abstractions and environments evaluate to closures

The C++ evaluator returns closures for:

- `boxAbstr`
- `boxEnvironment`
- `boxComponent`
- `boxLibrary`

Those are not fully normalized box trees yet. They are evaluator values that
carry a captured environment and can later be:

- applied,
- accessed (`boxAccess`),
- rewritten (`boxModifLocalDef`),
- lowered by `a2sb`.

## 2.4 `boxAccess` and `boxModifLocalDef` depend on closure identity

Two C++ branches depend directly on the closure model:

- `boxAccess(body, field)`:
  if `body` evaluates to a closure/environment value, the field is resolved in
  the closure's captured environment.
- `boxModifLocalDef(body, defs)`:
  if `body` evaluates to a closure, C++ rebuilds a new environment by copying
  the closure environment, rewiring enclosed closures via `updateClosures(...)`,
  and replacing selected bindings with `copyEnvReplaceDefs(...)`.

This is the strongest concrete argument against keeping an eager
environment-threading-only model: `boxModifLocalDef` is defined in terms of
captured-environment rewriting.

## 2.5 Pattern matchers also capture environment state

`evalCase(...)` in C++ caches a pattern matcher for a specific `(rules, env)`
pair and seeds one barrier environment per rule with:

- `listn(len(rules), pushEnvBarrier(env))`

The rule RHSs are closures that retain the lexical environment visible when the
`case` was evaluated. A fully faithful port therefore needs stable environment
identity, not only evaluated rule trees.

## 2.6 `a2sb()` lowers evaluator values, not only syntax nodes

The C++ `a2sb()` pass lowers:

- residual abstraction closures,
- residual pattern matchers,
- access-driven captured environments,
- any remaining evaluator results that are still closure-shaped.

The current Rust adaptation lowers residual syntax that survived eager
evaluation. That restores practical behavior for the current corpus, but it is
not the same representation contract.

---

## 3. Current Rust State

The current Rust evaluator is still an **adapted** model, not a 1:1 closure
port.

## 3.1 Environment contents

`Environment` currently stores:

- `Vec<(SymId, TreeId)>`
- optional parent
- barrier flag

Bindings are raw box-tree ids, not captured evaluator values.

## 3.2 Identifier forcing

`eval_box(...)` on `Ident`:

1. looks up the bound `TreeId`,
2. calls `eval_box(value, env, ...)` again with the **current** environment.

This is the key structural divergence from C++ `evalClosure(...)`.

## 3.3 Abstractions are normalized eagerly

`BoxMatch::Abstr` currently evaluates the body immediately in a child scope and
returns a normalized `abstr(arg, evaluated_body)` node. No explicit closure
object survives inside the evaluator.

That means the Rust evaluator currently cannot:

- distinguish "syntax abstraction" from "captured closure value",
- rewrite closure environments directly,
- reuse the captured environment as a first-class value.

## 3.4 `access` is still a structural approximation

The current Rust `eval_access(...)` works by recognizing `with` / `withrec`
shells and by checking for `environment` after evaluating the body. This
restores common cases, but it is still not a true "access the captured closure
environment" model.

## 3.5 No Rust equivalent yet for `copyEnvReplaceDefs(...)`

Rust still has no direct equivalent for:

- `copyEnvReplaceDefs(...)`
- `updateClosures(...)`

This is the main blocker for a real `boxModifLocalDef` port and for any future
feature that depends on environment rewriting instead of flat shadowing.

## 3.6 Loop detection and caches still assume raw-tree bindings

The current `LoopDetector` tracks raw trees and fresh slots. C++ recursion
detection is really tied to the pair `(id, env)`. Likewise, pattern-matcher and
symbolic caches become more robust when keyed by stable environment identity.

---

## 4. Why The True Closure Port Is Worth Doing

## 4.1 It closes the remaining semantic hole instead of adding more special cases

The current eager adaptation already needed dedicated fixes for:

- grouped definitions,
- rule barriers,
- evaluated patterns,
- residual `a2sb` lowering,
- modulation.

Continuing in that direction would likely add more structural special cases for
`access`, `expr { defs }`, and future environment-sensitive forms. Porting the
real closure model addresses the common root instead.

## 4.2 It matches the C++ evaluator contract directly

With real closures:

- `pushMultiClosureDefs(...)` maps 1:1,
- `evalIdDef(...)` maps 1:1,
- `boxAccess(...)` maps 1:1,
- `boxModifLocalDef(...)` becomes implementable without ad hoc rewrites,
- `a2sb(...)` can be simplified toward the original C++ structure.

## 4.3 It gives stable identities for visited sets and caches

A true closure/environment model naturally provides stable `EnvId` handles for:

- recursion detection,
- rule environment capture,
- pattern-matcher caches,
- later parity work on metadata or environment-aware transforms.

---

## 5. Recommended Rust Internal Design

## 5.1 Introduce explicit evaluator values

Keep the public API unchanged (`eval_process(...) -> TreeId`), but make the
internal evaluator work on explicit values, for example:

```rust
enum EvalValue {
    Box(TreeId),
    Closure(ClosureId),
    PatternMatcher(PatternMatcherId),
}
```

with a closure payload conceptually equivalent to:

```rust
struct ClosureValue {
    expr: TreeId,
    genv: Option<EnvId>,
    visited: VisitedSetId,
    lenv: EnvId,
}
```

The exact storage container can vary, but the evaluator must be able to
distinguish:

- plain box syntax already in normal form,
- captured closures,
- pattern-matcher values carrying rule environments.

## 5.2 Replace cloned environments with stable environment ids

The current clone-based `Environment` type is convenient, but it does not give
stable environment identity. A true closure port needs that identity.

Recommended direction:

- introduce `EnvId`,
- store environment layers in an arena owned by the evaluator context,
- keep each layer as:
  - local bindings,
  - parent `EnvId`,
  - barrier bit.

This is the recommended design choice for implementation because it solves both
parity and performance concerns:

- stable ids for `(id, env)` recursion checks,
- no deep `Environment` cloning on scope pushes,
- direct parity path for `copyEnvReplaceDefs(...)`.

## 5.3 Bind symbols to evaluator values, not raw `TreeId`

Once `EnvId` exists, bindings should store value handles:

- `Vec<(SymId, ValueId)>`

instead of raw tree ids. `pushValueDef(...)` and `pushMultiClosureDefs(...)`
then become direct semantic ports rather than approximations.

---

## 6. Staged Correction Plan

## Stage 0. Documentation and invariants

Deliverables:

- Rustdoc in `crates/eval` states clearly that the current environment model is
  `adapted`, not a 1:1 closure port.
- This plan document becomes the reference for the closure migration.

Exit criteria:

- no Rustdoc still claims that explicit closures are unnecessary for full
  parity.

## Stage 1. Introduce stable environment identity

Deliverables:

- evaluator-owned environment arena with `EnvId`,
- parity-preserving `lookup`, `lookup_local`, `lookup_until_barrier`,
- unchanged public API and unchanged compiler outputs.

Recommended first implementation step:

- keep storing raw `TreeId` temporarily inside the new environment arena so the
  representation migration can be split from the semantic migration.

Exit criteria:

- current tests stay green,
- environment push/lookup semantics stay unchanged,
- pattern-matcher barrier tests keep passing.

## Stage 2. Introduce explicit closure values

Deliverables:

- new internal `EvalValue` representation,
- `pushMultiClosureDefs(...)` equivalent that binds closures capturing the new
  `EnvId`,
- value forcing helper equivalent to C++ `evalClosure(...)`.

Required follow-up:

- split current `eval_box(...)` into a "produce evaluator value" path and a
  "force/lower to box tree" path.

Exit criteria:

- identifier resolution no longer reuses the caller environment for captured
  definitions,
- builder-level regression tests cover lexical capture under shadowing.

## Stage 3. Port identifier forcing and recursion detection

Deliverables:

- Rust equivalent of `evalIdDef(...)`,
- visited-key tracking based on `(SymId, EnvId)` rather than raw tree identity,
- loop detector updated for closure forcing.

Exit criteria:

- recursive definition diagnostics stay stable,
- mutually recursive and shadowed definition tests match the C++ reference.

## Stage 4. Port closure-valued forms

Deliverables:

- `Abstr` returns closures internally,
- `Environment`, `Component`, and `Library` return closure-like evaluator
  values with captured environments,
- `Access` resolves through captured environments instead of structural shell
  inspection.

Exit criteria:

- existing `access` tests still pass,
- new differential tests cover access through captured closures, not only local
  `with {}` wrappers.

## Stage 5. Port environment rewriting

Deliverables:

- Rust equivalent of `copyEnvReplaceDefs(...)`,
- Rust equivalent of `updateClosures(...)`,
- `boxModifLocalDef` node family plus parser/eval integration if still absent.

This stage is where the true closure port pays off: the C++ semantics can be
ported directly instead of approximated by eager rebinding.

Exit criteria:

- `expr { defs }` behaves like C++ on targeted differential fixtures,
- modified closures preserve references to the rewritten environment and not the
  original one.

## Stage 6. Port pattern-matcher capture to the same value model

Deliverables:

- explicit pattern-matcher runtime value or `boxPatternMatcher` equivalent,
- per-rule captured barrier environments stored by stable id,
- cache keys based on rule tree plus captured environment identity.

Exit criteria:

- repeated compilation of the same `case` under different captured environments
  cannot cross-contaminate matcher state,
- residual case lowering in `a2sb` follows the same closure-driven flow as C++.

## Stage 7. Simplify `a2sb()` around real evaluator values

Deliverables:

- remove or reduce the current adapted lowering shortcuts,
- lower residual closures and pattern matchers from `EvalValue`,
- preserve `slot` / `symbolic` output contract for `propagate`.

Exit criteria:

- `a2sb` shape and control flow are explainable directly from C++ `real_a2sb`,
- current lambda / case / modulation fixtures remain green.

## Stage 8. Expand differential coverage

Deliverables:

- builder-level tests in `crates/eval/tests/core_eval.rs` for:
  - captured abstraction shadowing,
  - environment access through closure results,
  - `expr { defs }` environment rewrites,
  - closure-valued `component` / `library`,
  - recursion detection keyed by environment identity,
  - case-rule environment capture.
- compiler-level differentials against the C++ reference for the same families.

Exit criteria:

- the closure model is guarded both structurally and end-to-end,
- known-gap documentation for environment capture can be retired.

---

## 7. Files Likely To Change

- `crates/eval/src/lib.rs`
- `crates/eval/src/pattern_matcher.rs`
- `crates/boxes/src/lib.rs`
- `crates/parser/src/lib.rs`
- `crates/parser/src/grammar/faustparser.y`
- `crates/eval/tests/core_eval.rs`
- `crates/compiler/tests/signal_pipeline.rs`
- `tests/cpp_parity_known_gaps/` or successor parity fixtures

---

## 8. Recommended Order

The pragmatic order is:

1. environment ids,
2. explicit evaluator values,
3. captured identifier forcing,
4. closure-valued `abstr` / `environment` / `access`,
5. `copyEnvReplaceDefs(...)` + `boxModifLocalDef`,
6. pattern-matcher capture,
7. `a2sb` simplification,
8. differential expansion.

This order keeps the hardest semantic dependency in the right place:
`boxModifLocalDef` should not be attempted before the evaluator can represent
and rewrite captured environments explicitly.

---

## 9. Mapping Status Summary

- Public `eval_process(...)` API: `adapted`, intentionally source-compatible.
- Internal environment representation: `adapted representation`, `1:1` semantics through
  explicit [`EvalValue`] closures and stable `EnvId`.
- Identifier forcing semantics: `1:1`.
- `access` semantics: `1:1`.
- `expr { defs }` / `boxModifLocalDef`: `1:1`.
- Captured pattern-matcher environment model: `1:1` semantics with adapted Rust
  `EvalValue::PatternMatcher(...)`.
- `a2sb` lowering contract: `1:1` semantics through `a2sb_value(...)`, with
  first-order box output preserved for later passes.
