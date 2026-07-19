/-
  Lean 4 specification for:

    lean-normalization-rewrites-formal-spec-proposal-2026-07-19-en.md

  Scope
  -----
  This file mechanizes the rule-cascade and canonical-term core of the
  `crates/normalize` port:

  * a first-match rule cascade with a proved earliest-rule theorem — the
    rule application order is a first-class, checkable object (the
    `needs_separate_loop` lesson applied preventively);
  * a small exact-semantics term language with per-rule soundness and
    strict size decrease proved for every rule in the cascade;
  * the `-1 · y` exception encoded structurally: no cascade rule folds a
    `-1` coefficient against a non-constant operand (guarded below);
  * the mterm/aterm canonical layer: signed-exponent factor lists, the
    signature-keyed additive merge with its proved soundness, and named
    obligations for the multiplicative merge (gate N1).

  This is the N0 skeleton of the proposal (its adequacy review is a
  separate gate). Semantics are exact over `Int`: every soundness statement
  is algebraic identity in an idealized ring, never a floating-point claim
  — numeric parity remains the impulse corpus's job.

  This file uses only Lean's bundled Std library. It contains no `sorry`
  and no axioms. Validate it with:

      lean porting/normalization-rewrites-formal-spec.lean

  Naming conventions
  ------------------
  Names ending in `B` return `Bool` and can be evaluated. Rules use `if`
  guards rather than literal patterns so every proof splits uniformly.
-/

import Std

namespace Faust.NormalizationRewrites

/-! ## Term language and exact semantics

A deliberately small expression skeleton: enough structure to state and
prove rule-cascade properties, without becoming a shadow signal language.
The production `SigId` graph maps into it through exported facts only. -/

abbrev Var := Nat

inductive Term where
  | cst (k : Int)
  | var (x : Var)
  | add (a b : Term)
  | mul (a b : Term)
  deriving Repr, DecidableEq

def eval (ρ : Var → Int) : Term → Int
  | .cst k => k
  | .var x => ρ x
  | .add a b => eval ρ a + eval ρ b
  | .mul a b => eval ρ a * eval ρ b

def size : Term → Nat
  | .cst _ => 1
  | .var _ => 1
  | .add a b => size a + size b + 1
  | .mul a b => size a + size b + 1

/-! ## Rules and the first-match cascade -/

abbrev Rule := Term → Option Term

/-- A rule is sound when firing preserves exact semantics. -/
def RuleSound (r : Rule) : Prop :=
  ∀ (ρ : Var → Int) (t t' : Term), r t = some t' → eval ρ t' = eval ρ t

/-- A rule is reducing when firing strictly decreases term size — the
termination measure of the memoized rewrite engine. -/
def RuleReducing (r : Rule) : Prop :=
  ∀ (t t' : Term), r t = some t' → size t' < size t

/-- First-match evaluation of an ordered rule list. Returns the index of
the rule that fired together with the rewritten term, so rule *selection*
(not only the result) is observable and testable against Rust. -/
def firstMatch : List Rule → Term → Option (Nat × Term)
  | [], _ => none
  | r :: rs, t =>
    match r t with
    | some t' => some (0, t')
    | none =>
      match firstMatch rs t with
      | some (i, t') => some (i + 1, t')
      | none => none

theorem memOfGetElem? : ∀ {α : Type} (l : List α) (n : Nat) (a : α),
    l[n]? = some a → a ∈ l := by
  intro α l
  induction l with
  | nil => intro n a h; simp at h
  | cons x xs ih =>
    intro n a h
    cases n with
    | zero =>
      simp at h
      exact h ▸ List.mem_cons_self
    | succ k =>
      simp at h
      exact List.mem_cons_of_mem x (ih k a h)

/-- The fired rule really is in the list at the reported index, and it
really fired. -/
theorem firstMatch_hit : ∀ (rs : List Rule) (t : Term) (i : Nat) (t' : Term),
    firstMatch rs t = some (i, t') →
    ∃ r, rs[i]? = some r ∧ r t = some t' := by
  intro rs
  induction rs with
  | nil => intro t i t' h; simp [firstMatch] at h
  | cons r rs ih =>
    intro t i t' h
    unfold firstMatch at h
    cases hr : r t with
    | some u =>
      rw [hr] at h
      simp at h
      obtain ⟨rfl, rfl⟩ := h
      exact ⟨r, by simp, hr⟩
    | none =>
      rw [hr] at h
      cases hfm : firstMatch rs t with
      | none => rw [hfm] at h; simp at h
      | some p =>
        obtain ⟨i₀, u⟩ := p
        rw [hfm] at h
        simp at h
        obtain ⟨rfl, rfl⟩ := h
        obtain ⟨r₀, hidx, hr₀⟩ := ih t i₀ u hfm
        exact ⟨r₀, by simpa using hidx, hr₀⟩

/-- The earliest-rule theorem: every rule strictly before the fired index
declined. This is the normative statement of first-match order — the
property whose silent inversion produced the `needs_separate_loop` port
bug in the scheduling stream. -/
theorem firstMatch_earliest : ∀ (rs : List Rule) (t : Term) (i : Nat) (t' : Term),
    firstMatch rs t = some (i, t') →
    ∀ (j : Nat) (r : Rule), j < i → rs[j]? = some r → r t = none := by
  intro rs
  induction rs with
  | nil => intro t i t' h; simp [firstMatch] at h
  | cons r rs ih =>
    intro t i t' h j rj hj hidx
    unfold firstMatch at h
    cases hr : r t with
    | some u =>
      rw [hr] at h
      simp at h
      omega
    | none =>
      rw [hr] at h
      cases hfm : firstMatch rs t with
      | none => rw [hfm] at h; simp at h
      | some p =>
        obtain ⟨i₀, u⟩ := p
        rw [hfm] at h
        simp at h
        obtain ⟨rfl, rfl⟩ := h
        cases j with
        | zero =>
          simp at hidx
          exact hidx ▸ hr
        | succ k =>
          simp at hidx
          exact ih t i₀ u hfm k rj (by omega) hidx

/-! ## The concrete cascade

Transcription targets: the C++ `simplify` cascade's additive/multiplicative
identity and constant-folding rules. `if` guards keep coefficients
inspectable. The `-1 · y` sign exception is encoded by *absence*: `mulOneL`
fires only for coefficient `1` and `mulZeroL` only for `0`, so
`(-1) · y` with non-constant `y` falls through the whole cascade untouched
(see the regression guard below), while two genuine constants always fold. -/

def addZeroL : Rule
  | .add (.cst k) b => if k = 0 then some b else none
  | _ => none

def addZeroR : Rule
  | .add a (.cst k) => if k = 0 then some a else none
  | _ => none

def mulZeroL : Rule
  | .mul (.cst k) _ => if k = 0 then some (.cst 0) else none
  | _ => none

def mulZeroR : Rule
  | .mul _ (.cst k) => if k = 0 then some (.cst 0) else none
  | _ => none

def mulOneL : Rule
  | .mul (.cst k) b => if k = 1 then some b else none
  | _ => none

def mulOneR : Rule
  | .mul a (.cst k) => if k = 1 then some a else none
  | _ => none

def constFoldAdd : Rule
  | .add (.cst k₁) (.cst k₂) => some (.cst (k₁ + k₂))
  | _ => none

def constFoldMul : Rule
  | .mul (.cst k₁) (.cst k₂) => some (.cst (k₁ * k₂))
  | _ => none

/-- The normative rule order. Rust `simplify` must fire the same rule
index on every input (rule-index parity, gate N2). -/
def simplifyRules : List Rule :=
  [constFoldAdd, constFoldMul, addZeroL, addZeroR,
   mulZeroL, mulZeroR, mulOneL, mulOneR]

def simplifyStepB (t : Term) : Option (Nat × Term) :=
  firstMatch simplifyRules t

/-! ## Per-rule soundness and decrease -/

theorem addZeroL_sound : RuleSound addZeroL := by
  intro ρ t t' h
  unfold addZeroL at h
  split at h
  · split at h
    · next hk => cases h; subst hk; simp [eval]
    · cases h
  · cases h

theorem addZeroR_sound : RuleSound addZeroR := by
  intro ρ t t' h
  unfold addZeroR at h
  split at h
  · split at h
    · next hk => cases h; subst hk; simp [eval]
    · cases h
  · cases h

theorem mulZeroL_sound : RuleSound mulZeroL := by
  intro ρ t t' h
  unfold mulZeroL at h
  split at h
  · split at h
    · next hk => cases h; subst hk; simp [eval]
    · cases h
  · cases h

theorem mulZeroR_sound : RuleSound mulZeroR := by
  intro ρ t t' h
  unfold mulZeroR at h
  split at h
  · split at h
    · next hk => cases h; subst hk; simp [eval]
    · cases h
  · cases h

theorem mulOneL_sound : RuleSound mulOneL := by
  intro ρ t t' h
  unfold mulOneL at h
  split at h
  · split at h
    · next hk => cases h; subst hk; simp [eval]
    · cases h
  · cases h

theorem mulOneR_sound : RuleSound mulOneR := by
  intro ρ t t' h
  unfold mulOneR at h
  split at h
  · split at h
    · next hk => cases h; subst hk; simp [eval]
    · cases h
  · cases h

theorem constFoldAdd_sound : RuleSound constFoldAdd := by
  intro ρ t t' h
  unfold constFoldAdd at h
  split at h
  · cases h; simp [eval]
  · cases h

theorem constFoldMul_sound : RuleSound constFoldMul := by
  intro ρ t t' h
  unfold constFoldMul at h
  split at h
  · cases h; simp [eval]
  · cases h

theorem addZeroL_reducing : RuleReducing addZeroL := by
  intro t t' h
  unfold addZeroL at h
  split at h
  · split at h
    · cases h; simp only [size]; omega
    · cases h
  · cases h

theorem addZeroR_reducing : RuleReducing addZeroR := by
  intro t t' h
  unfold addZeroR at h
  split at h
  · split at h
    · cases h; simp only [size]; omega
    · cases h
  · cases h

theorem mulZeroL_reducing : RuleReducing mulZeroL := by
  intro t t' h
  unfold mulZeroL at h
  split at h
  · split at h
    · cases h; simp only [size]; omega
    · cases h
  · cases h

theorem mulZeroR_reducing : RuleReducing mulZeroR := by
  intro t t' h
  unfold mulZeroR at h
  split at h
  · split at h
    · cases h; simp only [size]; omega
    · cases h
  · cases h

theorem mulOneL_reducing : RuleReducing mulOneL := by
  intro t t' h
  unfold mulOneL at h
  split at h
  · split at h
    · cases h; simp only [size]; omega
    · cases h
  · cases h

theorem mulOneR_reducing : RuleReducing mulOneR := by
  intro t t' h
  unfold mulOneR at h
  split at h
  · split at h
    · cases h; simp only [size]; omega
    · cases h
  · cases h

theorem constFoldAdd_reducing : RuleReducing constFoldAdd := by
  intro t t' h
  unfold constFoldAdd at h
  split at h
  · cases h; simp only [size]; omega
  · cases h

theorem constFoldMul_reducing : RuleReducing constFoldMul := by
  intro t t' h
  unfold constFoldMul at h
  split at h
  · cases h; simp only [size]; omega
  · cases h

theorem allRulesSound : ∀ r ∈ simplifyRules, RuleSound r := by
  intro r hr
  simp [simplifyRules] at hr
  obtain rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl := hr
  · exact constFoldAdd_sound
  · exact constFoldMul_sound
  · exact addZeroL_sound
  · exact addZeroR_sound
  · exact mulZeroL_sound
  · exact mulZeroR_sound
  · exact mulOneL_sound
  · exact mulOneR_sound

theorem allRulesReducing : ∀ r ∈ simplifyRules, RuleReducing r := by
  intro r hr
  simp [simplifyRules] at hr
  obtain rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl := hr
  · exact constFoldAdd_reducing
  · exact constFoldMul_reducing
  · exact addZeroL_reducing
  · exact addZeroR_reducing
  · exact mulZeroL_reducing
  · exact mulZeroR_reducing
  · exact mulOneL_reducing
  · exact mulOneR_reducing

/-- Whole-cascade soundness: one simplification step preserves exact
semantics, whichever rule fired. -/
theorem simplifyStepB_sound {t : Term} {i : Nat} {t' : Term}
    (h : simplifyStepB t = some (i, t')) (ρ : Var → Int) :
    eval ρ t' = eval ρ t := by
  obtain ⟨r, hidx, hr⟩ := firstMatch_hit _ _ _ _ h
  exact allRulesSound r (memOfGetElem? _ _ _ hidx) ρ t t' hr

/-- Whole-cascade termination measure: one step strictly shrinks the term,
so the memoized engine's iteration is well-founded. -/
theorem simplifyStepB_reducing {t : Term} {i : Nat} {t' : Term}
    (h : simplifyStepB t = some (i, t')) : size t' < size t := by
  obtain ⟨r, hidx, hr⟩ := firstMatch_hit _ _ _ _ h
  exact allRulesReducing r (memOfGetElem? _ _ _ hidx) t t' hr

/-! ## The mterm/aterm canonical layer

`Factors` mirrors `mterm.rs`: non-constant factors with exponents, kept in
strictly ascending variable order (`canonB`). The skeleton uses `Nat`
exponents (numerator side); signed exponents — the denominator side — join
with the division obligations at gate N1. `Aterm` mirrors `aterm.rs`: an
association list keyed by signature, so "equal signatures merge by adding
coefficients" is provable, not asserted. -/

abbrev Factors := List (Var × Nat)

def Factors.canonB : Factors → Bool
  | [] => true
  | [(_, e)] => decide (0 < e)
  | (x, e) :: (y, d) :: rest =>
    decide (0 < e) && decide (x < y) && Factors.canonB ((y, d) :: rest)

def evalFactors (ρ : Var → Int) : Factors → Int
  | [] => 1
  | (x, e) :: fs => ρ x ^ e * evalFactors ρ fs

structure Mterm where
  coef : Int
  factors : Factors
  deriving Repr, DecidableEq

def Mterm.eval (ρ : Var → Int) (m : Mterm) : Int :=
  m.coef * evalFactors ρ m.factors

/-- Sorted merge with exponent addition — the C++ `mterm::operator*=`
factor walk. -/
def mulFactors : Factors → Factors → Factors
  | [], g => g
  | f, [] => f
  | (x, e) :: fs, (y, d) :: gs =>
    if x < y then (x, e) :: mulFactors fs ((y, d) :: gs)
    else if y < x then (y, d) :: mulFactors ((x, e) :: fs) gs
    else (x, e + d) :: mulFactors fs gs
  termination_by f g => f.length + g.length

def Mterm.mul (m₁ m₂ : Mterm) : Mterm :=
  ⟨m₁.coef * m₂.coef, mulFactors m₁.factors m₂.factors⟩

/-- N1 obligation: the factor merge is multiplicative under exact
semantics (needs `x^(e+d) = x^e * x^d` and commutativity juggling). -/
def MulFactorsSound : Prop :=
  ∀ (ρ : Var → Int) (f g : Factors),
    evalFactors ρ (mulFactors f g) = evalFactors ρ f * evalFactors ρ g

/-- N1 obligation: the factor merge preserves canonical form. -/
def MulFactorsCanonical : Prop :=
  ∀ (f g : Factors), f.canonB = true → g.canonB = true →
    (mulFactors f g).canonB = true

/-- An aterm is an association list from signature (the mterm minus its
coefficient) to coefficient. -/
abbrev Aterm := List (Factors × Int)

def Aterm.eval (ρ : Var → Int) : Aterm → Int
  | [] => 0
  | (sig, k) :: rest => k * evalFactors ρ sig + Aterm.eval ρ rest

/-- The signature-keyed additive merge — C++ `aterm::operator+=(mterm)`:
an existing signature absorbs the coefficient, a new signature is
appended. -/
def Aterm.addMterm : Aterm → Mterm → Aterm
  | [], m => [(m.factors, m.coef)]
  | (sig, k) :: rest, m =>
    if sig = m.factors then (sig, k + m.coef) :: rest
    else (sig, k) :: Aterm.addMterm rest m

/-- Proved N0 anchor: merging a mterm into an aterm adds exactly its value
— "terms with identical signatures merge by adding their coefficients" is
sound, not just a doc-comment invariant. -/
theorem addMterm_sound : ∀ (a : Aterm) (m : Mterm) (ρ : Var → Int),
    Aterm.eval ρ (a.addMterm m) = Aterm.eval ρ a + m.eval ρ := by
  intro a
  induction a with
  | nil =>
    intro m ρ
    simp [Aterm.addMterm, Aterm.eval, Mterm.eval]
  | cons hd rest ih =>
    intro m ρ
    obtain ⟨sig, k⟩ := hd
    unfold Aterm.addMterm
    split
    · next heq =>
      simp only [Aterm.eval, Mterm.eval, heq, Int.add_mul]
      generalize evalFactors ρ m.factors = F
      generalize Aterm.eval ρ rest = R
      omega
    · next hne =>
      simp only [Aterm.eval, Mterm.eval, ih m ρ]
      generalize evalFactors ρ sig = S
      generalize evalFactors ρ m.factors = F
      generalize Aterm.eval ρ rest = R
      omega

/-! ## Regression fixtures

Rule-index guards freeze the cascade order; the `-1 · y` guard freezes the
sign-form exception; the mterm/aterm guards mirror `mterm.rs`/`aterm.rs`
unit cases. -/

-- constant folding fires first (index 0 / 1), even for -1 constants
#guard simplifyStepB (.add (.cst 2) (.cst 3)) = some (0, .cst 5)
#guard simplifyStepB (.mul (.cst (-1)) (.cst 3)) = some (1, .cst (-3))
-- identity rules and their indices
#guard simplifyStepB (.add (.cst 0) (.var 0)) = some (2, .var 0)
#guard simplifyStepB (.add (.var 0) (.cst 0)) = some (3, .var 0)
#guard simplifyStepB (.mul (.cst 0) (.var 0)) = some (4, .cst 0)
#guard simplifyStepB (.mul (.var 0) (.cst 0)) = some (5, .cst 0)
#guard simplifyStepB (.mul (.cst 1) (.var 0)) = some (6, .var 0)
#guard simplifyStepB (.mul (.var 0) (.cst 1)) = some (7, .var 0)
-- the -1 · y exception: the cascade must leave the sign form untouched
#guard simplifyStepB (.mul (.cst (-1)) (.var 0)) = none
-- no rule fires on an irreducible term
#guard simplifyStepB (.add (.var 0) (.var 1)) = none
-- factor merge: x^1 · (x^2 · y^1) = x^3 · y^1, order kept
#guard mulFactors [(0, 1)] [(0, 2), (1, 1)] = [(0, 3), (1, 1)]
#guard Factors.canonB [(0, 3), (1, 1)] = true
#guard Factors.canonB [(1, 1), (0, 3)] = false
#guard Factors.canonB [(0, 0)] = false
-- aterm merge: equal signature adds coefficients
#guard Aterm.addMterm [([(0, 1)], 2)] ⟨3, [(0, 1)]⟩ = [([(0, 1)], 5)]
-- distinct signature appends
#guard Aterm.addMterm [([(0, 1)], 2)] ⟨3, [(1, 1)]⟩ =
  [([(0, 1)], 2), ([(1, 1)], 3)]
-- exact semantics fixture: 2 · x with x = 5, merged twice → 25... no: 2·5 + 3·5
#guard Aterm.eval (fun _ => 5) (Aterm.addMterm [([(0, 1)], 2)] ⟨3, [(0, 1)]⟩) = 25

end Faust.NormalizationRewrites
