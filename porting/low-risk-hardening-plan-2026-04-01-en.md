# Low-Risk Hardening Plan for the Rust Faust Port

**Date:** 2026-04-01
**Status:** proposed
**Target crates:** `tlib`, `boxes`, `signals`, `transform`, `fir`, `compiler`, `xtask`
**Primary risk area:** `signal_prepare -> signal_fir -> backend`
**Goal:** make the port easier to modify safely by adding executable invariants,
verifiers, and differential/property checks before introducing more semantic work

---

## 1. Purpose

At the current stage of the Faust C++ to Rust port, the highest-value
hardening work is not new lowering logic. It is making the existing contracts
explicit and executable.

The repository already has:

- a broad and mostly stable front-end pipeline,
- a real Rust-native `signal -> FIR` fast lane,
- a first-class FIR IR with an existing verifier,
- strong differential and snapshot tooling.

The remaining parity pressure is concentrated in the active lowering slice, not
in the basic existence of the pipeline.

So the recommended strategy is:

1. add checks at phase boundaries,
2. add internal "verified state" wrappers,
3. tighten structural validators for canonical IR encodings,
4. expand low-risk test oracles,
5. defer behavior-changing refactors until these guards exist.

This is the best path to improve robustness and modifiability without changing
observable compiler behavior.

---

## 2. Guiding Principle

The main rule for this hardening work is:

> Prefer executable contracts over algorithmic changes.

That means the first additions should be:

- validators,
- debug assertions,
- type-consistency checks,
- structural non-regression tests,
- differential/metamorphic checks,
- verified wrappers that prevent invalid states from crossing stage boundaries.

It explicitly does **not** mean:

- rewriting `eval`,
- changing propagation ownership or caches,
- changing `signal_fir` semantics,
- broad IR redesign,
- replacing the canonical `sigRec/sigProj` representation,
- introducing new optimizations in parity-sensitive code paths.

---

## 3. Current Leverage Points

The current codebase already contains several good insertion points:

- `crates/transform/src/signal_prepare.rs`
  - documents a strong boundary contract for prepared signals, but that
    contract is not yet fully executable as a dedicated verifier.
- `crates/fir/src/checker.rs`
  - already provides a substantial FIR verifier and diagnostic model.
- `crates/compiler/src/lib.rs`
  - is the natural façade boundary for introducing verified wrappers between
    compile stages.
- `crates/tlib`, `crates/signals`, `crates/boxes`
  - already document canonical shape invariants for trees, lists, recursion,
    and builder/matcher APIs.
- `crates/xtask/src/main.rs`
  - already hosts snapshot, differential, and trace-based validation tooling.

These existing seams mean the hardening work can be layered on top of the
current design rather than forcing a redesign.

---

## 4. Recommended Additions

### 4.1 `signal_prepare`: add a real prepared-forest verifier

**Primary file:** `crates/transform/src/signal_prepare.rs`

Add one explicit verifier:

- `verify_prepared_signals(prepared: &PreparedSignals, ui: &UiProgram) -> Result<(), SignalPrepareError>`

or equivalently:

- `PreparedSignals::verify(&self, ui: &UiProgram) -> Result<(), SignalPrepareError>`

The verifier should check only postconditions that are already documented or
already assumed by downstream code.

Recommended checks:

- no reachable prepared node remains in de Bruijn recursion form,
- every reachable node has an entry in `types`,
- every reachable node has an entry in `sig_types`,
- `SimpleSigType` matches the reduced view of `SigType`,
- recursion groups are structurally well formed,
- `proj(index, group)` indices are in range for the referenced symbolic group,
- unary-group canonicalization is complete (`proj(k, unary_group)` is normalized
  to slot `0`),
- prepared output count matches the caller-visible output arity,
- UI control references used by prepared signals remain valid against the
  `UiProgram`.

This is the single highest-value low-risk addition because many implicit
assumptions in `signal_fir` are really assumptions about prepared-signal
correctness.

### 4.2 `compiler`: introduce verified stage wrappers

**Primary file:** `crates/compiler/src/lib.rs`

Add internal wrappers that encode "this stage has been checked" without changing
the public compile behavior.

Candidate wrappers:

- `VerifiedPreparedSignals`
- `VerifiedFirModule`

Construction rule:

- these wrappers are only created after the corresponding verifier succeeds.

Benefits:

- invalid intermediate states stop at the boundary that produced them,
- later refactors cannot accidentally bypass verification,
- API signatures become self-documenting without changing runtime behavior.

The wrappers can remain crate-private or module-private initially.

### 4.3 `tlib`: add structural validators for lists and recursion trees

**Primary files:** `crates/tlib/src/lib.rs`, `crates/tlib/src/recursion.rs`

Add cheap explicit validators for canonical tree encodings that are already
documented.

Recommended helpers:

- `validate_faust_list(arena, root)`
- `validate_closed_de_bruijn_tree(arena, root)`
- `validate_symbolic_recursion_tree(arena, root)`

Recommended checks:

- proper `cons/nil` termination,
- valid de Bruijn payload shapes,
- valid symbolic recursion shapes,
- no malformed `SYMREC` / `SYMREF`,
- deterministic, closed recursion where required by the caller boundary.

These should be reusable by `signal_prepare` tests and by any future
normalization or transform pass.

### 4.4 `signals` and `boxes`: tighten canonical builder/matcher invariants

**Primary files:** `crates/signals/src/lib.rs`, `crates/boxes/src/builder.rs`,
`crates/boxes/src/lib.rs`

The builders and matchers already define the canonical IR surface. They are the
right place for debug-only hardening.

Recommended additions:

- targeted `debug_assert!` on local encoding invariants,
- internal helpers such as `assert_canonical_sig_shape(...)`,
- tests that verify builder/matcher round-trip stability for representative
  families.

Good targets:

- index-bearing nodes (`Input`, `Output`, `Proj`),
- slider/list payload encodings,
- numeric node surfaces (`i32` public surface over `i64` storage),
- multi-child tags with fixed child order.

This keeps canonical encoding drift from spreading into downstream phases.

### 4.5 `fir`: extend the verifier rather than the lowerer

**Primary file:** `crates/fir/src/checker.rs`

The FIR verifier already exists and should be expanded before more lowering
logic is changed.

Recommended additions:

- checks for backend-ready expression positions,
- stronger table/index invariants,
- stronger function-call/type checks where current behavior is already implied,
- verifier helpers tailored for tests and transform passes,
- optional stricter profiles for `signal_fir` and backend validation.

Important constraint:

- add checks only where the current code already relies on the property,
  directly or indirectly.

The goal is to surface hidden assumptions, not to redefine FIR semantics.

### 4.6 Add property and metamorphic tests before deep refactors

**Primary files:**

- `crates/tlib/tests/recursive_trees.rs`
- `crates/tlib/tests/core_semantics.rs`
- `crates/signals/tests/core_api.rs`
- `crates/boxes/tests/core_api.rs`
- `crates/transform/src/signal_prepare/tests.rs`
- `crates/compiler/tests/signal_fir_lane.rs`

There is currently little or no property-testing infrastructure in the
workspace. Adding a small one is a low-risk way to protect canonical behavior.

Recommended property/metamorphic checks:

- `list_to_vec(vec_to_list(xs)) == xs`,
- builder/matcher round-trips for canonical subsets,
- `de_bruijn_to_sym` rejects open trees and preserves structural shape,
- `prepare_signals_for_fir` preserves output count,
- prepared forests contain no untyped reachable nodes,
- optimized vs unoptimized runtime equivalence on representative traces,
- legacy vs fast-lane equivalence on the already supported subset.

The first target should be structural properties, not random full-program
generation.

### 4.7 `xtask`: promote existing differential tooling into stronger gates

**Primary file:** `crates/xtask/src/main.rs`

The repository already has strong tooling:

- golden snapshots,
- runtime traces,
- backend alignment smoke tests,
- lane comparisons,
- backend diff reports.

The next improvement is not "more tools", but more explicit use of those tools
as invariants.

Recommended additions:

- representative opt-level parity checks (`opt_level=0` vs max),
- explicit lane-equivalence subsets,
- a dedicated "structural hardening" smoke target that runs only low-cost
  invariants and differentials,
- stricter failure messages that name the violated contract, not just the file.

---

## 5. Concrete File Plan

| File | Low-risk hardening addition |
|---|---|
| `crates/transform/src/signal_prepare.rs` | add `PreparedSignals` verifier and postcondition checks |
| `crates/transform/src/signal_prepare/tests.rs` | add prepared-forest contract tests |
| `crates/compiler/src/lib.rs` | add internal verified wrappers for prepared signals and FIR |
| `crates/tlib/src/lib.rs` | add strict list validators / helpers |
| `crates/tlib/src/recursion.rs` | add recursion-shape validators and more explicit validation APIs |
| `crates/tlib/tests/recursive_trees.rs` | add structural non-regression and property tests |
| `crates/signals/src/lib.rs` | add debug-only canonical-shape assertions |
| `crates/signals/tests/core_api.rs` | add builder/matcher contract tests |
| `crates/boxes/src/builder.rs` | add local encoding assertions where shape is fixed |
| `crates/boxes/tests/core_api.rs` | extend canonical round-trip coverage |
| `crates/fir/src/checker.rs` | extend verifier with currently-assumed invariants |
| `crates/fir/src/checker/tests.rs` | add non-regression cases for new diagnostics |
| `crates/xtask/src/main.rs` | add or tighten low-cost differential/trace gates |

---

## 6. Suggested Implementation Order

### Phase 1 — Boundary verification first

1. Add `PreparedSignals` verification.
2. Wire it into `compiler` in debug/tests.
3. Add targeted tests for prepared-forest contracts.

This phase should land before any further `signal_fir` broadening.

### Phase 2 — Structural validators in foundational IR crates

1. Add `tlib` list and recursion validators.
2. Add `signals` / `boxes` debug-only canonical assertions.
3. Add builder/matcher structural tests.

This phase reduces accidental IR drift during future refactors.

### Phase 3 — FIR verifier tightening

1. Add the next layer of `fir::checker` diagnostics.
2. Use the verifier more consistently around transform/backend tests.
3. Record any newly surfaced pre-existing gaps as explicit follow-up work.

### Phase 4 — Property and metamorphic validation

1. Add a small property-testing dependency if needed.
2. Start with `tlib` and `signal_prepare`.
3. Extend `xtask` smoke gates for lane and runtime parity on selected cases.

---

## 7. Acceptance Criteria

This hardening plan is successful when:

- prepared-signal postconditions are machine-checked,
- invalid recursion/list/canonical-shape states fail close to their source,
- FIR verification covers more of the invariants already assumed by backends,
- low-cost property/metamorphic tests protect core IR contracts,
- the default development loop gets stronger without changing compiler output,
- future semantic changes in `signal_fir` can rely on verified preconditions
  instead of ad hoc assumptions.

---

## 8. Non-Goals

This plan deliberately does **not** include:

- broad refactors of `eval` or `propagate`,
- replacing `sigRec/sigProj` as canonical external form,
- changing public CLI or FFI behavior,
- introducing new optimizations in the active fast lane,
- redesigning the TreeArena model,
- changing corpus semantics to fit new internal abstractions.

Any work in those categories should happen only after the hardening layers above
exist and are active.

---

## 9. Why This Is the Right Time

The port is no longer in the earliest bootstrap stage:

- the front-end pipeline is already broad and mostly stable,
- the active parity bottleneck is concentrated in `signal_prepare` /
  `signal_fir`,
- FIR is already important enough to justify stronger verified boundaries,
- CI and xtask tooling are now rich enough to support low-risk hardening work.

That makes this the right point to add safety rails before the next round of
feature or parity expansion.

---

## 10. References

- `porting/faust-rs-porting-status-2026-03-27-en.md`
- `porting/faust-rs-supported-faust-subset-en.md`
- `porting/faust-rust-recursion-model-note-en.md`
- `crates/transform/src/signal_prepare.rs`
- `crates/fir/src/checker.rs`
- `crates/compiler/src/lib.rs`
- `crates/tlib/src/lib.rs`
- `crates/tlib/src/recursion.rs`
- `crates/signals/src/lib.rs`
- `crates/boxes/src/builder.rs`
- `crates/xtask/src/main.rs`
