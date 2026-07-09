# FFT scalability: CSE is skipped inside `ondemand` guarded blocks — diagnosis and correction plan

**Date**: 2026-07-09
**Scope**: `crates/transform/src/signal_fir/{cse.rs,module/build.rs,module/clocked.rs}`
**Branch context**: `ondemand-vec-fad-synthesis` (the spectral / FFT-on-`ondemand` work)
**Related**: [`fir-cse-runtime-optimizations-plan-2026-04-03-en.md`](fir-cse-runtime-optimizations-plan-2026-04-03-en.md)
(the pass this note extends), [`ondemand-vec-fad-interleave-synthesis-2026-07-07-en.md`](ondemand-vec-fad-interleave-synthesis-2026-07-07-en.md),
`docs/ondemand-fft-spectral-comparison-en.md`.

**Status**: **Phase A implemented 2026-07-09** (`cse.rs` per-scope recursion).
The framed FFT is now O(N log N): arithmetic ops for N=8→128 went
188/1080/6488/39048/234632 → 104/301/834/2179/5428 (**43× fewer at N=128**,
`ops/(N log N)` flat 4.3→6.1, ×/doubling ≈ 2.5); generated code at N=256 shrank
9.2 M → 60 k interp lines (~152×) and compile time 8.6 s → 1.8 s. Numerics
unchanged (interleave_fft, impulse-runner, cpp_clocked_differential all pass;
190 goldens unaffected). Phase B/C still open (see §5); N=512 still hits the
eval-budget wall.

---

## 1. Symptom

The frame-rate FFT built on `ondemand` (`tests/corpus/ondemand_fft_framed_*.dsp`)
does not scale: `N=256` compiles in ~8.6 s and 9.2 M `.fbc` lines, `N=512`
fails on the evaluator recursion budget, and the round-trip effects are heavier
still. The naive reading was "Faust unrolls everything, so large N is
expensive." That reading is **wrong about the dominant cost**.

## 2. Measurement: the generated FFT is O(N^2.6), not O(N log N)

Counting floating-point `+`/`*` in the generated C++ of the **framed**
(analysis-only, in-`ondemand`) FFT:

| N | arithmetic ops | ops / (N log₂N) | ×/doubling |
|---|----------------|-----------------|------------|
| 8   | 188     | 7.8   | —    |
| 16  | 1 080   | 16.9  | 5.74 |
| 32  | 6 488   | 40.5  | 6.01 |
| 64  | 39 048  | 101.7 | 6.02 |
| 128 | 234 632 | 261.9 | 6.01 |

~6× per doubling ⇒ **≈ O(N^2.58)** — worse than a naive O(N²) DFT. The
normalized column diverges without bound: the butterfly sharing (the entire
point of an FFT — an O(N log N) DAG of intermediate results, each with fan-out
2) is gone. For `N=32` the generated code holds only ~65 temporaries (≈ 2N, the
output bins) and **zero intermediate-stage temporaries**: the DAG is flattened
into 2N independent trees.

## 3. It is specific to the `ondemand` block

The **plain** FFT (`process = an.fft(N)`, no `ondemand`) does **not** blow up:

| N | ops | ops / (N log₂N) | temporaries | ×/doubling |
|---|-----|-----------------|-------------|------------|
| 8  | 192   | 8.0 | 61    | —    |
| 16 | 527   | 8.2 | 193   | 2.74 |
| 32 | 1 376 | 8.6 | 539   | 2.61 |
| 64 | 3 439 | 9.0 | 1 399 | 2.50 |

`ops / (N log N)` is flat (8.0 → 9.0), temporaries grow as O(N log N): **the
plain FFT is O(N log N) and CSE works perfectly.** Same core (`an.fftb`), same
compiler — the *only* difference is whether the transform sits inside an
`ondemand` guarded block. Framed `fft(32)` = ~65 temps; plain `fft(32)` = 539
temps.

## 4. Root cause (pinned)

The sharing exists at every representation level:

- **Signals are hash-consed.** `SigId = TreeId`; `TreeArena::intern`
  (`crates/tlib/src/arena.rs`) deduplicates by `(kind, children)`. In the
  `--dump-sig` of `an.fft(4)`, the sub-FFT term
  `input[2] + (1·input[6] − 0·input[7])` is one node referenced by all 8
  outputs.
- **FIR nodes are hash-consed** too (`fir::encoding::intern_tag`).
- **A CSE materialization pass exists and is correct**
  (`signal_fir/cse.rs`): it ref-counts `FirId` fan-out per bucket and wraps
  multi-referenced non-trivial nodes in `DeclareVar` + `LoadVar`. This is what
  makes the *plain* FFT O(N log N).

The defect is that **CSE never sees the statements inside the guarded block.**
`build.rs` runs CSE on exactly three flat buckets — `constants_statements`,
`control_statements`, and each `sample_loop_statements` — and the reference
counter deliberately does **not** descend into nested scopes.
`cse.rs::value_children_of` documents it:

> For **statement nodes** this returns embedded values … *but not structural
> children such as block bodies or loop bodies — those are separate execution
> scopes and should not be traversed by intra-bucket CSE.*

and for the guard itself returns only the condition:

```rust
FirMatch::If { cond, .. } | FirMatch::Control { cond, .. } => vec![cond],
```

Consequences:

- **Plain FFT** → all butterfly statements live directly in the flat
  `sample_loop_statements` bucket → CSE ref-counts them, materializes the
  shared stage results → O(N log N).
- **Framed FFT** → the butterflies live in the `then`-body of the `BoolIf`
  guard emitted by `module/clocked.rs` (the `ondemand` block). That body is a
  nested scope, so CSE visits only the guard's `cond` and skips the entire
  transform. With no `DeclareVar` temporaries inserted, the backend prints each
  hash-consed value node inline every time it is referenced → the DAG is
  re-expanded into 2N trees → **O(N^2.6)**.

So the wall is not "unrolling", not the eval budget, and not the FFT
formulation. It is a **one-line scope-coverage gap in the CSE pass**: intra-
bucket CSE stops at guarded-block boundaries, and *every* stateful `ondemand` /
`upsampling` / `downsampling` block (P3) is such a boundary. Any nontrivial
computation inside a clock-domain block currently pays the fully-inlined cost.

## 5. Correction plan

### Phase A — per-scope CSE inside guarded blocks (the fix) — DONE

*Implemented 2026-07-09.* `signal_fir/cse.rs` now runs CSE **per execution
scope**: `materialize_scope` recurses into `If`/`Control`/loop/`Block` bodies as
independent buckets (fresh ref-counts, temporaries local to the body), with the
`fTemp`/`iTemp` counters threaded through the whole scope tree for unique names.
Guard conditions and loop headers are left untouched (evaluated in the enclosing
scope). `build.rs` call sites dropped the pre-computed `ref_counts` argument.
The results above confirm O(N log N). Design as originally specified below.

Make CSE run **once per execution scope**, treating each block / loop / guard
body as its own bucket, materializing temporaries **local to that scope**
(declared inside the body, used inside the body — which is exactly the
correctness constraint the current "separate execution scopes" comment is
protecting: a temp must not be hoisted across a conditional boundary).

Concretely:

1. Add a recursive driver that, for every statement carrying a structural body
   (`If`/`Control` `then`/`else`, loop bodies, nested `Block`s), collects that
   body's statement list and runs the existing
   `count_fir_value_uses` + `materialize_shared_values` on it as an independent
   bucket, with `fTemp`/`iTemp` counters threaded through so names stay unique.
2. Recurse depth-first so inner scopes are materialized before their parents.
3. Leave the cross-scope rule intact: a node used in two sibling scopes is
   materialized independently in each (correct, and matches the current flat
   behavior). A future refinement can hoist a node used in a scope **and** its
   dominator to the dominator; not needed for the FFT.

Expected payoff: the in-block FFT drops from O(N^2.6) to **O(N log N)** in both
generated-code size and arithmetic — i.e. compile time *and* runtime improve
together. This is the single highest-leverage change and it is localized to
`cse.rs` + the call site in `build.rs` (plus, if guard bodies are not already a
walkable statement list at CSE time, a small hook in `module/clocked.rs`).

**Validation**:
- Re-run the op-count harness (§2/§3): the framed FFT's `ops/(N log N)` must go
  flat, matching the plain FFT within a small constant.
- Numerical parity unchanged: `crates/compiler/tests/interleave_fft.rs` (bins ==
  direct DFT) and the impulse-runner effect checks must still pass bit-for-bit.
- Add a regression asserting the framed FFT emits O(N log N) temporaries (e.g.
  temp count at N∈{16,32,64} grows ≈ ×2.5, not ×6).

### Phase B — lift the front-end ceiling (needed to exploit A at large N)

Even with A, box evaluation still recurses; `N=512` fails at
`FRS-EVAL-0099` (guarded depth 1024). Make the evaluator iterative (explicit
work stack) or raise the budget behind a larger worker stack, and land the
propagate-memoization port ([[project-propagate-memoization-port]]) so the two
`fftb(N/2)` halves are not re-derived. After A this unblocks `N≥1024`.

### Phase C — constant-factor runtime wins (after A)

- **Real-FFT**: the window taps are real; `(_,0)` doubles the work. An `rfft`
  core halves arithmetic and exposes only N/2+1 bins.
- **Twiddle folding**: snap `cos/sin` of rational angles so exact 0 / ±1 / ±j
  are recognized (the code currently carries `6e-17` residues and `×1`); drop
  the dead multiplies.
- **`-vec` / SIMD** across bins once the in-block graph is O(N log N) (the
  STFT `ondemand` is the D1 "scalar island").

### Non-goals / recorded caveats

- True duration-changing time-stretch stays out of the synchronous 1:1 model
  (see the comparison doc). Unrelated to this fix.
- If, after A, a residual super-linear factor remains, revisit whether the
  clocked-block lowering itself duplicates statements per output bin (it should
  not, but verify with the temp-count regression).

## 6. One-line summary

FFT-in-Faust is not slow because Faust unrolls — it is slow because the FIR CSE
pass does not descend into `ondemand` guarded blocks, so a fully-shared
O(N log N) butterfly DAG is emitted as O(N^2.6) inlined trees. Extending CSE to
run per nested scope is the fix, and it improves compile time and runtime at
once.
