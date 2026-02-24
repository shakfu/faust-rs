# FIR Function Inliner Plan (Rust, Module-Level)

## 0. Current Implementation Status (as of 2026-02-23, sessions 15–19)

The Rust FIR inliner has progressed beyond the original "design only" stage.
`crates/fir/src/inliner.rs` now contains a working staged implementation for
Phases A–E (analysis, hygienic clone, parameter materialization, one-pass
callsite rewriting, and iterative/fixpoint driving).

### 0.1 Implemented today in `crates/fir/src/inliner.rs`

Implemented public APIs (current names):

- Analysis / candidate selection:
  - `analyze_fir_inliner(...)`
  - `FirInlineOptions`
  - `FirInlineAnalysis`, `FirFunctionSummary`, `FirInlineScc`
  - `FirInlineCandidateDecision`, `FirInlineSkipReason`
- Hygienic clone / rename substrate:
  - `clone_fir_hygienic(...)`
  - `clone_fir_hygienic_with_state(...)`
  - `FirHygienicCloneState`, `FirHygienicCloneOptions`
  - `FirHygienicCloneResult`, `FirLocalRename`, `FirLocalRenameKind`
- Parameter materialization + `kFunArgs` substitution:
  - `prepare_callee_body_for_inlining(...)`
  - `FirPreparedInlineBody`, `FirMaterializedArgBinding`
- One-pass callsite rewrite:
  - `inline_fir_module_once(...)`
  - `FirInlineRewriteStats`, `FirInlineRewriteError`
- Iterative/fixpoint module driver (Phase E):
  - `inline_fir_module(...)`
  - `FirInlineFixpointStats`, `FirInlineFixpointStopReason`

What works today:

- direct `FunCall` inlining from same-module `DeclareFun` bodies
- SCC-based analysis and recursive-SCC skipping (default)
- hygienic renaming of local names in cloned callee bodies
- left-to-right argument materialization (conservative: materialize all args)
- statement splicing + return-value extraction for canonical callee bodies
- iterative reruns until fixpoint / iteration cap / expansion budget
- checker-driven validation in unit tests (`verify_fir_module` on rewritten FIR)

### 0.2 Current known limitations (important)

The implementation is intentionally conservative and not yet "complete" in the
strongest compiler-inliner sense.

- Body-shape limitation:
  - only canonical callee bodies with top-level `Block(..., Return(Some(v)))`
    are inlined
  - non-canonical returns (multiple returns, earlier returns in prefix, etc.)
    are skipped
- Rewrite coverage limitation:
  - some statement forms (notably loop/switch internals) are currently cloned
    hygienically but not recursively rewritten for nested `FunCall` inlining
- No recursive inlining by default:
  - recursive/self-recursive SCCs are skipped unless policy changes later
- Profitability is v1-simple:
  - body node threshold + `is_inline`/policy filters only
  - no "called once" heuristics, no dynamic cost model
- Argument materialization is conservative:
  - all actual arguments are materialized into temps (safe but not minimal)
- No cleanup pass yet (Phase F):
  - dead helper functions / dead prototypes may remain after inlining
- Not integrated in `crates/compiler` yet:
  - no CLI/compiler flag and no automatic pre-codegen inliner pipeline stage

### 0.3 What remains to reach the "complete" goal in this plan

Priority remaining work:

1. Compiler integration (planned milestone gate)
- run inliner before codegen in `crates/compiler`
- add opt-in options/flags and post-inline FIR checker validation
- corpus smoke testing for regressions

2. Broaden rewrite legality/coverage
- support more callee return/control-flow shapes (or canonicalization helper)
- recursively rewrite nested callsites in currently cloned-only statement forms
  (loops/switch/while subtrees)

3. Optional cleanup pass (Phase F)
- remove unused helper functions/prototypes after inlining (FIR→FIR cleanup)

4. Heuristics improvements (later)
- trivial-argument fast path (avoid materializing all args)
- better profitability model / called-once heuristics
- optional recursive policy extensions

## 1. Goal

Design and implement a **complete `FunctionInliner`** for the Rust FIR module IR
(`crates/fir`) that can:

- inline FIR function calls (`FunCall`) using `DeclareFun` bodies in the same FIR `Module`,
- preserve FIR semantics (evaluation order, scope, access kinds, types),
- run as a **module-level FIR→FIR transformation pass**,
- produce FIR that can be validated by the existing verifier (`crates/fir/src/checker.rs`).

This plan is based on:

- existing C++ Faust work started in `compiler/generator/fir_to_fir.cpp/.hh`,
- standard compiler inliner functionality (legality + profitability + call-graph handling),
- Rust FIR constraints and current checker capabilities.

## 2. Reference Baseline (C++ Faust)

### 2.1 Existing C++ implementation fragments

The current C++ FIR inlining support is partial and expression-oriented:

- `FunctionInliner::ReplaceParameterByArg(...)`
- `FunctionInliner::ReplaceParametersByArgs(...)`
- `FunctionCallInliner::visit(FunCallInst*)`

Observed behavior in `compiler/generator/fir_to_fir.cpp`:

- substitutes parameters by actual arguments inside a cloned callee block,
- counts parameter load occurrences,
- materializes a temporary local (`tmp_in*`) when an argument expression is reused,
- extracts callee return value and injects callee statements into caller block,
- rewrites only direct calls to a known `DeclareFun`.

Observed limitation (explicit TODO in C++):

- local stack variables are **not renamed**, so inlining the same function multiple times can create name collisions.

### 2.2 Implication for Rust

The Rust version should preserve the useful semantics from C++ (argument sharing,
statement splicing, return extraction) while fixing the missing pieces:

- hygienic renaming,
- robust legality checks,
- module-wide call graph handling,
- clear pass options and deterministic behavior.

## 3. What A “Complete” Function Inliner Must Do (Adapted to FIR)

This section combines compiler practice (LLVM/MLIR/GCC references) with Faust FIR
constraints.

### 3.1 Legality checks (must-have)

Before inlining a callsite, the pass must prove the transformation is legal:

- callee exists in module symbol table and has a body (`DeclareFun.body = Some(...)`)
- direct call only (FIR `FunCall` by name already is direct)
- argument count matches callee signature
- callee body has a supported form for inlining (see staged scope below)
- recursion policy is respected (no recursive inlining in v1)
- no unsupported constructs requiring extra control-flow lowering in v1

MLIR’s inliner model is a useful reference here: it explicitly separates
**“is legal to inline”** decisions from the transformation mechanics and passes
remapped values into legality hooks (`IRMapping` concept).

### 3.2 Argument substitution semantics (must-have)

Inlining must preserve call semantics:

- actual arguments are evaluated in caller context
- parameter uses in callee are replaced by those evaluated values
- repeated parameter uses must not duplicate expensive/side-effecting expressions

For FIR, the safe default is:

- evaluate each actual argument exactly once into a fresh `kStack` temp, unless it
  is proven trivial (literal / simple load / null-like value)
- substitute parameter loads with the temp load

This generalizes the C++ occurrence-count optimization and avoids semantic drift.

### 3.3 Hygienic renaming (must-have)

All callee-local names introduced into caller scope must be renamed to avoid
capture/clashes:

- `DeclareVar(kStack|kLoop)`
- `DeclareTable(kStack|kLoop)` if present in bodies
- loop iterator names
- any generated temporaries for parameter materialization

Also rewrite all matching load/store/table accesses to renamed symbols.

Without this, inlining the same callee twice (or inlining into a caller with the
same local names) is incorrect.

### 3.4 Return-value extraction / statement splicing (must-have)

Because FIR calls are value expressions (`FunCall`) while functions contain
statement blocks:

- clone and inline callee body statements into the surrounding caller block,
- extract the returned value expression from callee `Return(Some(v))`,
- replace the original `FunCall` expression with the extracted value,
- drop/remove the callee `Return` statement in the spliced block.

Unsupported in v1:

- multiple returns in divergent control flow,
- `Return(None)` in non-void contexts,
- bodies where no unique inlinable returned expression can be extracted.

These can be added later with CFG-like normalization or canonicalization.

### 3.5 Call graph / recursion handling (must-have for module pass)

A module inliner should reason over the call graph, not isolated calls.

Minimum viable behavior:

- build a direct call graph over `DeclareFun` bodies,
- detect SCCs,
- inline only **acyclic** callees in v1,
- skip self-recursive / mutually recursive SCCs by default.

LLVM’s inliners operate over SCC/module structures and expose recursion-related
controls in inline parameters; Rust FIR can start simpler (skip recursive SCCs).

### 3.6 Profitability / cost control (must-have, simple version)

A complete inliner should not inline everything blindly.

v1 heuristics (deterministic and easy to test):

- inline if `callee.is_inline == true`
- inline if callee body “size” <= threshold (node count / statement count)
- inline functions called once (optional phase 2)
- never inline excluded DSP API methods by default (`compute`, `metadata`, etc.)
- configurable max expansion budget per caller/module

GCC/LLVM both rely on thresholds and heuristics; Rust FIR should expose simple
options first, then grow a richer cost model later.

### 3.7 Cleanup after inlining (should-have in same pass or follow-up pass)

Typical inlining produces dead code and unused functions.

At minimum:

- keep module valid even without cleanup
- optionally run a follow-up FIR cleanup pass later (dead prototypes/functions)

Do not block v1 on full DCE. LLVM also separates inlining from broader cleanup in
many pipelines.

## 4. Rust FIR-Specific Design Constraints

### 4.1 Current IR shape (important)

Rust FIR is a tree-encoded IR with:

- `Module { dsp_struct, globals, declarations }`
- `DeclareFun { name, typ, args, body, is_inline }`
- statement/value split (`Block`, `DeclareVar`, `StoreVar`, `Return`, `FunCall`, ...)

Inlining therefore requires a **rewriter that can transform values while
inserting statements into the enclosing block** (same core challenge as the C++
`FunctionCallInliner`).

### 4.2 Verifier integration as pass oracle

`crates/fir/src/checker.rs` already checks:

- scope/access correctness (SCxx),
- function call signatures (FCxx),
- type consistency (B/U/C/T/MA/etc.),
- module structure (M/G/S/Fxx).

This enables a practical validation loop:

- `verify_fir_module(before)` for baseline sanity (optional)
- run inliner
- `verify_fir_module(after)` must have **no new errors**

The checker should be treated as the primary correctness oracle during rollout.

## 5. Rust API (Design vs Current Implementation)

Create a new FIR→FIR transform entrypoint (suggested location):

- `crates/fir/src/inliner.rs` (or `crates/fir/src/transforms/inliner.rs`)

Originally proposed API (design target):

```rust
pub struct FirInlineOptions {
    pub enabled: bool,
    pub inline_marked_only: bool,
    pub max_callee_nodes: usize,
    pub max_inline_depth: usize,
    pub max_expansion_factor: usize,
    pub allow_recursive: bool, // default false
    pub verify_after_each_function: bool,
}

pub struct FirInlineStats {
    pub callsites_seen: usize,
    pub callsites_inlined: usize,
    pub functions_skipped_recursive: usize,
    pub functions_skipped_unsupported: usize,
}

pub fn inline_fir_module(
    store: &FirStore,
    module: FirId,
    options: &FirInlineOptions,
) -> Result<(FirStore, FirId, FirInlineStats), FirInlineError>;
```

Notes:

- return a new `FirStore` + `module` id (persistent/functional style)
- avoid mutating the input store in v1
- stats are essential for debugging and tests

### 5.1 Current implemented API snapshot (actual names/types)

The implementation has split the functionality into several staged APIs and
stats types rather than a single monolithic `FirInlineStats`/`FirInlineError`.

- Iterative driver (Phase E):
  - `inline_fir_module(...) -> Result<(FirStore, FirId, FirInlineFixpointStats), FirInlineRewriteError>`
- One-pass rewrite:
  - `inline_fir_module_once(...) -> Result<(FirStore, FirId, FirInlineRewriteStats), FirInlineRewriteError>`
- Analysis:
  - `analyze_fir_inliner(...) -> Result<FirInlineAnalysis, FirInlineAnalysisError>`
- Preparation / clone helpers:
  - `prepare_callee_body_for_inlining(...)`
  - `clone_fir_hygienic(...)`, `clone_fir_hygienic_with_state(...)`

This staged surface has proven useful for targeted unit tests and checker-driven
validation of each phase independently.

## 6. Transformation Architecture (Implementation Plan)

### Phase A — Analysis scaffolding (no rewrite yet)

Status: `Implemented`

Deliverables:

- module function index (`name -> DeclareFun`)
- call graph extraction from all function bodies
- SCC detection
- callee size metric (node count / statement count)
- inlining candidate decision function with explainable reasons

Validation:

- unit tests over synthetic modules (acyclic / recursive / extern / missing body)

### Phase B — Hygienic clone + rename engine

Status: `Implemented`

Deliverables:

- deep FIR subtree clone into a destination `FirStore`
- rename map for local symbols (`HashMap<String, String>`)
- consistent rewriting of all name-bearing ops:
  - `DeclareVar`, `DeclareTable`
  - `LoadVar`, `LoadVarAddress`, `StoreVar`, `TeeVar`
  - `LoadTable`, `StoreTable`, `ShiftArrayVar`
  - loop constructs / iterator names

Key invariant:

- names introduced by the callee are fresh in caller context

Validation:

- checker passes after inlining same function twice into same block
- dedicated collision tests

### Phase C — Parameter materialization and substitution

Status: `Implemented` (conservative "materialize all args" policy)

Deliverables:

- classify “simple” vs “non-simple” actual arguments
- generate fresh temp declarations for non-simple args
- parameter substitution map (`param -> replacement value or temp load`)
- preserve evaluation order (left-to-right actual argument evaluation)

Pragmatic rule for v1:

- materialize **all** actual arguments into temps (simpler, safer)
- optimize later with trivial-value fast path

Validation:

- checker passes
- regression tests for repeated parameter use

### Phase D — Inline expression calls inside blocks

Status: `Implemented (v1 subset)`

Deliverables:

- rewrite value expressions recursively
- when `FunCall(callee)` is inlinable:
  - clone callee body with rename/substitution
  - splice statements into current block
  - extract returned value expression
  - substitute original `FunCall`

Required helper:

- “block rewrite context” that accumulates statement prefixes while rewriting a
  value expression (to emulate C++ `fBlockStack` behavior)

Scope for v1:

- only inline callees whose body has a canonical single `Return(Some(v))`
  reachable at top level (possibly with preceding statements)
- skip bodies with unsupported control-flow return shapes

Additional current limitation:

- some statement kinds (loop/switch internals) are still cloned without nested
  callsite rewriting; this is correct but not maximal inlining

Validation:

- end-to-end module tests
- post-pass `verify_fir_module`

### Phase E — Module iteration strategy

Status: `Implemented (v1)`

Deliverables:

- iterate callsites/functions until fixpoint or budget reached
- process functions in reverse-topological order of SCC DAG (acyclic SCCs first)
- skip recursive SCCs unless `allow_recursive`

Validation:

- stats-based tests (number of inlines)
- stable output determinism tests

Current implementation notes:

- iterative driver: `inline_fir_module(...)`
- deterministic function rewrite order uses reverse-topological SCC-DAG order
  (callees before callers), while preserving module declaration order
- stop conditions implemented:
  - fixpoint (no inlines in a pass)
  - `max_inline_depth` iteration cap
  - simple module-node expansion budget from `max_expansion_factor`

### Phase F — Optional cleanup pass (follow-up)

Not required for initial inliner correctness, but useful:

- remove now-unused private/local helper functions
- remove dead extern prototypes introduced only for intermediate forms

This can be a separate FIR pass.

Status: `Not implemented yet`

Planned v1 cleanup scope (recommended):

- remove unreachable helper `DeclareFun` bodies after inlining
- remove unused extern/prototype `DeclareFun { body: None }`
- preserve reserved DSP API functions by default
- keep cleanup as a separate FIR→FIR pass (do not block inliner correctness)

## 7. Inlining Legality Matrix (FIR v1)

### 7.1 Callee-level eligibility

Inline allowed only if all are true:

- `DeclareFun.body.is_some()`
- not in excluded API set (default):
  - `compute`, `metadata`, `buildUserInterface`, `instance*`, `classInit`, `init`, `getSampleRate`
- body shape is supported (canonical return extraction)
- callee not in recursive SCC (unless explicitly enabled)

### 7.2 Callsite-level eligibility

Inline allowed only if all are true:

- `FunCall.name` resolves to a unique `DeclareFun`
- actual arg count matches formal count
- call expression appears in a rewrite context that supports statement splicing
  (inside a `Block`-owned statement traversal)

### 7.3 Skip-first unsupported patterns (explicit)

Skip with reason/stats in v1:

- callee with multiple dynamic returns
- callee with `Return(None)` in value context
- callee using unsupported node kinds not yet handled by renamer
- recursive/self-recursive inlining (default)

## 8. Checker-Driven Validation Strategy

### 8.1 Mandatory pass checks

For every inliner test:

1. build/obtain FIR module
2. run `verify_fir_module(before)` (optional baseline assertion)
3. run inliner
4. run `verify_fir_module(after)`
5. assert:
   - no verifier errors introduced
   - no `FC01` regressions
   - no scope errors (`SC01/SC02/SC04/SC05`) from renaming/substitution

### 8.2 Suggested test classes

- Simple leaf expression helper inlining
- Repeated parameter use (temp sharing)
- Name collision across two inline sites
- Nested inlining (`f -> g -> h`)
- Extern/prototype call should not inline
- Recursive function should be skipped
- DSP API functions should remain non-inlined by default
- `is_inline=true` function forced inline (if legal)

### 8.3 Corpus-level smoke checks

After integration in compiler pipeline (later phase):

- run `--dump-fir-verify` on `tests/corpus/rep_*.dsp`
- confirm no increase in FIR verifier failures

## 9. Integration Plan in Rust Workspace

### 9.1 Implementation location (recommended)

- `crates/fir/src/inliner.rs`
- exported from `crates/fir/src/lib.rs`

Reason:

- pass is FIR-specific and should live beside `checker.rs` and FIR builders/matchers

### 9.2 Compiler integration (later, gated)

Integrate in `crates/compiler` only after:

- unit tests are stable,
- checker validation is integrated around the pass,
- default policy is conservative (`off` by default or marked-only only).

Suggested CLI flags later:

- `--fir-inline`
- `--fir-inline-marked-only`
- `--fir-inline-max-callee-nodes N`

## 10. Milestones and Acceptance Criteria

### Milestone 1 — Analyzer only

- call graph + SCC + candidate selection implemented
- no rewriting yet
- deterministic stats

Status: `Done`

### Milestone 2 — Leaf expression inlining

- inline simple non-recursive helpers with canonical single return
- hygienic renaming implemented
- checker passes on all unit tests

Status: `Done` (implemented through staged Phases B–D)

### Milestone 3 — Nested/block-aware inlining

- nested `FunCall` rewriting in value trees with statement splicing
- parameter materialization stable
- checker-validated end-to-end examples

Status: `Partially done`

- done:
  - nested value-tree rewriting and statement prefix splicing in supported shapes
  - parameter materialization + checker-validated end-to-end examples
- remaining for full milestone intent:
  - broader nested rewriting coverage in loop/switch/other currently cloned-only subtrees
  - broader callee return/control-flow shape support

### Milestone 4 — Compiler integration (optional gate)

- pass can run before backend codegen
- post-inline FIR checker validation available
- corpus smoke test shows no regressions

Status: `Not started`

## 11. Open Questions / Follow-up Decisions (updated after Phases A–E)

Resolved / current decisions:

1. `materialize all args` in v1 was chosen and implemented (safe default).
2. Recursive SCCs are skipped by default (`allow_recursive=false` policy).
3. Cleanup is deferred to a dedicated follow-up pass (Phase F), not mixed into the inliner.

Still open (practical follow-ups):

1. Should default policy inline heuristic-small callees even when `is_inline=false`, or require `inline_marked_only=true` in compiler integration?
2. Which additional FIR statement/value forms should be upgraded from "clone-only" to full nested rewrite in v1.1?
3. Do we want a canonicalization helper for multi-return bodies before inlining, or keep inliner legality strict?
4. Should compiler integration run post-inline `verify_fir_module` unconditionally when FIR verify is enabled, or behind a dedicated inliner-verify option?

## 12. Internet Research Notes / Sources

The following sources were used to ground the feature set (legality hooks, cost
heuristics, recursion handling, pass scope):

- LLVM Passes documentation (transform pass overview, `inline`, verifier pass context):
  - https://llvm.org/docs/Passes.html
- LLVM `InlineParams` (inline thresholds, deferral, recursive-call policy knob):
  - https://llvm.org/doxygen/structllvm_1_1InlineParams.html
- LLVM `ModuleInlinerPass` (module-level inliner concept):
  - https://llvm.org/doxygen/classllvm_1_1ModuleInlinerPass.html
- MLIR `DialectInlinerInterface` (explicit inlining legality hooks + remapping concepts):
  - https://mlir.llvm.org/doxygen/classmlir_1_1DialectInlinerInterface.html
- MLIR Interfaces documentation (motivation + inliner interface example):
  - https://mlir.llvm.org/docs/Interfaces/
- GCC optimize options (heuristic/threshold-driven inlining, called-once, indirect inlining):
  - https://gcc.gnu.org/onlinedocs/gcc-13.2.0/gcc/Optimize-Options.html

## 13. Local Provenance (Faust C++ Reference)

- `/Users/letz/Developpements/RUST/faust/compiler/generator/fir_to_fir.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/generator/fir_to_fir.hh`

Relevant fragments:

- `FunctionInliner`
- `FunctionCallInliner`
- parameter replacement / temporary materialization logic
- TODO on local variable renaming (must be fixed in Rust implementation)
