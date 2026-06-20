# Signal → FIR as typed rewriting: a refinement-sort calculus

**Date:** 2026-06-20
**Scope:** an abstract model of the `crates/transform` pipeline (`signal_prepare` + `signal_fir`)
**Audience:** anyone who wants the *mental model* of the transform without reading the implementation
**Concrete companion:** [`signal-to-fir-transform-analysis-2026-06-20-en.md`](signal-to-fir-transform-analysis-2026-06-20-en.md)
(step-by-step implementation review; `W#` findings referenced below come from it)

---

## 1. Why a formal view

The step-by-step analysis describes *what each pass does*. This document answers a different
question: **what is the type of each pass, and what must hold for the composition to be correct?**

The idea is to treat every phase as a **typed rewrite**

```
phase : { t : T | Pre(t) }  ⟶  { t : T | Post(t) }
```

where `T` is a term universe and `Pre`/`Post` are *refinement predicates* (invariants) carving out
sub-languages. The whole pipeline is then a chain of such arrows, and it is "well-typed" exactly
when each phase's output sort entails the next phase's input sort. This altitude lets us reason
about the transform without tracking field-level details, and — as Section 8 argues — it makes
several real bug classes visible *as type errors* rather than as runtime surprises.

This is the standard refinement-typing / Hoare-triple view of a compiler pipeline, specialised to
the two term algebras at play here.

---

## 2. The two term algebras and the denotation

We work over two many-sorted signatures.

**Signal terms** `𝕋_sig` — the algebra produced by propagation and consumed/rewritten by
`signal_prepare`. Representative constructors (Faust `SigMatch`):

```
Int ℤ | Real ℝ | Input ℕ | BinOp(⊙, ·, ·) | Sin · | … | Delay(·,·) | Delay1 · | Prefix(·,·)
| IntCast · | FloatCast · | BitCast · | Select2(·,·,·) | RdTbl(·,·) | WrTbl(·,·,·,·) | Waveform[·]
| Proj(k, ·) | REC · | REF k                      -- de Bruijn recursion (input only)
| SYMREC(ν, [·]) | SYMREF ν                        -- symbolic recursion (after step 3.x)
| BlockReverseAD{ body, primal_count, seeds, cotangents } | ReverseTimeRec ·
| Button c | Slider c | Bargraph(c, ·) | Soundfile c | FConst… | FVar… | FFun…
```

**FIR terms** `𝕋_fir` — a *different* algebra: a module made of typed value-expressions and
lifecycle *statements* (`StoreVar`, `StoreTable`, `DeclareVar`, `For`, `If`, …) over a `FirStore`.
Crucially `𝕋_fir ≠ 𝕋_sig`: the signal→FIR step is a **change of algebra**, not an endo-rewrite.

**Denotation.** Fix a stream semantics

```
⟦·⟧ : (well-typed term)  ⟶  (ℝ^inputs)^ω → (ℝ^outputs)^ω
```

assigning to each term the sample-stream function it computes. Every *value-preserving* phase must
satisfy the semantic obligation `⟦phase(t)⟧ = ⟦t⟧`. (For FIR we use the obvious executable
semantics of the module's `compute`.) Keeping `⟦·⟧` explicit is what turns "is this rewrite
correct?" into a provable/testable statement instead of a vibe.

---

## 3. The tower of refinement sorts

Define invariant predicates over `𝕋_sig` (each is a decidable property of a term + its type map):

| Pred | Meaning |
|------|---------|
| `Sym` | no de Bruijn `REC`/`REF` remain (recursion is `SYMREC`/`SYMREF`) |
| `NoBareRef` | every `SYMREF` occurs under a `Proj` |
| `NoLegacyRec` | no `REC` survives as a value node |
| `Canon₁` | `Proj(k, g)` with `arity(g)=1 ⟹ k = 0` |
| `Typed` | a total `SigType` map `Γ` exists over all reachable nodes |
| `P` (`Promoted`) | every operator's operands are domain-consistent (the *promotion invariant*, §3 of the analysis): arithmetic `BinOp` operands share the result domain; bitwise/shift and delay/table indices are `IntCast`; `Div` is real; `Delay1`/`Prefix` satisfy `type(init)=type(value)` |
| `D1` (`Delay1Canon`) | no `Delay(_, Int 1)` (one-sample delays are `Delay1`) |
| `RedTyped` | reduced map `τ : node → {Int,Real,Sound}` is total and coherent with `Γ` |
| `BraOk` | `BlockReverseAD` structural invariants (`|body| = |cotangents| = primal_count`, slot bounds) |
| `UiOk` | every control reference resolves in the `UiProgram` |

The **lowering precondition** — the sort the FIR translation is *documented against* — is their
conjunction:

```
L_prep  :=  Sym ∧ NoBareRef ∧ NoLegacyRec ∧ Canon₁ ∧ Typed ∧ P ∧ D1 ∧ RedTyped ∧ BraOk ∧ UiOk
```

`L₀` is the raw propagated forest (only loosely constrained: de Bruijn recursion allowed, mixed
domains, non-canonical delays). The preparation pipeline is a path from `L₀` up to `L_prep`.

> The invariants are **not** a strict chain: some passes establish one invariant while *temporarily
> breaking* another (algebraic `simplify` can break `P`). So the right model is "the set of
> invariants currently guaranteed," i.e. a Hoare pre/post per phase — not a single monotone ladder.
> Making that explicit is already informative: it is exactly why the pipeline re-establishes `P`
> with a *second* promotion at the end (analysis finding **W4**).

---

## 4. A taxonomy of phases (six kinds)

Every pass falls into one of six categories, and the category *is* its type shape:

| Kind | Shape | Phases |
|------|-------|--------|
| **(a) Refresh** | `S → S` (mod ownership), `⟦⟧`-preserving | clone into private arena |
| **(b) Narrowing rewrite** | `{t\|Pre} → {t\|Post}` on `𝕋_sig`, ideally `⟦⟧`-preserving | `de_bruijn_to_sym`, `canonicalize_unary`, `promote`, `simplify`, `merge`, `canonicalize_one_sample` |
| **(c) Decoration** | `{t\|Pre} → Attr` (term unchanged) | `TypeAnnotator`, `derive_simple_types`, placement analysis, delay analysis |
| **(d) Membership decision** | `𝕋_sig → Bool` (a `χ`) | `verify` |
| **(e) Translation** | `L_prep ⇀ 𝕋_fir` (partial, state-threaded) | `build_module` / `lower_signal` |
| **(f) Intra-FIR rewrite** | `𝕋_fir → 𝕋_fir`, `⟦⟧`-preserving | CSE, variability materialization |

Two observations fall out immediately:

- **Decorations (c) cannot be "wrong terms," only wrong *attributes*.** Their correctness is an
  *abstraction-soundness* property, not a rewrite property (see `derive_simple_types` in §8).
- **Translation (e) is partial.** Its domain `dom(⇝)` is the set of constructors it has a rule
  for. "Returns `UnsupportedSignalNode`" is precisely "input ∉ `dom(⇝)`," i.e. the rewrite is
  *stuck*. This reframing is the source of several findings below.

---

## 5. Typed signatures of the preparation phases

Listed in pipeline order, each as `pre ⊢ phase : … → …`. The complete rule set of every phase
follows in §6.

```
clone                 : L₀                          → L₀                      (a, ⟦⟧-preserving)
de_bruijn_to_sym      : L₀                          → { Sym ∧ NoLegacyRec }   (b)
canonicalize_unary    : { Sym }                     → { Sym ∧ Canon₁ }        (b)
type #1               : { Sym ∧ Canon₁ }            → Γ₁                       (c)
promote #1            : (t, Γ₁)                     → { … ∧ P }               (b)
type #2               : { P }                       → Γ₂                       (c)
simplify #1           : (t, Γ₂)                     → { … }   [P may break]    (b, ⟦⟧-preserving*)
merge_symrec          : { Sym }                     → { Sym }                  (b, ⟦⟧-preserving)
type #3 ; simplify #2 : …                           → …                       (c,b)
canonicalize_1sample  : { Sym }                     → { … ∧ D1 }              (b, ⟦⟧-preserving)
type #4               : …                           → Γ₄                       (c)
promote #2            : (t, Γ₄)                     → { … ∧ P ∧ D1 }          (b)  ← re-establish P
type #5               : { P ∧ D1 }                  → Γ₅                       (c)
derive_simple_types   : (t, Γ₅)                     → τ  (RedTyped)            (c)
verify                : 𝕋_sig                        → Bool  (≈ χ_{L_verify})  (d)
─────────────────────────────────────────────────────────────────────────────────────
build_module (⇝)      : L_prep                       ⇀ 𝕋_fir                   (e)
CSE                   : 𝕋_fir                         → 𝕋_fir                   (f, ⟦⟧-preserving)
```

`*` `simplify` is value-preserving only under the IEEE-754 caveat of §8.

The interesting line is the last preparation arrow: `verify` is supposed to be the runtime witness
that `Post(prepare) ⊑ Pre(lowering)`, i.e. `χ_{L_prep}`. As §8 shows, the *implemented* `χ` is
weaker than `L_prep`.

---

## 6. Complete rewrite rules, phase by phase

This section gives the *full* rule set of every phase (grounded in the implementation), not just a
sample. Read top-to-bottom it is the operational definition of the whole transform.

### 6.0 Notation

- `Γ ⊢ x : θ` is the canonical type annotation; `kind(x) ∈ {Int, Real, Sound}` its reduced nature.
- **Smart coercions** (cast inserted *only* when the domain actually differs, and pushed *inside* a
  `Clocked` wrapper rather than around it):

  ```
  ⟨x⟩ᵢ = x                              if kind(x)=Int
  ⟨x⟩ᵢ = Clocked(env, IntCast v)        if promote(x)=Clocked(env,v)
  ⟨x⟩ᵢ = IntCast(promote x)             otherwise                         -- coerce-to-Int
  ⟨x⟩ᵣ = x                              if kind(x)=Real
  ⟨x⟩ᵣ = Clocked(env, FloatCast v)      if promote(x)=Clocked(env,v)
  ⟨x⟩ᵣ = FloatCast(promote x)           otherwise                         -- coerce-to-Real
  ⟨x⟩_like(t) = smart_cast(kind(t), x)                                     -- coerce to another operand's domain
  ```
- Lowering judgment `Γ; σ ⊢ s ⇝ (e, σ′)`: under typing `Γ` and lowering state `σ` (FIR store, the
  six statement buckets, delay/recursion/BRA sub-state, counters), signal `s` produces FIR value
  `e` and updated state `σ′`. `firTy(θ)`: `Int↦Int32`, `Real↦real_ty`, `Sound↦Sound`.
- `f(t₁..tₙ) ⇒ f(⟦t₁⟧..⟦tₙ⟧)` denotes the **congruence** (structural recursion) closure, implied
  for every rewrite phase and not repeated per constructor. All phases are **memoised** over the
  DAG, so structural sharing is preserved.

---

### 6.1 Refresh — `clone_forest_from` (kind a)

A homomorphic copy into a fresh arena, memoised so sharing is preserved. `⟦·⟧`- and
sort-preserving; the only effect is ownership (the source arena is not mutated).

```
copy(f(t₁..tₙ)) = f(copy t₁ .. copy tₙ)        with memo: copy(t) computed once per node
```

---

### 6.2 `de_bruijn_to_sym` — binder reification (kind b, nominal)

Nameless→named conversion. `ρ` is the stack of binders in scope; `Rec` binds a whole group (a body
list), `Ref(k)` names the `k`-th enclosing group.

```
B⟦Rec(bodies)⟧ρ  ⇒  SYMREC(ν, [ B⟦b⟧(ν·ρ) | b ∈ bodies ])        ν fresh
B⟦Ref(k)⟧ρ       ⇒  SYMREF(ρ[k])
B⟦f(t..)⟧ρ       ⇒  f(B⟦t⟧ρ ..)                                  (congruence)
```

Postcondition `Sym ∧ NoLegacyRec`: no `Rec`/`Ref` survives. Failure (open/ill-scoped `Ref`) is a
typed `RecursionError`.

---

### 6.3 `canonicalize_unary` — degenerate-projection normalisation (kind b)

Let `U = { ν | SYMREC(ν, bodies) reachable ∧ |bodies| = 1 }`.

```
Proj(k, SYMREC(ν,[b]))  ⇒  Proj(0, SYMREC(ν,[b]))                ν ∈ U
Proj(k, SYMREF ν)       ⇒  Proj(0, SYMREF ν)                     ν ∈ U
Proj(k, g)              ⇒  Proj(k, g)                            otherwise
```

Postcondition `Canon₁`. (Explicitly *not* a port of C++ `inlineDegenerateRecursions`: no
dependency-graph analysis, only index canonicalisation — analysis finding W2.)

---

### 6.4 `promote` — type-directed cast insertion (kind b)

The complete inventory (helpers `⟨·⟩ᵢ`, `⟨·⟩ᵣ`, `⟨·⟩_like` from §6.0). **`P` is exactly the normal
form of this system**: `Promoted(t)` ⟺ no rule below would insert a cast.

```
-- leaves (no children): identity
Int n | Real r | Input i | Button c | Checkbox c | Slider c | NumEntry c | Soundfile c
| FConst… | FVar…                                   ⇒  itself

-- arithmetic BinOp: result domain decides
BinOp(⊕,x,y) , ⊕∈{+,−,*} , kind(node)=Int           ⇒  BinOp(⊕, promote x, promote y)
BinOp(⊕,x,y) , ⊕∈{+,−,*} , kind(node)=Real          ⇒  BinOp(⊕, ⟨x⟩ᵣ, ⟨y⟩ᵣ)
BinOp(/,x,y)                                         ⇒  BinOp(/, ⟨x⟩ᵣ, ⟨y⟩ᵣ)                -- Div always Real
BinOp(%,x,y) , kind(x)=kind(y)=Int                  ⇒  BinOp(%, promote x, promote y)
BinOp(%,x,y) , otherwise                            ⇒  Fmod(⟨x⟩ᵣ, ⟨y⟩ᵣ)                    -- Rem on reals becomes Fmod!
BinOp(⊙,x,y) , ⊙ comparison , kind(x)=kind(y)       ⇒  BinOp(⊙, promote x, promote y)       -- result Int
BinOp(⊙,x,y) , ⊙ comparison , otherwise             ⇒  BinOp(⊙, ⟨x⟩ᵣ, ⟨y⟩ᵣ)
BinOp(&,x,y) , & ∈ {and,or,xor,<<,>>a,>>l}          ⇒  BinOp(&, ⟨x⟩ᵢ, ⟨y⟩ᵢ)

-- math
Pow|Atan2|Fmod|Remainder (x,y)                      ⇒  op(⟨x⟩ᵣ, ⟨y⟩ᵣ)
Sin|Cos|Tan|Asin|Acos|Atan|Exp|Exp10|Log|Log10|Sqrt|Floor|Ceil|Rint|Round (x)  ⇒  op(⟨x⟩ᵣ)
Min|Max (x,y) , kind(x)=kind(y)                     ⇒  op(promote x, promote y)              -- int min/max stays int
Min|Max (x,y) , otherwise                           ⇒  op(⟨x⟩ᵣ, ⟨y⟩ᵣ)
Abs(x) , kind(x)=Int                                ⇒  Abs(promote x)
Abs(x) , otherwise                                  ⇒  Abs(⟨x⟩ᵣ)

-- delay / state: amount is Int, carried value keeps its own domain
Delay1(x)                                           ⇒  Delay1(promote x)
Delay(x,n)                                          ⇒  Delay(promote x, ⟨n⟩ᵢ)
Prefix(i,x) , kind(i)=kind(x)                       ⇒  Prefix(promote i, promote x)
Prefix(i,x) , otherwise                             ⇒  Prefix(⟨i⟩ᵣ, ⟨x⟩ᵣ)
ZeroPad(x,n)                                        ⇒  ZeroPad(promote x, ⟨n⟩ᵢ)

-- tables: indices Int, write signal homogenised to the generator/element domain
RdTbl(t,i)                                          ⇒  RdTbl(promote t, ⟨i⟩ᵢ)
WrTbl(sz,gen,nil,nil)                               ⇒  WrTbl_ro(promote sz, promote gen)
WrTbl(sz,gen,wi,ws)                                 ⇒  WrTbl(promote sz, promote gen, ⟨wi⟩ᵢ, ⟨ws⟩_like(gen))
Waveform[v..] , all kind(vᵢ)=Int                    ⇒  Waveform[promote vᵢ ..]
Waveform[v..] , otherwise                           ⇒  Waveform[⟨vᵢ⟩ᵣ ..]

-- control / selection / IO
Select2(c,a,b) , kind(a)=kind(b)                    ⇒  Select2(⟨c⟩ᵢ, promote a, promote b)
Select2(c,a,b) , otherwise                          ⇒  Select2(⟨c⟩ᵢ, ⟨a⟩ᵣ, ⟨b⟩ᵣ)
Enable(x,g)                                         ⇒  Enable(promote x, ⟨g⟩ᵢ)
VBargraph|HBargraph(c,x)                            ⇒  Bargraph(c, ⟨x⟩ᵣ)
SoundfileLength|Rate(sf,part)                       ⇒  op(promote sf, ⟨part⟩ᵢ)
SoundfileBuffer(sf,ch,part,idx)                     ⇒  op(promote sf, promote ch, ⟨part⟩ᵢ, ⟨idx⟩ᵢ)

-- casts: collapse / smart-insert
IntCast(x)                                          ⇒  smart_int_cast(promote x, x)          -- elides redundant casts
FloatCast(x)                                        ⇒  ⟨x⟩ᵣ
BitCast(x)                                          ⇒  BitCast(promote x)

-- clock family: first item carries the (Int) clock; rest promoted generically
OnDemand|Upsampling|Downsampling[c, items..]        ⇒  op(smart_clock_cast c, promote items ..)
Clocked(env,v)                                      ⇒  Clocked(promote env, promote v)

-- structural pass-through (children promoted; node rebuilt)
Output | Attach | Control | Seq | Gen | Lowest | Highest | TempVar | PermVar
| Fir | Iir | Proj | Rec | ReverseTimeRec | BlockReverseAD | AssertBounds | FFun  ⇒  congruence
```

`smart_int_cast` / `smart_float_cast` are the elision rules of `⟨·⟩` (no double-cast, clock-aware).
Note two semantically meaningful rewrites hiding in the table: **`%` on reals becomes `Fmod`**, and
**`FloatCast` is collapsed into a smart coercion** (so `FloatCast(Real)` disappears).

---

### 6.5 `simplify` — algebraic value rules (kind b, ⟦·⟧-preserving*)

Run with a memo cache; recursion-safe via a sentinel on `SYMREC`/`Rec` (insert `None`, recurse on
the body). `*` value-preserving modulo the IEEE-754 caveat (§8.4).

```
-- constant folding
math1(c)              ⇒ Real(fold)                        c numeric        (acos…round)
math2(c₁,c₂)          ⇒ fold                              both numeric     (atan2,fmod,remainder,pow,min,max)
BinOp(op,c₁,c₂)       ⇒ fold_binop(op,c₁,c₂)              both numeric
IntCast(Int n)        ⇒ Int n        IntCast(Real r)      ⇒ Int (r as i32)
FloatCast(Int i)      ⇒ Real i       FloatCast(Real r)    ⇒ Real r

-- neutral / absorbing elements
0 + x ⇒ x    x + 0 ⇒ x    x − 0 ⇒ x    1 * x ⇒ x    x * 1 ⇒ x
0 * x ⇒ 0    x * 0 ⇒ 0    (and the AND/OR neutral & absorbing analogues)
0 − x ⇒ (−1) * x

-- self-operation  (x op x)
x − x ⇒ 0     x & x ⇒ x     x | x ⇒ x
x ≥ x | x ≤ x | x == x ⇒ 1        x > x | x < x | x ≠ x | x % x | x ^ x ⇒ 0

-- boolean AND/OR with literal 1
1 & b ⇒ b      1 | b ⇒ 1        (b a boolean-valued signal; symmetric)

-- selection / gating
Select2(0, t, _) ⇒ t      Select2(n≠0, _, e) ⇒ e      Select2(_, t, t) ⇒ t
Enable(t, 0) ⇒ 0          Enable(t, 1) ⇒ t
Control(t, 0) ⇒ 0         Control(t, 1) ⇒ t

-- delays and default arithmetic → canonical normal form
Delay1(x)            ⇒ normalize_delay1_term(x)
Delay(x, n)          ⇒ normalize_delay_term(x, n)
BinOp(op, x, y)      ⇒ normalize_add_term(BinOp(op,x,y))   -- when no rule above fires
```

`normalize_add_term` / `normalize_mul_term` (modules `aterm.rs` / `mterm.rs`) put expressions in the
**canonical sum-of-products form**: a weighted sum of monomials with folded numeric coefficients and
a stable ordering — the Faust signal normal form that makes structurally-equal expressions
`SigId`-equal (and thus enables cross-tree `x − x ⇒ 0`, which is exactly why `merge` runs first).

---

### 6.6 `merge_isomorphic_symrec_groups` — recursion CSE (kind b)

Compute a structural **signature** of each `SYMREC` by *opening* its body with a hole replacing the
self-edge, group by equal signature, substitute every member to one canonical representative, and
iterate to a fixpoint.

```
sig(SYMREC(ν, bs)) = open(bs)[ SYMREF ν ↦ HOLE ]
{ SYMREC(ν₁,bs₁), …, SYMREC(νₖ,bsₖ) } with equal sig , k ≥ 2
        ⇒  every SYMREC(νᵢ,·) ↦ SYMREC(ν*,·) ,  every SYMREF νᵢ ↦ SYMREF ν*
repeat until no group of size ≥ 2 remains
```

---

### 6.7 `canonicalize_one_sample_delays` (kind b)

```
Delay(x, Int 1)  ⇒  Delay1(x)                      (congruence elsewhere; recurses into SYMREC bodies)
```

Postcondition `D1`.

---

### 6.8 Decorations — attribute rules (kind c)

These do not rewrite terms; they compute attributes. Presented as judgment/transfer rules.

**Type annotation** `Γ ⊢ t : ⟨nature, variability, interval⟩` (canonical `sigtype`; full lattice
deferred to that crate). Shape of the rules that the pipeline relies on:

```
nature(BinOp(⊕,x,y))   = join(nature x, nature y)         (⊕ arithmetic)
nature(comparison)     = Int        nature(bitwise)        = Int
variability(t)         = ⊔ variability(childᵢ)            (Konst ⊑ Block ⊑ Samp)
variability(Proj(_,SYMREC…)) ⊒ Samp
interval(Delay(x,n))   = interval(x)   ;   check_delay_interval(n) bounds the line
recursion              : least fixpoint starting from the TREC approximation
```

**Reduced typing** `α : ⟨SigType⟩ → {Int, Real, Sound}` (`derive_simple_types`, complete):

```
α(t) = Sound                       if t = Soundfile(_)
α(t) = Real                        if unresolved_rec_proj(t)
α(t) = Int                         if nature(Γ t) = Int
α(t) = Real                        if nature(Γ t) ∈ {Real, Any}
   where unresolved_rec_proj(t) ⟺ t = Proj(_, SYMREC(ν,[ Proj(_, SYMREF ν) ]))
```

The `unresolved_rec_proj` clause is the one place `α` is **not** a homomorphism of `Γ` (finding W3).

**Delay analysis** `A⟦t⟧d` — abstract interpretation accumulating delay `d`, memoised on
`best_seen_delay` (revisit only on a strictly larger `d`):

```
A⟦Delay(x,n)⟧d            = rec_record(x,d) ; A⟦x⟧(d + ub(n)) ; A⟦n⟧0
A⟦Delay1(x)⟧d             = A⟦x⟧(d+1)
A⟦Prefix(i,x)⟧d           = A⟦x⟧(d+1) ; A⟦i⟧0
A⟦Proj(k, SYMREC(ν,bs))⟧d = rec_out[(ν,k)] ⊔= d ; ∀b∈bs: A⟦b⟧0
A⟦Proj(k, SYMREF ν)⟧d     = rec_out[(ν,k)] ⊔= d
A⟦f(..tᵢ..)⟧d             = ∀i: A⟦tᵢ⟧0                          (non-delay node resets accumulator)
```

A second walk (`scan_signals`) yields, per non-recursion carrier `x`, `maxDelay[x]`, skipping
carriers whose access is `Delay1ᵏ(Proj(…))` (those are merged into the recursion array).

**Placement analysis** `analyze_sig_rec` (computes `refcount`, `boundary`, `konstEscapes`):

```
refcount[t] += 1                                              (on every visit)
parent_var > var(t)            ⇒ boundary ∋ t ; if var(t)=Konst ⇒ konstEscapes ∋ t
inside_BRA ∧ var(t)=Konst      ⇒ konstEscapes ∋ t
descend once (visited gate); child parent_var := var(t); child inside_BRA := inside_BRA ∨ isBRA(t)
```

---

### 6.9 `verify` — the membership predicate `χ` (kind d)

The inductive acceptance relation `⊢ok t` (rejections are the negative rules):

```
⊬ok t            if t contains de Bruijn Rec/Ref            (¬Sym)
⊬ok t            if t = SYMREF ν not under a Proj           (¬NoBareRef)
⊬ok t            if t = legacy Rec                          (¬NoLegacyRec)
⊢ok Proj(k,g)    requires g = SYMREC/SYMREF (or BRA/RTR), arity registered, k in range,
                          and (arity=1 ⇒ k=0)               (Canon₁ + bounds)
⊢ok SYMREC(ν,bs) requires bs non-empty and ν used with one consistent arity
⊢ok BRA{…}       requires |body| = |cotangents| = primal_count ≥ 0, slots in range (BraOk)
⊢ok WrTbl(…)     requires readonly/write placeholders consistent
⊢ok ctrl(c,…)    requires c resolves in UiProgram           (UiOk)
⊢ok t            requires reduced τ(t) present and equal to the freshly derived α(t)  (RedTyped)
                 and full Γ(t) present                       (Typed)
⊢ok f(t..)       if ⊢ok tᵢ for all children                 (congruence)
```

> Crucially `⊢ok` does **not** check `P` (promotion invariant) or `D1` (one-sample-delay form):
> `L_verify ⊊ L_prep`. See §8.1 — this is the one *new* gap the formalisation exposes.

---

### 6.10 Lowering `⇝` — the translation (kind e)

`Γ; σ ⊢ s ⇝ (e, σ′)`. State deltas (declarations, sample-phase statements) are written as
`σ ⊕ …`. The translation is **partial**: any constructor with no rule below is *stuck* (the
implementation raises `UnsupportedSignalNode`); `dom(⇝)` is the set of covered constructors.

**Leaves and IO**

```
Int n               ⇝ FIR.Int32 n
Real r              ⇝ FIR.Float(real_ty) r
Input i             ⇝ cast(real_ty, LoadTable(inputᵢ, i0, FaustFloat))     σ ⊕ {alias inputᵢ once}
FConst "fSamplingFreq" ⇝ LoadVar(fSampleRate, Int32)  then cast to firTy(node) if Real
FVar "count"        ⇝ LoadVar(count, FunArgs)
FVar name           ⇝ LoadVar(name, Global)                                 σ ⊕ {extern decl name}
Button|Checkbox c   ⇝ boundary-load of zone(c)                              σ ⊕ {zone, UI, reset}
Slider|NumEntry c   ⇝ boundary-load of zone(c)                              σ ⊕ {zone, UI, reset}
Bargraph(c, x)      ⇝ let (eₓ,σ₁)=⇝x in eₓ                                  σ₁ ⊕ {store zone(c)=eₓ}
Soundfile c         ⇝ LoadVar(zone(c), Sound)
```

**Arithmetic and selection** (T-BinOp's side condition is discharged by `P`):

```
Γ ⊢ node:θ   ⇝x=(eₓ,σ₁)   ⇝y=(e_y,σ₂)   firTy(eₓ)=firTy(e_y)=firTy(θ)        (T-BinOp)
────────────────────────────────────────────────────────────────────────────
BinOp(⊕,x,y) ⇝ (FIR.BinOp(⟦⊕⟧, eₓ, e_y, firTy(θ)), σ₂)

Math1(op,x)         ⇝ math_call(op,[eₓ],real_ty)         σ ⊕ used_math_ops∋op
Math2(op,x,y)       ⇝ math_call(op,[eₓ,e_y],real_ty)
Min|Max(x,y) θ=Int  ⇝ fun_call(min_i|max_i,[eₓ,e_y],Int32)        else math_call(Min|Max,…)
Abs(x) θ=Int        ⇝ fun_call(abs,[eₓ],Int32)                    else math_call(Abs,…)
IntCast(x)          ⇝ FIR.Cast(Int32, eₓ)
FloatCast(x)        ⇝ FIR.Cast(real_ty, eₓ)        BitCast(x) ⇝ FIR.Bitcast(real_ty, eₓ)
Select2(c,a,b)      ⇝ FIR.Select2(e_c, eₐ, e_b, firTy(node))
```

**Delay family** — three resolution layers, tried in order:

```
-- (1) recursion-merged: value = Delay1ᵏ(Proj(i,g)) resolving to an active/materialised carrier
Delay1ᵏ(Proj(i,g))  ⇝ read carrier @ offset (k or k+amount):
                         SingleScalar → LoadVar(field)
                         ExactShift   → LoadTable(field, offset)
                         Circular     → LoadTable(field, (iota−offset)&(size−1))
-- (2) standalone Delay1 with shift strategy (mcd ≥ 1, not a recursion chain)
Delay1(x)           ⇝ LoadTable(bufₓ, 1)   ;  σ ⊕ {immediate: bufₓ[0]=eₓ; post_output: shift}
-- (2′) Delay1 fallback / Prefix → 2-slot circular state cell
Delay1(x)|Prefix    ⇝ LoadTable(state,(iota−1)&1) ; σ ⊕ {immediate: state[iota&1]=next}
-- (3) general fixed delay on a pre-allocated line (strategy emitter)
Delay(x,n)          ⇝ emit_fixed_delay(line(x), eₓ, eₙ, strategy)
                         Shift       : read buf[n]; write buf[0]; post shift
                         CircularPow2: read buf[(iota−n)&(S−1)]; write buf[iota&S−1]; iota++
                         IfWrapping  : read buf[wrap(idx−n)]; write buf[idx]; idx=bump
```

**Recursion** `Proj(i, g)`:

```
-- fast paths
Proj(i, SYMREF ν on stack)            ⇝ load feedback carrier current slot
Proj(i, materialised scalar)          ⇝ LoadVar(current-value binding)
Proj(i, materialised array)           ⇝ LoadTable(carrier, current index)
-- first encounter of a top-level SYMREC group g (T-Proj-SYMREC)
allocate one carrier per body slot, sized from delay analysis:
     unary ∧ maxDelay≤1 → SingleScalar ;  maxDelay < mcd → ExactShift ;  else → Circular(pow2)
push g; ∀ body bⱼ: (eⱼ,σ)=⇝bⱼ ;  (multi-output: snapshot all eⱼ before any store)
emit current-slot writes (immediate) + exact-shift finalize copies (post_output) ; pop
return slot i
```

**Block Reverse AD** `Proj(i, BRA{body,M=primal_count,seeds,cot})`:

```
i < M  (primal)   ⇝ ⇝(bodyᵢ) ; σ ⊕ {tape stores for non-trivially-reverse-evaluable operands}
i ≥ M  (gradient) ⇝ ensure_backward_sweep(g) once ; return grad_cache[(g, i−M)]
  backward_sweep:  postorder(body)
                   pre-seed feedback carries: ∀ Delay1(Proj(slot, SYMREF ν)): adj[bodyν,slot] += carry
                   seed cotangents:           ∀k: adj[bodyₖ] += cotₖ
                   reverse-walk postorder:    propagate_adj(node, adj[node])     (chain rule via ad_rules,
                                              tape-aware operand loads)
                   ∀ seed j: grad_cache[(g,j)] := adj[seedⱼ] (or 0)
```

**Tables, control wrappers, FFI, output**

```
Waveform[v..]       ⇝ const-static table + read                σ ⊕ {static decl}
RdTbl(t,i)          ⇝ LoadTable(table(t), normIndex(eᵢ))
WrTbl(sz, GEN, …)   ⇝ if GEN compile-time evaluable: interpret_generator → const table
                       else runtime write table
Output(i,x)         ⇝ ⇝x
Attach(l,r)         ⇝ ⇝r (kept for effects) ; ⇝l               Control(l,r) likewise
Enable(l,g)         ⇝ Select2(e_g, eₗ, 0, firTy(node))
Lowest|Highest(x)   ⇝ ⇝x
FFun(ff,args)       ⇝ FunCall(name, eargs, ret)                σ ⊕ {extern proto}
-- top-level output store (per output channel)
lower_output(i,s)   ⇝ let (e,σ)=⇝s in σ ⊕ { store outputᵢ[i0] = (cast FaustFloat if needed) e }
                       surplus signal (i ≥ num_outputs) ⇒ Drop e
```

**Placement gate** (applied to the result of every `⇝`, then cached — §5.1):

```
if ¬trivial(e) ∧ ¬recProj(s) ∧ ¬WrTbl(s) ∧ (refcount[s]≥2 ∨ boundary∋s):
     var(s)=Konst ⇒ materialise e in constants bucket (Struct storage if konstEscapes∋s)
     var(s)=Block ⇒ materialise e in control bucket
     var(s)=Samp  ⇒ leave inline
cache[s] := e        (except recursive projections, which are never cached)
```

Because the `T-BinOp` side condition is supplied by the input sort `P`, the lowering rules insert
**no implicit casts** — that property is literally the precondition annotation on `⇝`.

---

### 6.11 CSE / materialisation — intra-FIR (kind f)

Per statement bucket (`constants`, `control`, each sample loop), value-number the FIR store:

```
refcountFIR[v] = fan-out of value node v within the bucket
v , refcountFIR[v] ≥ 2 , ¬trivialFIR(v)
        ⇒  v ↦ LoadVar(nameᵥ)  ;  emit DeclareVar(nameᵥ, typ v, v) at first use
prefixes:  constants → fConst/iConst   control → fSlow/iSlow   sample loop → fTemp/iTemp
```

`⟦·⟧`-preserving (pure value numbering); descends into value children only, never across block/loop
scopes.

---

## 7. The composition obligations

The pipeline `p₁ ; p₂ ; … ; pₙ` is well-typed iff, for each adjacent pair,

```
Post(pᵢ)  ⊑  Pre(pᵢ₊₁)            (entailment / refinement-subtyping)
```

plus, for value-preserving phases, the semantic obligation `⟦pᵢ(t)⟧ = ⟦t⟧`.

Two obligations carry almost all the weight:

1. **`Post(prepare) ⊑ Pre(lower)`**, i.e. `L_verify ⊑ L_prep`. This is what `verify` is *meant* to
   discharge at runtime.
2. **`L_prep ⊑ dom(⇝)`**: every well-prepared term must have a lowering rule. This is a pure
   *coverage* statement about the translation's domain.

Stating these explicitly is what surfaces the bugs in §8 — they are obligations the current code
does **not** fully discharge.

---

## 8. What the method lets you verify — and the bugs it surfaces

This is the answer to "does this help check properties / find bugs?" — **yes**, in five concrete ways.

### 8.1 The runtime gate is *weaker* than the lowering precondition (new)

`verify` is the implementation of `χ`. Comparing what it actually checks against `L_prep`:

```
verify enforces :  Sym ∧ NoBareRef ∧ NoLegacyRec ∧ Canon₁ ∧ (Γ present) ∧ RedTyped
                   ∧ BraOk ∧ UiOk ∧ recursion-arity-consistency
L_prep needs    :  … all of the above …  ∧  P  ∧  D1
```

So **`verify` checks neither `P` (promotion invariant) nor `D1` (one-sample-delay canonical form)**
— exactly the two invariants the lowering rules are *documented to assume*. The gap is currently
patched operationally:

- `P` is re-checked *lazily, per node* inside `lower_binop` (the `operands_ok` test → `UnsupportedBinOp`);
- `D1` is merely *established* by `canonicalize_1sample` and never re-guarded — if a future pass
  reintroduced `Delay(x, Int 1)`, `verify` would pass and lowering would silently take the general
  fixed-delay path (a 2-slot buffer) instead of the `Delay1` fast path. No crash, just a silent
  divergence.

The refinement-sort view makes this precise: **the boundary verifier under-approximates the
precondition.** Closing it (have `verify` check `P` and `D1`) converts a documented-but-unenforced
assumption into a checked one — a cheap, high-value fix.

### 8.2 Lowering coverage as a subset obligation → finding W8

Because `⇝` is partial, `L_prep ⊑ dom(⇝)` is a real obligation, and it does **not** hold:
`verify` accepts `OnDemand`/`Upsampling`/`Downsampling`/`Clocked`/`ZeroPad`/`Fir`/`Iir`/
`AssertBounds`/`TempVar`/`PermVar`/`Seq` while `lower_signal` has no rule for them (`L_verify ⊄
dom(⇝)`). The formalism makes the mechanical check obvious — enumerate what each side handles — and
that check is now executable as the wildcard-free `lowering_coverage` classifier in
`signal_fir/tests.rs`.

But — and this is the important correction to the first draft of this section — the obligation is
violated **by design**, not by accident. `signal_prepare` is a deliberate *superset* of the current
fast-lane lowerer: it types, promotes, and *keeps* these carriers in anticipation of future lowering
(and for other consumers). This is enforced by explicit tests:
`prepare_signals_for_fir_accepts_filter_carrier_children` and
`prepare_signals_for_fir_recovers_shared_zero_pad_amount_from_float_context` assert that preparation
returns a forest still containing `Fir`/`ZeroPad`. An attempt (2026-06-20) to "close the gap" by
rejecting these families in `verify` broke exactly those tests — empirically confirming the superset
is intentional.

So the honest obligation is `L_prep ⊑ dom(⇝) ∪ Deferred`, where `Deferred` is the set prepare
accepts but the fast-lane does not yet lower. The right resolution of **W8** is therefore **not** to
reject (that regresses the contract) but to either (a) *implement* lowering for a family when a DSP
needs it — shrinking `Deferred` — or (b) leave it deferred and rely on the classifier as a **drift
guard**: it documents the current `Deferred` set and fails to compile when a new `SigMatch` variant
is added without a conscious classification. Asserting `{verify-accepted} == {lower-handled}` would
itself be wrong, because the two are intentionally unequal.

One genuine defect survives *inside* `Deferred`: such a program passes the staged contract and then
fails **late**, deep in lowering, with the generic `UnsupportedSignalNode` code — reframing **W13**
(the "stuck" cases are not classified by *which* precondition failed: outside-domain vs.
`P`-violation vs. delay-too-large). That late, coarse failure is the part worth improving,
independently of the coverage question.

### 8.3 Decorations must be *sound abstractions* → finding W3

`derive_simple_types` is `α : SigType → {Int,Real,Sound}`. For `RedTyped` to be meaningful, `α`
must be a homomorphic abstraction of the canonical typing (`α ∘ Γ` must agree with the reduced
typing everywhere). The `is_unresolved_recursive_projection` override (analysis **W3**) forces an
unconstrained self-loop to `Real`, *contradicting* the canonical `sigtype` result. In the formal
view this is an **abstraction function that does not commute with the concrete typing** — a
soundness hole in `α`, not a term bug. That diagnosis tells you the fix shape: either push the rule
into the canonical typing, or document it as an explicit, tested exception to the homomorphism.

### 8.4 Semantic-preservation obligations catch unsound rewrites

Each `(b)`/`(f)` phase carries `⟦phase(t)⟧ = ⟦t⟧`. Discharging it for `simplify` exposes the
IEEE-754 caveat: `x * 0 ⇒ 0` is **not** value-preserving when `x ∈ {NaN, ±∞}`. The current
`catch_unwind` fallback in `simplify` is an operational hedge, not a soundness argument. Writing the
obligation down tells you precisely which rules need a finiteness side condition — and it is exactly
the kind of property a **metamorphic / property test** can check (random typed signal, assert the
stream denotation is unchanged across the rewrite).

### 8.5 Hidden preconditions on the *output* type → finding W5

The produced FIR module has an implicit refinement on its entry point:
`compute(count, …)` is only correct for `count ≤ MAX_BRA_TAPE_BLOCK_SIZE` when a `BlockReverseAD`
carrier is present (analysis **W5**). In a dependent/refinement reading of the FIR signature this is
a precondition on `count` that is currently *unwritten* and *unchecked* — the tape silently
overflows above the bound. Even without dependent types, naming the obligation says: emit a guard,
or carry the bound in the module's type.

### 8.6 Preservation lemmas formalise the "pseudo-fixpoint" worry → finding W4

The "are the prepare passes a closed fixpoint?" worry (**W4**) becomes a precise checklist of
**preservation lemmas**: does `merge` preserve `P`? does `promote #2` preserve `D1`? (it does —
`promote` only wraps the *amount* of a `Delay`, and `Int 1` is already `Int`, so it is left intact;
but that is a lemma you can now *state and test*, not hope for). Each "phase X preserves invariant
Y" pair is a one-line property test. The same framing answers **confluence/determinism** questions
relevant to golden-test stability.

### Summary of payoff

| Property the model exposes | Bug class / finding |
|----------------------------|---------------------|
| `L_verify ⊑ L_prep` (gate ⊒ precondition) | `verify` omits `P`, `D1` (new) |
| `L_prep ⊑ dom(⇝) ∪ Deferred` (coverage) | W8 (intentional prepare-superset; drift-guarded), W13 (late generic error) |
| `α` homomorphic (decoration soundness) | W3 (reduced-type override) |
| `⟦phase(t)⟧ = ⟦t⟧` (semantic preservation) | unsound float simplifications |
| output-type refinement | W5 (BRA tape bound) |
| invariant preservation across phases | W4 (fixpoint/ordering brittleness) |

---

## 9. Limits of the method (honest scope)

The typed-rewriting view is a *modelling discipline*, not a free proof. Its limits here:

- **Binders and α-conversion.** `de_bruijn_to_sym` is a nominal/binding-aware transformation; pure
  first-order TRS does not capture it. A faithful treatment needs nominal rewriting or an explicit
  freshness/substitution calculus.
- **State threading.** Lowering `⇝` is *effectful* (delay lines, recursion carriers, CSE counters,
  six statement buckets live in `σ`). Reasoning about it cleanly needs a monadic/attribute-grammar
  or separation-logic-flavoured account of the `FirStore`, not just `pre → post` on terms.
- **Floating-point semantics.** `⟦·⟧` over IEEE-754 makes equational reasoning subtle; "semantic
  preservation" is only meaningful relative to a denotation that fixes NaN/Inf/rounding behaviour.
- **Change of algebra.** The `𝕋_sig → 𝕋_fir` step is a compilation, so "subject reduction" does not
  apply across it; you need a *cross-language* simulation (`⟦lower(t)⟧ = ⟦t⟧`) instead of a
  same-sort preservation lemma.
- **Cost.** Writing and maintaining the rules/obligations has overhead. The payoff is the discipline
  (explicit pre/post, coverage and soundness obligations as tests), not a mechanised proof — unless
  one later invests in a proof assistant.

---

## 10. Concrete, cheap wins suggested by the framing

None of these require a proof assistant; each is a test or a guard:

1. **Make `verify` enforce the full `L_prep`** — add `P` and `D1` checks so the boundary gate
   matches the lowering precondition (§8.1). *(Landed 2026-06-20.)*
2. **Add a domain-coverage classifier** — a wildcard-free `match` over `SigMatch` that documents the
   `Deferred` set and fails to compile when a new variant is added unclassified. This is a *drift
   guard*, not an equality assertion: `{verify-accepted} == {lower-handled}` is intentionally false,
   because prepare is a deliberate superset (§8.2). *(Landed 2026-06-20.)*
3. **Make `α` total and homomorphic, or pin the exception** for `is_unresolved_recursive_projection`
   with a dedicated test (§8.3).
4. **Property-test value preservation** of each `simplify`/CSE rule on random typed signals; add
   finiteness side conditions where it fails (§8.4).
5. **Emit a guard or carry the bound** for the BRA `count` precondition (§8.5).
6. **Encode phase pre/post as `debug_assert!` gates** between passes, turning the implicit ordering
   of `prepare_signals_for_fir_unverified` into explicit, testable contracts (§8.6).

In short: framing the transform as typed rewriting does not by itself prove anything, but it
*relocates* correctness questions to a small set of named obligations (gate ⊒ precondition,
domain coverage, abstraction soundness, semantic preservation, invariant preservation) — and each
of those becomes a concrete, cheap check. Several already point at live gaps (notably §8.1 and §8.2).

---

## 11. Concepts used and bibliography

This document borrows standard ideas from programming-language theory, semantics, and compiler
construction. This section names each concept, points to where it is used above, and gives
references. Nothing here is novel; the contribution of the document is only the *application* of
these tools to the `transform` pipeline.

### 11.1 Refinement types & predicate subtyping
*Used in:* the core framing — phases typed as `{t | Pre(t)} → {t | Post(t)}`, the sort tower (§3),
composition obligations (§7).
- T. Freeman, F. Pfenning. *Refinement Types for ML.* PLDI 1991.
- P. Rondon, M. Kawaguchi, R. Jhala. *Liquid Types.* PLDI 2008.
- J. Rushby, S. Owre, N. Shankar. *Subtypes for Specifications: Predicate Subtyping in PVS.* IEEE TSE 24(9), 1998.
- N. Vazou et al. *Refinement Types for Haskell* (LiquidHaskell). ICFP 2014. Project/blog: <https://ucsd-progsys.github.io/liquidhaskell/>

### 11.2 Hoare logic (pre/post specifications)
*Used in:* reading each pass as `{Pre} pass {Post}`, the preservation lemmas (§3, §7, §8.6).
- C. A. R. Hoare. *An Axiomatic Basis for Computer Programming.* CACM 12(10), 1969.

### 11.3 Term rewriting: normal forms, confluence, termination
*Used in:* the narrowing-rewrite phases (§6.2–§6.7), "`P` is the normal form of the promotion TRS"
(§6.4), confluence/termination remarks (§8.6, §9).
- F. Baader, T. Nipkow. *Term Rewriting and All That.* Cambridge University Press, 1998.
- N. Dershowitz, J.-P. Jouannaud. *Rewrite Systems.* Handbook of Theoretical Computer Science, Vol. B, 1990.
- D. Knuth, P. Bendix. *Simple Word Problems in Universal Algebras.* In *Computational Problems in Abstract Algebra*, 1970 (confluence / completion).

### 11.4 Denotational & stream/synchronous semantics
*Used in:* the denotation `⟦·⟧ : term → (ℝ^in)^ω → (ℝ^out)^ω` and the semantic-preservation
obligations (§2, §8.4).
- D. Scott, C. Strachey. *Toward a Mathematical Semantics for Computer Languages.* Oxford PRG, 1971.
- G. Winskel. *The Formal Semantics of Programming Languages.* MIT Press, 1993.
- J. Rutten. *A Coinductive Calculus of Streams.* Mathematical Structures in Computer Science 15(1), 2005.
- P. Caspi, D. Pilaud, N. Halbwachs, J. Plaice. *LUSTRE: A Declarative Language for Programming Synchronous Systems.* POPL 1987.

### 11.5 Faust signal algebra & semantics
*Used in:* the signal term algebra `𝕋_sig`, recursion, the block-diagram→signal model (§2, throughout).
- Y. Orlarey, D. Fober, S. Letz. *Syntactical and Semantical Aspects of Faust.* Soft Computing 8(9), 2004.
- Y. Orlarey, D. Fober, S. Letz. *An Algebra for Block Diagram Languages.* ICMC 2002.

### 11.6 Type soundness: progress & preservation (subject reduction)
*Used in:* "well-typed terms don't get stuck", lowering partiality = stuck state, `dom(⇝)` coverage
(§4, §6.10, §8.2).
- A. Wright, M. Felleisen. *A Syntactic Approach to Type Soundness.* Information and Computation 115(1), 1994.
- B. Pierce. *Types and Programming Languages.* MIT Press, 2002 (progress/preservation, ch. 8).

### 11.7 Abstract interpretation, Galois connections, lattices
*Used in:* the delay analysis and placement analysis as abstract interpretations, the reduced-type
map `α` as a sound abstraction, the variability lattice `Konst ⊑ Block ⊑ Samp` (§6.8, §8.3).
- P. Cousot, R. Cousot. *Abstract Interpretation: A Unified Lattice Model…* POPL 1977.
- F. Nielson, H. R. Nielson, C. Hankin. *Principles of Program Analysis.* Springer, 1999.
- B. Davey, H. Priestley. *Introduction to Lattices and Order,* 2nd ed. Cambridge University Press, 2002.

### 11.8 Binding: de Bruijn indices & nominal techniques
*Used in:* `de_bruijn_to_sym` binder reification (§6.2).
- N. G. de Bruijn. *Lambda Calculus Notation with Nameless Dummies…* Indagationes Mathematicae 34, 1972.
- M. Fernández, M. Gabbay. *Nominal Rewriting.* Information and Computation 205(6), 2007.
- A. Pitts. *Nominal Logic, a First Order Theory of Names and Binding.* Information and Computation 186, 2003.

### 11.9 Attribute grammars & syntax-directed translation
*Used in:* the decoration phases (typing, analyses) and the lowering as an attributed, state-threaded
translation (§4 kind c/e, §6.8, §6.10).
- D. Knuth. *Semantics of Context-Free Languages.* Mathematical Systems Theory 2(2), 1968.

### 11.10 Structured recursion (catamorphisms / folds)
*Used in:* the memoised DAG folds shared by `promote`, `simplify`, and lowering (§6.0 congruence).
- E. Meijer, M. Fokkinga, R. Paterson. *Functional Programming with Bananas, Lenses, Envelopes and Barbed Wire.* FPCA 1991.

### 11.11 Hash-consing & maximal sharing (DAGs)
*Used in:* arena interning / memoisation, "structurally-equal ⇒ same `SigId`" enabling cross-tree
`x − x ⇒ 0` (§6.0, §6.5, finding W10).
- J.-C. Filliâtre, S. Conchon. *Type-Safe Modular Hash-Consing.* ACM ML Workshop, 2006.

### 11.12 Common subexpression elimination / global value numbering
*Used in:* the CSE phase and the placement materialisation (§6.11, §5.1).
- J. Cocke. *Global Common Subexpression Elimination.* Proc. Symp. on Compiler Optimization, 1970.
- B. Alpern, M. Wegman, K. Zadeck. *Detecting Equality of Variables in Programs.* POPL 1988.
- C. Click. *Global Code Motion / Global Value Numbering.* PLDI 1995.
- S. Muchnick. *Advanced Compiler Design and Implementation.* Morgan Kaufmann, 1997.

### 11.13 Compiler correctness & translation validation
*Used in:* the cross-language obligation `⟦lower(t)⟧ = ⟦t⟧` and the invariant-preservation discipline
(§7, §8.6, §9 "change of algebra").
- X. Leroy. *Formal Verification of a Realistic Compiler* (CompCert). CACM 52(7), 2009. <https://compcert.org/>
- A. Pnueli, M. Siegel, E. Singerman. *Translation Validation.* TACAS 1998.

### 11.14 IEEE-754 floating-point semantics
*Used in:* the unsound-simplification caveat (`x * 0 ⇒ 0` for NaN/±∞), §6.5 and §8.4.
- *IEEE Standard for Floating-Point Arithmetic,* IEEE 754-2019.
- D. Goldberg. *What Every Computer Scientist Should Know About Floating-Point Arithmetic.* ACM Computing Surveys 23(1), 1991.
- D. Monniaux. *The Pitfalls of Verifying Floating-Point Computations.* ACM TOPLAS 30(3), 2008.

### 11.15 Reverse-mode automatic differentiation
*Used in:* the `BlockReverseAD` lowering and backward sweep (§6.10).
- A. Griewank, A. Walther. *Evaluating Derivatives: Principles and Techniques of Algorithmic Differentiation,* 2nd ed. SIAM, 2008.
- A. Baydin, B. Pearlmutter, A. Radul, J. Siskind. *Automatic Differentiation in Machine Learning: a Survey.* JMLR 18, 2018.
- J. Engel, L. Hantrakul, C. Gu, A. Roberts. *DDSP: Differentiable Digital Signal Processing.* ICLR 2020.
