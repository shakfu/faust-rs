---
title: "Lean formalization proposal: interval arithmetic"
date: 2026-07-19
page-size: A4
margins: 20 22
page-numbers: true
font-body: Roboto
font-heading: Roboto Condensed
font-mono: Roboto Mono
---

**Date:** 2026-07-19

**Status:** proposal with its I0 skeleton delivered:
[`interval-arithmetic-formal-spec.lean`](interval-arithmetic-formal-spec.lean)
compiles (Lean 4.31, bundled Std, no `sorry`/`axiom`) with the linear core
(`neg`/`add`/`sub`/`union`/`inter`), the monotone-unary lemma and its
tightness companion proved over `Int` endpoints, and `mul`/`div` inclusion
as named gate-I2 obligations. The gate-0 adequacy review is still pending;
the `R-real` regime over real endpoints and the `lsb` ledger are not in the
skeleton (§6).

**Companions:** the methodology overview
[`../docs/lean-usage-methodology-en.md`](../docs/lean-usage-methodology-en.md),
the roadmap
[`lean-formalization-roadmap-2026-07-19-en.md`](lean-formalization-roadmap-2026-07-19-en.md),
and the original port plan
[`interval-port-plan-2026-03-12-en.md`](interval-port-plan-2026-03-12-en.md).

::: toc+
- **Scope and goal** — why interval is the best-shaped candidate.
- **The formal core** — the interval domain, the inclusion property, and the theorems to prove.
- **Three proof regimes** — reals, integers, and declared placeholders.
- **Connection to Rust** — oracle strategy against the `interval` crate.
- **Staged gates** — I0 to I3.
- **Risks and boundaries** — floats, lsb precision, and downstream stakes.
:::

## 1. Scope and goal

The [`interval`](../crates/interval/src/lib.rs) crate is a completed,
standalone port of C++ `compiler/interval/`: the `Interval` type (bounds +
`lsb` precision, empty encoded via NaN), set operations, and the operator
algebra in `ops/{arithmetic,casts,logic,math,trig,ui,delay_table}.rs`,
with 62 passing unit tests. Its results are not cosmetic: downstream they
size **delay lines and tables** and will drive `SIGDELAY` migration once
intervals are attached to `signal_prepare` (the next step of the original
port plan). An unsound interval is a heap overflow or a wrong table size
waiting for the corpus gap that exposes it.

Interval soundness is also the most classical formalization target of the
three proposals — the property is one sentence, and the proof-assistant
literature has mechanized it many times. The goal is a Lean file that:

1. defines the interval domain and its lattice structure;
2. states the **inclusion property** once, as the single normative
   contract every operator must satisfy;
3. proves it for the arithmetic/comparison/cast core, and *decides* it
   exhaustively for the integer bitwise core;
4. names explicitly the operators that do **not** satisfy it today
   (the `missing.rs` placeholders), so unsoundness is a tracked fact
   rather than a surprise.

## 2. The formal core

### 2.1 Domain

```adt
IntervalL ::= Empty                    (* C++ empty interval, NaN bounds *)
            | Iv(lo, hi, lsb)          (* lo ≤ hi ∈ ℝ; lsb ∈ ℤ *)
```

Membership `x ∈ I` ignores `lsb` (precision is a separate ledger, §2.3).
Set operations give the lattice: `union`/`intersection` with the usual
absorption and monotonicity laws, `Empty` as bottom, the default interval
`[MIN, MAX]` as the practical top. These small lemmas come first because
every operator proof leans on them.

### 2.2 The inclusion property

For a real function `f : ℝᵏ → ℝ` and its interval transfer function
`F : IntervalLᵏ → IntervalL`, the single normative contract is:

```math
\forall I_1 … I_k,\; \forall x_1 ∈ I_1, …, x_k ∈ I_k:\quad
f(x_1, …, x_k) \;∈\; F(I_1, …, I_k)
```

plus **inclusion monotonicity** (`Iᵢ ⊆ Jᵢ` implies `F(I) ⊆ F(J)`), which
is what makes fixpoint iteration on recursive signals sound. For the
monotone unary operators the crate routes through
`exact_precision_unary` (which takes a plain `fn(f64) -> f64`, not a
closure — a port constraint worth preserving in the spec's shape), a
single generic lemma covers the whole family:

::: proposition [Monotone transfer]
If `f` is monotone on `[lo, hi]`, then `F([lo, hi]) = [f lo, f hi]`
satisfies inclusion, and is the *tightest* interval that does.
:::

Non-monotone operators each get their case analysis: `mul` (four sign
cases), `div` (zero-crossing denominators → union or top), `mod`, and the
trigonometric family (period counting, the actual source of subtle C++
bugs). Tightness (the result is not just sound but minimal) is proved
where the C++ is tight and *explicitly not claimed* where the C++
over-approximates — the spec documents which, operator by operator, ending
the current situation where only a careful reading of `interval*.cpp`
reveals it.

### 2.3 The lsb precision ledger

`lsb` is kept out of membership on purpose: it is a best-effort fixed-point
precision annotation, combined with saturated arithmetic in the port
(`saturated_precision_add/sub`, a deliberate divergence from C++ unchecked
`int` that the doc comments already record). The spec models it as a
separate monotone ledger with its own small lemmas, so precision claims
can never contaminate soundness claims.

## 3. Three proof regimes

Not every operator deserves the same treatment, and pretending otherwise
would stall the stream. The spec declares the regime per operator family:

```csv
Regime, Families, Method, Assurance vocabulary
R-real, arithmetic; casts; comparisons; math; trig, inclusion proved over ℝ, kernel checked
R-int, bitwise (bitwise.rs); logic on machine ints, decidable statement over ℤ/2ⁿ; exhaustive for small widths + structural lemmas for 32-bit, kernel checked (decide) / runtime certified
R-declared, missing.rs placeholders returning interval(0); ui; delay_table heuristics, no soundness claim; each listed with a named Prop obligation, formally specified only
```

The R-declared list is the honesty mechanism: `missing.rs` currently
returns `interval(0)` to match C++ — sound for nothing except the constant
zero. Every such operator gets a named open obligation in the spec, and
the roadmap treats burning down that list as measurable progress. No
document may describe the interval layer as "verified" while the list is
non-empty.

## 4. Connection to Rust

- **Fixture parity (L1)** — the 62 existing unit tests mirrored as Lean
  `#guard`s, run in both directions.
- **Property bridge (L1+)** — a Rust proptest suite generating (interval,
  point-in-interval) pairs per operator and checking membership of the
  result; counterexamples are exported as JSON fixtures that become
  permanent Lean `#guard` regressions. The spec is on ℝ, the tests on f64
  — this bridge is precisely where the rounding gap of §6 is monitored.
- **Annotation certificates (L2/L3)** — once intervals attach to
  `signal_prepare`, export per-node interval annotations for the corpus;
  an independent Rust checker re-derives each node's interval from its
  children (one step, no fixpoint), and the shared Lean importer replays
  the same step against the spec's transfer functions. Delay/table sizing
  decisions then cite a checked artifact instead of an internal value.

## 5. Staged gates

```csv
Gate, Deliverable, Acceptance
I0, spec skeleton: domain + lattice + inclusion contract compiling, no sorry/axiom; adequacy checklist passes; R-declared list complete
I1, R-real core proved (add/sub/mul/div/casts/comparisons + monotone lemma), theorems kernel-checked; 62-fixture parity green both ways
I2, trig/mod case analyses + R-int decidable core + proptest bridge, remaining R-real families proved; bitwise decided; bridge in CI
I3, signal_prepare annotation certificates at L2 and L3, corpus green in CI; delay/table sizing consumes checked annotations
```

## 6. Risks and boundaries

- **ℝ vs f64.** The spec proves inclusion over the reals; the crate
  computes with f64 endpoints without directed rounding — faithfully to
  C++. A real-valued theorem plus an unlucky rounding on an endpoint can
  still exclude an attainable value. The property bridge of §4 is the
  standing detector; if it ever fires, the fix (endpoint widening by one
  ulp) is a *port policy decision* to take with the C++ reference in view,
  not something the spec can decide alone.
- **Tightness inflation.** Inclusion is cheap to over-satisfy (return top
  everywhere). The spec must pair every soundness theorem with either a
  tightness theorem or an explicit non-tightness note, or it degenerates
  into the vacuity trap the adequacy reviews exist to catch.
- **Fixpoint semantics deferred.** Recursive-signal interval iteration
  (widening, convergence) belongs to the `signal_prepare` attachment
  work, not to this spec. I3 checks *one derivation step* per node; the
  fixpoint engine gets its own contract when that port lands.
- **Cheapest, highest-leverage stream.** The crate is frozen, standalone,
  and already fully tested — the spec can be written against stable code
  with no moving target, which is why the roadmap schedules it first.
