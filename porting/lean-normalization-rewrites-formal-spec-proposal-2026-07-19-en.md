---
title: "Lean formalization proposal: normalization rewrites"
date: 2026-07-19
page-size: A4
margins: 20 22
page-numbers: true
font-body: Roboto
font-heading: Roboto Condensed
font-mono: Roboto Mono
---

**Date:** 2026-07-19

**Status:** proposal with its N0 skeleton delivered:
[`normalization-rewrites-formal-spec.lean`](normalization-rewrites-formal-spec.lean)
compiles (Lean 4.31, bundled Std, no `sorry`/`axiom`) with the first-match
cascade and its earliest-rule theorem proved, per-rule soundness and strict
size decrease proved for an eight-rule cascade, the `-1 · y` exception
guarded structurally, and the aterm signature merge proved sound; the
multiplicative factor-merge lemmas are named gate-N1 obligations. The
gate-0 adequacy review is still pending.

**Companions:** the methodology overview
[`../docs/lean-usage-methodology-en.md`](../docs/lean-usage-methodology-en.md)
and the roadmap
[`lean-formalization-roadmap-2026-07-19-en.md`](lean-formalization-roadmap-2026-07-19-en.md).

::: toc+
- **Scope and goal** — the mterm/aterm algebra as the formal core.
- **The formal core** — canonical terms, exact semantics, and the theorems to prove.
- **Rule order as a first-class object** — the lesson from `needs_separate_loop`.
- **Connection to Rust** — oracle strategy against the `normalize` crate.
- **Staged gates** — N0 to N3.
- **Risks and boundaries** — exact algebra vs floating point, and scope control.
:::

## 1. Scope and goal

The [`normalize`](../crates/normalize/src/lib.rs) crate ports the C++
`compiler/normalize/` pipeline in five layers:

```tree "normalize crate layering"
normalform (pipeline coordinator)
  simplify (memoized rewrite engine)
    normalize (add-term + delay-term normalization)
      aterm (additive term: sum of mterms)
        mterm (multiplicative term: k · x^n · y^m / …)
```

Every layer is a bundle of *claims*: that a rewrite preserves meaning, that
merging two terms with the same signature is sound, that `gcd`/`factorize`
produce divisors, that the canonical tree reconstruction round-trips. The
C++ encodes these claims implicitly in code order — including exceptions
like the `-1 · y` sign rule — and the port must reproduce them exactly.
`mterm` is complete; `aterm`, `normalize`, `simplify`, `normalform` are in
progress, which makes this the right moment to freeze the contract.

The goal is a Lean file that:

1. defines the **canonical term algebra** (mterm, aterm) with an exact
   semantics into an idealized commutative field;
2. proves the **soundness of each normalization step** against that
   semantics;
3. makes the **rule application order** an explicit, checkable object;
4. provides executable normalizers as oracles for corpus-scale parity
   tests against the Rust crate.

## 2. The formal core

### 2.1 Canonical terms

```adt
Factor ::= Var(x)                  (* opaque non-constant signal *)

Mterm  ::= MT(k, F)                (* k ∈ K; F : Factor →₀ ℤ, signed exponents *)

Aterm  ::= AT(S)                   (* S : Signature →₀ Mterm, keyed by factor part *)
```

`K` is the coefficient field. The Rust invariants become definitional:
the constant coefficient always lives in `k`; exponent 0 entries are
removed by `cleanup`; positive exponents are numerator factors, negative
are denominator. The `Aterm` map keyed by signature (the mterm minus its
coefficient) makes "terms with identical signatures merge by adding
coefficients" true by construction — and provable, rather than asserted in
a doc comment.

### 2.2 Exact semantics

The meaning function `⟦·⟧ρ : Mterm → K` (under an environment ρ assigning
field values to variables) is:

```math
⟦MT(k, F)⟧_ρ = k \cdot \prod_{x \in F} ρ(x)^{F(x)}
```

extended additively to aterms. Every operation of the Rust API surface
gets a soundness lemma against it:

```csv
Rust operation, Lean lemma, Statement
Mterm::mul_mterm / div_mterm, mul_sound / div_sound, ⟦m₁ · m₂⟧ = ⟦m₁⟧ · ⟦m₂⟧ (ρ nonzero on denominators)
Mterm::add_mterm (same signature), add_sound, ⟦m₁ + m₂⟧ = ⟦m₁⟧ + ⟦m₂⟧
gcd(m₁ m₂), gcd_divides, gcd(m₁,m₂) divides both arguments
Aterm::factorize(d), factorize_sound, ⟦factorize(A,d)⟧ = ⟦A⟧
Aterm::greatest_divisor, divisor_maximal, no strictly larger common divisor exists in the covered fragment
normalized_tree ∘ from_sig, roundtrip, ⟦normalize(t)⟧ = ⟦t⟧ for the covered constructors
```

Division requires the usual side condition (denominator factors evaluate
nonzero); the spec states it explicitly instead of inheriting it silently
from C++.

### 2.3 Termination and canonicity

Two structural theorems close the algebra:

- **Termination** — the rewrite engine's measure (the `complexity`
  function ported in `mterm`) strictly decreases on every applied rule; in
  Lean this is the well-founded recursion justification of the executable
  normalizer, so it is not optional.
- **Canonicity on the covered subset** — two terms with equal exact
  semantics *within the mterm/aterm fragment* normalize to syntactically
  equal canonical trees. This is a confluence statement made tractable by
  the map-based representation: it reduces to determinism of signature
  ordering (Rust `BTreeMap` iteration ↔ Lean sorted association lists,
  with `sig_order` as the shared total order).

## 3. Rule order as a first-class object

The single most valuable deliverable is the smallest one. The scheduling
stream's worst port bug — `needs_separate_loop` inlining signals that
`max_delay > 0` should have forced into their own loop — was a
**first-match order** inverted between C++ and Rust. The `simplify` engine
has exactly the same shape: an ordered cascade of pattern rules where the
first match wins, with documented exceptions (the `-1 · y` sign-propagation
rule that must *not* be folded like other constant multiplications).

The spec therefore models the rule cascade as an explicit ordered list:

```
def simplifyRules : List Rule := [ … ]        -- order is normative
def simplifyStepB (t : Term) : Option Term    -- first matching rule fires
```

with a theorem that `simplifyStepB` fires the *earliest* applicable rule,
and an exhaustive small-term enumeration (all terms up to depth 3 over a
small alphabet, constants drawn from a boundary set including 0, ±1)
checking the Rust engine picks the same rule index on every input. The
`-1 · y` exception becomes a named rule with its own regression `#guard`.

## 4. Connection to Rust

- **Fixture parity (L1)** — the existing `mterm`/`aterm` unit tests are
  mirrored as Lean `#guard`s on the same inputs, so both languages agree
  on the concrete algebra before any theorem is attempted.
- **Rule-index parity (L1+)** — the exhaustive small-term enumeration of
  §3, comparing rule *selection*, not only final results. Two engines can
  agree on outputs while disagreeing on paths, and path disagreements are
  latent bugs waiting for a new rule.
- **Corpus certificates (L2/L3)** — export `(input hash, canonical
  normalized tree)` pairs per DSP from `normalform`; an independent Rust
  checker verifies the pair semantically (re-evaluating both sides on
  rational test points), and the shared Lean importer replays
  normalization on the mirrored algebra. This reuses the certificate
  machinery of the vector stream unchanged.

## 5. Staged gates

```csv
Gate, Deliverable, Acceptance
N0, spec skeleton: Mterm/Aterm + exact semantics compiling, no sorry/axiom; adequacy checklist passes (no vacuous Prop)
N1, soundness lemmas for the mterm layer + fixture parity, mterm table of §2.2 fully proved; Rust/Lean fixtures agree
N2, ordered rule cascade + rule-index enumeration vs Rust simplify, enumeration green; -1·y exception guarded; termination proved
N3, aterm/normalform certificates checked at L2 and L3, corpus green in CI on both sides
```

## 6. Risks and boundaries

::: important [Exact algebra, not floating point]
Every soundness lemma is stated over an idealized field. The C++ compiler
itself applies these rewrites to expressions that will execute in floating
point, so *exact-vs-float divergence is inherited behavior, not a port
bug* — but the spec must say so explicitly. A theorem `⟦m₁ · m₂⟧ = ⟦m₁⟧ ·
⟦m₂⟧` over K licenses nothing about f32 output; the impulse-test corpus
remains the sole authority on numeric parity.
:::

- **Scope control.** The term algebra covers the mterm/aterm fragment and
  the simplify cascade — not delay-line normalization heuristics, not
  `rec_merge` (isomorphic recursive-group merging), and not typed-phase
  promotion in `normalform`. Each of those is a candidate *extension*,
  gated separately, never a silent scope growth.
- **Moving target.** `aterm`/`simplify`/`normalform` are still being
  ported. The spec should be written against the C++ contract (the API
  mapping tables already in the Rust doc comments), so it leads the port
  rather than chasing it — that is the point of doing it now.
- **Largest of the three proposals.** Termination and canonicity proofs
  are real work; the gates are ordered so that N1/N2 already pay for
  themselves (rule-order safety) even if N3 slips.
