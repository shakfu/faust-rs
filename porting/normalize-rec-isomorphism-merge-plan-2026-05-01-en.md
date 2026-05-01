# Normalize — Isomorphic Recursive Group Merging Plan

**Date:** 2026-05-01
**Status:** Planned
**Scope:** Add a pre-simplification pass in `crates/normalize/` that detects
structurally isomorphic `SIGREC` groups and unifies them to a single canonical
representative, enabling downstream algebraic simplification (e.g. `x - x → 0`)
across signal trees produced by independent constructions.

**Motivating case:** `fad_rec.dsp`

```faust
process_fad = step ~ (_, _);           -- manual FAD: builds Rec₁
fad(process_original, (g))             -- auto FAD:   builds Rec₂

process = _<: (process_fad : *(-1),*(-1)), fad(process_original, (g)) :> _,_;
-- Expected: both outputs are zero.  Actual: two live recursion arrays.
```

Both `Rec₁` and `Rec₂` implement the same first-order IIR `y[n] = x[n] + g·y[n-1]`
and the same gradient `dy[n] = y[n-1] + g·dy[n-1]`. Because they are constructed
by separate pipeline stages (propagation for the FAD transform, evaluation for the
`step ~ (_, _)` literal), they receive different `SigId`s. The simplifier's
`x - x → 0` rule (commit `215b688`) therefore cannot fire: `Proj(i, Rec₁) ≠ Proj(i, Rec₂)`
as `SigId`s even though the two recursive systems are semantically identical.

**Why here (normalize) and not in eval or propagation:**

- The FAD transform runs in `propagate`, which has no knowledge of the
  simultaneously-evaluated `step ~ (_, _)` subtree.  Merging there would require
  cross-subtree analysis that does not belong to a single-signal transform.
- The evaluator (`eval`) builds `Rec` nodes eagerly during beta-reduction;
  injecting a unification step there would complicate the substitution model.
- `normalize` already holds the complete signal forest for all output channels
  and is the natural place for a global, graph-wide structural pass.  The
  existing `SimplifyCache` provides the memoization infrastructure.

---

## Core concept: opening a recursive group

A `SIGREC(body)` node is **self-referential**: `body` contains `SIGPROJ(i, R)`
back-edges that point to `R` itself.  Because `R`'s `SigId` is determined by
`body` (hash-consed), and `body` depends on `R`'s `SigId`, two independently
constructed but semantically equivalent Rec nodes end up with different `SigId`s.

The fix is to **open** each Rec before comparing: replace every `Proj(i, R)` in
`body` with a canonical sentinel `Hole(i)`, yielding an acyclic DAG.  Two Rec
nodes are **isomorphic** iff their opened DAGs are identical (same SigId after
hash-consing in the arena, since all other sub-nodes are already structural).

For multi-output groups (`k` output lanes), the opened form is the k-tuple of
opened per-lane bodies.

---

## Algorithm

### Step 1 — Collect reachable Rec nodes

Traverse all output signal roots with a depth-first walk (using a `visited:
HashSet<SigId>` to avoid re-visits).  Each `SIGREC` node encountered is added
to a `Vec<SigId>` of candidates.

### Step 2 — Compute the opened signature of each Rec

For a Rec node `R` with body `body_R`:

1. Choose `HOLE_BASE`: a fixed `SigId` obtained by interning a sentinel tag
   (e.g. `SIGHOLE`) once into the arena at the start of the pass.  Individual
   holes are represented as `SIGPROJ(i, HOLE_BASE)` — these are normal
   hash-consed arena nodes, so two `Hole(i)` built from the same `i` are
   always the same `SigId`.

2. Traverse `body_R` with a recursive sig-map that replaces `Proj(i, R)` →
   `Proj(i, HOLE_BASE)` and recurses into all other nodes.  A local
   `HashMap<SigId, SigId>` caches the traversal.  Rec nodes *other than R*
   encountered inside `body_R` are treated as opaque leaves (no recursion
   into foreign Rec bodies).

3. The result `opened_R: SigId` is the opened body, hash-consed in the arena.

### Step 3 — Group Rec nodes by opened signature

Build a `HashMap<SigId, Vec<SigId>>` mapping `opened_R → [R₁, R₂, …]`.
Groups with exactly one member need no action.

### Step 4 — Build the substitution map

For each group with two or more members, elect the representative with the
smallest `SigId` (deterministic choice that depends only on graph structure,
not construction order).  Populate a `HashMap<SigId, SigId>` mapping each
non-canonical `Rᵢ → R_canon`.

### Step 5 — Apply substitution to the signal forest

Traverse all output roots with a memoized sig-map that:
- On `Proj(i, R)` where `R` is in the substitution map: return `Proj(i, R_canon)`.
- On `Rec(body)` where the Rec itself is non-canonical: replace with the
  canonical Rec.  (Its body is identical by definition, so this is safe; the
  original non-canonical `Rec` node simply becomes unreachable.)
- On all other nodes: recurse into children, rebuild with arena.intern.

The traversal uses the same sentinel pattern as `sig_map` in `simplify.rs` to
break Rec cycles.

### Step 6 — Run simplify

After substitution, `Proj(0, Rec₁)` and `Proj(0, Rec₂)` are now both
`Proj(0, R_canon)` — the same `SigId`.  The existing `x - x → 0` rule in
`simplification()` fires on the subtracted pair and reduces both output channels
to `Int(0)`.

---

## Nested Rec groups

If a Rec body itself references another Rec (mutual recursion or nested
feedback), the opening step treats inner Rec nodes as opaque.  Two outer Rec
nodes are isomorphic only if their opened bodies (including the opaque inner Rec
SigIds) are identical.  This is correct: if inner Rec nodes are also isomorphic,
Step 3 will group them independently and Step 5 will unify them first (the pass
processes the substitution map in a single graph walk — one pass suffices because
the substitution map is built globally before any rewriting starts).

---

## Implementation plan

### New file: `crates/normalize/src/rec_merge.rs`

```rust
pub(crate) fn merge_isomorphic_rec_groups(
    arena: &mut TreeArena,
    roots: &[SigId],
) -> Vec<SigId>;
```

Performs Steps 1–5 and returns the substituted roots.  Pure signal-graph
transformation; no type information needed.

Internal helpers:
- `collect_rec_nodes(arena, roots) -> Vec<SigId>`
- `open_rec(arena, rec: SigId, hole_base: SigId) -> SigId`
- `build_substitution(arena, recs: &[SigId]) -> HashMap<SigId, SigId>`
- `apply_substitution(arena, roots: &[SigId], subst: &HashMap<SigId, SigId>) -> Vec<SigId>`

### Integration point: `crates/normalize/src/lib.rs`

Call `merge_isomorphic_rec_groups` on the output signal roots immediately before
the existing `simplify` (or `simplify_with_cache`) call in the normalize
pipeline entry point.

```rust
let roots = merge_isomorphic_rec_groups(arena, &roots);
let simplified: Vec<SigId> = roots
    .iter()
    .map(|&r| simplify(arena, types, r))
    .collect();
```

### Tests: `crates/normalize/src/rec_merge.rs` (inline test module)

| Test name | What it checks |
|---|---|
| `merge_identical_single_output_recs` | Two scalar `Rec(x + g*Proj(0,self))` built separately → unified to one representative |
| `merge_identical_multi_output_recs` | Two 2-lane `Rec` (primal + gradient) → unified |
| `merge_does_not_unify_distinct_recs` | Two Recs with different bodies → no substitution |
| `merge_followed_by_simplify_gives_zero` | Full round-trip: build `Proj(0,R1) - Proj(0,R2)` for isomorphic R1, R2 → simplify → `Int(0)` |
| `merge_nested_rec_groups` | Outer Recs that reference distinct but isomorphic inner Recs → all four unified |
| `merge_is_idempotent` | Running the pass twice produces the same result |

---

## Complexity

| Step | Cost |
|---|---|
| Collect Rec nodes | O(N) nodes visited |
| Open each Rec | O(D) per Rec where D = body depth |
| Group by signature | O(R) where R = number of Rec nodes |
| Apply substitution | O(N) nodes visited |
| **Total** | **O(N + R·D)** — linear in graph size for fixed-depth bodies |

For typical Faust DSPs, R ≤ 20 and D ≤ 50, so the pass is negligible relative
to the rest of the normalize pipeline.

---

## Non-goals

- Semantic equivalence beyond structural isomorphism (e.g. commutativity,
  algebraic identities inside Rec bodies).  Two Recs with bodies `x + g*y` and
  `g*y + x` are not merged.
- Merging Rec nodes that differ only in their output arity.
- Changes to the FAD transform or evaluator.
- Any change to code generation or FIR lowering.

---

## Reference

- `crates/normalize/src/simplify.rs` — `sig_map` cycle-breaking pattern reused
  in Steps 2 and 5.
- Commit `215b688` — adds `BinOp::Sub => 0` self-operation rule (Cause 2 fix),
  which this pass activates for FAD-produced signal graphs.
- `fad_rec.dsp` — motivating test case (manual FAD vs `fad()` macro).
