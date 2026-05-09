# RAD Block Reverse AD Signal-IR Plan

Date: 2026-05-07 (last revised 2026-05-09)

Status: design plan, expanded with implementation details

## 1. Decision

The first robust RAD model must not depend on complete LTI detection.

`faust-rs` introduces a **Signal-IR-level** block reverse-mode AD operator
(`SigBlockReverseAD`) that differentiates the same primitive surface already
supported by FAD, including recursive/time-dependent graphs, by replaying one
compute block backwards. LTI recognition and `ReverseTimeRec` transposition
remain valuable, but they become a *phase-2 optimization* rather than the
foundation of correctness.

Target layering:

```text
rad(expr, seeds)
  -> reverse_ad::generate_rad_signals
       -> Phase B/C symbolic transpose            (current, feed-forward only)
       -> SigBlockReverseAD(...)                  (NEW general fallback)
       -> ReverseTimeRec / SigIIR transpose       (LTI fast path, current)
  -> signal_prepare validation
  -> signal_fir lowering: forward tape + reverse sweep
```

## 2. Motivation

The LTI path requires several coupled pieces (strict LTI detection, affine
seed provenance through canonicalization, IIR factorization, state-space
mapping, `ReverseTimeRec` lowering). It is useful but a poor first
correctness foundation: many useful DSP graphs are recursive but not strictly
LTI (nonlinear filters, waveguides, physical models, time-varying filters,
state-variable filters, saturating feedback loops).

A tape/replay model differentiates these without recognizing a closed-form
transfer function.

## 3. Where We Stand Today (2026-05-09 audit)

Implemented in `crates/propagate/src/reverse_ad.rs`:

- DAG-shared three-pass reverse sweep (`ReverseADTransform::run`):
  postorder DFS, adjoint accumulation, seed extraction.
- FAD-aligned local rules for all phase-B/C primitives: `BinOp` (Add/Sub/Mul/
  Div/Rem and discrete arms with zero contribution), `Pow`, `Min`/`Max`,
  `Atan2`, `Fmod`, `Remainder`, full smooth unaries (`Sin`/`Cos`/`Tan`/`Exp`/
  `Log`/`Log10`/`Sqrt`/`Abs`/`Acos`/`Asin`/`Atan`), `IntCast`/`FloatCast`,
  `Select2`, `RdTbl` (read-only with finite-difference slope), unary `FFun`
  family (`tanh`/`sinh`/`cosh` and inverse trig), pass-through wrappers
  (`Attach`/`Enable`/`Control`/`Output`).
- LTI fast path (phase E1) via `recursive_projection_frontier` +
  `build_lti_recursive_adjoint_projections` + `transpose_ad::
  transpose_lti_de_bruijn_rec_with_cotangents` lowering to `ReverseTimeRec`.
  Drive- and feedback-coefficient seeds inside an LTI recursion are routed
  through `propagate_lti_drive_adjoint`. The FIR backend then emits a reverse
  sample loop (see §6).
- `SigIIR` carriers up to order 2: `propagate_iir_adjoint` rebuilds an
  equivalent de Bruijn rec group, runs the LTI bridge, and returns
  `Proj(0, ReverseTimeRec(...))`.

Rejected today with `PropagateError::RadUnsupportedNode`:

- Any `Delay1`/`Delay`/`Prefix` reached by the reverse sweep (`kind =
  "delay-or-prefix"`).
- `Proj`/`Rec` not classified as `LinearTranspose` (`kind = "recursive-*"`,
  with `BlockLinearTimeVarying` and `BpttRequired` already separated by
  `stateful_rad::classify_recursive_projection_rad_mode`).
- Direct IIR with feedback length > 2.
- Writable tables, soundfile content, opaque foreign families, representation
  casts, generators.

The `BlockLinearTimeVarying` and `BpttRequired` classification slots in
`stateful_rad.rs` already exist precisely so `BlockReverseAD` can opt-in
later: nothing else has to change there.

## 4. Signal-Level Requirement

The fallback **must** be expressed in Signal IR, not only in FIR or backend
imperative code. Otherwise non-FIR backends and analyses cannot reason about
RAD nodes, diagnostics point at generated loops rather than Faust signal
structure, and we cannot share the FAD rule surface or de Bruijn invariants.

### 4.1 Signal node shape

Add one new tag and one matcher to `crates/signals/src/lib.rs`:

```text
SIG_BLOCK_REVERSE_AD_TAG = "SIGBLOCKREVERSEAD"

children: [
  body_list,        // L list of primal signal outputs   (≥ 1 element)
  primal_count,     // Int : number of primal outputs   (= len(body_list))
  seed_list,        // L list of seed leaf SigIds       (= input bus addresses)
  cotangent_list,   // L list of cotangent SigIds       (same length as body_list,
                    //                                    1.0 entries today)
  policy,           // Int : TapeFull=0 | Checkpointed=1 | Recompute=2
]
```

Outputs of a `SigBlockReverseAD` node are addressed exclusively through
`Proj(slot, group)`:

- `Proj(0..primal_count-1, group)` → primal outputs of `body_list`,
- `Proj(primal_count + k, group)`  → per-sample gradient contribution for
  `seed_list[k]`.

This is the same projection contract as `SigRec` / `SigReverseTimeRec`, which
keeps `signal_prepare` and FIR projection lowering largely uniform.

### 4.2 SigBuilder / SigMatch

```rust
// signals::SigBuilder
pub fn block_reverse_ad(
    &mut self,
    body: &[SigId],
    seeds: &[SigId],
    cotangents: &[SigId],
    policy: BlockRevPolicy,
) -> SigId;

// signals::SigMatch
pub enum SigMatch<'a> {
    ...
    BlockReverseAD {
        body: SigId,           // L list head
        primal_count: i32,
        seeds: SigId,          // L list head
        cotangents: SigId,     // L list head
        policy: BlockRevPolicy,
    },
}
```

`BlockRevPolicy` is a small `repr(i32)` enum mirrored as an `Int` child; the
public Rust API does not need it before Phase B2 but the on-arena field is
allocated up front so we can roll out checkpointing without bumping the tag.

### 4.3 Cotangent slot

Today every primal carries an implicit `1.0` cotangent. The cotangent list is
made explicit on the node so:

- a future VJP API can populate it with user-provided seeds without any
  arena layout change;
- DAG sharing keeps the constants interned.

Phase B0 always fills `cotangents` with `1.0` constants (one per primal),
matching the existing `rad(expr, seeds)` convention `J = sum(expr_outputs)`.

## 5. Initial Scope: Same Differentiable Surface As FAD

Accepted in phase B0 (semantic guarantees, must round-trip through
`SigBlockReverseAD` body):

- numeric constants and audio inputs;
- UI controls as differentiable seeds when listed in `seed_list`;
- arithmetic `BinOp` (Add/Sub/Mul/Div/Rem) and the discrete arms (zero
  contribution, same as FAD);
- smooth unaries (`Sin`/`Cos`/`Tan`/`Exp`/`Log`/`Log10`/`Sqrt`/`Abs`/`Acos`/
  `Asin`/`Atan`), binary math (`Pow`/`Atan2`/`Fmod`/`Remainder`),
  `Min`/`Max`, casts (`IntCast`/`FloatCast`), `Select2`;
- read-only `RdTbl` (same `is_readonly_table_source` predicate already used
  by FAD/RAD);
- unary `FFun` family already in `RAD_FFUN_UNARY_NAMES`;
- pass-through wrappers (`Attach`/`Enable`/`Control`/`Output`/bargraphs);
- **`Delay1`, `Delay(c)`, `Prefix(c, x)` with constant or sample-variable
  delay**;
- **De Bruijn `Rec`/`Proj` recursion** — the whole point of this work.

Rejected/deferred in phase B0 (raises a `BlockReverseADUnsupported`
diagnostic at backend lowering time, not at propagation time):

- writable table adjoints,
- soundfile content adjoints,
- side-effectful or opaque foreign functions without derivative rules,
- `Delay(c, x)` whose interval upper bound on `c` exceeds the pinned
  minimum block size (see §5.1 below).

**Not rejected, but documented as a known bias** (see §14):

- recursive graphs whose impulse response decays slower than the block
  length. Block-local semantics deliberately resets adjoint state at every
  `compute()` call, so the gradient is truncated. This is the same
  truncation that the existing `ReverseTimeRec` lowering performs through
  `emit_reverse_time_rec_compute_resets`. We do not — and cannot in
  general — prove the impulse response is short, so we never reject; an
  optional `--rad-warn-block-truncation` advisory may surface the case
  later.

### 5.1 How the delay-bound check works in practice

The check is fully static, no runtime probe:

1. `signal_prepare` already attaches an `interval::Interval` to every
   `SigId` via `sig_types: &HashMap<SigId, SigType>`. For `Delay(c, x)`
   the relevant bound is the interval of `c`. The interval-port work
   landed in `crates/interval/` populates this for sample-variable
   delays too.
2. `build_block_reverse_ad` queries the bound when it walks the body:
   - constant `c` (`SigMatch::Int`) → known exactly;
   - sample-variable `c` → take `interval.hi().ceil() as i64`;
   - unbounded / `interval = ⊤` → refuse with
     `RadUnsupportedNode { kind: "delay-bound-unknown" }`.
3. Compare against a pinned minimum block size `BS_min` (compile-time
   default `64`, overridable by a new `--rad-min-block-size N` flag, same
   shape as the existing `--rad-horizon` reservation). If `c.hi < BS_min`,
   accept. Otherwise refuse with
   `RadUnsupportedNode { kind: "delay-too-long-for-block" }`.

The diagnostic includes the inferred `c.hi`, the pinned `BS_min`, and a
hint to either lower the delay length, raise `BS_min`, or factor the
delay out of the differentiated body.

`Delay1` and `Prefix(c0, x)` are always within range (`c = 1`) and need
no check.

## 6. Output Convention

Preserved from current `rad(expr, seeds)`:

```text
[primals..., gradient_contribution(seed_0), …, gradient_contribution(seed_{N-1})]
```

For multi-output `expr`, the implicit cotangent is `1.0` per primal so the
emitted gradient is `d sum(expr_outputs) / d seed_k`, matching today.

## 7. Block Semantics

Phase B0 commits to a *block-local* semantics:

- forward sweep runs from frame `0` to `BS-1`;
- reverse sweep runs from frame `BS-1` to `0`;
- adjoint terminal state at the end of the block is implicit zero,
  matching the existing `ReverseTimeRec` reset emitted by
  `emit_reverse_time_rec_compute_resets` in
  `crates/transform/src/signal_fir/module.rs`;
- no adjoint state is carried across blocks;
- primal DSP state (delay lines, recursion carriers) follows normal Faust
  execution semantics — only the adjoint arrays reset per block;
- gradient outputs are *per-sample contributions*, not a block-summed scalar.

Aggregation over `ma.BS` is left to Faust user code, exactly like FAD today.

## 7.1 Relation to Truncated BPTT (TBPTT)

The block-local convention is exactly **Truncated BPTT** in the sense of
Williams & Peng (1990), which parameterises reverse-through-time training
of recurrent networks by two integers:

- **k1** — how often a backward sweep is launched (every k1 forward steps);
- **k2** — how far each backward sweep reaches into the past.

Three regimes are commonly distinguished in the ML literature:

| Regime | (k1, k2) | Memory | Compute | Bias |
|--------|----------|--------|---------|------|
| **Full BPTT** | k1 = k2 = T | O(T) — full tape | O(T) | none, modulo numeric |
| **Overlapping TBPTT** | k1 < k2 | O(k2) | O((k2/k1)·T) — sweeps overlap | small, decays with k2 |
| **Non-overlapping TBPTT** | k1 = k2 = k | O(k) | O(T) | structural, at every block boundary |

Faust's `BlockReverseAD` block-local semantics in §7 corresponds to:

```text
TBPTT(k1 = BS, k2 = BS), with no adjoint state carried across blocks.
```

That is the cheapest TBPTT regime: forward primal state (delay lines,
recursion carriers) crosses the block boundary normally, but adjoint state
(`adj_carrier_*`, `adj_x[BS-1]`) is forced to zero at the start of every
`compute()`. Anything an LTI/IIR impulse response or a recursive nonlinear
loop would have contributed to the gradient *before* the current block is
truncated.

### How PyTorch and JAX handle the same question

Neither framework imposes a built-in BPTT window. Both expose primitives
and let the user choose the truncation point.

**PyTorch.** The reverse graph is whatever the autograd ops accumulated.
Three usual patterns:

1. *Manual chunking* on RNNs: process the sequence in chunks of length
   `k`, call `loss.backward()`, then `hidden = hidden.detach()` between
   chunks. The `detach()` is exactly the Faust block boundary — primal
   carry, adjoint kill. This is non-overlapping TBPTT(k, k).
2. *`torch.utils.checkpoint`*: keeps the logical graph but recomputes
   activations at backward time. Full-BPTT semantics, sub-linear memory
   (O(√T) with uniform checkpoints, O(log T) with Revolve).
3. *`torch.func.vjp` / `grad`*: materialises the full tape over the
   traced region; chunking is the user's responsibility.

**JAX.** The whole traced program is reversed by `jax.grad` / `jax.vjp`,
materialising the full tape over the trace. Three usual patterns:

1. *`jax.lax.scan`*: the RNN-like primitive. Its `vjp` is a backward
   `scan` of the same length — full BPTT over what was passed in. To do
   TBPTT, the user calls `scan` separately per chunk and applies
   `lax.stop_gradient` on the carried state between chunks.
2. *`jax.checkpoint` (a.k.a. `remat`)*: same trade as PyTorch's
   `checkpoint`, with configurable policies (uniform, dot-only, custom).
3. *Optimal checkpointing via Revolve* in libs like Diffrax/Equinox for
   long ODE/RNN traces.

The relevant comparison for `BlockReverseAD`:

| Aspect | PyTorch / JAX | Faust `BlockReverseAD` |
|---|---|---|
| Truncation granularity | user-chosen (`detach`, chunked `scan`, `stop_gradient`) | fixed by audio runtime = `count` of `compute()` |
| Default tape policy | full tape over the traced region | `TapeFull`, block-local |
| Checkpointing | `checkpoint` / `remat`, O(√T) or O(log T) | reserved by `policy ∈ {Checkpointed, Recompute}` (post-B0) |
| Adjoint carry across boundary | optional (`retain_graph=True`, fold state into graph) | **forbidden by default** (terminal-zero) |
| Truncation bias | under user control | structural, tied to `BS` |

The audio runtime forces the truncation point on us, which is why
non-overlapping TBPTT is the only sensible default. The two evolution
paths in §11.5b below relax this when the user is willing to pay for it.

## 8. Tape And Checkpointing Policy

The IR carries `policy ∈ {TapeFull, Checkpointed, Recompute}`. Phase B0
implements `TapeFull` only:

- record every active value referenced by a reverse rule for the current
  block;
- backend-allocated buffer sized `block_size × active_value_count`;
- prefer correctness and simple diagnostics over memory optimality.

Later phases can add `Checkpointed` and `Recompute` without changing the
front-end (no new propagation surface). `Revolve`-style schedules are a
long-term reference, not a phase-B0 dependency.

## 9. Why Not Implement This Only In FIR/Backend IR

Putting reverse-through-time below the signal layer would:

- hide RAD semantics from non-FIR backends and analysis passes,
- send diagnostics to generated loops rather than Faust signal structure,
- prevent reuse of FAD rule surface and de Bruijn invariants.

The backend still has to execute `BlockReverseAD`, but the compiler must
carry it as an explicit Signal IR node until lowering.

## 10. Relationship To Existing `ReverseTimeRec`

`ReverseTimeRec` becomes a fast path:

```text
BlockReverseAD  : works for any FAD-surface graph, recursive or not (B0+).
ReverseTimeRec  : kept as an LTI optimization, used only when the strict
                  LTI classifier accepts the recursion.
```

Priority inversion vs. previous LTI-centred work: correctness first
(`BlockReverseAD`), optimization second (LTI / `SigIIR` / `StateSpace` /
`ReverseTimeRec`), codegen perf later (FIR/IIR specialised loops,
checkpointing).

The current `SigIIR → StateSpace → ReverseTimeRec` work is **not** discarded.
It is the first optimization candidate once block-reverse semantics are in
place; it will be reached from a dispatch step that prefers the fast path
when classification succeeds.

## 11. Implementation Phases

### 11.1 Phase B0 — Signal carrier + minimal validation

Files touched:

| File | Change |
|------|--------|
| `crates/signals/src/lib.rs` | Add `SIG_BLOCK_REVERSE_AD_TAG`, `BlockRevPolicy`, `SigBuilder::block_reverse_ad`, `SigMatch::BlockReverseAD`, decoder arm in `match_sig`. Round-trip Rustdoc. |
| `crates/signals/tests/core_api.rs` | Round-trip + decoder test, malformed-children rejection. |
| `crates/sigtype/src/rules.rs` | Type rule: each `Proj(i, BlockReverseAD)` adopts the type of `body[i]` for `i < primal_count`, the type of `seeds[i - primal_count]` (cast to the body's real precision) otherwise. Variability is at least `Samp` for every output, since the per-sample contributions read state. |
| `crates/normalize/src/normalform.rs` | Recurse into `body`/`seeds`/`cotangents` lists and rebuild via `block_reverse_ad`. No semantic rewrites in B0. |
| `crates/transform/src/signal_prepare.rs` | Mirror the `ReverseTimeRec` arm: validate that `body` and `cotangents` have the same length, that `primal_count` matches, that every seed appears in `seed_list` exactly once, and that all children verify. |

Pass criteria:

- Rustdoc states block-local semantics, output layout, and tape policy.
- `signal_prepare` preserves the new node and validates its children.
- Unsupported backends emit `BlockReverseADUnsupported` (a fresh
  `PropagateError`-style diagnostic owned by the backend) rather than
  silently dropping gradients.
- New round-trip and validation tests pass.

### 11.2 Phase B1 — Lower `rad(...)` to `BlockReverseAD`

Files touched:

| File | Change |
|------|--------|
| `crates/propagate/src/reverse_ad.rs` | Add `lower_to_block_reverse_ad(arena, primals, seeds) -> Vec<SigId>`. Wire `generate_rad_signals` to call the symbolic transpose first, fall back to the LTI bridge (current code), and finally to `lower_to_block_reverse_ad` when both refuse. |
| `crates/propagate/src/stateful_rad.rs` | Tag `BlockLinearTimeVarying` / `BpttRequired` cases as eligible for the block fallback (keep `LinearTranspose` routed through `ReverseTimeRec` for now). |
| `crates/propagate/tests/core_api.rs` | New structural tests: a recursive `rad(...)` produces a `Proj(_, BlockReverseAD(...))` instead of `delay-or-prefix` / `recursive-linear-transpose`. |

Dispatch order (single attempt per call, deterministic):

```text
generate_rad_signals(primals, seeds):
  try ReverseADTransform::run                    // current symbolic sweep
    on RadUnsupportedNode { kind = "delay-or-prefix"
                          | "recursive-bptt-required"
                          | "recursive-block-linear-time-varying"
                          | "recursive-projection" }:
      build_block_reverse_ad(primals, seeds)
    other errors propagate unchanged              // arity, malformed FFun, etc.
```

`build_block_reverse_ad` does **not** rewrite the body: it assembles
`SigBlockReverseAD { body = primals, seeds, cotangents = [1.0; N],
policy = TapeFull }` and returns:

```text
[ Proj(0, BRAD), …, Proj(M-1, BRAD),
  Proj(M, BRAD),   …, Proj(M+N-1, BRAD) ]
```

The body must remain a closed De Bruijn term (the existing
`is_de_bruijn_closed` / `check_de_bruijn_coherence` checks at the bottom of
`generate_rad_signals` apply unchanged because the carrier is closed).

Pass criteria:

- non-recursive RAD behavior is bit-identical (same SigIds emitted);
- recursive graphs from the FAD primitive surface produce a
  `BlockReverseAD` carrier;
- existing LTI fast-path tests keep passing or are explicitly gated as
  optimizations (i.e. the LTI dispatch arm runs *before* the block
  fallback, so `lti_recursive_*` tests in `reverse_ad.rs` are unaffected).

### 11.3 Phase B2 — Reference executor + finite-difference tests

Files touched:

| File | Change |
|------|--------|
| `crates/propagate/tests/block_reverse_ad_reference.rs` | New end-to-end test harness: build a `SigBlockReverseAD` from a Faust source via the public compile API, evaluate it with a tiny tape-and-replay reference, compare against a finite-difference probe of the same primal expression. Reuses the FAD test corpus (`crates/propagate/tests/fad_*`). |
| `crates/transform/src/signal_fir/block_reverse_ad.rs` (new) | First backend-side reference lowering, behind a `#[cfg(test)]` execution path *or* an `opt_level=0` mode, gated by a runtime flag so we can turn it on in tests before C/Cranelift codegen is ready. |

The reference executor is intentionally tiny — it lives in tests and is the
oracle for the FIR lowering. It does **not** need to be efficient.

Pass criteria:

- block-local forward tape and reverse sweep agree with finite-difference
  for representative recursive (one-pole, biquad, comb) and non-recursive
  graphs on the FAD operation surface;
- diagnostics identify the first unsupported primitive inside the block
  (the reference executor walks the body once and reports
  `BlockReverseADUnsupported { node, kind }` before allocating tape).

### 11.4 Phase B3 — FIR / C / Cranelift backend lowering

Files touched:

| File | Change |
|------|--------|
| `crates/transform/src/signal_fir/module.rs` | Add `lower_block_reverse_ad`. Reuses `classify_reverse_time_outputs` infrastructure (rename to `classify_reverse_loop_outputs` and accept either `ReverseTimeRec` or `BlockReverseAD` projections). |
| `crates/transform/src/signal_fir/block_reverse_ad.rs` | Owns tape-buffer state declarations, forward sweep code emission, and reverse sweep code emission. Produces FIR statements consumed by the existing two-loop scheduler in `module.rs`. |
| `crates/transform/src/signal_fir/recursion.rs` | Extend the `ReverseTimeRec` body extraction to also accept `BlockReverseAD` bodies for adjoint state arrays. |
| `crates/transform/src/signal_fir/tests/*` | Golden FIR tests for one-pole, biquad, time-varying SVF, comb. |

#### 11.4.1 Active-value collection

A pre-pass over the body of the carrier collects:

- every `SigId` whose reverse rule needs the *primal* value (e.g. `Mul(a, b)`
  needs both, `Sin(x)` needs `x`, `Pow(x, y)` needs both, `Select2(c, x, y)`
  needs `c`),
- every recursive carrier referenced by a `Proj(_, Rec)` reachable in the
  body,
- every `Delay1`/`Delay(c)` argument.

Active values are placed in a deterministic order (postorder of the body
DAG, dedup-ed). The list length is `K`. The tape is a flat array of size
`BS × K` of the body's real precision (`Float32` or `Float64`).

#### 11.4.2 Forward loop

```text
for i in 0 .. count:
    // identical to the normal forward lowering of `body`,
    // with one extra store per active value:
    tape[i*K + k] = active_value_k(i)
    output_p(i)   = body_primal_p(i)               // for primal outputs only
    advance primal recursion / delay state
```

The forward loop reuses the existing FIR signal-to-FIR lowering for the
primal body. The only addition is one `store_table("tape", …)` per active
value, scheduled in `sample_phases.sample_end` so it happens after all
primal computations of frame `i` finish. The primal output projections are
emitted exactly like today.

#### 11.4.3 Reverse loop

```text
zero adj_carrier_*[]                              // emitted as control_statements,
                                                    same shape as
                                                    emit_reverse_time_rec_compute_resets

for i in (count-1) ..= 0:                         // simple_for_loop(reverse=true)
    seed reverse rules from cotangent_list[p] for each primal output p
    walk the body in reverse postorder:
        for each visited y with adjoint y_bar:
            reload primals from tape[i*K + k]
            emit child_bar += y_bar * d y / d child
                              using SAME local rules as ReverseADTransform
        for each Delay1/Delay(c):
            adj_x[i] += adj_y[i + 1]              // anti-causal, c may be > 1
        for each Proj(slot, Rec(body)):
            adj_state_slot[i] += adj_y[i]
            adj_branch_slot[i] += adj_state_slot[i+1]   // reverse-time recursion

    write per-sample gradient contributions:
        gradient_seed_k(i) = adj_seed_k[i]
```

Concrete FIR shape (per active value `k`):

- a struct field `tape_k : Array(real_ty, BS)` declared via
  `ensure_named_struct_var`;
- a struct field `adj_k : Array(real_ty, BS)` for adjoints that are tape-
  scoped (a recursive carrier, a `Delay1` argument, etc.);
- adjoints local to a single frame stay on the stack inside the reverse
  loop body.

Reverse-rule emission reuses the local rule table from
`reverse_ad::ReverseADTransform::propagate_adjoint`. The lowering pass
imports those rules verbatim — the rules are pure on `SigBuilder`/`FirBuilder`
and do not depend on the postorder traversal in `propagate`.

#### 11.4.4 Recursive primitives

Each de Bruijn `Rec` body inside `BlockReverseAD` produces:

- a forward primal carrier (existing `Rec` lowering); plus
- one block-sized adjoint array per recursion slot, zero-initialized at the
  start of each `compute()` reverse sweep, matching the convention of
  `emit_reverse_time_rec_compute_resets`.

The reverse rule for `Proj(slot, Rec(body))` is mechanically:

```text
adj_branch_slot[i]  = adj_state_slot[i] (incoming) + adj_proj_slot[i]
adj_state_slot[i-1] += contribution_of_branch_slot_to_state(slot, body)
```

The block adjoint arrays are kept inside the reverse loop's strict scope:
they live across iterations of the same `compute()` call but reset at the
start of every call (cf. terminal-zero block semantics of §7).

#### 11.4.5 Delay primitives

```text
y = Delay1(x):           adj_x[i]   += adj_y[i + 1]   for i < BS-1
                         adj_x[BS-1] += 0              (terminal zero)
y = Delay(c, x):         adj_x[i]   += adj_y[i + c]   for i + c < BS
                         the rest is dropped under block-local semantics
y = Prefix(c0, x):       adj_x[i]   += adj_y[i + 1]   identical to Delay1
                         adj_c0     += adj_y[0]
```

`Delay(c)` with non-constant `c` is rejected at the carrier level by the
B0/B1 classifier; B3 only has to lower constant or sample-variable `c`
where `c < BS`.

Pass criteria:

- generated FIR (and Cranelift / C lowering) matches the reference executor
  on the test corpus;
- tape allocation is deterministic from `block_size × active_value_count`;
- gradients are stable across `opt_level=0` and optimised lowering (same
  cross-check that already exists for FAD output goldens).

### 11.5b Beyond non-overlapping TBPTT (post-B4 evolution paths)

§7.1 ranks `BlockReverseAD` as TBPTT(BS, BS) non-overlapping with no
adjoint carry. Two well-understood ways to relax that, both inheritable
from the PyTorch/JAX practice surveyed in §7.1, fit the existing Signal-IR
shape without a tag rewrite:

**(a) Overlapping TBPTT(k1, k2) with k1 < k2.** Make the reverse sweep
reach `k2 - k1` frames into the *previous* block's primal state by
keeping a circular ring buffer of size `k2` for the active values. The
ring is sized statically from `--rad-min-block-size` and a new
`--rad-truncation-depth K2` flag. The forward loop writes into the ring
modulo `k2`; the reverse loop walks it backwards, blending with the
current block's tape. Memory cost is `O(k2 × K)` instead of `O(BS × K)`
when `k2 < BS`, and `O(k2 × K)` (extra ring) on top of the per-block
tape when `k2 > BS`. The adjoint is still reset at the start of the ring,
but the truncation point is now `k2` frames earlier than the block
boundary.

Plumbing: a fourth integer field `truncation_depth` is added next to
`policy` on `SigBlockReverseAD`. `0` (default) means "= BS, classic
non-overlapping". A non-zero value enables the ring buffer.

**(b) Inter-block adjoint carry.** Extend `SigBlockReverseAD` with an
optional `adj_state_carry` slot, a list of `SigId` adjoint carriers that
*do* persist across `compute()` calls — the reverse-mode analogue of the
primal carry already implemented for `SigRec`. Backend lowering then
allocates per-recursion adjoint carriers as struct fields zeroed only at
`instanceClear()`, not at every `compute()`. This corresponds to removing
the `lax.stop_gradient` between chunks in JAX, or to *not* calling
`hidden.detach()` between chunks in PyTorch.

This is exactly the contract that the previously sketched
`rad(expr, seeds, horizon)` form and the `--rad-horizon N` flag from the
April 2026 plan reserved. Reviving them here means: the Signal-IR carrier
gains an opt-in `carry: bool` flag, the reverse sweep becomes
"pseudo-full BPTT over a sliding horizon of N blocks", and the user
accepts that gradients are now implicit functions of state set in much
older `compute()` calls.

Both paths leave the B0–B3 carrier and lowering untouched — they are
strict supersets gated by new fields and flags. They are mentioned here
to keep the long-term roadmap visible; neither is in scope for B0–B4.

### 11.5 Phase B4 — LTI fast path stays as an optimization

The existing
`build_lti_recursive_adjoint_projections` /
`transpose_lti_de_bruijn_rec_with_cotangents` /
`ReverseTimeRec` chain is preserved as-is. Dispatch order in
`reverse_ad::generate_rad_signals` is:

```text
1. symbolic feed-forward sweep (today's ReverseADTransform)
2. LTI fast path (today's recursive_projection_frontier + IIR bridge)
3. SigBlockReverseAD (NEW general fallback)
```

Pass criteria:

- LTI and `BlockReverseAD` agree numerically on first- and second-order
  filters (cross-check tests reuse the FAD finite-difference oracle);
- high-order direct IIRs remain rejected by the LTI path unless factorized,
  but are now accepted by the block fallback (with a `RadUnsupportedNode`
  hint upgraded to a warning suggesting factorization for performance);
- affine seed provenance tests still apply to the LTI fast path; the block
  tape relies only on local chain rules and does not need affine
  provenance.

## 12. Diagnostics

New variants/diagnostics:

```rust
PropagateError::RadUnsupportedNode { kind: "block-tape-unsupported", … }
// emitted by build_block_reverse_ad if a child operation is outside the
// FAD primitive surface AND outside the LTI fast path
```

```rust
SignalPrepareError::Validation("BlockReverseAD body/cotangent length mismatch …")
SignalPrepareError::Validation("BlockReverseAD primal_count out of range …")
```

The backend reports `BlockReverseADUnsupported { node, kind }` if an
unsupported primitive shows up after lowering — that path must point at the
*signal* that triggered it, not at the generated FIR.

## 13. Tests

Phase B0:

- `crates/signals/tests/core_api.rs`: build, decode, list-shape round-trip.
- `crates/transform/src/signal_prepare/tests.rs`: validation success and
  the four canonical malformed shapes.

Phase B1:

- `crates/propagate/tests/core_api.rs`: structural tests proving
  `Proj(_, BlockReverseAD(...))` is produced for
  `rad(x ~ _ * 0.5, x)`, `rad(delay(x, 1), x)`, `rad(svf(...), …)`.
- Existing tests in `reverse_ad.rs` (LTI bridge) keep their assertions; we
  add a pair of negative tests proving the dispatcher prefers the LTI fast
  path over the block fallback when the classifier returns
  `LinearTranspose`.

Phase B2:

- `crates/propagate/tests/block_reverse_ad_reference.rs`:
  finite-difference oracle for one-pole, biquad, comb, time-varying SVF,
  saturating feedback (`tanh`-in-loop). Tolerance `1e-5` at f32 and `1e-9`
  at f64, same shape as FAD tests.

Phase B3:

- `crates/transform/src/signal_fir/tests/block_reverse_ad_*`: golden FIR
  for each test case + cross-check that the C/Cranelift outputs match the
  reference oracle bit-for-bit at f64, within tolerance at f32.

## 14. Risks

- **Tape size.** Active-value count can grow large for audio blocks. B0 is
  acceptable for correctness but must be measured before production
  defaults. A `--rad-tape-stats` debug flag prints `K`, `BS×K`, peak
  active-set per body so we can size checkpointing later.
- **Block-local ≠ infinite-horizon adjoints.** The terminal-zero convention
  must stay visible in docs and tests. A mid-block reset between two
  `compute()` calls is **expected** behaviour, not a bug. We do not (and
  cannot in general) detect when the recursion's impulse response is long
  enough that this truncation matters. An optional
  `--rad-warn-block-truncation` flag can surface a structural advisory
  (e.g. "this recursion has feedback magnitude ≥ 1 - 1/BS_min, gradient is
  likely truncated") but it is heuristic, not a veto.
- **Backend support is mandatory.** A signal-level node without lowering is
  only a structural placeholder; B0 ships only the carrier and validation,
  but B2 must land in the same release train as B1 to keep the dispatcher
  safe.
- **FAD-supported primitive surface vs. RAD reverse rules.** Most rules
  share the FAD chain rule, but a few (`Delay1`, `Rec`/`Proj`,
  `Prefix`) require *primal-state recording* that FAD does not need. The
  rule table in B3 documents what each reverse rule records on the tape.
- **LTI work must remain available.** It does not block the general model
  but must not be regressed; the dispatcher tests in B4 keep the contract.

## 15. Recommended Next Patch (Phase B0 only)

1. Add `SIG_BLOCK_REVERSE_AD_TAG`, `BlockRevPolicy`, `SigBuilder::
   block_reverse_ad`, `SigMatch::BlockReverseAD`, decoder arm + Rustdoc.
2. Preserve the new node through `normalform.rs` (descend, rebuild) and
   validate it in `signal_prepare.rs` (mirror the `ReverseTimeRec` arm,
   plus list-length and `primal_count` checks).
3. Add the type rule in `sigtype/rules.rs` (each output adopts the type of
   the corresponding `body`/`seed` element, variability ≥ `Samp`).
4. Add one structural propagation test proving a recursive `rad(...)`
   *can* produce the block carrier instead of `delay-or-prefix` /
   `recursive-linear-transpose`. Keep execution unsupported in this patch
   — return a clear `BlockReverseADUnsupported` diagnostic at the FIR
   stage and gate it on a feature flag so the LTI fast path remains the
   default lowering until B2.
