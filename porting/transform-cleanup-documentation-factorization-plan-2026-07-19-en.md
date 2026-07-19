# `crates/transform` cleanup, documentation, and factorization plan

**Date:** 2026-07-19

**Baseline:** `c70f97d9c0e5f00782ca65e61b4373b6d92a5273` (`main-dev`; first
measured at `e8c49891`, re-measured after the X2b/X3 qualification, the
clocked-vector ZeroPad/guard fixes, and the C-family condition cleanup landed)

**Status:** proposed

**Scope:** structural cleanup and documentation of `crates/transform`; no
intentional change to signal semantics, scheduling semantics, vector admission,
FIR shape, generated code, or public compiler behavior.

Related documents:

- [`signal-to-fir-transform-analysis-2026-06-20-en.md`](signal-to-fir-transform-analysis-2026-06-20-en.md)
- [`signal-to-fir-lower-struct-decomposition-plan-2026-06-20-en.md`](signal-to-fir-lower-struct-decomposition-plan-2026-06-20-en.md)
  (complete; do not repeat its `SignalToFirLower` state extraction)
- [`delay-rs-simplification-experiment-2026-06-21-en.md`](delay-rs-simplification-experiment-2026-06-21-en.md)
  (complete for the current delay layout)
- [`vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md`](vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md)
- [`scheduling-vectorization-implementation-review-2026-07-16-en.md`](scheduling-vectorization-implementation-review-2026-07-16-en.md)
- [`vector-plan-contexts-divergence-plan-2026-07-18-en.md`](vector-plan-contexts-divergence-plan-2026-07-18-en.md)
- [`factorization-god-files-plan-2026-05-25-en.md`](factorization-god-files-plan-2026-05-25-en.md)
- [`../AGENTS.md`](../AGENTS.md) (contribution conventions; commit and journal
  discipline in §7 follows it)

## 1. Executive decision

`transform` should be refactored, but not as one broad rewrite. The scalar
lowerer and the delay subsystem were already substantially decomposed in June.
The new concentration of complexity is the checked vector pipeline added in
July. Its large files combine four different reasons to change:

1. versioned artifact data models;
2. artifact producers;
3. independent checkers;
4. large in-file test suites and fixtures.

The safe sequence is therefore:

1. freeze behavior and the compatibility surface;
2. correct the architecture documentation;
3. move tests without changing production code;
4. introduce shared *vocabulary-only* modules;
5. split each producer/checker pair while preserving independent derivation;
6. clean imports, names, and visibility only after the new boundaries compile;
7. enforce the resulting documentation and size rules mechanically.

The refactor must remain FIR- and output-neutral. Any observed golden, vector
coverage, certificate, or runtime change is a defect in this work unless it is
split into a separately approved semantic change.

## 2. Measured current state

The following measurements were taken at the baseline commit.

| Area | Rust lines | Observation |
|---|---:|---|
| complete crate | 56,704 | now one of the largest middle-end crates |
| `signal_fir/` | 48,950 | 86% of the crate |
| `signal_fir/vector/` | 27,253 | 48% of the crate; main new complexity center |
| `signal_fir/module/` | 6,964 | scalar lowerer already split by concern |
| `signal_fir/delay/` | 2,243 | June refactor is still structurally sound |
| `signal_prepare/` | 2,849 | implementation is cohesive; tests are not split |
| `schedule/` | 2,479 | good reference layout, including split tests |
| `clk_env/` + `hgraph/` | 2,390 | cohesive analysis modules with strong headers |

Largest files:

| File | Lines | Main issue |
|---|---:|---|
| `signal_fir/tests.rs` | 4,324 | unrelated scalar fixtures and assertions in one file |
| `vector/assemble.rs` | 3,409 | model + materializer + checker + tests |
| `vector/events.rs` | 3,324 | model + producer + independent checker + tests |
| `vector/lower.rs` | 3,000 | lowering + boundary verification + tests |
| `vector/verify.rs` | 2,982 | plan DTOs + error taxonomy + checker + 42 tests |
| `vector/analysis.rs` | 2,884 | conditions + dependencies + effects + uses + tests |
| `vector/state.rs` | 2,468 | model + producer + checker + executable simulator + tests |
| `vector/plan.rs` | 2,204 | placement + fusion + effects + reachability + tests |
| `vector/route.rs` | 2,066 | route model + mutable session + independent checker + tests |
| `vector/module.rs` | 1,851 | final assembly + lifecycle + final checker + tests |
| `loop_graph.rs` | 1,793 | legacy/transitional graph plus FIR splitting and tests |
| `vector/clock_ad.rs` | 1,380 | model + producer + shared verify (guards) + simulator + tests |

These figures move with every correctness fix — `lower.rs`, `state.rs`, and
`clock_ad.rs` each grew during the July clocked-vector work between the two
baseline measurements. R0 must therefore re-snapshot the metrics at its actual
freeze commit rather than reuse this table.

Validation baseline:

- `cargo test -p transform --lib`: **385 passed, 0 failed**.
- `cargo rustdoc -p transform --lib -- -D missing-docs`: fails with **799**
  missing-documentation diagnostics and eight Rustdoc warnings.
- Vector coverage: **96 of 132 corpus DSPs certified in all 16
  `f32/f64 x -lv 0/1 x -ss 0..3` modes**; `vector-coverage-check` retains
  1,536 certified mode/DSP pairs.
- Workspace production consumers use the scalar/vector selection entry points,
  the option/status enums, and `SchedulingStrategy`. Most vector artifact APIs
  are consumed only by tests in `crates/compiler`.

These numbers are baselines, not targets by themselves. A small file can still
be incoherent, and an independent checker may legitimately repeat logic that a
normal application would factor away.

## 3. Current architecture and invariants to preserve

### 3.1 Pipeline ownership

The crate currently owns two connected paths:

```text
propagated signal forest + UiProgram
              |
              v
signal_prepare -> VerifiedPreparedSignals
              |
              +---------------- scalar -----------------+
              |                                          |
              |             clk_env / hgraph / schedule  |
              |                         |                |
              |                         v                |
              |                 scalar SignalToFirLower  |
              |                                          |
              +---------------- vector -----------------+
                                        |
        analysis -> decorations -> VectorPlan -> state/clock policy
                                        |
                route -> lower -> event certificate -> FIR assembly
                                        |
                             final module verification
                                        |
                                        v
                                  SignalFirOutput
```

`schedule` is shared infrastructure. `clk_env` and `hgraph` provide scalar
clock-domain and scheduling facts. `signal_prepare` is an arena-owning staging
boundary. `signal_fir` owns both scalar lowering and the checked vector
selection/fallback policy.

### 3.2 Trust-boundary chain

The vector design intentionally uses opaque `Verified*` wrappers. Refactoring
must preserve these boundaries and their construction rules:

| Artifact | Producer responsibility | Checker responsibility |
|---|---|---|
| `VerifiedPreparedSignals` | normalize/canonicalize/type the forest | recheck staging postconditions |
| `VerifiedDecorationCertificate` | record exact signal facts | recompute source-aligned decorations |
| `VerifiedVectorPlan` | place signals, loops, transports, epochs | independently validate coverage, order, effects, witnesses |
| `VerifiedVectorStatePlan` | derive delay/recursion/UI state phases | independently validate geometry and phase coverage |
| `VerifiedVectorClockAdPlan` | derive clock islands and AD policy | independently validate source/domain alignment |
| `VerifiedRoutedFir` | materialize region-local definitions and transports | independently inspect FIR evidence |
| `VerifiedPureVectorProgram` | lower signal closures | verify bodies against route and plan evidence |
| `VerifiedEventOrderCertificate` | construct scalar/vector dynamic orders | independently reconstruct required events and dependencies |
| `VerifiedVectorFirAssembly` | assemble loops, state, and islands | independently inspect exact assembly coverage |
| final module | add outputs, lifecycle and chunk driver | FIR checker plus vector-specific final checks |

The checker must not call the producer, reuse a producer cache, or accept a
producer-derived expected result as evidence. Moving two algorithms into one
file is harmless; making one call the other is not.

### 3.3 Semantic contracts that are frozen during this work

- deterministic ordering for every schedule and serialized certificate;
- exact producer/checker schema versions and stable diagnostic codes;
- fail-closed vector fallback reasons and effective-mode reporting;
- scalar/vector bit-exactness and scalar fallback behavior;
- delay geometry, recursion simultaneity, clock-domain state ownership, UI and
  table effect attribution;
- the July clocked-vector correctness semantics: `ZeroPad` fire-index gating
  in the pure lowerer, cross-group symbolic back-edges served from accepted
  P6.1 history, `attach` as an `Effect`-kind ordering edge, and the
  `UnadoptedStatefulRead` fail-closed rejection of fire-time reads of
  audio-rate stateful producers;
- C++ lifecycle ordering and authoritative compiled initialization;
- `RealType`, external `FaustFloat`, integer lowering, and cast boundaries;
- public compiler facade behavior and C/FFI caller behavior;
- optimized/unoptimized runtime parity on the existing representative subset.

## 4. Findings

### 4.1 Documentation has fallen behind the implementation

The crate README and `lib.rs` still describe scheduling/vectorization as future
or not production-active. `signal_fir/mod.rs` is a chronological list of
`Step 2A..2H`, `P5`, `P6`, and RAD development slices. Some statements say an
artifact is not selected by `build_module` even though later bullets describe
its production activation. This is history, not a readable current contract.

The vector leaf modules have valuable provenance comments, but many are
phase-number centric. The reader has to consult old plans to determine current
input, output, invariant, and caller. `vector/mod.rs`, the natural architecture
entry point, currently contains only one sentence and temporary alias wiring.

Public DTO fields and error payloads are insufficiently documented. The
`missing-docs` experiment reports 794 failures. Enabling the lint immediately
would create a large noisy patch; the visibility and module cleanup should
precede enforcement.

### 4.2 The vector namespace exposes implementation history

`signal_fir/mod.rs` exposes both `signal_fir::vector::*` and aliases such as
`signal_fir::vector_plan`, `vector_verify`, and `vector_clock_ad`. Internal
vector modules then preserve old names again (`vector_analysis`,
`vector_route`, and so on). This doubles the vocabulary and makes imports show
migration history instead of ownership.

Recommendation: use `signal_fir::vector::{analysis, plan, ...}` internally and
through workspace tests. Keep compatibility re-exports during this refactor.
Removing public aliases or reducing public visibility is an API decision and
requires an explicit compatibility decision and mapping record before landing.

### 4.3 Large files mix data, construction, checking, and tests

The worst files are not merely long. For example:

- `analysis.rs` owns execution-condition DNF, dependency semantics, occurrence
  semantics, effect vocabulary, effect propagation, use tables, scalar fast
  analysis, full vector analysis, and tests.
- `verify.rs` is both the canonical `VectorPlan` data model and its independent
  checker.
- `events.rs` contains its public certificate model, producer, independent
  event-table reconstruction, dependency checking, and extensive fixtures.
- `assemble.rs` both emits FIR and independently inspects the emitted FIR.
- `state.rs` combines a versioned artifact, its producer/checker, and generic
  executable delay simulators.
- `lower.rs`, `route.rs`, and `module.rs` similarly combine construction and
  verification responsibilities.

The factorization axis must be *reason to change*, not arbitrary line count.

### 4.4 Tests hide the production structure

`signal_fir/tests.rs` covers options, typed errors, UI, tables, delay policies,
recursion, placement, reverse AD, and lowering coverage. Its 4,324 lines make
feature ownership hard to see. `signal_prepare/tests.rs` has the same issue at
smaller scale. Most large vector files dedicate hundreds of lines to embedded
fixtures and mutation tests.

`schedule/tests/` already demonstrates the desired layout: a small `mod.rs`,
shared fixtures, and files grouped by contract.

### 4.5 Wildcard imports conceal scalar-module coupling

Every scalar lowering leaf (`arithmetic.rs`, `bra.rs`, `build.rs`,
`clocked.rs`, `core_lowering.rs`, `setup.rs`, `state.rs`, `tables.rs`, and
`ui_lowering.rs`) starts from `use super::*`. The June state decomposition
reduced `SignalToFirLower`, but wildcard imports still make each leaf appear to
own the entire parent namespace. Explicit imports will reveal real coupling and
make later extraction safer.

This change should happen after file moves; doing it first produces churn that
is hard to review and likely to conflict with structural patches.

### 4.6 Some duplication is removable; some is assurance

Safe candidates for shared, pure, total utilities include:

- prepared-signal ID indexing (currently repeated in analysis/state/clock/AD
  paths);
- canonical `ValueType`/FIR-type conversion and zero-value construction;
- exhaustive FIR child traversal and statement-containment queries (the
  current typed walkers — `fir_children` in `lower.rs`, `fir_reachable` in
  `assemble.rs` — silently skip unknown node kinds, which already hid a
  transport from the body verifier once during E2; the shared primitive must
  fail on an unclassified `FirMatch` variant instead);
- canonical comparison/key helpers and checked integer conversions;
- test-only certificate and DSP fixtures.

The following should remain independently implemented even when similar:

- producer and checker reachability/transitive-closure calculations;
- producer and checker effect summaries;
- expected event/dependency reconstruction;
- expected state transitions and assembly coverage;
- producer scheduling and checker order validation.

A shared semantic predicate such as `effects_conflict` is acceptable as a
domain axiom. A shared function that produces the expected plan or certificate
is not.

### 4.7 Older subsystems do not need another large rewrite

- The seven `SignalToFirLower` sub-state extractions are complete.
- The delay planner/manager/strategy split is complete and coherent.
- `signal_prepare` has a clear `Staging` driver and verification boundary.
- `clk_env` and `hgraph` are long but cohesive and well documented.
- `schedule` is already well factored and should be used as the test-layout
  model.

Work in those areas should be limited to documentation, test relocation,
explicit imports, and small proven utility extraction unless a new measured
problem appears.

### 4.8 Shared verify paths carry obligations that a build/check split can drop

Several stages enforce admission guards inside one function that both the
producer and the independent checker call. Concrete example: the clock plan's
`reject_unadopted_stateful_reads` runs inside
`verify_vector_clock_ad_plan_after_vector_plan`, which `build_vector_clock_ad_plan`
calls at the end of production and `verify_vector_clock_ad_plan` calls on
replay. This is currently correct precisely because the path is shared. When
R6 separates `build.rs` from `check.rs`, each such guard must demonstrably
remain on **both** paths — a split that keeps the guard only on the build side
silently weakens the checker, and only on the check side delays rejection past
production. The split commits must list every guard in the moved verify
function and add or keep a rejection test that fails through the checker
entry point alone.

## 5. Target structure

This is a responsibility map, not a requirement to create every file in one
commit. Preserve existing public module paths with facade `mod.rs` files and
re-exports while moving implementations behind them.

```text
transform/src/
  lib.rs
  clk_env/{mod.rs, tests.rs}
  hgraph/{mod.rs, tests.rs}
  schedule/                         # keep current shape
  signal_prepare/
    mod.rs
    rewrites.rs
    verify.rs
    tests/{mod.rs, staging.rs, typing.rs, recursion.rs, verify.rs}
  signal_fir/
    mod.rs                          # current architecture + public facade
    error.rs
    scalar/ or existing module/     # do not rename until vector cleanup lands
    delay/                          # keep current shape
    tests/
      mod.rs
      contract.rs
      ui_tables.rs
      delays.rs
      recursion.rs
      placement.rs
      reverse_ad.rs
      coverage.rs
    vector/
      mod.rs                        # pipeline map and compatibility facade
      common/                       # vocabulary-only helpers
        ids.rs
        types.rs
        fir_walk.rs
      analysis/
        mod.rs
        conditions.rs
        dependencies.rs
        effects.rs
        uses.rs
        tests/
      plan/
        mod.rs
        build.rs
        fusion.rs
        producer_reachability.rs
        tests.rs
      verify/
        mod.rs
        model.rs                    # canonical DTO vocabulary
        error.rs
        check.rs
        fused_groups.rs
        checker_reachability.rs
        tests/
      state/
        mod.rs
        model.rs
        build.rs
        check.rs
        simulation.rs
        tests.rs
      clock_ad/{mod.rs, model.rs, build.rs, check.rs, simulation.rs, tests.rs}
      route/{mod.rs, model.rs, session.rs, check.rs, tests.rs}
      lower/{mod.rs, program.rs, signal.rs, tables.rs, check.rs, tests.rs}
      events/{mod.rs, model.rs, produce.rs, check.rs, tests.rs}
      assemble/{mod.rs, model.rs, materialize.rs, check.rs, tests.rs}
      module/{mod.rs, build.rs, outputs.rs, lifecycle.rs, check.rs, tests.rs}
```

The exact split should follow borrow and data ownership. Avoid one-function
files and avoid generic `utils.rs`. Every module name must describe a domain
concept or a trust-boundary role.

## 6. Implementation phases

### R0 — Freeze the baseline and decide the API boundary

Deliverables:

1. Record baseline HEAD, re-measured file metrics, the transform unit-test
   count (385 at this plan's baseline), golden status, vector coverage status
   (96/132 in 16 modes, 1,536 retained pairs at this plan's baseline), and
   representative compile budget in the daily journal.
2. Inventory every `transform` item used outside the crate and classify it:
   - stable compiler/FFI contract;
   - public diagnostic/testing surface;
   - internal implementation detail accidentally public.
3. Add the required public API mapping (`1:1`, `adapted`, or `deferred`) for
   touched surfaces to this plan or the journal.
4. Choose explicitly whether old `vector_*` aliases are compatibility API.
   Default for this plan: migrate workspace users but retain facade re-exports;
   do not silently break external paths.
5. Capture a representative FIR dump/generated-code hash basket for scalar,
   vector-certified, and scalar-fallback cases. The proven mechanism is a
   frozen worktree: `git worktree add <dir> <baseline>`, build its release
   compiler once, and diff emissions (`-lang cpp -double`, both `-lv`
   variants, per certified corpus DSP) against the working tree after every
   structural milestone. Byte identity is the gate; any difference must be
   reduced, explained, and oracle-arbitrated before the commit lands — the
   X2/X2b qualifications used exactly this procedure.

Pass criteria:

- `cargo test -p transform --lib` is green;
- `cargo run -p xtask -- golden-check` is green;
- `cargo run -p xtask -- vector-coverage-check` is green;
- no implementation edit has landed yet;
- any API contraction is explicitly approved before implementation.

### R1 — Rewrite documentation as current-state architecture

Make documentation changes before file movement so reviewers can compare the
new structure with an agreed model.

1. Rewrite `crates/transform/README.md`:
   - all five public modules, not only `signal_prepare`/`signal_fir`;
   - scalar and checked-vector pipeline diagrams;
   - stable vs diagnostic/experimental API classification;
   - fallback policy and lifecycle ownership;
   - links to the active plans and current validation commands.
2. Rewrite `src/lib.rs` to describe the current production role. Remove claims
   that scheduling/vectorization are merely planned.
3. Replace the chronological header in `signal_fir/mod.rs` with:
   - inputs and outputs;
   - scalar/vector selection;
   - preparation and verification boundaries;
   - fallback semantics;
   - a concise module map;
   - current known unsupported behavior.
   Keep development history in `porting/` and the journal.
4. Expand `vector/mod.rs` into the authoritative artifact-flow map. For every
   stage document input, output, invariant, producer, checker, and C++ source
   provenance/adaptation.
5. Convert leaf headers from phase logs into current contracts. Phase numbers
   may remain as links, not as the primary explanation.
6. Fix Rustdoc private links, redundant explicit links, and stale path links.
7. Document stable public items first: entry points, options, outputs, errors,
   diagnostic status, `SchedulingStrategy`, and verified wrapper semantics.

Pass criteria:

- `cargo doc -p transform --no-deps` completes without Rustdoc warnings;
- README and crate/module docs agree on active scalar and vector paths;
- no code or test behavior changes;
- source provenance and API mapping status are present for every touched public
  API.

### R2 — Split tests and fixtures, with zero production movement

1. Convert `signal_fir/tests.rs` to `signal_fir/tests/` grouped by contract,
   UI/tables, delay, recursion, placement, BRA, and coverage.
2. Convert `signal_prepare/tests.rs` to stage/typing/recursion/verification
   groups.
3. Move embedded vector tests into per-stage `tests/` modules. Keep private
   access through child test modules; do not widen production visibility for
   tests.
4. Extract genuinely shared test builders to narrowly named fixture modules.
   Do not create a global fixture bag.
5. Preserve every test name where practical so CI history remains readable.

Pass criteria:

- exactly the R0-recorded transform unit-test count remains (385 at this
  plan's baseline) unless an explicit test-only gap is added and documented;
- `cargo test -p transform --lib` remains green after every move;
- production `.rs` bodies are unchanged apart from `#[cfg(test)] mod tests;`;
- no `pub` visibility is added to satisfy relocated tests.

### R3 — Normalize the vector namespace without breaking compatibility

1. Migrate internal imports from `vector_analysis`, `vector_plan`, etc. to
   `vector::{analysis, plan, ...}` ownership paths.
2. Migrate workspace tests to the grouped namespace.
3. Keep old public aliases as facade re-exports for this refactor unless R0
   explicitly authorized removal.
4. Remove aliases that are strictly private and have no caller.
5. Use `pub(crate)`/`pub(super)` for new implementation seams. Do not expand
   public API during file splitting.

Pass criteria:

- one canonical namespace is used inside `transform`;
- compatibility paths selected in R0 still compile;
- no stable diagnostic code, schema path, or compiler facade changes.

### R4 — Extract shared vocabulary and total primitives

Create only helpers that do not collapse a trust boundary.

1. Move canonical plan/state/route/event/assembly DTO definitions into model
   modules and re-export them from their old paths.
2. Extract prepared-ID indexing with checked conversion and duplicate-ID
   rejection.
3. Extract canonical `ValueType` to FIR-type conversion and zero-value creation
   where the policy is truly identical. If one caller has different admission
   semantics, keep separate wrappers around a shared total conversion.
4. Introduce an exhaustive FIR child traversal/query primitive. Prefer placing
   a generally useful traversal in `crates/fir`; if scope is kept local, add an
   exhaustiveness test that fails when a new `FirMatch` variant is unclassified.
5. Share domain axioms (`effects_conflict`, canonical keys), not expected
   producer results.
6. Add structural tests for each extracted representation-level adaptation.

Pass criteria:

- producer/checker modules share immutable DTO vocabulary only;
- checkers do not call producers or producer reachability/effect-summary code;
- all helper APIs are total or return typed errors;
- no new `unwrap`/`expect` is used across phase boundaries;
- FIR and generated-output snapshots remain identical.

### R5 — Split vector analysis, plan, and plan verification

Do this in small commits, with file relocation separate from logic cleanup.

1. `analysis`: split conditions, dependency/occurrence rules, effects, and use
   aggregation. Keep one facade that exposes the existing API.
2. `verify`: split canonical plan DTOs, error taxonomy, base checker, fused
   group checker, lockstep checker, and independent reachability.
3. `plan`: split placement, ordinary edges, effect orientation, fused serial
   groups, lockstep integration, and producer-only reachability.
4. Document intentional producer/checker duplication adjacent to both
   implementations.
5. Keep schema version constants and serialization field order unchanged.

Pass criteria:

- no resulting production file should normally exceed about 1,200 lines;
  exceptions require a written cohesion rationale, not mechanical splitting;
- every error variant and mutation-rejection test remains reachable;
- plan JSON/certificate bytes and hashes are unchanged;
- all four scheduling strategies retain their existing accepted orders and
  invariants.

### R6 — Split state, clock/AD, routing, and lowering

1. `state`: separate model, producer, checker, delay simulation, and recursion
   simulation. Simulators remain executable reference models, not lowering
   helpers.
2. `clock_ad`: separate clock/AD model, producer, checker, and simulation.
3. `route`: separate route DTOs, mutable `VectorRouteSession`, FIR evidence
   builder helpers, and independent checker.
4. `lower`: separate orchestration, core signal lowering, table/UI lowering,
   type handling, and final body checker. Preserve region-local caches and
   scope ownership.
5. Review each context struct after the split. Group fields only when they have
   one lifecycle and invariant; do not create a generic mega-context.

Pass criteria:

- verified wrappers can still be constructed only by their producer/checker
  boundary;
- route caches cannot escape their region;
- state and clock simulators still pass exhaustive/bounded tests;
- scalar/vector interpreter comparisons stay bit-exact for the retained
  vector corpus.

### R7 — Split events, FIR assembly, and final module assembly

These are the highest-risk moves because they encode the final assurance
boundary.

1. `events`: separate certificate DTO, producer order construction, independent
   expected-event reconstruction, dependency checker, and tests.
2. `assemble`: separate artifact DTO, state/clock materialization, top-level
   assembly, and independent FIR inspection.
3. `module`: separate pipeline orchestration, output materialization, lifecycle
   assembly, chunk drivers, final module checks, and fallback mapping.
4. Keep producer and checker call graphs visibly disjoint. Add a code comment
   and a structural test or simple source-level guard if necessary to prevent a
   checker from calling a producer entry point. Apply the §4.8 rule here and in
   R6: every admission guard living in a shared verify function must remain
   reachable from both the producer's terminal verification and the standalone
   checker after the split, each covered by a rejection test.
5. Preserve complete and compact event-certificate limits and exact versioned
   schemas.

Pass criteria:

- complete and compact event certificates are byte-identical;
- all mutation-rejection tests still fail for the same error classes;
- lifecycle conformance remains green;
- certified/fallback counts and stable reason codes are unchanged;
- vector compile-budget change is within measurement noise or explained by a
  measured improvement.

### R8 — Clean the scalar side and smaller modules

Only after the vector layout stabilizes:

1. Replace `use super::*` in scalar module leaves with explicit imports.
2. Review `bra.rs`, `build.rs`, and `core_lowering.rs` for one additional split
   only if explicit imports reveal a real independent responsibility.
3. Keep the completed `SignalToFirLower` sub-state decomposition and delay
   architecture; do not churn them for line-count symmetry.
4. Correct small panic/expect sites that cross phase boundaries. Retain local
   invariant assertions where construction and use are in the same trusted
   function, and document the invariant.
5. Retire `pv_slice`/shadow or transitional code only under its dedicated
   parity/coverage gate. Moving files is not authorization to delete an
   experimental or diagnostic surface.

Pass criteria:

- leaf imports state their actual dependencies;
- no circular module dependency is introduced;
- no public API or transitional path is removed without its explicit gate;
- scalar generated FIR and golden output are unchanged.

### R9 — Enforce the new quality contract

1. Finish documentation for the public surface selected in R0.
2. Enable `#![warn(missing_docs)]` for `transform`, or an equivalent CI
   Rustdoc command, only when the warning set is clean.
3. Add a lightweight structural check for:
   - stale legacy internal `vector_*` imports;
   - production files above the agreed review threshold;
   - checkers importing producer entry points;
   - Rustdoc warnings.
4. Update `crates/transform/README.md`, this plan's status, the daily journal,
   and `porting/HANDOFF.md`.
5. Record any intentionally retained duplication with its assurance rationale.

Pass criteria:

- `cargo rustdoc -p transform --lib -- -D missing-docs` is green;
- the structural checks are deterministic on Linux, macOS, and Windows;
- no absolute checkout path is written to versioned reports;
- the full validation matrix below is green.

## 7. Commit strategy

Use small, bisectable commits. Recommended pattern:

1. documentation current-state rewrite;
2. scalar tests split;
3. prepare tests split;
4. one vector test-suite relocation per stage or cohesive group;
5. namespace migration with compatibility facade;
6. one vocabulary/helper extraction per commit;
7. one production module split per commit;
8. explicit-import cleanup per subsystem;
9. missing-docs enforcement and final documentation.

Do not combine a file move with semantic simplification. First move with
history-preserving content; then simplify in a separately reviewable commit.
Each notable commit receives a top entry in the current daily journal file,
including source commit and validation run.

## 8. Validation matrix

### Per mechanical/test move

```bash
cargo fmt --all
cargo test -p transform --lib
```

### Per production-module split

```bash
cargo fmt --all
cargo clippy -p transform --all-targets -- -D warnings
cargo test -p transform --all-targets
cargo test -p compiler --test vector_mode
cargo test -p compiler --test ondemand_pipeline
```

Run the focused test target matching the moved artifact as well (plan, route,
state, event, assembly, or module verification).

### At every milestone R1–R9

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run -p xtask -- golden-check
cargo run -p xtask -- vector-coverage-check
```

For R6–R9 also run:

```bash
cargo run --release -p xtask -- vector-compile-budget-check
make -C tests/impulse-tests backend-matrix-smoke
```

Before declaring completion, run the full relevant impulse/backend matrix when
the local toolchains are available. If a required external tool is unavailable,
record the exact missing dependency and the unrun gate; do not describe the
result as fully green.

Two recorded harness traps apply to every impulse run in this plan: delete the
relevant `ir/<mode>/` outputs before the run (the make-driven harness reports
green from cached `.ir` files) and assert the `filesCompare` invocation count
matches the number of DSPs exercised. Structural certification is never
numeric proof; where a refactor commit produces any emission difference, the
byte-identity gate of R0.5 hands the decision to the impulse oracle, not to
the pipeline's own checkers.

## 9. Review checklist

- [ ] Current-state docs replaced phase/changelog narratives.
- [ ] C++ provenance and Rust adaptation status remain attached to code.
- [ ] Stable, diagnostic, and internal APIs are classified.
- [ ] Compatibility aliases were retained or explicitly approved for removal.
- [ ] Tests are grouped by contract and no production visibility was widened.
- [ ] Versioned DTOs are separate from producers and checkers.
- [ ] Producers and checkers have disjoint derivation logic.
- [ ] Shared utilities are vocabulary/total traversal, not shared expected facts.
- [ ] Scalar and vector FIR/output snapshots are unchanged.
- [ ] Vector certified/fallback counts and error codes are unchanged.
- [ ] Lifecycle, clock-state, recursion, table/UI effects, and AD policies are unchanged.
- [ ] Full Rustdoc, clippy, test, golden, vector coverage, budget, and smoke gates pass.
- [ ] Daily journal and handoff describe the final module map.

## 10. Completion definition

This plan is complete when a contributor can start at `transform`'s README,
follow the scalar or vector pipeline through modules whose names match their
responsibilities, distinguish every producer from its independent checker,
find tests by contract, and build warning-free Rustdoc—while all pre-refactor
FIR, generated output, certificates, fallback decisions, runtime traces, and
public compiler behavior remain unchanged.
