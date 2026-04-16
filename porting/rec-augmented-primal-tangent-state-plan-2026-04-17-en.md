# `Rec` on Augmented Primal/Tangent State — Forward AD Plan

**Date:** 2026-04-17  
**Status:** design plan  
**Scope:** replace the current "expand-after-Rec" handling of `fad(exp, x)` by a
`Rec` model that can carry primal and tangent channels directly when a recursive
branch needs to consume `fad` outputs locally

---

## 1. Problem statement

The current explicit-seed forward AD model in `faust-rs` works in two modes:

- outside recursion, `fad(exp, x)` immediately expands to two outputs per body
  output: `[primal, tangent]`;
- inside `boxRec(A, B)`, the `propagate` pass activates `suppress_fad`, keeps
  `fad` arity-transparent during the internal recursive wiring, and only expands
  the tangent outputs **after** the recursive group has been built.

This is sufficient for programs where `fad` appears *inside* a recursive branch
but its two outputs are **not** consumed locally within that branch.

It fails for programs such as:

```faust
step(prev) = prev - lr * grad
with {
    loss = (prev - target) ^ 2;
    grad = fad(loss, prev) : !, _;
};

process = step ~ _;
```

Observed failure:

- `fad(loss, prev)` is still treated as one output during the internal `Rec`
  wiring;
- `(!, _)` expects two inputs;
- propagation reports a sequential composition mismatch.

This is the precise failure mode behind `auto_pan5.dsp`.

---

## 2. Root cause

The current `FlatNodeKind::Rec` handler in `crates/propagate/src/lib.rs` does
this:

1. detect whether either branch contains `ForwardAD`;
2. if yes, set `ctx.suppress_fad = true`;
3. propagate the right and left branches as if every `ForwardAD` node were
   arity-transparent;
4. build the recursive `sigRec(...)` group on the primal signals only;
5. expand the pending FAD tangent outputs afterwards through
   `generate_fad_signals_multi(...)`.

This means the current recursive contract is:

- **inside** `Rec`, `fad(exp, x)` behaves like "just `exp`";
- **outside** `Rec`, it behaves like `[primal, tangent]`.

That contract is too weak for local consumers like:

- `fad(loss, prev) : !, _`,
- `fad(loss, prev) : (_, !)` or equivalent projections,
- local `par/seq/split/merge` structures that expect the full dual output.

---

## 3. Goal

Change the recursion model so that, when required, `boxRec(...)` is built over
an **augmented state space** carrying both:

- primal recursive signals,
- tangent recursive signals.

In other words, move from:

- **current model:** "expand-after-Rec"

to:

- **target model:** "`Rec` on augmented primal+tangent state"

for recursive subgraphs that locally consume FAD outputs.

This should make expressions like:

```faust
grad = fad(loss, prev) : !, _;
```

behave inside recursion the same way they already behave outside recursion.

---

## 4. Non-goals

This plan does **not** propose:

- changing the external semantics of `fad(exp, x)` outside recursion;
- changing `fad` to "tangent-only";
- replacing canonical signal recursion (`sigRec/sigProj`);
- implementing reverse mode `rad`;
- redesigning the evaluator alias-preservation model again.

The external AD contract remains:

```text
fad(exp, x) = [primal_0, tangent_0, primal_1, tangent_1, ...]
```

The change is strictly in how recursive propagation constructs and wires the
signal group.

---

## 5. Conceptual model

### 5.1 Current model

For one single-output recursive state:

```text
prev[n]    = state[n-1]
state[n]   = f(prev[n])
fad(loss, prev) inside f
```

the current implementation builds only the primal recursive group:

```text
sigRec([ state_body ])
```

and tries to recover AD afterwards.

This fails if `f(...)` itself needs the tangent as an intermediate signal.

### 5.2 Target model

For the same single-output recursive state, the recursive group should be able
to carry:

```text
[state_primal, state_tangent]
```

or, more generally, the full augmented bus required by local FAD consumers.

For a one-output `fad(loss, prev)` node used inside `f(...)`, the body sees two
signals immediately:

```text
[loss(prev), d loss / d prev]
```

and local projections/compositions are wired against those two signals before
the enclosing `Rec` is finalized.

The resulting recursive signal group is still emitted canonically as
`sigRec/sigProj`; only the number and role of the internal lanes change.

---

## 6. Design options

### Option A — Full dual-state recursion (recommended)

When `Rec` detects a `ForwardAD` whose outputs are consumed locally inside a
branch, recursively propagate that branch on the **real expanded AD arity**.

This means:

- no `suppress_fad` for that recursive subgraph;
- internal `Rec` wiring sees the full `[primal, tangent]` outputs;
- the recursive group is built directly over the augmented bus.

Pros:

- semantically direct;
- matches user intuition;
- general solution for local AD consumption inside recursion;
- keeps `fad` semantics uniform inside and outside recursion.

Cons:

- requires a deeper rewrite of `FlatNodeKind::Rec`;
- more subtle lane accounting;
- higher risk around de Bruijn lifting and recursive apertures.

### Option B — Selective tangent projection primitive

Keep the current suppressed `Rec` model, but add a special mechanism for local
requests such as "give me the tangent of this FAD result" without fully
expanding `fad` inside recursion.

Pros:

- more local change.

Cons:

- ad hoc;
- weaker composability;
- introduces a second semantic path for AD inside recursion;
- does not generalize cleanly to arbitrary local consumers of `[primal, tangent]`.

Conclusion: **do not choose Option B** unless Option A proves too invasive.

---

## 7. Proposed implementation strategy

### Phase A — Recursion classification

Add a structural analysis pass on `boxRec(left, right)` to distinguish:

1. **transparent FAD recursion**
   `ForwardAD` appears inside a branch but no local composition consumes its
   expanded outputs before the `Rec` boundary;
2. **augmented-state FAD recursion**
   a local consumer inside the branch requires the real expanded outputs.

Examples:

- transparent:
  - `+~(fad(*(g), g))`
  - `fad(+ ~ *(fb), fb)` where expansion is only needed after the full `Rec`
- augmented-state:
  - `fad(loss, prev) : !, _`
  - `par(fad(loss, prev), something_else)` where both FAD outputs participate
    locally

Deliverable:

- one helper such as `rec_fad_mode(arena, left, right) -> RecFadMode`

Possible enum:

```rust
enum RecFadMode {
    None,
    ExpandAfterRec,
    AugmentedState,
}
```

### Phase B — Real expanded arity inside `Rec`

Teach `box_arity_wiring(...)` and/or a dedicated recursive arity helper to
compute the **augmented internal wiring arity** when `RecFadMode::AugmentedState`
is active.

The current rule:

- `ForwardAD` is transparent in wiring arity

must become conditional:

- transparent only for `ExpandAfterRec`,
- expanded for `AugmentedState`.

This is the first point where the internal recursive bus width changes.

### Phase C — Build augmented recursive projections

In `FlatNodeKind::Rec`, replace the current unconditional `suppress_fad`
protocol by a mode switch:

- `None` → ordinary recursion path
- `ExpandAfterRec` → current protocol unchanged
- `AugmentedState` → build the recursive group directly from the expanded
  branch signals

For `AugmentedState`:

1. construct the feedback placeholders (`l0`) at the augmented width;
2. propagate the right branch on that augmented bus;
3. build `rec_inputs` from the augmented outputs + lifted external inputs;
4. propagate the left branch without suppressing `ForwardAD`;
5. build the final `sigRec(...)` over the expanded body signals;
6. emit the resulting projections directly, without a final
   `generate_fad_signals_multi(...)` pass.

### Phase D — `ForwardAD` in augmented recursive mode

The `FlatNodeKind::ForwardAD` arm currently does:

- if `ctx.suppress_fad`: push seed and return body signals only
- else: expand immediately

In the new model:

- `AugmentedState` mode must reach the "expand immediately" branch even inside
  `Rec`
- and any seed depending on recursive projections must participate correctly in
  the de Bruijn-lifted recursive environment

This will likely require a new context flag or mode field rather than the
current single boolean `suppress_fad`.

Candidate replacement:

```rust
enum FadRecHandling {
    Normal,
    SuppressAndExpandAfterRec,
    ExpandInsideRec,
}
```

### Phase E — Signal-level invariants

Validate the following invariants on the augmented recursive group:

- deterministic lane ordering,
- stable primal/tangent pairing,
- correct `de_bruijn_aperture_with_memo(...)` behavior,
- no dangling recursive references,
- correct slot lifting for symbolic/eval-preserved aliases,
- no accidental capture of outer recursive references by inner groups.

---

## 8. Lane-order contract

The augmented recursive state needs a deterministic output ordering.

Recommended contract:

For each original branch output `s_i`, store:

```text
[primal_i, tangent_i]
```

and for multiple outputs:

```text
[primal_0, tangent_0, primal_1, tangent_1, ...]
```

Rationale:

- matches the public `fad(exp, x)` contract;
- makes local projections intuitive;
- avoids a second ordering scheme inside recursion.

Do **not** switch to:

```text
[all primals..., all tangents...]
```

unless there is a compelling implementation reason, because that would create
two incompatible mental models for the same operator.

---

## 9. Interaction with recursive alias preservation

The `eval` extension from
`porting/eval-recursive-alias-preservation-plan-2026-04-16-en.md`
remains complementary:

- it ensures that names such as `prev = state` survive `eval` long enough to
  reach `propagate`;
- this new plan ensures that, once in `propagate`, local AD consumers can use
  the full `[primal, tangent]` outputs inside recursion.

In short:

- **alias preservation** solves "can the recursive variable be named?"
- **augmented-state recursion** solves "can local recursion logic consume the
  AD outputs immediately?"

Both are needed for source patterns like `auto_pan5.dsp`.

---

## 10. Tests

### 10.1 Positive fixtures

Add targeted fixtures for:

1. **Tangent projection in feedback function**

```faust
process = step ~ _
with {
    step(prev) = prev - lr * (fad((prev-target)^2, prev) : !, _);
};
```

2. **Primal projection in feedback function**

Use the primal output of `fad(...)` locally to confirm both outputs are truly
available in the recursive branch.

3. **Both outputs consumed locally**

Use `par`/`seq` so both primal and tangent are routed inside the recursive
branch.

4. **Existing transparent recursive FAD cases**

Re-run all `fad_recursive*.dsp` fixtures to ensure the old mode still works.

5. **Recursive alias + local FAD consumption**

`prev = state; grad = fad(loss, prev) : !, _;`

This is the direct regression test for `auto_pan5`-style programs.

### 10.2 Negative fixtures

Keep or add failures for:

- true malformed recursive arity mismatches,
- malformed local projections on non-FAD outputs,
- unsupported nested cases if the first slice deliberately excludes them.

---

## 11. Compatibility impact

### 11.1 User-visible behavior

Positive impact:

- programs like `auto_pan5.dsp` become expressible;
- `fad(exp, x)` behaves more uniformly inside and outside recursion.

Potential risk:

- some recursive programs previously interpreted under the old "expand-after-Rec"
  model may produce a different internal signal structure.

Mitigation:

- preserve `ExpandAfterRec` for the existing corpus family where no local AD
  consumer forces the expanded outputs;
- only switch to `AugmentedState` when structurally required.

### 11.2 Mapping status

This is an **adapted** Rust behavior relative to the current C++ compiler.

Current upstream C++ Faust does not support the full `fad(exp, x)` explicit-seed
model in the same form, and the old `expand-after-Rec` parity target is already
a Rust-side design choice. This plan extends that design choice further.

---

## 12. Recommended incremental rollout

### Slice 1

Implement `RecFadMode` classification and support the minimal case:

- single-output recursion,
- one local tangent projection `: !, _`,
- one `ForwardAD` node in the recursive branch.

Goal:

- make `auto_pan5.dsp`-style programs compile through the signal pipeline.

### Slice 2

Support:

- local use of both primal and tangent,
- multiple `ForwardAD` nodes in one recursive branch,
- recursive alias + explicit-seed AD together.

### Slice 3

Stress and hardening:

- nested recursions,
- multiple-output recursions,
- FIR/backend validation,
- differential corpus updates.

---

## 13. Success criteria

This plan is successful when:

1. `auto_pan5.dsp`-style programs compile through `eval -> propagate`.
2. Existing `fad_recursive*.dsp` tests remain green.
3. The recursive signal output remains canonical `sigRec/sigProj`.
4. Lane ordering is deterministic and documented.
5. No regression appears in ordinary non-AD recursion.

---

## 14. Deliverables checklist

- [ ] `RecFadMode` analysis added.
- [ ] `FlatNodeKind::Rec` supports `AugmentedState`.
- [ ] `ForwardAD` propagation can expand inside recursion when requested.
- [ ] Lane-order contract documented in Rustdoc.
- [ ] Positive regression test for `auto_pan5`-style program added.
- [ ] Existing recursive FAD corpus revalidated.
- [ ] Journal entry recorded with compatibility note.
