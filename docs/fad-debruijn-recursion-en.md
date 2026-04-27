# Forward-Mode AD over de Bruijn Recursive Signals

Synthesis note describing how the forward-mode automatic differentiation
(FAD) pass implemented in `crates/propagate/src/forward_ad.rs` handles
recursive signal groups expressed in de Bruijn form, with particular focus
on `transform_uncached`, `match_de_bruijn_rec`, and the `SigMatch::Proj`
arm.

## 1. Background

In the `propagate` phase, every recursive signal group is materialized in
**de Bruijn form**, i.e. as the pair of tag nodes:

- `DEBRUIJNREC(body)` — a binder introducing a recursive scope whose body is
  a cons-list of element signals;
- `DEBRUIJNREF(level)` — an integer reference to the `level`-th enclosing
  `DEBRUIJNREC` (innermost = `1`).

Individual outputs of a group are accessed through projection nodes
`Proj(index, group)`, where `group` is either a `DEBRUIJNREC` directly or a
`DEBRUIJNREF` resolving to one. Symbolic-form recursions (`SYMREC` /
`SYMREF`) are introduced only later, by `de_bruijn_to_sym` in
`signal_prepare`, and are therefore never observed by FAD.

## 2. Internal representation of differentiated signals

FAD operates on a dual carrier:

```text
Dual { primal: SigId, tangents: [SigId; N] }
```

where `N` is the number of seed lanes (the seeds passed to
`fad(expr, seeds)`). For a recursion of arity `k`, FAD must produce a
single rebuilt group whose body has arity `k · (1 + N)`, interleaving each
slot's primal with its `N` tangent lanes:

```text
[ p0, t0_s0, …, t0_s{N-1},
  p1, t1_s0, …, t1_s{N-1},
  … ]
```

This interleaving is the contract that `Proj` will rely on when selecting a
specific lane out of the group.

## 3. Three kinds of indices

The recursion handling rests on three distinct numeric notions which the
implementation keeps separate:

| Notion              | Carrier                               | Meaning                                                                 |
|---------------------|---------------------------------------|-------------------------------------------------------------------------|
| **Level**           | payload of `DEBRUIJNREF(level)`       | Static integer naming the `k`-th enclosing `DEBRUIJNREC` (innermost = 1) |
| **`debruijn_depth`**| dynamic counter on `ForwardADTransform` | Number of `DEBRUIJNREC` scopes the *current* descent has itself rewritten |
| **Slot**            | `index` payload of `Proj(index, …)`   | Position of an output element inside a recursion body                   |

The level is a property of the source term; `debruijn_depth` is a property
of the rewriter's traversal; the slot is a property of the projection. FAD
correctness depends on relating them correctly when a `Proj` reaches the
arm responsible for it.

## 4. Entry into a `DEBRUIJNREC` body — `transform_uncached`

When `transform_uncached` is called on a signal `sig`, it consults the
arena via `match_de_bruijn_rec(arena, sig)`. If that returns `Some(body)`,
the following sequence is executed (forward_ad.rs, lines 560–615):

1. **Cycle-breaking placeholder.** Insert a self-referential `Dual` for
   `sig` into the cache *before* descending. Any `DEBRUIJNREF(1)`
   encountered in the body is then resolved through `match_de_bruijn_ref`
   into a fresh `Dual` whose primal and every tangent point back at the
   original (un-rewritten) group. Because this entry is overwritten with
   the rebuilt interleaved group on return, the placeholder is never
   observed externally — its only role is to make the back-edge resolvable
   while slots are being interned.

2. **Increment `debruijn_depth`** by `+1` to reflect that the rewriter has
   now entered one more recursive scope on this descent path.

3. **Lift active seeds.** Crossing a `DEBRUIJNREC` binder shifts every free
   `DEBRUIJNREF` in the surrounding term by one level. To keep `SigId`
   equality in seed recognition meaningful inside the body, every seed is
   transformed through `lift_de_bruijn(arena, seed)` and the reverse index
   is rebuilt. The previous `(diff_seeds, diff_seed_index)` is stashed and
   restored on exit.

4. **Differentiate the body.** `transform_list(body)` walks each element of
   the cons-list and produces a `Vec<Dual>` of the same length `k`.

5. **Restore state.** Pop the seed snapshot and decrement
   `debruijn_depth`.

6. **Interleave and re-intern.** For every element dual `(p_i, t_i_*)`,
   push `p_i` followed by every tangent lane `t_i_s0, …, t_i_s{N-1}` into a
   single linear vector of length `k · (1 + N)`. Wrap that as a cons-list
   `list_node` and feed it to `de_bruijn_rec` to obtain the rebuilt group
   `fad_rec`.

7. **Return.** The returned `Dual` has primal `fad_rec` and *all* tangent
   lanes equal to `fad_rec`. At the group level there is no meaningful
   primal/tangent split: differentiation is realised only when a `Proj`
   selects a specific slot inside the interleaved body.

`match_de_bruijn_ref` is also consulted at the top of
`transform_uncached`: a bare `DEBRUIJNREF(level)` carries no slot of its
own, so it is forwarded as primal with all tangent lanes pointing to the
same reference. The interpretation of that reference is deferred to the
enclosing `Proj`.

## 5. Projection — `SigMatch::Proj`

The `Proj(index, group)` arm (forward_ad.rs, lines 936–997) is where the
three kinds of indices are reconciled. Let `N` be the seed count and let
`L = 1 + N` (the per-slot **lane count**, exposed by `bundle_lane_count`).

The arm first classifies the `group` operand into three cases via a local
`GroupKind` enum:

| `GroupKind`      | Test                                                        | Intuition                                                                                  |
|------------------|-------------------------------------------------------------|--------------------------------------------------------------------------------------------|
| `BoundRec`       | `match_de_bruijn_rec(arena, group)` is `Some`, **or** `match_de_bruijn_ref(arena, group)` returns a level `≤ debruijn_depth` | The targeted recursion has been rebuilt by *this* transformer on the current descent path. Its body uses the interleaved `1 + N` layout. |
| `UnboundRef`     | `match_de_bruijn_ref(arena, group)` returns a level `> debruijn_depth` | The reference points at an outer recursion that this transformer has not entered on the current path. Its body still uses the original slot numbering. |
| `Other`          | Neither of the above                                        | Defensive fallback for non-de-Bruijn shapes.                                               |

Each case rewrites the projection differently:

- **`BoundRec`.** The slot arithmetic uses the interleaved layout:
  ```text
  primal  = Proj(index · L,           dual_group.primal)
  t_s_j   = Proj(index · L + 1 + j,   dual_group.tangents[j])     for j ∈ 0..N
  ```
  Note that because every tangent of `dual_group` aliases `fad_rec`,
  `dual_group.tangents[j]` is the same node as `dual_group.primal`; the
  distinction matters only for keeping the type uniform.

- **`UnboundRef`.** The targeted group has not been re-interleaved; its
  slot numbering is the original one. The primal is therefore forwarded
  unchanged and *every* tangent lane is forced to the constant `0.0`:
  ```text
  primal  = Proj(index, dual_group.primal)
  t_s_j   = 0.0                                                   for j ∈ 0..N
  ```
  This is the subtle case: the rewriter is differentiating inside an inner
  recursion, but the projection points at an enclosing recursion whose
  body has not been rebuilt by this descent. No `(1 + N)` expansion is
  available in that enclosing body, so propagating tangents through it
  would be ill-typed against the un-rewritten slot layout. Forcing the
  tangent to zero is consistent: from the inner recursion's local
  viewpoint, the outer slot is treated as an external constant on the
  current path.

- **`Other`.** The projection is propagated lane-wise:
  ```text
  primal  = Proj(index, dual_group.primal)
  t_s_j   = Proj(index, dual_group.tangents[j])                   for j ∈ 0..N
  ```
  This branch only fires for shapes outside the expected de-Bruijn-only
  flow and is kept for robustness rather than for any first-class semantic
  case.

## 6. Rewriting rules — formalisation

We summarize the rewriter as a system of structural rules. Let:

- `T[·]` denote the transformation `transform`, returning a pair
  `⟨ p ; t_0, …, t_{N-1} ⟩`;
- `d` denote the current `debruijn_depth`;
- `↑σ` denote `lift_de_bruijn` applied to seed term `σ`;
- `seeds = (σ_0, …, σ_{N-1})` denote the current seed vector;
- `L = 1 + N`.

### 6.1 Seed recognition (highest priority)

```
                 sig ≡ σ_j       (for some j ∈ 0..N)
   (Seed)  ─────────────────────────────────────────────
           T_seeds, d ⊢ sig  ⟹  ⟨ sig ; e_j ⟩
```
where `e_j` is the unit lane vector `(0,…,0,1,0,…,0)` (real `1.0` on lane
`j`, real `0.0` elsewhere). Repeated seeds set every matching lane to
`1.0`.

### 6.2 Free de Bruijn reference

```
   (Ref)   ─────────────────────────────────────────────
           T_seeds, d ⊢ DEBRUIJNREF(ℓ)  ⟹  ⟨ ref ; ref, …, ref ⟩
```

The reference is preserved primally; tangent slots alias the same node and
are reinterpreted by the enclosing `Proj`.

### 6.3 Recursive binder

```
                      seeds' = (↑σ_0, …, ↑σ_{N-1})
                T_seeds', d+1 ⊢ b_i  ⟹  ⟨ p_i ; t_i_0, …, t_i_{N-1} ⟩    for i ∈ 0..k
                body' = [ p_0, t_0_0, …, t_0_{N-1},  p_1, t_1_0, …, t_{k-1}_{N-1} ]
                R = DEBRUIJNREC(body')
   (Rec)  ──────────────────────────────────────────────────────────────────────────
           T_seeds, d ⊢ DEBRUIJNREC([b_0, …, b_{k-1}])  ⟹  ⟨ R ; R, …, R ⟩
```

Two side conditions implement the cycle-breaking and seed-lifting logic
described in §4:

1. Before the body premises are evaluated, the cache is augmented with
   `DEBRUIJNREC(b_*) ↦ ⟨ DEBRUIJNREC(b_*) ; DEBRUIJNREC(b_*), … ⟩` so that
   any back-edge `DEBRUIJNREF(1)` introduced while differentiating the
   body resolves consistently. This binding is overwritten by the rule's
   conclusion before returning to the parent.
2. The `seeds'` and depth `d+1` apply only inside the premises; the
   conclusion restores the outer `seeds` and `d`.

### 6.4 Projection

The arm distinguishes three cases. Let `T_seeds, d ⊢ G ⟹ ⟨ pG ; tG_0, …, tG_{N-1} ⟩`.

**Bound recursion.** `G` is `DEBRUIJNREC(_)`, or `G` is `DEBRUIJNREF(ℓ)`
with `ℓ ≤ d`:

```
   (Proj-Bound)  ───────────────────────────────────────────────────────────────────
                 T_seeds, d ⊢ Proj(i, G)  ⟹
                    ⟨ Proj(i·L, pG)  ;
                      Proj(i·L + 1, tG_0), …, Proj(i·L + N, tG_{N-1}) ⟩
```

**Unbound reference.** `G` is `DEBRUIJNREF(ℓ)` with `ℓ > d`:

```
   (Proj-Unbound) ──────────────────────────────────────────────────────────────────
                  T_seeds, d ⊢ Proj(i, G)  ⟹  ⟨ Proj(i, pG) ;  0.0, …, 0.0 ⟩
```

**Defensive fallback.** Any other shape:

```
   (Proj-Other)  ───────────────────────────────────────────────────────────────────
                 T_seeds, d ⊢ Proj(i, G)  ⟹
                    ⟨ Proj(i, pG)  ;  Proj(i, tG_0), …, Proj(i, tG_{N-1}) ⟩
```

### 6.5 DAG sharing

The transform is memoized: every `SigId` rewritten by `T` is cached in
`ForwardADTransform::cache`. The `(Rec)` rule's pre-insertion of a
placeholder makes the cache act simultaneously as a cycle-breaker for the
back-edges introduced by `DEBRUIJNREF(1)` inside the recursion body.

## 7. Worked example

Take the single-seed program:

```text
process = fad((2 : + ~ *(p)), p);
```

After lowering, the recursion is:

```text
R = DEBRUIJNREC([ + ( 2 , * ( DEBRUIJNREF(1) , p ) ) ])
y = Proj(0, R)
```

With `N = 1` lane (seed `p`), the rewriter:

1. Enters `R` via `(Rec)`: pre-installs the placeholder, sets
   `debruijn_depth = 1`, lifts the seed (the lifted seed is still `p`
   because `p` carries no `DEBRUIJNREF`).
2. Differentiates the body element `+ ( 2 , * ( DEBRUIJNREF(1) , p ) )`.
   Inside the multiplication, `(Seed)` fires on `p`, producing tangent
   `1.0`. `(Ref)` fires on `DEBRUIJNREF(1)`, producing the aliased
   reference. The product rule produces:
   ```
   t_mul = DEBRUIJNREF(1) · 1.0  +  ref_lane · p
   ```
   which simplifies to `DEBRUIJNREF(1) + ref_lane · p`. Addition then
   forwards `t_mul` (the `+ 2` constant contributes `0`).
3. Reconstructs `body' = [ primal_body , tangent_body ]` and produces:
   ```
   R' = DEBRUIJNREC([ + ( 2 , * ( DEBRUIJNREF(1) , p ) ) ,
                      ⟨body's tangent lane⟩ ])
   ```
4. The output projections are:
   ```
   y      = Proj(0, R')         ;    via (Proj-Bound) with i=0, L=2
   dy/dp  = Proj(1, R')         ;    via (Proj-Bound) with i=0, j=0
   ```

Conceptually the emitted recursion is the joint system:

```text
y[n]      = p · y[n-1] + 2
dy/dp[n]  = y[n-1] + p · dy/dp[n-1]
```

with one shared recursive group instead of duplicated primal shadows.

## 8. Invariants

The rules above preserve the following invariants:

- **Layout invariant.** A rebuilt recursion of original arity `k` has
  arity `k · L` with the interleaving `[p_0, t_0_*, p_1, t_1_*, …]`. Every
  `Proj` arm produced by `(Proj-Bound)` respects that layout.

- **Depth invariant.** `debruijn_depth` strictly counts the
  `DEBRUIJNREC` scopes rewritten on the current path; it is always
  matched by a corresponding decrement on body completion (`(Rec)`).

- **Seed-lifting invariant.** Inside the body of `(Rec)`, the active seed
  set is exactly the lifted snapshot, which keeps `SigId` equality the
  correct seed-recognition test even after a binder has been crossed.

- **Cycle-safety invariant.** The placeholder inserted by `(Rec)` before
  body evaluation is overwritten before returning, so no externally
  observable result projects from the placeholder; back-edges resolved
  through it are always re-projected against the rebuilt interleaved
  group through the `(Proj-Bound)` rule.

- **Soundness boundary at unbound references.** `(Proj-Unbound)` is the
  only place where a tangent is forced to zero for structural rather than
  mathematical reasons. It triggers exactly when an inner recursion
  references an outer recursion whose body has not been re-interleaved on
  the current path, and is the price paid for not eagerly rewriting
  enclosing recursions on every descent into an inner one.

## 9. Source locations

- `transform_uncached` and the `DEBRUIJNREC` arm:
  [crates/propagate/src/forward_ad.rs:540](crates/propagate/src/forward_ad.rs:540)
- `SigMatch::Proj` arm and `GroupKind` classifier:
  [crates/propagate/src/forward_ad.rs:936](crates/propagate/src/forward_ad.rs:936)
- Module-level discussion of de Bruijn handling:
  [crates/propagate/src/forward_ad.rs:262](crates/propagate/src/forward_ad.rs:262)
