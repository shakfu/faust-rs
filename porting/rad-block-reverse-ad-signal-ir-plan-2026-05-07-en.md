# RAD Block Reverse AD Signal-IR Plan

Date: 2026-05-07

Status: design plan

## Decision

The first robust RAD model should not depend on complete LTI detection.

Instead, `faust-rs` should introduce a signal-level block reverse-mode AD
operator that can differentiate the same primitive surface already supported by
FAD, including recursive/time-dependent graphs, by replaying one compute block
backwards. LTI recognition and transposition remain valuable, but they become a
phase-2 optimization rather than the foundation of correctness.

The target layering is:

```text
rad(expr, seeds)
  -> BlockReverseAD(expr, seeds)          // general, correct fallback
  -> optional LTI fast paths later        // SigIIR/SigFIR/StateSpace optimizations
  -> backend/interpreter implementation
```

## Motivation

The LTI path currently requires several coupled pieces:

- detect raw recursive/delay syntax as FIR/IIR;
- preserve affine seed provenance through canonicalization;
- reject or factorize high-order IIRs for numerical stability;
- map the result to state-space;
- lower the transposed state into `ReverseTimeRec`.

This is useful for optimization, but it is a poor first correctness foundation.
Many useful DSP graphs are recursive but not strictly LTI: nonlinear filters,
waveguides, physical models, time-varying filters, state variable filters,
saturating feedback loops, and other audio structures. A tape/replay model can
differentiate these without recognizing a closed-form transfer function.

## Signal-Level Requirement

The fallback must be expressed in Signal IR, not only in Faust FIR or backend
imperative representation.

That means adding a semantic signal node, for example:

```text
BlockReverseAD(body, seeds, outputs, cotangents, policy)
```

or a TreeArena-compatible equivalent such as:

```text
SIG_BLOCK_REVERSE_AD(
  body_group,
  output_list,
  seed_list,
  cotangent_list,
  policy
)
```

The exact shape can change during implementation, but the semantic contract
must be owned by the signal layer:

```text
over the current block:
  forward-evaluate the primal body sample-by-sample;
  record or checkpoint active values required by reverse rules;
  run a reverse sweep from frame BS-1 down to 0;
  emit primal outputs plus per-sample gradient contributions for each seed.
```

The backend decides how to allocate tape, checkpoint, or recompute. The Signal
IR decides what the node means.

## Initial Scope: Same Differentiable Surface As FAD

The first `BlockReverseAD` implementation should target the operations already
covered by the FAD transform, because those local derivative rules are already
specified and tested.

Accepted in phase B0:

- numeric constants and audio inputs;
- UI controls as differentiable seeds when explicitly listed;
- arithmetic `+`, `-`, `*`, `/`, remainder-like forms already handled by FAD
  where the derivative convention is defined;
- supported smooth primitives: `sin`, `cos`, `tan`, `exp`, `log`, `sqrt`,
  `abs`, inverse trig functions, casts following FAD conventions;
- `select2` with the same branch-local derivative policy as current FAD/RAD;
- read-only table reads if the FAD/RAD read-only approximation is already
  accepted;
- de Bruijn recursion and delay forms, because the whole purpose of
  `BlockReverseAD` is reverse-through-time over these structures.

Rejected or deferred in phase B0:

- writable table adjoints;
- soundfile content adjoints;
- side-effectful or opaque foreign functions without derivative rules;
- dynamic memory-dependent features whose active values cannot be recorded
  deterministically over the block;
- gradients that require state continuity across blocks.

## Output Convention

Preserve the current `rad(expr, seeds)` output layout:

```text
[primals..., gradient_contribution(seed_0), gradient_contribution(seed_1), ...]
```

The gradient outputs are per-sample contributions. Users can aggregate over the
current block in DSP code, using the block size convention already discussed
around `ma.BS`.

For multi-output `expr`, the first implementation keeps the current implicit
cotangent convention:

```text
J = sum(expr_outputs)
```

A later VJP API may expose explicit output cotangents.

## Block Semantics

Phase B0 uses a block-local convention:

- the forward sweep runs from frame `0` to `BS-1`;
- the reverse sweep runs from frame `BS-1` to `0`;
- adjoint terminal state at the end of the block is zero;
- no adjoint state is carried across blocks;
- primal DSP state follows the normal Faust execution semantics;
- gradient outputs are contributions for the current sample/frame, not an
  automatically block-summed scalar.

This convention is intentionally limited. It is sufficient for block-local
optimization and for DSP code that explicitly aggregates contributions. Later
phases can add inter-block adjoint state or checkpointed long-horizon training
when the semantics are specified.

## Tape And Checkpointing Policy

The Signal IR should not expose low-level tape buffers as ordinary user-visible
signals. It should expose a policy field or variant:

```text
TapeFull
Checkpointed
Recompute
```

Phase B0 should implement `TapeFull` first:

- record every active value needed by reverse rules for the current block;
- allocate storage proportional to `block_size * active_value_count`;
- prefer correctness and simple diagnostics over memory optimality.

Later phases can add checkpointing. This follows the standard reverse-mode AD
tradeoff: naive reverse mode stores values proportional to runtime, while
checkpointing reduces memory by recomputation. The `Revolve` family of
checkpointing schedules is a relevant long-term reference, but phase B0 should
not depend on implementing it.

## Why Not Implement This Only In FIR/Backend IR?

Implementing reverse-through-time only after FIR lowering would make RAD depend
on one backend-oriented imperative representation. That would have several
costs:

- RAD semantics would be hidden below the signal layer;
- non-FIR backends or analysis passes could not reason about RAD nodes;
- diagnostics would point at low-level generated loops rather than Faust signal
  structure;
- it would be harder to share the FAD rule surface and de Bruijn recursion
  invariants.

The backend still has to execute `BlockReverseAD`, but the compiler should
carry it as an explicit Signal IR node until lowering.

## Relationship To Existing `ReverseTimeRec`

`ReverseTimeRec` remains useful, but it should become a fast path:

```text
BlockReverseAD general fallback:
  works for FAD-surface recursive/time-dependent graphs

ReverseTimeRec LTI optimization:
  works when the graph is already proven strict LTI and can be transposed
  without a tape
```

This reverses the priority from the previous LTI-centered work:

- correctness first: `BlockReverseAD`;
- optimization second: LTI detection, `SigIIR`, `StateSpace`, `ReverseTimeRec`;
- codegen performance later: FIR/IIR specialized loops and checkpointing.

The existing `SigIIR -> StateSpace -> ReverseTimeRec` work should not be
discarded. It becomes the first optimization candidate once the general
block-reverse semantics are in place.

## Implementation Phases

### Phase B0: Signal Node And Semantics

Add a `SigBuilder`/`SigMatch` carrier for block reverse AD.

Pass criteria:

- Rustdoc documents block-local semantics, output layout, and tape policy;
- `signal_prepare` preserves the new node and validates its children;
- unsupported backends emit a clear diagnostic rather than silently dropping
  gradients.

### Phase B1: Lower `rad(...)` To `BlockReverseAD`

Change `propagate::reverse_ad` so recursive/time-dependent cases can lower to
the new block node instead of immediately requiring LTI transposition.

Pass criteria:

- non-recursive RAD behavior remains unchanged;
- recursive graphs from the FAD-supported operation surface produce a
  `BlockReverseAD` node;
- existing LTI fast-path tests either keep passing or are explicitly gated as
  optimizations.

### Phase B2: Interpreter Backend Execution

Implement `BlockReverseAD` in the interpreter or a small reference executor
first.

Pass criteria:

- block-local forward tape and reverse sweep are implemented for the initial
  FAD operation surface;
- finite-difference tests pass for representative recursive and non-recursive
  DSP graphs;
- diagnostics identify the first unsupported primitive inside the block.

### Phase B3: FIR/C Backend Lowering

Lower `BlockReverseAD` to explicit backend code.

Pass criteria:

- generated C/C++ or FIR execution matches the interpreter reference on a test
  corpus;
- tape allocation is deterministic from block size and active value count;
- gradients are stable across `opt_level=0` and optimized lowering.

### Phase B4: LTI Fast-Path Reintroduction

Reintroduce strict-LTI detection as an optional optimization:

```text
if strict LTI and supported section:
  use SigIIR/StateSpace/ReverseTimeRec
else:
  use BlockReverseAD
```

Pass criteria:

- optimized LTI and generic `BlockReverseAD` agree numerically on first-order
  and second-order filters;
- high-order direct IIRs remain rejected by the LTI path unless factorized, but
  may still run through generic `BlockReverseAD`;
- affine seed provenance is tested only where the LTI fast path rewrites
  coefficients; the generic block tape can rely on ordinary local chain rules.

## Risks

- Tape size can become large for audio blocks with many active intermediates.
  This is acceptable for phase B0/B2 correctness, but must be measured before
  production defaults.
- Block-local gradients are not the same as infinite-horizon recursive
  adjoints. The plan must keep the terminal-zero convention visible in docs and
  tests.
- Backend support is mandatory. A signal-level node without executable
  lowering is only a structural placeholder.
- The FAD-supported primitive surface is a good starting point, but RAD may
  need extra stored primal values for some rules. The rule table must document
  what each reverse rule records.
- Existing LTI work must be kept as optimization infrastructure, not allowed to
  block the general model.

## Recommended Next Patch

1. Add a `BlockReverseAD`/`SIG_BLOCK_REVERSE_AD` signal carrier with Rustdoc.
2. Preserve it through normalization and `signal_prepare`.
3. Add one structural propagation test proving a recursive `rad(...)` can
   produce the block node instead of a `delay-or-prefix` or
   `recursive-linear-transpose` diagnostic.
4. Keep execution unsupported initially if necessary, but make the diagnostic
   explicit and attach it to the backend stage, not to `propagate`.
