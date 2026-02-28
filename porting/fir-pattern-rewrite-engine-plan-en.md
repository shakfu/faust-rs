# Pattern Matching and Rewrite Engine Plan (Signals-first, FIR-second)

Status: proposed  
Date: 2026-02-28  
Target crates:
- `crates/patterns` (new, IR-agnostic matcher + rewrite core)
- `crates/signals` + `crates/transform` (primary rule packs in v1)
- `crates/fir` (secondary rule packs for low-level shaping)

---

## 1. Objective

Replace repeated ad-hoc `match ...` logic in analyses and transforms with a
shared declarative engine:

- rules are described as patterns + guards + actions,
- matching and traversal are implemented once,
- analyses and rewrites reuse the same machinery,
- behavior remains deterministic and parity-safe.

Placement decision:
- semantic/high-value optimization rules live first in `signals`/`transform`,
- FIR keeps backend-shaping and low-level canonicalization rules,
- matching/rewrite runtime is shared in `crates/patterns`.

---

## 2. Scope and Non-Goals

### In scope (v1)

- IR-agnostic pattern DSL core (`NodeKind`/fields/predicates).
- Adapter layer for concrete IRs (`signals` first, then `fir`).
- Capture-aware matcher with typed predicates.
- Rule runner for:
  - analysis-only rules,
  - rewrite rules.
- Deterministic traversal strategies and fixpoint execution.
- Basic performance infrastructure (memoization, budget/fuel, stats).

### Out of scope (v1)

- Full e-graph saturation engine.
- Cross-module global optimization.
- Semantics-changing approximations.
- Backend-specific vectorization/autotuning logic.

---

## 3. Design Constraints

1. Semantics and parity first: every rewrite must be equivalence-preserving.
2. Deterministic output required across platforms/runs.
3. Explicit termination conditions (max iterations, no unbounded loops).
4. Default pipeline must remain available (feature-gated migration).
5. Rule failures must produce actionable diagnostics.

---

## 3.1 Architecture Placement (new crate + adapters)

Core crate (`crates/patterns`) defines:
- generic pattern/matcher/rewrite engine,
- traits for node inspection and rebuilding,
- traversal/fixpoint/memoization/statistics.

IR adapter crates define:
- mapping from local IR node representation to engine traits,
- typed helper predicates/builders,
- rule packs specific to that IR stage.

Rust sketch:

```rust
pub trait IrAdapter {
    type NodeId: Copy + Eq + Hash;
    type TypeRef: Clone;
    type OpRef: Clone;

    fn node_kind(&self, id: Self::NodeId) -> NodeKind<Self::OpRef>;
    fn children(&self, id: Self::NodeId) -> SmallVec<[Self::NodeId; 4]>;
    fn rebuild(&mut self, id: Self::NodeId, new_children: &[Self::NodeId]) -> Self::NodeId;
    fn type_of(&self, id: Self::NodeId) -> Option<Self::TypeRef>;
}
```

Initial adapters:
- `signals` adapter in `crates/transform` (first consumer),
- `fir` adapter in `crates/fir` (second consumer).

---

## 4. Data Model

## 4.1 Pattern Representation

Core patterns are IR-agnostic; IR-specific helpers convert to/from concrete
constructs (e.g. `FirBinOp::Add`, signal binary ops).

```rust
/// Identifier used to bind and re-reference matched subtrees.
pub type CaptureId = u16;

#[derive(Clone, Debug)]
pub enum Pattern {
    Any,
    Capture(CaptureId, Box<Pattern>),
    Node(NodePattern),
    And(Vec<Pattern>),
    Or(Vec<Pattern>),
    Not(Box<Pattern>),
}

#[derive(Clone, Debug)]
pub enum NodePattern {
    Kind(NodeKindPattern),
    /// Optional generic field predicates (name, access class, type class, ...).
    Pred(Vec<FieldPredicate>),
    BinOp {
        op: Option<OpPattern>,         // adapter-resolved operation
        lhs: Box<Pattern>,
        rhs: Box<Pattern>,
        typ: TypePattern,              // adapter-resolved type class
        commutative: bool,             // allow lhs/rhs swap at match-time
    },
    UnaryOp {
        op: UnaryOpPattern,
        arg: Box<Pattern>,
        typ: TypePattern,
    },
    Select2 {
        cond: Box<Pattern>,
        then_v: Box<Pattern>,
        else_v: Box<Pattern>,
        typ: TypePattern,
    },
    LoadVar {
        name: NamePattern,
        access: AccessPattern,
        typ: TypePattern,
    },
    Const(ConstPattern),
    // Extend incrementally for IR-specific variants through adapters.
}

#[derive(Clone, Debug)]
pub enum TypePattern { Any, Exact(TypeToken), Numeric, Integer, Float, BoolLike }

#[derive(Clone, Debug)]
pub enum NamePattern { Any, Exact(String), Prefix(String), Regex(String) }

#[derive(Clone, Debug)]
pub enum AccessPattern { Any, Exact(AccessToken), OneOf(Vec<AccessToken>) }
```

Notes:
- `Pattern` supports composition (`And/Or/Not`) for analysis expressivity.
- `NodePattern` is explicit and typed, avoiding string-based AST matching.
- `commutative` is rule-local and only applied when algebraically valid.

## 4.2 Capture Environment

```rust
#[derive(Default)]
pub struct CaptureEnv {
    // capture id -> matched node id (adapter-defined)
    pub ids: FxHashMap<CaptureId, NodeId>,
}
```

Capture invariant:
- if a capture id appears multiple times in a pattern, all occurrences must map
  to the same node id (structural equality by adapter node identity).

## 4.3 Rule Model

```rust
pub type GuardFn = fn(&MatchCtx<'_>, &CaptureEnv) -> bool;
pub type BuildFn = fn(&mut RewriteCtx<'_>, &CaptureEnv) -> Result<NodeId, RuleError>;
pub type AnalyzeFn = fn(&AnalysisCtx<'_>, &CaptureEnv, &mut RuleReport);

pub struct RewriteRule {
    pub name: &'static str,
    pub phase: RulePhase,
    pub pattern: Pattern,
    pub guard: Option<GuardFn>,
    pub build: BuildFn,
    pub priority: i16,        // lower = earlier
    pub enabled: bool,
}

pub struct AnalysisRule {
    pub name: &'static str,
    pub pattern: Pattern,
    pub guard: Option<GuardFn>,
    pub analyze: AnalyzeFn,
}
```

## 4.4 Execution Options and Stats

```rust
pub enum Traversal { TopDown, BottomUp, PostOrderStable }

pub struct RewriteOptions {
    pub traversal: Traversal,
    pub max_passes: usize,         // fixpoint cap
    pub max_rewrites: usize,       // fuel cap
    pub memoize_matches: bool,
    pub verify_after_each_pass: bool,
}

pub struct RewriteStats {
    pub nodes_visited: u64,
    pub matches_attempted: u64,
    pub matches_succeeded: u64,
    pub rewrites_applied: u64,
    pub passes: u32,
    pub elapsed_match_ns: u128,
    pub elapsed_build_ns: u128,
}
```

## 4.5 Human-Friendly Rule DSL (builder layer)

The internal `Pattern` AST remains the canonical format. A small DSL layer is
added for readability in real rule files.

Example API shape:

```rust
use patterns::dsl as p;

let pat = p::select2(
    p::any(),
    p::cap("x", p::any()),
    p::same("x"),
    p::ty_any(),
);
```

Design rule:
- `Pattern` is low-level internal representation.
- `pattern!{...}` is the preferred authoring interface for humans.
- `dsl` helpers are the explicit fallback and expansion target for the macro.

---

## 5. Core Algorithms

## 5.1 Node Matching

Algorithm: `match_pattern(adapter, root_id, pattern, env) -> bool`

1. Dispatch on pattern kind.
2. For `Capture(id, inner)`:
   - if `id` not bound: try match `inner`, then bind `id -> root_id`;
   - if bound: require `bound_id == root_id`, then match `inner`.
3. For `Node(NodePattern::...)`:
   - decode adapter node view once,
   - check node shape,
   - recursively match children.
4. For commutative binops:
   - try `(lhs, rhs)`; if fail, try swapped order.
5. For `And/Or/Not`: short-circuit evaluation.

Complexity (single pattern, no memo):
- Typical tree pattern: `O(size(pattern) * local_depth)`.
- Commutative branches add a small constant factor (`<=2` branch factor).

## 5.2 Traversal and Rule Application

### Top-down mode

- Visit parent first.
- Apply first matching rule by priority.
- If rewritten, restart on rewritten node (same location) to maximize local
  simplification.

### Bottom-up mode

- Rewrite children first.
- Then attempt rules on parent.
- Better for algebraic canonicalization and fold-after-rewrite behavior.

## 5.3 Fixpoint Loop

Pseudo flow:

1. pass = 0
2. changed = true
3. while changed and pass < max_passes and rewrites < max_rewrites:
   - changed = run_one_pass(...)
   - optionally run stage verifier (signals or FIR)
   - pass += 1
4. return transformed root + stats + diagnostics.

Termination guarantees:
- bounded by `max_passes` and `max_rewrites`,
- optional monotonic measure assertion for selected canonicalization rules
  (e.g., node count non-increasing where required).

## 5.4 Match Memoization

Key:

```text
(node_id, pattern_id, revision_epoch) -> MatchOutcome
```

- `revision_epoch` increments when a rewrite modifies any ancestor path
  relevant to a subtree.
- Conservative invalidation strategy in v1:
  - bump global epoch per rewrite.
- v2 optimization:
  - subtree-local epoch/invalidation for finer reuse.

Expected benefit:
- avoids repeated re-matching in multi-rule pipelines and fixpoint passes.

---

## 6. API Surface (v1)

`crates/patterns/src/pattern.rs`
- pattern data structures and helper constructors.

`crates/patterns/src/matcher.rs`
- `match_pattern(...)`
- `Matcher` with memoization + instrumentation.

`crates/patterns/src/rewrite.rs`
- rule definitions
- rewrite runner / traversal / fixpoint engine.

`crates/patterns/src/adapter.rs`
- `IrAdapter` trait and reusable context types.

`crates/patterns/src/dsl.rs`
- human-friendly constructors for common patterns and captures.

`crates/patterns/src/macros.rs` (target v1, fallback v1.1)
- `pattern!{...}` + `rewrite_rule!{...}` + `analysis_rule!{...}` compact authoring.

`crates/transform/src/signal_opt/rewrite_rules.rs`
- signal-stage rule packs and adapter.

`crates/fir/src/rewrite_rules.rs` (phase 2)
- FIR-stage low-level rule packs and adapter.

---

## 7. Integration Plan

## Phase A — Foundation (`patterns` core)

Deliverables:
- pattern model + matcher core + unit tests.

Tests:
- capture binding behavior,
- commutative matching,
- composed patterns (`And/Or/Not`),
- deterministic matching results.

## Phase B — Rewrite Runner (`patterns`)

Deliverables:
- traversal engine + fixpoint + stats + rewrite diagnostics.

Tests:
- local rewrite on simple expressions,
- multi-pass convergence,
- fuel/pass cap behavior.

## Phase B.1 — DSL Layer for Readability

Deliverables:
- `dsl` helper API used by rule packs (instead of raw `Pattern::Node`).
- capture name support (`"x"`) resolved to compact internal capture ids.

Tests:
- DSL-to-AST equivalence tests,
- duplicate capture name behavior (`cap("x")` + `same("x")`),
- helpful error messages for invalid DSL construction.

## Phase C — Signals Rule Pack (safe canonicalization, primary)

Initial rewrite rules:
- `x + 0 -> x`
- `x - 0 -> x`
- `x * 1 -> x`
- `x * 0 -> 0` (type-safe zero literal)
- `select(c, a, a) -> a`
- constant binop fold for pure literals.

Tests:
- equivalence tests (before/after evaluator comparison where possible),
- no rewrite on unsupported types.

## Phase D — Migrate One Existing Signals Optimizer Pass

Candidate: localized algebraic simplification currently implemented with manual
signal node matching.

Deliverables:
- old and new signal pass produce equivalent output on target corpus,
- optional side-by-side differential test.

## Phase E — FIR Adapter + FIR Low-level Rules (secondary)

Use the same engine with a FIR adapter for low-level backend-shaping rules
only. Keep high-level algebraic/canonical semantic rewrites in `signals`.

## Phase F — Analysis Rules Adoption (both IRs)

Use same matcher in at least one analysis pass per IR (signals and FIR) to
validate analysis+rewrite unification.

---

## 8. Usage Examples (Human-friendly DSL + use-cases)

## 8.1 `select(cond, x, x) => x`

```rust
use patterns::dsl as p;

let rule_select_same = RewriteRule {
    name: "select-same-branches",
    phase: RulePhase::Canonicalize,
    pattern: p::select2(
        p::any(),
        p::cap("x", p::any()),
        p::same("x"),
        p::ty_any(),
    ),
    guard: None,
    build: |ctx, env| env.id("x"),
    priority: 10,
    enabled: true,
};
```

## 8.2 Algebraic identities (commutative and non-commutative)

```rust
use patterns::dsl as p;

let add_zero = p::rule(
    "add-zero",
    p::bin_comm("add", p::cap("x", p::numeric()), p::const_zero(), p::ty_numeric()),
    |ctx, env| env.id("x"),
);

let sub_zero = p::rule(
    "sub-zero",
    p::bin("sub", p::cap("x", p::numeric()), p::const_zero(), p::ty_numeric()),
    |ctx, env| env.id("x"),
);
```

Use-case:
- one rule handles both `x + 0` and `0 + x` through `bin_comm`.

## 8.3 Constant folding use-case

```rust
use patterns::dsl as p;

let fold_add_i32 = p::rule_with_guard(
    "const-fold-add-i32",
    p::bin(
        "add",
        p::cap("a", p::const_i32_any()),
        p::cap("b", p::const_i32_any()),
        p::ty_i32(),
    ),
    None,
    |ctx, env| {
        let a = env.const_i32("a")?;
        let b = env.const_i32("b")?;
        ctx.const_i32(a + b)
    },
);
```

Use-case:
- keeps folding logic local and readable; no explicit raw node destructuring.

## 8.4 Analysis use-case: division by zero warning

```rust
use patterns::dsl as p;

let warn_div_zero = AnalysisRule {
    name: "warn-div-zero",
    pattern: p::bin("div", p::any(), p::const_zero(), p::ty_numeric()),
    guard: None,
    analyze: |ctx, env, report| report.warn_code("FIR-B04", ctx.current_node()),
};
```

Use-case:
- same matcher core reused for diagnostics, not only rewrites.

## 8.5 Analysis use-case: suspicious redundant cast chains

```rust
use patterns::dsl as p;

let redundant_cast = AnalysisRule {
    name: "warn-redundant-cast-chain",
    pattern: p::cast(
        p::cap(
            "inner",
            p::cast(p::cap("src", p::any()), p::cap_type("t"), p::cap_type("t")),
        ),
        p::same_type("t"),
        p::same_type("t"),
    ),
    guard: None,
    analyze: |ctx, env, report| report.warn("redundant cast chain"),
};
```

Use-case:
- complex structural patterns remain readable with named captures.

## 8.6 `pattern!` macro syntax (preferred authoring style)

The macro is the preferred human-facing syntax for rule files. It expands to
the same canonical `Pattern` AST and uses the same matcher.

### 8.6.1 Basic matching and captures

```rust
// select(cond, x, x)
let p1 = pattern! { select2(_, $x, $x, _) };

// x + 0 OR 0 + x
let p2 = pattern! { (add $x 0) | (add 0 $x) };

// double-negation
let p3 = pattern! { neg(neg($x)) };
```

### 8.6.2 Type-constrained patterns

```rust
// integer-only add-zero
let p = pattern! { (add:int $x 0) | (add:int 0 $x) };

// float multiply-by-one
let p = pattern! { (mul:float $x 1.0) | (mul:float 1.0 $x) };
```

### 8.6.3 Named fields (FIR-oriented examples)

```rust
// load from funargs inputs with constant index capture
let p = pattern! {
  load_table(name="inputs", access="kFunArgs", index=$i:int, typ=_)
};

// store_table where written value is exactly the current loaded value
let p = pattern! {
  store_table(name=$t, index=$i, value=load_table(name=$t, index=$i, typ=_), typ=_)
};
```

### 8.6.4 Guards

```rust
// division by constant non-zero (safe strength reduction candidate)
let p = pattern! { (div $x $c) if const_non_zero($c) };

// power-of-two divisor
let p = pattern! { (div:int $x $c) if is_pow2_const($c) };
```

### 8.6.5 Rule declaration examples

```rust
let r1 = rewrite_rule! {
  name: "select-same-branches",
  when: pattern! { select2(_, $x, $x, _) },
  rewrite: $x
};

let r2 = rewrite_rule! {
  name: "add-zero",
  when: pattern! { (add $x 0) | (add 0 $x) },
  rewrite: $x
};

let r3 = rewrite_rule! {
  name: "fold-add-i32",
  when: pattern! { (add:i32 $a:const_i32 $b:const_i32) },
  rewrite_with: |ctx, env| ctx.const_i32(env.i32("a")? + env.i32("b")?)
};
```

### 8.6.6 Analysis-only examples

```rust
let warn_div_zero = analysis_rule! {
  name: "warn-div-by-zero",
  when: pattern! { (div _ 0) | (div _ 0.0) },
  report: |ctx, env, rep| rep.warn_code("FIR-B04", ctx.current_node())
};

let warn_redundant_select = analysis_rule! {
  name: "warn-redundant-select",
  when: pattern! { select2(_, $x, $x, _) },
  report: |ctx, env, rep| rep.note("redundant select", ctx.current_node())
};
```

### 8.6.7 Multi-rule set example

```rust
let rules = ruleset![
  rewrite_rule! { name: "add-zero", when: pattern! { (add $x 0) | (add 0 $x) }, rewrite: $x },
  rewrite_rule! { name: "sub-zero", when: pattern! { (sub $x 0) }, rewrite: $x },
  rewrite_rule! { name: "mul-one",  when: pattern! { (mul $x 1) | (mul 1 $x) }, rewrite: $x },
  rewrite_rule! { name: "mul-zero", when: pattern! { (mul _ 0) | (mul 0 _) }, rewrite: 0_like($0) },
];
```

Notes:
- Macro examples are declarative and concise by design.
- Final exact token grammar (`:type`, named fields, helper atoms) is defined in
  `crates/patterns/src/macros.rs` and validated with parser tests.
- If macro support is unavailable in a context, `dsl` builders remain a strict
  fallback with identical semantics.

## 8.7 Recognition vs Rewrite (step-by-step examples)

Rule authoring mental model:
- `pattern!` describes **what to recognize**.
- `rewrite_rule!` adds **how to rewrite** when matched.
- `analysis_rule!` adds **what to report** when matched (no rewrite).

### 8.7.1 Direct capture rewrite

```rust
rewrite_rule! {
  name: "select-same",
  when: pattern! { select2(_, $x, $x, _) },
  rewrite: $x
}
```

Interpretation:
1. Recognize `select2(cond, x, x, typ)`.
2. Rewrite the full node into captured `$x`.

### 8.7.2 Alternative shapes with one rewrite target

```rust
rewrite_rule! {
  name: "add-zero",
  when: pattern! { (add $x 0) | (add 0 $x) },
  rewrite: $x
}
```

Interpretation:
1. Recognize either `x + 0` or `0 + x`.
2. Rewrite to the same captured `$x`.

### 8.7.3 Computed rewrite from captures

```rust
rewrite_rule! {
  name: "fold-add-i32",
  when: pattern! { (add:i32 $a:const_i32 $b:const_i32) },
  rewrite_with: |ctx, env| {
    let a = env.i32("a")?;
    let b = env.i32("b")?;
    ctx.const_i32(a + b)
  }
}
```

Interpretation:
1. Recognize two integer constants.
2. Build a new constant node from captured values.

### 8.7.4 Guarded rewrite

```rust
rewrite_rule! {
  name: "div-pow2-to-shr",
  when: pattern! { (div:int $x $c) if is_pow2_const($c) },
  rewrite_with: |ctx, env| {
    let sh = env.pow2_shift("c")?;
    ctx.shr(env.id("x")?, sh)
  }
}
```

Interpretation:
1. Recognize integer division pattern.
2. Apply only if guard is true.
3. Rewrite to a shift expression.

### 8.7.5 Analysis-only pattern

```rust
analysis_rule! {
  name: "warn-div-zero",
  when: pattern! { (div _ 0) | (div _ 0.0) },
  report: |ctx, _env, rep| rep.warn_code("FIR-B04", ctx.current_node())
}
```

Interpretation:
1. Recognize problematic expression.
2. Emit diagnostic.
3. Keep IR unchanged.

## 8.8 Using a Rewrite Rule Set Inside a Pass

This example shows the full flow:
1. define a reusable rewrite rule set,
2. run the rewrite engine with fixpoint options,
3. integrate it into a module-level pass.

### 8.8.1 Define the rule set

```rust
use patterns::prelude::*;

fn canonical_rules() -> Vec<RewriteRule> {
    vec![
        rewrite_rule! {
            name: "add-zero",
            when: pattern! { (add $x 0) | (add 0 $x) },
            rewrite: $x,
            priority: 10
        },
        rewrite_rule! {
            name: "sub-zero",
            when: pattern! { (sub $x 0) },
            rewrite: $x,
            priority: 20
        },
        rewrite_rule! {
            name: "select-same",
            when: pattern! { select2(_, $x, $x, _) },
            rewrite: $x,
            priority: 30
        },
        rewrite_rule! {
            name: "fold-add-i32",
            when: pattern! { (add:i32 $a:const_i32 $b:const_i32) },
            rewrite_with: |ctx, env| {
                let a = env.i32("a")?;
                let b = env.i32("b")?;
                ctx.const_i32(a + b)
            },
            priority: 40
        },
    ]
}
```

### 8.8.2 Run one rewrite pass on a root node

```rust
pub fn run_canonical_rewrite_pass<A: IrAdapter>(
    adapter: &mut A,
    root: A::NodeId,
) -> Result<(A::NodeId, RewriteStats), RewriteError> {
    let rules = canonical_rules();

    let opts = RewriteOptions {
        traversal: Traversal::BottomUp,
        max_passes: 8,
        max_rewrites: 100_000,
        memoize_matches: true,
        verify_after_each_pass: false,
    };

    let mut engine = RewriteEngine::new(adapter, opts);
    let out = engine.run(root, &rules)?;
    Ok((out.root, out.stats))
}
```

### 8.8.3 Integrate at module pass level

```rust
pub fn rewrite_module<A: IrAdapter>(
    adapter: &mut A,
    roots: &[A::NodeId],
) -> Result<Vec<A::NodeId>, RewriteError> {
    let mut out = Vec::with_capacity(roots.len());

    for &root in roots {
        let (new_root, stats) = run_canonical_rewrite_pass(adapter, root)?;
        tracing::debug!(
            "rewrite root={:?} passes={} rewrites={}",
            root,
            stats.passes,
            stats.rewrites_applied
        );
        out.push(new_root);
    }

    Ok(out)
}
```

Integration note:
- the pass itself does not manually pattern-match IR nodes;
- all matching/rewrite behavior is centralized in `RewriteEngine` + rules.

---

## 9. Expected Performance

Important: expected performance is split by compile-time and runtime impact.

## 9.1 Compile-Time Expectations

Without memoization:
- naive multi-rule fixpoint can regress compile-time noticeably (`+15%` to
  `+60%` in worst-case rule sets) due to repeated traversal + matching.

With memoization + priority ordering + bounded passes:
- target envelope for v1:
  - median compile-time delta: `-10%` to `+10%` versus current manual passes,
  - p95 compile-time delta: `< +20%`.

Potential wins:
- fewer bespoke passes,
- reduced duplicated match logic,
- easier pruning of low-value rules via per-rule stats.

## 9.2 Runtime Expectations (Generated DSP)

The engine itself affects runtime only indirectly via better canonical
Signals/FIR.

Expected backend runtime effects (after rules are first migrated in signals,
then refined in FIR):
- neutral to modest win in most cases (`0%` to `+8%`) from cleaner canonical IR,
- larger gains possible (`+10%` to `+20%`) on expressions currently missed by
  duplicated/manual simplification logic.

No regression rule:
- if generated runtime regresses on benchmark corpus, affected rules are gated
  behind feature flags until corrected.

## 9.3 Memory Overhead Expectations

- capture envs: small, rule-local (`O(#captures)`).
- memoization table:
  - roughly `O(#visited_nodes * #active_patterns_per_pass)` entries worst case.
- expected practical overhead for v1:
  - `+5%` to `+20%` peak memory in rewrite-heavy compile phases.

---

## 10. Performance Validation Protocol

Bench sets:
- `tests/corpus/rep_*.dsp` representative subset.
- heavy fixtures used in backend benchmarks.

Measure:
1. Signal->FIR pass time.
2. Rewrite pass time and stats per rule.
3. FIR node count delta.
4. End-to-end backend runtime on representative DSPs.

Acceptance gates:
- correctness: no semantic diff vs baseline corpus outputs.
- compile-time: within envelope from §9.1.
- runtime: no systematic regression; targeted improvement on migrated cases.

---

## 11. Risks and Mitigations

1. Rule interaction cycles:
- mitigation: fixpoint caps + monotonic checks + cycle regression tests.

2. Hard-to-debug rewrites:
- mitigation: trace mode logging `(rule, node_before, node_after)`.

3. Pattern explosion/perf:
- mitigation: priority staging, rule groups, memoization.

4. Parity drift:
- mitigation: differential corpus tests and gated rollout by pass.

---

## 12. Immediate Next Steps

1. Implement Phase A (`pattern.rs` + `matcher.rs`) with full Rustdoc.
2. Add a minimal Phase C safe signals rule pack.
3. Migrate one existing signals simplification pass in controlled A/B mode.
4. Add Phase E FIR adapter only for low-level shaping rules.
5. Record compile/runtime deltas in journal with rule-level stats.
