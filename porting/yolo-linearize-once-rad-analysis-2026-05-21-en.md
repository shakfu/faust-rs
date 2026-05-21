# "You Only Linearize Once" — Feasibility Analysis for faust-rs RAD

**Date:** 2026-05-21
**Status:** analysis + staged plan (no implementation yet)
**Paper:** Radul, Paszke, Frostig, Johnson, Maclaurin, *You Only Linearize Once:
Tangents Transpose to Gradients*, arXiv:2204.10923v2 (POPL 2023).
**Scope:** decide whether the YOLO decomposition of reverse-mode AD can replace
or simplify the current `rad(expr, seeds)` implementation in
`crates/propagate` and `crates/transform`, and, if so, propose a staged path.

---

## 1. The paper in one paragraph

YOLO decomposes reverse-mode AD into three *separate* code transforms whose
composition is reverse mode:

```text
reverse_AD  =  𝒯  ∘  𝒰  ∘  𝒥
```

- **𝒥 (forward differentiation / JVP).** The familiar, covariant forward-mode
  transform. It is *the only place derivative rules live*. It turns a primal
  program into one that computes primal + tangent together (`jax.jvp`).
- **𝒰 (unzipping).** Partial evaluation that splits the differentiated program
  into a non-linear forward phase `f.nonlin` (computes the primal and produces
  the *tape* of intermediates) and a purely-linear residual `f.lin` (consumes
  the tape, computes the directional derivative). Checkpointing is a *free
  choice* here: store an intermediate on the tape, or recompute it in `f.lin`.
- **𝒯 (transposition).** Reverses *only* the linear residual `f.lin` to get
  `f.lin^T`, which runs the derivative backward (`jax.linear_transpose`).

The enabling idea is **Linear A**, a substructurally-linear-typed intermediate
language. A `(non-linear ; linear)` semicolon splits each expression; every
linear variable must be used exactly once, up to explicit `dup` (fan-out) and
`drop` (dead). Linearity is the abstraction boundary: `𝒰` and `𝒯` know nothing
about derivatives, only about linearity. The payoff:

- **Write derivative rules once** (forward only). Reverse is derived
  mechanically. For a language with `N` primitives, `H` higher-order and `L`
  linear, you implement `N + H + L` rules instead of `2N`.
- Fan-out and dead code transpose mechanically: `dup` ↔ `+`, `drop` ↔ `0`.
- A custom-derivative author supplies only the forward rule.

Crucial limitation, stated by the authors (§4.1, §10): Linear A is **total,
first-order, straight-line — no control flow, no recursion**. Data-dependent
control and loops/recursion are explicitly future work.

### 1.1 Why the split exists: execution causality, and two senses of "reverse"

Two facts explain why reverse mode cannot be implemented as a same-direction
dual-number carrier analogous to forward mode, and why this plan factors it into
linearization, residual capture, and transposition. They are the conceptual
backbone of everything below.

**(a) Execution causality forbids an "inverse dual number."** Forward mode works
because a dual number `(v, v̇)` propagates in *execution order*: at `z = x·y` the
tangent `ż = ẋ·y + x·ẏ` reads only values already computed — the cause precedes
the effect. A naïve reverse analogue would carry `(v, v̄)`, where the cotangent
`v̄` is the sensitivity of the *final* output to `v`; but that depends on
everything `v` flows into *later* in execution. Evaluating it on the fly would
mean reading the future of the program — a causality violation. Two useful
implementation families avoid that trap:

- **closures / backpropagators** (Pearlmutter–Siskind): each op returns a recipe
  `v̄ ↦ (…)` replayed later, with captured forward values or explicit tape
  entries providing the values needed by the replay;
- **linearize once, then transpose** (YOLO, this plan): do the causal part
  *forward* with ordinary dual numbers (`𝒥`), *materialize* the resulting linear
  program (`𝒰`), then `𝒯` it. Transposition is a static program rewrite, so it is
  allowed to "run backward" — nothing is evaluated out of causal order during
  tracing.

This is the deep reason the architecture is `𝒯 ∘ 𝒰 ∘ 𝒥` and not "forward mode
with reversed dual numbers."

**(b) Two independent senses of "reverse."** Conflating them causes most of the
confusion around temporal AD:

| Sense | What is reversed | When it appears | faust-rs home |
|---|---|---|---|
| **Computer-time reverse** | graph / execution depth (edge reversal, transpose) | *every* reverse-mode AD | the transposition `𝒯` (Signal) |
| **Physical-time reverse** | sample order `n → n−1` | only with delays / feedback | reverse-time region + TBPTT (FIR scheduling) |

A purely feed-forward `rad` needs only the first (the gradient is produced at the
same sample — no physical-time reversal). Delays / recursion add the second.
JAX's `scan` transpose (`reverse = not reverse`, §12.4) is the physical-time
reversal; the per-equation `lin_eqns[::-1]` walk (§12.3) is the computer-time
reversal. The **forward-value tape (§10.6) is orthogonal to both** — it is forced
by value-dependent local Jacobians, not by either reversal. Temporal reverse may
still require reverse-loop state, carries, and block-boundary storage; those are
execution-scheduling costs, not forward-value tape. This is the *FAD_RAD et sens
du temps* pedagogical framing made precise, and it underlies the three-axes
decomposition of §10.6.

---

## 2. Current faust-rs RAD architecture

AD runs at the **Signal IR** level during `propagate`, expanding into ordinary
hash-consed `SigId` nodes. Pipeline:
`parse → boxes → eval → propagate → normalize → transform → fir → backend`.

There are, today, **one forward path and three reverse paths**:

### 2.1 Forward — `fad(expr, seed)`
`crates/propagate/src/forward_ad.rs` (~1750 lines). `ForwardADTransform` carries
a dual number `Dual { primal, tangents: [d/ds0, …] }`, memoized over the DAG,
native on de Bruijn recursion, two recursion modes (expand-after-Rec,
augmented-state Rec). **Owns a full hand-written forward rule table** (constants,
`BinOp`, transcendentals, `pow`/`min`/`max`/`atan2`, delays, recursion, tables).

### 2.2 Reverse path A — symbolic feed-forward sweep
`crates/propagate/src/reverse_ad.rs` (~1265 lines). `ReverseADTransform`:
1. postorder DFS of the active subgraph, stopping at seeds;
2. init `adjoints[primal] = 1`, walk postorder in reverse, distribute
   `y_bar` to children with local transpose rules;
3. read off `adjoints[seed]`.
Feed-forward only. `Delay1`/`Delay`/`Prefix`/`Proj`/`Rec`/`Iir` raise
`RadUnsupportedNode` and route to path B.

### 2.3 Reverse path B — block-reverse-AD (the real stateful path)
`SigBlockReverseAD` carrier (built in `reverse_ad.rs`) lowered by
`crates/transform/src/signal_fir/{block_reverse_ad.rs,module.rs}`. A
TBPTT(BS,BS) reverse-time sweep over a finite block, with an optional per-sample
forward tape. `is_trivially_reverse_evaluable` decides recompute-vs-tape;
`collect_tape_needed_values` chooses what to record. Handles delays (carry
buffers), prefix, recursion (SYMREC back-edges), full math coverage.

### 2.4 Reverse path C — dormant LTI transpose
`crates/propagate/src/transpose_ad.rs` (~1075 lines) +
`crates/propagate/src/stateful_rad.rs` classifier. Extracts the state-transition
matrix `A` from an *affine LTI* `DEBRUIJNREC` group and emits the transposed
recurrence `y_bar[n] = cotangent[n] + Aᵀ·y_bar[n+1]` wrapped in
`ReverseTimeRec`. **Not wired into `rad(...)`** — preparatory only.
Classifier lattice: `RadRecLinearity::{LinearLti, LinearTimeVarying, Nonlinear}`
and `RecRadMode::{LinearTranspose, BlockLinearTimeVarying, BpttRequired}`.

### 2.5 Shared *reverse* rule table
`crates/signals/src/ad_rules.rs`. A `RadFormulaBuilder` trait plus
`rad_unary_contribution` / `rad_binary_contributions` / `rad_binop_contributions`.
The 2026-05-17 factorization shared these **between the two reverse paths**
(symbolic sweep and BRA/FIR). It did **not** unify forward vs reverse: the
forward rules (`forward_ad.rs`) and the reverse transpose rules (`ad_rules.rs`)
are still two parallel, independently-maintained derivative tables.

---

## 3. Mapping: current code ↔ YOLO

| YOLO transform | What plays its role today | Faithful? |
|---|---|---|
| **𝒥** forward diff | `forward_ad.rs` (`fad`) | Yes — this *is* JVP. |
| **𝒰** unzip / tape | `is_trivially_reverse_evaluable` + `collect_tape_needed_values` in `block_reverse_ad.rs` (FIR level); the symbolic sweep keeps no explicit tape | Partial — done as a FIR heuristic, not a structural transform; the checkpoint knob is hard-coded. |
| **𝒯** transpose | (a) the adjoint-accumulation reverse walk in `reverse_ad.rs`; (b) the matrix `A → Aᵀ` extraction in `transpose_ad.rs`; (c) the FIR reverse sweep in `module.rs` | Partial / fragmented — three different mechanisms, none a generic structural transposer. |
| **Linear A** typing | none — Signal IR has no `(nonlin ; lin)` split, no `dup`/`drop`, no substructural typing | Absent. Linearity is implicit, recovered ad hoc per path. |

### 3.1 The key observation

**The feed-forward symbolic sweep (path A) already *is* a transposition — it
just fuses linearize + transpose and therefore needs a hand-written reverse
rule table.** Concretely, in `reverse_ad.rs::run`:

- the reverse postorder walk = `𝒯` of a linear map;
- `add_adjoint`'s summation on shared nodes = the `dup ↔ +` rule (fan-out
  transposes to sum);
- but it never materializes `f.lin`. Instead, at each node it re-derives the
  local Jacobian from the *primal* node via `ad_rules.rs`.

That fusion is *exactly why `ad_rules.rs` exists*: there is no `f.lin` program to
transpose generically, so every local transpose must be written by hand — a
second derivative table parallel to `forward_ad.rs`.

### 3.2 What is already YOLO-shaped

- **Checkpointing is already present** as the recompute-vs-tape decision in
  `collect_tape_needed_values`. YOLO frames this as the free knob in `𝒰`. The
  faust-rs version is a working heuristic, not a tunable structural choice.
- **Fan-out transposes to sum** — already implemented implicitly by the adjoint
  map. No `dup` node is needed because the hash-consed DAG *is* the dup graph
  and the accumulation map performs the sum.

---

## 4. Where YOLO fits — and where it does not

**Fits well:** the feed-forward subset (path A). It is straight-line, total,
first-order — precisely Linear A's domain. Here YOLO's promise (one rule table,
mechanical transpose) is directly realizable and would *delete* the reverse
rule table.

**Does not fit as written:** delays, `prefix`, `rec` — the stateful streaming
core of Faust, and the most valuable part of faust-rs RAD. Linear A has no
recursion or temporal operator. The paper stops here.

**But there is a clean bridge:** a Faust DSP processing a block of `N` samples is
a *straight-line* computation once unrolled over the block, with the delay line
modelled as linear array reads/writes into the tape. That is exactly the TBPTT
view path B already takes. So YOLO does give a *principled framework* for what
the BRA fallback does by hand: `𝒰` builds the per-block tape, `f.lin` is the
per-sample linear map over the unrolled block, and `𝒯` is the reverse-time
sweep. The transpose of a (linear) `Delay` is an advance; the transpose of the
linear `rec` back-edge is the reverse-time recurrence — which is what
`ReverseTimeRec` and the LTI scaffold (path C) compute by special-case algebra.

**Implication:** a single *generic structural transposer* over a linear Signal
sub-IR, scheduled forward for feed-forward and reverse-time across the block for
temporal nodes, would **subsume all three reverse paths** and the LTI matrix
extraction would become a special case (or be retired).

---

## 5. Do we need full Linear A typing?

No, not for the engineering win. The practical payoff (one derivative table)
needs only the structural invariant:

> the tangent program produced by `𝒥`/`fad` is linear in the seed tangents
> by construction.

faust-rs can rely on that invariant without a substructural type checker,
because the linear fragment is identified *by construction* (it is the
seed-tangent sub-DAG emitted by forward diff), not recovered by inference.

The full Linear A type system remains valuable for two reasons that matter to a
thesis but not to shipping code: (1) it makes transposition *provably* total and
correct in a general language, and (2) it cleanly rejects expressions with no
transpose. These are research contributions, not prerequisites for the refactor.

---

## 6. Options

### Option A — status quo, keep factoring (baseline)
Continue the 2026-05-17 trajectory: share more helpers between the two reverse
paths, leave forward and reverse rule tables separate.
- *Pro:* zero risk, incremental.
- *Con:* the core duplication (forward rules vs reverse rules) remains; three
  reverse paths remain; the "more elegant" structure the paper offers is not
  captured.

### Option B — *Linearize once* for the feed-forward subset (recommended first step)
Reorganize path A so the reverse contributions are **derived from the forward
linearization + a generic transpose**, deleting the hand-written reverse rule
table for the feed-forward case.

Mechanically:
1. Make forward diff able to emit the tangent program with the **seed tangents
   as free linear leaves** `ṡ_j` (a `LinearSeed(j)` marker), instead of
   substituting the basis value `1` immediately. The existing forward rules
   already build the correct linear combination (`x*y → x'*y + x*y'`); the only
   change is *not collapsing* the seed tangent to a constant.
2. Identify the resulting tangent DAG as the linear fragment `f.lin`: its nodes
   are `+`, `c * _` (scale by a seed-independent coefficient such as `cos(x)`),
   negation, and fan-out.
3. Implement **one generic transposer** over that fragment: reverse the edges,
   `+ → fan-out to both`, `c * _ → c * _bar` (scaling is self-transpose),
   `fan-out → sum`, seeded with the output cotangent; read seed adjoints off the
   linear leaves.

This reproduces every entry of `ad_rules.rs` as a *derived* result. Example:
`fad(sin x)` tangent is `cos(x) * ṡ`; the transposer sees `cos(x)` as a constant
coefficient and a linear `* ṡ` node, and yields `ṡ_bar += cos(x) * y_bar` —
exactly today's `Sin` rule, now derived from the forward rule.

- *Pro:* deletes the parallel reverse math table; `rad` correctness reduces to
  `fad` correctness + a ~5-rule generic transposer; squarely the paper's idea;
  bounded blast radius (feed-forward only).
- *Con:* requires the "linear seed leaf" change to forward diff and a new
  transpose pass; must prove numerical parity with the current sweep.

### Option C — unified linear sub-IR + generic transpose for *all* paths (research track)
Extend Option B's generic transposer with transpose rules for the **linear
temporal primitives** (`Delay`, the `rec` back-edge) and reverse-time block
scheduling, so it subsumes paths A, B, and C. Optionally add a Linear A-style
`(nonlin ; lin)` typed sub-IR and an explicit `𝒰` unzip with a tunable
checkpoint knob.
- *Pro:* one linearize + one transposer replaces three reverse mechanisms; the
  LTI matrix extraction retires; checkpointing becomes a real knob; strongest
  thesis story.
- *Con:* large; temporal transposition correctness is the hard part and goes
  beyond the paper; touches FIR scheduling. High risk if attempted in one shot.

---

## 7. Recommended staged plan

Stage the work so each stage lands independently and de-risks the next.

**Stage 0 — spike (read-only, ~1 day).** On 3–4 feed-forward corpus fixtures
(e.g. `rad(sin(x*y),(x,y))`), hand-trace forward-tangent-then-transpose and
confirm it reproduces the current sweep's adjoints. Pure paper exercise; no code.

**Stage 1 — generic linear transposer (Option B core).**
- Add a `LinearSeed(j)` leaf concept and a forward-diff mode that keeps seed
  tangents symbolic.
- Add `propagate::transpose` (new module): transpose the linear tangent DAG.
- Gate behind an internal flag; keep the existing sweep as the default.
- Differential test: for every existing feed-forward `rad` corpus and the
  `ad_rules` unit cases, assert the new path's adjoints match the old path's
  (structurally after CSE, or numerically via the interpreter). Use
  `faust -lang jax` + `jax.grad` as an independent ground-truth oracle (§12.6).

**Stage 2 — switch feed-forward `rad` to the derived path; delete dead rules.**
Once parity holds across the corpus, make the derived transpose the default for
feed-forward, and remove the now-unused reverse formulas from `ad_rules.rs`
(keep only what BRA/FIR still needs until Stage 3).

**Stage 3 — temporal transpose (Option C, research track, separate plan).**
Extend the transposer with `Delay`/`rec` linear transpose rules and reverse-time
scheduling; evaluate subsuming the BRA fallback and retiring `transpose_ad.rs`'s
matrix extraction. This deserves its own plan and its own validation against the
TBPTT convergence suite (`crates/compiler/tests/rad_runtime.rs`). See §10 for the
transpose identities (`z⁻ᵏ → z⁺ᵏ`, linear `rec → recᵀ`), the time-varying
coefficient tape, and the real-time scheduling constraint that bounds this stage;
and §11 for the Signal-vs-FIR layering — retiring the `SigBlockReverseAD` carrier
and the `propagate_bra_adj` sweep, and building a *generic* cross-loop value cache
to replace the AD-specific `fBraTapeN` tape.

---

## 8. Risks and non-goals

**Risks**
- *Numerical drift.* The derived transpose may produce algebraically-equal but
  bit-different expressions (e.g. operand order). Mitigate with tolerance-based
  numeric parity tests, not structural equality.
- *Coefficient capture.* The transposer must treat seed-independent
  sub-expressions (`cos(x)`) as constants and seed-dependent ones as linear; a
  misclassification silently corrupts gradients. The forward pass already tracks
  seed dependence via `SigId` equality — reuse it, don't re-derive it.
- *Stage 3 temporal correctness* is genuinely hard and unproven by the paper;
  do not let it block Stages 1–2.

**Non-goals**
- Do not adopt a full substructural type checker for the engineering refactor
  (Stages 1–2). It is optional and belongs to the research track.
- Do not change the public `rad(expr, seeds)` output layout
  (`[primals…, seed-grads…]`, implicit all-ones cotangent).
- Do not make `propagate` depend on `fir`.
- Do not touch the BRA tape/scheduling in Stages 1–2.

---

## 9. Pass criteria

Per stage:

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p signals ad_rules`
- `cargo test -p propagate` (forward + reverse)
- `cargo test -p transform`
- `cargo test -p compiler --test rad_runtime` (TBPTT convergence unaffected)

Stage 1 additionally requires the forward-tangent-transpose path to match the
existing sweep on the full feed-forward `rad` corpus before Stage 2 flips the
default.

---

## 10. Deep dive: recursion and delays

This is the part the paper does **not** cover (Linear A is straight-line, §4.1
and §10) and the part where faust-rs already does the most work. Conclusion up
front: **𝒥 → 𝒰 → 𝒯 maps cleanly onto delays and recursion under the block/TBPTT
view, and the current BRA sweep is an un-factored, hand-written instance of it.
The transpose of every *linear* temporal primitive is mechanical; what is
genuinely Faust-specific — and outside the paper — is the *scheduling* of the
backward pass under real-time causality.**

### 10.1 What 𝒥 (`fad`) produces for temporal/recursive nodes

`forward_ad.rs` already gives the JVP rules, and they are *linear in the
tangents* whenever the structural parameters (delay length, recursion topology)
are seed-independent:

| Primal node | Primal | Tangent (per lane) | Linear in tangents? |
|---|---|---|---|
| `Delay1(x)` | `delay1(x)` | `delay1(x')` | yes — pure `z⁻¹` on the tangent |
| `Delay(x, d)`, `d` const | `delay(x, d)` | `delay(x', d)` | yes — `z⁻ᵈ` on the tangent |
| `Delay(x, d)`, `d` seeded | `delay(x, d)` | `delay(x', d) − d'·delay(x − delay1(x), d)` | yes, with a primal-dependent coefficient `(x − z⁻¹x)` |
| `Prefix(x, y)` | `prefix(x, y)` | `prefix(x', y')` | yes |
| `Proj(i, REC)` | interleaved `1+N` layout | tangent lane of the **augmented** recurrence | yes when feedback is linear |

So 𝒥 already emits a linear `f.lin` for the temporal core. The only
non-linearity is in *coefficients* — `(x − delay1(x))` for a variable delay,
`cos(state[n])` for a nonlinear recurrence — which 𝒰 puts on the tape.

This table assumes the delay *length* is a constant (`z⁻ᵏ`). A data-dependent
length `d[n]` makes the primal a dynamic gather `x[n−d[n]]`, whose transpose is a
dynamic **scatter**, not a constant advance — a separate design discussed in
§11.5, not covered by the `z⁻ᵏ→z⁺ᵏ` identities below.

### 10.2 How the current reverse paths transpose them

Two distinct strategies coexist:

**(a) BRA hand-written transpose** (`module.rs::propagate_bra_adj` +
`ensure_bra_backward_sweep`). Transposes the *primal* graph node-by-node with
bespoke carry mechanics:

| Primal (forward) | Adjoint (transpose) | Mechanism |
|---|---|---|
| `Delay1`: `y[n]=x[n−1]` | `adj[x][n] += adj[y][n+1]` | scalar carry struct field (`z⁻¹ᵀ = z⁺¹`) |
| `Delay(c)`: `y[n]=x[n−c]` | `adj[x][n] += adj[y][n+c]` | circular carry buffer size `c` (`z⁻ᶜᵀ = z⁺ᶜ`) |
| `Prefix(init,x)` | `Delay1` rule + `adj[init] += adj[y][0]` | carry + frame-0 boundary `Select2` |
| `rec`: `y[n]=f(y[n−1],…)` | `adj[y[n]] = c[n] + carry_{n+1}` | TBPTT: feedback `Delay1(Proj(SYMREF))` carry pre-loaded before the reverse walk |

**(b) LTI matrix transpose** (`transpose_ad.rs`, dormant). Extracts the constant
matrix `A` from an affine LTI `DEBRUIJNREC` and emits
`y_bar[n] = c[n] + Aᵀ·y_bar[n+1]` as a `ReverseTimeRec` group. This is the only
YOLO-style "isolate the linear map, transpose the map" path, but specialized to
constant `A`.

Both confirm the transpose identities: **`z⁻ᵏ` transposes to `z⁺ᵏ`** (a delay
backward in time is an advance forward in the reverse pass), and a **linear
recurrence with matrix `A` transposes to the reverse-time recurrence with `Aᵀ`**.

### 10.3 The mapping to YOLO

Under the **block-as-straight-line (TBPTT)** view, a Faust DSP over `N` samples
is straight-line, with the delay line as indexed array reads — exactly Linear
A's domain. Then:

- **𝒥** = the fad rules in §10.1 (already a linear `f.lin`, even for temporal
  nodes).
- **𝒰** = build the block tape: record the non-linear coefficients
  (`cos(state[n])`, `(x−z⁻¹x)`) and the primal state trajectory; `f.lin` is the
  per-sample linear recurrence with those coefficients frozen.
  `collect_tape_needed_values` / `is_trivially_reverse_evaluable` *are* this
  unzip's checkpoint knob.
- **𝒯** = transpose `f.lin` mechanically: every linear primitive has a fixed
  transpose (`+`→dup, `c·_`→`c·_`, `z⁻ᵏ`→`z⁺ᵏ`, linear `rec`→`recᵀ`), and
  "forward in time" becomes "backward in time".

The current BRA sweep is precisely this, *un-factored*: `propagate_bra_adj`
hand-codes the transpose of each primitive instead of (i) materializing `f.lin`
from 𝒥 and (ii) applying a generic transposer. The delay carry logic is the
hand-written transpose of `z⁻¹`; the TBPTT recursion carry is the hand-written
transpose of the linear `rec` back-edge.

### 10.4 The hard parts — and what YOLO does *not* fix

1. **Time-varying coefficients (nonlinear recurrence).** For `state×state` or
   `sin(state)`, `f.lin`'s coefficients depend on the primal trajectory and must
   be replayed from the tape. YOLO handles this *in principle* — the residual is
   still linear, the coefficients are just non-linear tape values — but it
   requires the tape, and the tape is finite. This is exactly the
   `LinearTimeVarying` / `BpttRequired` distinction the classifier already draws.

2. **Truncation is necessary, not incidental.** TBPTT sets `y_bar[N]=0` at block
   boundaries; no adjoint crosses blocks. This is *not* a wart to be removed by
   better factoring — it is forced by **real-time streaming**: a true
   reverse-time pass needs the whole (future) signal, which a real-time block
   callback does not have. YOLO has no streaming/causality notion, so it cleanly
   gives "what the transpose is" but says nothing about "when you are allowed to
   run it backward." Truncation also has a *lower* bound: the window must span at
   least one period of the lowest frequency the gradient should perceive — too
   short (e.g. `N = 1`) makes the gradient myopic and high-variance (it cannot
   "see" resonance or inertia and chases the instantaneous sample), so the block
   size trades memory/CPU against this perceptual floor.

3. **Exact vs truncated, as a scheduling choice.** For a *stable LTI* recurrence
   the transposed recurrence is itself a stable IIR; running it as a full
   reverse-time pass gives the *exact* infinite-horizon gradient (the LTI
   scaffold's intent), strictly better than TBPTT truncation — but non-real-time.
   So transposition is mechanical; **scheduling the transposed recurrence
   (block-truncated/real-time vs. full-reverse/exact) is a separate
   Faust-specific axis** the unified design must expose, not hide.

4. **IR level.** `fad` and the LTI scaffold work on de Bruijn Signal IR
   (`DEBRUIJNREC`/`DEBRUIJNREF`/`Proj`); BRA works on post-`de_bruijn_to_sym`
   symbolic FIR (`SYMREC`/`SYMREF`). A unified transposer should transpose the
   **linear Signal sub-IR** and emit a `ReverseTimeRec`-style group
   (transpose_ad.rs's *output* shape is right; its *matrix-extraction method* is
   too narrow), letting existing reverse-time FIR scheduling run it.

### 10.5 What a unified transposer subsumes

A single "linearize once (reuse fad, symbolic seed tangents) + generic
structural transposer (incl. `z⁻ᵏ→z⁺ᵏ`, linear `rec→recᵀ`) + explicit
reverse-time scheduling" would:

- replace all three reverse paths (feed-forward sweep, BRA hand-coded sweep, LTI
  matrix extraction) with one;
- delete the bespoke delay/prefix/rec transpose code in `propagate_bra_adj`,
  since those become "transpose the linear temporal primitive emitted by 𝒥";
- make the LTI matrix extraction a *special case* (constant coefficients ⇒ empty
  tape) rather than a separate algorithm — likely retiring `transpose_ad.rs`'s
  extractor while keeping its `ReverseTimeRec` output target;
- keep the truncation/exactness tradeoff as an explicit scheduling knob, not an
  accident of which path was taken.

This is Stage 3 (Option C). It is the elegant end state, but the
time-varying-coefficient tape and the real-time scheduling constraint mean it is
strictly *more* than "apply the paper": the paper supplies the transpose,
faust-rs must still own the streaming schedule.

### 10.6 What actually forces the tape: three orthogonal axes

Reverse mode over a Faust block decomposes into **three independent concerns**.
Conflating them is the usual source of confusion; separating them is what makes
the unified design tractable.

| Axis | Triggered by | Cost / consequence | IR home |
|---|---|---|---|
| **Spatial transposition** (fan-out ↔ sum, edge reversal) | *any* reverse mode | mechanical, work-preserving | Signal (`propagate`) |
| **Reverse-time traversal** (`z⁻ᵏ→z⁺ᵏ`, linear `rec→recᵀ`, block boundary) | **delays / feedback** | reverse loop + TBPTT truncation | Signal structure (`ReverseTimeRec`) + FIR scheduling |
| **Value tape** (store forward primal values) | **non-linearity** (value-dependent local derivatives) | memory ∝ (#such values) × N | FIR cross-loop cache |

The load-bearing, often-misunderstood fact — verified against the AD literature,
not only the companion note:

> **The value tape is forced by non-linearity, not by time.** A primitive needs
> its forward operand stored *iff* its local derivative depends on that value —
> i.e. the operation is non-linear. Linear operations (`+`, `c·x` with `c`
> constant) have a *constant* Jacobian, so their transpose needs **no** stored
> forward value. Time does not *create* the tape; it multiplies its **size**
> (one slot per sample) and forces the reverse traversal.

Read "non-linearity" as **value-dependence** in the broad sense:

- `x·z`, `sin(x)`, `exp(x)`, saturations → local derivative depends on forward
  values → tape;
- an LTV coefficient *computed* by a non-linear expression → tape that
  coefficient;
- a data-dependent delay length `d[n]` (gather→scatter, §11.5) → tape the index;
- but a **pure LTI** recurrence (constant coefficients) → **no value tape** — it
  is "just" a transposed filter (Tellegen / transposed direct form) run backward
  over the block.

Two honesty caveats so this is not overstated:

- "LTI ⇒ zero memory" concerns the *value* tape only. A linear temporal system
  still needs the reverse-time *pass* and block-boundary handling (`ȳ[N]=0`);
  value-tape and reverse-pass machinery are distinct (the "double burden").
- The *theoretical* necessity is non-linearity; naïve frameworks tape every
  intermediate out of convenience. The unified model should exploit the
  linear / constant-Jacobian case to *skip* the tape — exactly what the LTI
  scaffold's constant-coefficient transpose already does.

This is why the YOLO split is the right backbone: `𝒰` puts exactly the
non-linear intermediates on the tape (the inputs `f.lin` reads), `𝒯` transposes
the linear residual with no new tape, and the LTI case degenerates to an *empty
tape* — a special case, not a separate algorithm (§10.5).

**Evidence** (full citations in §13): Hogan's *Adept* paper states that "the
linearity of algorithms means … no intermediate values need to be stored in
hand-coded adjoints"; the BPTT literature shows tape memory scales with the
number of *steps* (arXiv:2103.15589), while the LSTM *constant error carousel*
shows a constant-Jacobian path needs **no** stored activations — i.e. storage is
driven by non-linear operations, not by the recurrence / time per se.

---

## 11. Layering: what the new model does at the Signal level vs. FIR

A natural question: today, temporal/recursive reverse mode is realized in the
**Signal → FIR** pass — `propagate` only emits a `SigBlockReverseAD` *carrier*,
and `module.rs` materializes the tape arrays, the carry buffers, the backward
sweep, and the loop schedule. Does the new model do everything necessary purely
at the **Signal** level instead?

### 11.1 Today's split, and why it exists

- `propagate::reverse_ad` (Signal IR) emits, for the temporal/recursive case, an
  opaque carrier `SigBlockReverseAD(body, seeds, cotangents, policy)` via
  `build_block_reverse_ad`. It does **not** compute the reverse program.
- `transform::signal_fir` materializes it: `block_reverse_ad.rs` +
  `propagate_bra_adj` + `ensure_bra_backward_sweep` + the `bra_*_carry_vars`
  fields allocate `fBraTapeN` arrays, emit carry struct fields, run the backward
  sweep, and choose split-vs-inline loop placement.

The code gives two reasons (see the `reverse_ad.rs` module doc): (1) backend
objects such as `fBraTapeN` are not Signal nodes and cannot take part in
`normalform`/`signalPromotion`; (2) the correct loop schedule depends on the FIR
context (public backward loop vs. inline adaptive update).

Note the asymmetry: the **feed-forward** reverse sweep is *already* fully
Signal-level (it expands into ordinary `SigId` adjoint expressions); only the
**temporal/recursive** case escapes to a FIR-level carrier. The LTI scaffold is
the proof of concept that temporal reverse *can* be Signal-level: it emits
`reverse_time_rec(DEBRUIJNREC(...))` — a pure Signal graph — and FIR already
knows how to lower `ReverseTimeRec`.

### 11.2 What the new model moves to the Signal level

**The derivation and structure of reverse mode become Signal-to-Signal — but the
residual is an *open* graph, not a closed one.** Faithful to YOLO, unzipping
splits `𝒥(fad)` into `f.nonlin` (forward; produces the primal *and emits the
tape* of non-linear intermediates) and `f.lin` (reads that tape as free
non-linear inputs). Transposition reverses `f.lin → f.linᵀ`, which reads the
*same* tape. So the unified transposer produces a transposed **Signal** graph
whose:

- pointwise transposes (`+`→dup, `c·_`→`c·_`) are ordinary `SigId` arithmetic —
  already true for the feed-forward sweep;
- **time reversal** of delays/recursion (constant `z⁻ᵏ→z⁺ᵏ`, linear `rec→recᵀ`)
  is wrapped in `ReverseTimeRec`, as the LTI scaffold already does — a
  `Delay1`/`Proj` *inside* a reverse-time region is "the next sample in reverse
  time", i.e. the advance;
- non-linear **coefficient leaves** (`cos(state[n])`, `(x−z⁻¹x)`) are **not**
  recomputable subexpressions: if the coefficient reads state, re-evaluating it
  in the reverse loop reads the *wrong* sample. They are bound to the **forward
  value at the matching sample** — i.e. tape handles. The Signal level can carry
  the residual *and the binding* (which forward signal each non-linear leaf
  refers to), but that binding is a cross-loop dependency, not a free
  subexpression.

So the reverse *program structure* is a Signal artifact, and the opaque
`SigBlockReverseAD` carrier is replaced by an explicit pair — a transposed
residual graph plus the forward-value bindings of its non-linear leaves. The
residual is **not closed**: it depends on forward values that only FIR can
materialize. This is the same shape feed-forward reverse already has, except
feed-forward happens to be *closed* (no cross-loop dependency), which is exactly
why it needed no tape.

### 11.3 What necessarily stays in FIR — but stops being AD-specific

The new model does **not** eliminate FIR involvement, because three things are
intrinsically about *loops*, and loops exist only in FIR:

1. **Reverse-time region lowering.** Emitting the `n = count−1 … 0` loop for a
   `ReverseTimeRec` graph. **Already exists** in FIR for the LTI path
   (`classify_reverse_time_outputs`, `emit_reverse_time_rec_compute_resets`).
   Generic — "evaluate this recurrence backward over the block" — not AD-specific.

2. **Cross-loop value caching (the de-AD-ified tape).** A coefficient produced
   in the forward loop and consumed in the reverse-time region must be stored in
   an array indexed by sample. Today this is `fBraTapeN`, owned by AD code. In
   the new model it is a **generic** concern: "this signal is referenced from
   both a forward-time and a reverse-time region → cache it per sample." The
   *decision* of what to cache is a structural Signal-graph property (a value
   live across a time-direction boundary) and can be computed at the Signal
   level or as a generic FIR analysis; the *realization* (allocate, store, load)
   is FIR. Either way it is no longer an AD concept — it is the same mechanism
   any forward-value/reverse-consumer pair would need.

3. **Scheduling and storage realization.** Split public backward loop vs. inline
   adaptive update; reverse-loop bounds; carry-buffer reset placement; the
   TBPTT-truncation-vs-exact policy (§10.4). These are loop/storage decisions.
   The Signal level can *annotate intent* (e.g. a policy field on the
   reverse-time node), but FIR realizes it. Delay storage *inside* a reverse-time
   region is just ordinary delay-line lowering of the `Delay1`/`Delay` nodes that
   the transposer emitted — generic, already exists.

### 11.4 Net effect on layering

| Concern | Level | AD-specific? |
|---|---|---|
| Linearize (`fad`, symbolic seed tangents) | Signal | yes → lives in `propagate` |
| Transpose pointwise / fan-out | Signal | yes → `propagate` |
| Time reversal of delay/`rec` (reverse-time region) | Signal | yes → `propagate` |
| Tape-handle *bindings* (which forward value each non-linear leaf reads) | Signal | yes → `propagate` decides |
| Reverse-time loop emission | FIR | **no** — generic reverse-time lowering |
| Cross-loop value cache (tape array) | FIR | **no** — generic forward→reverse caching |
| Variable-delay scatter (transpose of dynamic gather) | FIR | **no** — generic scatter store |
| Delay storage in reverse region | FIR | **no** — generic delay lowering |
| Split/inline schedule, truncation policy | FIR | **no** — generic loop scheduling |

The *target* layering is cleaner than today: AD knowledge concentrates in
`propagate` (one Signal-to-Signal transposition that emits the residual + its
tape bindings), and FIR keeps only reusable, non-AD primitives (reverse-time
loop, cross-loop cache, scatter store, delay lines). But this is a
**destination, not a safe single step.** The `SigBlockReverseAD` carrier,
`propagate_bra_adj`, `block_reverse_ad.rs`, and the `bra_*_carry_vars` fields can
be retired *only after* their generic replacements exist and pass parity — see
§11.6.

### 11.5 Caveats and limits (why this is a destination, not a safe step)

These are the reasons `SigBlockReverseAD` cannot be removed directly — only after
generic replacements land:

- **The cross-loop cache is load-bearing, not a nicety.** It has no standalone
  FIR form today — it is exactly what the BRA mechanism encodes
  (`ensure_bra_tape_stores` / `is_trivially_reverse_evaluable` /
  `collect_tape_needed_values`). Any residual whose coefficients read state
  *requires* it for correctness (re-evaluating a stateful coefficient in the
  reverse loop reads the wrong sample); this need is dictated by non-linearity,
  not time (§10.6). The work does not vanish; it is **relocated and
  generalized**, and must be built *before* BRA is retired.
- **`ReverseTimeRec` is not yet a general reverse-time region.** It is the
  *recursive* carrier. Non-recursive anti-causal adjoints (e.g. the adjoint of a
  pure feed-forward delay line with no feedback) are reverse-time but not a
  recurrence. The unified model needs a genuine **reverse-time region** Signal
  construct, of which `ReverseTimeRec` is a special case — not the whole thing.
- **Variable / data-dependent delays are not `z⁻ᵏ→z⁺ᵏ`.** When the delay length
  `d[n]` is itself a signal, `y[n]=x[n−d[n]]` is a dynamic *gather*, whose
  transpose is a dynamic **scatter**: `adj[x][m] += Σ_{n: n−d[n]=m} adj[y][n]`.
  This needs a dedicated scatter design (write to a data-dependent index in the
  reverse loop), not the constant-shift carry buffers. The simple identity table
  in §10 does not cover it.
- **The inline adaptive schedule stays contextual / FIR.** When a RAD gradient is
  consumed inside a *forward* recursion (an in-graph learning update), there may
  be no public reverse loop at all and the sweep is emitted inline. Which
  schedule applies is a FIR/use-site decision, not something the Signal residual
  fixes.
- **Policy must migrate to an annotation.** What lives on `BlockRevPolicy`
  (e.g. `TapeFull`) must move onto the Signal-level reverse-time node so FIR can
  pick a schedule without an AD-specific carrier.

### 11.6 The position the plan adopts

Conservative and correctly sequenced:

> **Signal decides AD semantics and carries the linearized residual (plus its
> forward-value / tape bindings); FIR materializes tapes, carries, types, sample
> phases, and scheduling.**

`SigBlockReverseAD` *can* eventually be replaced — that is the right direction —
but only after all of the following exist and pass parity, as a Stage-3
prerequisite checklist:

1. a shared **linearized residual** representation (the output of `𝒥`+`𝒰`, with
   explicit tape-input leaves);
2. a general **reverse-time region** Signal construct (generalizing
   `ReverseTimeRec`);
3. a generic **cross-loop forward→reverse value cache** (generalizing
   `fBraTapeN`), including a variable-delay **scatter** design;
4. **policy annotations** on the reverse-time node (replacing `BlockRevPolicy`);
5. **parity tests** vs. the current BRA path across the feed-forward and TBPTT
   corpora *before* any retirement, using `faust -lang jax` + `jax.grad` as the
   exact, full-horizon reference gradient (§12.6).

Until then, `SigBlockReverseAD` stays as the temporal/recursive carrier. The
Stage-1/2 "linearize once" work (feed-forward only, §6 Option B) proceeds
independently and depends on none of this.

**Bottom line for this question:** the new model lets the Signal level own *all
AD semantics* — reverse mode becomes one Signal-to-Signal transposition emitting
a linearized residual plus its tape bindings. It does **not** let FIR disappear,
and it does **not** make `SigBlockReverseAD` removable in one step. FIR's role
*shrinks to generic, reusable machinery* (reverse-time region lowering,
cross-loop cache, scatter store, delay lines, scheduling), and the carrier is
retired only once those generics exist and parity holds.

---

## 12. JAX reference implementation: an existence proof for Options B/C

JAX implements reverse mode as exactly `𝒯 ∘ 𝒰 ∘ 𝒥`. It is a production existence
proof for the architecture this plan recommends — including the temporal axis.
(Source: `jax-ml/jax`, branch `main`, fetched 2026-05-21; line numbers are
snapshot-specific.)

### 12.1 The mapping

| YOLO | Public API | Internal (file:line) |
|---|---|---|
| **𝒥** (JVP) | `jax.jvp` | `ad.jvp` (`ad.py:61`), `JVPTrace` (`:599`); per-primitive rules via `defjvp`/`deflinear`/`defbilinear` (`:1121/1087/1153`) |
| **𝒰** (unzip) | *(none public)* `jax.linearize` | `ad.linearize` (`ad.py:327`); `_linearize_jaxpr` (`:216`) |
| **𝒯** (transpose) | `jax.linear_transpose` | `ad.backward_pass3` (`ad.py:381`); registry `primitive_transposes` (`:1084`) |
| **reverse AD** | `jax.vjp` / `jax.grad` | `_vjp` (`api.py:1671`) = `linearize` then `backward_pass3` (`:1731`) |

### 12.2 The composition is literally `𝒯 ∘ 𝒰 ∘ 𝒥`

In `_vjp` (`api.py`):

```python
out_primals_flat, out_known, jaxpr, residuals = ad.linearize(flat_fun, *primals_flat, is_vjp=True)  # :1679
...
ad.backward_pass3(jaxpr, True, residuals, maybe_accums, cts_flat)                                    # :1731
```

`linearize` returns the **linear** `jaxpr` (`f.lin`) plus `residuals` (the tape);
`backward_pass3` transposes it. Reverse mode is *derived*, not hand-written.

### 12.3 Facts that validate this plan

1. **Rules written once (forward only).** `defjvp`→`standard_jvp` (`:1127`) sums
   `rule(t, *primals)` over tangents; `deflinear` (`:1087`) registers a JVP *and*
   a transpose for a linear primitive; `defbilinear` (`:1153`) handles
   multiplication — JVP derived from bilinearity, transpose distributed
   (`bilinear_transpose :1171`). **There is no per-primitive "reverse formula"
   table** — only a transpose per *linear* primitive. This is exactly Option B's
   pitch for retiring `ad_rules.rs`.
2. **The tape is the partial-eval residual; `f.lin` is an open graph.** In
   `_linearize_jaxpr`, the primal jaxpr emits `*out_primals, *tangent_consts`
   (`:266`) — primal **+** residuals; the `tangent_jaxpr` (= `f.lin`, `:253`)
   reads them as consts (`convert_constvars_jaxpr :257`). Confirms §11.2 (open
   residual + tape bindings).
3. **Transpose = reverse walk over a *materialized* linear jaxpr.**
   `backward_pass3` rebuilds the env forward, collects `lin_eqns`, then iterates
   `lin_eqns[::-1]` (`:426`), applies `primitive_transposes[p]`, and
   **accumulates** into input `GradAccum`s (`:454-456`) — `accum` *is* the
   fan-out→sum (dup↔+) of §3.1.
4. **The checkpoint knob exists: `allow_fwds`/`fwds`** (`:201, 259-262`) chooses
   which primals are forwarded as residuals vs. recomputed — the free choice of
   `𝒰` (§10.5).
5. **Pure `𝒯`: `jax.linear_transpose`** (`api.py:1862`) traces the
   (promised-linear) function with all inputs UNKNOWN (`instantiate=True`), then
   `ad.backward_pass` with `UndefinedPrimal` dummies (`:1931-1932`) — no forward
   pass.

### 12.4 The temporal analogue — the part that matters most for Faust

JAX models sequential/recurrent computation with `lax.scan`, and its AD rules are
the precise analogue of faust-rs's temporal RAD:

- **`_scan_linearize` (`loops.py:802`) = `𝒰` over the block.** It calls
  `ad.linearize_jaxpr` on the scan *body* (`:814`) to split it into a primal
  body and a tangent body, then runs the **primal scan forward** (`:857`)
  producing `primals_out` and `ext_res` — the **per-step residuals stacked over
  the scan length**, i.e. the tape, whose size grows with `N`. The tangent scan
  (`f.lin` over time) consumes those residuals (`:872`). Per-binder forwarding is
  the `allow_fwds` knob (`:807`).
- **`_scan_transpose_fancy` (`loops.py:1056`) = `𝒯` over the block.** The
  transpose of a scan is **a scan run in the opposite time direction**:
  `scan_p.bind(*trans_in, reverse=not reverse, …)` (`:1110`), with the transposed
  body, accumulating carry cotangents backward (`:1114-1115`). This is exactly
  the `z⁻ᵏ→z⁺ᵏ` / linear `rec→recᵀ` reverse-time identity of §10.2–§10.3, at the
  JAX level.

So JAX's `scan` *is* the "block-as-straight-line" view of §10.3 made into a
primitive: stacked per-step residuals = the tape (∝ `N`, driven by body
non-linearity, §10.6), and `reverse=not reverse` = the reverse-time sweep.

**One decisive difference from Faust.** JAX's scan transpose buffers the **full**
length `N` (all residuals stacked) — it is the *exact, full-horizon, offline*
end of the truncation/exactness axis (§10.4). faust-rs cannot do that in a
real-time block callback: the BRA fallback is the **truncated (TBPTT), real-time**
version of the *same* mechanism. JAX therefore validates the transpose
machinery, but the real-time truncation remains a Faust-specific obligation the
paper and JAX do not address.

### 12.5 Net delta for faust-rs

JAX confirms the target is real and standard. Two gaps remain, both already named
in this plan:

1. **A materialized linear sub-IR.** JAX transposes a jaxpr (ANF, explicit
   `eqns`) in reverse; faust-rs has a hash-consed Signal DAG and must either
   materialize a linear Signal residual (the §11.6 "linearized residual") or
   transpose the DAG via the adjoint map (today's feed-forward sweep).
2. **Real-time truncation.** JAX has no streaming/causality constraint; the
   TBPTT block boundary (§10.4) is faust-rs's own.

Files inspected: `jax/_src/interpreters/ad.py`,
`jax/_src/interpreters/partial_eval.py`, `jax/_src/api.py`,
`jax/_src/lax/control_flow/loops.py`.

### 12.6 The Faust JAX backend: a reference oracle for native RAD

The Faust C++ compiler ships a JAX backend (`faust -lang jax foo.dsp`, verified
on v2.85.5). It does **not** differentiate anything: it *exports* the Signal
graph as a Flax `nn.Module` whose per-sample `tick(state, inputs) → (state,
output)` is exactly a `lax.scan` body — recursive state and delay lines live in
the carry, UI parameters live in `state` (so they are differentiable leaves), and
non-linearities are plain `jnp.*` calls. AD is then **delegated to JAX**
(`jax.grad`/`jvp`/`vjp`): the exact §12.1–§12.4 machinery, including the scan
transpose (`reverse = not reverse`).

So `faust -lang jax` + `jax.grad` is the **delegated** path to differentiable
Faust; `rad`/`fad` (this plan) is the **native** path:

| | `-lang jax` + `jax.grad` (delegated) | `rad`/`fad` native (this plan) |
|---|---|---|
| Who differentiates | JAX (Python/XLA), at run time | the compiler, at compile time |
| AD machinery | the §12 YOLO implementation | built in faust-rs |
| Output | Flax module; needs the JAX/XLA runtime | C++/Rust/…; standalone, embeddable |
| Real-time | no — offline / training / GPU-TPU | yes — block callback, no Python |
| Horizon | full scan: **exact**, grows with length | **TBPTT** block truncation |
| Use case | offline DDSP parameter training | real-time, in-graph adaptive DSP |

Three consequences for this plan:

1. **Empirical validation of §12.** The Faust→JAX→`jax.grad` path runs the YOLO
   decomposition on real Faust DSPs — a Faust-specific instance of the §12
   existence proof.
2. **A parity oracle for native RAD.** JAX's full scan transpose yields the
   *exact, full-horizon* gradient. For any DSP, `faust -lang jax` + `jax.grad`
   gives a ground-truth gradient to validate (a) the feed-forward derived
   transpose (Stage 1) and, crucially, (b) the error of the **TBPTT-truncated**
   BRA versus the exact gradient (§10.4). This is the recommended source of
   reference gradients for the parity tests (§7 Stage 1, §11.6).
3. **It clarifies — not removes — the plan's niche.** The JAX backend cannot emit
   a real-time, Python-free, embeddable artifact, nor do *in-graph adaptive* DSP
   (a gradient consumed inside a forward recursion, §11.5). Native RAD targets
   exactly that niche. They are **complementary**: train/prototype parameters in
   JAX, deploy the tuned DSP through the native real-time backend.

Verified: the generated code *shape* (Flax module, scan-body `tick`, params in
`state`, non-linearity as `jnp` calls) on faust 2.85.5; `jax.grad` itself was not
executed here, but this is the canonical differentiable setup and §12 covers the
rest.

---

## 13. Bottom line

The current implementation already realizes the *spirit* of YOLO informally —
`fad` is `𝒥`, the tape heuristic is `𝒰`'s checkpoint knob, the adjoint sweep is
a fused `𝒯` — but in a fragmented form: forward and reverse rules are duplicated,
and three distinct reverse paths coexist. The single most valuable, tractable
slice of the paper for faust-rs is **"linearize once"**: derive the reverse
contributions from the forward rules plus one generic linear transposer
(Option B), which deletes the parallel reverse rule table for the feed-forward
subset. The fully unified, temporal-capable transposer (Option C) is elegant and
thesis-worthy but extends beyond the paper and should be a separate, later track.
Full Linear A *typing* is not required for the engineering win, only for the
formal correctness story.

---

## 14. References

All URLs below were verified via web search on 2026-05-21.

**Primary — the decomposition this analysis evaluates**

- A. Radul, A. Paszke, R. Frostig, M. J. Johnson, D. Maclaurin, *You Only
  Linearize Once: Tangents Transpose to Gradients*, POPL 2023, arXiv:2204.10923.
  <https://arxiv.org/abs/2204.10923>

**Reverse-mode AD theory and the "tape ← non-linearity" principle (§10.6)**

- R. J. Hogan, *Fast Reverse-Mode Automatic Differentiation using Expression
  Templates in C++* (Adept) — "the linearity of algorithms means … no
  intermediate values need to be stored in hand-coded adjoints".
  <https://www.met.reading.ac.uk/~swrhgnrj/publications/adept.pdf>
- A. G. Baydin, B. A. Pearlmutter, A. A. Radul, J. M. Siskind, *Automatic
  Differentiation in Machine Learning: a Survey*, JMLR 18 (2018), arXiv:1502.05767
  — note Radul is also a YOLO author, and Pearlmutter/Siskind are the
  "backpropagator / closures" authors the companion PDF cites.
  <https://arxiv.org/abs/1502.05767>
- C. C. Margossian, *A Review of Automatic Differentiation and its Efficient
  Implementation*, 2019, arXiv:1811.05031. <https://arxiv.org/abs/1811.05031>
- MIT 18.S096 *Matrix Calculus for Machine Learning and Beyond*, Lecture 8 —
  *Forward and Reverse-Mode Automatic Differentiation*.
  <https://ocw.mit.edu/courses/18-s096-matrix-calculus-for-machine-learning-and-beyond-january-iap-2023/mit18_s096iap23_lec08.pdf>
- Rufflewind, *Reverse-mode automatic differentiation: a tutorial*, 2016.
  <https://rufflewind.com/2016-12-30/reverse-mode-automatic-differentiation>
- CMU 10-605, *Automatic Reverse-Mode Differentiation: Lecture Notes*.
  <https://www.cs.cmu.edu/~wcohen/10-605/notes/autodiff.pdf>

**Backprop ↔ circuit transposition (the core analogy of §3.1, §10.2)**

- C. Olah, *Calculus on Computational Graphs: Backpropagation*, 2015 — fan-out
  becomes a sum under reverse-mode. <https://colah.github.io/posts/2015-08-Backprop/>
- J. O. Smith III, *Transposed Direct-Forms*, in *Introduction to Digital Filters
  with Audio Applications* (CCRMA) — flow-graph reversal turns branch-points into
  summers and summers into branch-points, preserving the transfer function; the
  classical DSP statement of the same transposition RAD performs.
  <https://ccrma.stanford.edu/~jos/fp/Transposed_Direct_Forms.html>

**AD systems and differentiable DSP (context)**

- *The Autodiff Cookbook* — JAX documentation. JVP = forward (push-forward),
  VJP = reverse (pull-back = Jacobian-transpose-vector product): the 𝒥/𝒯
  vocabulary of YOLO. <https://docs.jax.dev/en/latest/notebooks/autodiff_cookbook.html>
- W. S. Moses, V. Churavy, *Instead of Rewriting Foreign Code for Machine
  Learning, Automatically Synthesize Fast Gradients* (Enzyme), NeurIPS 2020,
  arXiv:2010.01709 — LLVM/MLIR-level AD that builds the tape after optimization
  and differentiates Rust, C++, Julia, …  <https://arxiv.org/abs/2010.01709>
- J. Engel, L. Hantrakul, C. Gu, A. Roberts, *DDSP: Differentiable Digital Signal
  Processing*, ICLR 2020, arXiv:2001.04643 — the differentiable-DSP application
  context. <https://arxiv.org/abs/2001.04643>

**Temporal / BPTT (tape size scales with time; storage driven by non-linearity)**

- *Backpropagation Through Time For Networks With Long-Term Dependencies*, 2021,
  arXiv:2103.15589. <https://arxiv.org/abs/2103.15589>
- A. N. Gomez, M. Ren, R. Urtasun, R. B. Grosse, *The Reversible Residual
  Network: Backpropagation Without Storing Activations*, NeurIPS 2017.
  <https://papers.neurips.cc/paper/6816-the-reversible-residual-network-backpropagation-without-storing-activations.pdf>

**JAX reference implementation of `𝒯 ∘ 𝒰 ∘ 𝒥` (§12; `jax-ml/jax`, branch `main`)**

- `jax/_src/interpreters/ad.py` — `jvp` (𝒥), `linearize`/`_linearize_jaxpr` (𝒰),
  `backward_pass3` and `primitive_transposes` (𝒯).
  <https://github.com/jax-ml/jax/blob/main/jax/_src/interpreters/ad.py>
- `jax/_src/interpreters/partial_eval.py` — the known/unknown partial-evaluation
  split used by `linearize`.
  <https://github.com/jax-ml/jax/blob/main/jax/_src/interpreters/partial_eval.py>
- `jax/_src/api.py` — public `jvp`, `linearize`, `vjp` (= linearize + transpose),
  `linear_transpose`.
  <https://github.com/jax-ml/jax/blob/main/jax/_src/api.py>
- `jax/_src/lax/control_flow/loops.py` — `_scan_linearize` (block unzip) and
  `_scan_transpose_fancy` (`reverse=not reverse`: the reverse-time analogue).
  <https://github.com/jax-ml/jax/blob/main/jax/_src/lax/control_flow/loops.py>
