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

Numerical stability caveat: this direct companion-state conversion is only a
safe first implementation target for first-order and second-order sections.
Direct companion or direct-form state realization of higher-order IIRs is
sensitive to coefficient quantization and roundoff. Therefore the initial RAD
bridge must reject direct `IirFilter -> StateSpace` conversion when the scalar
denominator order is greater than two, unless a preceding pass has factorized
the filter into stable first-order/second-order sections. Higher-order filters
should eventually be lowered as cascades or parallel compositions of biquads
and first-order sections before RAD transposition.

## Why `sigFIR` / `sigIIR` Are Not Quite The Whole RAD IR

`sigFIR` and `sigIIR` should be the canonical detection and algebraic source
form. They are compact and close to C++ parity.

RAD still benefits from a private lowered form, because reverse transposition
does not execute a transfer function directly. It executes state equations:

```text
x[n] = A * x[n-1] + B * u[n]
y[n] = C * x[n] + D * u[n]
```

The proposed layering is therefore:

```text
raw signal graph
  -> revealSum/revealFIR/revealIIR-style canonicalization
  -> sigFIR/sigIIR-like compact filter form
  -> private StateSpace view when needed
  -> RAD transpose or FIR/backend lowering
```

The private view can be a Rust struct, not a public signal node. That keeps
external signal parity close to C++ while giving RAD and optimizers an explicit
matrix/state-space representation.

Even while the first reconstruction target is SISO, the internal representation
must use canonical state-space terminology from day one:

- `A`: state transition;
- `B`: input-to-state map;
- `C`: state-to-output map;
- `D`: direct input-to-output map.

For phase L3 these matrices may be stored sparsely as row vectors of linear
terms, but the fields and documentation should use the `A/B/C/D` names rather
than ad hoc `equations`/`outputs` terminology. This makes the SISO
implementation a restricted instance of the later MIMO design instead of a
shape that must be renamed when cross-coupled filter networks, FDNs, or
multi-output recurrences are added.

## Pipeline Placement Constraint: RAD Runs In `propagate`

`faust-rs` currently expands both `fad(...)` and `rad(...)` inside
`crates/propagate`, before the later normalization, `transform::signal_prepare`,
FIR lowering, and backend stages. The relevant `propagate` flow is:

```text
FlatNodeKind::ReverseAD
  -> propagate seed box to seed signals
  -> propagate body box to body signals
  -> reverse_ad::generate_rad_signals(body_sigs, seed_sigs)
```

Therefore any LTI reconstruction needed by RAD must happen before or during
`reverse_ad::generate_rad_signals`. A `revealIIR` pass that only runs later in
`transform` or `signal_fir` is too late for RAD: by then `rad(...)` has already
either produced gradients or rejected the graph.

This has several architectural consequences:

- `transform::signal_prepare` must not become the only owner of
  `revealFIR/revealIIR` if RAD needs those carriers. That pass is downstream of
  `propagate`, and it also converts recursion into prepared symbolic forms for
  FIR lowering.
- `propagate` should not depend on `transform` just to reuse a late FIR
  preparation pass. That would invert the intended workspace layering and risks
  circular dependencies.
- The reusable C++-parity algebra belongs in `signals` or another upstream
  crate that both `propagate` and `transform` can call. `propagate` can then
  run a narrow RAD-LTI preparation step without importing backend/FIR staging.
- RAD must see the same De Bruijn recursion form it already analyzes today:
  `DEBRUIJNREC` / `DEBRUIJNREF` are still the active representation during
  propagation. Later conversion to prepared symbolic recursion cannot be the
  first place where LTI structure is discovered.

The intended RAD-local staging is:

```text
propagated body/seed signals
  -> RAD-LTI preparation
       revealSum/revealFIR/revealIIR-style reconstruction
       simple affine seed provenance
       strict LTI/LTV/nonlinear classification
  -> reverse_ad::generate_rad_signals
       SigIIR/SigFIR consumers
       StateSpace bridge
       ReverseTimeRec output
```

The backend/codegen pipeline may run its own broader filter preparation later,
but that preparation is a code-generation optimization. RAD correctness needs
an earlier, explicitly scoped preparation boundary.

## Consequences For FAD

FAD shares the same `propagate` placement, but its needs are different. FAD
mostly applies local chain-rule rewrites while walking the primal graph; it
does not need to discover a transposed recursive execution strategy before it
can make progress. RAD does: reverse transposition of recursive LTI state is
anti-causal in stream time and must be represented as a block-local
`ReverseTimeRec`.

This means the LTI preparation is urgent for RAD even if FAD can continue to
operate on raw delayed recursive syntax. A future shared preparation pass must
not change FAD seed identity or recursion scoping unless equivalent
non-regression tests exist.

## Consequences For Seed Identity

Because RAD receives explicit `seed_sigs` from `propagate`, seed recognition is
currently identity-based on `SigId`. LTI canonicalization can rewrite the same
source-level parameter into a derived coefficient:

```text
seed
-seed
const - seed
seed / const
```

If RAD-LTI preparation replaces a raw expression with `SigIIR([..., -seed])`
without preserving provenance, the requested seed may no longer match the
coefficient node visited by `reverse_ad`. Therefore the placement constraint
and the affine-provenance requirement are coupled: the same preparation step
that reveals `SigIIR` must also record enough `derived = a*seed + b`
provenance to remap coefficient gradients back to the user-supplied seeds.

Without this, `revealIIR` can make the recursion structurally recognizable
while accidentally losing the gradient target the user requested.

## Runtime Consequence: `ReverseTimeRec` Still Needs Backend Semantics

Running LTI preparation inside `propagate` only makes RAD produce the correct
structural graph. It does not by itself complete execution semantics.
`ReverseTimeRec(DEBRUIJNREC(...))` still requires backend/interpreter support
for:

- reverse evaluation over the current compute block;
- terminal adjoint state initialized to zero;
- no hidden state carry across blocks unless a later phase explicitly changes
  the convention;
- block-size agreement with the DSP side, including user-visible helpers such
  as `ma.BS` when sample-wise gradient contributions are aggregated by the
  program.

Therefore the RAD-LTI detection work and the backend `ReverseTimeRec` lowering
work are separate gates. A program can pass RAD-LTI structural preparation and
still require backend work before it is fully executable.

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
- choose direct, transposed, cascaded-biquad, or general state-space execution
  form from filter order, numerical stability, and backend constraints.

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

struct StateSpace {
    states: Vec<StateSlot>,
    inputs: Vec<InputSlot>,
    outputs: Vec<OutputSlot>,
    a_rows: Vec<Vec<LinearTerm>>, // A: previous state -> current state
    b_rows: Vec<Vec<LinearTerm>>, // B: input -> current state
    c_rows: Vec<Vec<LinearTerm>>, // C: current state -> output
    d_rows: Vec<Vec<LinearTerm>>, // D: input -> output
}
```

The first two mirror C++ `sigFIR`/`sigIIR`; the last is the RAD/codegen view.
The conversion `IirFilter -> StateSpace` is deterministic for first-order and
second-order sections:

- `feedback.len()` gives the order;
- slot 0 is the current recursive output;
- slots 1..N-1 are delay-chain slots;
- the first `A` row contains the feedback coefficients;
- the first `B` row contains the independent input gain;
- remaining `A` rows shift previous slots forward;
- `C` selects state slot 0 for the scalar output;
- `D` is zero for canonical recursive IIR sections unless a feedthrough term is
  explicitly represented.

For denominator order greater than two, the conversion must return a diagnostic
until a factorization pass can build a cascade or parallel composition of
first-order and second-order `StateSpace` sections. This is not just an
implementation shortcut: it is a numerical stability requirement.

## Porting Strategy

The goal is a complete Rust port of the C++ `sigFIR`/`sigIIR` algebra, not a
RAD-only subset. The narrowness must be in how the new module is connected to
production consumers, not in the module's ambition.

This distinction matters:

- the algebraic module should cover the full C++ helper surface with parity
  tests and documented invariants;
- RAD should initially consume only the LTI cases whose transpose semantics are
  already specified;
- FIR/codegen should initially consume only the shapes where emitted-code parity
  and CPU behavior have been measured;
- broader Z-transform-guided rewrites should be enabled only after structural
  and runtime parity tests are in place.

That gives `faust-rs` one reusable filter algebra foundation for RAD, symbolic
rewriting, and optimized code generation, while avoiding a single large switch
that changes every downstream pipeline at once.

## Implementation Plan

### Phase L1: Full C++-Parity Filter Algebra Port

Add a Rust FIR/IIR algebra module that ports the C++ helper surface around
`sigFIR` and `sigIIR`. It should include the reconstruction ordering:

```text
reveal_sum
  -> reveal_fir
  -> reveal_iir
```

and the full helper behavior needed to manipulate the resulting forms:

- `delaySigFIR`, `addSigFIR`, `subSigFIR`, `negSigFIR`, `mulSigFIR`,
  `divSigFIR`, `simplifyFIR`, `combineFIRs`, and `convertFIR2Sig`;
- `proj2SigIIR`, `delaySigIIR`, `addSigIIR`, `subSigIIR`, `mulSigIIR`,
  `divSigIIR`, and `embeddedIIR`;
- coefficient normalization, zero-tap removal, and degenerate fallbacks such as
  zero or plain gain;
- sign preservation for subtraction-based library feedback terms.

Pass criteria:

- every ported helper has focused Rust unit tests with C++ provenance in
  Rustdoc;
- structural tests cover FIR delay shifting, compatible FIR addition,
  incompatible FIR fallback, IIR delay shifting, IIR addition/subtraction,
  IIR scaling, and `embeddedIIR`;
- differential tests compare selected reconstructed signal dumps or generated
  FIR behavior against the C++ reference;
- unsupported cases stay explicit and auditable, rather than silently guessing;
- the module is available to RAD/codegen behind explicit integration points, not
  implicitly applied to the whole production pipeline on day one.

### Phase L2: RAD E1 Input Canonicalization

Before the E1 extractor gives up on an LTI recursive group, try to convert the
group through the FIR/IIR reconstruction path.

Because `rad(...)` is expanded in `propagate`, this canonicalization must be
available to `propagate::reverse_ad` before its active-subgraph sweep rejects
temporal or recursive shapes. It may call shared helpers from `signals`, but it
must not depend on downstream `transform::signal_prepare` or FIR lowering.

The first RAD-local preparation boundary should take propagated body/seed
signals and return either:

- a structurally equivalent graph containing `SigFIR`/`SigIIR` carriers with
  seed provenance metadata sufficient for L4b; or
- an explicit diagnostic explaining whether the failure is non-LTI,
  time-varying, seed-provenance loss, unsupported temporal placement, or an
  unimplemented reconstruction pattern.

Pass criteria:

- `rad(_ : fi.iir((1), (p, q)), (p, q))` compiles;
- the resulting gradient contributions match a closed-form second-order
  adjoint recurrence;
- unsupported temporal forms continue to produce explicit diagnostics;
- zero-multiplied delay branches are eliminated before they can trigger
  `delay-or-prefix`.
- a regression test proves that an LTI reconstruction pass placed only in
  `transform` is insufficient for RAD, and that the RAD-local preparation path
  runs before `reverse_ad` rejects the raw recursive/delay form.

### Phase L3: Private State-Space View

Add `IirFilter -> StateSpace` conversion and reuse the existing E1 transpose
path on the generated state-space view.

The view must expose canonical `A/B/C/D` slots even if the initial storage is
sparse and SISO-only. Direct conversion is limited to order 1 and order 2 IIR
sections. Higher-order IIRs must be rejected with a diagnostic that asks for
section factorization, rather than silently building a numerically fragile
companion matrix.

Pass criteria:

- first-order and second-order IIRs share the same transposition code path as
  hand-written state-space fixtures;
- third-order and higher direct IIR conversion is rejected explicitly until
  factorization into stable sections exists;
- `A/B/C/D` naming appears in the Rustdoc and tests for the state-space view;
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

### Phase L4b: Simple Affine Seed Provenance

Adopt the simple affine provenance option explicitly for the first seed-identity
extension. RAD should recognize coefficient expressions of the form:

```text
derived_seed = a * seed + b
```

where `a` and `b` are independent of the seed and are constant/LTI for the
current E1 block. The gradient remapping is:

```text
dJ/dseed += a * dJ/dderived_seed
```

The initial accepted surface is deliberately small:

- `seed`;
- `-seed`;
- `seed + const`;
- `const + seed`;
- `seed - const`;
- `const - seed`;
- `const * seed`;
- `seed * const`;
- `seed / const`.

The first implementation must reject, not approximate, non-affine provenance:

- `seed * seed`;
- `sin(seed)`, `exp(seed)`, or any nonlinear primitive over the seed;
- `1 / seed` or `const / seed`;
- branches/selects controlled by the seed;
- temporal forms such as `delay(seed)`;
- sample-rate or UI-varying affine coefficients in the E1/LTI path.

This choice covers the library feedback-coefficient rewrite `a1 -> -a1`
without requiring a general symbolic differentiation pass for parameter
expressions.

### Phase L4c: Nonlinear Parameter Provenance And Chain Rule

Document and prototype the next provenance tier after L4b. Real audio
parameters such as cutoff frequency, resonance, damping, or Q usually map to
filter coefficients through nonlinear expressions: divisions, square roots,
trigonometric functions, exponentials, bilinear-transform formulas, or
normalization by `a0`.

L4b deliberately rejects these cases, but the roadmap must not stop at raw
coefficient learning. L4c should represent coefficient provenance as an
expression graph with analytic derivatives back to the user-level parameter:

```text
coef = f(seed)
dJ/dseed += (df/dseed) * dJ/dcoef
```

The initial L4c target should remain block-level/LTI for the current RAD block:
the parameter expression may be nonlinear, but its value and derivative must be
constant over the block accepted by E1. Sample-rate-varying nonlinear
parameters belong to a later LTV/tape-based phase, not to strict LTI E1.

Candidate accepted cases:

- `const / seed` and `1 / seed`, with explicit nonzero-domain diagnostics;
- `sin(seed)`, `cos(seed)`, `tan(seed)` where the math primitive already has a
  known derivative in the compiler;
- `exp(seed)`, `log(seed)`, and `sqrt(seed)` with domain checks;
- coefficient normalization patterns such as `b0 / a0`, `a1 / a0`,
  `a2 / a0`;
- standard-library biquad coefficient formulas once their block-level
  parameter dependencies have been made explicit.

Pass criteria:

- L4b affine provenance remains the default low-risk path;
- each nonlinear primitive has an analytic derivative test;
- coefficient-gradient accumulation is checked against finite differences on
  representative biquad formulas;
- domain failures are diagnostics, not silent NaNs;
- sample-varying nonlinear provenance stays rejected by E1.

### Phase L5: Codegen Use

Teach the Rust FIR/backend path to lower structured filters directly when that
beats the expanded delay expression.

Pass criteria:

- large/dense FIRs lower to coefficient-table accumulation loops;
- small/sparse FIRs remain expanded expressions;
- first-order and biquad IIRs can lower through numerically stable direct or
  transposed section forms;
- higher-order IIRs lower only after factorization into stable first-order or
  second-order sections, not as a single direct companion form;
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

The chosen first policy is to preserve simple affine provenance only. That
means `-seed`, `2 * seed`, and `1 - seed` remain connected to the requested
seed, with the corresponding affine derivative applied to the gradient.
Nonlinear parameter-expression differentiation is deferred to phase L4c, not
left undefined.

This decision must be implemented before promising standard-library direct-form
biquad training support, because those forms commonly rewrite feedback
coefficients through negation.

## Risks

- C++ `sigFIR`/`sigIIR` are permissive helper nodes whose behavior affects
  algebra, simplification, and code-generation cost. They should be ported as a
  complete, well-tested module, but their use in RAD and backend pipelines must
  be enabled progressively behind explicit pass gates.
- Sign conventions must be tested carefully. Faust library definitions often
  express feedback with subtraction while IIR coefficient vectors store the
  post-simplification signed terms.
- Coefficient variability matters. Constant/LTI, block-rate/LTV, and sample-rate
  cases must remain distinct because RAD E1, E2, and runtime codegen have
  different tape/replay requirements.
- Multi-lane recursive groups are already supported in hand-written state-space
  form. Reconstruction should handle single-lane higher-order IIR through
  factorized first-order/biquad sections before general multi-lane transfer
  matrices.
- Direct companion realizations are numerically fragile for denominator order
  greater than two. Do not make them the default bridge from reconstructed IIR
  to RAD or backend codegen; require first-order/biquad factorization instead.
- If state-space data structures are named around the first SISO use case,
  later MIMO support will require an avoidable refactor. Use `A/B/C/D`
  terminology and row-based storage immediately, even if only one input and one
  output are populated at first.
- L4b affine provenance is useful but insufficient for user-level audio
  parameters such as cutoff and Q. Without L4c, RAD can learn raw coefficients
  but not reliably train the higher-level parameters that generated those
  coefficients.

## Recommended Next Patch

The first two implementation slices now exist:

- typed `FirFilter` / `IirFilter` extraction helpers and `IirFilter ->
  StateSpace` conversion;
- an internal `propagate::transpose_ad` bridge that lowers first-order and
  second-order `IirFilter` sections through `StateSpace -> DEBRUIJNREC ->
  ReverseTimeRec`;
- `reverse_ad` can consume a hand-built `SigIIR` carrier and route gradients to
  its independent input and feedback coefficients, while direct order-3 IIRs
  remain rejected.

The next patch should move from hand-built carriers to RAD-local
canonicalization of real propagated forms:

1. Add a `propagate`-visible RAD-LTI preparation function that runs before
   `reverse_ad::generate_rad_signals` rejects raw delay/recursion forms.
2. Reconstruct at least the strict second-order pattern
   `y = x - p*y@1 - q*y@2` into `SigIIR([y, x, -p, -q])` while preserving
   De Bruijn coherence.
3. Add L4b affine provenance tests for `-seed` and `const - seed`, because
   sign-preserving `revealIIR` rewrites commonly derive feedback coefficients
   from user-supplied seeds.
4. Add a higher-level RAD test where the input is a raw propagated recursive
   form or a small DSP equivalent, not a hand-built `SigIIR` carrier.
5. Keep public `rad(...)` support gated until the reconstructed path has
   diagnostics for non-LTI, LTV, unsupported temporal placement, and seed
   provenance failure.
