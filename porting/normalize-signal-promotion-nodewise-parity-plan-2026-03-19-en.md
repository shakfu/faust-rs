# Plan — Node-wise `SignalPromotion` Parity vs C++ `sigPromotion.cpp`

Date: 2026-03-19

## Goal

Refactor Rust signal promotion so it follows the C++
`SignalPromotion::transformation(Tree sig)` model more faithfully:

- promotion remains driven by the canonical type annotation of the original
  graph,
- each node is rebuilt according to the rule of the current parent node,
- context-sensitive cases such as `select2`, delay indices, table indices, and
  mixed arithmetic are handled explicitly at the call site,
- shared DAG nodes do not get "polluted" by a promotion decision taken for a
  different parent context.

This is a parity-first refactor for `crates/normalize/src/normalform.rs`, not a
backend workaround.

## C++ Reference

Primary file:
- `/Users/letz/Developpements/RUST/faust/compiler/transform/sigPromotion.cpp`

Relevant functions:
- `SignalPromotion::transformation(Tree sig)`
- `SignalPromotion::smartCast(Type t1, Type t2, Tree sig)`
- `SignalPromotion::smartIntCast(Type t, Tree sig)`
- `SignalPromotion::smartFloatCast(Type t, Tree sig)`
- `signalPromote(Tree sig, bool trace)`

Type assumptions consumed by this pass:
- `/Users/letz/Developpements/RUST/faust/compiler/signals/sigtyperules.cpp`
- `getCertifiedSigType(...)`

## Problem Statement

The current Rust promoter in `normalform.rs` is already fed by the canonical
`SigType` map, but it is still structurally weaker than the C++ pass:

- it memoizes promotion by `SigId` only,
- it assumes one promoted node is valid for all parent contexts,
- it sometimes repairs a context mismatch after the fact (`smart_int_cast`
  wrapping an already promoted subtree),
- it therefore does not model the C++ rule "rebuild this node according to the
  current parent transformation rule" closely enough.

This weakness shows up on shared sub-DAGs such as:
- a comparison reused both in a real arithmetic expression and as the selector
  of `select2`,
- table indices reused in numeric and index-only contexts,
- any node whose original type is stable but whose promoted representation is
  context-sensitive.

The `motion_wrapper_demo_test` regression is a concrete example:
- a `SIGSELECT2` selector ended up as `Float32` in FIR,
- while C++ expects `smartIntCast(ts, self(sel))`,
- because Rust reused a previously float-promoted version of the shared
  comparison.

## Why "node-wise parity" is preferable here

The C++ pass does not treat promotion as a single globally memoized rewrite for
each original node. It applies transformation rules node by node:

- for `sigBinOp`, it decides promotion from the parent binop rule,
- for `sigSelect2`, it forces `smartIntCast` on the selector and promotes
  branches according to branch compatibility,
- for delays and tables, it forces integer contexts only where the parent node
  requires them.

The Rust port should therefore prefer:
- explicit parent-rule reconstruction,
- context-local promotion requests,
- memoization only where the promoted result is actually context-invariant.

## Non-Goal

This plan does **not** introduce a generalized "promotion context lattice" as
the primary design.

Although contextual memoization would fix some cases, it would still encode the
Rust pass around a cache-centric model that does not match the C++ structure
well enough. The priority here is to replay the C++ node-wise rules more
directly.

## Target Architecture

Refactor `SignalPromoter` into a small rule engine whose public entry point is
still:

```rust
fn promote(sig: SigId) -> Result<SigId, NormalFormError>
```

but whose internal reconstruction APIs distinguish between:

1. **plain child reconstruction**
   - `promote_child(sig)`
2. **child reconstruction for integer-required parents**
   - `promote_child_as_int(sig)`
3. **child reconstruction for real-required parents**
   - `promote_child_as_float(sig)`
4. **child reconstruction compatible with a reference operand/node type**
   - `promote_child_like(reference_sig, child_sig)`

These helpers should mirror the C++ `smartIntCast` / `smartFloatCast` use at
the parent rule site, not patch a mismatch only after a context-free rewrite.

## Core Design Rules

### R1. Promotion decisions remain based on original canonical types

All promotion rules must continue to consume the original canonical `SigType`
map from `TypeAnnotator`.

The pass must not infer target numeric natures from already promoted subtrees.
That would drift away from C++ `getCertifiedSigType(...)`.

### R2. Parent rules own context-sensitive reconstruction

Examples:
- `Select2(selector, x, y)` owns the integer requirement of `selector`.
- `Delay(value, amount)` owns the integer requirement of `amount`.
- `RdTbl(table, index)` owns the integer requirement of `index`.
- arithmetic and comparisons own the float-coercion decision for their
  operands when operand natures differ.

No generic child visitor should silently decide those contexts on its own.

### R3. Context-free memoization only

Memoization may remain for cases that are provably context-invariant:
- list reconstruction,
- symbolic recursion/list structure,
- leaves whose promoted form is identical to the original form,
- nodes whose parent rules do not change child representation.

If a node can legitimately be rebuilt differently under different parents, the
pass must not cache one promoted form and reuse it blindly everywhere.

### R4. `smart_*_cast` helpers remain simple wrappers

`smart_int_cast` and `smart_float_cast` should operate on a child rebuilt for
the current parent rule, not serve as repair logic for cross-context cache
contamination.

The desired model is:
- rebuild child under parent rule,
- apply `smartIntCast` / `smartFloatCast` exactly where C++ does,
- return.

## Deliverables

### D1. Audit current promotion rules against C++

Create a mapping table between Rust `SigMatch` cases and C++
`SignalPromotion::transformation(...)` branches.

Each entry should state:
- parity status: `1:1`, `adapted`, or `incorrect`,
- whether child promotion is context-sensitive,
- whether current Rust memoization is safe for that case.

Primary target file:
- `crates/normalize/src/normalform.rs`

### D2. Split child reconstruction APIs

Introduce internal helpers that make parent-context intent explicit, e.g.:

```rust
fn promote_plain(&mut self, sig: SigId) -> Result<SigId, NormalFormError>;
fn promote_as_int(&mut self, sig: SigId) -> Result<SigId, NormalFormError>;
fn promote_as_float(&mut self, sig: SigId) -> Result<SigId, NormalFormError>;
fn promote_like(&mut self, reference: SigId, sig: SigId) -> Result<SigId, NormalFormError>;
```

These names are illustrative; the implementation can choose clearer names as
long as the parent-rule intent is explicit.

### D3. Make `select2` a parity reference case

Port `Select2` to match the C++ structure directly:

- selector reconstructed with the integer-required helper,
- branches rebuilt with plain reconstruction first,
- branch compatibility then handled according to the original branch types,
- no reuse of a float-polluted selector from another parent path.

This case should be used as the first parity anchor because it currently
exposes the architectural weakness most clearly.

### D4. Apply the same parent-owned pattern to index-only contexts

Refactor these cases to use explicit integer-required child promotion:
- `Delay`
- `ZeroPad`
- `RdTbl`
- `WrTbl` write indices
- `SoundfileLength`
- `SoundfileRate`
- `SoundfileBuffer` part/index

The goal is to match the C++ "parent node enforces int here" style directly.

### D5. Revisit binop/comparison rules for shared nodes

Ensure the `BinOp` branch mirrors C++:
- mixed `Add/Sub/Mul` and comparisons coerce operands to float,
- integer-only ops coerce operands to int,
- the result type remains whatever `SigType` says for the original node.

This must be validated on shared comparison DAGs used in both arithmetic and
selector contexts.

### D6. Narrow memoization scope if necessary

After refactoring the rule engine, reduce memoization to the subset of cases
that remain context-invariant.

If a full `memo: HashMap<SigId, SigId>` is no longer sound, replace it with a
more conservative cache or remove it selectively for sensitive node families.

This should be guided by parity and correctness first; performance tuning comes
after correctness is re-established.

## Migration Plan

### Phase A — Parity audit and rule inventory

1. Audit each Rust `SigMatch` promotion branch against C++.
2. Mark which branches are context-sensitive.
3. Record where current memoization is unsound.

Exit condition:
- a concrete rule inventory exists,
- `select2` and index contexts are explicitly identified as context-sensitive.

### Phase B — Introduce parent-owned reconstruction helpers

1. Add the new internal helper APIs.
2. Convert `Select2` first.
3. Convert delay/table/index-only cases next.

Exit condition:
- no parent rule relies on a context-free promoted child where C++ would apply
  `smartIntCast` or `smartFloatCast` at the parent site.

### Phase C — Reduce unsafe memoization

1. Identify which node families can still use plain memoization safely.
2. Disable or narrow memoization for context-sensitive families.
3. Re-measure the current performance-sensitive demos after correctness is
   restored.

Exit condition:
- shared-node regressions such as `motion_wrapper_demo_test` are gone,
- the promotion result is stable under DAG sharing.

### Phase D — Documentation and non-regression coverage

1. Add Rustdoc comments in `normalform.rs` documenting the C++ provenance and
   the rule-by-rule parity contract.
2. Add structural tests for:
   - shared comparison reused in arithmetic and `select2`,
   - shared real/int index expressions reused in ordinary arithmetic and table
     index contexts,
   - delay/index cases that must remain integer after promotion.
3. Update `porting/MEMOIZATION.md` if memoization scope changes.

Exit condition:
- parity-sensitive node families are documented and covered by tests,
- memoization documentation matches the implemented design.

## Validation Requirements

Mandatory checks before declaring this refactor ready:

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`

Targeted parity regressions that must pass:
- `motion_wrapper_demo_test`
- shared `select2` selector non-regression tests in `transform`
- existing fast-lane FIR verification tests
- at least one table/index reproducer with shared DAG inputs

## Success Criteria

The refactor is considered successful when:

1. `select2` selectors no longer become spuriously floating under DAG sharing.
2. Promotion no longer depends on cache pollution from unrelated parent
   contexts.
3. Rustdoc for `normalform.rs` clearly states the C++ rule provenance and the
   remaining adapted areas, if any.
4. Any remaining memoization is explicitly justified as context-invariant.

## Open Questions

These should be answered during implementation, not guessed up front:

- Which `SigMatch` families are truly context-invariant and safe to memoize?
- Is a small amount of context-tagged memoization still useful for performance
  after the node-wise refactor, or can the parity cases stay cache-free?
- Are there additional C++ parent-rule cases beyond `select2` and table/delay
  indices that Rust currently treats too generically?
