# LTI Filter Intermediate Form Plan

Date: 2026-05-06

## Scope

This note records the C++ compiler evidence for using a structured LTI filter
form in `faust-rs`, then turns it into an implementation plan. The immediate
driver is RAD phase E1 over recursive LTI DSPs, but the same representation is
also useful for algebraic filter transformations guided by the Z transform and
for lower-CPU code generation.

This is a planning document, not an implementation patch.

## C++ Reference Points

The relevant C++ files are:

- `compiler/transform/revealSum.cpp`
- `compiler/transform/revealFIR.cpp`
- `compiler/transform/revealIIR.cpp`
- `compiler/transform/factorizeFIRIIRs.cpp`
- `compiler/signals/sigFIR.cpp`
- `compiler/signals/sigIIR.cpp`
- `compiler/generator/compile_scal_fir.cpp`
- `compiler/generator/compile_scal_iir.cpp`
- `compiler/generator/instructions_compiler.cpp`
- `compiler/transform/signalFIRCompiler.cpp`
- `compiler/transform/signalFIRCompiler.hh`

The active scalar and FIR-instruction C++ pipelines apply:

```text
newConstantPropagation
  -> revealSum
  -> revealFIR
  -> revealIIR
  -> optional factorizeFIRIIRs
  -> annotation / scheduling / FIR code generation
```

This is the key architectural point: C++ does not rely only on late syntactic
matching of raw `delay/add/mul` shapes. It first reconstructs algebraic
`sigFIR` and `sigIIR` nodes, then lets later passes use those nodes for
analysis and code generation.

## What `sigFIR` Represents

C++ `sigFIR([S, C0, C1, ...])` means:

```text
C0 * S[n] + C1 * S[n-1] + C2 * S[n-2] + ...
```

The C++ helper layer is not just a tag:

- `delaySigFIR` turns constant delays into coefficient shifts.
- `addSigFIR` combines compatible FIRs on the same base signal.
- `negSigFIR`, `subSigFIR`, `mulSigFIR`, and `divSigFIR` keep FIR form when
  doing so does not increase sample-rate cost unexpectedly.
- `combineFIRs` groups terms by base signal and coefficient vector.
- `convertFIR2Sig` can expand the compact form back to ordinary delayed signal
  expressions for consumers that need the raw representation.

This is already a compact Z-domain numerator:

```text
B(z) = C0 + C1 z^-1 + C2 z^-2 + ...
```

For `faust-rs`, a direct `SigKind::Fir` or an internal equivalent would be a
good source canonical form for FIR detection, algebraic rewriting, and efficient
lowering.

## What `sigIIR` Represents

C++ `sigIIR([V, X, C1, C2, ...])` means:

```text
V[n] = X[n] + C1 * V[n-1] + C2 * V[n-2] + ...
```

The associated helpers form a partial linear algebra around one recursive
projection:

- `proj2SigIIR` turns the target recursive projection into an IIR identity.
- `delaySigIIR` shifts the recursive coefficients when a constant delay is
  applied.
- `addSigIIR` and `subSigIIR` add/subtract the input term and recursive
  coefficients.
- `mulSigIIR` and `divSigIIR` scale the input term and coefficients when the
  other operand is independent of the concerned recursive variable.
- `embeddedIIR` rewrites a FIR applied to an IIR into an IIR applied to a FIR.

This is already a compact Z-domain denominator:

```text
V(z) = X(z) / (1 - C1 z^-1 - C2 z^-2 - ...)
```

depending on the sign convention of the original recursive body. Faust library
filters often spell feedback as subtraction, so `revealIIR` and the helper
layer must preserve the actual coefficient signs after simplification.

## C++ Code Generation Use

The structured forms are used directly for lower-CPU code generation.

For FIR, `ScalarCompiler::generateFIR` and
`InstructionsCompiler::generateFIR`:

- special-case a single coefficient as a gain;
- avoid loops for small filters or sparse low-density filters;
- build a coefficient table when a filter is large/dense enough;
- choose the coefficient table storage from coefficient variability:
  compile-time static, init-time, block-time, or sample-time;
- emit an explicit accumulation loop:

```text
acc = 0
for ii in first_non_zero_tap .. tap_count {
    acc += coef[ii] * delayed_input[ii]
}
```

For IIR, `generateIIR`:

- constructs the feedback expression from the compact coefficient vector;
- reads delayed recursive values through the normal delay-line machinery;
- chooses delay implementation using `analyzeDelayType`;
- forces larger IIR feedback histories toward ring-buffer delay
  implementation when the configured threshold is crossed.

The generic delay-line layer then picks specialized implementations:

- zero delay;
- mono/single delay;
- copy delay;
- dense delay;
- masked/select ring delay.

The newer `SignalFIRCompiler` path shows the same target in FIR IR terms:
signals are compiled into `init`, `clear`, control, and per-sample FIR blocks,
with delay lines and tables materialized as explicit FIR load/store operations.

## Why This Matters For RAD

Current RAD E1 already supports hand-written strict-LTI recursive state-space
fixtures. The next failure is not a mathematical limitation: standard library
forms such as `fi.iir((1), (p, q))` are LTI, but they reach RAD as raw delayed
recursive expressions such as:

```text
y[n] = x[n] - p * y[n-1] - q * y[n-2]
```

represented structurally as nested `delay1`/`delay` terms. The E0 classifier can
recognize this as LTI, but the current E1 extractor only accepts a narrower
state-space syntax.

Using a `sigIIR`-like canonical form before RAD gives E1 the same information
that C++ already reconstructs for code generation:

```text
sigIIR([y, x, -p, -q])
```

From there, RAD can convert the scalar higher-order IIR into explicit companion
state:

```text
s0[n] = x[n] - p * s0[n-1] - q * s1[n-1]
s1[n] = s0[n-1]
y[n]  = s0[n]
```

and reuse the existing LTI transpose machinery.

## Why `sigFIR` / `sigIIR` Are Not Quite The Whole RAD IR

`sigFIR` and `sigIIR` should be the canonical detection and algebraic source
form. They are compact and close to C++ parity.

RAD still benefits from a private lowered form, because reverse transposition
does not execute a transfer function directly. It executes state equations:

```text
state[n] = A * state[n-1] + B * input[n]
output[n] = C * state[n] + D * input[n]
```

The proposed layering is therefore:

```text
raw signal graph
  -> revealSum/revealFIR/revealIIR-style canonicalization
  -> sigFIR/sigIIR-like compact filter form
  -> private LinearRecurrence / StateSpace view when needed
  -> RAD transpose or FIR/backend lowering
```

The private view can be a Rust struct, not a public signal node. That keeps
external signal parity close to C++ while giving RAD and optimizers an explicit
matrix/companion-state representation.

## Broader Algebraic Uses

Once FIR/IIR filters are represented as coefficient vectors, `faust-rs` can
support transformations that are hard to do reliably on raw syntax:

- combine sums of delays over the same base signal;
- remove zero taps before unsupported temporal nodes escape into AD;
- multiply/divide by constant or block-rate gains while preserving FIR/IIR
  structure;
- factor common coefficients when it reduces runtime cost;
- commute FIR over IIR where valid, following the C++ `embeddedIIR` precedent;
- estimate gain or stability from numerator/denominator coefficients;
- canonicalize equivalent direct-form library spellings to one denominator
  representation;
- choose direct, transposed, companion, or state-space execution form from
  filter order and backend constraints.

These are Z-transform-guided transformations: operate on numerator and
denominator polynomials first, then materialize a concrete runtime form only
after the algebraic decision is made.

## Proposed Rust Representation

Do not start by exposing new public signal nodes. Start with an internal module
that can later be promoted if it proves stable.

Suggested internal types:

```rust
struct FirFilter {
    base: SignalId,
    coeffs: Vec<SignalId>, // c0, c1, ...
}

struct IirFilter {
    state: SignalId,
    input: SignalId,
    feedback: Vec<SignalId>, // c1, c2, ...
}

struct LinearRecurrence {
    states: Vec<StateSlot>,
    equations: Vec<LinearExpr>,
    outputs: Vec<LinearExpr>,
}
```

The first two mirror C++ `sigFIR`/`sigIIR`; the last is the RAD/codegen view.
The conversion `IirFilter -> LinearRecurrence` is deterministic:

- `feedback.len()` gives the order;
- slot 0 is the current recursive output;
- slots 1..N-1 are delay-chain slots;
- the first equation contains the input and feedback coefficients;
- remaining equations shift previous slots forward.

## Implementation Plan

### Phase L1: C++-Parity Reconstruction

Add a Rust FIR/IIR reconstruction module that mirrors the C++ ordering:

```text
reveal_sum
  -> reveal_fir
  -> reveal_iir
```

Pass criteria:

- reconstruct simple constant-delay FIRs;
- combine FIR terms with the same base signal;
- reconstruct single-projection IIRs from recursive sums containing one FIR over
  the concerned projection;
- preserve coefficient sign exactly;
- keep unsupported cases as raw signals rather than guessing.

Initial users should be tests and RAD, not the whole production pipeline.

### Phase L2: RAD E1 Input Canonicalization

Before the E1 extractor gives up on an LTI recursive group, try to convert the
group through the FIR/IIR reconstruction path.

Pass criteria:

- `rad(_ : fi.iir((1), (p, q)), (p, q))` compiles;
- the resulting gradient contributions match a closed-form second-order
  adjoint recurrence;
- unsupported temporal forms continue to produce explicit diagnostics;
- zero-multiplied delay branches are eliminated before they can trigger
  `delay-or-prefix`.

### Phase L3: Private State-Space View

Add `IirFilter -> LinearRecurrence` conversion and reuse the existing E1
transpose path on the generated state-space view.

Pass criteria:

- first-order and second-order IIRs share the same transposition code path as
  hand-written state-space fixtures;
- multi-output state-space fixtures remain unchanged;
- tests cover coefficient derivatives and input derivatives separately.

### Phase L4: Z-Domain Algebraic Rewrites

Move safe transformations from ad hoc simplification into the structured filter
module:

- FIR combination;
- zero-tap pruning;
- coefficient factoring;
- FIR-over-IIR embedding where valid;
- equivalent direct-form canonicalization for standard library filters.

Pass criteria:

- every rewrite has a structural test and a runtime parity test;
- rewrites are opt-in or guarded until parity confidence is high;
- diagnostics explain when a filter is rejected because of coefficient
  variability, nonlinear feedback, or unsupported seed identity.

### Phase L5: Codegen Use

Teach the Rust FIR/backend path to lower structured filters directly when that
beats the expanded delay expression.

Pass criteria:

- large/dense FIRs lower to coefficient-table accumulation loops;
- small/sparse FIRs remain expanded expressions;
- IIR delay strategy can select simple delay, copy delay, dense delay, or ring
  delay based on max delay and access pattern;
- C, C++, Cranelift, and interpreter backends agree on runtime output;
- benchmark coverage tracks FIR and IIR CPU changes.

## Seed-Identity Caveat

Structured filter canonicalization does not by itself solve seed identity.
Library forms such as `tf21` and `tf22t` can rewrite a coefficient `a1` as
`-a1`. If the user seeds a literal value, RAD currently tracks signal identity,
not source-level algebraic provenance. Therefore the transformed coefficient may
no longer be recognized as the requested seed.

This needs a separate decision:

- keep identity-only seeds and document that users must seed the exact
  coefficient expression consumed by the filter;
- preserve simple affine provenance such as `-seed`;
- or add a parameter-binding layer that tracks source-level coefficient names
  through algebraic rewrites.

This decision should be made before promising full standard-library biquad
training support.

## Risks

- C++ `sigFIR`/`sigIIR` are permissive helper nodes; copying all behavior at
  once would add too much surface area. Start with the subset needed by RAD E1.
- Sign conventions must be tested carefully. Faust library definitions often
  express feedback with subtraction while IIR coefficient vectors store the
  post-simplification signed terms.
- Coefficient variability matters. Constant/LTI, block-rate/LTV, and sample-rate
  cases must remain distinct because RAD E1, E2, and runtime codegen have
  different tape/replay requirements.
- Multi-lane recursive groups are already supported in hand-written state-space
  form. The first reconstruction phase should handle single-lane higher-order
  IIR before general multi-lane transfer matrices.

## Recommended Next Patch

Implement the smallest reusable filter-reconstruction crate/module needed by
RAD:

1. Add internal `FirFilter` / `IirFilter` extraction helpers with Rustdoc
   provenance pointing to C++ `revealSum`, `revealFIR`, `revealIIR`,
   `sigFIR`, and `sigIIR`.
2. Add unit tests for:
   - `x@2 -> FIR[x, 0, 0, 1]`;
   - `x + c*x@1 -> FIR[x, 1, c]`;
   - `y = x - p*y@1 - q*y@2 -> IIR[y, x, -p, -q]`.
3. Convert the extracted second-order IIR to companion state-space.
4. Feed that state-space view into the existing E1 transpose tests before
   exposing new public `rad(...)` support.

