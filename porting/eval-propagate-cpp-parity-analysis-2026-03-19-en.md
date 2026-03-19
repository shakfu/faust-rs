# Eval / Propagate C++ Parity Analysis

Date: 2026-03-19

Reference C++ baseline:
- `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/evaluate/environment.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/evaluate/loopDetector.hh`
- `/Users/letz/Developpements/RUST/faust/compiler/propagate/propagate.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/boxes/boxtype.cpp`

Rust scope analyzed:
- [crates/eval/src/lib.rs](/Users/letz/Developpements/RUST/faust-rs/crates/eval/src/lib.rs)
- [crates/eval/tests/core_eval.rs](/Users/letz/Developpements/RUST/faust-rs/crates/eval/tests/core_eval.rs)
- [crates/propagate/src/lib.rs](/Users/letz/Developpements/RUST/faust-rs/crates/propagate/src/lib.rs)

## 1. Executive Summary

Current status:
- `eval`: semantically close to C++ on the active production corpus, but still uses an adapted Rust representation for environments, closures, pattern matchers, and source loading.
- `propagate`: semantically close to the C++ post-`evalprocess -> a2sb -> propagate` boundary, with an explicit typed Rust boundary (`FlatBoxId`) and an adapted grouped-UI ownership model.
- Global conclusion: parity is strong on the production path, but not yet a byte-for-byte or architecture-identical port.

Current mapping status:

| Subsystem | Status | Meaning |
|---|---|---|
| `eval` semantics | `adapted` but close to `1:1` | Same target behavior, different host-side representation |
| `propagate` semantics | `adapted` | Same target behavior after `eval/a2sb`, different explicit boundary and UI ownership |
| `eval -> propagate` boundary | `adapted` and explicit | Rust makes the C++ implicit post-eval flat subset explicit via `FlatBoxId` |
| Generated C++ text shape | `not 1:1` | Semantic parity is the target, not textual codegen identity |

The main result of the recent audit is:
- the remaining `bitonicSort_test` mismatch was a real `eval` parity bug,
- it is now fixed at the `route(...)` evaluation boundary,
- the Rust-generated C++ is still textually different from Faust C++, but the evaluated signal graph is now semantically aligned.

## 2. Methodology

This analysis is based on:
- direct source comparison between Rust and C++ implementation points,
- the Rustdoc/source provenance already present in the Rust crates,
- targeted differential debugging on representative failing library cases,
- focused regression tests added in `eval`,
- signal-level inspection with `--dump-sig`.

Important scope note:
- this document is an analysis of current parity status,
- not a claim that every `eval` / `propagate` case has been differentially proven across the full Faust corpus.

## 3. Eval: Current Parity Status

### 3.1 What is now aligned with C++

The following semantic areas are now explicitly aligned with `eval.cpp`:

- Top-level execution path:
  - C++: `evalprocess(...) -> a2sb(...) -> boxSimplification(...)`
  - Rust: `eval_process(...) -> a2sb_value(...) -> box_simplification(...)`

- Closure / residual form handling:
  - Rust now matches C++ semantics for residual closures and pattern matchers through `a2sb(...)` memoization and explicit closure values.
  - This fixed production cases such as:
    - `virtual_analog_oscillators.dsp`
    - `gate_compressor.dsp`
    - `matMul_test`

- `inputs(...)` / `outputs(...)` parity:
  - Rust now computes arity on `a2sb(eval(expr,...))`, like C++ `applyList(...)` / `boxlistOutputs(...)`.
  - This fixed iterator-count and application-arity failures such as:
    - `reverbTank_demo_test`
    - `matMul_test`

- `route(...)` evaluation parity:
  - Rust now matches the C++ `isBoxRoute` branch more closely:
    - evaluates `ins`, `outs`, `routes`,
    - lowers through `a2sb(...)`,
    - propagates a constant `0->N` route specification into an integer list,
    - rebuilds a canonical `par(int, ...)` route spec.
  - This fixed:
    - `route0idx`-style computed route specs,
    - `bitonicBuilderLayer(...)`,
    - `bitonicSort_test`

- Multi-output seq constant-folding guard:
  - Rust no longer folds multi-output sequences by taking only the first propagated output.
  - This matches the C++ singleton-list contract used in `eval.cpp`.

- `waveform` handling:
  - Rust now keeps `BOXWAVEFORM` as a leaf in `eval` / `a2sb`, like C++.
  - This fixed:
    - `jprev_demo_test` functional mismatch,
    - stack blow-up on large inline waveforms

- `iseq(..., 0, body)` neutral element:
  - Rust now follows C++ `neutralExpSeq(...)` semantics instead of always using a `route(0,0,...)` empty neutral.
  - This fixed:
    - `mth_octave_filterbank_alt_test`

- Label interpolation:
  - Rust now truncates exact real constants to int for `%ident` label placeholders, like C++ `evalLabel -> eval2int -> tree2int`.
  - This fixed:
    - `%att` in `debug.lib`

- Pattern simplification:
  - exact integer reals are collapsed back to integer constants for case matching, aligning observed C++ behavior on route/comparator cases.
  - This fixed:
    - `bitonicSort_test`

### 3.2 Current Rust adaptations in `eval`

These are intentional representation-level adaptations, not semantic bugs:

| Area | C++ | Rust | Status |
|---|---|---|---|
| Environment representation | persistent tree-encoded layers with property tables | arena of `EnvId` layers storing `Vec<(SymId, EvalValue)>` | `adapted` |
| Closures / PM values | tree-encoded `closure(...)` / `boxPatternMatcher(...)` | `EvalValue::{Closure, PatternMatcher}` | `adapted` |
| Source loading | process-global `gReader` | explicit `EvalSourceContext` | `adapted` |
| Stats | `gGlobal->gStats` | explicit `EvalStats` | `adapted` |

These adaptations are already documented in [crates/eval/src/lib.rs](/Users/letz/Developpements/RUST/faust-rs/crates/eval/src/lib.rs).

### 3.3 Remaining eval gaps / risks

The main remaining `eval` parity risks are structural, not local:

- Import / loaded-source flow is still not a close structural port of the C++ `formatDefinitions(...)` / import boundary.
  - This remains a known architecture gap.
  - The active plan is tracked in:
    - [parser-import-format-definitions-cpp-parity-plan-2026-03-19-en.md](/Users/letz/Developpements/RUST/faust-rs/porting/parser-import-format-definitions-cpp-parity-plan-2026-03-19-en.md)

- `boxSimplification(...)` remains parity-sensitive.
  - Recent fixes showed that scalar simplification must never consume multi-output propagated lists.
  - This area is now much safer, but still deserves continued differential coverage.

- Rust `eval` is not a byte-for-byte closure port.
  - The semantics are intentionally close, but the value representation is different.

## 4. Propagate: Current Parity Status

### 4.1 What is now aligned with C++

The Rust propagation subsystem now matches the intended C++ production boundary much more closely:

- Rust propagates only the post-`eval/a2sb` first-order box subset.
  - This is explicit through [`FlatBoxId`](/Users/letz/Developpements/RUST/faust-rs/crates/propagate/src/lib.rs).
  - C++ has the same conceptual boundary, but it is implicit.

- Supported flat families now include the important production cases:
  - primitives,
  - composition algebra,
  - `route`,
  - `ffun`,
  - `soundfile`,
  - `waveform`,
  - `ondemand`,
  - `upsampling`,
  - `downsampling`

- Route propagation now follows the C++ route model:
  - nested `par(...)` route specs are flattened like C++,
  - endpoints are interpreted with the same 1-based convention,
  - direct one-to-one routes no longer inject spurious `0 + x` sums,
  - disconnected outputs become `0`,
  - multiply-driven outputs mix by addition.

The recent `route` bug sequence showed the important invariant:
- `propagate` is correct only if `eval` has already reduced the route specification to the flat constant form expected by C++.

### 4.2 Current Rust adaptations in `propagate`

These are deliberate architecture adaptations:

| Area | C++ | Rust | Status |
|---|---|---|---|
| Post-eval boundary | implicit | explicit `FlatBoxId` validation | `adapted` |
| UI ownership | backend-local / clockenv-driven effects | explicit `PropagateOutput { signals, ui }` | `adapted` |
| Entry points | raw tree flow | typed `box_arity_typed` / `propagate_typed` plus compatibility wrappers | `adapted` |
| Caching | implicit global/property style | explicit `ArityCache`, explicit DAG visited sets | `adapted` |

These are not parity failures by themselves. They are explicit Rust ownership and typing decisions around the same semantic target.

### 4.3 Remaining propagate gaps / risks

- `propagate` still depends on `eval` to fully remove evaluator-only syntax.
  - This matches the C++ pipeline conceptually,
  - but in Rust it is enforced by a stricter typed boundary.

- The grouped-UI rewrite is behaviorally aligned, but architecturally adapted.
  - This is already acknowledged in the crate docs and UI architecture contract.

- Any future route-spec or residual-form regression should be treated as an `eval`/boundary issue first, not as a `propagate`-only bug.

## 5. Concrete Differential Results

### 5.1 Route / bitonic family

The following cases were critical in the recent audit:

#### Before the fix

- `bitonicSort_test`
  - Rust signal dump collapsed to:
    - `int(0), int(0), int(0), int(0)`
  - Generated Rust C++ therefore produced incorrect constant-zero outputs.

#### After the fix

Validated results:

- `route_direct.dsp`
  - Rust now preserves the original 4 constants after `route(4,4,1,1,2,2,3,3,4,4)`.

- `route0idx_direct.dsp`
  - Rust now reduces the computed route specification to the correct canonical constant route spec.

- `bitonic_builder_layer.dsp`
  - Rust now emits the expected min/max network instead of zeros.

- `bitonicSort_test`
  - Rust now emits the expected nested `SIGMIN` / `SIGMAX` sorting network on the four sliders.

Observed remaining difference:
- Generated C++ is still not textually identical to Faust C++.
- Example:
  - Faust C++ emits temporaries like `fSlow10`, `fSlow11`, ...
  - Rust emits inline `std::fmin(...)` / `std::fmax(...)`
- This is a code-shape difference, not an `eval` / `propagate` semantic mismatch.

### 5.2 Representative commands used

- `cargo run -q -p compiler -- --dump-sig /tmp/route_direct.dsp`
- `cargo run -q -p compiler -- --dump-sig /tmp/route0idx_direct.dsp`
- `cargo run -q -p compiler -- --dump-sig /tmp/bitonic_builder_layer.dsp`
- `cargo run -q -p compiler -- --dump-sig -pn bitonicSort_test -I /Users/letz/Developpements/faustlibraries /Users/letz/Developpements/faustlibraries/tests/routes_tests.dsp`
- `faust -d -pn bitonicSort_test /Users/letz/Developpements/faustlibraries/tests/routes_tests.dsp -I /Users/letz/Developpements/faustlibraries -o /tmp/bitonic_cpp.cpp`
- `cargo run -q -p compiler -- -pn bitonicSort_test -I /Users/letz/Developpements/faustlibraries /Users/letz/Developpements/faustlibraries/tests/routes_tests.dsp -o /tmp/bitonic_rust.cpp`

## 6. Current Parity Rating

This is the current qualitative assessment for the production path:

| Area | Rating | Reason |
|---|---|---|
| `eval` production semantics | High but not complete | most known corpus regressions in this phase have been closed |
| `propagate` production semantics | High on validated flat subset | typed boundary and route support are now robust |
| Architecture identity with C++ | Medium | several explicit Rust adaptations remain |
| Differential proof coverage | Medium | strong targeted evidence, not yet exhaustive |

## 7. Precise Remaining Work

The most important remaining parity work around `eval` / `propagate` is:

1. Close parser/import/loading parity so `eval` sees a source graph closer to C++.
2. Keep expanding differential `eval` coverage on:
   - residual closures,
   - route patterns,
   - iterative combinators,
   - mixed constant simplification cases.
3. Continue to treat `propagate` bugs as boundary bugs first when residual evaluator forms survive too long.
4. Keep structural tests for multi-output constant networks so scalar simplification cannot regress again.

## 8. Immediate Parity Improvement Plan

The following work can be done immediately, without reopening large architecture decisions.

### 8.1 Priority A: tighten `eval` scalar-vs-vector simplification rules

Status:
- completed on 2026-03-19

Goal:
- ensure every helper that uses propagation for constant folding obeys the same singleton-list rule as C++.

Why:
- recent regressions (`bitonicSort_test`, multi-output `seq`, route networks) all came from scalar simplification being applied to boxes that were not truly `0->1`.

What to do now:
- audit every `eval` helper that calls `propagate_typed(...)` or `propagate_box_and_simplify(...)`.
- require an explicit `signals.len() == 1` contract anywhere the result is consumed as one scalar.
- keep separate helpers for:
  - `0->1` scalar constant evaluation,
  - `0->N` integer-list evaluation for `route(...)`,
  - non-scalar structured fallback.

Files:
- [crates/eval/src/lib.rs](/Users/letz/Developpements/RUST/faust-rs/crates/eval/src/lib.rs)

Expected parity gain:
- prevents new false constant collapses in `eval.cpp`-equivalent logic.

Completed in:
- `3ac60d8` `Align eval route-spec lowering with C++`

### 8.2 Priority A: expand differential `route(...)` coverage

Status:
- completed on 2026-03-19 for the currently identified edge cases

Goal:
- prove that Rust route evaluation matches the C++ `isBoxRoute` branch over more than one library reproducer.

Why:
- route-spec computation is a high-risk parity zone because it mixes:
  - `eval`,
  - `a2sb`,
  - constant propagation,
  - canonical list rebuilding.

What to do now:
- add focused tests covering:
  - direct constant route spec,
  - computed `0->N` route spec,
  - route specs with exact integer reals,
  - route specs that must remain symbolic in pattern contexts,
  - routes with disconnected outputs and multi-driver outputs.

Files:
- [crates/eval/tests/core_eval.rs](/Users/letz/Developpements/RUST/faust-rs/crates/eval/tests/core_eval.rs)
- [crates/propagate/src/lib.rs](/Users/letz/Developpements/RUST/faust-rs/crates/propagate/src/lib.rs)

Expected parity gain:
- closes the most recently active regression family at the right abstraction level.

Completed in:
- `3ac60d8` `Align eval route-spec lowering with C++`
- `pending` current step: exact-int reals in route specs and out-of-range endpoints

### 8.3 Priority A: add a corpus of `eval` differential micro-fixtures

Status:
- completed on 2026-03-19

Goal:
- stop discovering `eval` parity bugs only through large library DSPs.

Why:
- the bugs fixed recently were all easier to understand in tiny reduced forms:
  - residual closure arity,
  - `inputs(...)` / `outputs(...)`,
  - `waveform`,
  - `route0idx`,
  - zero-iteration `seq`.

What to do now:
- create a small dedicated parity fixture set for `eval`-level semantics:
  - route spec lowering,
  - residual apply arity,
  - `case` exact-int-vs-real matching,
  - `waveform` leaf handling,
  - neutral element of zero-iteration combinators.
- wire those fixtures to both:
  - Rust `--dump-sig`
  - Faust C++ `-norm` / `-d`

Suggested location:
- `tests/cpp_parity_known_gaps/` for current gaps
- or a new focused folder under `tests/` for closed differential micro-cases

Expected parity gain:
- moves the workflow from reactive debugging to stable differential guarding.

Completed in:
- `pending` current step: `tests/eval_micro_fixtures/` corpus and
  `crates/compiler/tests/eval_cpp_micro_differential.rs`

### 8.4 Priority B: reduce `eval` adaptation around import/loading shape

Status:
- in progress on 2026-03-19

Goal:
- make `eval` receive source graphs closer to the ones C++ actually evaluates.

Why:
- the biggest structural parity weakness currently identified is not inside `eval` dispatch itself, but in the parser/import/loading boundary upstream.

What to do now:
- start executing the existing plan:
  - [parser-import-format-definitions-cpp-parity-plan-2026-03-19-en.md](/Users/letz/Developpements/RUST/faust-rs/porting/parser-import-format-definitions-cpp-parity-plan-2026-03-19-en.md)
- short-term first slice:
  - differential fixtures where duplicated imported aliases currently produce Rust-only shapes,
  - then move import/definition flow closer to C++ `formatDefinitions(...)`.

Expected parity gain:
- removes a whole class of Rust-only evaluator edge cases instead of patching them one by one.

Started in:
- `pending` current step: external differential tracker on `faustlibraries/tests/dx7_tests.dsp`
  `operator_test`

### 8.5 Priority B: audit `a2sb(...)` memoization boundaries

Goal:
- ensure Rust memoization matches the observable C++ `gSymbolicBoxProperty` behavior without over-reusing lowered nodes across semantically distinct contexts.

Why:
- several past regressions were caused by shared residual forms being lowered incorrectly or too late.

What to do now:
- review every call site that assumes `a2sb(...)` is safe on a shared residual form.
- keep explicit tests for:
  - reused residual closures,
  - reused pattern matchers,
  - reused route-spec builders,
  - reused apply-time arity probes.

Files:
- [crates/eval/src/lib.rs](/Users/letz/Developpements/RUST/faust-rs/crates/eval/src/lib.rs)

Expected parity gain:
- lowers the risk of DAG-sharing regressions in the highest-leverage evaluator helper.

### 8.6 Priority C: continue shrinking the semantic surface of `propagate`

Goal:
- keep `propagate` as a strict consumer of the post-`eval/a2sb` flat subset, not a compensating phase.

Why:
- recent debugging confirmed that many apparent `propagate` bugs were actually upstream evaluation-boundary failures.

What to do now:
- keep rejecting non-flat residual forms at `FlatBoxId` validation.
- when a new failure appears in `propagate`, first ask:
  - should `eval` have normalized this already?
- only add true `propagate` support for families that are valid post-`eval/a2sb` in C++.

Files:
- [crates/propagate/src/lib.rs](/Users/letz/Developpements/RUST/faust-rs/crates/propagate/src/lib.rs)

Expected parity gain:
- preserves a clean C++-like phase contract and avoids Rust-only repair logic in propagation.

### 8.7 Recommended next concrete sequence

This is the suggested immediate execution order:

1. Finish the `route(...)` differential matrix in `eval` and `propagate`.
2. Add micro-fixtures for the already-fixed `eval` parity families.
3. Start the parser/import flow parity plan, because it is now the largest identified structural gap affecting `eval`.
4. Continue targeted C++/Rust audits on residual closure and arity-probing cases after the parser/import work begins.

## 9. Bottom Line

Precise result of this analysis:

- `eval` and `propagate` are no longer the main known parity blockers on the active production route corpus.
- They are not architecturally identical to C++, but they are much closer semantically than before.
- The largest remaining structural parity weakness is now upstream of them:
  - parser/import/loading shape,
  - not the current `route`/`bitonic` semantics themselves.

For current maintenance, the correct interpretation is:
- `eval` / `propagate` parity is now good enough to support production debugging and continued corpus closure,
- but still requires continued differential auditing rather than being considered mathematically closed.
