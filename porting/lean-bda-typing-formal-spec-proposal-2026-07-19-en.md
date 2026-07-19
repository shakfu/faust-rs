---
title: "Lean formalization proposal: block-diagram algebra typing"
date: 2026-07-19
page-size: A4
margins: 20 22
page-numbers: true
font-body: Roboto
font-heading: Roboto Condensed
font-mono: Roboto Mono
---

**Date:** 2026-07-19

**Status:** proposal with its B0 skeleton delivered:
[`bda-typing-formal-spec.lean`](bda-typing-formal-spec.lean) compiles
(Lean 4.31, bundled Std, no `sorry`/`axiom`) with the syntax, both arity
judgments, scoping, and the soundness/completeness/functionality theorems
of §2 proved. The gate-0 adequacy review is still pending; in the Lean file
the recursion constructor is spelled `recur` (`rec` collides with the
generated recursor).

**Companions:** the methodology overview
[`../docs/lean-usage-methodology-en.md`](../docs/lean-usage-methodology-en.md),
the roadmap
[`lean-formalization-roadmap-2026-07-19-en.md`](lean-formalization-roadmap-2026-07-19-en.md),
and the De Bruijn recursion notes in `docs/`.

::: toc+
- **Scope and goal** — what is formalized, what is deliberately excluded.
- **The formal core** — box syntax, arity judgment, scoping, and the theorems to prove.
- **Connection to Rust** — oracle strategy against `propagate::arity` and `eval`.
- **Staged gates** — B0 to B3.
- **Risks and boundaries** — where this spec must stop.
:::

## 1. Scope and goal

The block-diagram algebra (BDA) is the mathematical heart of Faust: five
composition operators over boxes, each with an exact arity rule, plus
recursion with precise scoping semantics. In the port these rules live in
[`crates/propagate/src/arity.rs`](../crates/propagate/src/arity.rs)
(`box_arity_typed`, `box_arity_wiring`) and in the `eval` crate's
application machinery (`infer_box_arity`), including the port-specific
**FAD/RAD arity extensions** that do not exist in upstream C++ teaching
material and are documented only in code comments.

The goal is a single Lean file that:

1. defines the box syntax and the **arity typing judgment** as the
   normative statement of what `box_arity_typed` computes;
2. defines **well-scopedness** for De Bruijn recursion references;
3. proves the judgment functional (at most one arity per box) and total on
   well-formed boxes;
4. provides an executable `boxArityB` usable as an oracle for exhaustive
   Rust parity tests.

Excluded: evaluation itself (pattern matching, environments, lambda
application), UI widgets, symbolic-name resolution, and everything about
*signal values*. This spec types boxes; it does not run them.

## 2. The formal core

### 2.1 Syntax

```adt
Box ::= Const(k)                  (* k ∈ ℤ ∪ ℝ, arity (0,1) *)
      | Wire                      (* _, arity (1,1) *)
      | Cut                       (* !, arity (1,0) *)
      | Prim(p, m, n)             (* primitive with declared arity (m,n) *)
      | Seq(Box, Box)             (* A : B *)
      | Par(Box, Box)             (* A , B *)
      | Split(Box, Box)           (* A <: B *)
      | Merge(Box, Box)           (* A :> B *)
      | Rec(Box, Box)             (* A ~ B *)
      | Ref(i)                    (* De Bruijn recursion reference, i ∈ ℕ *)
      | Fad(Box, Box)             (* fad(body, seed) — port extension *)
      | Rad(Box, Box)             (* rad(body, seeds) — port extension *)
```

`Prim` abstracts the primitive alphabet: what matters to this spec is the
declared arity, not the operation. Binary arithmetic is `Prim(+, 2, 1)`,
and so on. This keeps the spec closed under new primitives without ever
becoming a shadow signal language.

### 2.2 The arity judgment

The judgment `⊢ b : (m, n)` reads "box b consumes m inputs and produces n
outputs". The five composition rules:

```inference (T-Seq)
⊢ A : (m, n); ⊢ B : (n, q)
---
⊢ Seq(A, B) : (m, q)
```

```inference (T-Par)
⊢ A : (m, n); ⊢ B : (p, q)
---
⊢ Par(A, B) : (m + p, n + q)
```

```inference (T-Split)
⊢ A : (m, n); ⊢ B : (p, q); n > 0; n ∣ p
---
⊢ Split(A, B) : (m, q)
```

```inference (T-Merge)
⊢ A : (m, n); ⊢ B : (p, q); p > 0; p ∣ n
---
⊢ Merge(A, B) : (m, q)
```

```inference (T-Rec)
⊢ A : (m, n); ⊢ B : (p, q); q ≤ m; p ≤ n
---
⊢ Rec(A, B) : (m - q, n)
```

And the port-specific AD extensions, transcribed from
`propagate/src/arity.rs` (which follows C++ `boxtype.cpp:371` for `fad`):

```inference (T-Fad)
⊢ A : (m, n); ⊢ S : (p, s)
---
⊢ Fad(A, S) : (m, n · (1 + s))
```

```inference (T-Rad)
⊢ A : (m, n); ⊢ S : (p, s)
---
⊢ Rad(A, S) : (m, n + s)
```

A second judgment `⊢ʷ` ("wiring view") types `Fad` transparently as
`(m, n)` — it is the Lean counterpart of `box_arity_wiring`, used by the
`RecFadMode::ExpandAfterRec` path where the recursive port algebra is
computed on primal lanes only. The relationship between the two views on
`Rec` nodes is itself a theorem target (see B2).

### 2.3 Scoping

Recursion references use De Bruijn indices. Well-scopedness is a separate
judgment `d ⊢ b wf` (d = enclosing recursion depth): `Ref(i)` is
well-formed iff `i < d`, each `Rec` increments `d` for the appropriate
subterm, and every other constructor passes `d` through. Lifting and
substitution lemmas connect this to the symbolic-name lowering described
in the De Bruijn notes in `docs/`.

### 2.4 Executable check and theorems

`boxArityB : Box → Option (Nat × Nat)` is the executable oracle
(first-match, same failure points as the Rust `PropagateError` taxonomy).
The proof obligations, in priority order:

1. **Functionality** — `hasArity_functional`: `⊢ b : (m, n)` and
   `⊢ b : (m', n')` imply `(m, n) = (m', n')`. This is the theorem the
   vector-stream adequacy review showed must never be assumed (the free
   clock parameter bug).
2. **Soundness/completeness of the checker** —
   `boxArityB b = some (m, n) ↔ ⊢ b : (m, n)`, stated against the
   relational judgment, not by `rfl`.
3. **Wiring/typed agreement** — on `Fad`-free boxes, `⊢` and `⊢ʷ`
   coincide; on any box, the input arities agree.
4. **Scoped totality** — a closed (`0 ⊢ b wf`) box built from arity-typed
   primitives either has an arity or fails on a named side condition
   (`n = 0` split, non-divisibility, rec port mismatch).

## 3. Connection to Rust

Three bridges, in increasing strength:

- **Fixture parity (L1)** — a small library of hand-typed boxes (including
  every side-condition failure) shared as JSON between a Rust test in
  `propagate` and Lean `#guard`s.
- **Exhaustive enumeration (L1+)** — enumerate all boxes up to depth 3
  over a fixed primitive alphabet with arities ≤ 4 (the domain is finite
  and small), and compare `boxArityB` with `box_arity_typed` verdict for
  verdict, including the error class. This is the analogue of the 33,867
  DAG enumeration that bound the scheduling spec to Rust.
- **Corpus certificates (L2/L3)** — extend the compiler to export, per
  compiled DSP, the flat post-eval box tree skeleton with the arity
  decision at every node; a small independent Rust checker replays the
  judgment, and the shared Lean JSON importer (see roadmap §4) re-checks
  the same artifact. Divergence anywhere in the 132-DSP corpus fails CI.

The FAD/RAD rules deserve special attention: they are port-authored, load
bearing for the whole differentiation stack, and currently documented only
in a doc comment. The spec makes them normative.

## 4. Staged gates

```csv
Gate, Deliverable, Acceptance
B0, plan review + spec skeleton (syntax + judgments) compiling, no sorry/axiom; adequacy checklist §5 of the roadmap passes
B1, executable boxArityB + functionality and checker theorems, all theorems kernel-checked; fixture parity green in both languages
B2, wiring/typed agreement + scoping lemmas + exhaustive depth-3 enumeration vs Rust, enumeration green; every Rust error class reached at least once
B3, corpus arity certificates checked by Rust (L2) and Lean (L3), 132-DSP corpus green in CI on both sides
```

## 5. Risks and boundaries

- **Shadow-AST pull.** The `Box` type must stay an arity skeleton. The
  moment it grows evaluation semantics, it becomes a second interpreter —
  the explicit anti-goal of the working rules.
- **`eval` is out of scope on purpose.** `infer_box_arity_for_apply`
  probes arity *during* evaluation of not-yet-flat terms; the spec only
  covers validated post-eval flat boxes (`FlatBoxId`), where the C++
  `getBoxType` contract is clean. Extending to mid-eval probing would drag
  environments into the spec for little assurance gain.
- **Arity ≠ semantics.** `Split`'s modulo fan-out duplication order and
  `Merge`'s summation are *propagation* behavior, verified by the existing
  impulse corpus, not by this spec.
