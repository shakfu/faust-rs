# Phase 5 — Normalization & Signal Transformations

> **Crates**: `normalize`, `transform`
> **Estimate**: 30–40 person days
> **Prerequisites**: Phases 1–4

---

## 1. C++ Inventory

### 1.1 normalize/ — 2098 lines, 10 files

| File | Lines | Role |
|---------|--------|------|
| `normalize.hh/.cpp` | 208 | Entry point: `normalizeAddTerm`, `normalizeDelayTerm` |
| `simplify.hh/.cpp` | 537 | Algebraic simplification: `simplify(sig)`, propagation of constants, reductions |
| `aterm.hh/.cpp` | 416 | Additive terms: sum of products (additive normal form `a*x + b*y + ...`) |
| `mterm.hh/.cpp` | 664 | Multiplicative terms: product of factors (form `x^a * y^b * ...`) |
| `normalform.hh/.cpp` | 273 | Conversion to/from additive-multiplicative normal form |

### 1.2 transform/ — 11,327 lines, 58 files

**Transformation infrastructure:**

| File | Lines | Role |
|---------|--------|------|
| `treeTransform.hh/.cpp` | 257 | `TreeTransform`: recursive tree transformation with memoization |
| `treeTraversal.hh/.cpp` | 152 | `TreeTraversal`: recursive traversal without transformation |
| `rewriteRule.hh/.cpp` | 365 | **Rewriting rule system** (Yann, 2025): `RewriteRule`, `Normalize` (fixpoint) |
| `ruleAtom.hh/.cpp` | 258 | Atoms of rules: patterns and conditions |
| `sigTransform.hh/.cpp` | 249 | `sigTransform`: recursive application of normalization to signals |
| `sigIdentity.hh/.cpp` | 367 | Identity transformation on signals (model for others) |
| `signalVisitor.hh/.cpp` | 341 | Signal visitor (route without modification) |
| `signalValidator.hh/.cpp` | 140 | Structural validation of signals |

**Concrete transformations:**

| File | Lines | Role |
|---------|--------|------|
| `sigConstantPropagation.hh/.cpp` | 199 | Propagation of constants (old system) |
| `sigNewConstantPropagation.hh/.cpp` | 207 | Propagation of constants (new, via rewrite rules) |
| `sigPromotion.hh/.cpp` | 1,585 | **Largest file**: int→float promotion, cast insertion |
| `sigSelect2Simplification.hh/.cpp` | 124 | Simplification of `select2` |
| `sigDegenerateRecursionElimination.hh/.cpp` | 213 | Elimination of degenerate recursions |
| `sigRecursivenessChecker.hh/.cpp` | 129 | Checking recursion |
| `sigRecursiveDependencies.hh/.cpp` | 289 | Calculating recursive dependencies |
| `sigRetiming.hh/.cpp` | 448 | Retiming: redistribution of delays |
| `sigTypeChecker.hh/.cpp` | 137 | Type checking (post-inference) |
| `sigDependenciesGraph.hh/.cpp` | 598 | Construction of the signal dependency graph |
| `signal2Elementary.hh/.cpp` | 266 | Decomposition into elementary signals |

**FIR/IIR pattern detection:**

| File | Lines | Role |
|---------|--------|------|
| `revealFIR.hh/.cpp` | 426 | Detection of FIR patterns in signals |
| `revealIIR.hh/.cpp` | 217 | IIR pattern detection |
| `revealSum.hh/.cpp` | 117 | Structured sum detection |
| `factorizeFIRIIRs.hh/.cpp` | 126 | Factorization of detected FIR/IIR |

**Box transformations:**

| File | Lines | Role |
|---------|--------|------|
| `boxIdentity.hh/.cpp` | 199 | Identity transformation on boxes |
| `boxVisitor.hh/.cpp` | 241 | Box Visitor |
| `boxModulation.hh/.cpp` | 159 | Widget modulation detection/extraction |
| `boxModulationImplanter.hh/.cpp` | 260 | Implementation of modulation |

**Signal compilation → FIR (new pipeline):**

| File | Lines | Role |
|---------|--------|------|
| `signalFIRCompiler.hh/.cpp` | 1,804 | **Signal→FIR compiler**: resource allocation, generation of FIR blocks |
| `signalRenderer.hh/.cpp` | 1,454 | **FIR rendering**: emission of FIR instructions by signal |

**Status note (important)**: `signalFIRCompiler` and `signalRenderer` are currently test/experimental developments. They should be kept and improved, but they are **not critical** and must not block MVP parity.

### 1.3 Dependencies/ — 1,568 lines, 10 files

| File | Lines | Role |
|---------|--------|------|
| `DependenciesGraph.hh/.cpp` | 217 | Construction of the dependency graph (signal digraph) |
| `DependenciesScheduling.hh/.cpp` | 78 | Hierarchical scheduling (`Hsched`) |
| `DependenciesAudit.hh/.cpp` | 240 | Audit/validation of dependencies |
| `DependenciesPrinting.hh/.cpp` | 792 | Graph display (dot, text) |
| `DependenciesUtils.hh/.cpp` | 241 | Utilities (subgraph extraction, filtering) |

### 1.4 parallelize/ — 1,586 lines, 6 files

| File | Lines | Role |
|---------|--------|------|
| `loop.hh/.cpp` | 653 | `Loop`: DSP calculation loop (init/compute/post) |
| `code_loop.hh/.cpp` | 792 | `CodeLoop`: loop tree with dependencies |
| `graphSorting.hh/.cpp` | 141 | Topological sorting of loops |

---

## 2. Mapping C++ → Rust

### 2.1 normalize

```rust
/// Multiplicative term: x^a * y^b * ...
/// Representation of a monomial in normal form
pub struct MTerm {
    coefficient: f64,
    factors: Vec<(TreeId, i32)>,  // (signal, exponent)
}

impl MTerm {
    pub fn one() -> Self;
    pub fn from_signal(arena: &TreeArena, sig: TreeId) -> Self;
    pub fn mul(&self, other: &MTerm) -> MTerm;
    pub fn div(&self, other: &MTerm) -> MTerm;
    pub fn is_one(&self) -> bool;
    pub fn to_signal(&self, arena: &mut TreeArena) -> TreeId;
    pub fn normalize(&mut self);
}

/// Additive term: sum of mterms
/// a₁·M₁ + a₂·M₂ + ...
pub struct ATerm {
    terms: Vec<MTerm>,
}

impl ATerm {
    pub fn zero() -> Self;
    pub fn from_signal(arena: &TreeArena, sig: TreeId) -> Self;
    pub fn add(&self, other: &ATerm) -> ATerm;
    pub fn sub(&self, other: &ATerm) -> ATerm;
    pub fn mul_scalar(&self, k: f64) -> ATerm;
    pub fn to_signal(&self, arena: &mut TreeArena) -> TreeId;
    pub fn simplify(&mut self);
}

/// Algebraic simplification of a signal
pub fn simplify(
    arena: &mut TreeArena,
    sig: TreeId,
    cache: &mut TreeProperty<TreeId>,
) -> TreeId;

/// Complete normalization of a signal
pub fn normalize_signal(
    arena: &mut TreeArena,
    sig: TreeId,
) -> TreeId;
```

### 2.1 normalize — Recommended restructuring during the Rust port

The audit of `normalize/` (`normalize.cpp`, `simplify.cpp`, `aterm.cpp`, `mterm.cpp`, `normalform.cpp`) shows high-value simplifications:

1. Replace legacy recursive property-mutating walkers (`sigMap`/`sigMapRename`) with explicit pass contexts and session-scoped memoization.
2. Replace imperative hard-coded sequencing in `simplifyToNormalFormAux` with a declarative normalization pipeline.
3. Reduce repeated full retyping by defining where type invalidation occurs and running `typeAnnotation` only at stable checkpoints.
4. Move option gates (`gRangeUI`, `gFreezeUI`, `gFTZMode`, `gAutoDifferentiate`, `gCheckTable`, `gCheckIntRange`) into explicit normalization config objects.
5. Replace pointer/serial-based ordering heuristics in additive canonicalization with deterministic structural ordering keys.
6. Redesign `aterm`/`mterm` internals around typed monomial keys to avoid repeated signature tree reconstruction.
7. Improve factorization scalability (`greatestDivisor` pairwise scans) with grouped candidate strategies.
8. Replace global property keys (`SIMPLIFIED`, `NORMALFORM`) with per-session caches and avoid mutating shared tree properties.
9. Isolate printing state from global clears by introducing dedicated printer state/context objects.
10. Preserve behavior while hardening arithmetic edge cases in normalize internals (especially divisor/zero guards).

Recommended rollout:

1. Keep output parity first with golden tests on normalized signals.
2. Introduce explicit pipeline + cache contexts without changing simplification semantics.
3. Then refactor `aterm`/`mterm` canonicalization internals for determinism and maintainability.

### 2.2 transform — Infrastructure

#### Rewrite rule system (rewriteRule.hh)

This is the **key system** introduced by Yann in 2025. All future transformations should use it.

```rust
/// A rewrite rule: Tree → Option<Tree>
pub trait RewriteRule: Send + Sync {
    fn name(&self) -> &str;
    fn apply(&self, arena: &mut TreeArena, sig: TreeId) -> Option<TreeId>;
}

/// Fixed-point normalizer: applies rules until convergence
pub struct Normalizer {
    rules: Vec<Box<dyn RewriteRule>>,
    debug_level: DebugLevel,
}

impl Normalizer {
    pub fn new(rules: Vec<Box<dyn RewriteRule>>) -> Self;

    /// Normalizes a signal (fixed point over all rules)
    pub fn normalize(
        &self,
        arena: &mut TreeArena,
        sig: TreeId,
        cache: &mut TreeProperty<TreeId>,
    ) -> TreeId;
}

/// Recursive application of a transformation to sub-signals
pub fn sig_transform(
    arena: &mut TreeArena,
    sig: TreeId,
    transform: impl FnMut(&mut TreeArena, TreeId) -> TreeId,
    cache: &mut TreeProperty<TreeId>,
) -> TreeId;
```

#### TreeTransform and TreeTraversal

```rust
/// Recursive tree transformation with memoization
pub trait TreeTransformer {
    fn transform(&mut self, arena: &mut TreeArena, t: TreeId) -> TreeId;
}

/// Recursive traversal without transformation
pub trait TreeVisitor {
    fn visit(&mut self, arena: &TreeArena, t: TreeId);
}

/// Signal visitor (specialized for signal constructors)
pub trait SignalVisitor {
    fn visit_signal(&mut self, arena: &TreeArena, sig: TreeId);
    fn visit_root_list(&mut self, arena: &TreeArena, list: TreeId) {
        // traverses the list, calls visit_signal for each element
    }
}
```

### 2.3 transform — Concrete transformations

Each C++ transformation becomes a struct implementing `RewriteRule` or `TreeTransformer`:

```rust
// Constant propagation (new rewrite rule style)
pub struct ConstantPropagation;
impl RewriteRule for ConstantPropagation {
    fn name(&self) -> &str { "ConstantPropagation" }
    fn apply(&self, arena: &mut TreeArena, sig: TreeId) -> Option<TreeId> { /* ... */ }
}

// int → float promotion
pub struct TypePromotion;
impl RewriteRule for TypePromotion {
    fn name(&self) -> &str { "TypePromotion" }
    fn apply(&self, arena: &mut TreeArena, sig: TreeId) -> Option<TreeId> { /* ... */ }
}

// select2 simplification
pub struct Select2Simplification;
impl RewriteRule for Select2Simplification { /* ... */ }

// Degenerate recursion elimination
pub struct DegenerateRecursionElimination;
impl RewriteRule for DegenerateRecursionElimination { /* ... */ }

// Retiming
pub struct Retiming;
impl RewriteRule for Retiming { /* ... */ }

// Validation
pub struct SignalValidator;
impl SignalVisitor for SignalValidator {
    fn visit_signal(&mut self, arena: &TreeArena, sig: TreeId) { /* ... */ }
}
```

### 2.4 transform — Dependency graph and scheduling

```rust
/// Signal dependency graph
pub fn build_dependency_graph(
    arena: &TreeArena,
    signals: &[TreeId],
) -> DiGraph<TreeId>;

/// Hierarchical scheduling
pub struct HierarchicalSchedule {
    pub output_signals: Vec<TreeId>,
    pub controls: Schedule<TreeId>,
    pub signal_schedules: HashMap<TreeId, Schedule<TreeId>>,
}

pub fn schedule_signals(
    arena: &TreeArena,
    signals: &[TreeId],
    strategy: impl Fn(&DiGraph<TreeId>) -> Schedule<TreeId>,
) -> HierarchicalSchedule;
```

### 2.5 transform — Widget Modulation

```rust
/// Widget modulation detection in boxes
pub fn detect_modulation(
    arena: &TreeArena,
    box_tree: TreeId,
) -> Vec<ModulationInfo>;

pub struct ModulationInfo {
    pub widget_path: String,
    pub modulation_signal: TreeId,
}

/// Modulation implantation in signals
pub fn implant_modulation(
    arena: &mut TreeArena,
    signals: &mut [TreeId],
    modulations: &[ModulationInfo],
);
```

### 2.6 transform — Signal→FIR compilation

**Note**: `signalFIRCompiler` and `signalRenderer` bridge phases 5 and 6, but in the current roadmap they are treated as an **experimental/non-blocking lane**. Phase 5 MVP is achieved through core transform parity first (normalization, rewrite rules, dependency/scheduling), then optional work on these two modules.

```rust
/// Result of signal → FIR compilation
pub struct FirBlocks {
    pub global_block: Vec<FirInst>,     // global declarations
    pub declare_block: Vec<FirInst>,    // DSP struct fields
    pub init_block: Vec<FirInst>,       // instanceInit
    pub reset_block: Vec<FirInst>,      // instanceResetUserInterface
    pub clear_block: Vec<FirInst>,      // instanceClear
    pub metadata_block: Vec<FirInst>,   // metadata()
    pub ui_block: Vec<FirInst>,         // buildUserInterface()
    pub tables_block: Vec<FirInst>,     // table filling
    pub control_block: Vec<FirInst>,    // per-block computations in compute()
    pub sample_block: Vec<FirInst>,     // per-sample DSP loop body
}

/// Signal to FIR compiler
pub fn compile_signals_to_fir(
    arena: &TreeArena,
    signals: &[TreeId],
    num_inputs: usize,
    num_outputs: usize,
    config: &CompilerConfig,
) -> Result<FirBlocks, FaustError>;
```

### 2.7 Parallelize

```rust
/// DSP loop with dependencies
pub struct DspLoop {
    pub init_code: Vec<FirInst>,
    pub compute_code: Vec<FirInst>,
    pub post_code: Vec<FirInst>,
    pub dependencies: Vec<usize>,  // indices of prerequisite loops
}

/// Loop tree (for parallelization)
pub struct LoopGraph {
    pub loops: Vec<DspLoop>,
    pub execution_order: Vec<usize>,  // topological sort
}

pub fn build_loop_graph(fir_blocks: &FirBlocks) -> LoopGraph;
pub fn topological_sort_loops(graph: &LoopGraph) -> Vec<usize>;
```

The audit of `parallelize/` (`loop.cpp`, `code_loop.cpp`, `graphSorting.cpp`) shows high-value simplifications:

1. Converge duplicated loop representations (`Loop`, `CodeLoop`) into one typed loop-graph model.
2. Replace duplicated topo-sort logic (`graphSorting` and `CodeLoop::sortGraph`) with one shared scheduler service.
3. Replace pointer-set dependencies with stable loop IDs and deterministic ordering.
4. Move mutable scheduling counters (`fOrder`, `fUseCount`) to explicit analysis outputs/maps.
5. Replace `dynamic_cast`-based block stacks (`IF`/`OD`/`US`/`DS`) with enum-scoped block states.
6. Route parallel/scheduling options via explicit context/config objects instead of `gGlobal`.
7. Represent OpenMP/work-stealing semantics as structured IR annotations, not textual labels/pragmas.
8. Deduplicate sequence-grouping algorithms (`computeUseCount`/`groupSeqLoops`) currently repeated in multiple modules.
9. Harden DS condition generation to avoid non-short-circuit modulo guards and zero-divisor edge cases.
10. Replace `pow`-based loop-stride scaling with integer-safe helpers and explicit overflow checks.

Recommended rollout:

1. Keep output parity with golden tests for loop ordering and generated parallel regions.
2. Introduce shared loop-graph/scheduler APIs first, then migrate existing generators to that API.
3. Refactor block-scope modeling and parallel annotations once scheduling parity is locked.

### 2.7.1 Dependencies — Recommended restructuring during the Rust port

The audit of `Dependencies/` (`DependenciesGraph.cpp`, `DependenciesUtils.cpp`, `DependenciesScheduling.cpp`, `DependenciesPrinting.cpp`, `DependenciesAudit.cpp`) shows high-value simplifications:

1. Replace implicit graph identity keyed by root `Tree` (`siggraph[signalList]`) with explicit graph IDs and role tags (`main`, `controls`, `subgraph`).
2. Replace duplicated recursive builders (`addDependencies` and `simpleAddDependencies`) with one traversal kernel parameterized by dependency policy.
3. Represent dependency classes explicitly (immediate/delayed/external/control) instead of ad hoc vectors/sets.
4. Replace OD/US/DS branch parsing based on `nil` separators with typed node payloads.
5. Move clock-env and type preconditions into explicit analysis context inputs (instead of hidden global/property lookups).
6. Replace throw/assert-heavy control flow in dependency extraction with structured diagnostics/results.
7. Consolidate schedule printing and DOT generation through one formatter backend with selectable output modes.
8. Reactivate graph auditing as a real validation pass (currently stubbed) and wire it to regression tests.
9. Remove or isolate debug/commented instrumentation from production dependency-graph paths.
10. Guarantee deterministic schedule and graph output ordering independent of pointer/map iteration behavior.

Recommended rollout:

1. Keep parity first with golden tests on graph shape + scheduling order + debug render output.
2. Introduce explicit graph IDs and typed dependency edges before changing scheduling logic.
3. Then unify traversal/formatting and turn validation auditing into a mandatory test stage.

### 2.8 Recommended transform restructuring during the Rust port

The audit of `transform/` shows high-value simplifications to fold into Phase 5:

1. Replace duplicated large `isSig*` dispatch chains with a shared typed signal-node traversal kernel.
2. Split resource planning (delays/tables/UI discovery) from execution semantics and share planner outputs.
3. Break `sigPromotion` into a pipeline of focused passes instead of one monolithic transformation.
4. Route transform options, warnings, and mode flags through explicit pass context objects (remove hidden global coupling).
5. Converge on one transform engine built around rewrite rules and fixed-point normalization.
6. Replace recursion sentinels encoded in temporary AST rewrites with explicit recursion-state tracking.
7. Merge dependency analyses into one graph service with different query modes.
8. Decompose oversized modules (`signalFIRCompiler.hh`, `signalRenderer.hh`) into planner/semantics/runtime/API layers.
9. Unify diagnostics and error reporting; avoid mixed asserts, stderr logging, and global warning side effects in pass code.
10. Keep `signalRenderer` and `signalFIRCompiler` maintained but isolated as an optional track after core transform parity.

Recommended rollout:

1. Port core transform infrastructure and high-value passes first.
2. Stabilize with differential tests (normalized signals, dependency graphs, scheduling order).
3. Port and simplify `signalRenderer`/`signalFIRCompiler` as a non-blocking experimental stream.

---

## 3. Complete transformation pipeline

The order of application of transformations in the C++ compiler (and to be preserved in Rust):

```
Raw signals (output of propagate)
    │
    ├─ 1. sigRecursivenessChecker   (verification)
    ├─ 2. sigDegenerateRecursionElim (cleanup)
    ├─ 3. sigTypeChecker             (verification)
    ├─ 4. sigPromotion               (int → float casts)
    ├─ 5. sigConstantPropagation     (constant propagation)
    ├─ 6. simplify / normalize       (algebra: aterm/mterm)
    ├─ 7. sigSelect2Simplification   (select2 simplification)
    ├─ 8. sigRetiming                (delay redistribution)
    ├─ 9. revealFIR / revealIIR      (pattern detection)
    ├─10. factorizeFIRIIRs           (factorization)
    ├─11. signal2Elementary          (decomposition)
    │
    ▼
Normalized signals
    │
    ├─12. buildDependencyGraph       (dependency graph)
    ├─13. scheduleSigList            (scheduling)
    │
    ▼
    signalFIRCompiler + signalRenderer → FirBlocks   (experimental/non-blocking lane)
```

---

## 4. Dependencies

```
normalize  → tlib, signals, interval
transform  → tlib, signals, normalize, graph, errors, interval
```

Core `transform` should remain independent from FIR generation details.

For the optional `signalFIRCompiler`/`signalRenderer` lane:
- **Option A**: keep FIR-facing adapter code inside `transform` (temporary coupling, lower short-term cost)
- **Option B** (recommended): isolate shared FIR-facing contracts in a lightweight bridge crate/module

---

## 5. Known pitfalls

### 5.1 sigPromotion is the largest file (1,585 lines)
Type promotion inserts `int→float` and `float→int` casts into signals. It's complex because you have to treat each signal constructor differently. This is a good candidate for the rewrite rules system.

### 5.2 Old vs new transformation system
There are two systems coexisting:
- **Former**: `TreeTransform` / `SignalIdentity` (legacy, virtual)
- **New**: `RewriteRule` / `Normalize` (functional, 2025)

→ In Rust, port everything to the new system (trait `RewriteRule`). Old classes become structs implementing the trait.

### 5.3 signalFIRCompiler/signalRenderer — experimental bridge complexity
These modules are large bridge layers between signals and FIR/runtime behavior. They do:
- Allocation of variables (delay lines, tables, UI)
- Assigning unique names
- FIR/runtime emission and DSP factory-facing logic

They are useful to keep, but should remain **non-blocking** for Phase 5 MVP.

### 5.4 Memoization and sharing
Many transformations use `property<Tree>` for cache (memoization). In Rust, this will be `TreeProperty<TreeId>`. Be careful not to invalidate the cache when the arena is mutated.

### 5.5 Dependencies/ is recent and well structured
This module is well isolated and well documented. This is a good candidate for direct porting.

### 5.6 Dispatch duplication across transform modules
Multiple modules reimplement long ad hoc signal dispatch chains (`SignalVisitor`, `SignalIdentity`, `SignalRenderer`, `SignalFIRCompiler`). This increases drift risk and should be unified early.

### 5.7 Global-state coupling in transform passes
Several transform paths depend on mutable global options/warning channels. Rust should enforce explicit pass/session contexts for deterministic behavior and easier testing.

### 5.8 Hard-coded normalization pipeline and repeated retyping
`normalform.cpp` currently chains many option-dependent passes with repeated `typeAnnotation` calls. Rust should model this as a declarative pass graph with explicit type invalidation points.

### 5.9 Legacy property-based simplify cache coupling
`simplify.cpp` caches through tree properties with global keys/sentinels. Rust should use session-owned memoization maps to avoid hidden shared-state coupling.

### 5.10 Canonicalization stability and arithmetic edge checks in `aterm`/`mterm`
Current normalization internals rely on legacy heuristics (including serial-based ordering) and contain fragile arithmetic guard patterns. Rust should use deterministic keys and explicit checked arithmetic paths.

### 5.11 Duplicate loop-graph scheduling logic across `parallelize/` and generator paths
Topological sorting and sequence grouping logic are duplicated (`graphSorting`, `CodeLoop::sortGraph`, and similar code in generator paths), which increases drift and maintenance risk.

### 5.12 Pointer-based dependency ordering and mutable in-node scheduling counters
Using `std::set<Loop*>`/`std::set<CodeLoop*>` with mutable `fOrder`/`fUseCount` embedded in nodes makes ordering less explicit and harder to reason about deterministically.

### 5.13 Textual pragma/guard emission in loop parallelization paths
Parallel semantics and DS guards are partially encoded as strings/labels; this should become structured IR semantics with explicit safe guards.

### 5.14 Dependencies graph identity and traversal duplication
`DependenciesGraph.cpp` uses implicit root-keyed graph identity and duplicates traversal paths (`addDependencies` vs `simpleAddDependencies`), which complicates reasoning and maintenance.

### 5.15 Sentinel-based dependency semantics and global precondition coupling
Dependency extraction still depends on `nil` separators and hidden prerequisites (`ClkEnvInference`/type annotation properties), increasing fragility when pass ordering changes.

### 5.16 Schedule/debug rendering duplication and disabled audit path
Printing/DOT logic duplicates traversal concerns, and `DependenciesAudit` is currently effectively disabled, reducing confidence in graph invariants over time.

---

## 6. Testing

- **Unit**: Algebraic simplification (`0+x → x`, `1*x → x`, `x-x → 0`)
- **Unit**: MTerm and ATerm (construction, normalization, round-trip)
- **Unit**: Each RewriteRule individually on known signals
- **Unit**: Dependency graph on simple examples
- **Unit**: Scheduling on known graphs
- **Integration**: Complete pipeline on simple Faust files (`process = + ~ _;`)
- **Differential**: Compare C++ vs Rust normalized signals on 20+ examples
- **Bench**: criterion on `sigPromotion` and `simplify` (the most expensive)

---

## 7. "Done" criteria

- [ ] All transformations ported with the new system `RewriteRule`
- [ ] Complete functional normalization pipeline
- [ ] Dependency graph and functional scheduling
- [ ] Modulation widget detected and implemented
- [ ] Passing differential tests on 20+ examples
- [ ] No legacy `TreeTransform` system based on inheritance
- [ ] Normalize pipeline is declarative and driven by explicit options/context (no hidden global pass sequencing)
- [ ] Normalize/simplify memoization is session-scoped (no tree-property global cache coupling)
- [ ] Algebra canonicalization is deterministic and independent from pointer/serial ordering
- [ ] Normalize arithmetic edge cases are covered by dedicated regression tests (division/zero/range)
- [ ] Parallelize uses a single shared loop-graph scheduler API (no duplicated topo/grouping implementations)
- [ ] Loop dependency ordering is stable and ID-driven (not pointer-order dependent)
- [ ] Parallel semantics (OpenMP/work-stealing) are represented as structured IR annotations
- [ ] Parallelize edge cases are covered by dedicated regression tests (DS zero/modulo guards, grouping determinism)
- [ ] Dependencies graph model uses explicit graph identities/roles and typed dependency edge kinds
- [ ] Dependencies traversal/scheduling is unified (no duplicated simple/full dependency builders)
- [ ] Dependencies validation audit is active in CI (not stubbed) with regression coverage
- [ ] Dependencies textual outputs (schedule/DOT) are produced via one deterministic formatter backend
- [ ] Core Phase 5 parity is validated without relying on `signalFIRCompiler`/`signalRenderer`
- [ ] `signalFIRCompiler` and `signalRenderer` parity improvements are delivered as optional/non-blocking outputs

---

## 8. Detailed Effort

| Sub-module | LOC C++ | Estimated LOC Rust | Days |
|-------------|---------|-----------------|-------|
| normalize/ (aterm, mterm, simplify) | 2,098 | 1,500 | 5–6 |
| transform/infrastructure(rewriteRule, treeTransform, sigTransform) | 1,572 | 1,000 | 4–5 |
| transform/ concrete transformations (12 passes) | 4,453 | 3,000–3,500 | 10–12 |
| transform/ signalFIRCompiler + signalRenderer (experimental lane) | 3,258 | 2,500 | 8–10 (optional) |
| transform/ box transforms (modulation, visitor) | 935 | 600 | 2–3 |
| Dependencies/ | 1,568 | 1,000 | 3–4 |
| parallelize/ | 1,586 | 1,000 | 3–4 |
| Tests + docs | — | 1,500 | 4–5 |
| **Total Phase 5** | **15,470** | **12,100–13,600** | **39–49** |

Core (non-experimental) Phase 5 effort is lower by the optional `signalFIRCompiler`/`signalRenderer` lane and should be tracked separately in execution planning.
