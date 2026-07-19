/-
  Lean 4 specification for:

    lean-bda-typing-formal-spec-proposal-2026-07-19-en.md

  Scope
  -----
  This file mechanizes the arity typing core of the Faust block-diagram
  algebra as ported in `crates/propagate/src/arity.rs`:

  * the box syntax skeleton (composition operators, De Bruijn references,
    and the port-authored FAD/RAD extensions);
  * the arity typing judgment `HasArity` and its executable checker
    `boxArityB`;
  * the wiring view `boxArityWB` that types `fad` transparently
    (Rust `box_arity_wiring`, used by `RecFadMode::ExpandAfterRec`);
  * De Bruijn well-scopedness `WellScoped` / `wellScopedB`;
  * soundness, completeness, and functionality theorems binding the
    executable checkers to the relational judgments.

  This is the B0 skeleton of the proposal (its adequacy review is a separate
  gate). It deliberately does not model evaluation, pattern matching, UI
  widgets, or signal semantics: `Box` is an arity skeleton, not a shadow AST.

  This file uses only Lean's bundled Std library. It contains no `sorry` and
  no axioms. Validate it with:

      lean porting/bda-typing-formal-spec.lean

  Naming conventions
  ------------------
  Names ending in `B` return `Bool` (or decidable data) and can be evaluated.
  The recursion constructor is spelled `recur` because `rec` would collide
  with the recursor Lean generates for every inductive type. Divisibility
  side conditions are stated as `_ % _ = 0` in both the judgment and the
  checker so no dvd/mod bridging is needed.
-/

import Std

namespace Faust.BdaTyping

/-! ## Syntax

`prim m n` abstracts the primitive alphabet by its declared arity: binary
arithmetic is `prim 2 1`, a constant is `const` (arity `(0, 1)`), and so on.
`ref i` is a flat single-lane De Bruijn recursion reference (arity `(0, 1)`;
its index is governed by `WellScoped`, not by the arity judgment). -/

inductive Box where
  | const                     -- numeric literal, arity (0, 1)
  | wire                      -- `_`, arity (1, 1)
  | cut                       -- `!`, arity (1, 0)
  | prim (ins outs : Nat)     -- primitive with declared arity
  | seq (a b : Box)           -- A : B
  | par (a b : Box)           -- A , B
  | split (a b : Box)         -- A <: B
  | merge (a b : Box)         -- A :> B
  | recur (a b : Box)         -- A ~ B
  | ref (i : Nat)             -- De Bruijn recursion reference
  | fad (body seed : Box)     -- fad(expr, seed) — port extension
  | rad (body seed : Box)     -- rad(expr, seeds) — port extension
  deriving Repr, DecidableEq

/-! ## The arity judgment

`HasArity b m n` reads "box `b` consumes `m` inputs and produces `n`
outputs". The five composition rules follow the C++ `getBoxType` contract;
`fad`/`rad` transcribe `propagate/src/arity.rs` (C++ `boxtype.cpp:371` for
`fad`): a body `(m, n)` with a seed of `k` outputs yields `n * (1 + k)`
outputs under `fad` (one primal plus one tangent lane per seed output) and
`n + k` under `rad` (primals first, then one gradient lane per seed
output). Seed inputs are not constrained by the arity rule itself. -/

inductive HasArity : Box → Nat → Nat → Prop where
  | const : HasArity .const 0 1
  | wire : HasArity .wire 1 1
  | cut : HasArity .cut 1 0
  | prim {m n} : HasArity (.prim m n) m n
  | ref {i} : HasArity (.ref i) 0 1
  | seq {a b m n q} :
      HasArity a m n → HasArity b n q → HasArity (.seq a b) m q
  | par {a b m n p q} :
      HasArity a m n → HasArity b p q → HasArity (.par a b) (m + p) (n + q)
  | split {a b m n p q} :
      HasArity a m n → HasArity b p q → 0 < n → p % n = 0 →
      HasArity (.split a b) m q
  | merge {a b m n p q} :
      HasArity a m n → HasArity b p q → 0 < p → n % p = 0 →
      HasArity (.merge a b) m q
  | recur {a b m n p q} :
      HasArity a m n → HasArity b p q → q ≤ m → p ≤ n →
      HasArity (.recur a b) (m - q) n
  | fad {a s m n p k} :
      HasArity a m n → HasArity s p k → HasArity (.fad a s) m (n * (1 + k))
  | rad {a s m n p k} :
      HasArity a m n → HasArity s p k → HasArity (.rad a s) m (n + k)

/-! ## The executable checker (typed view) -/

def boxArityB : Box → Option (Nat × Nat)
  | .const => some (0, 1)
  | .wire => some (1, 1)
  | .cut => some (1, 0)
  | .prim m n => some (m, n)
  | .ref _ => some (0, 1)
  | .seq a b =>
    match boxArityB a, boxArityB b with
    | some (m, n), some (p, q) => if n = p then some (m, q) else none
    | _, _ => none
  | .par a b =>
    match boxArityB a, boxArityB b with
    | some (m, n), some (p, q) => some (m + p, n + q)
    | _, _ => none
  | .split a b =>
    match boxArityB a, boxArityB b with
    | some (m, n), some (p, q) =>
      if 0 < n ∧ p % n = 0 then some (m, q) else none
    | _, _ => none
  | .merge a b =>
    match boxArityB a, boxArityB b with
    | some (m, n), some (p, q) =>
      if 0 < p ∧ n % p = 0 then some (m, q) else none
    | _, _ => none
  | .recur a b =>
    match boxArityB a, boxArityB b with
    | some (m, n), some (p, q) =>
      if q ≤ m ∧ p ≤ n then some (m - q, n) else none
    | _, _ => none
  | .fad a s =>
    match boxArityB a, boxArityB s with
    | some (m, n), some (_, k) => some (m, n * (1 + k))
    | _, _ => none
  | .rad a s =>
    match boxArityB a, boxArityB s with
    | some (m, n), some (_, k) => some (m, n + k)
    | _, _ => none

/-! ## Checker completeness, soundness, and functionality

Completeness and soundness anchor `boxArityB` to the relational judgment;
functionality (`HasArity` assigns at most one arity per box — the property
the vector-stream adequacy review showed must be proved, never assumed) is
then a corollary via injectivity of `some`. -/

theorem boxArityB_complete {b m n} (h : HasArity b m n) :
    boxArityB b = some (m, n) := by
  induction h with
  | const => rfl
  | wire => rfl
  | cut => rfl
  | prim => rfl
  | ref => rfl
  | seq _ _ iha ihb => simp [boxArityB, iha, ihb]
  | par _ _ iha ihb => simp [boxArityB, iha, ihb]
  | split _ _ hn hmod iha ihb => simp [boxArityB, iha, ihb, hn, hmod]
  | merge _ _ hp hmod iha ihb => simp [boxArityB, iha, ihb, hp, hmod]
  | recur _ _ hq hp iha ihb => simp [boxArityB, iha, ihb, hq, hp]
  | fad _ _ iha ihs => simp [boxArityB, iha, ihs]
  | rad _ _ iha ihs => simp [boxArityB, iha, ihs]

theorem boxArityB_sound : ∀ b {m n}, boxArityB b = some (m, n) → HasArity b m n := by
  intro b
  induction b with
  | const =>
    intro m n h
    simp [boxArityB] at h
    obtain ⟨rfl, rfl⟩ := h
    exact .const
  | wire =>
    intro m n h
    simp [boxArityB] at h
    obtain ⟨rfl, rfl⟩ := h
    exact .wire
  | cut =>
    intro m n h
    simp [boxArityB] at h
    obtain ⟨rfl, rfl⟩ := h
    exact .cut
  | prim mm nn =>
    intro m n h
    simp [boxArityB] at h
    obtain ⟨rfl, rfl⟩ := h
    exact .prim
  | ref i =>
    intro m n h
    simp [boxArityB] at h
    obtain ⟨rfl, rfl⟩ := h
    exact .ref
  | seq a b iha ihb =>
    intro m n h
    unfold boxArityB at h
    split at h
    next m₁ n₁ p₁ q₁ ha hb =>
      split at h
      next hcond =>
        cases h
        exact .seq (iha ha) (hcond ▸ ihb hb)
      next => cases h
    next => cases h
  | par a b iha ihb =>
    intro m n h
    unfold boxArityB at h
    split at h
    next m₁ n₁ p₁ q₁ ha hb =>
      cases h
      exact .par (iha ha) (ihb hb)
    next => cases h
  | split a b iha ihb =>
    intro m n h
    unfold boxArityB at h
    split at h
    next m₁ n₁ p₁ q₁ ha hb =>
      split at h
      next hcond =>
        cases h
        exact .split (iha ha) (ihb hb) hcond.1 hcond.2
      next => cases h
    next => cases h
  | merge a b iha ihb =>
    intro m n h
    unfold boxArityB at h
    split at h
    next m₁ n₁ p₁ q₁ ha hb =>
      split at h
      next hcond =>
        cases h
        exact .merge (iha ha) (ihb hb) hcond.1 hcond.2
      next => cases h
    next => cases h
  | recur a b iha ihb =>
    intro m n h
    unfold boxArityB at h
    split at h
    next m₁ n₁ p₁ q₁ ha hb =>
      split at h
      next hcond =>
        cases h
        exact .recur (iha ha) (ihb hb) hcond.1 hcond.2
      next => cases h
    next => cases h
  | fad a s iha ihs =>
    intro m n h
    unfold boxArityB at h
    split at h
    next m₁ n₁ p₁ k₁ ha hs =>
      cases h
      exact .fad (iha ha) (ihs hs)
    next => cases h
  | rad a s iha ihs =>
    intro m n h
    unfold boxArityB at h
    split at h
    next m₁ n₁ p₁ k₁ ha hs =>
      cases h
      exact .rad (iha ha) (ihs hs)
    next => cases h

theorem boxArityB_iff {b m n} : boxArityB b = some (m, n) ↔ HasArity b m n :=
  ⟨boxArityB_sound b, boxArityB_complete⟩

theorem hasArity_functional {b m n m' n'}
    (h : HasArity b m n) (h' : HasArity b m' n') : m = m' ∧ n = n' := by
  have e := (boxArityB_complete h).symm.trans (boxArityB_complete h')
  simp at e
  exact e

/-! ## The wiring view

`boxArityWB` types `fad` transparently (body arity passes through); it is
the Lean counterpart of Rust `box_arity_wiring`, used where the recursive
port algebra is computed on primal lanes only. On `fad`-free boxes the two
views coincide. The general input-arity agreement on arbitrary boxes is
stated as an explicit obligation for gate B2. -/

def boxArityWB : Box → Option (Nat × Nat)
  | .fad a _ => boxArityWB a
  | .const => some (0, 1)
  | .wire => some (1, 1)
  | .cut => some (1, 0)
  | .prim m n => some (m, n)
  | .ref _ => some (0, 1)
  | .seq a b =>
    match boxArityWB a, boxArityWB b with
    | some (m, n), some (p, q) => if n = p then some (m, q) else none
    | _, _ => none
  | .par a b =>
    match boxArityWB a, boxArityWB b with
    | some (m, n), some (p, q) => some (m + p, n + q)
    | _, _ => none
  | .split a b =>
    match boxArityWB a, boxArityWB b with
    | some (m, n), some (p, q) =>
      if 0 < n ∧ p % n = 0 then some (m, q) else none
    | _, _ => none
  | .merge a b =>
    match boxArityWB a, boxArityWB b with
    | some (m, n), some (p, q) =>
      if 0 < p ∧ n % p = 0 then some (m, q) else none
    | _, _ => none
  | .recur a b =>
    match boxArityWB a, boxArityWB b with
    | some (m, n), some (p, q) =>
      if q ≤ m ∧ p ≤ n then some (m - q, n) else none
    | _, _ => none
  | .rad a s =>
    match boxArityWB a, boxArityWB s with
    | some (m, n), some (_, k) => some (m, n + k)
    | _, _ => none

/-- `true` iff the box contains no `fad` node. -/
def fadFreeB : Box → Bool
  | .const | .wire | .cut | .prim _ _ | .ref _ => true
  | .seq a b | .par a b | .split a b | .merge a b | .recur a b | .rad a b =>
    fadFreeB a && fadFreeB b
  | .fad _ _ => false

theorem wiring_eq_typed_of_fadFree :
    ∀ b, fadFreeB b = true → boxArityWB b = boxArityB b := by
  intro b
  induction b with
  | const => intro _; rfl
  | wire => intro _; rfl
  | cut => intro _; rfl
  | prim m n => intro _; rfl
  | ref i => intro _; rfl
  | seq a b iha ihb =>
    intro h
    simp [fadFreeB] at h
    simp [boxArityWB, boxArityB, iha h.1, ihb h.2]
  | par a b iha ihb =>
    intro h
    simp [fadFreeB] at h
    simp [boxArityWB, boxArityB, iha h.1, ihb h.2]
  | split a b iha ihb =>
    intro h
    simp [fadFreeB] at h
    simp [boxArityWB, boxArityB, iha h.1, ihb h.2]
  | merge a b iha ihb =>
    intro h
    simp [fadFreeB] at h
    simp [boxArityWB, boxArityB, iha h.1, ihb h.2]
  | recur a b iha ihb =>
    intro h
    simp [fadFreeB] at h
    simp [boxArityWB, boxArityB, iha h.1, ihb h.2]
  | fad a s iha ihs =>
    intro h
    simp [fadFreeB] at h
  | rad a s iha ihs =>
    intro h
    simp [fadFreeB] at h
    simp [boxArityWB, boxArityB, iha h.1, ihs h.2]

/-- B2 obligation: on any box where both views succeed, input arities agree
(outputs may differ only through `fad` expansion). Proved at gate B2. -/
def WiringInputAgreement : Prop :=
  ∀ b m n m' n', boxArityB b = some (m, n) → boxArityWB b = some (m', n') →
    m = m'

/-! ## De Bruijn well-scopedness

`WellScoped d b` reads "under `d` enclosing recursion binders, every
reference in `b` is bound". Following the flat single-binder convention of
the De Bruijn lowering notes, `recur a b` binds one level for both
branches. -/

inductive WellScoped : Nat → Box → Prop where
  | const {d} : WellScoped d .const
  | wire {d} : WellScoped d .wire
  | cut {d} : WellScoped d .cut
  | prim {d m n} : WellScoped d (.prim m n)
  | ref {d i} : i < d → WellScoped d (.ref i)
  | seq {d a b} : WellScoped d a → WellScoped d b → WellScoped d (.seq a b)
  | par {d a b} : WellScoped d a → WellScoped d b → WellScoped d (.par a b)
  | split {d a b} : WellScoped d a → WellScoped d b → WellScoped d (.split a b)
  | merge {d a b} : WellScoped d a → WellScoped d b → WellScoped d (.merge a b)
  | recur {d a b} :
      WellScoped (d + 1) a → WellScoped (d + 1) b → WellScoped d (.recur a b)
  | fad {d a s} : WellScoped d a → WellScoped d s → WellScoped d (.fad a s)
  | rad {d a s} : WellScoped d a → WellScoped d s → WellScoped d (.rad a s)

def wellScopedB (d : Nat) : Box → Bool
  | .const | .wire | .cut | .prim _ _ => true
  | .ref i => i < d
  | .seq a b | .par a b | .split a b | .merge a b | .fad a b | .rad a b =>
    wellScopedB d a && wellScopedB d b
  | .recur a b => wellScopedB (d + 1) a && wellScopedB (d + 1) b

theorem wellScopedB_iff : ∀ b d, wellScopedB d b = true ↔ WellScoped d b := by
  intro b
  induction b with
  | const =>
    intro d
    simp [wellScopedB]
    exact .const
  | wire =>
    intro d
    simp [wellScopedB]
    exact .wire
  | cut =>
    intro d
    simp [wellScopedB]
    exact .cut
  | prim m n =>
    intro d
    simp [wellScopedB]
    exact .prim
  | ref i =>
    intro d
    constructor
    · intro h
      simp [wellScopedB] at h
      exact .ref h
    · intro h
      cases h with
      | ref hi =>
        simp [wellScopedB]
        exact hi
  | seq a b iha ihb =>
    intro d
    constructor
    · intro h
      simp [wellScopedB] at h
      exact .seq ((iha d).mp h.1) ((ihb d).mp h.2)
    · intro h
      cases h with
      | seq ha hb =>
        simp [wellScopedB]
        exact ⟨(iha d).mpr ha, (ihb d).mpr hb⟩
  | par a b iha ihb =>
    intro d
    constructor
    · intro h
      simp [wellScopedB] at h
      exact .par ((iha d).mp h.1) ((ihb d).mp h.2)
    · intro h
      cases h with
      | par ha hb =>
        simp [wellScopedB]
        exact ⟨(iha d).mpr ha, (ihb d).mpr hb⟩
  | split a b iha ihb =>
    intro d
    constructor
    · intro h
      simp [wellScopedB] at h
      exact .split ((iha d).mp h.1) ((ihb d).mp h.2)
    · intro h
      cases h with
      | split ha hb =>
        simp [wellScopedB]
        exact ⟨(iha d).mpr ha, (ihb d).mpr hb⟩
  | merge a b iha ihb =>
    intro d
    constructor
    · intro h
      simp [wellScopedB] at h
      exact .merge ((iha d).mp h.1) ((ihb d).mp h.2)
    · intro h
      cases h with
      | merge ha hb =>
        simp [wellScopedB]
        exact ⟨(iha d).mpr ha, (ihb d).mpr hb⟩
  | recur a b iha ihb =>
    intro d
    constructor
    · intro h
      simp [wellScopedB] at h
      exact .recur ((iha (d + 1)).mp h.1) ((ihb (d + 1)).mp h.2)
    · intro h
      cases h with
      | recur ha hb =>
        simp [wellScopedB]
        exact ⟨(iha (d + 1)).mpr ha, (ihb (d + 1)).mpr hb⟩
  | fad a s iha ihs =>
    intro d
    constructor
    · intro h
      simp [wellScopedB] at h
      exact .fad ((iha d).mp h.1) ((ihs d).mp h.2)
    · intro h
      cases h with
      | fad ha hs =>
        simp [wellScopedB]
        exact ⟨(iha d).mpr ha, (ihs d).mpr hs⟩
  | rad a s iha ihs =>
    intro d
    constructor
    · intro h
      simp [wellScopedB] at h
      exact .rad ((iha d).mp h.1) ((ihs d).mp h.2)
    · intro h
      cases h with
      | rad ha hs =>
        simp [wellScopedB]
        exact ⟨(iha d).mpr ha, (ihs d).mpr hs⟩

/-! ## Regression fixtures

Small hand-typed boxes, including the accumulator `1 : +~_` and the
FAD/RAD-distinguishing case (body 2 outputs, seed 1 output: fad 4, rad 3).
The failure guards keep every side condition reachable. -/

-- 1 : +~_  (the accumulator): (0, 1)
#guard boxArityB (.seq .const (.recur (.prim 2 1) .wire)) = some (0, 1)
-- A ~ B port arithmetic: (2,1) ~ (1,1) → (1, 1)
#guard boxArityB (.recur (.prim 2 1) .wire) = some (1, 1)
-- parallel arities add
#guard boxArityB (.par .wire (.prim 2 1)) = some (3, 2)
-- split needs n ∣ p: (1 out) <: (2 in) is fine
#guard boxArityB (.split .wire (.prim 2 1)) = some (1, 1)
-- merge needs p ∣ n: (2 out) :> (1 in) is fine
#guard boxArityB (.merge (.prim 1 2) .wire) = some (1, 1)
-- fad expands outputs: body (1,2), seed (1,1) → (1, 4)
#guard boxArityB (.fad (.prim 1 2) .wire) = some (1, 4)
-- rad appends gradient lanes: body (1,2), seed (1,1) → (1, 3)
#guard boxArityB (.rad (.prim 1 2) .wire) = some (1, 3)
-- wiring view is transparent on fad
#guard boxArityWB (.fad (.prim 1 2) .wire) = some (1, 2)
-- seq arity mismatch fails
#guard boxArityB (.seq .const (.prim 2 1)) = none
-- split with zero fan-out source fails
#guard boxArityB (.split .cut .wire) = none
-- split non-divisibility fails: (2 out) <: (3 in)
#guard boxArityB (.split (.prim 1 2) (.prim 3 1)) = none
-- merge non-divisibility fails: (3 out) :> (2 in)
#guard boxArityB (.merge (.prim 1 3) (.prim 2 1)) = none
-- rec port mismatch fails: B produces more than A consumes
#guard boxArityB (.recur .const (.prim 1 2)) = none
-- scoping: ref 0 needs one enclosing rec
#guard wellScopedB 0 (.ref 0) = false
#guard wellScopedB 0 (.recur (.seq (.ref 0) .wire) .wire) = true
#guard wellScopedB 0 (.recur (.ref 1) .wire) = false

end Faust.BdaTyping
