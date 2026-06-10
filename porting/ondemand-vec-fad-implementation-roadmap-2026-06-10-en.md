# Clock Domains, FAD/RAD, and Vector Mode: Consolidated Implementation Roadmap

Date: 2026-06-10

Status: proposed — living document; tick the checkboxes as work lands and
record deviations in the journal.

This roadmap merges into **one ordered sequence** every implementation step
defined by the three analysis documents:

- the **base plan**:
  [ondemand-clock-domains-analysis-port-plan-2026-06-10-en.md](ondemand-clock-domains-analysis-port-plan-2026-06-10-en.md)
  — C++ clock-domain analysis and the 8-step port plan (plan §7);
- the **cohabitation doc**:
  [ondemand-fad-rad-cohabitation-2026-06-10-en.md](ondemand-fad-rad-cohabitation-2026-06-10-en.md)
  — FAD/RAD × clock domains (phases A/B/C, cohabitation §8);
- the **vector doc**:
  [vector-mode-analysis-port-plan-2026-06-10-en.md](vector-mode-analysis-port-plan-2026-06-10-en.md)
  — `-vec` base port (V1–V6) and composition with domains (D1/D2,
  vector doc §5–§6).

Cross-references use the family conventions: **plan §N**,
**cohabitation §N**, **vector doc §N**. The C++ parity reference is pinned
at branch `master-dev-ocpp-od-fir-2-FIR19`, commit `8eebea429`; re-syncing
to a newer commit is a deliberate, journaled decision (plan §8 item 1).

Size legend: **S** = one contained change set; **M** = a few coherent PRs;
**L** = multi-PR architectural effort.

## 1. Ordering rationale

1. **The correctness cliff comes first.** The moment `signal_prepare`
   stops rejecting ondemand programs, `fad` across a domain boundary would
   compile with *silently zero* gradients (cohabitation §4). The loud FAD
   diagnostic must therefore land **in the same change set** as the
   `signal_prepare` fix — phase P0 is indivisible.
2. **Analyses before code generation.** Clock-environment inference and
   the hierarchical schedule (P1) are pure, independently testable
   functions of the signal graph; code generation (P3) consumes their
   output. Building codegen first would mean guessing their interface.
3. **One region abstraction, two consumers.** Scalar guarded blocks (P3)
   and the vector loop DAG (P6) restructure the same `signal_fir` compute
   assembly and share one CSE invariant — *never hoist a value across a
   region boundary* (plan §7 Step 4; vector doc §6). P2 builds that shared
   infrastructure once, as a behavior-preserving refactor, before either
   consumer exists.
4. **Research value before performance.** The clock-domain track through
   FAD Phases A/B (P3–P5) unlocks the control-rate in-graph learning use
   cases (cohabitation §2) that motivated the whole effort. Vector mode
   (P6–P7) is a performance feature with no semantic urgency; a second
   track can run it in parallel starting after P2.
5. **RAD last.** The clock-aware reverse tape (P8) is the hardest piece;
   its semantics should be validated against proven FAD forward behavior
   first (cohabitation §7).

## 2. Phase overview and dependencies

| # | Phase | Size | Depends on | Source sections |
|---|---|---|---|---|
| P0 | Guards & groundwork | S–M | — | plan §7 Step 1; cohabitation §8 item 1 |
| P1 | Clock-domain analyses | M | P0 | plan §7 Steps 2–3 |
| P2 | Compute-region infrastructure | M–L | — (design only reads P1/P6 specs) | plan §7 Step 4; vector doc §4/§6 |
| P3 | Scalar OD/US/DS lowering + backends | L | P1, P2 | plan §7 Steps 5–8 |
| P4 | FAD Phase A (inside-domain) | S–M | P3 | cohabitation §8 item 2 |
| P5 | FAD Phase B (block augmentation) | M | P4 | cohabitation §6 |
| P6 | Vector mode base (V1–V6) | L | P2 | vector doc §5 |
| P7 | D1 scalar islands (`-vec` × domains) | M | P3, P6 | vector doc §6 |
| P8 | RAD Phase C (clock-aware tape) | L | P3–P5 | cohabitation §7 |
| P9 | Optimizations (optional) | L | P7 (D2), P8 (LPTV) | vector doc §6; cohabitation §7 |

```
P0 ──→ P1 ──┐
            ├──→ P3 ──→ P4 ──→ P5 ──┐
P2 ─────────┤                       ├──→ P8 ──→ P9 (LPTV transpose)
            └──→ P6 ──┐             │
                      ├──→ P7 ──────┴──→ P9 (D2, hoisting, -omp/-sch)
P3 (from above) ──────┘
```

Parallel-track note: P2 and P6 touch only `signal_fir`/`fir`/`codegen`;
P0/P1 touch only `propagate`/`transform` analyses. If two work streams are
available, run {P0, P1} ∥ {P2}, then {P3, P4, P5} ∥ {P6}, joining at P7.
With a single stream, follow the numeric order.

## 3. P0 — Guards & groundwork (S–M)

Goal: ondemand programs flow through propagation and `signal_prepare`;
every unsupported downstream path fails with a structured `FRS-` code; no
silent wrong-gradient state is reachable. **P0.1–P0.4 land together.**

### P0.1 `signal_prepare` survival of clocked nodes

- [ ] Treat the clock-env child of `Clocked(clkenv, y)` as an opaque
      annotation in `verify_prepared_signal`
      (`crates/transform/src/signal_prepare.rs:560` area) and in every
      other traversal (simplification, occurrences, CSE) — never visit it
      as a signal (plan §6.3).
- [ ] Add the missing match arms for
      `Seq`/`TempVar`/`PermVar`/`ZeroPad`/`OD`/`US`/`DS`.
- [ ] `signal_fir` rejects OD/US/DS nodes with a structured
      `FRS-SFIR` "ondemand not lowered yet" error (clean failure until
      P3), never a panic or `FRS-SFIR-0004`.
- [ ] Pipeline tests (`signal_pipeline.rs` style) for one `ondemand`, one
      `upsampling`, one `downsampling` fixture:
      `process = (button("gate"), _) : ondemand(*(2));` et al. pass
      `signal_prepare`.

### P0.2 `ClockDomain` side-table and instance uniqueness

- [ ] Replace the cons-tuple clock-env identity with an arena
      `ClockDomain { parent: ClockDomainId, kind: Od|Us|Ds, clock: SigId,
      instance: UniquenessToken }` (plan §5.3), fixing the nil
      `slotenv`/`path` in `make_clock_env`
      (`crates/propagate/src/engine.rs:1086-1098`).
- [ ] Regression test: two structurally identical ondemand instances of
      the same circuit in different contexts get **distinct** domain ids
      (the C++ de Bruijn collision, plan §3.4).

### P0.3 Propagate memoization key audit

- [ ] `propagate_in_slot_env` cache key includes the clock env; test:
      the same box propagated under two domains yields distinct,
      correctly-clocked signals.

### P0.4 Loud AD diagnostics at domain boundaries

- [ ] Replace the silent `zero_tangent` catch-all
      (`crates/propagate/src/forward_ad.rs:1075`) with a structured
      `FRS-PROP` error for `Seq`/`Clocked`/`TempVar`/`PermVar`/`ZeroPad`/
      `OD`/`US`/`DS`: *"fad cannot yet differentiate across an
      ondemand/upsampling/downsampling boundary"*.
- [ ] Improve the RAD rejection (`reverse_ad.rs:322-334`) to name the
      construct instead of kind `"other"`.
- [ ] Snapshot tests (`diagnostic_errors.rs` style) for fad-around,
      fad-inside-with-crossing-seed, rad-around — the cohabitation §4
      table becomes the expected-output fixture.

**Exit criteria**: the four cohabitation §4 rows produce, respectively:
clean compile-to-`signal_fir`-rejection, `FRS-PROP` boundary error,
`FRS-PROP` boundary error, named RAD rejection. No path produces zero
tangents silently.

## 4. P1 — Clock-domain analyses (M)

### P1.1 Clock environment inference (plan §7 Step 2, §3.6, §4.1)

- [ ] New module (e.g. `crates/transform/src/clk_env.rs` or a dedicated
      crate): `is_ancestor_clk_env`, `max_clk_env` (structured error
      naming the two incomparable domains *and* the offending signal),
      `collect_rec_groups`, `infer_clk_env_with_hypothesis` implementing
      the rules — keep the C++ rule names `R_PROJ`, `R_CLOCKED`, `R_CD`,
      `R_SEQ`, table rules, `R_COMPOSITE` in comments for parity audits.
- [ ] `find_fixpoint`: Jacobi-style Kleene iteration from `nil`,
      iteration bound, deterministic.
- [ ] Entry point `annotate(outputs) -> HashMap<SigId, ClockDomainId>` —
      side map, no tree-property mutation (faust-rs house style).
- [ ] One unit test per rule, a fixpoint-convergence test (recursion
      spanning a boundary), and the incomparable-domain diagnostic test.

### P1.2 Hierarchical dependency graph + schedule (plan §7 Step 3, §3.7, §4.2)

- [ ] `Hgraph { out_sigs, controls: Digraph, siggraph: HashMap<SigId,
      Digraph> }` with `needs_subgraph` (OD/US/DS keyed subgraphs, clock
      kept outside), `is_external` (strict-ancestor signals →
      parent-graph edge / `controls`), `get_signal_dependencies`
      (immediate vs delayed split; `Seq(od, y)` depends only on `od`).
- [ ] One deterministic depth-first toposort (no strategy plug-in yet).
- [ ] Port `auditHgraph` as debug assertions (partition property: every
      signal in exactly one graph).
- [ ] Schedule snapshot tests on the plan §2.4 fixtures.

**Exit criteria**: for each corpus fixture, the (domains, Hsched) pair is
golden-snapshotted and stable; incomparable-domain programs fail with the
named diagnostic.

## 5. P2 — Compute-region infrastructure in `signal_fir` (M–L)

The architectural step shared by scalar blocks (P3) and vector loops (P6).

### P2.1 Region model design note

- [ ] Short design note (porting/ or module rustdoc) defining the
      `Region` tree: scalar mode = `SampleLoop ⊃ GuardedBlock(OD/US/DS,
      nested)`; vector mode = `ChunkLoop ⊃ {LoopNode | Island ⊃
      GuardedBlock}`. One visibility rule for CSE/occurrences/placement:
      *a value computed in region R is reusable only in R and its
      descendants; cross-region reuse goes through named storage*
      (locals, struct fields, chunk buffers).
- [ ] Record the FIR-vocabulary decision: reuse the existing generic
      `If`/`SimpleForLoop`/`Block` statements (vector doc §4 finding)
      vs. dedicated block nodes (plan §7 Step 4) — default to reuse
      unless checker/inliner constraints say otherwise.

### P2.2 Behavior-preserving refactor of compute assembly

- [ ] Replace the flat `sample_phases` accumulator and per-bucket
      CSE/refcount in
      `crates/transform/src/signal_fir/module/build.rs`,
      `cse.rs`, `placement.rs` with the region tree, instantiated with
      exactly one `SampleLoop` region (plus the existing reverse-time
      loop as a sibling region).
- [ ] **Acceptance: all existing golden snapshots and runtime tests
      unchanged.** This is a pure refactor; any diff is a bug.

### P2.3 Domain storage classes

- [ ] `PermVar` → struct fields cleared to 0; `TempVar` → locals in the
      parent region; per-clock-domain `IOTA`/`DSCounter` fields with
      declare/retrieve keyed by `ClockDomainId`; guarded-block post-code
      increments (plan §2.4 reference code).
- [ ] Unit tests on declaration/clear/increment emission.

**Exit criteria**: P2.2 lands diff-free on goldens; the region API is the
only way lowering code appends compute statements.

## 6. P3 — Scalar OD/US/DS lowering + first backends (L)

### P3.1 Hsched-driven lowering (plan §7 Step 5, §3.8)

- [ ] Compile `controls` first, then the top-level schedule; on an
      OD/US/DS node open the matching guarded region — boolean OD `if`
      chosen from the clock type's interval (the interval crate is wired
      into `signal_prepare`), integer OD / US counted loop, DS modulo
      guard — compile that node's sub-schedule, close.
- [ ] Node generators: `TempVar` → local store; `PermVar` → field write;
      `ZeroPad(x, H)` → select on last inner iteration index; `Seq(od,
      y)` → compile `od` then `y`; `Clocked` → passthrough (annotation
      only).
- [ ] Delay lines inside a domain use that domain's IOTA
      (`delay.rs` integration).
- [ ] FIR-dump tests: structure matches the plan §2.4 captured C++ for
      the three reference programs.

### P3.2 First backends: C and C++ (plan §7 Step 6a)

- [ ] Structured emission of the nested blocks in `compute` for the C
      and C++ backends.
- [ ] Every other backend keeps a structured `FRS-SFIR` rejection
      ("ondemand not supported by backend X yet") — replacing the P0.1
      blanket rejection, which is removed here.
- [ ] Generated C++ for the plan §2.4 fixtures compiles and reproduces
      the captured reference behavior.

### P3.3 SR adaptation + UI inside domains (plan §7 Step 7)

- [ ] `ma.SR` becomes `SR*H` under US and `SR/H` under DS in the Rust
      FConst propagation path, unrolling nested factors (plan §2.3).
- [ ] UI elements inside a body reach the UI tree with the right path
      (C++ threads `path` through the clock env for this reason).
- [ ] Fixtures: a frequency formula inside US; a slider inside an OD
      body.

### P3.4 Differential validation harness (plan §7 Step 8)

- [ ] Impulse-response comparison against the branch binary (pinned
      `8eebea429`, faust 2.84.3), in the existing
      `crates/compiler/tests/cpp_signal_differential.rs` style.
- [ ] Corpus: stateless body; delay body (per-domain IOTA); recursive
      body (`+ ~ _`); nested domains (OD in OD, US in DS); constant
      clocks 0/1; `ma.SR` under US/DS; UI inside the body;
      multi-instance uniqueness; boolean vs integer OD clock.

### P3.5 Remaining backends (plan §7 Step 6b — staggered, after P3.4)

- [ ] Interp bytecode: conditional/loop opcodes or block-call
      indirection — the largest backend change.
- [ ] Cranelift, wasm, julia: native control flow exists; lowering work.
- [ ] Long tail (csharp, dlang, jax, …) as needed; each backend lifts
      its P3.2 diagnostic when its corpus subset is green.

**Exit criteria**: P3.4 corpus green on C/C++ paths; every other backend
fails cleanly with its named diagnostic.

## 7. P4 — FAD Phase A: `fad` strictly inside a domain (S–M)

Expected to need **zero new AD code**: tangent lanes are domain-local
arithmetic carried by the base port (cohabitation §3). This phase is
corpus + runtime tests; failures indicate base-port bugs, not AD bugs.

- [ ] `fad` inside `ondemand`, slider seed (happy path), finite-diff
      oracle (`fad_recursive_runtime.rs` harness style).
- [ ] Control-rate Adam: `ondemand(ad.fit_adam …)` with a pulse clock;
      convergence vs the audio-rate reference (cohabitation §2 case 1).
- [ ] Event-gated learning: boolean clock from a level detector;
      parameters freeze exactly during silence (case 2).
- [ ] Runtime-count Newton under `upsampling` with signal-valued
      iteration count vs statically unrolled `ad.newton` (case 5).
- [ ] `fad` inside `upsampling` (exact slope): tangent vs finite
      differences at the inner rate (case 6).
- [ ] seed = clock, and clock-depends-on-seed: documented zero through
      the clock, must not crash (cohabitation §5 policy).
- [ ] The P0.4 boundary diagnostic stays in force throughout.

**Exit criteria**: all six corpus families green with finite-difference
oracles; the flagship `ondemand(ad.fit_adam …)` pattern demonstrably
converges at control rate.

## 8. P5 — FAD Phase B: block augmentation (M)

Cross-boundary forward AD per cohabitation §6 ("augment once").

- [ ] Dual rules in `forward_ad.rs` for the glue:
      `TempVar(u) → TempVar(u')`, `Clocked(c, u) → Clocked(c, u')`
      (clock-env child never traversed), `double_clocked`, `ZeroPad(u,
      H) → ZeroPad(u', H)`, `PermVar(u) → PermVar(u')`,
      `Seq(OD, y) → Seq(OD_aug, y/y')`.
- [ ] `OD_aug` built **once per source block node** (memoized rewrite;
      every `Seq` consumer rerouted) — one block, not two: a stateful
      body must not execute twice per fire (cohabitation §6 point 1).
- [ ] Same clock env reused by `OD_aug` (legal per cohabitation §6
      point 2 — augmentation rewrites signal nodes only).
- [ ] `suppress_fad`/`RecFadMode::ExpandAfterRec` nesting (clocked
      wrapper between `Rec` and deferred `fad`,
      `crates/propagate/src/engine.rs:551-621`): dedicated test.
- [ ] Relax the P0.4 diagnostic for the now-supported shapes.
- [ ] Corpus: `fad` around `ondemand` with the seed feeding the wrapper
      *input* — exact value vs finite differences (the contribution that
      was silently dropped); nested `ondemand` under one `fad`
      (augmentation at two depths); `fad` under `Rec` with a clocked
      wrapper in between; structural assertion that IOTA advances once
      per fire in the augmented block.

**Exit criteria**: boundary-crossing gradients exact vs finite
differences; the P0.4 error remains only for genuinely unsupported
shapes (none known at this point on the FAD side).

## 9. P6 — Vector mode base, V1–V6 (L; parallel track possible after P2)

### P6.1 V1 — options plumbing (S)

- [ ] `SignalFirOptions.compute_mode: ComputeMode::{Scalar, Vector {
      vec_size: u32 /* default 128 */, loop_variant: 0|1 }}`; CLI
      `-vec` / `-vs N` / `-lv 0|1`; facade/golden/JSON plumbing.
- [ ] Policy: modules containing a reverse-time sample loop (RAD/BRA)
      force scalar mode with a note diagnostic (TBPTT window question,
      vector doc §5; revisited in P9).

### P6.2 V2 — `LoopGraph` (M)

- [ ] `LoopId` arena of `LoopNode { kind: Vectorizable |
      Recursive(rec set) | Island, pre/exec/post, deps: BTreeSet<LoopId>
      }` built on the P2 region tree; `open_loop`/`close_loop` stack;
      memo-hit dependency recording (the four `CS` cases, vector doc
      §2 item 2). Deterministic ordering by `LoopId`.

### P6.3 V3 — separation criterion + chunk buffers (M)

- [ ] Port `needSeparateLoop` verbatim (vector doc §2 table); shared and
      delayed sample values via `vec_size` chunk buffers
      (`Vector`/`Zec`/`Yec` equivalents); slow values keep
      `control_statements` unchanged.

### P6.4 V4 — vector delay strategies (M)

- [ ] Copy dual-buffer (`_tmp`/`_perm`, pre/post copies, delay rounded
      to a multiple of 4) below `max_copy_delay`; ring buffer
      `pow2(delay + vec_size)` with `_idx`/`_idx_save` pre/post updates
      at or above; waveform index post-increment per chunk. Hosted in
      `LoopNode.pre/post`; integrated into the `delay.rs` strategy set.

### P6.5 V5 — emission (M)

- [ ] `sortGraph` levelization port; `-lv 0` / `-lv 1` chunk drivers;
      I/O pointers rebased per chunk; each loop node a `SimpleForLoop`
      over the chunk `count`; per-loop CSE/refcounting via the P2
      region rule.

### P6.6 V6 — validation (continuous)

- [ ] **Bit-exact** scalar vs `-vec` within faust-rs on the existing
      impulse corpus (primary oracle).
- [ ] Differential vs **upstream `master`** `faust -vec -lv 0|1`
      (the research branch rejects `-vec`).
- [ ] Loop-DAG golden snapshots (levels, buffer kinds/sizes, pre/post).
- [ ] Chunk-edge cases: delay > `vec_size`; `count` not a multiple of
      `vec_size`; `count < vec_size`; `fullcount == 0`.
- [ ] Backend smoke: interp `kLoop` path, cranelift, wasm.

**Exit criteria**: bit-exactness corpus green in both loop variants; C++
differential green; goldens stable.

## 10. P7 — D1 scalar islands: `-vec` × clock domains (M)

Requires P3 (scalar block lowering + Hsched) and P6 (LoopGraph). Design:
vector doc §6.

- [ ] Each top-level OD/US/DS schedule node lowers to one serial
      `Island` loop node: per-`i` `TempVar` reads from upstream chunk
      buffers, guarded scalar body (nested domains included), per-`i`
      hold-expansion write `permvec[i] = fPermVar`.
- [ ] `Hgraph` edges → `LoopGraph` edges 1:1 (`Seq` consumers depend on
      the island; island depends on its externals' loops).
- [ ] Structural assertion: chunk buffers indexed by `i` exist **only**
      for top-level-domain signals (inner-domain signals never get
      outer-rate buffers).
- [ ] Slow (`kBlock`) clock: guard reads the control scalar (hoisting
      deferred to P9).
- [ ] `-vec` accepted with clocked primitives — remove any rejection;
      degradation is local (islands serial), semantics exact.
- [ ] Tests: bit-exact vs scalar on plan §2.4 + cohabitation §8 corpus
      under `-vec`; two islands sharing one upstream vector loop; DS
      counter phase across chunks; fire exactly at a chunk boundary;
      P4 (FAD Phase A) corpus re-run under `-vec`.

**Exit criteria**: every clock-domain fixture bit-exact under `-vec` in
both loop variants; FAD-A corpus green under `-vec`.

## 11. P8 — RAD Phase C: clock-aware tape (L)

Reverse mode across domains per cohabitation §7 (adjoints = multirate
transposes: hold ↔ accumulate-at-fire, zero-pad ↔ decimate, snapshot ↔
impulse deposit).

- [ ] Clock-aware `SigBlockReverseAD` tape: record the firing pattern
      per outer tick; variable-rate storage for inner intermediates
      (×H under US, ÷H under DS); per-domain reverse time stamps.
- [ ] `ondemand(B)` adjoint as gated integrate-and-dump: accumulate `ȳ`
      scanning the hold period in reverse; at a recorded fire instant,
      push through `Bᵀ` and deposit into `ū`.
- [ ] Staged enablement, relaxing the named rejection per kind:
      boolean-clock `ondemand` first, then integer OD, then US/DS.
- [ ] Corpus: `rad` around each wrapper kind graduates from diagnostic
      snapshot to numeric finite-difference test (`rad_runtime.rs` /
      `block_reverse_ad.rs` harness style).
- [ ] Measured payoff: tape size and reverse-sweep cost scale ÷H for a
      decimated loss (cohabitation §2 case 3).

**Exit criteria**: rad-around corpus numerically green for the enabled
kinds; remaining kinds still fail loudly by name.

## 12. P9 — Optimizations (optional, independent items)

- [ ] **D2** — inner-rate vectorization of *literal-constant* US/DS
      interiors (vector doc §6): `vec_size % H == 0`, tail chunks fall
      back to the D1 island; recursive interiors stay serial; stateless
      interiors vectorize at the inner rate.
- [ ] **LPTV transpose** of constant-rate US/DS in the YOLO
      linearize-once path (after P8;
      `yolo-linearize-once-rad-analysis-2026-05-21-en.md`).
- [ ] Hoist slow-clock island guards out of the chunk loop.
- [ ] Scheduling strategies beyond depth-first (`-ss` parity) if
      profiling justifies them.
- [ ] `-omp`/`-sch` groundwork on `LoopGraph` levels (vector mode is the
      substrate of all parallel modes, vector doc §2 item 7).
- [ ] Decide the reverse-time × chunking semantics (TBPTT window under
      `-vec`) and lift the P6.1 force-scalar policy accordingly.

## 13. Cross-cutting workstreams (continuous)

- **Differential and numeric corpus.** The P3.4 harness is the backbone;
  it grows with every phase. FAD/RAD combinations have **no upstream
  reference** — the oracle is finite differences
  (`fad_recursive_runtime.rs` / `rad_runtime.rs` /
  `block_reverse_ad.rs` style).
- **Diagnostics discipline.** Every intermediate state fails with a
  structured `FRS-` code naming the construct — never a panic, never a
  silent fallback (the P0.4 rule generalizes to backends and modes).
- **Reference pinning.** C++ parity targets commit `8eebea429`; the
  research branch is unstable (plan §8 item 1). Upstream `master` is the
  reference for base `-vec` only.
- **Documentation.** Each landed work package gets a journal entry; the
  three analysis documents are amended when implementation deviates from
  the plan; this roadmap's checkboxes are the tracking surface.

## 14. Flat landing order (single stream)

1. **P0** guards & groundwork — indivisible change set (S–M)
2. **P1.1** clock inference, **P1.2** Hgraph/Hsched (M)
3. **P2.1** region design, **P2.2** diff-free refactor, **P2.3** storage
   classes (M–L)
4. **P3.1** lowering, **P3.2** C/C++ backends, **P3.3** SR+UI,
   **P3.4** differential harness (L) — then **P3.5** backends staggered
5. **P4** FAD Phase A corpus (S–M)
6. **P5** FAD Phase B block augmentation (M)
7. **P6.1–P6.6** vector mode base (L) — *second track may start this
   right after step 3*
8. **P7** D1 scalar islands (M)
9. **P8** RAD Phase C (L)
10. **P9** optimizations, as profiling/needs dictate (optional)
