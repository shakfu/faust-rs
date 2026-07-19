/-
  Lean 4 specification for:

    lean-interval-arithmetic-formal-spec-proposal-2026-07-19-en.md

  Scope
  -----
  This file mechanizes the inclusion core of the interval domain ported in
  `crates/interval/`:

  * the interval type with the ordered-bounds well-formedness invariant and
    the empty interval as `Option` bottom (`IvL`);
  * the membership and subset contracts with executable checkers;
  * the single normative inclusion property, proved for the linear core
    (`neg`, `add`, `sub`, `union`, `inter`) and for the generic monotone
    unary transfer (with its tightness lemma);
  * named `Prop` obligations for the nonlinear transfers (`mul`, `div`),
    whose sign case analyses are gate I2 work;
  * the R-declared regime: the `missing.rs` placeholders carry no soundness
    claim, recorded here as an explicit obligation.

  This is the I0 skeleton of the proposal (its adequacy review is a separate
  gate). The formal domain uses `Int` endpoints: the proved statements are
  the order-theoretic content of the C++/Rust algebra, which is what the
  linear core exercises. The `R-real` regime over real endpoints and the
  `lsb` precision ledger are deliberately out of this skeleton; the f64
  endpoint gap is monitored by the Rust property bridge (proposal §4), not
  by this file.

  This file uses only Lean's bundled Std library. It contains no `sorry` and
  no axioms. Validate it with:

      lean porting/interval-arithmetic-formal-spec.lean

  Naming conventions
  ------------------
  Names ending in `B` return `Bool` and can be evaluated. `imin`/`imax` are
  local if-based definitions so every lattice proof discharges by `split`
  followed by `omega`.
-/

import Std

namespace Faust.IntervalArith

/-! ## Domain -/

/-- A non-empty interval candidate: `[lo, hi]` over `Int` endpoints.
Well-formedness (`lo ≤ hi`) is a separate invariant, as in the Rust type
where the empty interval is a distinguished state. -/
structure Iv where
  lo : Int
  hi : Int
  deriving Repr, DecidableEq

/-- The interval lattice with an explicit bottom: `none` is the empty
interval (Rust encodes emptiness via NaN bounds; the model uses `Option`). -/
abbrev IvL := Option Iv

def Iv.Wf (i : Iv) : Prop := i.lo ≤ i.hi

def Iv.wfB (i : Iv) : Bool := decide (i.lo ≤ i.hi)

theorem Iv.wfB_iff {i : Iv} : i.wfB = true ↔ i.Wf := by
  simp [Iv.wfB, Iv.Wf]

/-! ## Membership and subset -/

def Mem (x : Int) (i : Iv) : Prop := i.lo ≤ x ∧ x ≤ i.hi

def memB (x : Int) (i : Iv) : Bool := decide (i.lo ≤ x) && decide (x ≤ i.hi)

theorem memB_iff {x : Int} {i : Iv} : memB x i = true ↔ Mem x i := by
  simp [memB, Mem]

def Subset (i j : Iv) : Prop := j.lo ≤ i.lo ∧ i.hi ≤ j.hi

def subsetB (i j : Iv) : Bool := decide (j.lo ≤ i.lo) && decide (i.hi ≤ j.hi)

theorem subsetB_iff {i j : Iv} : subsetB i j = true ↔ Subset i j := by
  simp [subsetB, Subset]

theorem mem_of_subset {x : Int} {i j : Iv} (hs : Subset i j) (hx : Mem x i) :
    Mem x j := ⟨Int.le_trans hs.1 hx.1, Int.le_trans hx.2 hs.2⟩

/-! ## Local min/max

If-based so that every proof below reduces to `split <;> omega`. -/

def imin (a b : Int) : Int := if a ≤ b then a else b
def imax (a b : Int) : Int := if a ≤ b then b else a

theorem imin_le_left {a b : Int} : imin a b ≤ a := by
  simp only [imin]; split <;> omega

theorem imin_le_right {a b : Int} : imin a b ≤ b := by
  simp only [imin]; split <;> omega

theorem le_imax_left {a b : Int} : a ≤ imax a b := by
  simp only [imax]; split <;> omega

theorem le_imax_right {a b : Int} : b ≤ imax a b := by
  simp only [imax]; split <;> omega

/-! ## The linear transfer core

The single normative contract is inclusion: every pointwise result of the
concrete operator lies in the transferred interval. The linear operators
are proved here; each also preserves well-formedness. -/

def neg (i : Iv) : Iv := ⟨-i.hi, -i.lo⟩

def add (i j : Iv) : Iv := ⟨i.lo + j.lo, i.hi + j.hi⟩

def sub (i j : Iv) : Iv := add i (neg j)

def union (i j : Iv) : Iv := ⟨imin i.lo j.lo, imax i.hi j.hi⟩

/-- Intersection lands in the lattice: it is the first operator that can
produce the empty interval. -/
def inter (i j : Iv) : IvL :=
  if imax i.lo j.lo ≤ imin i.hi j.hi then
    some ⟨imax i.lo j.lo, imin i.hi j.hi⟩
  else none

theorem neg_sound {x : Int} {i : Iv} (hx : Mem x i) : Mem (-x) (neg i) := by
  simp [Mem, neg] at *; omega

theorem add_sound {x y : Int} {i j : Iv} (hx : Mem x i) (hy : Mem y j) :
    Mem (x + y) (add i j) := by
  simp [Mem, add] at *; omega

theorem sub_sound {x y : Int} {i j : Iv} (hx : Mem x i) (hy : Mem y j) :
    Mem (x - y) (sub i j) := by
  have := add_sound hx (neg_sound hy)
  simp [Mem, add, neg, sub] at *; omega

theorem add_wf {i j : Iv} (hi : i.Wf) (hj : j.Wf) : (add i j).Wf := by
  simp [Iv.Wf, add] at *; omega

theorem neg_wf {i : Iv} (hi : i.Wf) : (neg i).Wf := by
  simp [Iv.Wf, neg] at *; omega

theorem union_sound_left {x : Int} {i j : Iv} (hx : Mem x i) :
    Mem x (union i j) := by
  simp only [Mem, union] at *
  simp only [imin, imax]
  constructor <;> split <;> omega

theorem union_sound_right {x : Int} {i j : Iv} (hx : Mem x j) :
    Mem x (union i j) := by
  simp only [Mem, union] at *
  simp only [imin, imax]
  constructor <;> split <;> omega

theorem inter_sound {x : Int} {i j : Iv} (hi : Mem x i) (hj : Mem x j) :
    ∃ k, inter i j = some k ∧ Mem x k := by
  have hb : imax i.lo j.lo ≤ x ∧ x ≤ imin i.hi j.hi := by
    simp only [Mem] at hi hj
    simp only [imin, imax]
    constructor <;> split <;> omega
  unfold inter
  split
  · exact ⟨_, rfl, hb.1, hb.2⟩
  · next hc => exact absurd (Int.le_trans hb.1 hb.2) hc

/-- Inclusion monotonicity for `add` — the property that keeps fixpoint
iteration on recursive signals sound. The same statement is owed for every
transfer as it lands. -/
theorem add_subset_mono {i i' j j' : Iv} (hi : Subset i i') (hj : Subset j j') :
    Subset (add i j) (add i' j') := by
  simp [Subset, add] at *; omega

/-! ## Generic monotone unary transfer

Rust routes monotone unary operators through `exact_precision_unary`
(which takes a plain `fn(f64) -> f64`). One lemma covers the family, and
its tightness companion shows the endpoints are attained — the transfer is
not merely sound but minimal. -/

def mapIv (f : Int → Int) (i : Iv) : Iv := ⟨f i.lo, f i.hi⟩

theorem mono_unary_sound (f : Int → Int)
    (hf : ∀ a b, a ≤ b → f a ≤ f b) {x : Int} {i : Iv} (hx : Mem x i) :
    Mem (f x) (mapIv f i) :=
  ⟨hf _ _ hx.1, hf _ _ hx.2⟩

theorem mono_unary_tight (f : Int → Int)
    (hf : ∀ a b, a ≤ b → f a ≤ f b) {i : Iv} (hw : i.Wf) :
    Mem (f i.lo) (mapIv f i) ∧ Mem (f i.hi) (mapIv f i) :=
  ⟨⟨Int.le_refl _, hf _ _ hw⟩, ⟨hf _ _ hw, Int.le_refl _⟩⟩

/-! ## Nonlinear transfers: executable, obligation-gated

`mul` takes the min/max of the four endpoint products; `div` refuses
zero-crossing denominators. Their inclusion proofs are the sign case
analyses of gate I2 — until then they are named obligations, not claims. -/

def mul (i j : Iv) : Iv :=
  ⟨imin (imin (i.lo * j.lo) (i.lo * j.hi)) (imin (i.hi * j.lo) (i.hi * j.hi)),
   imax (imax (i.lo * j.lo) (i.lo * j.hi)) (imax (i.hi * j.lo) (i.hi * j.hi))⟩

def div (i j : Iv) : IvL :=
  if j.lo ≤ 0 ∧ 0 ≤ j.hi then none
  else some ⟨imin (imin (i.lo / j.lo) (i.lo / j.hi)) (imin (i.hi / j.lo) (i.hi / j.hi)),
             imax (imax (i.lo / j.lo) (i.lo / j.hi)) (imax (i.hi / j.lo) (i.hi / j.hi))⟩

/-- I2 obligation: inclusion for `mul` (four-sign case analysis). -/
def MulTransferSound : Prop :=
  ∀ (x y : Int) (i j : Iv), Mem x i → Mem y j → Mem (x * y) (mul i j)

/-- I2 obligation: inclusion for `div` on non-zero-crossing denominators. -/
def DivTransferSound : Prop :=
  ∀ (x y : Int) (i j : Iv) (k : Iv), Mem x i → Mem y j →
    div i j = some k → y ≠ 0 → Mem (x / y) k

/-- R-declared regime (proposal §3): the `crates/interval/src/ops/missing.rs`
placeholders return `interval(0)` for C++ parity and are sound for nothing
except the constant zero. Formally: a transfer that ignores its inputs
satisfies inclusion only if the concrete operator is constantly zero. No
document may call the interval layer verified while placeholders remain;
burning this list down is tracked, measurable work. -/
def PlaceholderUnsound : Prop :=
  ∀ (f : Int → Int), (∀ x i, Mem x i → Mem (f x) ⟨0, 0⟩) → ∀ x, f x = 0

theorem placeholderUnsound_holds : PlaceholderUnsound := by
  intro f h x
  have := h x ⟨x, x⟩ ⟨Int.le_refl x, Int.le_refl x⟩
  simp [Mem] at this
  omega

/-! ## Regression fixtures

Mirrors of representative `crates/interval` unit-test cases on the linear
core, plus the endpoint-product `mul` shape. -/

#guard add ⟨1, 2⟩ ⟨3, 4⟩ = ⟨4, 6⟩
#guard neg ⟨-1, 5⟩ = ⟨-5, 1⟩
#guard sub ⟨0, 1⟩ ⟨2, 3⟩ = ⟨-3, -1⟩
#guard union ⟨-1, 2⟩ ⟨0, 5⟩ = ⟨-1, 5⟩
#guard inter ⟨-1, 2⟩ ⟨0, 5⟩ = some ⟨0, 2⟩
#guard inter ⟨0, 1⟩ ⟨2, 3⟩ = none
#guard mul ⟨-2, 3⟩ ⟨-1, 4⟩ = ⟨-8, 12⟩
#guard div ⟨-4, 8⟩ ⟨2, 4⟩ = some ⟨-2, 4⟩
#guard div ⟨1, 2⟩ ⟨-1, 1⟩ = none
#guard memB 2 ⟨1, 3⟩ = true
#guard memB 4 ⟨1, 3⟩ = false
#guard subsetB ⟨1, 2⟩ ⟨0, 3⟩ = true
#guard subsetB ⟨0, 3⟩ ⟨1, 2⟩ = false
#guard (⟨3, 1⟩ : Iv).wfB = false

end Faust.IntervalArith
