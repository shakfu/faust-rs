# Experiment: making `signal_fir/delay.rs` simpler to read without losing rigor or speed

**Date:** 2026-06-21
**Scope:** `crates/transform/src/signal_fir/delay.rs` (1671 lines), its call sites in
`crates/transform/src/signal_fir/module/` and `recursion.rs`.
**Status:** Implemented on branch `delay_rewrite1` (2026-06-21). Proposals C, B and A landed
FIR-identical (103 `transform` tests green throughout), followed by the per-strategy file split
into `signal_fir/delay/` — see the journal entry for 2026-06-21. The `file:line` references below
point at the **pre-refactor single-file `delay.rs`** and are kept as the design's "before" snapshot.
**Goal:** Restructure the delay subsystem into steps a human can read independently,
document in isolation, and recombine — while emitting **byte-identical FIR** (hence identical
generated C/WASM and identical runtime performance).
**Companion docs:** [`delay-manager-design-2026-04-06-en.md`](delay-manager-design-2026-04-06-en.md),
[`delay-strategy-abstraction-plan-2026-04-08-en.md`](delay-strategy-abstraction-plan-2026-04-08-en.md),
[`delay-merging-plan-2026-04-05-en.md`](delay-merging-plan-2026-04-05-en.md),
[`cpp-delay-analysis-parity-plan-2026-04-08-en.md`](cpp-delay-analysis-parity-plan-2026-04-08-en.md),
[`signal-to-fir-transform-analysis-2026-06-20-en.md`](signal-to-fir-transform-analysis-2026-06-20-en.md).
**French twin:** [`delay-rs-simplification-experiment-2026-06-21-fr.md`](delay-rs-simplification-experiment-2026-06-21-fr.md).

---

## 0. Where this code lives in the compilation chain

### 0.1 The pipeline above `delay.rs`

```
boxes ──► propagate ──► signals (+ UiProgram)
                            │
                            ▼
        ┌───────────────────────────────────────────────┐
        │ crates/transform  (mid-level lowering)         │
        │                                                │
        │   signal_prepare ──► signal_fir ──► FIR        │
        │     (staging)        (lowering)                │
        └───────────────────────────────────────────────┘
                            │
                            ▼
              fir ──► codegen (C / C++ / WASM / Cranelift / FBC)
```

`transform` is the layer between *propagation* (which owns the signal model and recursion as
de-Bruijn) and the *FIR backends* (which own code generation). The single public entry is
`compile_signals_to_fir_fastlane_with_ui(...)` at
[`signal_fir/mod.rs:205`](../crates/transform/src/signal_fir/mod.rs), which runs three stages:
contract gate (`planner`), staging (`signal_prepare`), and FIR emission (`module::build_module`).

`delay.rs` is the part of stage 3 that turns Faust's `@(n)` operator and the single-sample state
edges (`Delay1`, `Prefix`) into concrete ring buffers, counters, and read/write instructions.

### 0.2 What the file is responsible for

Faust's `@` maps to one of three buffer strategies, chosen by the delay amount against two
thresholds (`-mcd` default 16, `-dlt` default `u32::MAX`):

| Delay range | Strategy | Buffer | Pointer |
|-------------|----------|--------|---------|
| `[1, mcd)` | **Shift** | exact `N+1` | none — shift every sample, read `buf[N]` |
| `[mcd, dlt)` | **CircularPow2** | `next_pow2(N+1)` | shared `fIOTA`, masked index |
| `[dlt, ∞)` | **IfWrapping** | exact `N+1` | per-line `fIdx<id>`, `if`-wrap |

It also handles the **recursion+delay merge**: when a delayed signal ultimately reads from an
active recursion carrier (`Delay1^k(Proj(i, group))`), no separate buffer is allocated; the
recursion array is upsized to hold the history instead.

### 0.3 The three phases delay.rs participates in

The subsystem is **not** a single function call. It is woven into three distinct moments of a
module build, with state (`DelayManager`) carried between them:

```
PHASE 1 — PREPARE  (setup.rs::prepare_delay_lines, build.rs:152)
  delay.analyze_signals(...)   → fills rec_output_analysis  (recursion sizing metadata)
  delay.scan_signals(...)      → returns max_delays: HashMap<SigId,i32>
  for (carried, delay): ensure_delay_line(carried, delay, &mut DelayFirCtx)
                               → declares fVec*/iVec*, registers instanceClear loop,
                                 declares fIOTA or fIdx* as needed

PHASE 2 — LOWER  (core_lowering.rs::lower_fixed_delay / lower_shift_delay1)
  module.rs keeps orchestration (recursion reuse, amount eval, write dedup),
  delegates the concrete read/write to:
      emit_fixed_delay_for_line(&mut DelayLoweringCtx, &line, ...)
      emit_delay1_for_line(&mut DelayLoweringCtx, &line, ...)

PHASE 3 — SAMPLE END  (build.rs:212 / build.rs:235)
  delay.emit_sample_end_updates(store, uses_iota)
                               → fIOTA += 1, and each fIdx* wrap-advance

CROSS-CUTTING — RECURSION  (recursion.rs::ensure_recursion_array_for_group)
  reads delay.rec_output_analysis(var, index) to size recursion arrays that double
  as merged delay buffers
```

The data passed between phases is the crux: `DelayManager` owns `delay_lines`,
`rec_output_analysis`, and `scheduled_delay_writes`; the two borrow bundles `DelayFirCtx`
(8 fields, allocation-time) and `DelayLoweringCtx` (4 fields, lowering-time) carry references to
disjoint fields of `SignalToFirLower` so the manager and the rest of the lowerer can be borrowed
at the same time.

---

## 1. What `delay.rs` contains today

The file is correct, well-commented, and already factored once (see the two 2026-04 design docs).
But it packs **eight distinct concern clusters** into one 1671-line module:

| # | Cluster | Lines | Kind |
|---|---------|-------|------|
| 1 | Sizing/analysis free fns (`pow2limit_for_delay`, `*_delay_amount`, `*_max_bound`, `delay_size_for_amount`) | 220–368 | pure, stateless |
| 2 | Data types (`DelayOptions`, `DelayStrategy`, `DelayLineInfo`, `DelayAnalysisEntry`) | 129–218 | plain data |
| 3 | `GlobalCircularCursor` (the `fIOTA` service) | 370–439 | stateful-emit ZST |
| 4 | `RingDelayModel` trait + `CircularPow2Model` + `IfWrappingModel` | 441–615 | geometry |
| 5 | `DelayFirCtx` (allocation-time borrow bundle + its methods) | 617–776 | wiring |
| 6 | `DelayLoweringCtx` + `DelayStrategyEmitter` + 2 emitters + dispatch | 778–983 | lowering-emit |
| 7 | Free FIR emission helpers (`masked_delay_index`, `emit_*shift*`, `if_wrapping_*`, `bump_*`) | 985–1145 | leaf emit |
| 8 | `DelayManager` (state + 2 tree walks + selection + allocation + accessors) | 1147–1671 | orchestration |

A newcomer must hold all eight in their head at once, because the concerns are interleaved by
*phase* rather than separated by *concept*: e.g. "everything about `IfWrapping`" is spread across
clusters 2 (enum variant), 4 (`IfWrappingModel`), 6 (dispatch arm), 7 (`if_wrapping_read_index`,
`bump_if_wrapping_counter`), and 8 (selection branch in `ensure_delay_line`).

---

## 2. Why it is hard to read today (the concrete friction)

These are the specific costs the experiment should remove. Each motivates one or more proposals
in §3.

**F1 — Two near-duplicate tree walks.** `analyze_signals → analyze_node → analyze_child`
([`delay.rs:1222-1381`](../crates/transform/src/signal_fir/delay.rs)) and
`scan_signals → scan_node → scan_child` ([`delay.rs:1252-1491`](../crates/transform/src/signal_fir/delay.rs))
both traverse the prepared DAG, both call `delay_size_for_amount`, both special-case
`Delay`/`Delay1`/`Proj`, both walk list children with identical `is_list/hd/tl` boilerplate. They
differ only in *what they accumulate*: `analyze` tracks accumulated delay along the path (memoized
by `best_seen_delay` keyed on accumulated value) to size recursion carriers; `scan` records
per-carrier max owned delay (memoized by a `seen` set) for standalone lines. A reader must
diff two ~70-line traversals to see they are "the same walk, two accumulators." A *third* walk in
`recursion.rs` then consumes the first walk's output.

**F2 — One strategy concept, five scattered sites.** `DelayStrategy` (data) /
`RingDelayModel` (geometry, ring strategies only) / `DelayStrategyEmitter` (full lowering, all
three) are three abstractions for one idea. The 3-way dispatch is written twice
(`emit_fixed_delay_for_line` and `emit_delay1_for_line`,
[`delay.rs:937-978`](../crates/transform/src/signal_fir/delay.rs)). `runtime_state_for_line`
([`delay.rs:925`](../crates/transform/src/signal_fir/delay.rs)) maps strategy→runtime state with a
`debug_assert!(false)` for the `Shift` case that cannot occur.

**F3 — Impossible branches the types don't forbid.** Because `DelayRuntimeState` is shared by both
ring models, `CircularPow2Model::write_index/read_index` carry `Counter(_)` arms that are never
reached (CircularPow2 is always `GlobalIota`), and `IfWrappingModel::read_index/emit_advance` carry
`debug_assert!(false)` fallbacks for the `GlobalIota` case that cannot occur
([`delay.rs:518-614`](../crates/transform/src/signal_fir/delay.rs)). These dead arms exist purely
because the invariant "model M only ever sees state S(M)" lives in comments, not in the type.

**F4 — Selection logic split three ways inside one function.** `ensure_delay_line`
([`delay.rs:1533-1618`](../crates/transform/src/signal_fir/delay.rs)) picks the strategy in one
`if/else`, computes the size in a *second* `match` on the same strategy, and emits ancillary
declarations (`ensure_iota` / `ensure_if_wrapping_counter`) in a *third* `match` — three matches on
the same value, each a place to forget a case.

**F5 — Two borrow bundles, hand-assembled at every call site.** `DelayFirCtx` (8 fields) and
`DelayLoweringCtx` (4 fields) are rebuilt with the same struct-literal-with-split-borrow incantation
at four sites in `state.rs`/`core_lowering.rs`, each repeating the "do NOT construct via `&mut self`"
caveat. The coupling to `SignalToFirLower`'s field layout is implicit and easy to break.

**F6 — Builder boilerplate buries the arithmetic.** Every constant and binop is
`let x = { let mut b = FirBuilder::new(store); b... };`. `if_wrapping_read_index` and
`bump_if_wrapping_counter` ([`delay.rs:1068-1145`](../crates/transform/src/signal_fir/delay.rs)) are
~40 lines that encode two one-line formulas — `(counter + size − amount) wrap-if ≥ size` and
`(counter + 1 ≥ size) ? 0 : counter + 1` — which the module doc-comment already states in ASCII but
the code does not visibly match.

None of these is a bug. They are all comprehension taxes paid on every read.

---

## 3. Three independent ways to restructure

The three proposals attack **three orthogonal axes** and can be adopted one at a time or stacked:

- **A — vertical** — slice by *strategy*: one self-contained unit per delay strategy.
- **B — horizontal** — slice by *phase*: one analysis pass producing an explicit `DelayPlan`
  value, then a pure emitter that consumes it.
- **C — depth** — slice at the *leaf*: a thin arithmetic helper so index formulas read like the
  doc-comments.

```
        depth (C: legible formulas)
          ▲
          │
   ───────┼───────────────►  phase (B: plan → emit)
          │
          ▼
     strategy (A: one unit per strategy)
```

Each section gives the idea, a before/after sketch, what gets simpler, the interaction surface
("composability"), the cost, and how independent it is from the other two.

### Proposal A — Strategy as a closed object (vertical slicing)

**Idea.** Replace clusters 2/4/6/7's per-strategy fragments with **one cohesive type per strategy**
behind a single trait, so a reader interested in `IfWrapping` opens one file and reads it
top-to-bottom.

```rust
/// Everything one delay strategy must answer. No shared runtime-state enum.
pub(super) trait DelayKind {
    fn buffer_size(&self, max_delay: i32) -> Result<usize, SignalFirError>;
    fn declare_state(&self, ctx: &mut DelayDecls);          // fIOTA / fIdx* / nothing
    fn emit_read (&self, e: &mut Emit, line: &DelayLineInfo, amount: FirId, ty: FirType) -> FirId;
    fn emit_write(&self, e: &mut Emit, line: &DelayLineInfo, current: FirId);
    fn emit_advance(&self, e: &mut Emit, line: &DelayLineInfo) -> Option<FirId>;
}
```

`delay/` becomes a small directory:

```
signal_fir/delay/
  mod.rs            // re-exports, the selection fn, DelayManager
  options.rs        // DelayOptions, DelayStrategy selector
  shift.rs          // ShiftKind: size, no state, store@0 + shift loop, read buf[N]
  circular_pow2.rs  // CircularPow2Kind: pow2 size, fIOTA, masked read/write/advance
  if_wrapping.rs    // IfWrappingKind: exact size, fIdx*, if-wrap read/advance
  sizing.rs         // the pure cluster-1 free fns (unchanged)
```

Selection is one function returning the chosen kind; `DelayLineInfo` stores it (as an `enum` for
zero-cost dispatch — see migration §4). The shared `DelayRuntimeState` enum, `runtime_state_for_line`,
the duplicated `emit_*_for_line` dispatch, and the impossible `Counter(_)`/`GlobalIota` arms (F3) all
disappear, because each kind only ever touches its own counter.

**What gets simpler.**
- *To read:* one concept = one file; the three doc-comment ASCII blocks now sit beside the code
  that realizes them.
- *To document:* each file has one `//!` header; no "see also" hops across five sites (F2).
- *To extend:* a fourth strategy is a fourth file + one selector arm, nothing else.
- F3 and the double dispatch (F2) are gone by construction.

**Composability / interaction surface.** Exactly one trait with five methods. Strategies never
reference each other; their only contract is `DelayKind`. The manager↔strategy interaction is
"selector picks a kind; phases call its methods."

**Cost.** A trait + per-kind types; must keep dispatch zero-cost (use an `enum DelayKind { Shift,
CircularPow2, IfWrapping }` with a `match` shim, *not* `dyn`, to preserve monomorphization — see
§4 performance).

**Independence.** Fully independent of B and C. Touches organization of clusters 2/4/6/7; leaves
the tree walks (cluster 8) and leaf math (cluster 7 bodies) as-is.

### Proposal B — A `plan → emit` pipeline with an explicit `DelayPlan` (horizontal slicing)

**Idea.** Collapse the two tree walks (F1) into **one traversal** whose output is an inspectable,
side-effect-free value — `DelayPlan` — and make every later phase a pure reader of that value.

```rust
/// The entire delay decision, as plain data. No FIR, no FirStore.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(super) struct DelayPlan {
    /// Standalone lines to allocate: carried signal → required geometry.
    pub lines: BTreeMap<SigId, PlannedLine>,        // {max_delay, strategy, size, name}
    /// Recursion-output sizing metadata (today's rec_output_analysis).
    pub rec_outputs: BTreeMap<(u32, usize), DelayAnalysisEntry>,
}

/// One pass, no FirStore argument, returns data the caller can assert on.
pub(super) fn plan_delays(
    arena: &TreeArena,
    sig_types: &HashMap<SigId, SigType>,
    signals: &[SigId],
    options: &DelayOptions,
) -> Result<DelayPlan, SignalFirError>;
```

Then emission is split into pure consumers of the plan:

```
plan_delays(...)               // ONE walk, no FIR            ← replaces analyze_* + scan_*
  └─► DelayPlan (immutable for the rest of the build)
        ├─ declare_lines(&plan, &mut DelayDecls)              // fVec*/iVec*, clears, fIOTA/fIdx*
        ├─ emit_read/emit_write(&plan[carried], &mut Emit)    // phase 2
        ├─ emit_sample_end(&plan, &mut Emit)                  // phase 3
        └─ rec sizing reads plan.rec_outputs                  // recursion.rs
```

The unified walk carries *both* accumulators the two current walks track (path-accumulated delay for
recursion outputs **and** per-carrier owned max), so it produces both maps in one pass.

**What gets simpler.**
- *To read:* one traversal instead of two near-duplicates (kills F1); list-child boilerplate is
  written once.
- *To document:* the phase boundary is now a **type** — "`DelayPlan` is everything decided before any
  FIR exists" is a one-sentence invariant.
- *To test:* assertions move from "grep the generated FIR for `fVec42`" to
  `assert_eq!(plan.lines[&s], PlannedLine{ size: 8, strategy: CircularPow2, .. })`. The plan is
  `PartialEq` data; tests stop depending on emission.
- The immutability of `DelayPlan` during lowering makes the "decide once, then only read" rule
  (already the intent, see `delay_line_info` in `state.rs`) impossible to violate.

**Composability / interaction surface.** Phases interact through **one immutable value**. Planning
has no `FirStore`; emission never re-decides. This is the simplest possible interface between steps:
a data structure.

**Cost.** Unifying the two memo strategies into one walk needs care (the accumulated-delay memo and
the per-carrier-max memo must coexist — both are monotone, so a single node visit can update both).
`recursion.rs` switches from `delay.rec_output_analysis(var, i)` to `plan.rec_outputs[&(var,i)]`.

**Independence.** Independent of A and C. It reorganizes clusters 1/8 (analysis) and the phase
plumbing; it does not care how a strategy emits its read (that is A) or how the leaf math is spelled
(that is C). Stacks cleanly on A: `PlannedLine.strategy` becomes A's `enum DelayKind`.

### Proposal C — A thin arithmetic layer so formulas read like the docs (depth slicing)

**Idea.** Attack F6 directly: introduce a *compile-time-only* expression helper so the index math is
written the way the module doc-comment already describes it, and keep each geometry's formulas in one
glance.

```rust
// A zero-cost wrapper carrying (FirId, &mut FirStore) so operators chain.
let idx = e.iota() - amount;          //  builds Sub(load fIOTA, amount)
let masked = idx & (size - 1);        //  builds And(idx, mask)
// if-wrapping read, today 40 lines, becomes:
let raw = e.counter(name) + size - amount;
let read = raw.wrap_if_ge(size);      //  select2(raw>=size, raw-size, raw)
// counter advance:
let next = (e.counter(name) + 1).wrap_to_zero_if_ge(size);
```

Equivalently (if operator overloading is judged too magical for the codebase style), a handful of
named combinators: `e.sub`, `e.mask(size)`, `e.wrap_if_ge(raw, size)`, `e.bump_wrap(counter, size)`.
Either way the goal is that `if_wrapping_read_index` and `bump_if_wrapping_counter` shrink to ~5
self-evident lines each, visibly matching `buf[(idx + size - N) wrapped]` and
`idx = (idx+1 ≥ size) ? 0 : idx+1` from the module header.

**What gets simpler.**
- *To read:* the DSP arithmetic becomes legible and **eyeball-checkable against C++**
  `writeReadDelay`; the boilerplate-to-signal ratio drops (~40% fewer lines in cluster 7).
- *To document:* the code *is* the formula; doc-comments stop having to restate it.
- *To trust:* fewer intermediate `let`s ⇒ fewer places to transpose a `+`/`−`.

**Composability / interaction surface.** None to speak of — it is a pure leaf utility that produces
`FirId`s. It composes under A (each kind's `emit_*` uses it) and under B (emission consumers use it)
without any coupling.

**Cost.** A small new helper to learn and to prove correct *once*; must compile to the exact same
`FirBuilder` calls (golden-FIR equality, §4). Risk is concentrated and easy to gate.

**Independence.** The most independent of the three — it can land first, alone, lowest-risk, and it
makes the diffs of A and B readable.

#### Worked example — what "simpler" concretely buys (proof of concept)

The most persuasive evidence that the legibility gain is real and not cosmetic. Today
`if_wrapping_read_index` ([`delay.rs:1068`](../crates/transform/src/signal_fir/delay.rs)) spends ~42
lines of `FirBuilder` ceremony to encode a one-line formula. Below it is rendered verbatim in
structure, with `B!{…}` standing for the repeated `{ let mut b = FirBuilder::new(store); b.… }`
incantation:

```rust
// BEFORE — delay.rs:1068 (42 lines once the B!{…} blocks are expanded)
fn if_wrapping_read_index(store, counter_name, amount, size) -> FirId {
    let size_i32  = i32::try_from(size).unwrap_or(i32::MAX);
    let counter   = B!{ load_var(counter_name, Struct, Int32) };
    let size_fir  = B!{ int32(size_i32) };
    let plus_size = B!{ binop(Add, counter, size_fir, Int32) };
    let raw       = B!{ binop(Sub, plus_size, amount, Int32) };
    let cond      = B!{ binop(Ge, raw, B!{ int32(size_i32) }, Int32) };
    let adjusted  = B!{ binop(Sub, raw, B!{ int32(size_i32) }, Int32) };
    B!{ select2(cond, adjusted, raw, Int32) }
}
```

Under Proposal C the same function reads as the formula the module header already states
(`buf[(idx + size − N) wrapped]`):

```rust
// AFTER — 3 lines, identical emitted FIR
fn if_wrapping_read_index(e: &mut Emit, counter: &str, amount: FirId, size: usize) -> FirId {
    let raw = e.counter(counter) + e.int(size) - amount;  // counter + size − amount
    raw.wrap_if_ge(size)                                  // select2(raw ≥ size, raw − size, raw)
}
```

`Emit` is a thin `(FirId, &mut FirStore)` wrapper whose `+`/`-`/`&` build the same `FirBinOp` nodes
and whose `wrap_if_ge` builds the same `select2`. The golden-FIR diff (§4.2) stays empty; only the
line count — and the reader's effort to confirm the function matches C++ `writeReadDelay` — drop.
`bump_if_wrapping_counter` and `masked_delay_index` collapse the same way.

### 3.x Side-by-side

| Axis | A — strategy unit | B — plan/emit IR | C — arithmetic layer |
|------|-------------------|------------------|----------------------|
| Primary friction removed | F2, F3, F4 | F1, F5 (phase coupling) | F6 |
| Slicing direction | vertical (by concept) | horizontal (by phase) | depth (by leaf) |
| New artifact | `DelayKind` trait + 3 files | `DelayPlan` value + 1 pass | expr helper |
| Test style unlocked | per-strategy unit tests | assert-on-plan data tests | golden-FIR equality |
| Interaction surface | 1 trait, 5 methods | 1 immutable struct | none (leaf) |
| Risk | medium (dispatch) | medium (unify walks) | low (local) |
| Independence | full | full | full |
| Lines moved/removed | ~250 reorganized | ~140 deduplicated | ~80 shrunk |

**Recommended combination & order** (each step ships green on its own):
1. **C first** — lowest risk, no interface change, makes subsequent diffs legible.
2. **B second** — unify the walks behind `DelayPlan`; tests become data assertions, which de-risks A.
3. **A last** — with C's legible leaves and B's `PlannedLine.strategy`, the per-strategy files
   almost write themselves, and the impossible branches (F3) vanish.

If only one is done, do **C** (cheapest legibility win). If two, do **C + B** (kills the biggest
structural duplication, F1). All three together give the "one file per concept, one pass, formulas
that read like the spec" end state.

---

## 4. Migrating safely and testably

The non-negotiable contract: **identical emitted FIR** ⇒ identical generated C/WASM ⇒ identical
performance. Every step below is gated on that.

### 4.1 The safety net that already exists

- **71 `#[test]` functions** in `signal_fir/tests.rs`, of which **~30 are delay-specific** (shift
  d=1/2/3, circular at the `-mcd` boundary, if-wrapping at the `-dlt` boundary, variable/slider
  amounts, zero-delay passthrough, and every recursion-merge shape). These already assert on emitted
  FIR shape and on `rec_output_analysis`.
- **The impulse-tests oracle** (`tests/impulse-tests/` + `crates/impulse-runner`, see
  `project_impulse_tests_harness`): a genuine C++ 4-pass oracle, baseline **cpp 92/93**. Any delay
  regression that changes runtime samples shows up here.

### 4.2 Add one anchor before touching anything: a golden-FIR diff

Characterization-test the *output*, not just behavior. Build a small corpus DSP set covering every
strategy and the merge cases, dump the emitted FIR (the `dump_sig`/FIR-printer paths already used in
tests), and snapshot it. Refactors must produce a **byte-identical** (or AST-identical) dump. This
catches divergences the unit tests miss and turns "did performance change?" into "did the FIR
change?" — a mechanical check.

### 4.3 Generic principles

- **Interface-preserving moves.** Keep the `pub(super)` surface (`DelayManager`, `DelayFirCtx`,
  `DelayLoweringCtx`, `emit_*_for_line`, the free sizing fns) stable so the four `module/` call sites
  don't churn while internals move. Change call sites only in the dedicated step that retires an
  interface.
- **One structural change per commit.** Never combine a move with a behavior tweak; each commit
  compiles and passes `cargo test -p transform` **and** the impulse oracle.
- **Differential testing during a switch.** When introducing a parallel implementation (the new
  `plan_delays`, a new `DelayKind`), compute *both* old and new for a while and `debug_assert_eq!`
  they agree on the corpus; delete the old path only once they have agreed across the full test set.

### 4.4 Per-proposal recipe

**C (arithmetic layer) — land first.**
1. Add the helper with its own unit tests (each combinator builds the expected `FirId` shape).
2. Rewrite `masked_delay_index`, `if_wrapping_read_index`, `bump_if_wrapping_counter`, and the shift
   helpers to use it — *one helper per commit*, golden-FIR diff must stay empty.
3. No call-site changes; pure leaf swap.

**B (plan/emit) — land second.**
1. Write `plan_delays` *next to* the existing `analyze_signals`/`scan_signals`; do not remove them.
2. In `prepare_delay_lines`, call both and `debug_assert_eq!` that `DelayPlan` reproduces today's
   `max_delays` map and `rec_output_analysis` entries. Run the full suite + oracle.
3. Switch `prepare_delay_lines`, `ensure_recursion_array_for_group`, and the lowering queries to read
   `DelayPlan`; delete `analyze_*`/`scan_*` and the `rec_output_analysis`/`delay_lines` duplication.
4. Convert the ~30 delay tests that grep FIR for sizing into `assert_eq!` on `DelayPlan` where it
   reads more directly (optional, but it is the payoff).

**A (strategy units) — land last.**
1. Introduce `enum DelayKind` + the trait; implement it by *delegating to the current functions* so
   behavior is unchanged. Golden-FIR diff empty.
2. Port one strategy at a time into its own file, simplest first: **Shift → IfWrapping →
   CircularPow2**. After each, delete that strategy's arm from `emit_*_for_line` and its dead
   `DelayRuntimeState` branches (F3).
3. When all three are ported, remove `RingDelayModel`, `DelayRuntimeState`, `runtime_state_for_line`,
   and the duplicated dispatch.

### 4.5 Why performance is preserved (by construction)

- **Same FIR ⇒ same machine code.** The golden-FIR diff is the guarantee; the backends see identical
  input, so generated C/WASM/Cranelift is identical.
- **Dispatch stays static.** A uses an `enum` + `match` (or keeps the existing
  monomorphized `RingDelayStrategyEmitter<M>`), never `dyn` — no vtable in the hot emit path. Note
  that emission runs at *compile time* of the DSP, not in the audio loop, so even `dyn` would not
  touch runtime DSP speed; the `enum` choice is for zero-cost principle and inlining, not audio
  latency.
- **B is strictly less work at build time** (one traversal replaces two-plus), and produces the same
  declarations and instructions.
- **C is compile-time-only sugar** that lowers to the identical `FirBuilder` calls; `#[inline]` keeps
  it free even in the compiler binary.

---

## 5. Recommendation

The file is already correct and reasonably factored; this is a *legibility* experiment, not a bug
hunt. The highest value-per-risk path is **C → B → A**:

1. **C** buys immediate readability at near-zero risk and makes everything after it easier to review.
2. **B** removes the single biggest structural duplication (the two tree walks, F1) and turns delay
   decisions into testable data.
3. **A** then collapses the five-site strategy concept into one-file-per-strategy and deletes the
   impossible branches.

Stop after any step and the file is strictly clearer than today, with the test suite and impulse
oracle proving behavior — and the golden-FIR diff proving performance — unchanged.
