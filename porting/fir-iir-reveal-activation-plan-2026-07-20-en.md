# FIR/IIR Form Recognition in faust-rs: Analysis and Activation Plan

Date: 2026-07-20

## Scope

`faust-rs` already contains a real body of code that recognizes FIR and IIR
forms in signals — a recursive-linearity classifier, an affine-state
extractor, a full carrier algebra, and downstream tolerance for the carrier
nodes. Almost none of it is active: the extractor is called only by tests,
the carriers are never produced from a real DSP program, and no compiler flag
exposes any of it.

This document first inventories that existing Rust code precisely (what
recognizes what, on which representation, and how it is currently reached),
then gives a phased plan to put it into activity on two tracks:

- **Track A — activate the dormant RAD recognition path** (fastest: the
  recognition and transposition code is already written and tested);
- **Track B — activate pipeline-level reveal** (produce `sigFIR`/`sigIIR`
  carriers in the main compilation pipeline, C++ `-fir` parity, toward
  filter-aware codegen).

It extends `porting/lti-filter-intermediate-form-plan-2026-05-06-en.md`
("the LTI plan"), whose Phase L1/L3 delivered the algebra analyzed below.

## Part 1 — Analysis of the Existing Recognition Code

### 1.1 E0 classifier: `propagate/src/stateful_rad.rs` (ACTIVE, diagnostics only)

`classify_de_bruijn_rec_group` walks one raw `DEBRUIJNREC` group — plain
`Proj`/`BinOp`/`Delay1`/`Delay`/`Prefix` signals, before `signal_prepare` —
and classifies it on the lattice:

- `RadRecLinearity::LinearLti` — every current-recursion back-edge is linear
  under constant coefficients: this *is* IIR-form recognition (the group is a
  linear time-invariant recurrence);
- `LinearTimeVarying` — linear back-edge, at least one non-constant
  coefficient;
- `Nonlinear` — a back-edge crosses a nonlinear primitive, comparison,
  discrete cast, or branch.

Each node is summarized by an `ExprClass` (depends-on-current-state ×
worst linearity × independent-part variability), with a small rule table for
`Add/Sub` (additive), `Mul` (multiplicative), `Div` (denominator),
`Delay1`/`Delay`/`Prefix` (temporal shift; state-dependent delay amount is
nonlinear). It also has explicit carrier arms — `classify_fir_carrier`
(`stateful_rad.rs:651`) and `classify_iir_carrier` — so already-revealed
carriers classify correctly.

**Activity status:** reached in production, but only as a *gate for
diagnostics*: `reverse_ad` uses the derived `RecRadMode` to pick an error
kind (`recursive-linear-transpose`, `recursive-block-linear-time-varying`,
`bptt-required`) or to route to the `SigBlockReverseAD` fallback. The
classification result is never used to *build* anything in production.

### 1.2 E1 extractor: `propagate/src/transpose_ad.rs` (DORMANT)

This is the real IIR-form *extractor* on raw signals:

- `extract_affine_state_terms` (`transpose_ad.rs:461`) decomposes one
  recursive branch into affine `LinearTerm { source_output, state_slot,
  coeff }` contributions. It recognizes: direct `Proj(slot, REF)` state
  reads; `Add`/`Sub` (with coefficient negation); `Mul`/`Div` by
  state-independent expressions (coefficient accumulation);
  `Delay1(state)` as the canonical one-sample state read;
  `FloatCast`/`Output`/`Lowest`/`Highest` transparency. Anything else that
  touches the current state is a structured `TransposeAdError` — the module
  explicitly refuses to drop a state term silently, because a missing term
  would mean a wrong gradient.
- `transpose_lti_de_bruijn_rec_scaffold` / `..._with_cotangents`
  (`transpose_ad.rs:272/324`) build the transposed group
  `y_bar[n] = cotangent[n] + A^T * y_bar[n+1]` from the extracted matrix,
  guarded by an E0 `LinearLti` pre-check (a producer/checker pair: the
  classifier answers "possible?", the extractor independently answers "can I
  build it?").
- `iir_filter_to_de_bruijn_rec_group` (`transpose_ad.rs:361`) bridges a
  typed `IirFilter` carrier through `iir_filter_to_state_space` (order ≤ 2)
  into the same `DEBRUIJNREC` syntax the transpose accepts — i.e. the
  carrier-to-recurrence direction already exists.

**Activity status: dormant.** `reverse_ad.rs:67-68` states it plainly: the
"legacy `ReverseTimeRec` LTI/IIR path remains as dormant helper
infrastructure". The `build_lti_recursive_adjoint_*` wrappers
(`reverse_ad.rs:760/808/841`) are `pub(super)` and are called **only from
`mod tests`**; production `rad(...)` routes every temporal/recursive shape to
the `SigBlockReverseAD` numeric fallback and rejects `Iir` carriers in the
local sweep (`reverse_ad.rs:289`, kind `iir-state-space`). The extensive
in-module validation (analytic adjoint fixtures, cross-coupled and diagonal
LTI groups, rejection tests) runs only under `cargo test -p propagate`.

### 1.3 Carrier representation and algebra: `crates/signals` (LIBRARY ONLY)

- Carrier nodes `SIGFIR` / `SIGIIR` (`signals/src/lib.rs`, `SigBuilder::fir/
  iir`, `SigMatch::Fir/Iir`), layouts `[base, tap0, ...]` and
  `[recursive_target, input, fb...]`; `SIGCLOCKED` exists.
- `signals/src/filter_algebra.rs` (LTI plan L1/L3): typed views
  `FirFilter` / `IirFilter`; FIR helpers `make/delay/simplify/neg/add/sub/
  convert_fir_to_sig`; IIR helpers `proj_to_sig_iir`, `concerned_iir`,
  `delay/add/sub/mul/div_sig_iir`, `embedded_iir`; state-space view
  `iir_filter_to_state_space` (SISO, order ≤ 2, explicit rejection above).

The FIR *recognition* logic is latent in these helpers — `make_sig_fir`
turns `x@d` into a carrier, `delay_sig_fir` shifts taps, `add_sig_fir`
merges same-base FIRs — but **no driver ever walks a program applying
them**. Missing vs the C++ helper surface: `mul_sig_fir`, `div_sig_fir`,
`combine_firs`.

### 1.4 Downstream tolerance (TRAVERSAL, NO LOWERING)

Carriers are typed and traversed if they appear, but cannot be compiled:

| Consumer | Location | Behavior |
|---|---|---|
| Typing | `sigtype/src/rules.rs:358-359` | infers carrier types |
| Normal form | `normalize/src/normalform.rs:337-343` | promotes carrier children |
| Prepared-forest verify | `signal_prepare/verify.rs:309` | traverses coefs |
| Clock env | `clk_env/mod.rs:658` | traverses coefs |
| Vector analysis | `signal_fir/vector/analysis/effects.rs:272-273`, `dependencies.rs:442-448` | classifies as `StateCell::Fir/Iir` |
| Scalar/vector lowering | — | **no `Fir`/`Iir` arm**: dies as `UnsupportedSignalNode` |

### 1.5 What has no Rust counterpart at all

The C++ driver passes that *produce* carriers from a program
(faust-YO `compiler/transform/`): `revealSum` (n-ary `sigSum` flattening),
`revealFIR` (delay/product/sum-grouping rules), `revealIIR` (recursive
projection whose definition is "one self-FIR + independent terms"),
`factorizeFIRIIRs` (common tap factoring); flags `-fir` / `-ff`
(`global.cpp:452/464`), pipeline placement after constant propagation
(`instructions_compiler.cpp:119-152`); prerequisites `getSigOrder`
(`sigorderrules.cpp`, memoized structural order 0-3, no environments) and
`isDependingOn` (`sigRecursiveDependencies.hh:64`); filter codegen
(`compile_scal_fir.cpp` / `compile_scal_iir.cpp`). None of these exist in
Rust, and there is no `sigSum` node.

### 1.6 Summary picture

```text
              recognition            representation          consumption
  raw De Bruijn rec ──E0 classify──► RadRecLinearity ──────► diagnostics only   (ACTIVE)
  raw De Bruijn rec ──E1 extract───► LinearTerm/A^T rec ───► tests only         (DORMANT)
  IirFilter carrier ──state-space──► DEBRUIJNREC bridge ───► tests only         (DORMANT)
  real DSP program  ──(nothing)────► sigFIR/sigIIR ────────► RAD classifiers,   (NO PRODUCER)
                                                             typing, traversal,
                                                             no lowering
```

The shortest path to "activity" is therefore not writing new recognition
code — it is wiring what exists (Track A) and giving it a producer front-end
(Track B).

## Part 2 — Activation Plan

Methodology per the established phase discipline: every producer ships with
an independent checker and rejecting-mutation tests before qualification.
Known validation traps apply throughout: cached `.ir` false greens (clean
rebuild after mutations, watch the mtime trap), structural certification is
not numeric proof, and typed walkers must reject unknown node kinds instead
of skipping them.

### Track A — activate the dormant RAD recognition path

The recognition (E0) and extraction/transposition (E1) code is written,
documented, and test-validated. Activation means letting production
`rad(...)` use it for `LinearLti` groups instead of unconditionally falling
back to `SigBlockReverseAD`.

**A1 — wiring.** In `reverse_ad::generate_rad_signals`, when the recursive
group classifies `LinearLti` and the E1 extractor succeeds, lower through
`build_lti_recursive_adjoint_projections`; on any `TransposeAdError`, keep
the existing `SigBlockReverseAD` fallback (never a new rejection). The
block/tape reverse-time convention already exists in production via the
BlockReverseAD/`ReverseTimeRec` machinery validated on the ondemand branch;
A1 must reuse that evaluator contract, not invent a second one.

**A2 — independent check.** The transposed-group producer is checked by a
separate walker asserting: same arity as the primal group; every emitted
branch is `input(slot) + Σ coeff · Proj(src, REF)` with coefficients free of
current-state references; every primal `LinearTerm` appears transposed
exactly once (no silent drops — re-extract from the emitted group and
compare matrices).

**A3 — qualification.** Gradient-vs-finite-difference harness on LTI corpus
programs (`fi.pole`, `fi.iir((1),(p,q))`, biquads from the `fad_biquad*`
corpus files); numeric agreement of the E1 path vs the BlockReverseAD
fallback on the same programs (two independent implementations of the same
adjoint — a strong cross-check); rejecting mutations (sign flip in
`Sub` coefficient negation at `transpose_ad.rs:489-502`, dropped
`LinearTerm`, swapped `source_output`/`state_slot`) must be caught by A2 or
the numeric harness.

**A4 — carrier entry.** Accept `SigMatch::Iir` in the RAD sweep by routing
`extract_iir_filter → iir_filter_to_de_bruijn_rec_group → A1` instead of
rejecting with `iir-state-space` (`reverse_ad.rs:289`). Order > 2 keeps its
explicit diagnostic. This makes the LTI plan's L2 target
(`rad(_ : fi.iir((1),(p,q)), (p,q))`) compile through the exact path.

Track A touches only `propagate`, needs no new node kinds, no flags, and no
backend work. It should land first.

### Track B — activate pipeline-level reveal (carrier production)

Goal: real DSP programs produce `sigFIR`/`sigIIR` carriers in the main
pipeline, gated by C++-parity flags, first numerically inert, then feeding
codegen. The C++ reveal passes are the reference; the Rust helpers of
§1.3 are the algebra they drive.

**Design decisions**

- **D1 — passes live in `signals`** (new modules `sig_order`, `depend`,
  `reveal`): `propagate` depends on `signals` but not on
  `normalize`/`sigtype` (`propagate/Cargo.toml`), and RAD-side reveal (LTI
  plan L2) must be callable from `propagate`. C++ `getSigOrder` needs no
  environments, so the port adds no dependency edge.
- **D2 — `sigSum` is introduced** as a real n-ary node (`SigMatch::Sum`);
  every `SigMatch` consumer in the §1.4 table plus `dump_sig` and the RAD
  classifiers gets an explicit `Sum` arm (support or rejection) in the same
  patch. Checkers must *fail* on unexpected `Sum`, never walk past.
- **D3 — expansion fence**: while codegen support is absent, the reveal
  block is immediately followed by re-expansion (`convert_fir_to_sig`, new
  `convert_sum_to_sig` and `convert_iir_to_sig`), so `-fir` is numerically
  inert and qualifiable corpus-wide before any backend change. The fence is
  lifted per-shape in B6, never wholesale.
- **D4 — IIR slot-0 convention**: C++ `revealIIR` now emits `nil` in slot 0;
  Rust keeps its `[recursive_target, input, fb...]` convention (already
  consumed by `stateful_rad`/`transpose_ad`). Differential tests normalize
  slot 0. Recorded so a future C++ re-sync does not "fix" it backwards.
- **D5 — flags**: `-fir` / `-ff` parsed in `cli/args.rs`, default off, `-ff`
  requires `-fir`; threaded into a new `prepare_signals_for_fir_with`
  entry point (existing entries unchanged).
- **D6 — placement**: inside `signal_prepare` after `2.10 simplify #2` and
  before `2.11 canon_one_sample_delays` (reveal must see `Delay(x, d)`
  before the `Delay1` canonicalization hides one-sample delays; the staged
  driver re-types after the mutation). `reveal_sum` runs only under `-fir`
  (C++ runs it unconditionally; with the fence that would be pure churn).

**Phases**

- **B0 — prerequisites** (no pipeline change): port `sig_order`
  (`sigorderrules.cpp`; memoized `HashMap<SigId, u8>`; carrier arms
  included) and `is_depending_on` (projection reachability, sees through
  `Clocked` and carriers); add `mul_sig_fir` (both C++ cases of
  `sigFIR.cpp:446`: single-nonzero-tap × anything, multi-tap × numeric
  literal, else plain `Mul`), `div_sig_fir`, `combine_firs`, with provenance
  rustdoc and per-branch unit tests.
- **B1 — `sigSum` + `reveal_sum` + checker**: port `SumRevealer`
  (flatten `Add`/`Sub`, negate subtracted subterms, deterministic rec-group
  renaming); `convert_sum_to_sig` fence half; checker `check_sum_form` (no
  `Add`/`Sub` over a `Sum` operand, no nested `Sum`; after the fence, no
  `Sum` at all). Rejecting mutations: broken negation, dropped subterm.
- **B2 — `reveal_fir` + checker**: port `FIRRevealer::postprocess`
  (`revealFIR.cpp:108-263`) in C++ rule order — clock rules
  (`Clocked` push-through, product re-association), `Delay → delay_sig_fir`,
  FIR × / ÷ coefficient, order-based promotion (`sig_order(x) < 3` ×
  order-3 base → 1-tap carrier), sum grouping by base via `add_sig_fir`.
  Checker `check_fir_form`: carrier well-formedness *plus* numeric
  equivalence of `convert_fir_to_sig(carrier)` vs the original subtree on
  the impulse harness. Rejecting mutations: tap-shift off-by-one, swapped
  promotion operands, dropped coefficient in grouping.
- **B3 — `reveal_iir` + checker**: port `IIRRevealer::postprocess`
  (`revealIIR.cpp:162-204`): for `proj(p, rec)` defined by a `Sum`,
  partition terms into self-FIRs `R` / other state-dependent `D`
  (`is_depending_on`) / independent `L`; rewrite iff
  `|R| == 1 && |D| == 0 && |L| > 0`, storing the replaced projection in
  slot 0 (D4). `convert_iir_to_sig` fence half. Checker re-verifies
  independence of input and feedback terms with `is_depending_on` (not
  trusted from the producer). Rejecting mutations: `D` term folded into
  `L`, feedback sign flip.
- **B4 — `factorize_fir_iirs`** (`-ff`): port `FIRFactorizer`
  (most-common-tap analysis, nested `FIR([FIR(newC), c])`); requires
  nested-FIR acceptance in typing, `check_fir_form`, and a recursive fence.
  Rejecting mutation: factor extracted while `maxocc != nbfactors`.
- **B5 — activation behind `-fir`, fence on**: flags (D5), stage insertion
  (D6), a reveal-stage debug dump. Gates: flag **off** → byte-identical
  emitted code on the impulse-tests corpus (93 DSPs, C++ 4-pass oracle) and
  the 132-DSP certification corpus; flag **on** → numeric parity corpus-wide
  on all certified backends; differential vs `faust-YO -fir` normal-form
  dumps on a curated filter list (modulo D4 and rec-group naming); the
  emission-determinism gate must hold (reveal renames recursion groups —
  reuse the deterministic fresh-name discipline from `merge_iso_rec`).
- **B6 — consumers, fence lifted per shape**: (1) RAD-side reveal in
  `propagate` for E1 candidates that only classify `LinearLti` *after*
  reconstruction (LTI plan L2), joining Track A's wiring; (2) scalar
  codegen port of `compile_scal_fir.cpp` `generateFIR` (gain special case,
  unrolled small/sparse filters, coefficient table + accumulation loop,
  storage class from coefficient variability) then `compile_scal_iir.cpp`,
  each lifted shape carrying FIR-dump structural tests, numeric parity, and
  CPU measurements; (3) vector mode: real lowering behind
  `StateCell::Fir/Iir`, then 16-mode matrix re-certification.

### Risks

- **`sigSum` ripple** across every `SigMatch` consumer — mitigated by D2's
  one-dedicated-patch rule with rejecting arms.
- **Rec-group renaming churn** (all C++ revealers rename groups) vs the
  emission-determinism gate — deterministic naming required, checked in B5.
- **Clock interactions**: half the `revealFIR` rules exist because of
  `sigClocked`; the ondemand/clock-domain stream multiplies clocked shapes.
  B2 fixtures must include clocked FIRs; `clock_ad` checker independence
  must be preserved.
- **Track A convention risk**: the transposed group reads `Proj` edges as
  `y_bar[n+1]`; wiring must reuse the validated BlockReverseAD reverse-time
  evaluator contract, not assume forward-time semantics.
- **`-fir` on non-filter programs** must degrade to identity — covered by
  running the *whole* corpus flag-on in B5, not a filter-only list.
- **Moving C++ reference** (large `#if 0` regions, slot-0 `nil` change):
  provenance comments must cite the exact faust-YO revision.

### Recommended first patches

1. **Track A, phase A1-A3**: wire the existing E0/E1 recognition into
   production `rad(...)` with the fallback intact — activates already-written
   recognition code with no new representation.
2. **Track B, phase B0**: `sig_order` + `is_depending_on` +
   `mul/div/combine` FIR helpers — pure library groundwork with no pipeline
   change.

The two are independent and can proceed in parallel.
