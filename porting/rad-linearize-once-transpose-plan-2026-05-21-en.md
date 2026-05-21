# RAD Linearize-Once Transposition Plan

Date: 2026-05-21

Status: design plan

Scope: evaluate and stage a migration of `rad(expr, seeds)` toward the
architecture described by Radul et al., "You Only Linearize Once: Tangents
Transpose to Gradients" (2022), without regressing the current feed-forward and
block reverse-mode AD behavior.

## 1. Executive Decision

The technique is implementable in `faust-rs`, but it should be introduced as a
new internal AD foundation, not as a direct rewrite of `reverse_ad.rs`.

The desired architecture is:

```text
primal Signal graph
  -> linearize once
       -> primal outputs
       -> explicitly linear residual from seed tangents to output tangents
       -> nonlinear values required by that residual
  -> transpose the linear residual
       -> seed cotangents
  -> lower to Signal IR or FIR depending on temporal needs
```

This should eventually replace most hand-written reverse propagation logic in:

- `crates/propagate/src/reverse_ad.rs`;
- `crates/transform/src/signal_fir/module.rs` `BlockReverseAD` propagation;
- parts of `crates/transform/src/signal_fir/block_reverse_ad.rs` that infer
  tape needs from reverse rules.

It must not initially replace:

- the user-visible `fad(expr, seeds)` implementation;
- `SigBlockReverseAD` as the semantic carrier for temporal RAD;
- the current `BlockReverseAD` FIR lowering until parity is proven.

## 2. Motivation

Current RAD has three partially overlapping representations of the same
mathematics:

1. FAD rules in `crates/propagate/src/forward_ad.rs`.
2. Symbolic feed-forward RAD rules in `crates/propagate/src/reverse_ad.rs`.
3. FIR `BlockReverseAD` reverse rules in
   `crates/transform/src/signal_fir/module.rs`.

`crates/signals/src/ad_rules.rs` already reduces drift for local RAD formulas,
but it still starts from reverse-mode formulas. The deeper factorization is to
make the tangent program the source of truth and derive reverse mode by
transposing that tangent program.

Expected benefits:

- one derivative rule surface for both FAD and RAD;
- fewer opportunities for FAD/RAD formula drift;
- a principled place to express forward-value dependencies as nonlinear residual
  dependencies;
- a clearer invariant for transposition: only explicitly linear residuals are
  accepted;
- a better path to unify feed-forward RAD, block reverse AD, and future LTI
  recursive optimizations.

Conceptual guardrail: this plan does **not** model RAD as FAD with cotangent
dual numbers flowing in ordinary execution order. The causal forward step is
linearization; reverse mode is then obtained by transposing the resulting linear
residual. In Faust there are also two separate meanings of "reverse": graph
transposition, which every RAD path needs, and reverse DSP sample traversal,
which only temporal operators require. `NonlinearUse` records a dependency on a
forward value; FIR lowering still decides whether that dependency becomes a tape
load, recomputation, or an existing forward buffer read. Adjoint carries,
reverse-time buffers, and TBPTT boundaries are temporal scheduling state, not
forward-value tape.

## 2.1 JAX direct-linearize implementation check

JAX's current implementation is a useful concrete precedent for this plan. As
of the reviewed `jax-ml/jax` `main` branch, reverse mode is routed through
`ad.linearize(..., is_vjp=True)`, and `ad.linearize` uses the
`direct_linearize` path by default under the `jax_use_direct_linearize` flag.

The implementation shape is:

```text
jax.vjp / jax.grad
  -> ad.linearize(function, primals, is_vjp=True)
      -> direct_linearize
          -> run the function once under LinearizeTrace
          -> collect primal outputs
          -> build a tangent jaxpr for nonzero tangent outputs
          -> collect residual constants needed by the tangent jaxpr
  -> backward_pass3(tangent_jaxpr, residuals, output_cotangents)
      -> walk the tangent jaxpr backward
      -> apply primitive transpose rules
      -> accumulate input cotangents
```

This confirms the core YOLo structure in an industrial implementation:

- reverse mode is derived from a linearized tangent program, not independently
  from the primal graph;
- local primitive knowledge is expressed as linearization/JVP plus transpose
  rules;
- residuals are captured as values needed by the linearized tangent program;
- the transposer sees a linear program and accumulates cotangents by walking it
  backward.

The corresponding JAX source locations are:

- `jax._src.api._vjp`, which calls `ad.linearize(..., is_vjp=True)` and stores
  residuals in the returned `VJP` object;
- `jax._src.interpreters.ad.direct_linearize`, which builds the primal outputs,
  tangent jaxpr, and residual constants in one traced pass;
- `jax._src.interpreters.ad.LinearizeTrace.process_primitive`, which applies a
  primitive linearization rule and emits tangent code;
- `jax._src.interpreters.ad.fallback_linearize_rule`, which derives
  linearization from an existing JVP rule when no direct linearization rule is
  registered;
- `jax._src.interpreters.ad.backward_pass3`, which transposes the linearized
  jaxpr by walking linear equations backward and applying primitive transpose
  rules;
- `jax._src.api.linear_transpose`, which is the public primitive for
  transposing functions promised to be linear and avoids the forward pass.

Design implications for `faust-rs`:

- the planned `LinearizedProgram` is the Faust analogue of JAX's tangent jaxpr
  plus residual constants;
- `NonlinearUse` is the Faust analogue of values captured as residuals for the
  tangent program, but deliberately abstract so FIR lowering can choose tape,
  recomputation, or an existing forward buffer;
- the `LinNode` transposer should be the only feed-forward RAD reverse program,
  analogous to JAX's `backward_pass3` over the linearized jaxpr;
- a compatibility fallback from current FAD rules to residual linearization is
  reasonable, just as JAX falls back from direct linearization to JVP-derived
  linearization for primitives without a direct rule;
- `SigBlockReverseAD` remains necessary in Faust because JAX's core
  `direct_linearize` path does not solve DSP block scheduling, anti-causal
  delay transposes, recursive carries, or TBPTT boundaries.

This comparison strengthens the plan's main constraint: move AD semantics and
linear residual construction upward, but keep temporal execution policy in
`signal_fir` until a generic reverse-time region and storage scheduler exist.

## 2.2 Faust C++ JAX backend: external oracle, not the target architecture

The C++ Faust compiler already has a JAX backend (`faust -lang jax foo.dsp`).
That backend is highly relevant, but it sits at a different layer from this
plan:

```text
Faust DSP
  -> generated Python/JAX primal code
  -> tick(state, input_sample)
  -> jax.lax.scan(tick, state, input_block)
  -> optional JAX grad/vjp/linearize outside Faust
```

The backend generates a differentiable **primal** program. It does not build a
Faust linear residual, does not expose `NonlinearUse`, does not transpose a
Faust IR, and does not make Faust-side tape/recompute/carry decisions. If a user
applies `jax.grad`/`jax.vjp` to the generated module, JAX performs its own
linearization, residual capture, and transposition over the lowered Python/JAX
program.

This makes `-lang jax` a useful **external oracle** for small parity checks:
compare native `faust-rs rad(expr, seeds)` results against gradients produced by
JAX on the generated `tick + scan` program. It is not a replacement for this
plan, because this plan must make RAD a Faust compiler transformation available
to all backends and tied to Faust's Signal/FIR semantics. In particular:

- JAX differentiates the lowered representation (`state`, `.at[].set`,
  `jnp.roll`, `lax.scan`, Flax parameters, UI conversions), not the Signal IR
  directly;
- JAX owns the `scan` transpose and any full-sequence residual storage policy,
  whereas Faust must preserve the explicit block-local TBPTT contract;
- JAX can validate the mathematical direction, but it cannot decide the
  cross-backend Faust lowering policy for delay carries, recursion boundaries,
  or `NonlinearUse` tape/recompute choices.

## 3. Non-Goals

- Do not implement a full Linear A type system from the paper.
- Do not expose a public `linear_transpose` API yet.
- Do not change the public `rad(expr, seeds)` output layout.
- Do not change `fad(expr, seeds)` output layout.
- Do not remove `SigBlockReverseAD` in the first migration.
- Do not remove the existing `ReverseADTransform` until the new path is a
  default-on implementation with regression coverage.
- Do not attempt exact infinite-horizon gradients for recursive DSPs. The
  current block-local TBPTT boundary remains the semantic contract unless a
  later plan explicitly changes it.

## 4. Current Code Map

### 4.1 Feed-forward symbolic RAD

`crates/propagate/src/reverse_ad.rs` currently:

- walks the active Signal DAG in postorder;
- initializes every primal output cotangent to `1.0`;
- walks the postorder in reverse;
- emits local reverse contributions into an adjoint map;
- returns `[primals..., adjoint(seed_0), ...]`;
- rejects temporal or recursive nodes and routes selected unsupported kinds to
  `SigBlockReverseAD`.

This is already structurally similar to transposition, but it transposes
directly from primal node kinds. The linearized tangent program is implicit.

### 4.2 Local RAD formula sharing

`crates/signals/src/ad_rules.rs` currently:

- classifies supported unary and binary math nodes;
- exposes backend-neutral reverse contribution formulas;
- is used by both symbolic RAD and FIR `BlockReverseAD`;
- does not contain forward tangent formulas;
- does not represent linear residuals.

This module can either be generalized or left as a compatibility layer while a
new linearization module is introduced.

### 4.3 FAD

`crates/propagate/src/forward_ad.rs` currently:

- computes primals and one tangent lane per seed;
- handles seed lifting through recursive De Bruijn scopes;
- already contains the derivative rules that should become the source of truth;
- emits ordinary Signal tangent expressions, not an explicitly linear residual
  IR.

The new plan should reuse the FAD semantics, but not blindly reuse the current
multi-lane implementation as the first internal representation.

### 4.4 Block reverse AD

`crates/transform/src/signal_fir/module.rs` and
`crates/transform/src/signal_fir/block_reverse_ad.rs` currently:

- lower `SigBlockReverseAD` into a block-local reverse sweep;
- infer tape-needed forward values from reverse rules;
- emit delay/prefix/recursive adjoint carries;
- choose split public-output schedule or inline adaptive schedule;
- lower local reverse formulas in FIR.

This is the main area where the linearize-once design can remove special-case
duplication, but only after feed-forward parity is proven.

### 4.5 Stateful RAD scaffolding

`crates/propagate/src/stateful_rad.rs` and
`crates/propagate/src/transpose_ad.rs` already contain an LTI-oriented
linearity classifier and transposition scaffold. These should become
optimizations or validators over the future linear residual, not a separate
competing RAD path.

## 5. Target Internal Model

### 5.1 Linear residual invariant

Introduce an internal residual IR whose linear nodes are linear only in tangent
inputs. Nonlinear Signal values may appear only as coefficients, branch
conditions, table references, or tape dependencies.

The invariant is:

```text
residual(nonlin_values; tangent_inputs) -> tangent_outputs
```

where:

- `nonlin_values` are ordinary primal Signal values or FIR/tape loads;
- `tangent_inputs` represent perturbations of selected seeds and state lanes;
- every residual output is algebraically linear in `tangent_inputs`;
- nonlinear operations are allowed only outside the linear dependency path or
  as coefficients computed from `nonlin_values`.

This is weaker than the paper's full substructural type system, but strong
enough for `faust-rs` because Signal/FIR AD is first-order and scalar/product
based at this stage.

### 5.2 Proposed Rust data types

Start with an internal module:

```text
crates/propagate/src/linearize_once.rs
```

Initial private types:

```rust
pub(crate) struct LinearizedProgram {
    pub primals: Vec<SigId>,
    pub seed_inputs: Vec<SigId>,
    pub outputs: Vec<LinId>,
    pub nodes: Vec<LinNode>,
    pub nonlinear_uses: Vec<NonlinearUse>,
}

pub(crate) struct LinearizedValue {
    pub primal: SigId,
    pub tangent: LinId,
}

pub(crate) struct LinId(u32);

pub(crate) enum LinNode {
    Zero,
    Input { seed_index: usize },
    Add(LinId, LinId),
    Sub(LinId, LinId),
    Neg(LinId),
    Scale { coeff: NonlinearUseId, value: LinId },
    DivBy { denom: NonlinearUseId, value: LinId },
    Select {
        cond: NonlinearUseId,
        when_zero: LinId,
        when_nonzero: LinId,
    },
    Delay1(LinId),
    DelayConst { amount: i32, value: LinId },
    Prefix { init: LinId, value: LinId },
}

pub(crate) enum NonlinearUse {
    Signal(SigId),
    UnaryMath { op: NonlinearMathOp, x: SigId },
    BinaryMath { op: NonlinearMathOp, lhs: SigId, rhs: SigId },
}
```

Notes:

- `NonlinearUse` is deliberately not a tape slot. It is an abstract dependency
  on a forward value. Signal lowering can rebuild it; FIR lowering can choose a
  tape load or recomputation.
- `Scale` and `DivBy` are the primitive linear multiplication/division forms.
- `Select` is linear because `cond` is nonlinear/primal-only.
- `Delay1`, `DelayConst`, and `Prefix` must stay out of the first feed-forward
  default path, but including them in the IR early gives the block path a
  target.
- The IR does not need explicit `Dup`. A residual graph may fan out; its
  transpose accumulates cotangents in an adjoint map. That is equivalent to
  transposing explicit fan-out into addition.

### 5.3 Why not ordinary `SigId` tangents only?

Ordinary Signal tangents do not mark which subexpressions are linear in tangent
variables. A later transpose would have to rediscover linearity from arbitrary
Signal syntax, which is exactly what the paper avoids.

The explicit residual IR gives the transposer a small trusted language:

- zero;
- addition/subtraction;
- scaling by primal values;
- primal-controlled selection;
- temporal linear primitives.

Any unsupported shape fails at linearization time, before a wrong transpose can
be emitted.

### 5.4 Signal-level versus FIR-level responsibility

The linearize-once model does **not** mean that all `SigBlockReverseAD`
implementation work moves to the Signal level.

It moves the AD semantics and the differentiability proof boundary upward, but
the executable block schedule still belongs to `signal_fir`.

Signal/residual level should own:

- deciding that `rad(expr, seeds)` needs block-local reverse-mode semantics;
- building the primal outputs and the linear residual;
- proving or validating that the residual is linear in tangent inputs;
- recording which primal/nonlinear values the residual depends on;
- classifying temporal residual nodes (`Delay1`, `DelayConst`, `Prefix`,
  future recursion nodes);
- preserving the public `SigBlockReverseAD` projection contract:

  ```text
  Proj(0..M-1, group)     -> primal outputs
  Proj(M..M+N-1, group)   -> seed gradients
  ```

- emitting diagnostics when the residual cannot represent a required
  derivative.

Signal/residual level should **not** own:

- allocation of `fBraTapeN` arrays;
- selection of tape versus recomputation for one backend lowering context;
- allocation of `fBraCarryN` or `fBraDelayCarryN` fields;
- split public-output loop versus inline adaptive placement;
- `real_ty` selection and FIR value typing;
- `FAUSTFLOAT` boundary casts;
- recursion carrier allocation (`SingleScalar`, `TwoSlotShift`, `Circular`);
- sample-phase placement (`immediate`, `post_output`, preamble reset);
- use of public output buffers as block-local replay storage;
- C++/Julia/backend-specific storage and helper emission.

Only pure feed-forward RAD can be fully materialized back into ordinary Signal
IR after residual transposition. Once the residual contains block-time
operators, the Signal layer can only carry a semantic representation. The
actual imperative realization remains a lowering concern.

Therefore the target shape for temporal RAD is:

```text
propagate:
  Signal graph -> LinearizedProgram / residual -> SigBlockReverseAD carrier

signal_prepare:
  normal Signal preparation, including De Bruijn-to-symbolic recursion mapping

signal_fir:
  SigBlockReverseAD + residual metadata
    -> tape/recompute plan
    -> carry allocation
    -> split or inline loop schedule
    -> FIR statements
```

If the residual IR must be consumed by `signal_fir`, it may later need to move
from `propagate` to a shared crate or be embedded in a stable Signal-side
carrier payload. That is a crate-boundary decision, not a reason to force tape
allocation or loop scheduling into Signal IR.

### 5.5 Tape, primal dependencies, and time direction

This plan uses `tape` in the narrow implementation sense already present in
`BlockReverseAD`: storage or replay of forward/primal values needed by the
reverse sweep. It does **not** mean every reverse-time buffer.

Keep three storage classes separate:

| Storage class | Purpose | Trigger |
|---|---|---|
| primal value tape or recomputation | recover forward values used as local Jacobian coefficients | local derivative depends on a forward value or branch decision |
| adjoint carry/buffer | move cotangents through a transposed temporal operator | delay, prefix, recursion, or any anti-causal transpose |
| operation trace/residual IR | record the linearized program shape | needed for generic residual transposition |

The tape of primal values is therefore dictated by **primal-dependent local
Jacobians**, not by physical DSP time by itself. Nonlinearity is the most common
source of such Jacobians, but it is not the only one:

- `x * x` needs `x` in reverse because the local coefficient is `2 * x`;
- `sin(x)` needs `x` or `sin/cos(x)` in reverse;
- `pow(x, y)` needs operand/primal-output residual values;
- `min`, `max`, and `select2` need the forward branch predicate;
- state-dependent table reads or variable-delay slopes need forward indices or
  local slope values;
- active control flow, if later supported, needs the forward branch path.

By contrast, a linear time-invariant operator can force a reverse-time schedule
without forcing a primal tape:

```text
forward: y[n] = a * x[n - 1]     where a is constant
reverse: adj_x[n] += a * adj_y[n + 1]
```

This needs an adjoint carry for `adj_y[n + 1]`, but it does not need a primal
value tape for `x[n]`.

Time changes the scale and schedule of the storage problem:

- without temporal operators, a feed-forward graph may still need residual
  forward values for reverse mode;
- with temporal operators, the same residual values become indexed by sample
  inside the block;
- block-local TBPTT fixes how far the reverse-time cotangents and primal
  residual values are retained;
- checkpointing/recomputation is an implementation alternative to storing every
  `NonlinearUse` in a tape.

Consequences for this migration:

- `NonlinearUse` records a forward value dependency, not a mandatory tape slot;
- temporal `LinNode`s record reverse-time cotangent flow, not primal tape needs;
- FIR lowering decides whether each `NonlinearUse` is taped, recomputed, or
  available from an existing buffer;
- tests must separately count primal tape stores and adjoint carry fields.

## 6. Phase 0 Gate

Before implementation beyond tests/prototype, confirm:

1. Production path: the first wired path remains
   `parse -> boxes -> eval -> propagate -> normalize -> transform -> fir ->
   backend`.
2. Differential baseline: feed-forward RAD can be compared against the current
   `ReverseADTransform` and against FAD for small seed counts.
3. Global state: all linearize-once state lives in a transform object keyed by
   `SigId`; no global AD cache.
4. TreeArena sharing: no transform may duplicate a shared primal subgraph except
   where the current FAD/RAD implementation already does so for formula terms.
5. API lifecycle: public `rad(expr, seeds)` remains unchanged; custom output
   cotangents remain a future VJP API.
6. Temporal semantics: block-local TBPTT stays the contract for
   `SigBlockReverseAD`.

Pass criterion for Phase 0:

- a short implementation note appended to this plan or a follow-up plan;
- at least one test fixture selected for each of feed-forward, delay, and
  recursive block RAD parity;
- no code path switched by default.

## 7. Phase 1: Feed-Forward Prototype

### 7.1 Goal

Build a feed-forward linearize-once RAD path that produces the same outputs as
the current symbolic `ReverseADTransform` for non-temporal graphs.

### 7.2 Files

Add:

- `crates/propagate/src/linearize_once.rs`
- unit tests in `crates/propagate/src/linearize_once.rs`

Touch:

- `crates/propagate/src/lib.rs` to expose the private module;
- `crates/propagate/src/reverse_ad.rs` only for test-only comparison hooks at
  first.

### 7.3 Linearizer algorithm

For each primal output:

1. Memoize `SigId -> LinearizedValue`.
2. If `sig` equals seed `j`, return:

   ```text
   primal = sig
   tangent = LinNode::Input { seed_index: j }
   ```

3. If `sig` is a constant/input/UI leaf not equal to any seed, return:

   ```text
   primal = sig
   tangent = Zero
   ```

4. For each supported composite node, recursively linearize children and emit:

   ```text
   primal = original reconstructed primal
   tangent = derivative rule expressed in LinNode primitives
   ```

5. For unsupported temporal/recursive nodes, return a typed error:

   ```text
   LinearizeOnceError::NeedsBlockReverseAd { kind, node }
   ```

6. Preserve seed order and repeated seed lanes. If the same seed appears twice,
   create two `Input` lanes and map the same `SigId` to both lane indices.

### 7.4 Initial accepted feed-forward nodes

Match current symbolic RAD:

- `Int`, `Real`, `Input`;
- `HSlider`, `VSlider`, `NumEntry`, `Button`, `Checkbox`;
- `BinOp` Add/Sub/Mul/Div/Rem and discrete zero-gradient arms;
- `Sin`, `Cos`, `Tan`, `Exp`, `Log`, `Log10`, `Sqrt`, `Abs`;
- `Acos`, `Asin`, `Atan`;
- `Pow`, `Min`, `Max`, `Atan2`, `Fmod`, `Remainder`;
- `FloatCast`, `IntCast`;
- `Select2`;
- read-only `RdTbl`;
- recognized unary `FFun`;
- `Attach`, `Enable`, `Control`, `Output`;
- bargraphs as sinks.

Reject with the same fallback/hard-error distinction as current RAD:

- `Delay1`, `Delay`, `Prefix`;
- `Rec`, `Proj`, `Iir`;
- writable table paths;
- soundfile paths;
- opaque families.

### 7.5 Required derivative encodings

The residual must encode tangent rules, not reverse rules. Examples:

```text
d(x + y) = dx + dy
d(x - y) = dx - dy
d(x * y) = y * dx + x * dy
d(x / y) = dx / y - (x / (y * y)) * dy
d(sin x) = cos(x) * dx
d(cos x) = -sin(x) * dx
d(exp x) = exp(x) * dx
d(log x) = dx / x
```

`Pow` must use a stable base derivative:

```text
d(pow(x, y)) =
    y * pow(x, y - 1) * dx
  + pow(x, y) * log(x) * dy
```

Do not use:

```text
pow(x, y) * y * dx / x
```

because that regresses common cases such as `pow(0, 2)`.

`Min` and `Max`:

```text
d(min(x, y)) = select_nonzero(x <= y, dx, dy)
d(max(x, y)) = select_nonzero(x >= y, dx, dy)
```

`Select2(cond, x, y)`:

```text
d(select2(cond, x, y)) = select2(cond, dx, dy)
```

where `cond` is primal-only. Do not propagate a tangent through `cond`.

`RdTbl(table, idx)`:

```text
slope(idx) = (rdtbl(table, idx + 1) - rdtbl(table, idx - 1)) / 2
d(rdtbl(table, idx)) = slope(idx) * d_idx
```

### 7.6 Transposer algorithm

Input:

- `LinearizedProgram.outputs`;
- one cotangent per primal output, initially all `1.0`;
- the residual node arena.

Algorithm:

1. Build reverse postorder over `LinId` outputs.
2. Initialize `lin_adj[output_i] += output_cotangent_i`.
3. Walk reverse postorder.
4. For each `LinNode`, propagate cotangent to children:

   ```text
   Zero                       -> no-op
   Input(seed_index)          -> seed_adj[seed_index] += y_bar
   Add(a, b)                  -> adj[a] += y_bar; adj[b] += y_bar
   Sub(a, b)                  -> adj[a] += y_bar; adj[b] -= y_bar
   Neg(a)                     -> adj[a] -= y_bar
   Scale(c, a)                -> adj[a] += c * y_bar
   DivBy(d, a)                -> adj[a] += y_bar / d
   Select(cond, z, nz)        -> route y_bar by primal cond
   Delay1(a)                  -> block-only in later phase
   DelayConst { amount, a }   -> block-only in later phase
   Prefix { init, value }     -> block-only in later phase
   ```

5. Emit seed adjoints as Signal expressions in the same seed order.
6. Return `[primals..., seed_adjoints...]`.

For Phase 1, temporal linear nodes must not be accepted by the default
transposer. They are reserved for Phase 4.

### 7.7 Phase 1 tests

Add unit tests for:

- linear residual shape of `x * y`;
- linear residual shape of `pow(x, y)` with stable `x^(y - 1)` base term;
- repeated seed order `[x, x]`;
- unused seed returns zero;
- `select2` routes through branches only;
- `min`/`max` branch convention matches `ad_rules.rs`;
- read-only `RdTbl` slope is represented as a nonlinear coefficient;
- unsupported `Delay1` returns `NeedsBlockReverseAd`.

Add parity tests comparing old RAD and linearize-once RAD for:

- `sin(x * y)`;
- `pow(x, y)`;
- `atan2(y, x)`;
- `min/max`;
- mixed `select2`;
- read-only table lookup;
- recognized unary `FFun`.

Pass criteria:

- `cargo fmt --all`
- `cargo test -p propagate linearize_once`
- existing `reverse_ad` tests unchanged and passing

## 8. Phase 2: Optional Test-Only Wiring

### 8.1 Goal

Run the new path in tests without changing user-visible behavior.

### 8.2 API shape

Add an internal function:

```rust
pub(super) fn generate_rad_signals_linearize_once(
    arena: &mut TreeArena,
    primals: &[SigId],
    seeds: &[SigId],
) -> Result<Vec<SigId>, PropagateError>
```

Keep `generate_rad_signals` on the current implementation.

### 8.3 Comparison helper

Add a test helper:

```rust
fn assert_rad_equivalent_for_feed_forward_case(...)
```

Equivalence should not require textual identity. Prefer one of:

- normalized readable Signal dump after canonical simplification;
- runtime evaluation parity on a small grid;
- structural checks for critical formulas such as `pow`.

Pass criteria:

- all Phase 1 tests;
- parity cases pass across `Float32` and `Float64` real type where applicable.

## 9. Phase 3: Make Linearize-Once Feed-Forward RAD Default

### 9.1 Preconditions

- Phase 1 and Phase 2 pass.
- The new path produces the same fallback kinds as old RAD for temporal and
  recursive inputs.
- `pow(0, 2)` base derivative stability is covered.
- repeated seed semantics are covered.

### 9.2 Change

In `reverse_ad.rs`, switch feed-forward `generate_rad_signals` to:

1. call `generate_rad_signals_linearize_once`;
2. on `NeedsBlockReverseAd`, build `SigBlockReverseAD`;
3. on hard unsupported nodes, preserve current diagnostics.

Keep the old `ReverseADTransform` behind `#[cfg(test)]` for one cycle, or move
it to a test oracle module.

### 9.3 Deletion candidates after one stable cycle

- `ReverseADTransform::active_children`;
- `ReverseADTransform::propagate_adjoint`;
- `propagate_unary_math`;
- `propagate_binary_math`;
- `propagate_binop`;
- FFUN reverse-only formula arms, after they are represented in the common
  linearization table.

Pass criteria:

- `cargo fmt --all`
- `cargo test -p propagate reverse_ad`
- `cargo test -p signals ad_rules`
- `cargo test --workspace --all-targets` when feasible

## 10. Phase 4: Generalize Local Derivative Rules

### 10.1 Goal

Move from shared reverse formulas to shared derivative facts.

### 10.2 Proposed module boundary

Either extend `crates/signals/src/ad_rules.rs` or add:

```text
crates/signals/src/differential_rules.rs
```

The module should expose:

- rule classification for Signal math nodes;
- local JVP/tangent contribution builders;
- metadata about required primal nonlinear values;
- optional reverse contribution compatibility wrappers while old code exists.

### 10.3 Builder traits

Introduce a tangent formula builder separate from `RadFormulaBuilder`:

```rust
pub trait TangentFormulaBuilder {
    type Primal: Copy;
    type Tangent: Copy;

    fn zero_tangent(&mut self) -> Self::Tangent;
    fn add_tangent(&mut self, a: Self::Tangent, b: Self::Tangent) -> Self::Tangent;
    fn neg_tangent(&mut self, a: Self::Tangent) -> Self::Tangent;
    fn scale(&mut self, coeff: Self::Primal, tangent: Self::Tangent) -> Self::Tangent;
    fn div_by(&mut self, tangent: Self::Tangent, denom: Self::Primal) -> Self::Tangent;
    fn select_nonzero(
        &mut self,
        cond: Self::Primal,
        when_true: Self::Tangent,
        when_false: Self::Tangent,
    ) -> Self::Tangent;
}
```

The Phase 1 `LinearizeOnceBuilder` implements this by emitting `LinNode`s.
Existing FAD may later implement it by emitting ordinary Signal tangent
expressions.

### 10.4 Rule ownership

After this phase:

- FAD and RAD should share tangent rules;
- RAD should derive reverse behavior from transposition;
- `RadFormulaBuilder` should either become a temporary compatibility layer or
  be reduced to tests for the old path.

Pass criteria:

- shared tangent formula tests for all supported math operators;
- no local math derivative formula duplicated between FAD and RAD for the
  feed-forward subset;
- direct tests for FFUN tangent formulas.

## 11. Phase 5: BlockReverseAD from Linear Residuals

### 11.1 Goal

Replace hand-coded FIR `propagate_bra_adj` math dispatch with transposition of
the same residual IR used by feed-forward RAD.

### 11.2 Required extension

The linear residual must support temporal linear primitives:

```text
Delay1(dx)
DelayConst(amount, dx)
Prefix(dinit, dx)
RecLinearGroup(...)
```

The block transposer lowers these to FIR carries:

```text
Delay1:
  forward  y[n] = x[n - 1]
  reverse  adj_x[n] += adj_y[n + 1]

DelayConst(c):
  forward  y[n] = x[n - c]
  reverse  adj_x[n] += adj_y[n + c]

Prefix:
  forward  y[0] = init; y[n] = x[n - 1]
  reverse  adj_x[n] += adj_y[n + 1]
           adj_init += adj_y[0]
```

### 11.3 Tape inference changes

Current tape inference:

```text
reverse rule table -> required forward operands -> tape set
```

Target tape inference:

```text
linear residual -> NonlinearUse set -> primal tape/recompute policy
```

`collect_tape_needed_values` should become a compatibility wrapper over the
new residual nonlinear-use set.

Do not infer primal tape needs from temporal structure alone. A residual such as
`Delay1(dx)` needs an adjoint carry, but no forward-value tape. A residual such
as `Scale(y, Delay1(dx))` needs an adjoint carry for the delay and a
`NonlinearUse` for `y[n]`, which FIR lowering may implement as a tape load,
recomputation, or an existing forward buffer read.

The compatibility wrapper should therefore expose two separate products:

```text
NonlinearUse set       -> values requiring replay or recomputation
Temporal residual set  -> adjoint carry/buffer requirements
```

### 11.4 FIR lowering changes

Add a FIR residual transposer in `crates/transform/src/signal_fir`:

```text
linear_residual_fir.rs
```

Responsibilities:

- map `NonlinearUse::Signal(sig)` to `load_bra_fwd_value(sig)`;
- map `LinNode` transposition to FIR adjoint statements;
- allocate/use delay and prefix carry fields;
- accumulate seed gradients into `bra_grad_cache`;
- preserve split vs inline schedule decisions in `module.rs`.

Keep schedule ownership in `module.rs`. The residual transposer should emit
statements into the current sample phases; it should not decide whether the
surrounding loop is forward or reverse.

This is intentionally not a pure Signal-level lowering. `SigBlockReverseAD`
depends on facts that are unknown or not yet stable at propagation time:

- whether the gradient projection is a public output or an internal operand of
  a forward recursive update;
- whether `i0` is being emitted in a forward loop or a reverse loop;
- the backend internal real type;
- whether a nonlinear residual use can be recomputed safely or must be taped;
- the concrete delay strategy and recursion storage strategy selected by the
  FIR planner;
- the sample phase in which a tape store or carry store must be emitted.

The new model should change the source of the reverse program, not the owner of
the executable schedule:

```text
old:
  Signal node kind -> hand-written FIR reverse rule -> tape/carry statements

new:
  Signal linearization -> residual IR -> FIR transposition of residual
       -> tape/carry statements
```

Thus the Signal layer says *what linear operator must be transposed*. The FIR
layer still says *how that transpose is executed for this backend and loop
placement*.

### 11.5 Phase 5 tests

Compare old and new BRA lowering on:

- delay1 linear expression;
- delay const expression;
- prefix expression;
- `Delay1(x) * y`, requiring a taped nonlinear value;
- `pow(Delay1(x), y)`, requiring operand and primal output nonlinear uses;
- simple recursive one-pole;
- nonlinear recursive feedback with TBPTT.

Pass criteria:

- existing compiler `BlockReverseAD` tests pass;
- generated FIR for simple feed-forward BRA no longer contains duplicated local
  reverse math implementation;
- tape store count is no larger than current implementation for the listed
  fixtures unless documented.

## 12. Detailed Delay and Recursive-Form Analysis

This section is deliberately more precise than the high-level phase list. The
linearize-once migration is only correct if delays and recursion are modeled as
linear temporal operators with explicit block-boundary behavior.

### 12.1 Time and block convention

The existing `SigBlockReverseAD` contract is block-local TBPTT:

```text
forward pass: n = 0 .. count - 1
reverse pass: n = count - 1 .. 0
```

Adjoint carries are reset at `compute()` entry. Therefore:

- gradients do not cross host `compute()` calls;
- delay and recursion transposes are exact only inside the current block;
- any contribution that would come from `n >= count` is zero;
- any carry written for `n < 0` is discarded at the block boundary.

The residual IR must make this contract explicit. A temporal residual node is
not just a pure Signal expression; it is a block-local linear operator with a
defined transpose schedule.

This section is about reverse-time cotangent flow. It must not be confused with
primal tape requirements:

- `Delay1(dx)`, `DelayConst(dx)`, and `Prefix(dinit, dx)` require adjoint
  carries because their transpose reads future cotangents inside the block;
- they require no primal tape when their coefficients are constant;
- adding a primal-dependent coefficient, branch predicate, or variable-delay
  slope introduces `NonlinearUse` entries that FIR lowering must tape or
  recompute per sample.

### 12.2 Delay forms in Signal IR

The current relevant Signal forms are:

```text
Delay1(x)
Delay(x, amount)
Prefix(init, x)
Delay1^k(Proj(slot, group))
```

`Delay1^k(Proj(slot, group))` is both a delay chain and a possible recursion
carrier read. The implementation must classify it before lowering because the
storage/carry key differs for ordinary delays and recursive feedback.

### 12.3 `Delay1(x)`

Forward semantics:

```text
y[n] = x[n - 1]
y[0] = initial delay state
```

Linearization:

```text
dy[n] = dx[n - 1]
```

Residual node:

```text
LinNode::Delay1(dx)
```

Transpose inside one block:

```text
adj_x[n] += adj_y[n + 1]
```

FIR lowering shape:

```text
carry starts at 0
for n = count - 1 downto 0:
    adj_x[n] += carry        // carry is adj_y[n + 1]
    carry = adj_y[n]         // for the next reverse iteration, n - 1
```

Implementation requirements:

- one scalar carry per differentiated `Delay1` residual instance;
- carry reset in compute preamble;
- carry store emitted after the current reverse contribution has been consumed;
- no attempt to differentiate the implicit pre-block initial state unless a
  future API exposes it as a seed.

### 12.4 `Delay(x, amount)` with constant amount

Forward semantics for constant `c`:

```text
y[n] = x[n - c]
```

Linearization:

```text
dy[n] = dx[n - c]
```

Residual node:

```text
LinNode::DelayConst { amount: c, value: dx }
```

Transpose:

```text
adj_x[n] += adj_y[n + c]
```

FIR lowering shape:

```text
carry[0..c-1] starts at 0
for n = count - 1 downto 0:
    slot = n % c
    adj_x[n] += carry[slot]  // adj_y[n + c]
    carry[slot] = adj_y[n]   // for sample n - c
```

Special case:

```text
c == 0 -> identity
```

Implementation requirements:

- Phase 5 must accept only statically known integer amounts;
- negative amounts must be rejected before residual construction;
- the carry array size must be `c`, not the forward delay-line size;
- `c` must be bounded so the generated carry array is finite and reasonable.

### 12.5 `Delay(x, amount)` with variable amount

Variable delay is not the same residual operator as `DelayConst`.

Forward semantics:

```text
y[n] = x[n - d[n]]
```

Forward tangent, following the existing FAD approximation model, has two parts:

```text
dy[n] =
    delay(dx, d[n])
  - dd[n] * local_time_slope(x, n - d[n])
```

Reverse mode therefore needs:

1. a scatter-like contribution from `adj_y[n]` to `adj_x[n - d[n]]`;
2. a scalar contribution to `adj_d[n]` using the recorded local slope;
3. primal/tape values for `d[n]` and the local slope.

This cannot be represented by `LinNode::DelayConst`.

Initial policy:

- Phase 5 must reject variable-delay residuals explicitly, or keep routing them
  through the current documented fallback if that fallback is proven correct;
- do not silently coerce `amount` with `tree_to_int(...).unwrap_or(0)`;
- add a specific diagnostic kind such as `"variable-delay-rad-deferred"`;
- add a follow-up plan before accepting variable-delay RAD.

Required future residual form:

```text
LinNode::DelayVar {
    value_tangent: LinId,
    amount_tangent: LinId,
    amount_value: NonlinearUseId,
    slope_value: NonlinearUseId,
}
```

Required future FIR capability:

- dynamic indexed adjoint scatter or equivalent finite carry structure;
- tape of `amount_value` per sample;
- tape or recomputation of `slope_value`;
- bounds proof for `d[n]`.

### 12.6 `Prefix(init, x)`

Forward semantics:

```text
y[0] = init
y[n] = x[n - 1] for n > 0
```

Linearization:

```text
dy[0] = dinit
dy[n] = dx[n - 1] for n > 0
```

Residual node:

```text
LinNode::Prefix {
    init: dinit,
    value: dx,
}
```

Transpose:

```text
adj_init += adj_y[0]
adj_x[n] += adj_y[n + 1]
```

FIR lowering shape:

```text
for n = count - 1 downto 0:
    adj_x[n] += carry
    if n == 0:
        adj_init += adj_y[n]
    carry = adj_y[n]
```

Implementation requirements:

- do not treat `Prefix` as only `Delay1`;
- preserve the sample-0 boundary contribution to `init`;
- reject or document any case where `init` is discrete/non-real and cannot
  receive a real adjoint.

### 12.7 Recursive forms before and after preparation

Propagation-side recursion uses De Bruijn carriers:

```text
DEBRUIJNREC([body_0, body_1, ...])
DEBRUIJNREF(level)
Proj(slot, DEBRUIJNREF(level))
```

FIR lowering after `de_bruijn_to_sym` sees symbolic carriers:

```text
Proj(slot, SYMREC(var, body_list))  // top-level recursive output
Proj(slot, SYMREF(var))             // back-reference inside the body
Delay1(Proj(slot, SYMREF(var)))     // canonical one-sample feedback edge
```

The linearize-once implementation must not mix these layers:

- propagation/residual construction should reason in De Bruijn form;
- FIR residual lowering should reason in SYMREC/SYMREF form;
- conversion between them remains owned by the existing signal preparation
  path.

### 12.8 Recursive residual model

For a recursive group with `N` lanes, introduce state tangent inputs:

```text
state_dot_prev[0..N-1]
```

Linearizing the body produces:

```text
body_dot =
  residual(seed_dot, state_dot_prev, other_outer_state_dot)
```

The block transpose uses:

```text
body_bar[n] =
    external_cotangent[n]
  + state_bar_from_future[n]
```

Then transposes the body residual to produce:

```text
seed_bar[n]
state_bar_prev[n - 1]
outer_state_bar_prev[n - 1]
```

The previous-state adjoints become anti-causal carries, exactly like delay
transposes.

### 12.9 Recursive class matrix

The residual approach changes the meaning of the existing classes:

| Class | Current role | Residual role |
|---|---|---|
| LTI recursion | candidate for `ReverseTimeRec` scaffold | optimization over a residual whose coefficients are constants |
| LTV recursion | classified but no specialized path | exact block residual with coefficient tape/replay |
| Nonlinear recursion | generic BRA/BPTT | linearized body residual with primal coefficients taped/recomputed |

Important point: nonlinear recursive bodies still produce a linear residual
after linearization. They are not rejected because the residual is nonlinear;
they are rejected only if the linearizer cannot express the local derivative or
the block transposer cannot schedule the required temporal dependencies.

Storage implication:

- LTI recursion needs reverse-time adjoint carries, but no primal coefficient
  tape;
- LTV recursion needs the coefficient trajectory, either taped or recomputed;
- nonlinear recursion needs every primal-dependent local derivative required by
  the linearized body, again either taped or recomputed;
- these choices are storage policies over `NonlinearUse`, not changes to the
  mathematical residual.

### 12.10 Multi-output recursive groups

Multi-output groups require keying by both recursion identity and slot.

Do not key feedback carries by slot alone:

```text
wrong: slot
right: (recursion variable identity, slot, delay chain)
```

Reason: two independent recursion groups may both have `slot = 0`. The current
BRA pre-scan already matches `SYMREF(var)` to `SYMREC(var, ...)`; the residual
transposer must preserve that rule.

Required carry key:

```rust
struct RecAdjointCarryKey {
    group_var: TreeId,
    slot: usize,
    implicit_delay: usize,
    residual_group: SigId,
}
```

The exact struct may differ, but these four facts must be represented.

### 12.11 Nested recursive groups

Nested `DEBRUIJNREC` changes the meaning of `DEBRUIJNREF(level)`.

Rules:

- entering a nested group increments the current De Bruijn level;
- seed equality must be lifted exactly as current FAD does;
- state tangent inputs belong to the current group only;
- references to outer groups are nonlinear/time-varying inputs from the inner
  group's point of view unless the implementation explicitly models
  cross-group tangents.

Initial policy:

- Phase 6 should support one active recursive group first;
- nested recursion should be rejected with a dedicated diagnostic until tests
  cover level shifting and cross-group cotangent routing;
- do not let a nested `Proj(slot, DEBRUIJNREF(k))` accidentally bind to the
  wrong group.

### 12.12 Recursion plus internal RAD gradient projection

The current backend supports an inline adaptive schedule:

```text
recursive update body consumes Proj(M + j, BlockReverseAD)
```

In that case, the public output remains forward-time, but the BRA sweep is
emitted while lowering the recursive body.

The residual design must preserve this separation:

- `classify_reverse_time_outputs` must still stop at `SYMREC` boundaries;
- residual transposition must be callable from the current sample-loop slice;
- schedule ownership remains in `module.rs`;
- the residual transposer must not force a public reverse loop just because a
  gradient projection appears inside a recursive body.

### 12.13 IIR and FIR carriers

`SigIIR` and structured FIR/IIR carriers should not get special semantics in
the first residual implementation.

Initial policy:

- if the carrier has already been expanded to ordinary delay/recursion Signal
  forms, the residual path handles those forms;
- direct `SigIIR` may continue to use existing classification/diagnostics;
- direct state-space transposition remains an optimization, not a correctness
  baseline.

### 12.14 Delay and recursion acceptance matrix

| Form | Phase 1 feed-forward | Phase 5 block residual | Phase 6 recursive residual |
|---|---|---|---|
| no delay/no rec | accept | accept | accept |
| `Delay1(x)` | route to BRA | accept with scalar carry | accept inside body |
| `Delay(x, const)` | route to BRA | accept with carry array | accept inside body |
| `Delay(x, variable)` | route/defer | defer unless explicit scatter implemented | defer |
| `Prefix(init, x)` | route to BRA | accept with init boundary term | accept inside body |
| `Proj(slot, DEBRUIJNREF(1))` | route to BRA | not a FIR form | accept as state tangent input |
| `Proj(slot, SYMREC(var,...))` | not a propagate form | identity to body slot | FIR lowering only |
| `Proj(slot, SYMREF(var))` | not a propagate form | handled by feedback carry | FIR lowering only |
| nested `DEBRUIJNREC` | route/defer | after preparation only | defer initially |
| LTI recursion | route to BRA or optimization | accept | optimization candidate |
| LTV recursion | route to BRA | accept with coefficient tape | accept |
| nonlinear recursion | route to BRA | accept with primal tape | accept |
| direct `SigIIR` | current diagnostic/fallback | defer unless expanded | optimization candidate |

### 12.15 Required tests for this section

Add targeted tests before switching any temporal/recursive default:

- `Delay1(x)` gradient shifts one sample backward inside a block.
- `Delay(x, 3)` uses a carry array of size `3`.
- `Delay(x, 0)` is identity.
- `Prefix(init, x)` gives `init` only the sample-0 adjoint.
- `Delay1(Proj(slot, SYMREF(var)))` pre-seeds the matching
  `Proj(slot, SYMREC(var,...))` by `(var, slot)`, not slot alone.
- two independent one-lane recursive groups do not share adjoint carries.
- one two-lane recursive group routes slot `0` and slot `1` independently.
- nested recursion is rejected with a clear diagnostic until implemented.
- variable delay is rejected with `"variable-delay-rad-deferred"` until an
  explicit scatter/tape design exists.

## 13. Phase 6: Recursive Linearization

### 13.1 Goal

Use the same residual model for recursive bodies instead of maintaining a
separate recursive RAD model.

### 13.2 Strategy

For a recursive group:

1. Treat prior state lanes as additional tangent inputs.
2. Linearize the recursive body once.
3. The residual maps:

   ```text
   seed tangents + previous-state tangents -> output tangents
   ```

4. Transpose the residual in block order to obtain:

   ```text
   output cotangents -> seed cotangents + previous-state cotangents
   ```

5. Feed previous-state cotangents into the existing carry mechanism.

### 13.3 Relationship to `stateful_rad.rs`

`stateful_rad.rs` should remain useful as:

- an optimizer gate for LTI and LTV special cases;
- a diagnostic classifier;
- a way to select whether a closed-form transpose optimization is legal.

It should not be required for correctness of general recursive TBPTT once the
linear residual block path is implemented.

### 13.4 Relationship to `transpose_ad.rs`

`transpose_ad.rs` should become an optimization over residuals classified as
LTI:

- exact finite-block transpose remains the semantic baseline;
- LTI transpose may emit smaller/faster code;
- fallback remains the residual block transposer.

Pass criteria:

- recursive one-pole and two-pole parity against current BRA;
- nonlinear recurrence parity against finite-difference or reference executor;
- no user-visible regression in supported subset documentation.

## 14. Phase 7: Retire Compatibility Code

Only after the new path has been default for feed-forward and block cases:

1. Remove old symbolic reverse sweep code no longer used outside tests.
2. Remove old FIR `propagate_bra_adj` local math dispatch.
3. Keep small targeted tests for old bug classes:
   - `pow(0, 2)`;
   - repeated seeds;
   - branch subgradient convention;
   - delay carry direction;
   - recursive SYMREC/SYMREF carry matching.
4. Update `porting/faust-rs-supported-faust-subset-en.md`.
5. Update `JOURNAL.md` daily entry when the implementation lands.

Pass criteria:

- line deletion exceeds compatibility code added in the final cleanup commit;
- no duplicate feed-forward math derivative formulas remain in FAD/RAD;
- full workspace quality gate passes.

## 15. Diagnostics and Error Policy

Do not silently drop gradients.

Linearization errors should distinguish:

```rust
enum LinearizeOnceError {
    NeedsBlockReverseAd { node: SigId, kind: &'static str },
    UnsupportedHard { node: SigId, kind: &'static str },
    MalformedIr { node: SigId, detail: String },
    NonlinearResidual { detail: String },
}
```

Mapping:

- `NeedsBlockReverseAd` routes to `SigBlockReverseAD`.
- `UnsupportedHard` becomes `PropagateError::RadUnsupportedNode`.
- `MalformedIr` becomes a pass-specific internal consistency error.
- `NonlinearResidual` is a bug unless the node was explicitly accepted as
  nonlinear; add a regression test before fixing.

## 16. Validation Matrix

### 16.1 Unit tests

`signals`:

- shared tangent rule classification;
- `pow` stable base tangent;
- inverse trig rules;
- min/max branch convention;
- FFUN rule classification.

`propagate`:

- residual linearity validator;
- feed-forward linearizer;
- residual transposer;
- equivalence against old RAD;
- FAD/RAD consistency for scalar examples.

`transform`:

- FIR residual transposer;
- tape inference from nonlinear uses;
- separation between primal tape/recompute requirements and adjoint
  carry/buffer requirements;
- delay/prefix carry direction;
- recursive carry matching by SYMREC/SYMREF variable, not only slot.

### 16.2 Integration tests

Add or reuse corpus cases:

- `rad_filter1.dsp`;
- `rad_filter2.dsp`;
- `rad_fir1.dsp`;
- `rad_fir2.dsp`;
- one `pow`-heavy feed-forward DSP;
- one `select2`/`min`/`max` DSP;
- one delay-only BRA DSP;
- one recursive one-pole DSP;
- one nonlinear recursive DSP.

### 16.3 Differential checks

For small seed counts:

- compare RAD gradients against FAD by running one FAD lane per seed;
- compare runtime values against central finite differences where stable;
- compare old and new RAD outputs during transition.

### 16.4 Golden checks

Before switching defaults:

- run `cargo run -p xtask -- golden-check`;
- if snapshots change, document why the generated form changed;
- do not refresh golden outputs just because the internal AD path changed
  unless generated public output legitimately changed.

## 17. Performance Guardrails

The new path must not regress common cases:

- feed-forward RAD should stay linear in the active DAG plus emitted residual
  nodes;
- shared primal subgraphs must be memoized;
- residual transposition must accumulate by `LinId`, not clone subgraphs for
  every fanout;
- tape inference must be no more conservative than current `collect_tape_needed_values`
  for B4/B5 cases;
- `pow` should compute/reuse the primal `pow(x, y)` once when needed by the
  exponent term.

Add micro or structural checks for:

- number of residual nodes for `x * y + x * z`;
- number of primal tape stores and adjoint carries for `Delay1(x) * y`;
- zero primal tape stores for pure LTI `Delay1(x)` or `a * Delay1(x)` with
  constant `a`;
- no duplicated residual for shared subgraphs.

## 18. Open Decisions Before Implementation

These require an explicit decision before the affected phase starts:

1. Whether the residual IR lives permanently in `propagate` or moves to
   `signals` once FIR lowering consumes it.
2. Whether existing FAD is refactored to use the residual builder, or whether
   the residual builder remains RAD-only until after the default switch.
3. Whether temporal linear primitives are represented in `propagate` from the
   start or added only when Phase 5 begins.
4. Whether LTI `ReverseTimeRec` is kept as a public Signal carrier optimization
   or replaced by residual-level optimization.
5. Whether a future VJP API should reuse `SigBlockReverseAD` cotangent slots or
   introduce a distinct surface form.
6. How `signal_fir` obtains the residual for a `SigBlockReverseAD` carrier once
   the old hand-written reverse walker is removed:
   - re-run linearization from the carrier body during FIR lowering;
   - store a stable residual payload in the Signal carrier;
   - move the residual IR to a shared crate and let both `propagate` and
     `transform` consume it.

Default recommendations:

- keep the residual IR private to `propagate` through Phase 3;
- do not refactor public FAD until RAD parity is stable;
- include temporal variants in the enum early but reject them in Phase 1;
- keep `ReverseTimeRec` as an optimization target, not a correctness path;
- defer public VJP design;
- for Phase 5, prefer moving only the residual data model and validator to a
  shared non-backend module; keep tape allocation and scheduling in
  `signal_fir`.

## 19. Suggested Commit Slices

1. Add this plan.
2. Add residual IR and linearity validator tests.
3. Add feed-forward linearizer for leaves, seeds, Add/Sub/Mul/Div.
4. Add unary and binary math tangent rules, including stable `pow`.
5. Add residual transposer and feed-forward parity tests.
6. Add test-only `generate_rad_signals_linearize_once`.
7. Switch feed-forward RAD default.
8. Generalize shared derivative rule table and remove formula duplication.
9. Add FIR residual transposer for non-temporal BRA formulas.
10. Add temporal linear primitives and replace delay/prefix BRA propagation.
11. Add recursive residual linearization.
12. Remove compatibility reverse propagation code.

Each implementation commit should update the daily journal and, if user-facing
support changes, `porting/faust-rs-supported-faust-subset-en.md`.

## 20. Stop Conditions

Pause implementation and ask for direction if any of these occur:

- a required rule changes public `rad(expr, seeds)` semantics;
- the residual IR needs to cross crate boundaries earlier than planned;
- FAD and old RAD disagree on an existing supported feed-forward case;
- finite-difference checks disagree with both old RAD and the new transposer;
- block-local TBPTT semantics appear insufficient for an already documented
  supported RAD corpus case;
- a required optimization would introduce backend-specific behavior into
  `propagate`.

## 21. Final Success Criteria

The migration is complete when:

- FAD derivative formulas are the source of truth for RAD;
- feed-forward RAD is implemented as linearization plus transposition;
- `BlockReverseAD` tape needs come from residual nonlinear uses;
- temporal adjoint carry needs come from temporal residual nodes, separately
  from primal tape inference;
- FIR `BlockReverseAD` local math adjoints are emitted by residual
  transposition, not by a hand-written reverse formula table;
- recursive TBPTT uses the same residual transposer;
- LTI transpose remains only an optimization or specialized lowering;
- tests cover the old bug-prone cases directly;
- documentation clearly states the block-local temporal semantics.

## 22. External Cross-Checks

The tape/time distinction above was checked against independent AD references:

- JAX documents reverse mode as transposition of the linearized JVP program and
  exposes `linear_transpose` for functions promised to be linear, avoiding the
  forward pass overhead for that case:
  <https://docs.jax.dev/en/latest/jax-primitives.html> and
  <https://docs.jax.dev/en/latest/_autosummary/jax.linear_transpose.html>.
- JAX's direct-linearize migration note states that JAX changed internal
  autodiff from a separated JVP, partial-evaluation, and transposition pipeline
  to a linearization transformation that bundles JVP and partial evaluation:
  <https://docs.jax.dev/en/latest/direct_linearize_migration.html>.
- JAX source confirms the current default path: `jax.vjp` calls
  `ad.linearize(..., is_vjp=True)`, `ad.linearize` dispatches to
  `direct_linearize` when `jax_use_direct_linearize` is enabled, and
  `backward_pass3` transposes the resulting tangent jaxpr with primitive
  transpose rules:
  <https://github.com/jax-ml/jax/blob/main/jax/_src/api.py>,
  <https://github.com/jax-ml/jax/blob/main/jax/_src/interpreters/ad.py>, and
  <https://github.com/jax-ml/jax/blob/main/jax/_src/config.py>.
- `dolfin-adjoint` states that nonlinear forward models require the forward
  solution to be available for linearization, with checkpointing as the
  storage/recomputation tradeoff:
  <https://dolfin-adjoint-doc.readthedocs.io/en/latest/documentation/checkpointing.html>.
- MITgcm's AD documentation makes the same point for time-dependent models:
  reverse mode needs the forward trajectory in reverse order when local
  derivatives depend on trajectory values; storage and recomputation are the
  two implementation strategies:
  <https://mitgcm-gf.readthedocs.io/en/latest/autodiff/autodiff.html>.
- Hogan, "Fast Reverse-Mode Automatic Differentiation using Expression
  Templates in C++" (Adept), validates the implementation distinction between
  a recorded differentiable computation and its efficient traversal. Adept uses
  expression templates to represent each mathematical expression as a
  computational graph that can be traversed in either direction, with lower
  memory and runtime overhead than older operator-overloading libraries:
  <https://www.met.reading.ac.uk/~swrhgnrj/publications/adept.pdf> and
  <https://www.met.reading.ac.uk/~swrhgnrj/new/adept/>.
- MIT 18.S096, "Forward and Reverse-Mode Automatic Differentiation", is a
  concise external reference for the basic cost model: forward mode scales with
  input dimension, reverse mode scales with output dimension, and reverse mode
  usually requires a tape/record because it runs opposite to ordinary program
  execution:
  <https://ocw.mit.edu/courses/18-s096-matrix-calculus-for-machine-learning-and-beyond-january-iap-2023/resources/mit18_s096iap23_lec08_pdf/>.
- Rufflewind, "Reverse-mode automatic differentiation: a tutorial", is useful
  for the implementation shape this plan wants to preserve: a tape/expression
  graph can be separate from the adjoint array, and the reverse pass traverses
  the tape backward while accumulating adjoints into parent nodes:
  <https://rufflewind.com/2016-12-30/reverse-mode-automatic-differentiation>.
- Bird and Polivoda, "Backpropagation Through Time For Networks With
  Long-Term Dependencies", is relevant to the TBPTT warning: truncated BPTT
  assumes short-term dependencies, while their proposed alternative uses
  discrete forward sensitivity equations and still requires Jacobian
  computation. This supports treating block-local TBPTT as an explicit semantic
  choice rather than a transparent implementation detail:
  <https://arxiv.org/abs/2103.15589>.
- Gomez, Ren, Urtasun, and Grosse, "The Reversible Residual Network:
  Backpropagation Without Storing Activations", supports the storage-policy
  side of the plan: activations can sometimes be reconstructed instead of
  stored, but only when the computation is reversible; non-reversible layers
  still require explicit storage. This is analogous to using recomputation or
  specialized structure as an alternative to unconditional `NonlinearUse`
  taping:
  <https://papers.nips.cc/paper/2017/file/f9be311e65d81a9ad8150a60844bb94c-Paper.pdf>
  and <https://arxiv.org/abs/1707.04585>.

These references support the plan's separation:

```text
primal-dependent local Jacobian -> NonlinearUse -> tape or recomputation
temporal transpose              -> adjoint carry/buffer and reverse schedule
```
