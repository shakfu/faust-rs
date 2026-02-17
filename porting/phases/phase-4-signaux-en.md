# Phase 4 — Signals, Evaluation, Propagation

> **Crates**: `signals`, `eval`, `propagate`
> **Estimate**: 30–40 person days
> **Prerequisites**: Phases 1–3

---

## 1. C++ Inventory

### 1.1 signals/ — 10,596 lines, 30 files

| File | Lines | Role |
|---------|--------|------|
| `signals.hh/.cpp` | ~2,500 | **183 signal constructors/destructors**: `sigInt`, `sigReal`, `sigInput`, `sigOutput`, `sigBinOp`, `sigDelay`, `sigMem`, `sigTable`, `sigSelect2`, `sigButton`, `sigHSlider`, `sigVSlider`, `sigPrefix`, `sigAttach`, `sigControl`, `sigRec`, `sigProj`, `sigSoundfile`, `sigEnable`, `sigOnDemand`, `sigDownSampling`, `sigUpSampling`, etc. |
| `sigtype.hh/.cpp` | ~800 | Type system: `AudioType`, `SimpleType`, `TableType`, `TupletType` with nature, variability, computability, vectorizability |
| `sigtyperules.hh/.cpp` | ~600 | Type inference rules for each signal |
| `sigorderrules.hh/.cpp` | ~400 | Evaluation order rules |
| `binop.hh/.cpp` | ~600 | 13 binary operators (`BinOp`), operation tables, Wasm opcode |
| `prim2.hh/.cpp` | ~300 | arity-2 primitive functions: `sigAdd`, `sigSub`, etc. |
| `ppsig.hh/.cpp` | ~400 | Pretty-printing signals |
| `sigprint.hh/.cpp` | ~300 | Alternative printing for debugging |
| `sigvisitor.hh/.cpp` | ~300 | Signal Visitor (old style) |
| `recursiveness.hh/.cpp` | ~300 | Signal recursion calculation |
| `sharing.hh/.cpp` | ~200 | Sub-signal sharing analysis |
| `subsignals.cpp` | ~100 | Extraction of direct subsignals |
| `sigFIR.hh/.cpp` | ~300 | Detection/construction of FIR filters in signals |
| `sigIIR.hh/.cpp` | ~300 | Detection/construction of IIR filters in signals |
| `clkEnvInference.hh/.cpp` | ~300 | Clock environment inference (multi-rate) |
| `interval.hh` | ~50 | Re-export of type `interval` (not the `interval/` folder) |

### 1.2 evaluate/ — 2,343 lines, 6 files

| File | Lines | Role |
|---------|--------|------|
| `eval.hh/.cpp` | ~1,600 | **Evaluator**: name resolution, call expansion, iteration evaluation (`par(i,n,...)`, `sum(i,n,...)`), compile-time calculation |
| `environment.hh/.cpp` | ~350 | Evaluation environments: name→value connection, scoping |
| `loopDetector.hh/.cpp` | ~400 | Infinite loop detection in evaluation |

### 1.3 patternmatcher/ — 911 lines, 2 files

| File | Lines | Role |
|---------|--------|------|
| `patternmatcher.hh/.cpp` | 911 | Pattern matching automaton: compilation of patterns into automaton, execution |

### 1.4 propagate/ — 1,347 lines, 4 files

| File | Lines | Role |
|---------|--------|------|
| `propagate.hh/.cpp` | ~1,000 | **Propagation**: conversion boxes → signals. Propagates N input signals through a block diagram to obtain M output signals |
| `labels.hh/.cpp` | ~350 | Managing UI label paths (group paths) |

### 1.5 extended/ — 2,847 lines, 24 files

| File | Lines | Role |
|---------|--------|------|
| `xtended.hh/.cpp` | ~300 | Base class `xtended`: extended mathematical operation with semantics (interval propagation, types, etc.) |
| `sinprim.hh` … `tanprim.hh` | ~100 each | 22 files — one per mathematical function: `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `exp`, `log`, `log10`, `exp10`, `pow`, `sqrt`, `abs`, `min`, `max`, `floor`, `ceil`, `rint`, `round`, `remainder`, `fmod` |

---

## 2. Mapping C++ → Rust

### 2.1 signals

#### 2.1.1 Signal constructors

Same approach as for boxes — typed layer above `TreeArena`:

```rust
/// Typed signal constructor
pub struct SigBuilder<'a> {
    arena: &'a mut TreeArena,
}

impl<'a> SigBuilder<'a> {
    // Constants
    pub fn int(&mut self, n: i32) -> TreeId;
    pub fn real(&mut self, r: f64) -> TreeId;

    // Inputs/Outputs
    pub fn input(&mut self, i: i32) -> TreeId;
    pub fn output(&mut self, i: i32, sig: TreeId) -> TreeId;

    // Binary operations
    pub fn binop(&mut self, op: BinOp, a: TreeId, b: TreeId) -> TreeId;
    pub fn add(&mut self, a: TreeId, b: TreeId) -> TreeId;  // shortcut
    pub fn sub(&mut self, a: TreeId, b: TreeId) -> TreeId;
    pub fn mul(&mut self, a: TreeId, b: TreeId) -> TreeId;
    pub fn div(&mut self, a: TreeId, b: TreeId) -> TreeId;
    // ... (13 binop)

    // Delays and memory
    pub fn delay(&mut self, sig: TreeId, d: TreeId) -> TreeId;
    pub fn mem(&mut self, sig: TreeId) -> TreeId;
    pub fn prefix(&mut self, init: TreeId, sig: TreeId) -> TreeId;

    // Tables
    pub fn table(&mut self, size: TreeId, content: TreeId) -> TreeId;
    pub fn rdtable(&mut self, table: TreeId, idx: TreeId) -> TreeId;
    pub fn wrtable(&mut self, size: TreeId, content: TreeId, widx: TreeId, wsig: TreeId) -> TreeId;

    // UI
    pub fn button(&mut self, label: TreeId) -> TreeId;
    pub fn checkbox(&mut self, label: TreeId) -> TreeId;
    pub fn hslider(&mut self, label: TreeId, init: TreeId, lo: TreeId, hi: TreeId, step: TreeId) -> TreeId;
    pub fn vslider(&mut self, label: TreeId, init: TreeId, lo: TreeId, hi: TreeId, step: TreeId) -> TreeId;
    pub fn numentry(&mut self, label: TreeId, init: TreeId, lo: TreeId, hi: TreeId, step: TreeId) -> TreeId;
    pub fn hbargraph(&mut self, label: TreeId, lo: TreeId, hi: TreeId, sig: TreeId) -> TreeId;
    pub fn vbargraph(&mut self, label: TreeId, lo: TreeId, hi: TreeId, sig: TreeId) -> TreeId;
    pub fn soundfile(&mut self, label: TreeId, chan: TreeId) -> TreeId;

    // Recursion
    pub fn rec(&mut self, body: TreeId) -> TreeId;
    pub fn proj(&mut self, index: i32, rec_group: TreeId) -> TreeId;

    // Extended functions
    pub fn xtended(&mut self, op: XtendedOp, args: &[TreeId]) -> TreeId;

    // Control/Enable
    pub fn select2(&mut self, sel: TreeId, a: TreeId, b: TreeId) -> TreeId;
    pub fn attach(&mut self, sig: TreeId, attached: TreeId) -> TreeId;
    pub fn control(&mut self, sig: TreeId, cond: TreeId) -> TreeId;
    pub fn enable(&mut self, sig: TreeId, cond: TreeId) -> TreeId;

    // Multi-rate
    pub fn on_demand(&mut self, sig: TreeId) -> TreeId;
    pub fn downsampling(&mut self, sig: TreeId, factor: TreeId) -> TreeId;
    pub fn upsampling(&mut self, sig: TreeId, factor: TreeId) -> TreeId;

    // Casts
    pub fn int_cast(&mut self, sig: TreeId) -> TreeId;
    pub fn float_cast(&mut self, sig: TreeId) -> TreeId;
}

/// Pattern matching for signals
pub enum SigMatch {
    Int(i32),
    Real(f64),
    Input(i32),
    BinOp(BinOp, TreeId, TreeId),
    Delay(TreeId, TreeId),
    Mem(TreeId),
    Table(TreeId, TreeId),
    RdTable(TreeId, TreeId),
    WrTable(TreeId, TreeId, TreeId, TreeId),
    Button(TreeId),
    HSlider(TreeId, TreeId, TreeId, TreeId, TreeId),
    // ... (exhaustive)
    Proj(i32, TreeId),
    Rec(TreeId),
    Xtended(XtendedOp, Vec<TreeId>),
    Select2(TreeId, TreeId, TreeId),
    Unknown,
}

pub fn match_sig(arena: &TreeArena, id: TreeId) -> SigMatch;
```

Alignment rule with Phase 2:
- `SigBuilder`/`match_sig` must mirror the canonical boxes API style (`BoxBuilder`/`match_box`).
- `propagate` is expected to consume boxes via `match_box` and produce signals via `SigBuilder`.
- Avoid duplicated dispatch ladders (`isBox*`-style clones) in Phase 4 modules.

#### 2.1.2 Binary operators

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add, Sub, Mul, Div, Rem,
    Lsh, Rsh, Gt, Lt, Ge, Le, Eq, Ne,
    And, Or, Xor,
}

impl BinOp {
    pub fn symbol(&self) -> &'static str;
    pub fn compute_int(&self, a: i32, b: i32) -> i32;
    pub fn compute_float(&self, a: f64, b: f64) -> f64;
    pub fn is_commutative(&self) -> bool;
    pub fn is_associative(&self) -> bool;
    pub fn neutral_element(&self) -> Option<f64>;
    pub fn wasm_op_f32(&self) -> &'static str;
    pub fn wasm_op_f64(&self) -> &'static str;
}
```

#### 2.1.3 Extended functions (xtended)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum XtendedOp {
    Sin, Cos, Tan,
    Asin, Acos, Atan, Atan2,
    Exp, Log, Log10, Exp10,
    Pow, Sqrt, Abs,
    Min, Max,
    Floor, Ceil, Rint, Round,
    Remainder, Fmod,
}

impl XtendedOp {
    pub fn name(&self) -> &'static str;
    pub fn arity(&self) -> usize;  // 1 or 2
    pub fn compute(&self, args: &[f64]) -> f64;
    pub fn compute_interval(&self, args: &[Interval]) -> Interval;
    pub fn result_nature(&self, arg_natures: &[Nature]) -> Nature;
}
```

> Each file `sinprim.hh`, `cosprim.hh`, etc. becomes a branch of the `XtendedOp` implementation. No more vtable, no more inheritance — a single enum + match.

#### 2.1.4 Signal type system

```rust
/// Signal nature (integer or float)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nature { Int, Real }

/// Variability
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Variability { Const = 0, Block = 1, Sample = 3 }

/// Computability
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Computability { Comp = 0, Init = 1, Exec = 3 }

/// Vectorizability
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vectorability { NonVec, Vec, ScalVec, TrueScal }

/// Audio type
#[derive(Clone, Debug)]
pub enum AudioType {
    Simple {
        nature: Nature,
        variability: Variability,
        computability: Computability,
        vectorability: Vectorability,
        boolean: bool,
        interval: Interval,
    },
    Table {
        content: Box<AudioType>,
    },
    Tuplet {
        components: Vec<AudioType>,
    },
}

impl AudioType {
    pub fn nature(&self) -> Nature;
    pub fn variability(&self) -> Variability;
    pub fn computability(&self) -> Computability;
    pub fn is_boolean(&self) -> bool;
    pub fn interval(&self) -> Interval;
}

/// Type inference
pub fn infer_sig_type(
    arena: &TreeArena,
    sig: TreeId,
    env: &TypeEnv,
    cache: &mut TreeProperty<AudioType>,
) -> AudioType;
```

#### 2.1.5 signals — Recommended restructuring during the Rust port

The audit of `signals/` (`signals.cpp`, `sigtyperules.cpp`, `sigtype.cpp`, `subsignals.cpp`, `ppsig.cpp`, `sigorderrules.cpp`, `clkEnvInference.cpp`) shows high-value simplifications:

1. Replace duplicated `isSig*` dispatch chains with one canonical typed signal-node model (and shared traversal helpers).
2. Unify node semantics used by typing, order inference, sub-signal extraction/rebuild, and printers to avoid cross-module drift.
3. Replace `gGlobal->nil`-sentinel encodings (rdtable/rwtable, OD/US/DS branch conventions, clock-env list layout) with explicit Rust enums/struct variants.
4. Move annotation side effects (`type`, `order`, `recursiveness`, `sharing`, `clkEnv`) from Tree-global properties to session-scoped analysis stores.
5. Split `sigtyperules.cpp` into focused passes (core inference, recursive fixpoint/widening, FIR/IIR gain, UI/soundfile checks, diagnostics).
6. Route typing options (`causality`, narrowing/widening limits, diagnostics) through explicit config/context objects instead of mutable globals.
7. Replace `sigtype` inheritance + `dynamic_cast` + global memoized type table with immutable Rust enums and a dedicated interned `TypeId` store.
8. Merge printer variants (`ppsig`, `ppsigShared`, `sigprint`) into one renderer with formatting modes, not duplicated signal-case ladders.
9. Isolate debug/testing helpers from production logic (`testFIR`, ad hoc `std::cerr` tracing in core signal helpers).
10. Bound and cache expensive FIR/IIR gain computations explicitly in inference to keep complexity predictable on large coefficient sets.

Recommended rollout:

1. Lock behavior first with differential tests on signal construction, typing, and pretty-printing.
2. Introduce the canonical signal-node dispatch layer and migrate subsystems one by one.
3. Move annotation caches/config into session context, then split type inference and optimize FIR/IIR gain paths.

### 2.2 eval

```rust
/// Evaluation environment (name → tree binding)
#[derive(Clone)]
pub struct Environment {
    bindings: Vec<(SymId, TreeId)>,
    parent: Option<Box<Environment>>,
}

impl Environment {
    pub fn empty() -> Self;
    pub fn bind(&mut self, name: SymId, value: TreeId);
    pub fn lookup(&self, name: SymId) -> Option<TreeId>;
    pub fn push_scope(&self) -> Self;
}

/// Infinite loop detector
pub struct LoopDetector {
    call_stack: Vec<TreeId>,
    max_depth: usize,
}

/// Evaluation of a Faust program (boxes → resolved boxes)
pub fn eval_process(
    arena: &mut TreeArena,
    definitions: TreeId,
    diag: &mut DiagnosticCollector,
) -> Result<TreeId, FaustError>;

/// Complete evaluation of a box expression in an environment
pub fn eval_box(
    arena: &mut TreeArena,
    expr: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
    diag: &mut DiagnosticCollector,
) -> Result<TreeId, FaustError>;
```

The evaluator manages:
- Name resolution (`boxIdent` → definition)
- Expanding function calls
- Compile-time iterations (`par(i,n,...)`, `seq(i,n,...)`, `sum(i,n,...)`, `prod(i,n,...)`)
- Evaluation of constants
- Pattern matching (`case { ... }`)
- Components and imports (`component("file.dsp")`)
- Environments and `with { ... }`

### 2.3 Pattern Matcher

```rust
/// Compiled pattern matching automaton
pub struct PatternMatcher {
    states: Vec<PatternState>,
    rules: Vec<PatternRule>,
}

pub struct PatternRule {
    patterns: Vec<TreeId>,  // the patterns
    body: TreeId,           // the result expression
}

impl PatternMatcher {
    /// Compile a list of rules into an automaton
    pub fn compile(
        arena: &TreeArena,
        rules: &[(Vec<TreeId>, TreeId)],
    ) -> Self;

    /// Execute the pattern matching
    pub fn apply(
        &self,
        arena: &mut TreeArena,
        args: &[TreeId],
        env: &Environment,
    ) -> Option<TreeId>;
}
```

### 2.4 propagation

```rust
/// Propagation: boxes → signals
///
/// Propagates N input signals through a block diagram to obtain
/// M output signals.
pub fn propagate(
    arena: &mut TreeArena,
    box_tree: TreeId,
    inputs: &[TreeId],    // N input signals
    diag: &mut DiagnosticCollector,
) -> Result<Vec<TreeId>, FaustError>;  // M output signals

/// UI label management with group paths
pub struct LabelManager {
    group_stack: Vec<(GroupType, String)>,
}

#[derive(Clone, Copy)]
pub enum GroupType { Horizontal, Vertical, Tab }

impl LabelManager {
    pub fn push_group(&mut self, kind: GroupType, label: &str);
    pub fn pop_group(&mut self);
    pub fn current_path(&self) -> String;
    pub fn make_full_label(&self, widget_label: &str) -> TreeId;
}
```

Propagation is the key point that transforms the **functional block language** into a **signal graph**. This is an interpretation:
- `boxSeq(A, B)` → output of A propagated as input of B
- `boxPar(A, B)` → concatenation of outputs
- `boxSplit(A, B)` → duplication of outputs of A
- `boxMerge(A, B)` → addition of entries of B
- `boxRec(A, B)` → creation of recursive signals (`sigRec`, `sigProj`)

### 2.5 Diagnostics integration for `eval` + `propagate`

The current Rust Phase 4 implementation already has typed error enums (`EvalError`, `PropagateError`). The remaining work is to map them to the common diagnostics model with source labels and stable codes.

Required integration points:

1. `eval` errors:
- map top-priority classes to `FRS-EVAL-*` diagnostic codes.
- attach definition/use source labels when parser metadata is available.
- include short actionable help for common user mistakes (undefined symbol, bad iterator count, non-identifier parameter).

2. `propagate` errors:
- map arity/unsupported-node classes to `FRS-PROP-*` diagnostic codes.
- include expected/got details as structured notes (not only formatted strings).
- attach source labels on the offending box node when available.

3. compiler orchestration:
- preserve stage and code information when aggregating Phase 4 errors.
- avoid reducing failures to generic string messages in CLI/API paths.

Pass criterion for this subsection:
- negative corpus tests for eval/propagate assert `(stage, code, severity, location)` stability.
- phase-level outputs are available in human and machine-readable formats.

Reference model:
- `porting/faust-rust-diagnostics-model-en.md` (sections 4.1, 5.3, 5.4, 6-D/E).

---

## 3. Dependencies

```
signals    → tlib, errors, interval
eval       → tlib, boxes, errors
propagate  → tlib, boxes, signals, eval, errors
```

The order of development within the phase:
0. Confirm canonical boxes read/write API is available (`BoxBuilder` + `match_box`).
1. `signals` (types + constructors)
2. `eval` (box evaluation)
3. `propagate` (boxes → signals)

---

## 4. Known pitfalls

### 4.1 Recursion in the evaluator
`eval.cpp` is the most complex file in this phase (1,600 lines). It handles subtle cases:
- Lazy evaluation of `letrec`
- Infinite loop detection (important for poorly formed Faust programs)
- Managing closures and nested environments

> Document each case well, add tests for borderline cases.

### 4.2 `xtended` and the void* pattern
In C++, extended functions are stored as `void*` in tree nodes via `sigXtended(xtended*, ...)`. In Rust, we replace with an `XtendedOp` enum which can be stored in an `NodeValue`.

### 4.3 sigRec / sigProj and de-Bruijn
Recursive signals use a de-Bruijn representation and then are converted to symbolic form. This is already managed in `tlib` (Phase 1), but we must ensure that the `propagate` functions which create `sigRec`/`sigProj` correctly use this conversion.

### 4.4 Type inference is a prerequisite for normalization
`sigtyperules.cpp` must be functional before Phase 5 (standardization). Types are used for:
- Determine if a signal is integer or real (for implicit casts)
- Calculate variability (const/block/sample)
- Determine intervals (for optimizations)

### 4.5 Multi-rate and ClkEnvInference
The new multi-rate system (`clkEnvInference`) is recent. It will be necessary to understand its interaction with propagation and typing.

### 4.6 Global state in eval.cpp
The evaluator massively accesses `gGlobal` to:
- Compilation options (`gDetailsSwitch`, `gDrawSignals`, etc.)
- Global symbol tables
- Error counters

> Pass an `&EvalConfig` and an `&mut DiagnosticCollector` explicitly.

### 4.7 Duplicate signal dispatch logic across core modules
`sigtyperules`, `sigorderrules`, `subsignals`, `ppsig`/`ppsigShared`, and `sigprint` each reimplement large `isSig*` ladders. This creates high drift risk and expensive maintenance.

### 4.8 Annotation coupling through Tree properties and mutable globals
Signal analyses write/read global properties (`TYPEPROPERTY`, `ORDERPROP`, `RECURSIVNESS`, sharing keys, `CLKENVPROPERTY`) and depend on mutable global knobs, making pass composition less explicit.

### 4.9 Sentinel-based signal encodings and incomplete OD/US/DS handling
Several semantics are encoded via `gGlobal->nil` sentinels, and `subsignals.cpp` still contains TODOs around OD/US/DS branch interpretation.

### 4.10 Type inference monolith and expensive FIR/IIR gain paths
`sigtyperules.cpp` combines many responsibilities in one file, and IIR gain inference explores coefficient corners exponentially with the number of variable coefficients.

---

## 5. Testing

### 5.1 signals
- **Unit**: Creation of each type of signal, pattern matching round-trip
- **Unit**: `BinOp::compute_int` and `compute_float` for the 13 operators
- **Unit**: `XtendedOp::compute` for the 22 functions
- **Unit**: Type inference on known signals
- **Unit**: Pretty-printing vs expected strings

### 5.2 eval
- **Unit**: Evaluation of simple definitions (`foo = 42;`)
- **Unit**: Name resolution with nested environments
- **Unit**: Iterations (`par(i,4,_)` → `_,_,_,_`)
- **Unit**: Pattern matching (`case { (x) => x+1; }`)
- **Unit**: Detection of infinite loops
- **Integration**: Evaluation of complete Faust files from the standard library

### 5.3 propagation
- **Unit**: Propagation of primitives (`+` with 2 inputs → 1 output)
- **Unit**: Sequential composition (`_ : +` → 2 inputs, 1 output)
- **Unit**: Parallel composition (`_, _` → 2 inputs, 2 outputs)
- **Unit**: Recursion (`+ ~ _` → 1 input, 1 output)
- **Differential**: Compare the signals produced with C++ on 20+ examples

---

## 6. "Done" criteria

- [ ] The ~183 constructors/destructors of carried signals
- [ ] The 22 functional `xtended` functions with interval calculation
- [ ] Complete type inference (nature, variability, computability, interval)
- [ ] The evaluator resolves names, expands iterations, manages pattern matching
- [ ] Propagation converts boxes into signals correctly
- [ ] `process = + ~ _;` produces the expected signals
- [ ] Standard Faust examples pass the parser → eval → propagate pipeline
- [ ] One canonical signal-node dispatch layer is used by typing, ordering, sub-signal transforms, and printers
- [ ] Box-to-signal boundary uses canonical dispatch (`match_box` in `propagate`)
- [ ] Signal variants currently encoded with sentinels are represented explicitly (no semantic `nil` separators in core signal APIs)
- [ ] Signal annotation caches are session-scoped and explicit (no hidden cross-pass property coupling)
- [ ] Type inference is split into focused passes with explicit configuration/context
- [ ] FIR/IIR gain inference has explicit cost bounds/caching and dedicated regression coverage
- [ ] No `gGlobal`, no static state
- [ ] `Send + Sync` checked on all crates

---

## 7. Detailed Effort

| Crate | LOC C++ | Estimated LOC Rust | Days |
|-------|---------|-----------------|-------|
| signals (signals + types + binop) | 10,596 | 6,000–7,000 | 15–18 |
| signals (xtended) | 2,847 | 1200–1500 | 3–4 |
| eval (eval + env + loopDetector + patternmatcher) | 3,254 | 2,500–3,000 | 8–10 |
| propagate (propagate + labels) | 1,347 | 1000–1200 | 4–5 |
| Tests + docs | — | 1,500 | 4–5 |
| **Total Phase 4** | **18,044** | **12,200–14,200** | **34–42** |
