# Vector Mode (`-vec`): Analysis and Port Plan, with Clock Domains

Date: 2026-06-10

Status: proposed

Extracted from §10 of
[ondemand-clock-domains-analysis-port-plan-2026-06-10-en.md](ondemand-clock-domains-analysis-port-plan-2026-06-10-en.md)
(the *base plan*) when that document grew too large. Cross-references of
the form **plan §N** point to base-plan sections; **Step N** to the steps
of the plan §7 port plan; **cohabitation §N** to
[ondemand-fad-rad-cohabitation-2026-06-10-en.md](ondemand-fad-rad-cohabitation-2026-06-10-en.md).
The consolidated landing order across all documents is the
[roadmap](ondemand-vec-fad-implementation-roadmap-2026-06-10-en.md)
(this document's V1–V6 land as roadmap P6, D1 as P7, D2 in P9).

C++ reference: same branch/commit as the base plan
(`master-dev-ocpp-od-fir-2-FIR19`, commit `8eebea429`) for the clocked
machinery; the base `-vec` analysis (§2) equally applies to upstream
`master`, which is also the differential-test reference since the
research branch rejects `-vec`.

## 1. Goal

Vector mode does not exist in faust-rs at all, and on the C++ research
branch it is disabled outright (§3). This document analyzes what `-vec`
does in upstream C++, why it conflicts with the ondemand machinery there,
and how to port it to faust-rs in a way that *composes* with the clock
domains of the base plan (plan §3–§7) instead of excluding them.

## 2. What `-vec` does in C++

Scalar mode compiles the whole signal graph into one sample loop. Vector
mode — `-vec`, with `-vs N` (vector size, default 128) and `-lv 0|1`
(loop variant); option parsing and defaults in
[global.cpp:469-470, 1451-1532](RUST/faust/compiler/global.cpp) —
restructures `compute()` into:

```
for (index = 0; index < fullcount; index += vecsize)   // outer chunk loop
    count = min(vecsize, fullcount - index)
    ── DAG of small inner loops, each `for (i = 0; i < count; i++)`,
       executed in dependency order ──
```

so that the C compiler can auto-vectorize the *non-recursive* inner loops,
while recursive computations are quarantined in their own serial loops.

Two parallel implementations exist (same algorithm, different IR):

| Path | Compiler | Loop object | Emission |
|---|---|---|---|
| old C++ (`-ocpp`) | `VectorCompiler : ScalarCompiler` ([compile_vect.cpp](RUST/faust/compiler/generator/compile_vect.cpp), 555 lines) | `Loop` ([loop.hh](RUST/faust/compiler/parallelize/loop.hh)) held by `Klass` | string statements; chunk drivers `printComputeMethodVectorSimple/Faster` ([klass.cpp:1056-1180](RUST/faust/compiler/generator/klass.cpp)) |
| FIR backends | `DAGInstructionsCompiler : InstructionsCompiler` ([dag_instructions_compiler.cpp](RUST/faust/compiler/generator/dag_instructions_compiler.cpp), 671 lines) | `CodeLoop` ([code_loop.hh](RUST/faust/compiler/parallelize/code_loop.hh)) | `CodeContainer::generateDAGLoop` + chunk driver ([code_container.cpp:147-193](RUST/faust/compiler/generator/code_container.cpp)) |

Mechanics, identical in both paths:

1. **Loop separation.** `openLoop`/`closeLoop` maintain a current-loop
   stack; `generateCodeRecursions` opens a dedicated loop per recursive
   group (keyed by the rec symbol); `needSeparateLoop`
   ([compile_vect.cpp:304-339](RUST/faust/compiler/generator/compile_vect.cpp),
   [dag_instructions_compiler.cpp:370-393](RUST/faust/compiler/generator/dag_instructions_compiler.cpp))
   decides for everything else:

   | signal | separate loop? |
   |---|---|
   | used delayed (`maxDelay > 0`) | yes |
   | `verySimple` or slower than `kSamp` | no |
   | `sigDelay` read | no (compiled where used) |
   | recursive projection | yes (serial) |
   | shared (`hasMultiOccurrences`) sample expression | yes |
   | otherwise | no (inlined into the consumer's loop) |

2. **Cross-loop dependencies.** On a memo-cache hit, `CS()` records a
   `fBackwardLoopDependencies` edge from the current loop to the defining
   loop — with three extra patterns for delayed reads and projections
   ([compile_vect.cpp:74-124](RUST/faust/compiler/generator/compile_vect.cpp)).
   The loops form a DAG.
3. **Cross-loop data = chunk buffers.** A sample-rate value shared between
   loops is materialized in an array of `gVecSize` elements indexed by the
   chunk-local `i` (generated names `Vector*`/`Zec*`; delayed values
   `Yec*`), while slow (`< kSamp`) values stay scalars compiled once per
   `compute` outside the chunk loop
   ([compile_vect.cpp:220-297](RUST/faust/compiler/generator/compile_vect.cpp)).
   Short buffers are stack arrays in the FIR path; ring buffers and their
   indices live in the DSP struct.
4. **Delay lines change layout.** Below `gMaxCopyDelay`: a copy-based dual
   buffer — permanent `_perm` array plus a `_tmp` working array of
   `vecsize + delay` elements, with a pre-copy (`_perm → _tmp` head), exec
   writes at `[i]`, and a post-copy back. At or above: a ring buffer of
   `pow2(delay + vecsize)` elements with `_idx`/`_idx_save` struct fields
   updated in pre/post code
   ([compile_vect.cpp:471-544](RUST/faust/compiler/generator/compile_vect.cpp)).
   Hence every loop has **three phases** — `fPreCode` / `fExecCode` /
   `fPostCode` — printed around the per-chunk `for`.
5. **Topological emission.** `sortGraph`
   ([graphSorting.cpp](RUST/faust/compiler/parallelize/graphSorting.cpp))
   levelizes the loop DAG from the root; levels are emitted in order as
   `// Section : n` groups. Note `lset = std::set<Loop*>` is
   pointer-ordered, so emission order *within* a level is
   non-deterministic across runs — a defect, not a feature.
6. **Chunk drivers.** `-lv 0` (simple): one outer loop with
   `count = min(vecsize, fullcount - index)`. `-lv 1` (faster): a main
   loop with constant `const int count = vecsize` (helps the
   auto-vectorizer) plus a remainder block. I/O pointers are rebased per
   chunk (`input0 = &input[0][index]`).
7. **Vector mode is the substrate of the parallel modes**: `-omp` and
   `-sch` force `gVectorSwitch = true`
   ([global.cpp:1867-1870](RUST/faust/compiler/global.cpp)) and
   parallelize the levels of the same loop DAG.

## 3. Why `-vec` × ondemand is unsupported upstream

On the research branch the check is unconditional — any `-vec` (or `-omp`
/`-sch`, which imply it) throws
`ERROR : '-vec' is not yet supported with 'ondemand' primitive`
([global.cpp:1873-1876](RUST/faust/compiler/global.cpp)): vector mode is
entirely disabled there, even for programs that use no clocked primitive,
because the whole branch now compiles through the clocked machinery.

The conflict is structural, not incidental:

1. **Two incompatible code organizations.** Vector mode *partitions* the
   flat signal graph into sibling loops; the ondemand machinery *nests*
   guarded code blocks inside one loop via the per-loop
   `fCodeStack` of `CodeIFblock/CodeODblock/CodeUSblock/CodeDSblock`
   ([loop.hh:107](RUST/faust/compiler/parallelize/loop.hh), plan §3.8). Letting
   `needSeparateLoop` split code *inside* an OD body would tear the block
   structure apart; conversely the block stack knows nothing about loop
   boundaries.
2. **Indexing mismatch.** Chunk buffers are indexed by the outer
   chunk-local `i`; inside a domain, time is the local fire count
   (per-domain `IOTA`). An inner-domain value has no meaningful `[i]` slot
   at the outer rate, but the occurrence analysis and `Zec/Yec` allocation
   are clock-unaware and would happily allocate one.
3. **The hold output is a serial scan.** `PermVar` at the outer rate is
   `y[i] = fire[i] ? f(...) : y[i-1]` — a sample-and-hold recurrence that
   cannot be naively handed to the auto-vectorizer.

What upstream has *not yet* done is connect the hierarchical `Hgraph`
(plan §3.7) — which already isolates each domain in its own subgraph — to the
`Loop` machinery. That connection is exactly the design proposed for
faust-rs in §6.

## 4. faust-rs current state

- **No vector mode anywhere.** `SignalFirOptions` has no compute-mode
  field ([mod.rs:119-157](RUST/faust-rs/crates/transform/src/signal_fir/mod.rs));
  no `-vec`/`-vs`/`-lv` CLI plumbing exists.
- `build_module` emits: init-time constants, once-per-`compute`
  `control_statements` (the "slow code outside the loop" already matches
  C++), then **one flat forward sample loop** — plus an optional
  reverse-time loop for RAD `SigBlockReverseAD` carriers — assembled as a
  single `SimpleForLoop("i0", count)`
  ([build.rs:173-233, 410-431](RUST/faust-rs/crates/transform/src/signal_fir/module/build.rs)).
  There is no `fullcount`/`index` chunking and no loop graph.
- CSE/refcounting runs per statement bucket
  ([build.rs:251-283](RUST/faust-rs/crates/transform/src/signal_fir/module/build.rs),
  [cse.rs](RUST/faust-rs/crates/transform/src/signal_fir/cse.rs));
  delay strategies ([delay.rs](RUST/faust-rs/crates/transform/src/signal_fir/delay.rs))
  are scalar per-sample (Delay1 / shift-copy / ring-pow2 / exact-size,
  keyed by `max_copy_delay` and `delay_line_threshold`).
- The FIR IR **already has the statement vocabulary** vector mode needs —
  `Block`, `If`, `SimpleForLoop`, `ForLoop`, array declarations
  ([matcher.rs:173-205](RUST/faust-rs/crates/fir/src/matcher.rs)) — used
  today in init/clear/table paths. Vector mode is a **lowering
  organization change in `signal_fir` plus options plumbing, not an IR
  extension.**
- One decisive structural advantage over C++: faust-rs has a *single*
  lowering site feeding every backend (C, C++, Rust, cranelift, wasm,
  interp, …), so vector mode is implemented once. C++ maintains it twice
  (Klass strings + FIR instructions) and each text backend re-prints it.

## 5. Port plan part 1 — base vector mode (no clock domains)

Each step is independently testable; V1–V6 do not depend on the base plan
(plan §7) and can land first.

- **V1 — options plumbing (small).** Add to `SignalFirOptions` a
  `compute_mode: ComputeMode` with
  `Scalar | Vector { vec_size: u32 /* default 128 */, loop_variant: u8 }`;
  CLI `-vec` / `-vs N` / `-lv 0|1`; thread through the compiler facade,
  golden and JSON paths.
- **V2 — `LoopGraph` in `signal_fir` (medium, the architectural step).**
  A `LoopId` arena of
  `LoopNode { kind: Vectorizable | Recursive(rec set) | Island /* §6 */,
  pre: Vec<FirId>, exec: Vec<FirId>, post: Vec<FirId>,
  deps: BTreeSet<LoopId> }`. Replace the single `sample_phases`
  accumulator with a current-loop stack (`open_loop`/`close_loop`
  mirroring C++); on memo-hit, record the dependency edge (port the four
  `CS` cases of [compile_vect.cpp:74-124](RUST/faust/compiler/generator/compile_vect.cpp)).
  Determinism by construction: `LoopId`-ordered sets fix the C++
  pointer-set nondeterminism.
- **V3 — separation criterion + chunk buffers (medium).** Port
  `needSeparateLoop` verbatim (table in §2). Materialize cross-loop
  sample values in `vec_size` arrays (`Vector`/`Zec`/`Yec` equivalents);
  slow values keep using `control_statements` unchanged. Short buffers as
  stack arrays, ring buffers + indices as struct fields (FIR-path parity).
- **V4 — vector delay strategies (medium).** Extend the `delay.rs`
  strategy set with the two block-level layouts (copy dual-buffer with
  pre/post copy; ring with `_idx`/`_idx_save` pre/post updates), their
  pre/post statements hosted by `LoopNode.pre/post`. Reuse the existing
  `max_copy_delay` threshold; include the vector-mode waveform index
  post-increment ([compile_vect.cpp:546-555](RUST/faust/compiler/generator/compile_vect.cpp)).
- **V5 — emission (medium).** Port `sortGraph` levelization; emit
  `-lv 0`/`-lv 1` chunk drivers with rebased I/O pointers; each loop node
  becomes a `SimpleForLoop` over the chunk `count`, with pre/exec/post
  around it. CSE/refcounting becomes per-loop (values may never be hoisted
  across loop boundaries — the cross-loop interface is exclusively the
  named buffers of V3).
- **V6 — validation (continuous).** Differential vs **upstream `master-dev`**
  `faust -vec -lv 0|1` (the research branch rejects `-vec`); since vector
  mode performs the same per-sample arithmetic and only changes storage,
  outputs must be bit-exact vs scalar faust-rs — a cheap, strong oracle.
  Loop-DAG golden snapshots; backend smoke tests (interp `kLoop` path,
  cranelift, wasm).

Caveat — **reverse-time loops** (RAD/BRA, §4): chunking would change
the implicit TBPTT window from `count` to `vec_size`, which is a semantic
change. Policy: modules containing a reverse-time sample loop force
scalar mode (with a note-level diagnostic) until that window semantics is
deliberately decided.

## 6. Port plan part 2 — composing `-vec` with clock domains

There is no upstream reference for this combination (as with FAD × OD,
cohabitation §4): faust-rs defines the semantics. The key observation that makes the
composition natural: **the domain boundary glue is already
buffer-shaped.** `TempVar` (snapshot in) and `PermVar` (hold out) are
precisely the cross-loop interface that vector mode invents for ordinary
signals, and the `Seq(OD, permvar)` edges of the `Hgraph` (plan §3.7) are
exactly loop-dependency edges.

**Rule D1 — clocked blocks become scalar islands.** Every OD/US/DS node
of the *top-level* schedule becomes one dedicated **serial** loop node
(`LoopKind::Island`) in the `LoopGraph`:

```c++
// upstream vector loops fill iClockVec[] and fTempSrcVec[] …
for (int i = 0; i < count; i++) {        // island: serial chunk loop
    float fTemp0 = fTempSrcVec[i];       // TempVar snapshot (outer domain)
    if (iClockVec[i]) {                  // or OD-for / DS modulo guard
        …body, scalar, exactly the plan §7 Step 4-5 lowering, nested
         domains included…
        IOTA0 = IOTA0 + 1;               // local time
        fPermVar0 = …;
    }
    fPermVarVec0[i] = fPermVar0;         // hold-expansion into a chunk buffer
}
// downstream vector loops read fPermVarVec0[i] …
```

- **Edges.** The island depends backward on the loops producing its
  externals (clock chunk buffer, `TempVar` sources); every consumer of
  `Seq(OD, y)` depends backward on the island. This is a 1:1 image of the
  `Hgraph` edges — base-plan Step 3 already computes them.
- **The invariant that dissolves C++ conflict #2:** chunk buffers indexed
  by `i` exist **only for top-level-domain signals**. Inner-domain
  signals live in the island's scalar locals and struct fields and never
  receive outer-rate buffers. Domain → time-base mapping: top level ↔
  chunk index (vectorizable); every other domain ↔ its own event time
  (serial).
- **Chunk boundaries are transparent for free**: per-domain
  `IOTA`/`DSCounter` and `PermVar` held values are struct fields, so
  state crosses `index` chunks exactly as it crosses `compute` calls.
- **Slow clocks**: a `kBlock` clock (slider) is compiled in
  `control_statements`; the island guard reads the scalar. Hoisting the
  `if` out of the island loop is an optimization, not a correctness
  requirement.
- **Correctness argument**: per outer sample, the executed sequence
  (snapshot → guarded body → hold) is *identical* to the scalar port of
  plan §7; vector mode only changes where outer-domain values are stored.
  Output must be bit-exact vs scalar — the same oracle as V6.

Policy consequence: unlike C++, faust-rs should **not** reject `-vec` in
the presence of clocked primitives once D1 lands. Degradation is local —
the island runs serially, everything outside it vectorizes — and the
semantics is exact. This supersedes the rejection suggested in the
original plan §8 item 4.

**Phase D2 (optional, later) — vectorizing constant-factor US/DS
interiors.** For a *literal constant* integer factor `H`, the firing
pattern is static, and the domain interior can itself be lowered as a
loop DAG at the inner rate within each chunk:

| | inner trip count per chunk | input adaptation | output adaptation |
|---|---|---|---|
| `upsampling(H)` | `count * H` (buffers of `vec_size * H`) | zero-pad expansion: value at `i*H + H-1`, else 0 | decimation: outer `[i]` = inner `[i*H + H-1]` |
| `downsampling(H)` | `count / H` | gather with stride `H` (respecting the `DSCounter` phase) | hold-expansion: each inner value fills `H` outer slots |

Apply the V3 separation analysis *recursively inside the domain*:
recursive inner groups remain serial inner loops; stateless sections
vectorize at the inner rate. This is what makes
`upsampling(stateless nonlinearity)` — the cohabitation §2 case 6 oversampled-slope
pattern — genuinely SIMD. Constraints: literal `H` only (a runtime `H`
gives data-dependent trip counts); chunk alignment `vec_size % H == 0`,
with remainder/tail chunks falling back to the D1 island; boolean or
data-dependent `ondemand` interiors would need index compaction (gather
fire indices → batch the body → scatter) and stay out of scope as a
research note.

**Sequencing.** Part 1 (V1–V6) is independent of the base plan (plan §7) and can
land first. D1 requires plan §7 Steps 2–5 (clock inference, `Hsched`, scalar
block lowering) and then consists mostly of the `Island` loop kind plus
the `Hgraph` → `LoopGraph` edge mapping. D2 is pure optimization, after
both. The block-aware CSE invariant is shared: "never hoist across a loop
boundary" (V5) and "never hoist across a domain boundary" (plan §7 Step 4) are
the same rule on the same structure and should be one implementation.

**FAD/RAD note.** Vector mode is invisible to propagation-stage AD: FAD
tangent lanes are ordinary signals and partition into loops like any
other signal; the cohabitation analysis is unaffected. The single contact point is
the reverse-time-loop caveat of §5 (force-scalar until the TBPTT
window question is settled).

## 7. Validation

- **Bit-exactness scalar vs `-vec` within faust-rs** on the whole
  existing impulse corpus (base mode) and on the plan §2.4 + cohabitation §8 fixtures
  (D1) — the primary oracle, no FP tolerance needed.
- Differential vs upstream `master` `-vec -lv 0|1` for base mode only
  (upstream cannot serve as D1 reference — it rejects `-vec`).
- Loop-DAG golden snapshots: sections/levels, island placement, buffer
  kinds and sizes, pre/post phases.
- Chunk-edge cases: delay > `vec_size` (ring path), `count` not a
  multiple of `vec_size` (`-lv 1` remainder), `count < vec_size`,
  `fullcount == 0`, DS counter phase across chunks, clock firing exactly
  at a chunk boundary, two islands sharing one upstream vector loop.
- FAD Phase A corpus (cohabitation §8) re-run under `-vec` once D1 lands.

## 8. Risks specific to vector mode

1. **Per-loop CSE/refcount interplay** with the existing bucket-based
   pass ([placement.rs](RUST/faust-rs/crates/transform/src/signal_fir/placement.rs)):
   the no-hoisting-across-boundaries invariant must be enforced
   structurally (per-loop buckets), not by convention.
2. **Interp backend loop coverage**: the bytecode `kLoop` path is
   conservative today
   ([interp/compiler.rs:1507](RUST/faust-rs/crates/codegen/src/backends/interp/compiler.rs));
   chunk loops + island loops nesting needs explicit verification.
3. **Buffer memory**: one `vec_size` array per shared value (×`H` for
   D2-US). Stack arrays may need to move to the DSP struct beyond a size
   threshold, as the C++ FIR path does for ring buffers.
4. **Reverse-time loop window semantics** (§5 caveat) — decide before
   allowing `-vec` on RAD-bearing modules.

