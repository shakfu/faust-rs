# Plan: De Bruijn coherence check in the normalizer

**Date:** 2026-05-05  
**Context:** Follow-up to the FAD cache-poisoning fix (`61f1cb2`).  
**Goal:** Make structural De Bruijn errors compile-time failures instead of silent wrong-gradient bugs.

---

## Problem statement

The FAD cache-poisoning bug (fixed in `61f1cb2`) produced a signal tree that was
**structurally valid** in the De Bruijn sense (no dangling free references at the
root level) but **semantically wrong**: a cached tangent computed inside a
`DEBRUIJNREC` body was re-used at the outer scope for a different signal because
their `SigId`s hash-collided.

The tree passed all existing structural checks (`de_bruijn_aperture`, `de_bruijn_to_sym`)
and flowed silently through `normalize_add_term` and the full FIR lowering pipeline,
producing incorrect C++ output.

A stronger invariant is needed: **every `DEBRUIJNREF(k)` encountered at De Bruijn
nesting depth `d` must satisfy `1 <= k <= d`**.  The Rust port follows the C++
one-based convention where `DEBRUIJNREF(1)` targets the nearest enclosing
`DEBRUIJNREC`.  Violations indicate that a transform
(FAD, RAD, or a future pass) produced a tree that crosses scope boundaries.

This invariant is more precise than the global aperture check:
- `de_bruijn_aperture(root) <= 0` (current gate) — checks that the full tree
  has no *free* references at depth 0.  An inner `DEBRUIJNREF(2)` inside two
  nested `DEBRUIJNREC` binders is correctly closed and invisible to aperture.
- **Proposed coherence check** — verifies that, at every local scope during
  traversal, a `DEBRUIJNREF(k)` is bound by an *ancestor* `DEBRUIJNREC` at the
  point where the reference appears, not merely by some enclosing binder elsewhere
  in the tree.

The practical trigger: `normalize_add_term` receives De Bruijn form signals when
it is invoked on FAD/RAD output **before** `de_bruijn_to_sym` has been called.
Any incoherent reference in that input silently corrupts factorization results.

---

## Scope

| Crate | File(s) | Change type |
|---|---|---|
| `tlib` | `src/recursion.rs` | New validator + error variant |
| `propagate` | `src/forward_ad.rs` | Post-transform gate |
| `propagate` | `src/reverse_ad.rs` | Post-transform gate |
| `normalize` | `src/normalform.rs` | Pre-`de_bruijn_to_sym` gate |
| `normalize` | `src/normalize.rs` | Entry guard in `normalize_add_term` |

---

## Step 1 — `tlib`: `check_de_bruijn_coherence`

### 1a. New error variant in `RecursionError`

```rust
// crates/tlib/src/recursion.rs

pub enum RecursionError {
    // … existing variants …

    /// A `DEBRUIJNREF(k)` was found at nesting depth `depth`
    /// where `k < 1 || k > depth` — the reference escapes its binders.
    IncoherentDeBruijnReference {
        node: TreeId,
        /// The bad De Bruijn level stored in the DEBRUIJNREF node.
        level: i64,
        /// Number of enclosing DEBRUIJNREC binders at the point of discovery.
        depth: i64,
    },
}
```

Display message:
```
De Bruijn coherence error: DEBRUIJNREF(level={level}) at depth={depth}
— reference escapes its enclosing binders (node {node})
```

### 1b. New public function `check_de_bruijn_coherence`

```rust
/// Verify that every `DEBRUIJNREF(k)` in the tree rooted at `root`
/// satisfies `1 <= k <= depth` where `depth` is the number of enclosing
/// `DEBRUIJNREC` binders at the reference site.
///
/// Returns `Ok(())` if the tree is coherent; `Err(IncoherentDeBruijnReference)`
/// on the first violation found.
///
/// Note: this is stricter than `is_de_bruijn_closed`, which only checks
/// that the *root-level aperture* is non-positive.  A tree can be closed
/// at the root while containing an incoherent inner reference if the
/// violating `DEBRUIJNREF` is inside a nested `DEBRUIJNREC` that
/// partially absorbs it.
pub fn check_de_bruijn_coherence(
    arena: &TreeArena,
    root: TreeId,
) -> Result<(), RecursionError> {
    check_coherence_at_depth(arena, root, 0, &mut AHashMap::new())
}
```

Internal recursive helper (memoised over `(node, depth)` pairs):

```rust
fn check_coherence_at_depth(
    arena: &TreeArena,
    id: TreeId,
    depth: i64,
    memo: &mut AHashMap<(TreeId, i64), ()>,
) -> Result<(), RecursionError> {
    if memo.contains_key(&(id, depth)) {
        return Ok(());   // already verified at this depth
    }
    if let Some(level) = match_de_bruijn_ref(arena, id) {
        if level <= 0 || level > depth {
            return Err(RecursionError::IncoherentDeBruijnReference {
                node: id,
                level,
                depth,
            });
        }
        memo.insert((id, depth), ());
        return Ok(());
    }
    if let Some(body) = match_de_bruijn_rec(arena, id) {
        check_coherence_at_depth(arena, body, depth + 1, memo)?;
        memo.insert((id, depth), ());
        return Ok(());
    }
    // Generic node: recurse on children.
    let children = arena.children(id)
        .map(|ch| ch.to_vec())
        .unwrap_or_default();
    for child in children {
        check_coherence_at_depth(arena, child, depth, memo)?;
    }
    memo.insert((id, depth), ());
    Ok(())
}
```

**Key property:** The memo key is `(TreeId, depth)`, not just `TreeId`, because the
same hash-consed node can appear at different depths within the tree (this is precisely
the class of bug we are guarding against).

### 1c. Unit tests in `tlib/tests/recursive_trees.rs`

- `coherence_ok_for_closed_tree` — simple `DEBRUIJNREC(DEBRUIJNREF(1))` passes.
- `coherence_ok_for_nested_closed_tree` — two nested `DEBRUIJNREC` with correct
  inner `DEBRUIJNREF(1)` and outer `DEBRUIJNREF(2)`.
- `coherence_err_free_ref_at_root` — bare `DEBRUIJNREF(1)` at depth 0 returns
  `IncoherentDeBruijnReference { level: 1, depth: 0 }`.
- `coherence_err_inner_ref_escapes` — `DEBRUIJNREC(add(DEBRUIJNREF(1), DEBRUIJNREF(2)))`
  — inner `DEBRUIJNREF(2)` escapes (depth=1, level=2 → NOT coherent).
- `coherence_vs_aperture_distinction` — construct a tree that is closed at the root
  (`aperture <= 0`) but incoherent in an inner scope.  Verify `is_de_bruijn_closed`
  returns `true` while `check_de_bruijn_coherence` returns `Err`.

---

## Step 2 — `propagate`: post-transform gate

### 2a. `forward_ad.rs` — after `generate_fad_signals_multi`

```rust
// crates/propagate/src/forward_ad.rs

pub(super) fn generate_fad_signals_multi(
    arena: &mut TreeArena,
    outputs: &[SigId],
    seeds: &[SigId],
) -> Result<Vec<SigId>, PropagateError> {
    // … existing transform …

    // ── De Bruijn coherence gate ─────────────────────────────────────────
    for &sig in &result {
        tlib::check_de_bruijn_coherence(arena, sig)
            .map_err(|e| PropagateError::DeBruijnCoherence {
                pass: "FAD",
                detail: format!("{e}"),
            })?;
    }
    Ok(result)
}
```

### 2b. `reverse_ad.rs` — after `generate_rad_signals`

Same pattern, `pass: "RAD"`.

### 2c. New `PropagateError` variant

```rust
// crates/propagate/src/lib.rs

pub enum PropagateError {
    // … existing variants …

    /// A De Bruijn coherence violation was found in the output of an AD
    /// transform.  This indicates a bug in the transform itself.
    DeBruijnCoherence {
        /// Name of the pass that produced the incoherent tree ("FAD" / "RAD").
        pass: &'static str,
        /// Human-readable description of the first violation.
        detail: String,
    },
}
```

Display:
```
De Bruijn coherence error in {pass} transform: {detail}
```

---

## Step 3 — `normalize`: pre-`de_bruijn_to_sym` gate

### 3a. In `prepare_signals_multi` (normalform.rs)

Between signal collection and Step 1 (`de_bruijn_to_sym`):

```rust
// crates/normalize/src/normalform.rs  — prepare_signals_multi

// ── Step 0: De Bruijn coherence pre-check ─────────────────────────────
for &s in sigs {
    tlib::check_de_bruijn_coherence(arena, s)?;   // maps to NormalFormError via From<RecursionError>
}

// ── Step 1: de Bruijn → symbolic ──────────────────────────────────────
// …
```

`From<RecursionError> for NormalFormError` already exists; the new variant
`IncoherentDeBruijnReference` will propagate automatically as `NormalFormError::Recursion`.

### 3b. Entry guard in `normalize_add_term` (normalize.rs)

```rust
// crates/normalize/src/normalize.rs

pub(crate) fn normalize_add_term(
    arena: &mut TreeArena,
    types: &HashMap<SigId, SigType>,
    t: SigId,
) -> SigId {
    // Sanity: closed De Bruijn trees must be coherent before normalization.
    // Open subtrees can appear while recursive groups are still being built;
    // the mandatory whole-tree gate runs in prepare_signals_multi.
    #[cfg(debug_assertions)]
    if tlib::is_de_bruijn_closed(arena, t)
        && let Err(e) = tlib::check_de_bruijn_coherence(arena, t)
    {
        panic!(
            "normalize_add_term received an incoherent De Bruijn tree: {e}\n\
             Closed De Bruijn trees must be coherent before normalization."
        );
    }

    let mut a = Aterm::from_sig(arena, types, t);
    // …
}
```

Using `#[cfg(debug_assertions)]` keeps the production fast path free of the
traversal cost while catching bugs in test/debug builds.

---

## Step 4 — Error plumbing

| Error | Bubbles to | Via |
|---|---|---|
| `RecursionError::IncoherentDeBruijnReference` | `NormalFormError::Recursion` | existing `From<RecursionError>` |
| `PropagateError::DeBruijnCoherence` | `CompilerError::Propagate` | existing `From<PropagateError>` (in compiler) |

No new `CompilerError` variant is required.

---

## Step 5 — Integration test

Add a test in `crates/compiler/tests/signal_pipeline.rs` (or a new
`fad_coherence_test.rs`) that exercises the FAD gate end-to-end:

1. Construct a signal tree that would have triggered the pre-fix cache-poisoning
   bug (i.e., the tree from `fad_seed_not_poisoned_by_inner_rec_back_edge`).
2. Temporarily disable the cache-restore fix (comment it out) and assert that
   `generate_fad_signals_multi` now returns
   `Err(PropagateError::DeBruijnCoherence { pass: "FAD", .. })`.
3. Re-enable the fix and assert the call succeeds.

This test documents that the coherence gate is a *regression detector*, not
just documentation.

---

## Implementation order

1. `tlib`: new error variant + `check_de_bruijn_coherence` + unit tests.
2. `propagate`: `DeBruijnCoherence` variant + gates in FAD/RAD.
3. `normalize`: pre-check in `prepare_signals_multi`.
4. `normalize`: `#[cfg(debug_assertions)]` guard in `normalize_add_term`.
5. Compiler integration test.

Steps 1–2 can land independently; steps 3–4 depend on step 1.

## Implementation notes

- The existing Rust port follows the C++ one-based De Bruijn convention:
  `DEBRUIJNREF(1)` refers to the nearest enclosing `DEBRUIJNREC`.  The
  implemented coherence predicate is therefore `1 <= level <= depth`, rather
  than the zero-based `level < depth` notation used in the original sketch.
- FAD/RAD can be invoked while a recursive group is still being assembled, so an
  AD result may be temporarily open until the enclosing `DEBRUIJNREC` is built.
  The AD post-transform gates check outputs that are already closed; the
  mandatory full-tree gate remains `normalize::prepare_signals_multi`, before
  `de_bruijn_to_sym`.

---

## Complexity and cost

| Location | Cost mode | Traversal cost |
|---|---|---|
| FAD/RAD gate (step 2) | Always on | O(n) over the FAD output, memoised |
| `prepare_signals_multi` (step 3) | Always on | O(n) before de_bruijn_to_sym — saves a full clone vs the existing `validate_closed_de_bruijn_tree` |
| `normalize_add_term` (step 4) | Debug only | Zero in release |

The memoisation over `(SigId, depth)` pairs means each unique (node, depth)
combination is visited at most once.  For realistic DSP graphs the cost is
dominated by the `de_bruijn_to_sym` pass that immediately follows.

---

## What this does NOT cover

- **Semantic coherence** (wrong tangent cached but structurally valid tree): the
  check detects structural violations only.  The `fad_seed_not_poisoned_*` test
  remains the regression guard for the semantic class of bug.
- **Symbolic-form trees**: after `de_bruijn_to_sym`, there are no
  `DEBRUIJNREF`/`DEBRUIJNREC` nodes; the check is a no-op (fast path).
- **Hash-consing collisions**: the check cannot detect two different semantic
  values sharing a `SigId` — that class of bug requires value-level tracking.
