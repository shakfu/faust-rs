# RAD E1 Library Filter Shape Analysis

Date: 2026-05-06

## Scope

This note maps the next RAD E1 gap for Faust library IIR/biquad filters after
the accepted hand-written strict-LTI state-space fixtures landed in
`tests/corpus/rad_lti_recursive_*.dsp`.

The target question was whether standard library forms such as `fi.iir`,
`fi.tf2`, or the direct-form variants already fit the current public E1
transpose, and if not, why.

## Reproduction Inputs

The minimal library forms tested locally were:

```faust
import("stdfaust.lib");
p = 0.5;
q = 0.25;
process = rad(_ : fi.iir((1), (p, q)), (p, q));
```

```faust
import("stdfaust.lib");
p = 0.5;
q = 0.25;
process = rad(_ : fi.tf2(1, 0, 0, p, q), (p, q));
```

and the direct-form variants:

```faust
process = rad(_ : fi.tf21(1, 0, 0, p, q), (p, q));
process = rad(_ : fi.tf22(1, 0, 0, p, q), (p, q));
process = rad(_ : fi.tf22t(1, 0, 0, p, q), (p, q));
process = rad(_ : fi.tf21t(1, 0, 0, p, q), (p, q));
```

## Library Definitions

Current `filters.lib` defines:

```faust
iir(bv,av) = ma.sub ~ fir(av) : fir(bv);
tf2(b0,b1,b2,a1,a2) = iir((b0,b1,b2),(a1,a2));
```

and the direct forms include:

```faust
tf21(b0,b1,b2,a1,a2) =
    _ <:(mem<:((mem:*(b2)),*(b1))),*(b0) :>_
    : ((_,_,_:>_) ~(_<:*(-a1),(mem:*(-a2))));

tf22(b0,b1,b2,a1,a2) =
    _ : (((_,_,_:>_)~*(-a1)<:mem,*(b0))~*(-a2))
      : (_<:mem,*(b1)),_ : *(b2),_,_ :> _;

tf22t(b0,b1,b2,a1,a2) =
    _ : (_,_,(_ <: *(b2)',*(b1)',*(b0))
      : _,+',_,_ :> _)~*(-a1)~*(-a2) : _;

tf21t(b0,b1,b2,a1,a2) =
    tf22t(1,0,0,a1,a2) : tf22t(b0,b1,b2,0,0);
```

## Findings

### `fi.iir((1), (p, q))`

The non-RAD signal dump is:

```text
SIGPROJ(0, DEBRUIJNREC([
  input(0)
    - (0.5 * delay1(state)
       + 0.25 * delay(delay1(state), 1))
]))
```

The FIR fast lane lowers this to a normal second-order state buffer:

```text
fRec[0] = input - (0.5 * fRec[1] + 0.25 * fRec[2])
output = fRec[0]
fRec[2] = fRec[1]
fRec[1] = fRec[0]
```

RAD currently fails with:

```text
RadUnsupportedNode { kind: "recursive-linear-transpose" }
```

The reason is not that the filter is nonlinear or LTV. It is LTI, but the
recursive body contains `Delay(delay1(state), 1)`. The E0 classifier treats
that as an LTI temporal shift, while the current E1 transposition scaffold only
extracts direct `Proj(slot, DEBRUIJNREF(1))` and the canonical propagated
`delay1(Proj(slot, DEBRUIJNREF(1)))`. It still rejects general `Delay`/`Prefix`
over recursive state as needing an explicit block placement convention.

### `fi.tf2(1, 0, 0, p, q)`

The non-RAD FIR dump is effectively the same executable recurrence as
`fi.iir((1), (p, q))`, because the zero feed-forward taps are optimized away by
the later FIR path.

RAD fails earlier with:

```text
RadUnsupportedNode { kind: "delay-or-prefix" }
```

The reason is ordering: `rad(...)` sees the unsimplified `fir((1,0,0))` output
shape before the later FIR simplification removes `0 * delay(...)` terms. The
current reverse pass rejects the delay family before zero-multiplied temporal
branches are eliminated.

### `fi.tf21`, `fi.tf22t`, `fi.tf21t`

For the degenerate numerator `(1,0,0)`, `tf21`, `tf22t`, and `tf21t` can compile
through `rad(...)`, but their gradient lanes for `(p, q)` are currently zero.
This should not be treated as useful coefficient training coverage.

The immediate cause is seed identity. RAD seeds are signal identities, not
symbolic source variables. In these direct forms the library rewrites feedback
coefficients as `-a1` and `-a2`. With literal seeds:

```faust
p = 0.5;
q = 0.25;
```

the body contains literal `-0.5` and `-0.25`, which are not the same signal IDs
as seed literals `0.5` and `0.25`. Therefore the recursive group is classified
as seed-independent and the public E1 bridge emits zero requested gradients.

This is a representation limitation of the current seed model for library
rewrites. It is separate from the recursive transposition gap.

### `fi.tf22`

`tf22` still fails with:

```text
RadUnsupportedNode { kind: "delay-or-prefix" }
```

The direct-form-2 spelling exposes feed-forward memory/delay nodes to RAD before
they can be reduced into a pure accepted state-space form.

## Root Causes

There are three distinct blockers, not one:

1. **Higher-order recursive state encoded as `Delay(delay1(state), k)`.**
   `fi.iir` lowers naturally to a state buffer, but the RAD E1 scaffold has not
   learned to extract this representation as additional state slots.

2. **Zero-tap temporal branches are simplified too late for RAD.**
   `fi.tf2(1,0,0,p,q)` becomes the same FIR as `fi.iir((1),(p,q))`, but RAD
   rejects the unsimplified `0 * delay(...)` terms first.

3. **Library coefficient rewrites break literal seed identity.**
   Direct forms that use `-a1`/`-a2` can hide the user seed from RAD when the
   seed is a literal. UI seeds would keep identity better, but UI-dependent
   recursive coefficients are currently classified as E2/LTV rather than E1.

## Recommended Next Implementation Step

The most robust next step is a representation rewrite before RAD E1 extraction:

1. Detect single-lane LTI recursive bodies containing finite shifts of the same
   recursive state, such as:

   ```text
   y[n] = x[n] + a1 * y[n-1] + a2 * y[n-2]
   ```

2. Canonicalize them into an explicit multi-slot companion state-space group:

   ```text
   s0[n] = x[n] + a1 * s0[n-1] + a2 * s1[n-1]
   s1[n] = s0[n-1]
   y[n]  = s0[n]
   ```

3. Feed that canonical group to the existing E1 transpose path.

This directly addresses `fi.iir((1),(p,q))` and the denominator part of
`fi.tf2`. It also aligns the library form with the already accepted
hand-written state-space corpus fixtures.

## Secondary Follow-Ups

- Add an early algebraic cleanup in the RAD active-subgraph pass for
  zero-multiplied branches so `tf2(1,0,0,...)` does not fail on dead delays.
- Decide a seed policy for transformed coefficients:
  - keep the current identity-only rule and document that users should seed the
    exact coefficient expression used by the library, or
  - preserve source-level parameter identity through simple affine rewrites
    such as `-a1`, or
  - accept block-frozen UI coefficients into E1, which is a compatibility
    decision because UI signals are currently E2/LTV.

## Pass Criteria For The Next Patch

- `rad(_ : fi.iir((1), (p, q)), (p, q))` compiles and matches a closed-form
  one-input second-order recurrence gradient over an interpreter block.
- `rad(_ : fi.tf2(1, 0, 0, p, q), (p, q))` either compiles through the same
  canonicalization or fails only for a documented seed-identity reason, not for
  dead feed-forward delays.
- Add accepted corpus fixtures and golden snapshots for the first library IIR
  form that becomes supported.
