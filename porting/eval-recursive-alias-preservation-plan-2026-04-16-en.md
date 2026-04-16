# Recursive Alias Preservation in `eval` — Extension Plan

**Date:** 2026-04-16  
**Status:** design plan for a Rust-only compiler extension  
**Scope:** make the evaluator preserve selected recursive aliases instead of
eagerly forcing them into a loop during box evaluation

---

## 1. Problem statement

Today, both `faust-rs` and upstream C++ Faust reject source patterns where a
recursive value is rebound through a normal alias and then reused while the
recursive box is still being formed.

Minimal shape:

```faust
state = next ~ _;
prev = state;
next = f(prev);
```

Current observed behavior:

- `faust-rs`: `EvalError::LoopDetected`
- upstream C++ Faust: endless evaluation cycle after the evaluator step budget

When `fad(exp, x)` is involved, the same underlying issue appears in a more
visible form:

```faust
state = next ~ _;
prev = state;
grad = fad(loss(prev), prev);
next = update(prev, grad);
```

The important point is that this is **not fundamentally an autodiff bug**.
`fad` only makes a pre-existing evaluator limitation easier to hit.

---

## 2. Why this matters

This limitation blocks a class of programs that are semantically reasonable:

- recursive state updates written in a more explicit style,
- gradient-based adaptation where the differentiated variable is the recursive
  state itself,
- source-level refactorings that introduce a name for a recursive projection
  instead of inlining it everywhere,
- potential future library abstractions that package recursive state access in a
  helper binding.

Without an extension, users must either:

- inline the recursive value in places where a local name would be clearer, or
- avoid certain optimization/learning-style formulations entirely.

---

## 3. Non-goal: changing canonical signal recursion

This plan does **not** propose replacing canonical `sigRec/sigProj`.

The external signal contract remains:

- recursive groups are represented as `sigRec(...)`,
- recursive references are represented as `sigProj(i, group)`.

The change is strictly in the **evaluator boundary**:

- preserve one first-order box form long enough for `propagate` to build the
  recursive signal group correctly,
- do not force a recursive alias too early.

This is aligned with `porting/faust-rust-recursion-model-note-en.md`: keep
`sigRec/sigProj` as canonical output, improve internal operational behavior.

---

## 4. Root cause in the current Rust evaluator

In `crates/eval/src/lib.rs`, identifier evaluation is still eager enough that a
plain alias to a recursive definition is forced immediately:

1. `prev` is looked up.
2. The binding resolves to a closure/value whose body is the recursive box.
3. `eval_ident_value(...)` re-enters evaluation of that recursive body.
4. The loop detector sees the same recursive definition while the group is still
   being formed and reports a loop.

For ordinary recursive diagrams this is acceptable and mirrors C++ behavior.
For the alias-preservation extension, it is precisely the behavior that needs to
change.

---

## 5. Proposed extension

Introduce a **preserved recursive alias** path in `eval`:

- when an identifier resolves to a recursive box that is already in-flight,
  do **not** immediately force the closure/body,
- instead, rebuild a first-order box form that still points at the recursive
  value symbolically,
- let `propagate` lower that preserved structure into canonical recursive
  signals.

Operationally, this means:

- detect a specific kind of loop as **preservable** rather than fatal,
- only for recursive alias references,
- only when the resulting preserved box is still first-order and acceptable by
  `try_build_flat_box(...)`.

This is an intentional divergence from current C++ behavior. It should therefore
be documented as `adapted`, not `1:1`.

---

## 6. Candidate representation strategies

### 6.1 Preferred: preserve a box alias/symbolic form

When `prev` aliases a recursive value already in flight, return a box form that
preserves the reference instead of forcing it.

Candidate shapes:

- a preserved `Ident`-like node that survives `eval` only for this case, or
- a `Symbolic` wrapper using an explicit slot identity already accepted by
  `propagate`, or
- a dedicated box node such as `boxRecAlias(slot, body)` if existing forms are
  not expressive enough.

Requirements:

- the preserved node must remain hash-consable,
- it must be first-order by the time `propagate` sees it,
- it must preserve arity and lexical capture correctly.

### 6.2 Alternative: closure-side sentinel in `eval`

Instead of emitting a preserved box node immediately, `eval` could return a
host-side sentinel `EvalValue::RecursiveAlias(...)` and only lower it to a box
when forced by `force_value_to_box(...)`.

Pros:

- less surface change in the box layer initially.

Cons:

- a new evaluator-only value family complicates forcing/lowering logic,
- downstream box passes still need a concrete first-order representation.

Conclusion: useful as an implementation technique, but likely not the best final
representation.

---

## 7. Required compiler changes

### Phase A — Semantics and representation choice

Deliverable:

- choose the preserved representation (`Symbolic` reuse vs new box node vs
  evaluator-only sentinel + lowering rule),
- document invariants and parity impact.

Files likely touched:

- `porting/` plan docs,
- `crates/boxes/src/*` if a new box node is needed,
- `crates/eval/src/environment.rs` if a new `EvalValue` variant is introduced.

### Phase B — Loop classification in `eval`

Deliverable:

- distinguish "hard recursive loop" from "preservable recursive alias" in
  `eval_ident_value(...)`.

Possible implementation direction:

- when the loop detector detects re-entry on a recursive binding, inspect the
  current forcing context,
- if the re-entry is through an alias reference that can be preserved, return
  the preserved form instead of `LoopDetected`.

Files likely touched:

- `crates/eval/src/lib.rs`
- `crates/eval/src/loop_detector.rs`
- `crates/eval/src/error.rs`

### Phase C — Forcing boundary

Deliverable:

- ensure `force_value_to_box(...)` can lower preserved recursive aliases to a
  first-order box accepted by `propagate`.

Files likely touched:

- `crates/eval/src/lib.rs`
- potentially `crates/boxes/src/builder.rs`

### Phase D — Propagation bridge

Deliverable:

- accept the preserved alias shape in the flat-box builder or lower it away
  before flat-box construction.

Files likely touched:

- `crates/propagate/src/lib.rs`
- possibly `crates/propagate/src/forward_ad.rs` if the preserved shape reaches
  the explicit-seed AD path

### Phase E — Tests and diagnostics

Deliverable:

- new positive fixtures for preserved recursive aliases,
- preserve a clear error for genuinely non-preservable recursive loops.

Suggested test layers:

- unit tests in `crates/eval/tests/core_eval.rs`,
- integration tests in `crates/compiler/tests/signal_pipeline.rs`,
- corpus fixtures under `tests/corpus/`.

---

## 8. Initial acceptance target

The extension should be considered successful when all of the following are
true:

1. A minimal alias recursion such as
   `state = next ~ _; prev = state; next = prev;`
   compiles through `eval` and `propagate`.
2. The resulting signal graph still uses canonical recursive form
   (`sigRec/sigProj`), not a new public recursion encoding.
3. Existing negative tests for true recursive-evaluation bugs remain negative.
4. The explicit-seed FAD example family
   `grad = fad(loss(prev), prev); next = update(prev, grad);`
   can progress past `eval`.
5. Diagnostics still point at the correct source binding when a non-preservable
   cycle is encountered.

---

## 9. Expected usage outside FAD

This extension would be useful beyond autodiff.

### 9.1 Clearer source-level recursive code

Users could write:

```faust
state = next ~ _;
prev  = state;
next  = saturate(prev + input);
```

Semantically, this is a valid Faust recursion pattern. The intended meaning is:

- `state` is the output of the recursive group,
- `prev = state` names the recursive projection,
- inside the `~` model that projection denotes the **previous-sample** value of
  the state, not the instantaneous value currently being computed.

In sample-by-sample terms, the diagram corresponds to:

```text
state[n] = saturate(state[n-1] + input[n])
```

So the circuit is causal and computable: the feedback path still carries the
usual one-sample delay implied by Faust recursion. The current problem is
therefore **not** that the source describes a non-causal DSP graph. The problem
is that the evaluator expands `prev = state` too early, treating it like an
ordinary alias to force immediately instead of preserving it long enough for the
recursive group to be built.

With the current compiler behavior there is **not** a generally valid inline
equivalent for this style. In practice, users must often reformulate the
algorithm so that the recursive state is not rebound through a normal alias at
all, or avoid this source style entirely.

### 9.2 Recursive helper abstractions

Library code could introduce local helper names/functions around the recursive
state without forcing users to inline every reference manually.

Examples:

- naming the previous state before branching,
- packaging a recursive value through a local helper combinator,
- using local aliases for readability in multi-step updates.

### 9.3 Stateful optimization and adaptation patterns

This is the `auto_pan3.dsp` motivation:

- LMS-like update rules,
- one-step gradient descent on a recurrent parameter/state,
- adaptive filters or controllers where the updated quantity is also the
  differentiated variable.

### 9.4 General robustness of `eval`

Even outside recursion-heavy DSP design, preserving legitimate first-order alias
structure instead of over-eagerly forcing it is a robustness improvement:

- fewer false-positive evaluation loops,
- less pressure to encode source style around evaluator quirks,
- clearer separation between "lexical aliasing" and "semantic recursion".

---

## 10. Risks

### 10.1 Scope creep

It is easy to accidentally broaden this into a full lazy-evaluation rewrite.
This plan should stay narrow:

- preserve only the recursive alias family,
- do not redesign the whole closure model again,
- keep the output first-order.

### 10.2 New unsound loops

If the preservation rule is too permissive, genuinely malformed recursive
definitions could slip through `eval` and fail later in harder-to-debug ways.

Mitigation:

- preserve only well-identified recursive alias cases,
- keep a negative test corpus for true cycles.

### 10.3 Flat-box leakage

If the preserved representation is not accepted by `try_build_flat_box(...)`,
the extension simply moves the failure later.

Mitigation:

- define the representation and flat-box contract together,
- include an end-to-end compiler test in the same change.

### 10.4 Intentional C++ divergence

This is not a parity patch. It is a Rust extension.

Mitigation:

- document the mapping status as `adapted`,
- gate any future cross-compiler structural differential tests accordingly.

---

## 11. Recommended first implementation slice

The lowest-risk first slice is:

1. Add one minimal corpus fixture with a plain recursive alias and no `fad`.
2. Make `eval` preserve that alias long enough to reach `propagate`.
3. Confirm canonical `sigRec/sigProj` output.
4. Add one second fixture covering `fad(loss(prev), prev)` on the same pattern.

This keeps the first slice honest:

- if the plain alias case still fails, the problem is not FAD-specific,
- if the plain alias case passes but the FAD case still fails, the remaining
  work is genuinely in the explicit-seed AD boundary.

---

## 12. Deliverables checklist

- [ ] Representation choice documented (`Symbolic` reuse vs new box node vs
  sentinel + lowering).
- [ ] `eval` can classify preservable recursive alias re-entry.
- [ ] Preserved form lowers to first-order box IR accepted by `propagate`.
- [ ] Plain recursive alias positive fixture added.
- [ ] Recursive alias + explicit-seed FAD positive fixture added.
- [ ] Negative loop-detection fixtures kept or expanded.
- [ ] Journal entry added documenting the intentional Rust/C++ divergence.
