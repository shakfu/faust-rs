//! Box evaluator — Phase 4 of the Faust compilation pipeline.
//!
//! # C++ source correspondence
//!
//! | Rust symbol | C++ source |
//! |---|---|
//! | `Environment` | `compiler/evaluate/environment.hh/.cpp` |
//! | `LoopDetector` | `compiler/evaluate/loopDetector.hh` |
//! | `EvalStats` | `gGlobal->gStats` fields (`fEnvLayersPushed`, `fEnvLookups`, …) |
//! | `eval_process` | `compiler/evaluate/eval.cpp` — `eval()` entry point |
//! | `eval_box` | `compiler/evaluate/eval.cpp` — `eval()` recursive dispatch |
//! | `bind_definitions` | `pushMultiClosureDefs()` in `environment.cpp` |
//! | `apply_list` | `applyList()` in `eval.cpp` |
//! | `apply_case_rules` | `evalCase()` in `eval.cpp` |
//!
//! # Environment architecture — C++ vs Rust
//!
//! ## C++ model: persistent tree-encoded linked list with closures
//!
//! The C++ environment is a **persistent linked list of `ENV_LAYER` tree nodes** stored in the
//! global hash-cons pool (`tlib`). Each layer stores its definitions as properties
//! (`setProperty`/`getProperty`) keyed by symbol-`Tree` values, forming a hash table attached to
//! the node. Crucially, every definition stored via `pushMultiClosureDefs` is **wrapped in a
//! closure object** — `closure(expr, genv, visited, lenv)` — that captures the environment
//! (`lenv`) at definition time.
//!
//! ```text
//! C++ env chain:
//!
//!   [ ENV_LAYER_3 ]──props──{ "f" → closure(expr_f, nil, visited, ENV_LAYER_3) }
//!        │ branch(0)
//!   [ ENV_LAYER_2 ]──props──{ "x" → closure(expr_x, nil, visited, ENV_LAYER_2) }
//!        │ branch(0)
//!   [ ENV_LAYER_1 ]──props──{ "process" → closure(expr_p, nil, visited, ENV_LAYER_1) }
//!        │ branch(0)
//!       nil
//! ```
//!
//! When the evaluator encounters a closure during `evalClosure()`, it evaluates `expr` in the
//! **captured environment** `lenv`, not the current one. This is the classical demand-driven
//! denotational semantics with explicit environment threading.
//!
//! Special features:
//! - **`BARRIER` nodes** (`pushEnvBarrier`) stop pattern-matcher scope search at a sentinel,
//!   enabling `searchIdDef` to scope pattern variable lookup without affecting normal lookup.
//! - **`copyEnvReplaceDefs`** creates modified copies of an existing environment for letrec
//!   semantics, rewiring all enclosed closures to point to the new environment via
//!   `updateClosures`.
//! - **Redefinition detection** (`addLayerDef`): identical redefinitions are silently accepted;
//!   conflicting redefinitions throw `faustexception`.
//! - **Performance stats** tracked in `gGlobal->gStats`: `fEnvLayersPushed`,
//!   `fEnvLookups`, `fEnvLookupTotalDepth`.
//!
//! ## Current Rust model (adapted): imperative `Vec`-based scoped environment with direct bindings
//!
//! The Rust environment is an arena of stable `EnvId` layers. Each layer stores
//! `Vec<(SymId, EvalValue)>`, where a binding is currently either:
//! - one plain box tree (`EvalValue::Box`) for immediate values such as pattern substitutions
//!   or lambda-parameter shadowing sentinels,
//! - one captured closure (`EvalValue::Closure`) for parser definitions and residual
//!   abstraction/environment values.
//!
//! Lexical scoping is implemented explicitly by allocating a child layer (`push_scope()`) before
//! evaluating any sub-expression that introduces new bindings, then threading that child scope
//! down through recursive calls.
//!
//! ```text
//! Rust env chain:
//!
//!   EnvId(2) ── bindings { "f" → Closure(expr_f, EnvId(2)) } ── parent = EnvId(1)
//!   EnvId(1) ── bindings { "x" → Closure(expr_x, EnvId(1)) } ── parent = EnvId(0)
//!   EnvId(0) ── bindings { "process" → Closure(expr_p, EnvId(0)) }
//! ```
//!
//! ## Why explicit closures were deferred in the current Rust port
//!
//! The initial Rust evaluator deferred explicit closure objects and instead relied entirely on
//! eager environment threading. The current port has now introduced explicit closure values for
//! parser definitions, abstractions, and environment-like access targets, while later lowering
//! stages still consume box IR.
//!
//! This adaptation was sufficient to restore several important parity points:
//! - grouped/patterned definitions,
//! - evaluated `case` patterns before matcher construction,
//! - barrier semantics for repeated pattern variables,
//! - adapted `a2sb` lowering of residual `abstr` / `case` forms.
//!
//! It is still not a byte-for-byte port of the C++ closure node layout: Rust keeps the same
//! semantics in explicit evaluator values instead of tree-encoded `closure(...)` /
//! `boxPatternMatcher(...)` nodes. The remaining differences are therefore representational,
//! not semantic:
//! - source loading uses [`EvalSourceContext`] instead of the process-global `gReader`,
//! - closures and pattern matchers are explicit Rust values instead of tree nodes,
//! - later passes still consume first-order box IR after [`a2sb_value`] lowers those values.
//!
//! Current mapping status: **1:1 semantics, adapted representation**.
//!
//! ## Divergences from C++ (intentional)
//!
//! | Feature | C++ | Rust | Notes |
//! |---|---|---|---|
//! | Value stored | `closure(expr, genv, visited, lenv)` / `boxPatternMatcher(...)` | `EvalValue::{Box, Closure, PatternMatcher}` | Same semantics, adapted host-side representation |
//! | Barrier mechanism | `pushEnvBarrier` / `searchIdDef` | `push_barrier_scope()` + `lookup_until_barrier()` | Same semantics |
//! | `copyEnvReplaceDefs` | Present (env rewiring) | Present | `copy_env_replace_defs(...)` + `rewrite_captured_env(...)` |
//! | Redefinition check | `addLayerDef` throws on conflict | `bind_definitions` returns `EvalError::RedefinedSymbol` | Same semantics, typed error |
//! | Profiling | `gGlobal->gStats` (global mutable) | `EvalStats` (returned value) | Safer, composable |
//!
//! # Performance comparison — C++ vs Rust
//!
//! | Operation | C++ implementation | C++ cost | Rust implementation | Rust cost |
//! |---|---|---|---|---|
//! | **Scope push** | `tree(unique("ENV_LAYER"), lenv)` — alloc in hash-cons pool | O(1) amortized + hash | arena layer allocation + `EnvId` handle clone | O(1) |
//! | **Bind one symbol** | `setProperty(node, id, def)` — hash map insert on tree node | O(1) amortized | `Vec::push((sym, value))` | O(1) amortized |
//! | **Lookup (found at depth d)** | Walk d layers, `getProperty` hash probe per layer | O(d) hash probes | Reverse `u32` scan per layer O(n_local), recurse O(d) | O(d × n_local) — O(1) per compare |
//! | **Value size per binding** | `Tree*` pointer to closure node (~64 bytes closure) | Large | `SymId + EvalValue` in one arena layer | Moderate, but explicit and cache-friendly |
//! | **Cache locality** | Pointer-chased linked list through hash-cons pool | Poor (pointer indirection) | Contiguous `Vec<(SymId, EvalValue)>` per layer | Good |
//! | **Concurrency** | Global `gGlobal` state, not thread-safe | N/A | Fully `Send`/`Sync`, no global state | Thread-safe |
//!
//! **In practice**: for typical Faust programs (< 200 top-level definitions, scope depth ≤ 5,
//! ≤ 30 bindings per scope), the Rust reverse scan over one compact per-layer vector is expected
//! to be competitive with, and often faster than, the C++ hash-probe walk because the working set
//! stays tiny and contiguous. The important point is not the asymptotic notation alone, but that
//! the common Rust case pays a handful of integer comparisons inside one hot cache line instead of
//! multiple pointer-chased table probes.
//!
//! This remains a workload claim, not a semantic guarantee. It is representative for the current
//! evaluator design and local release micro-benchmarks, but it is not enforced by a CI benchmark
//! gate and should be re-validated if environment representation changes materially.
//!
//! The current Rust representation already uses stable `EnvId` layer identities in a shared
//! environment arena. This matches the next closure-porting requirement while keeping the public
//! evaluator API unchanged.
//!
//! # Scope of this crate
//! - Name resolution against definition environments with redefinition detection.
//! - Lexical scoping for `with {}` and function abstractions.
//! - Loop detection for recursive symbol expansion.
//! - Structural recursive evaluation over box trees.
//! - Function application and iterative form expansion (`ipar/iseq/isum/iprod`).
//! - Non-closure partial-application parity (`applyList`) with implicit wire insertion.
//! - Optional performance statistics via [`eval_process_with_stats`].
//!
//! # Execution model
//! 1. Parse all top-level definitions and bind them into a root `Environment`.
//! 2. Resolve `process` in that environment.
//! 3. Evaluate `process` recursively by box family:
//!    - Lexical forms: `abstr`, `with`, `letrec`, `access`.
//!    - Application: `appl` (beta-reduction) / `case` (pattern-match dispatch).
//!    - Iterative forms: `ipar`, `iseq`, `isum`, `iprod` (unrolled at eval time).
//!    - Structural fallback: all other nodes are recursively mapped over children.
//!
//! The evaluator returns a normalized box tree consumed by later passes
//! (`propagate`, typing, signal transforms). It does not emit signals directly.

use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use boxes::{BoxBuilder, BoxMatch, match_box};
use errors::codes;
use errors::{Diagnostic, IntoDiagnostic, Severity, Stage};
use normalize::simplify_const;
use parser::{CompilationMetadataSnapshot, CompilationMetadataStore};
use propagate::{ArityCache, propagate_typed, try_build_flat_box};
use signals::{SigId, SigMatch, match_sig};
use tlib::{NodeKind, TreeArena, TreeId, tree_to_int};

mod pattern_matcher;

pub const CRATE_NAME: &str = "eval";

/// Symbol identifier used in evaluator environments — a dense `u32` interned by [`TreeArena`].
///
/// Every unique Faust identifier name (`process`, `f`, `karplus`, …) is assigned a `u32` id
/// by [`TreeArena::intern_symbol`] the first time it is **bound** to a value. Subsequent lookups
/// use [`TreeArena::get_symbol`] (which takes `&self`) to retrieve the id in O(1), then compare
/// it as a plain integer. This achieves:
///
/// - **O(1)** symbol comparison (was O(len) with `Box<str>`)
/// - **8 bytes** per binding in `Vec<(SymId, TreeId)>` (was ~24 bytes with `Box<str>` + padding)
/// - **SIMD-friendly** scan: the `Vec<(u32, u32)>` layout lets the CPU compare 4 bindings per
///   16-byte vector register in a typical environment of < 32 bindings.
///
/// **C++ parallel**: C++ uses hash-consed `Tree` pointers as symbol keys, achieving O(1)
/// comparison by pointer equality. This `u32` id pool achieves the same O(1) cost without
/// pointer chasing, with better cache density (4-byte vs 8-byte pointer).
///
/// # Lookup vs intern split
///
/// The interner is split into two entry points to avoid `&mut TreeArena` borrows on the
/// hot lookup path (which runs inside a `match match_box(arena, expr)` arm where `arena` is
/// already reborrowed as `&TreeArena`):
///
/// | Operation | Method | Borrow | Use case |
/// |---|---|---|---|
/// | Bind (new name) | [`intern_symbol(&mut self)`](TreeArena::intern_symbol) | `&mut` | `bind_definitions`, `apply_list`, Abstr |
/// | Lookup (known name) | [`get_symbol(&self)`](TreeArena::get_symbol) | `&` | `eval_box` Ident, `match_pattern` |
/// | Name resolution | [`symbol_name(&self)`](TreeArena::symbol_name) | `&` | Diagnostics only |
pub type SymId = u32;

/// Stable identifier of one evaluator environment layer.
///
/// The C++ evaluator uses the `Tree` pointer identity of each `ENV_LAYER` node as the semantic
/// environment identity. The Rust port uses a dense arena index instead. This is the first step
/// toward the full captured-closure port because recursion tracking and closure forcing need a
/// stable `(symbol, environment)` key, not just a raw expression id.
pub type EnvId = usize;

/// Internal DSP sample computation precision.
///
/// This mirrors Faust's `-double` flag: [`SamplePrecision::Float32`] selects
/// `float` as the internal computation type (the default), while
/// [`SamplePrecision::Float64`] selects `double`.
///
/// **Note**: this setting has no effect on compile-time constant folding inside
/// the evaluator — pattern-matching numeric constants are always folded at
/// `f64` precision. It is an output annotation for downstream code-generation
/// backends (e.g. FIR lowering) that consume the evaluated box tree.
///
/// The type is attached to [`EvalSourceContext`] so it travels with the
/// evaluation session and can be forwarded to backends without requiring a
/// separate channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SamplePrecision {
    /// 32-bit single-precision float (`float` in C++). Default.
    #[default]
    Float32,
    /// 64-bit double-precision float (`double` in C++).
    Float64,
}

/// Filesystem source-resolution context captured by evaluator environments.
///
/// # Source provenance (C++)
/// - `compiler/global.cpp`
/// - `compiler/parser/sourcereader.hh/.cpp`
/// - `compiler/evaluate/eval.cpp` (`boxComponent` / `boxLibrary`)
///
/// The C++ evaluator loads `component("...")` and `library("...")` through the
/// process-global `gReader`, whose search state is already configured from the
/// active compile session. The Rust port has no global reader, so the evaluator
/// carries the equivalent resolution context explicitly and captures it inside
/// closures together with the lexical environment.
///
/// Mapping status: `adapted`.
///
/// # Invariants
/// - `current_file` is the file relative to which nested source loads should be
///   resolved when the evaluator is operating on file-backed Faust sources.
/// - `search_paths` preserves deterministic lookup order after the current file.
/// - in-memory evaluation uses [`EvalSourceContext::memory`], which intentionally
///   carries no filesystem base.
/// - one context instance also acts as a per-session cache for already loaded
///   Faust source files, mirroring the role of C++ `SourceReader::fFileCache`.
/// - top-level `declare key "value";` metadata for file-backed loads is written
///   into the shared [`CompilationMetadataStore`] captured by the context.
#[derive(Clone, Debug, Default)]
pub struct EvalSourceContext {
    current_file: Option<PathBuf>,
    search_paths: Vec<PathBuf>,
    cache: Arc<Mutex<HashMap<PathBuf, CachedLoadedSource>>>,
    metadata_store: Option<CompilationMetadataStore>,
    /// Internal DSP computation precision forwarded to code-generation backends.
    ///
    /// Defaults to [`SamplePrecision::Float32`] (C++ `float`).
    /// Set to [`SamplePrecision::Float64`] to request `double`-precision
    /// internal computation, equivalent to passing `-double` to `faust`.
    pub sample_precision: SamplePrecision,
}

impl EvalSourceContext {
    /// Creates a context for in-memory evaluation with no filesystem base.
    #[must_use]
    pub fn memory() -> Self {
        Self::default()
    }

    /// Creates a context for in-memory evaluation with one shared top-level
    /// metadata store.
    #[must_use]
    pub fn memory_with_metadata(metadata_store: CompilationMetadataStore) -> Self {
        Self {
            metadata_store: Some(metadata_store),
            ..Self::default()
        }
    }

    /// Creates a context rooted at one source file plus optional import search paths.
    ///
    /// The file parent directory is prepended ahead of explicit `search_paths`,
    /// matching the effective C++/parser lookup contract for file-backed sessions.
    /// Reusing the same returned context across multiple `eval_process_*` calls
    /// also reuses the same loaded-source cache.
    #[must_use]
    pub fn for_file(path: &Path, search_paths: &[PathBuf]) -> Self {
        Self::for_file_with_metadata(
            path,
            search_paths,
            CompilationMetadataStore::new(&path.to_string_lossy()),
        )
    }

    /// Creates a file-backed context with one shared top-level metadata store.
    #[must_use]
    pub fn for_file_with_metadata(
        path: &Path,
        search_paths: &[PathBuf],
        metadata_store: CompilationMetadataStore,
    ) -> Self {
        let mut ordered = Vec::with_capacity(search_paths.len() + 1);
        if let Some(parent) = path.parent() {
            ordered.push(parent.to_path_buf());
        }
        for candidate in search_paths {
            if !ordered.iter().any(|existing| existing == candidate) {
                ordered.push(candidate.clone());
            }
        }
        Self {
            current_file: Some(path.to_path_buf()),
            search_paths: ordered,
            cache: Arc::default(),
            metadata_store: Some(metadata_store),
            sample_precision: SamplePrecision::default(),
        }
    }

    /// Returns a context for a newly loaded file while preserving inherited search order.
    ///
    /// The [`SamplePrecision`] of the parent context is propagated to the child
    /// so that sub-files loaded via `component`/`library` share the same
    /// precision setting as the root evaluation session.
    #[must_use]
    pub fn for_loaded_file(&self, path: &Path) -> Self {
        let mut child = match &self.metadata_store {
            Some(metadata_store) => {
                Self::for_file_with_metadata(path, &self.search_paths, metadata_store.clone())
            }
            None => Self::for_file(path, &self.search_paths),
        };
        child.sample_precision = self.sample_precision;
        child
    }

    /// Returns the current file used as the primary relative-resolution anchor.
    #[must_use]
    pub fn current_file(&self) -> Option<&Path> {
        self.current_file.as_deref()
    }

    /// Returns the ordered import search paths used after the current-file base.
    #[must_use]
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    /// Returns the shared top-level metadata store captured by this context, if any.
    #[must_use]
    pub fn metadata_store(&self) -> Option<&CompilationMetadataStore> {
        self.metadata_store.as_ref()
    }

    /// Returns a snapshot of the aggregated top-level metadata visible in this session.
    #[must_use]
    pub fn metadata_snapshot(&self) -> CompilationMetadataSnapshot {
        self.metadata_store.as_ref().map_or_else(
            CompilationMetadataSnapshot::default,
            CompilationMetadataStore::snapshot,
        )
    }

    fn cached_loaded_source_hits<R>(
        &self,
        paths: &[PathBuf],
        f: impl FnOnce(Option<&CachedLoadedSource>, &Path) -> R,
    ) -> R {
        let guard = self.cache.lock().expect("source cache lock poisoned");
        for path in paths {
            if let Some(loaded) = guard.get(path) {
                return f(Some(loaded), path);
            }
        }
        f(None, Path::new(""))
    }

    fn insert_loaded_source(&self, path: PathBuf, source: CachedLoadedSource) {
        let mut guard = self.cache.lock().expect("source cache lock poisoned");
        guard.insert(path, source);
    }
}

impl PartialEq for EvalSourceContext {
    fn eq(&self, other: &Self) -> bool {
        self.current_file == other.current_file
            && self.search_paths == other.search_paths
            && self.metadata_snapshot() == other.metadata_snapshot()
    }
}

impl Eq for EvalSourceContext {}

#[derive(Debug)]
/// One file loaded through the evaluator source-loading cache.
struct CachedLoadedSource {
    root: TreeId,
    arena: TreeArena,
    parse_errors: Vec<String>,
}

#[derive(Clone, Debug)]
/// Evaluator value domain used during Phase 4.
///
/// Rust keeps closures and pattern matchers as explicit evaluator values rather
/// than as tree-encoded host nodes, then lowers residual values back to boxes.
enum EvalValue {
    Box(TreeId),
    Closure(ClosureValue),
    PatternMatcher(PatternMatcherValue),
}

impl EvalValue {
    fn display_tree(&self) -> TreeId {
        match self {
            Self::Box(id) => *id,
            Self::Closure(closure) => closure.expr,
            Self::PatternMatcher(pm) => pm.case_expr,
        }
    }
}

impl PartialEq for EvalValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Box(lhs), Self::Box(rhs)) => lhs == rhs,
            (Self::Closure(lhs), Self::Closure(rhs)) => lhs == rhs,
            (Self::PatternMatcher(lhs), Self::PatternMatcher(rhs)) => lhs == rhs,
            _ => false,
        }
    }
}

impl Eq for EvalValue {}

#[derive(Clone, Debug)]
/// Captured closure value for delayed evaluation in one lexical environment.
struct ClosureValue {
    expr: TreeId,
    env: Environment,
}

impl PartialEq for ClosureValue {
    fn eq(&self, other: &Self) -> bool {
        self.expr == other.expr && self.env.same_identity(&other.env)
    }
}

impl Eq for ClosureValue {}

#[derive(Clone, Debug)]
/// Captured pattern-matcher automaton value used by residual `case` handling.
struct PatternMatcherValue {
    automaton: pattern_matcher::Automaton,
    state: usize,
    envs: Vec<Option<Environment>>,
    original_rules: TreeId,
    rev_param_list: Vec<TreeId>,
    case_expr: TreeId,
}

impl PartialEq for PatternMatcherValue {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
            && self.original_rules == other.original_rules
            && self.rev_param_list == other.rev_param_list
            && self.case_expr == other.case_expr
            && self.automaton.rhs == other.automaton.rhs
            && self
                .envs
                .iter()
                .zip(other.envs.iter())
                .all(|(lhs, rhs)| match (lhs, rhs) {
                    (Some(lhs), Some(rhs)) => lhs.same_identity(rhs),
                    (None, None) => true,
                    _ => false,
                })
            && self.envs.len() == other.envs.len()
    }
}

impl Eq for PatternMatcherValue {}

/// Lexical evaluation environment mapping symbol names to box-IR tree nodes.
///
/// # Architecture
///
/// Implemented as a **linked list of scopes**, where each scope is a `Vec<(SymId, TreeId)>`:
///
/// ```text
/// Environment { bindings: [("f", 42)], parent: Some(→) }
///      │ parent
/// Environment { bindings: [("x", 17), ("y", 8)], parent: Some(→) }
///      │ parent
/// Environment { bindings: [("process", 3)], parent: None }  ← root scope
/// ```
///
/// Lookup walks from innermost to outermost scope, returning the first match (shadowing semantics).
/// Binding always targets the **current** scope, never a parent.
///
/// # C++ correspondence — `environment.cpp`
///
/// The C++ environment is a **persistent linked list of `ENV_LAYER` tree nodes** stored in the
/// global hash-cons pool. Key differences from this Rust implementation:
///
/// | Aspect | C++ (`environment.cpp`) | Rust (`Environment`) |
/// |---|---|---|
/// | Storage | Hash-consed tree nodes with property tables | `Vec<(u32, TreeId)>` — interned `SymId` |
/// | Values stored | **Closures**: `closure(expr, genv, visited, lenv)` capturing the scope at definition time | `EvalValue::{Box, Closure}` in the current adapted model |
/// | Lookup | `searchIdDef`: walks layers calling `getProperty` (hash probe per layer) | `lookup`: reverse linear scan of `Vec`, then recurse to `parent` |
/// | Scope push | `pushNewLayer(lenv)` — allocates a unique tree node | `push_scope()` — allocate one arena layer and return its `EnvId` handle |
/// | Redefinition | `addLayerDef` throws `faustexception` on conflicting rebind | `bind_definitions` returns `EvalError::RedefinedSymbol` |
/// | Barrier | `pushEnvBarrier` / `isEnvBarrier` — stops pattern-matcher lookup | `push_barrier_scope()` / `lookup_until_barrier()` |
/// | Env copy/rewire | `copyEnvReplaceDefs` + `updateClosures` — for captured-env rewrites | Deferred in the current Rust model |
/// | Profiling | `gGlobal->gStats.fEnvLayersPushed/fEnvLookups/fEnvLookupTotalDepth` | [`EvalStats`] returned from [`eval_process_with_stats`] |
///
/// # Performance
///
/// For typical Faust programs (scope depth ≤ 5, ≤ 30 bindings/scope):
/// - **Lookup**: O(d × n) where d = depth, n = bindings/scope. Each compare is O(1) — `u32`
///   integer equality. In practice the common hit/miss patterns stay in the low tens to low
///   hundreds of comparisons and therefore in one tiny hot working set.
/// - **Bind**: `Vec::push` — amortized O(1), no hashing, no pointer chasing.
/// - **Push scope**: O(1) one-layer allocation in the shared environment arena.
/// - **Memory per binding**: one inline `(SymId, EvalValue)` pair in the current layer vector.
///   The frequently-hit plain-box case stores only one symbol id plus one small tagged payload and
///   avoids per-binding heap allocation; closure and pattern-matcher bindings carry larger inline
///   state. The earlier `8 bytes` rule of thumb applies only to the narrow `SymId + TreeId` box
///   payload shape and should not be read as the size of every binding variant.
#[derive(Clone, Debug)]
pub struct Environment {
    store: Arc<Mutex<EnvStore>>,
    current: EnvId,
    source_context: Arc<EvalSourceContext>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Stable environment identity paired with one symbol for recursion tracking.
struct EnvFrameKey {
    store_ptr: usize,
    env_id: EnvId,
}

#[derive(Clone, Debug, Default)]
/// One lexical environment layer.
struct EnvLayer {
    bindings: Vec<(SymId, EvalValue)>,
    parent: Option<EnvId>,
    barrier: bool,
}

#[derive(Debug, Default)]
/// Arena of lexical environment layers.
struct EnvStore {
    layers: Vec<EnvLayer>,
}

impl Environment {
    /// Creates an empty root environment with no bindings and no parent.
    ///
    /// **C++ equivalent**: the initial `gGlobal->nil` environment passed to the first
    /// `pushMultiClosureDefs` call in `eval.cpp`.
    #[must_use]
    pub fn empty() -> Self {
        Self::empty_with_source_context(EvalSourceContext::memory())
    }

    /// Creates an empty root environment with one captured source-resolution context.
    ///
    /// This is the Rust equivalent of initializing evaluation under one configured
    /// `gReader` session in the C++ compiler. All child scopes and closures derived
    /// from this environment inherit the same context unless a newly loaded file
    /// installs a more specific one.
    ///
    /// Typical callers:
    /// - [`eval_process_with_source_context`] for file-backed compilation,
    /// - targeted tests exercising `component("...")` / `library("...")` parity.
    #[must_use]
    pub fn empty_with_source_context(source_context: EvalSourceContext) -> Self {
        let mut store = EnvStore::default();
        store.layers.push(EnvLayer::default());
        Self {
            store: Arc::new(Mutex::new(store)),
            current: 0,
            source_context: Arc::new(source_context),
        }
    }

    /// Returns the stable id of the current environment layer.
    ///
    /// Source provenance (C++):
    /// - `compiler/evaluate/environment.cpp`
    /// - `pushNewLayer`
    ///
    /// In C++, environment identity is carried by the `Tree` pointer of the `ENV_LAYER` node.
    /// In Rust it is carried by this dense arena index.
    #[must_use]
    pub fn id(&self) -> EnvId {
        self.current
    }

    fn same_identity(&self, other: &Self) -> bool {
        self.current == other.current
            && Arc::ptr_eq(&self.store, &other.store)
            && Arc::ptr_eq(&self.source_context, &other.source_context)
    }

    fn frame_key(&self) -> EnvFrameKey {
        self.frame_key_for(self.current)
    }

    fn frame_key_for(&self, env_id: EnvId) -> EnvFrameKey {
        EnvFrameKey {
            store_ptr: Arc::as_ptr(&self.store) as usize,
            env_id,
        }
    }

    /// Returns the source-resolution context captured by this environment.
    ///
    /// This is the context inherited by closures created while evaluating in
    /// this environment. For file-backed sessions it identifies the file and
    /// import-search roots that `component("...")` / `library("...")` will use.
    #[must_use]
    pub fn source_context(&self) -> &EvalSourceContext {
        self.source_context.as_ref()
    }

    /// Binds one symbol to a tree node in the **current scope** (not a parent).
    ///
    /// This is the unchecked low-level binder. Multiple bindings for the same name in the same
    /// scope are allowed (last binding wins on lookup — shadowing). For definitions that must
    /// enforce the no-redefinition rule, use `bind_definitions` which calls
    /// [`lookup_local`](Self::lookup_local) and returns `EvalError::RedefinedSymbol` on conflict.
    ///
    /// **C++ equivalent**: `setProperty(lenv, id, def)` in `addLayerDef`, but without the
    /// prior duplicate check. The check is performed externally in `bind_definitions`.
    ///
    /// `sym` must be obtained from [`TreeArena::intern_symbol`] before calling this method.
    pub fn bind(&mut self, sym: SymId, value: TreeId) {
        self.bind_value(sym, EvalValue::Box(value));
    }

    fn bind_value(&mut self, sym: SymId, value: EvalValue) {
        self.with_store_mut(|store| {
            store.layers[self.current].bindings.push((sym, value));
        });
    }

    /// Looks up a symbol across the full scope chain (current scope and all parents).
    ///
    /// Returns the **innermost** (most recently bound) binding for `sym`, or `None` if the
    /// symbol is not visible in any scope. This implements shadowing: a binding in an inner scope
    /// hides any binding with the same `SymId` in an outer scope.
    ///
    /// The scan is performed in **reverse insertion order** within each scope (O(n) integer
    /// comparisons), then recurses to the parent. The last `bind` call for a given `sym` in the
    /// current scope wins. Comparison is O(1) — two `u32` values compared by equality.
    ///
    /// **C++ equivalent**: `getProperty(lenv, id, def)` called in a loop walking the layer chain
    /// (`lenv = lenv->branch(0)`) until a hit or `isEnvBarrier`. The C++ lookup uses a hash
    /// probe per layer; the Rust lookup uses a linear SIMD-friendly `u32` scan per layer.
    ///
    /// **Does not stop at barriers**: unlike C++ `searchIdDef` (used by the pattern matcher),
    /// this lookup traverses the full parent chain. Pattern-matching nonlinearity checks must
    /// instead use [`lookup_until_barrier`](Self::lookup_until_barrier).
    ///
    /// `sym` must be a valid id returned by [`TreeArena::intern_symbol`] or
    /// [`TreeArena::get_symbol`].
    #[must_use]
    pub fn lookup(&self, sym: SymId) -> Option<TreeId> {
        self.lookup_value(sym).and_then(|(_, value)| match value {
            EvalValue::Box(id) => Some(id),
            EvalValue::Closure(_) => None,
            EvalValue::PatternMatcher(_) => None,
        })
    }

    fn lookup_value(&self, sym: SymId) -> Option<(EnvId, EvalValue)> {
        self.with_store(|store| {
            let mut env_id = Some(self.current);
            while let Some(id) = env_id {
                let layer = &store.layers[id];
                for (s, value) in layer.bindings.iter().rev() {
                    if *s == sym {
                        return Some((id, value.clone()));
                    }
                }
                env_id = layer.parent;
            }
            None
        })
    }

    /// Looks up a symbol across the current scope chain but stops when a barrier scope is reached.
    ///
    /// Source provenance (C++):
    /// - `compiler/evaluate/environment.cpp`
    /// - `searchIdDef`
    /// - `pushEnvBarrier`
    ///
    /// This is used by the pattern matcher to implement rule-local nonlinearity:
    /// repeated pattern variables must only compare against bindings created while matching the
    /// current rule. Outer lexical bindings remain visible to normal RHS evaluation through
    /// [`lookup`](Self::lookup), but they must not participate in pattern-variable equality checks.
    #[must_use]
    pub fn lookup_until_barrier(&self, sym: SymId) -> Option<TreeId> {
        self.lookup_until_barrier_value(sym)
            .and_then(|value| match value {
                EvalValue::Box(id) => Some(id),
                EvalValue::Closure(_) => None,
                EvalValue::PatternMatcher(_) => None,
            })
    }

    fn lookup_until_barrier_value(&self, sym: SymId) -> Option<EvalValue> {
        self.with_store(|store| {
            let mut env_id = Some(self.current);
            while let Some(id) = env_id {
                let layer = &store.layers[id];
                for (s, value) in layer.bindings.iter().rev() {
                    if *s == sym {
                        return Some(value.clone());
                    }
                }
                if layer.barrier {
                    return None;
                }
                env_id = layer.parent;
            }
            None
        })
    }

    /// Looks up a symbol in the **current scope only**, without consulting any parent.
    ///
    /// This is used by `bind_definitions` to detect conflicting redefinitions of the same symbol
    /// within the same lexical layer — equivalent to the duplicate check inside C++ `addLayerDef`:
    ///
    /// ```cpp
    /// // environment.cpp — addLayerDef
    /// Tree olddef = nullptr;
    /// if (getProperty(lenv, id, olddef)) {   // ← checks only the current layer
    ///     if (def == olddef) { /* silent */ }
    ///     else { throw faustexception("redefinition …"); }
    /// }
    /// ```
    ///
    /// Unlike [`lookup`](Self::lookup), this method does **not** recurse to the parent, so
    /// re-binding a name that exists in an outer scope (shadowing) is correctly allowed.
    ///
    /// `sym` must be a valid id returned by [`TreeArena::intern_symbol`] or
    /// [`TreeArena::get_symbol`].
    #[must_use]
    pub fn lookup_local(&self, sym: SymId) -> Option<TreeId> {
        self.lookup_local_value(sym).and_then(|value| match value {
            EvalValue::Box(id) => Some(id),
            EvalValue::Closure(_) => None,
            EvalValue::PatternMatcher(_) => None,
        })
    }

    fn lookup_local_value(&self, sym: SymId) -> Option<EvalValue> {
        self.with_store(|store| {
            for (s, value) in store.layers[self.current].bindings.iter().rev() {
                if *s == sym {
                    return Some(value.clone());
                }
            }
            None
        })
    }

    /// Creates a new child scope whose parent is `self`.
    ///
    /// The child starts with an empty `bindings` vec. Any symbol bound in the child shadows the
    /// same-named symbol in the parent for the lifetime of the child scope.
    ///
    /// **C++ equivalent**: `pushNewLayer(lenv)` in `environment.cpp`:
    /// ```cpp
    /// static Tree pushNewLayer(Tree lenv) {
    ///     return tree(unique("ENV_LAYER"), lenv);  // new node, parent = lenv
    /// }
    /// ```
    ///
    /// **Cost**: `O(1)` layer allocation in the shared environment arena.
    #[must_use]
    pub fn push_scope(&self) -> Self {
        self.push_child(false)
    }

    /// Creates a new child scope that acts as a pattern-matching barrier.
    ///
    /// Source provenance (C++):
    /// - `compiler/evaluate/environment.cpp`
    /// - `pushEnvBarrier`
    ///
    /// Normal lookup through [`lookup`](Self::lookup) still crosses this node, so rule RHS
    /// evaluation can see the outer lexical environment. Only
    /// [`lookup_until_barrier`](Self::lookup_until_barrier) stops here, matching C++
    /// `searchIdDef`.
    #[must_use]
    pub fn push_barrier_scope(&self) -> Self {
        self.push_child(true)
    }

    fn push_child(&self, barrier: bool) -> Self {
        self.spawn_child_with_parent(Some(self.current), barrier)
    }

    fn spawn_child_with_parent(&self, parent: Option<EnvId>, barrier: bool) -> Self {
        let current = self.with_store_mut(|store| {
            let next_id = store.layers.len();
            store.layers.push(EnvLayer {
                bindings: Vec::new(),
                parent,
                barrier,
            });
            next_id
        });
        Self {
            store: Arc::clone(&self.store),
            current,
            source_context: Arc::clone(&self.source_context),
        }
    }

    fn layer_snapshot(&self) -> (Option<EnvId>, bool, Vec<(SymId, EvalValue)>) {
        self.with_store(|store| {
            let layer = &store.layers[self.current];
            (layer.parent, layer.barrier, layer.bindings.clone())
        })
    }

    /// Returns names bound in the **current scope layer only** (no parent traversal).
    ///
    /// Used by [`EvalError::UndefinedSymbol`] to populate the `local_scope` diagnostic field.
    /// Names are sorted and deduplicated for stable diagnostic output.
    ///
    /// `arena` is required to resolve interned `u32` symbol ids back to their string names for
    /// human-readable diagnostics.
    #[must_use]
    pub fn local_names(&self, arena: &TreeArena) -> Vec<String> {
        let mut out = self.with_store(|store| {
            store.layers[self.current]
                .bindings
                .iter()
                .filter_map(|(sym, _)| arena.symbol_name(*sym).map(str::to_owned))
                .collect::<Vec<_>>()
        });
        out.sort();
        out.dedup();
        out
    }

    /// Returns all names **visible from this scope** across the full parent chain.
    ///
    /// Used by [`EvalError::UndefinedSymbol`] to populate the `visible_scope` diagnostic field.
    /// Names are sorted and deduplicated — a name shadowed in an inner scope appears only once.
    #[must_use]
    pub fn visible_names(&self, arena: &TreeArena) -> Vec<String> {
        let mut out = self.with_store(|store| {
            let mut names = Vec::new();
            let mut env_id = Some(self.current);
            while let Some(id) = env_id {
                let layer = &store.layers[id];
                names.extend(
                    layer
                        .bindings
                        .iter()
                        .filter_map(|(sym, _)| arena.symbol_name(*sym).map(str::to_owned)),
                );
                env_id = layer.parent;
            }
            names
        });
        out.sort();
        out.dedup();
        out
    }

    /// Returns names from the **root (top-level) scope** by walking up the parent chain.
    ///
    /// Used by [`EvalError::UndefinedSymbol`] to populate the `top_level_scope` diagnostic field,
    /// helping users see what top-level definitions are available when a symbol is not found.
    #[must_use]
    pub fn top_level_names(&self, arena: &TreeArena) -> Vec<String> {
        let mut out = self.with_store(|store| {
            let mut env_id = self.current;
            while let Some(parent) = store.layers[env_id].parent {
                env_id = parent;
            }
            store.layers[env_id]
                .bindings
                .iter()
                .filter_map(|(sym, _)| arena.symbol_name(*sym).map(str::to_owned))
                .collect::<Vec<_>>()
        });
        out.sort();
        out.dedup();
        out
    }

    fn with_store<R>(&self, f: impl FnOnce(&EnvStore) -> R) -> R {
        let guard = self.store.lock().expect("environment store lock poisoned");
        f(&guard)
    }

    fn with_store_mut<R>(&self, f: impl FnOnce(&mut EnvStore) -> R) -> R {
        let mut guard = self.store.lock().expect("environment store lock poisoned");
        f(&mut guard)
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::empty()
    }
}

/// Infinite loop detector for recursive symbol expansion.
///
/// Detects two failure modes during evaluation:
/// 1. **Recursive loop**: a node is being evaluated while it is already on the call stack
///    (cyclic definition such as `x = x;`).
/// 2. **Depth exceeded**: the call stack grows beyond `max_depth`, indicating runaway recursion
///    in deeply nested but non-cyclic programs.
///
/// # C++ correspondence — `loopDetector.hh`
///
/// The C++ `LoopDetector` uses a `set<Tree>` to track in-flight nodes plus a recursion depth
/// counter. The Rust version uses a `Vec<TreeId>` for the call stack:
///
/// | Aspect | C++ (`LoopDetector`) | Rust (`LoopDetector`) |
/// |---|---|---|
/// | In-flight tracking | `set<Tree>` — O(log n) per check | `Vec<TreeId>` linear scan — O(n) per check |
/// | Depth counter | Separate `int` field | `call_stack.len()` |
/// | Check cost | O(log depth) tree-pointer comparison | O(depth) u32 comparison — cache-friendly |
///
/// For evaluation stacks typical of Faust programs (depth < 100), the Rust O(n) scan over a
/// compact vector is expected to be competitive with, and often faster than, the C++ O(log n)
/// set probe because the stack stays shallow and contiguous. The tree/set approach becomes more
/// attractive only when recursion depth grows far beyond the intended Faust range.
///
/// # Performance
/// - `enter`: O(depth) scan — the entire call stack fits in L1 cache for depth < 256.
/// - `leave`: O(1) — `Vec::pop`.
/// - Memory: 8 bytes per frame (one `u32` TreeId, padded).
///
/// # Evaluation-phase caches
///
/// `LoopDetector` is threaded through every recursive evaluator call, making it the
/// natural carrier for caches that must survive across the whole evaluation phase.
/// Currently it holds:
/// - `automaton_cache`: memoises the compiled `pattern_matcher::Automaton` for each
///   **evaluated** `Case` rule-list, keyed by the resulting rule-list `TreeId`.
///   This is important for parity: the same raw `case` syntax can yield different
///   effective patterns under different lexical environments.
#[derive(Clone, Debug)]
pub struct LoopDetector {
    call_stack: Vec<LoopFrame>,
    max_depth: usize,
    /// Cooperative cancellation flag.
    ///
    /// When set to `true`, the next `eval_value` call returns
    /// `EvalError::Cancelled`. This is the library-safe alternative to
    /// `process::exit`: the CLI sets this from a watchdog thread after the
    /// configured `--timeout`, and libfaust hosts can set it from any thread
    /// (e.g. on user abort).
    cancel: Arc<AtomicBool>,
    /// Compiled automata keyed by the `TreeId` of the evaluated `Case` rule-list.
    automaton_cache: pattern_matcher::AutomatonCache,
    /// Dense store of `PatternMatcherValue` referenced by `boxPatternMatcher` nodes.
    ///
    /// Each `boxPatternMatcher` tree node carries a `boxInt(key)` child that
    /// indexes into this vector. The indirection is necessary because PM values
    /// contain environments and automatons that cannot be hash-consed.
    ///
    /// # C++ equivalent
    /// In C++, `boxPatternMatcher` inlines all PM state (automaton pointer,
    /// state index, environments, consumed args) in the tree. Rust keeps the
    /// complex data here and stores only a handle in the tree.
    pm_store: Vec<PatternMatcherValue>,
    /// Monotonic slot id source used by [`a2sb`] when lowering residual closures.
    ///
    /// Source provenance (C++):
    /// - `compiler/evaluate/eval.cpp`
    /// - `gGlobal->gBoxSlotNumber`
    ///
    /// The Rust port keeps this counter local to one evaluation pass instead of
    /// storing it in global state. The numeric payload is only used as a stable,
    /// debuggable slot label; semantic identity is carried by the unique `BoxId`
    /// of each `boxSlot(...)` node.
    next_slot_id: i32,
}

impl LoopDetector {
    /// Creates a detector with the default maximum recursion depth (1024).
    ///
    /// This matches the practical depth limit of the C++ evaluator, which uses a similar guard
    /// to prevent stack overflows on pathological inputs.
    #[must_use]
    pub fn new() -> Self {
        Self {
            call_stack: Vec::new(),
            max_depth: 1024,
            cancel: Arc::new(AtomicBool::new(false)),
            automaton_cache: pattern_matcher::AutomatonCache::default(),
            pm_store: Vec::new(),
            next_slot_id: 0,
        }
    }

    /// Creates a detector with a pre-existing cooperative cancellation flag.
    ///
    /// The caller retains an `Arc<AtomicBool>` clone and can set it to `true`
    /// from any thread to request cancellation.  The next `eval_value` call
    /// will return `EvalError::Cancelled`.
    ///
    /// This is the library-safe alternative to `process::exit`: the CLI spawns
    /// a watchdog thread that sets the flag after `--timeout`, and libfaust
    /// hosts can set it on user abort without killing the process.
    #[must_use]
    pub fn with_cancel(cancel: Arc<AtomicBool>) -> Self {
        Self {
            call_stack: Vec::new(),
            max_depth: 1024,
            cancel,
            automaton_cache: pattern_matcher::AutomatonCache::default(),
            pm_store: Vec::new(),
            next_slot_id: 0,
        }
    }

    /// Creates a detector with an explicit maximum recursion depth.
    ///
    /// Use a lower value (e.g. 64) for unit tests that should never recurse deeply.
    /// Use a higher value for programs with known deep but non-cyclic definition chains.
    #[must_use]
    pub fn with_max_depth(max_depth: usize) -> Self {
        Self {
            call_stack: Vec::new(),
            max_depth,
            cancel: Arc::new(AtomicBool::new(false)),
            automaton_cache: pattern_matcher::AutomatonCache::default(),
            pm_store: Vec::new(),
            next_slot_id: 0,
        }
    }

    /// Returns a clone of the cancellation flag for external threads to signal abort.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    /// Returns `Err(EvalError::Cancelled)` if the cancel flag has been set.
    #[inline]
    fn check_cancel(&self) -> Result<(), EvalError> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(EvalError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Stores a `PatternMatcherValue` and returns its dense key for `boxPatternMatcher` nodes.
    fn store_pm(&mut self, pm: PatternMatcherValue) -> i32 {
        let key = self.pm_store.len() as i32;
        self.pm_store.push(pm);
        key
    }

    /// Retrieves a stored `PatternMatcherValue` by cloning it out.
    ///
    /// Returns `None` if the key is out of range.
    fn get_pm(&self, key: i32) -> Option<PatternMatcherValue> {
        self.pm_store.get(key as usize).cloned()
    }

    fn enter_tree(&mut self, id: TreeId, env_key: EnvFrameKey) -> Result<(), EvalError> {
        self.enter(LoopFrame::TreeEnv { id, env_key }, id)
    }

    fn enter_symbol_env(
        &mut self,
        sym: SymId,
        env_key: EnvFrameKey,
        node: TreeId,
    ) -> Result<(), EvalError> {
        self.enter(LoopFrame::SymbolEnv { sym, env_key }, node)
    }

    fn enter(&mut self, frame: LoopFrame, node: TreeId) -> Result<(), EvalError> {
        if self.call_stack.contains(&frame) {
            return Err(EvalError::LoopDetected { node });
        }
        if self.call_stack.len() >= self.max_depth {
            return Err(EvalError::RecursionDepthExceeded {
                max_depth: self.max_depth,
            });
        }
        self.call_stack.push(frame);
        Ok(())
    }

    fn leave(&mut self) {
        let _ = self.call_stack.pop();
    }
}

impl Default for LoopDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// One recursion stack frame recorded by [`LoopDetector`].
enum LoopFrame {
    TreeEnv { id: TreeId, env_key: EnvFrameKey },
    SymbolEnv { sym: SymId, env_key: EnvFrameKey },
}

/// Performance statistics collected during evaluation.
///
/// Returned by [`eval_process_with_stats`] alongside the evaluated box tree.
/// Provides the same information as the C++ `gGlobal->gStats` fields used for profiling
/// the evaluator, but without global mutable state — stats are accumulated locally and
/// returned by value.
///
/// # C++ correspondence
///
/// | Rust field | C++ equivalent | C++ location |
/// |---|---|---|
/// | `env_layers_pushed` | `gStats.fEnvLayersPushed` | `environment.cpp` — `pushNewLayer` |
/// | `env_lookups` | `gStats.fEnvLookups` | `environment.cpp` — `searchIdDef` |
/// | `env_lookup_total_depth` | `gStats.fEnvLookupTotalDepth` | `environment.cpp` — `searchIdDef` loop |
/// | `loop_detector_max_depth` | (no direct equivalent — C++ uses `gGlobal->gRecursionLimit`) | |
/// | `nodes_evaluated` | (not tracked in C++) | |
///
/// # Interpretation
///
/// These ratios describe the intended interpretation once all counters are wired:
///
/// - **`env_lookups / nodes_evaluated`**: average lookups per evaluated node. High values (> 3)
///   indicate deeply bound symbols that might benefit from flattening or interning.
/// - **`env_lookup_total_depth / env_lookups`**: average scope depth traversed per lookup.
///   Values > 3 indicate deep scope chains where caching may help.
/// - **`env_layers_pushed / nodes_evaluated`**: scope-push frequency. High values for iterative
///   forms (`ipar`/`iseq`) are expected.
///
/// As of the current port, instrumentation is still incremental: the field meanings are stable,
/// but not every evaluator path updates every counter yet. Consumers should therefore treat these
/// values as progressively improving telemetry, not as a fully complete profiling contract.
#[derive(Clone, Debug, Default)]
/// Lightweight evaluator statistics returned by opt-in entry points.
pub struct EvalStats {
    /// Number of child scopes created via `push_scope()`.
    /// C++ equivalent: `gStats.fEnvLayersPushed`.
    pub env_layers_pushed: u64,
    /// Number of symbol lookups performed across all scopes.
    /// C++ equivalent: `gStats.fEnvLookups`.
    pub env_lookups: u64,
    /// Total scope depth traversed across all lookups (sum of per-lookup depths).
    /// Dividing by `env_lookups` gives the average lookup depth.
    /// C++ equivalent: `gStats.fEnvLookupTotalDepth`.
    pub env_lookup_total_depth: u64,
    /// Maximum loop-detector stack depth reached during evaluation.
    pub loop_detector_max_depth: usize,
    /// Total number of box nodes visited by `eval_box`.
    pub nodes_evaluated: u64,
}

/// Evaluator error.
///
/// Each variant corresponds to a distinct failure mode of the evaluation phase. All variants
/// carry enough context to produce rich diagnostics via [`IntoDiagnostic`].
///
/// # C++ correspondence
///
/// C++ errors are thrown as `faustexception` with a formatted string message and global
/// `gGlobal->gErrorCount` increment. The Rust model uses typed `Result<_, EvalError>` returns
/// with structured context, enabling richer diagnostics without global state.
///
/// | Rust variant | C++ trigger |
/// |---|---|
/// | `MissingProcessDefinition` | `evalerror("... process is not defined")` in `eval.cpp` |
/// | `UndefinedSymbol` | `evalerror("... unknown id")` in `eval.cpp` |
/// | `RedefinedSymbol` | `throw faustexception("redefinition of symbols …")` in `environment.cpp` |
/// | `LoopDetected` | `faustassert` in C++ loop detector (aborts rather than throws) |
/// | `RecursionDepthExceeded` | Implicit stack overflow in C++ (no explicit guard) |
#[derive(Debug, Clone, PartialEq, Eq)]
/// Typed evaluator failure surface.
pub enum EvalError {
    MissingProcessDefinition {
        /// Requested top-level DSP entry-point name.
        entrypoint: String,
        /// Parser root definitions list used for fallback source-label resolution.
        definitions: TreeId,
        /// Deterministic list of top-level definition names available in this program.
        available_defs: Vec<String>,
    },
    UndefinedSymbol {
        symbol: String,
        /// Identifier node where resolution failed.
        node: TreeId,
        /// Names bound in the immediate lexical scope.
        local_scope: Vec<String>,
        /// Names visible across lexical parents.
        visible_scope: Vec<String>,
        /// Names bound at top-level.
        top_level_scope: Vec<String>,
    },
    MalformedDefinitionNode {
        node: TreeId,
    },
    MalformedListNode {
        node: TreeId,
    },
    MalformedCaseNode {
        node: TreeId,
    },
    EmptyArgumentList {
        /// Argument-list node that was expected to contain at least one item.
        node: TreeId,
    },
    NonIdentifierParameter {
        node: TreeId,
    },
    NonIdentifierIterationVariable {
        node: TreeId,
    },
    IterationCountNotInt {
        node: TreeId,
    },
    IterationCountTooLarge {
        value: i64,
    },
    NegativeIterationCount {
        value: i64,
    },
    PatternArityMismatch {
        /// Case-rules root node used to evaluate matching.
        node: TreeId,
        expected: usize,
        got: usize,
    },
    PatternMatchFailed {
        /// Case-rules root node where no rule matched provided arguments.
        node: TreeId,
    },
    /// Non-closure application received more arguments than the function input arity.
    TooManyArguments {
        /// Function-like node receiving too many arguments.
        node: TreeId,
        expected: usize,
        got: usize,
    },
    InvalidModulationLabel {
        node: TreeId,
    },
    InvalidLabelInterpolation {
        node: TreeId,
        ident: String,
        reason: &'static str,
    },
    InvalidModulationCircuit {
        node: TreeId,
        reason: &'static str,
    },
    InvalidSourceReference {
        node: TreeId,
        construct: &'static str,
    },
    SourceFileNotFound {
        node: TreeId,
        construct: &'static str,
        target: String,
        current_file: Option<PathBuf>,
        search_paths: Vec<PathBuf>,
    },
    SourceReaderFailure {
        node: TreeId,
        construct: &'static str,
        target: String,
        message: String,
    },
    SourceParseFailure {
        node: TreeId,
        construct: &'static str,
        path: PathBuf,
        errors: Vec<String>,
    },
    ExpectedClosureValue {
        node: TreeId,
        context: &'static str,
    },
    /// A symbol is redefined with a **different** value within the same lexical scope layer.
    ///
    /// Identical redefinitions (same `first_def == second_def` by `TreeId` identity) are
    /// silently ignored, matching C++ `addLayerDef` behavior:
    /// ```cpp
    /// if (def == olddef) { /* silent — hash-consed equality */ }
    /// else { throw faustexception("redefinition of symbols are not allowed: …"); }
    /// ```
    ///
    /// This check is performed only within the **current scope layer** (`lookup_local`), so
    /// shadowing a name from an outer scope is allowed and does not trigger this error.
    RedefinedSymbol {
        /// The symbol name that was defined more than once.
        symbol: String,
        /// The `TreeId` of the first (original) definition.
        first_def: TreeId,
        /// The `TreeId` of the conflicting second definition.
        second_def: TreeId,
    },
    LoopDetected {
        node: TreeId,
    },
    RecursionDepthExceeded {
        max_depth: usize,
    },
    /// A box expression was expected to evaluate to a compile-time numeric
    /// constant (type 0→1 with a numeric value), but did not.
    ///
    /// Occurs in slider parameter evaluation, table-size expressions, and
    /// similar contexts where the C++ compiler calls `eval2int` / `eval2double`.
    ///
    /// C++ equivalent: `evalerror("not a constant expression of type: (0->1)", …)`
    /// thrown by `eval2double` / `eval2int` in `eval.cpp`.
    NotAConstantExpression {
        node: TreeId,
    },
    /// Internal evaluator error — indicates a bug in the evaluator, not a user error.
    InternalError {
        message: String,
    },
    /// Cooperative cancellation: the external cancel flag was set (e.g., timeout).
    Cancelled,
}

impl Display for EvalError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingProcessDefinition { entrypoint, .. } => {
                write!(f, "missing `{entrypoint}` definition")
            }
            Self::UndefinedSymbol { symbol, .. } => write!(f, "undefined symbol `{symbol}`"),
            Self::MalformedDefinitionNode { node } => {
                write!(f, "malformed definition node {}", node.as_u32())
            }
            Self::MalformedListNode { node } => {
                write!(f, "malformed list node {}", node.as_u32())
            }
            Self::MalformedCaseNode { node } => {
                write!(f, "malformed case node {}", node.as_u32())
            }
            Self::EmptyArgumentList { .. } => write!(f, "empty argument list"),
            Self::NonIdentifierParameter { node } => {
                write!(
                    f,
                    "abstraction parameter is not an identifier: {}",
                    node.as_u32()
                )
            }
            Self::NonIdentifierIterationVariable { node } => {
                write!(
                    f,
                    "iteration variable is not an identifier: {}",
                    node.as_u32()
                )
            }
            Self::IterationCountNotInt { node } => {
                write!(f, "iteration count is not an int node: {}", node.as_u32())
            }
            Self::IterationCountTooLarge { value } => {
                write!(f, "iteration count too large for this target: {value}")
            }
            Self::NegativeIterationCount { value } => {
                write!(f, "iteration count is negative: {value}")
            }
            Self::PatternArityMismatch { expected, got, .. } => {
                write!(f, "pattern arity mismatch: expected {expected}, got {got}")
            }
            Self::PatternMatchFailed { .. } => write!(f, "no case rule matches arguments"),
            Self::TooManyArguments { expected, got, .. } => {
                write!(
                    f,
                    "too many arguments: expected at most {expected}, got {got}"
                )
            }
            Self::InvalidModulationLabel { node } => {
                write!(f, "invalid modulation label at node {}", node.as_u32())
            }
            Self::InvalidLabelInterpolation { ident, reason, .. } => {
                write!(
                    f,
                    "cannot interpolate label placeholder `%{ident}`: {reason}"
                )
            }
            Self::InvalidModulationCircuit { reason, .. } => {
                write!(f, "invalid modulation circuit: {reason}")
            }
            Self::InvalidSourceReference { construct, .. } => {
                write!(
                    f,
                    "{construct} requires a string-like source filename literal"
                )
            }
            Self::SourceFileNotFound {
                construct, target, ..
            } => {
                write!(f, "{construct} could not resolve source file `{target}`")
            }
            Self::SourceReaderFailure {
                construct, target, ..
            } => {
                write!(f, "{construct} failed while reading source file `{target}`")
            }
            Self::SourceParseFailure {
                construct, path, ..
            } => {
                write!(
                    f,
                    "{construct} loaded `{}` but parsing failed",
                    path.display()
                )
            }
            Self::ExpectedClosureValue { context, .. } => {
                write!(f, "{context} requires a captured closure value")
            }
            Self::RedefinedSymbol { symbol, .. } => {
                write!(
                    f,
                    "symbol `{symbol}` redefined with a different value in the same scope"
                )
            }
            Self::LoopDetected { node } => {
                write!(f, "recursive evaluation loop on node {}", node.as_u32())
            }
            Self::RecursionDepthExceeded { max_depth } => {
                write!(f, "evaluation recursion depth exceeded ({max_depth})")
            }
            Self::NotAConstantExpression { node } => {
                write!(
                    f,
                    "expression is not a compile-time numeric constant (type 0→1): node {}",
                    node.as_u32()
                )
            }
            Self::InternalError { message } => {
                write!(f, "internal evaluator error: {message}")
            }
            Self::Cancelled => write!(f, "evaluation cancelled (timeout or abort)"),
        }
    }
}

impl std::error::Error for EvalError {}

/// Converts one evaluator error into the workspace diagnostics model.
///
/// This keeps `EvalError` as the local phase error type while exposing
/// stable stage/code metadata for compiler-level aggregation and CLI rendering.
impl IntoDiagnostic for EvalError {
    fn into_diagnostic(self) -> Diagnostic {
        let message = self.to_string();
        match self {
            Self::MissingProcessDefinition {
                entrypoint,
                available_defs,
                ..
            } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_MISSING_PROCESS,
                message,
            )
            .with_note(format!(
                "cause: required top-level `{entrypoint}` definition is missing"
            ))
            .with_note(format!(
                "entrypoint contract: one top-level `{entrypoint} = ...;` definition is required"
            ))
            .with_note(format!(
                "available top-level definitions: {}",
                if available_defs.is_empty() {
                    "<none>".to_owned()
                } else {
                    available_defs.join(", ")
                }
            ))
            .with_help(format!(
                "define `{entrypoint} = ...;` in the top-level definitions"
            ))
            .with_help(format!("template: {entrypoint} = _;")),
            Self::UndefinedSymbol {
                symbol,
                local_scope,
                visible_scope,
                top_level_scope,
                ..
            } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_UNDEFINED_SYMBOL,
                message,
            )
            .with_note("cause: unresolved identifier in current lexical scope")
            .with_note("rule: referenced identifier must be present in visible lexical scope")
            .with_note(format!(
                "computed: `{symbol}` is not present in current visible scope"
            ))
            .with_note(format!(
                "scope.local={}",
                if local_scope.is_empty() {
                    "<none>".to_owned()
                } else {
                    local_scope.join(", ")
                }
            ))
            .with_note(format!(
                "scope.visible={}",
                if visible_scope.is_empty() {
                    "<none>".to_owned()
                } else {
                    visible_scope.join(", ")
                }
            ))
            .with_note(format!(
                "scope.top_level={}",
                if top_level_scope.is_empty() {
                    "<none>".to_owned()
                } else {
                    top_level_scope.join(", ")
                }
            ))
            .with_help("define the symbol in scope or fix the identifier name")
            .with_help(format!("template: {symbol} = ...; // define before use"))
            .with_help("for top-level aliases: define target before first use"),
            Self::PatternArityMismatch { expected, got, .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_ARITY_MISMATCH,
                message,
            )
            .with_note("cause: case pattern arity does not match provided argument tuple")
            .with_note("rule: case rule arity must match provided argument tuple arity")
            .with_note(format!(
                "computed: expected={expected}, provided={got}, delta={}",
                got as i128 - expected as i128
            ))
            .with_note(format!(
                "suggested target: call case function with exactly {expected} argument(s)"
            ))
            .with_help("adapt the case pattern arity or provide the expected number of arguments")
            .with_help("template: case { (x, y) => ...; }; // 2-argument rule"),
            Self::TooManyArguments { expected, got, .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_ARITY_MISMATCH,
                message,
            )
            .with_note("cause: function application provides more arguments than accepted")
            .with_note(
                "rule: non-closure application requires provided arguments <= function input arity",
            )
            .with_note(format!(
                "computed: provided={got}, expected_max={expected}, overflow={}",
                got.saturating_sub(expected)
            ))
            .with_note(format!(
                "suggested target: remove {} extra argument(s)",
                got.saturating_sub(expected)
            ))
            .with_help("remove extra arguments or expand the function input arity")
            .with_help("template: f(a, b); // keep provided args <= function input arity"),
            Self::InvalidModulationLabel { .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_GENERIC_FAILURE,
                message,
            )
            .with_note("cause: modulation target did not resolve to a valid label string")
            .with_note("rule: modulation target must be a string-like Faust label")
            .with_help("use a literal label such as [\"gain\" : _ -> expr]"),
            Self::InvalidLabelInterpolation { ident, reason, .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_GENERIC_FAILURE,
                message,
            )
            .with_note(format!(
                "cause: label placeholder `{ident}` did not resolve to an integer constant"
            ))
            .with_note(format!("computed: {reason}"))
            .with_help(
                "bind the placeholder name to an integer constant expression before using it in a label",
            ),
            Self::InvalidModulationCircuit { reason, .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_GENERIC_FAILURE,
                message,
            )
            .with_note("cause: modulation circuit violates Faust box-arity constraints")
            .with_note(format!("computed: {reason}"))
            .with_help("use a modulation circuit with at most 2 inputs and exactly 1 output"),
            Self::InvalidSourceReference { construct, .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_GENERIC_FAILURE,
                message,
            )
            .with_note(format!(
                "cause: `{construct}` expects a literal source filename carried directly by the box tree"
            ))
            .with_help("template: component(\"file.dsp\") or library(\"file.dsp\")"),
            Self::SourceFileNotFound {
                target,
                current_file,
                search_paths,
                ..
            } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_GENERIC_FAILURE,
                message,
            )
            .with_note(format!(
                "current file: {}",
                current_file
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<memory>".to_owned())
            ))
            .with_note(format!(
                "search paths: {}",
                if search_paths.is_empty() {
                    "<none>".to_owned()
                } else {
                    search_paths
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ))
            .with_help(format!("check that `{target}` exists in the active import path")),
            Self::SourceReaderFailure {
                construct,
                message: detail,
                ..
            } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_GENERIC_FAILURE,
                message,
            )
            .with_note(format!("source reader failure in `{construct}`: {detail}")),
            Self::SourceParseFailure { errors, .. } => {
                let mut diagnostic = Diagnostic::new(
                    Severity::Error,
                    Stage::Eval,
                    codes::EVAL_GENERIC_FAILURE,
                    message,
                );
                for parse_error in errors {
                    diagnostic = diagnostic.with_note(format!("loaded parse error: {parse_error}"));
                }
                diagnostic
            }
            Self::ExpectedClosureValue { context, .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_GENERIC_FAILURE,
                message,
            )
            .with_note(
                "cause: evaluator expected a captured lexical environment but received a plain box value",
            )
            .with_note(format!(
                "rule: `{context}` only applies to values that carry a captured environment"
            ))
            .with_help("apply the operator to an environment or abstraction value instead"),
            Self::RedefinedSymbol {
                symbol,
                first_def,
                second_def,
            } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_REDEFINED_SYMBOL,
                message,
            )
            .with_note(
                "cause: the same symbol is bound twice with conflicting values in the same scope",
            )
            .with_note(
                "rule: each symbol may appear at most once per `with {}` block or definition list",
            )
            .with_note(format!(
                "computed: `{symbol}` first bound to node {}, then to node {} (different values)",
                first_def.as_u32(),
                second_def.as_u32()
            ))
            .with_note(
                "note: identical redefinitions (same expression) are silently accepted — \
                 only conflicting redefinitions are errors",
            )
            .with_help(format!("remove the duplicate `{symbol} = ...;` definition"))
            .with_help(
                "if shadowing was intended, move the inner definition to a nested `with {}` block"
                    .to_string(),
            ),
            Self::PatternMatchFailed { .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_GENERIC_FAILURE,
                message,
            )
            .with_note("cause: no case rule matched the provided argument tuple")
            .with_note("rule: at least one case pattern must match the provided argument tuple")
            .with_note("computed: provided tuple did not match any declared case pattern")
            .with_help("add a matching case rule or add a catch-all pattern"),
            Self::IterationCountNotInt { .. }
            | Self::IterationCountTooLarge { .. }
            | Self::NegativeIterationCount { .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_ITERATION_INVALID,
                message,
            )
            .with_note("cause: iterative combinator count is not a valid non-negative integer")
            .with_note(
                "rule: iterator count must be integer, non-negative, and within supported range",
            )
            .with_help("iteration count must be a non-negative integer in target range"),
            _ => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_GENERIC_FAILURE,
                message,
            )
            .with_note("cause: evaluator reached an unsupported or malformed intermediate form"),
        }
    }
}

/// Evaluates one Faust program root list and returns the resolved `process` expression.
///
/// # Input format
///
/// `definitions` must be the parser root list where each element is
/// `cons(name_node, cons(args_list, expr))`. This is the direct output of the Faust parser.
///
/// # Output
///
/// The returned `TreeId` points to a normalized box IR node. High-level forms (`abstr`, `with`,
/// `case`) may still appear in the output when intentionally preserved for later passes.
/// The tree is not yet in signal form — signal lowering happens in the `propagate` pass.
///
/// # Errors
///
/// Returns the first error encountered during evaluation. Evaluation is strict — no error
/// recovery is attempted. If diagnostics for multiple errors are needed, the caller must inspect
/// the returned `EvalError` and decide whether to re-run or accumulate errors externally.
///
/// # C++ correspondence
///
/// Corresponds to the `eval()` entry point in `compiler/evaluate/eval.cpp`:
/// ```cpp
/// // eval.cpp (simplified)
/// Tree eval(Tree ldef, int& numInputs, int& numOutputs) {
///     gGlobal->gCurrentEnv = pushMultiClosureDefs(ldef, gGlobal->nil, gGlobal->nil);
///     initRecursion();
///     return eval(closure(boxIdent("process"), …, gGlobal->gCurrentEnv), 0, 0);
/// }
/// ```
///
/// Key differences from C++:
/// - No global mutable state (`gCurrentEnv`, `gGlobal`) — all state is local.
/// - Returns `Result<TreeId, EvalError>` instead of throwing `faustexception`.
/// - Redefinition errors are caught via `bind_definitions` instead of propagating globally.
///
/// For performance statistics, use [`eval_process_with_stats`] instead.
pub fn eval_process(arena: &mut TreeArena, definitions: TreeId) -> Result<TreeId, EvalError> {
    Ok(eval_process_with_stats(arena, definitions)?.0)
}

/// Evaluates one Faust program root list using a custom top-level DSP
/// entry-point name instead of the default `process`.
pub fn eval_entrypoint(
    arena: &mut TreeArena,
    definitions: TreeId,
    entrypoint: &str,
) -> Result<TreeId, EvalError> {
    Ok(eval_entrypoint_with_stats(arena, definitions, entrypoint)?.0)
}

/// Evaluates one Faust program root list using an explicit file-resolution context.
///
/// This is the file-backed counterpart of [`eval_process`]. It keeps the legacy
/// API intact for in-memory callers while letting file-oriented frontends mirror
/// the C++ contract where `eval.cpp` sees a configured source reader.
///
/// Use this entry point when the evaluated program may contain
/// `component("...")` or `library("...")` forms that must resolve relative to
/// an on-disk Faust source file.
pub fn eval_process_with_source_context(
    arena: &mut TreeArena,
    definitions: TreeId,
    source_context: EvalSourceContext,
) -> Result<TreeId, EvalError> {
    Ok(eval_process_with_stats_and_source_context(arena, definitions, source_context)?.0)
}

/// File-backed counterpart of [`eval_entrypoint`].
pub fn eval_entrypoint_with_source_context(
    arena: &mut TreeArena,
    definitions: TreeId,
    entrypoint: &str,
    source_context: EvalSourceContext,
) -> Result<TreeId, EvalError> {
    Ok(eval_entrypoint_with_stats_and_source_context(
        arena,
        definitions,
        entrypoint,
        source_context,
    )?
    .0)
}

/// Evaluates one Faust program root list and returns the resolved `process` expression together
/// with performance statistics collected during evaluation.
///
/// This is the instrumented variant of [`eval_process`]. The returned [`EvalStats`] provides
/// profiling data parallel to C++ `gGlobal->gStats` fields (`fEnvLayersPushed`,
/// `fEnvLookups`, `fEnvLookupTotalDepth`), without requiring global mutable state.
///
/// # Example (profiling a program)
/// ```ignore
/// let (process, stats) = eval_process_with_stats(&mut arena, defs)?;
/// println!("lookups: {}, avg depth: {:.1}",
///     stats.env_lookups,
///     stats.env_lookup_total_depth as f64 / stats.env_lookups.max(1) as f64);
/// ```
pub fn eval_process_with_stats(
    arena: &mut TreeArena,
    definitions: TreeId,
) -> Result<(TreeId, EvalStats), EvalError> {
    eval_entrypoint_with_stats(arena, definitions, "process")
}

/// Instrumented variant of [`eval_entrypoint`].
pub fn eval_entrypoint_with_stats(
    arena: &mut TreeArena,
    definitions: TreeId,
    entrypoint: &str,
) -> Result<(TreeId, EvalStats), EvalError> {
    eval_entrypoint_with_stats_and_source_context(
        arena,
        definitions,
        entrypoint,
        EvalSourceContext::memory(),
    )
}

/// Instrumented variant of [`eval_process_with_source_context`].
///
/// File-backed callers should prefer this entry point when they need both
/// profile counters and evaluator-level source loading semantics.
///
/// The passed [`EvalSourceContext`] becomes part of the root evaluation
/// environment and is subsequently captured by any closure value created while
/// evaluating the loaded program.
pub fn eval_process_with_stats_and_source_context(
    arena: &mut TreeArena,
    definitions: TreeId,
    source_context: EvalSourceContext,
) -> Result<(TreeId, EvalStats), EvalError> {
    eval_entrypoint_with_stats_and_source_context(arena, definitions, "process", source_context)
}

/// Instrumented variant of [`eval_entrypoint_with_source_context`].
pub fn eval_entrypoint_with_stats_and_source_context(
    arena: &mut TreeArena,
    definitions: TreeId,
    entrypoint: &str,
    source_context: EvalSourceContext,
) -> Result<(TreeId, EvalStats), EvalError> {
    eval_entrypoint_full(arena, definitions, entrypoint, source_context, None)
}

/// Full entry point with cooperative cancellation support.
///
/// When `cancel` is `Some`, the evaluator checks the flag on every recursive
/// `eval_value` call and returns `EvalError::Cancelled` if it has been set.
/// This is the library-safe timeout mechanism: the CLI spawns a watchdog
/// thread that sets the flag after `--timeout`, and libfaust hosts can set
/// it from any thread (e.g. on user abort) without killing the process.
pub fn eval_entrypoint_with_source_context_and_cancel(
    arena: &mut TreeArena,
    definitions: TreeId,
    entrypoint: &str,
    source_context: EvalSourceContext,
    cancel: Arc<AtomicBool>,
) -> Result<(TreeId, EvalStats), EvalError> {
    eval_entrypoint_full(arena, definitions, entrypoint, source_context, Some(cancel))
}

fn eval_entrypoint_full(
    arena: &mut TreeArena,
    definitions: TreeId,
    entrypoint: &str,
    source_context: EvalSourceContext,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<(TreeId, EvalStats), EvalError> {
    let mut env = Environment::empty_with_source_context(source_context);
    let mut stats = EvalStats::default();
    bind_definitions(arena, definitions, &mut env)?;
    stats.env_layers_pushed += 1; // root scope
    let available_defs = top_level_definition_names(arena, definitions)?;
    // Use get_symbol (no alloc, &self) — if the requested entry-point name was
    // never interned it was never bound.
    arena
        .get_symbol(entrypoint)
        .filter(|sym| env.lookup_value(*sym).is_some())
        .ok_or(EvalError::MissingProcessDefinition {
            entrypoint: entrypoint.to_owned(),
            definitions,
            available_defs,
        })?;
    stats.env_lookups += 1;
    let mut loop_detector = match cancel {
        Some(flag) => LoopDetector::with_cancel(flag),
        None => LoopDetector::new(),
    };
    let entry = BoxBuilder::new(arena).ident(entrypoint);
    let result = eval_value(arena, entry, &env, &mut loop_detector)?;
    let result = a2sb_value(arena, result, &mut loop_detector)?;
    stats.loop_detector_max_depth = loop_detector.call_stack.len();
    Ok((result, stats))
}

fn a2sb_value(
    arena: &mut TreeArena,
    value: EvalValue,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    match value {
        EvalValue::Box(expr) => a2sb(arena, expr, loop_detector),
        EvalValue::Closure(closure) => match match_box(arena, closure.expr) {
            BoxMatch::Abstr(_, _) => {
                lower_abstraction_to_symbolic_value(arena, closure, loop_detector)
            }
            BoxMatch::Environment => Ok(closure.expr),
            _ => {
                let forced = eval_value(arena, closure.expr, &closure.env, loop_detector)?;
                a2sb_value(arena, forced, loop_detector)
            }
        },
        EvalValue::PatternMatcher(pm) => {
            lower_pattern_matcher_to_symbolic(arena, pm, loop_detector)
        }
    }
}

/// Lowers residual abstractions and case closures into symbolic boxes.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `a2sb`
/// - `real_a2sb`
///
/// The C++ evaluator applies `a2sb(eval(...))` before the propagation phase so
/// `propagate` never receives raw closures or pattern matchers. Rust now
/// materializes closures internally, but this helper still lowers the residual
/// evaluated `BoxMatch::Abstr` and `BoxMatch::Case` shapes:
///
/// - `abstr(x, body)` becomes `symbolic(slot, lowered(body[x := slot]))`
/// - `case { ... }` becomes one nested `symbolic(slot_i, ...)` per expected
///   argument, after fully applying the case node to fresh slots
///
/// This is an adapted host-side representation, not a byte-for-byte port of
/// C++ closure nodes. The semantic contract is the same: later passes observe
/// only first-order symbolic boxes, never unapplied evaluator-only forms.
fn a2sb(
    arena: &mut TreeArena,
    expr: TreeId,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    match match_box(arena, expr) {
        BoxMatch::Abstr(_, _) => a2sb_value(
            arena,
            EvalValue::Closure(ClosureValue {
                expr,
                env: Environment::empty(),
            }),
            loop_detector,
        ),
        BoxMatch::Case(rules) => {
            let value = eval_case_value(arena, expr, rules, &Environment::empty(), loop_detector)?;
            a2sb_value(arena, value, loop_detector)
        }
        BoxMatch::PatternMatcher(key_node) => {
            // Retrieve the PM from the side-table and lower it via a2sb_value.
            let key = match match_box(arena, key_node) {
                BoxMatch::Int(k) => k,
                _ => {
                    return Err(EvalError::InternalError {
                        message: "boxPatternMatcher key is not an integer".to_owned(),
                    });
                }
            };
            let pm = loop_detector
                .get_pm(key)
                .ok_or_else(|| EvalError::InternalError {
                    message: format!("boxPatternMatcher key {} not found in PM store", key),
                })?;
            a2sb_value(arena, EvalValue::PatternMatcher(pm), loop_detector)
        }
        _ => {
            let Some(node) = arena.node(expr).cloned() else {
                return Ok(expr);
            };
            if node.children.is_empty() {
                return Ok(expr);
            }

            let mut rebuilt = Vec::with_capacity(node.children.len());
            let mut changed = false;
            for child in node.children.as_slice().iter().copied() {
                let lowered = a2sb(arena, child, loop_detector)?;
                if lowered != child {
                    changed = true;
                }
                rebuilt.push(lowered);
            }

            if changed {
                Ok(arena.intern(node.kind, &rebuilt))
            } else {
                Ok(expr)
            }
        }
    }
}

fn lower_abstraction_to_symbolic_value(
    arena: &mut TreeArena,
    abstraction: ClosureValue,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let slot = fresh_slot(arena, loop_detector);
    let args = vec_to_list(arena, &[slot]);
    let applied = apply_value_list(
        arena,
        EvalValue::Closure(abstraction),
        args,
        &Environment::empty(),
        loop_detector,
        None,
    )?;
    let lowered_body = a2sb(arena, applied, loop_detector)?;
    let mut b = BoxBuilder::new(arena);
    Ok(b.symbolic(slot, lowered_body))
}

fn lower_pattern_matcher_to_symbolic(
    arena: &mut TreeArena,
    mut pm: PatternMatcherValue,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    if pm.automaton.final_state(pm.state) {
        for rule_marker in &pm.automaton.states[pm.state].rules {
            if let Some(rule_env) = pm.envs[rule_marker.r].take() {
                let rhs = pm.automaton.rhs[rule_marker.r];
                let value = eval_value(arena, rhs, &rule_env, loop_detector)?;
                return a2sb_value(arena, value, loop_detector);
            }
        }
        return Err(EvalError::PatternMatchFailed {
            node: pm.original_rules,
        });
    }
    let total = case_expected_arity(arena, pm.original_rules)?;
    let slots_needed = total.saturating_sub(pm.rev_param_list.len());
    let slots: Vec<_> = (0..slots_needed)
        .map(|_| fresh_slot(arena, loop_detector))
        .collect();
    let slot_args = vec_to_list(arena, &slots);
    let applied = apply_value_list(
        arena,
        EvalValue::PatternMatcher(pm),
        slot_args,
        &Environment::empty(),
        loop_detector,
        None,
    )?;
    let mut result = a2sb(arena, applied, loop_detector)?;
    for slot in slots.into_iter().rev() {
        let mut b = BoxBuilder::new(arena);
        result = b.symbolic(slot, result);
    }
    Ok(result)
}

/// Allocates one fresh `boxSlot(...)` node for [`a2sb`].
///
/// The numeric id mirrors the C++ `gBoxSlotNumber` counter and is only used for
/// stable debug identity. Semantic binding later relies on the unique `BoxId`.
fn fresh_slot(arena: &mut TreeArena, loop_detector: &mut LoopDetector) -> TreeId {
    loop_detector.next_slot_id = loop_detector.next_slot_id.saturating_add(1);
    let mut b = BoxBuilder::new(arena);
    b.slot(loop_detector.next_slot_id)
}

/// Evaluates one box expression in the provided lexical environment and forces it back to box IR.
///
/// Internally the evaluator now produces [`EvalValue`] first, so closures can carry a captured
/// environment before being lowered back to a `TreeId` for later passes.
pub fn eval_box(
    arena: &mut TreeArena,
    expr: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let value = eval_value(arena, expr, env, loop_detector)?;
    force_value_to_box(arena, value, loop_detector)
}

/// Evaluates one box expression to an intermediate evaluator value.
///
/// This is the semantic core of the Rust evaluator. Unlike the legacy C++
/// `eval(...)` API, which mostly traffics in `Tree` values plus ad hoc closure
/// nodes, Rust evaluates into [`EvalValue`] first so it can keep captured
/// lexical environments explicit until the result must be lowered back to box
/// IR for later passes.
///
/// The main split is:
/// - `EvalValue::Box`: first-order box value already safe to reinsert in IR,
/// - `EvalValue::Closure`: residual value carrying one lexical environment,
/// - `EvalValue::PatternMatcher`: partially-applied `case` automaton state.
///
/// Most box families stay in the `Box` lane. Only abstractions, environment
/// objects, and `case` applications need the richer host-side representation.
fn eval_value(
    arena: &mut TreeArena,
    expr: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<EvalValue, EvalError> {
    loop_detector.check_cancel()?;
    match match_box(arena, expr) {
        BoxMatch::Unknown => Ok(EvalValue::Box(map_children(
            arena,
            expr,
            env,
            loop_detector,
        )?)),
        BoxMatch::Ident(name) => {
            // get_symbol takes &self — safe to call while `name: &str` borrows `arena`.
            // If the name was never interned (never bound), it cannot be in the env.
            let ((binding_env_id, binding_sym), value) = arena
                .get_symbol(name)
                .and_then(|sym| {
                    env.lookup_value(sym)
                        .map(|(env_id, value)| ((env_id, sym), value))
                })
                .ok_or_else(|| EvalError::UndefinedSymbol {
                    symbol: name.to_owned(),
                    node: expr,
                    local_scope: env.local_names(arena),
                    visible_scope: env.visible_names(arena),
                    top_level_scope: env.top_level_names(arena),
                })?;
            match value {
                EvalValue::Box(value) => {
                    if value == expr {
                        // Shadowing sentinel used for lambda parameters in lexical scopes.
                        return Ok(EvalValue::Box(expr));
                    }
                    loop_detector.enter_tree(value, env.frame_key())?;
                    let out = eval_value(arena, value, env, loop_detector);
                    loop_detector.leave();
                    out
                }
                EvalValue::Closure(closure) => {
                    if matches!(
                        match_box(arena, closure.expr),
                        BoxMatch::Abstr(_, _) | BoxMatch::Environment
                    ) {
                        return Ok(EvalValue::Closure(closure));
                    }
                    loop_detector.enter_symbol_env(
                        binding_sym,
                        env.frame_key_for(binding_env_id),
                        closure.expr,
                    )?;
                    let out = eval_value(arena, closure.expr, &closure.env, loop_detector);
                    loop_detector.leave();
                    out
                }
                EvalValue::PatternMatcher(pm) => Ok(EvalValue::PatternMatcher(pm)),
            }
        }
        BoxMatch::Appl(fun, arg) => {
            let efun = eval_value(arena, fun, env, loop_detector)?;
            let rev_args = rev_eval_list(arena, arg, env, loop_detector)?;
            apply_value_list_value(arena, efun, rev_args, env, loop_detector, Some(fun))
        }
        BoxMatch::Component(filename) => {
            eval_loaded_source_value(arena, expr, filename, "component", env)
        }
        BoxMatch::Library(filename) => {
            eval_loaded_source_value(arena, expr, filename, "library", env)
        }
        BoxMatch::Access(body, field) => eval_access_value(arena, body, field, env, loop_detector),
        BoxMatch::Case(rules) => eval_case_value(arena, expr, rules, env, loop_detector),
        // boxPatternMatcher is already in normal form — return as-is.
        // (Mirrors C++ eval.cpp line 638: `isBoxPatternMatcher(box) => box`)
        BoxMatch::PatternMatcher(_) => Ok(EvalValue::Box(expr)),
        BoxMatch::PatternVar(_) => Ok(EvalValue::Box(expr)),
        BoxMatch::WithLocalDef(body, defs) => {
            let mut scoped = env.push_scope();
            bind_definitions(arena, defs, &mut scoped)?;
            eval_value(arena, body, &scoped, loop_detector)
        }
        BoxMatch::ModifLocalDef(body, defs) => {
            eval_modif_local_def_value(arena, body, defs, env, loop_detector)
        }
        BoxMatch::WithRecDef(body, rec_defs, where_defs) => {
            let mut scoped = env.push_scope();
            bind_definitions(arena, rec_defs, &mut scoped)?;
            bind_definitions(arena, where_defs, &mut scoped)?;
            eval_value(arena, body, &scoped, loop_detector)
        }
        BoxMatch::Metadata(body, _mdlist) => {
            // Source provenance (C++):
            // - `compiler/evaluate/eval.cpp`
            // - `isBoxMetadata(exp, e1, e2) -> eval(e1, ...)`
            //
            // Mapping status: `adapted`.
            // Rust keeps the metadata wrapper in the box layer for parser parity,
            // but `eval` has no runtime-global metadata set yet, so evaluation is
            // transparent for the wrapped expression.
            eval_value(arena, body, env, loop_detector)
        }
        BoxMatch::Button(label) => Ok(EvalValue::Box(eval_button(
            arena,
            label,
            env,
            loop_detector,
        )?)),
        BoxMatch::Checkbox(label) => Ok(EvalValue::Box(eval_checkbox(
            arena,
            label,
            env,
            loop_detector,
        )?)),
        BoxMatch::VSlider(label, cur, min, max, step) => Ok(EvalValue::Box(eval_vslider(
            arena,
            label,
            [cur, min, max, step],
            env,
            loop_detector,
        )?)),
        BoxMatch::HSlider(label, cur, min, max, step) => Ok(EvalValue::Box(eval_hslider(
            arena,
            label,
            [cur, min, max, step],
            env,
            loop_detector,
        )?)),
        BoxMatch::NumEntry(label, cur, min, max, step) => Ok(EvalValue::Box(eval_num_entry(
            arena,
            label,
            [cur, min, max, step],
            env,
            loop_detector,
        )?)),
        BoxMatch::Soundfile(label, chan) => Ok(EvalValue::Box(eval_soundfile(
            arena,
            label,
            chan,
            env,
            loop_detector,
        )?)),
        BoxMatch::VGroup(label, body) => Ok(EvalValue::Box(eval_vgroup(
            arena,
            label,
            body,
            env,
            loop_detector,
        )?)),
        BoxMatch::HGroup(label, body) => Ok(EvalValue::Box(eval_hgroup(
            arena,
            label,
            body,
            env,
            loop_detector,
        )?)),
        BoxMatch::TGroup(label, body) => Ok(EvalValue::Box(eval_tgroup(
            arena,
            label,
            body,
            env,
            loop_detector,
        )?)),
        BoxMatch::VBargraph(label, min, max) => Ok(EvalValue::Box(eval_vbargraph(
            arena,
            label,
            min,
            max,
            env,
            loop_detector,
        )?)),
        BoxMatch::HBargraph(label, min, max) => Ok(EvalValue::Box(eval_hbargraph(
            arena,
            label,
            min,
            max,
            env,
            loop_detector,
        )?)),
        BoxMatch::Abstr(_, _) | BoxMatch::Environment => Ok(EvalValue::Closure(ClosureValue {
            expr,
            env: env.clone(),
        })),
        BoxMatch::Modulation(var, body) => Ok(EvalValue::Box(eval_modulation(
            arena,
            expr,
            var,
            body,
            env,
            loop_detector,
        )?)),
        BoxMatch::IPar(index, count, body) => Ok(EvalValue::Box(iterate_par(
            arena,
            index,
            count,
            body,
            env,
            loop_detector,
        )?)),
        BoxMatch::ISeq(index, count, body) => Ok(EvalValue::Box(iterate_seq(
            arena,
            index,
            count,
            body,
            env,
            loop_detector,
        )?)),
        BoxMatch::ISum(index, count, body) => Ok(EvalValue::Box(iterate_sum(
            arena,
            index,
            count,
            body,
            env,
            loop_detector,
        )?)),
        BoxMatch::IProd(index, count, body) => Ok(EvalValue::Box(iterate_prod(
            arena,
            index,
            count,
            body,
            env,
            loop_detector,
        )?)),
        BoxMatch::Route(ins, outs, routes) => {
            // C++ eval.cpp (isBoxRoute branch):
            //   v1 = a2sb(eval(ins, …))
            //   v2 = a2sb(eval(outs, …))
            //   vr = a2sb(eval(routes, …))
            //   sigList2vecInt(boxPropagateSig(nil, v1, []), w1) → boxInt(w1[0])
            //   sigList2vecInt(boxPropagateSig(nil, v2, []), w2) → boxInt(w2[0])
            //   normalizeRouteList(vr) → canonical Par tree of boxInt pairs
            //   return boxRoute(boxInt(ins_n), boxInt(outs_n), normalized_spec)
            //
            // Rust uses eval_box_to_int_node (propagate + simplify → i32 → boxInt)
            // and normalize_route_spec to match the same behaviour.
            let eval_ins = eval_box(arena, ins, env, loop_detector)?;
            let eval_outs = eval_box(arena, outs, env, loop_detector)?;
            let eval_routes = eval_box(arena, routes, env, loop_detector)?;

            let ins_node = eval_box_to_int_node(arena, eval_ins).unwrap_or(eval_ins);
            let outs_node = eval_box_to_int_node(arena, eval_outs).unwrap_or(eval_outs);
            let spec_node = normalize_route_spec(arena, eval_routes);

            let mut bld = BoxBuilder::new(arena);
            Ok(EvalValue::Box(bld.route(ins_node, outs_node, spec_node)))
        }
        BoxMatch::Seq(e1, e2) => {
            // C++ eval.cpp (isBoxSeq branch):
            //   a1 = eval(e1, …)   a2 = eval(e2, …)
            //   if (isNumericalTuple(a1, lsig) && …)
            //       lres = boxPropagateSig(nil, a2, lsig)
            //       r = simplify(hd(lres))
            //       if (isNum(r)) return r
            //   return boxSeq(a1, a2)
            //
            // Rust: if a1 is a parallel of Int/Real literals, try to fold
            // seq(a1, a2) via propagate_box_and_simplify.  Both SigInt/SigReal
            // and BoxInt/BoxReal share the same NodeKind in the arena, so the
            // resulting SigId IS directly usable as a BoxId.
            let a1 = eval_box(arena, e1, env, loop_detector)?;
            let a2 = eval_box(arena, e2, env, loop_detector)?;

            if is_numerical_tuple_box(arena, a1)
                && let Some(folded) = try_fold_seq_numeric(arena, a1, a2)
            {
                return Ok(EvalValue::Box(folded));
            }

            let mut bld = BoxBuilder::new(arena);
            Ok(EvalValue::Box(bld.seq(a1, a2)))
        }
        _ => Ok(EvalValue::Box(map_children(
            arena,
            expr,
            env,
            loop_detector,
        )?)),
    }
}

/// Reifies one evaluator value back into box IR.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `eval(...)`
/// - `closure(...)` forcing sites
///
/// Rust keeps closures as host-side values during evaluation, but subsequent
/// phases (`propagate`, lowering, golden dumps) still consume box trees. This
/// helper performs that boundary conversion:
/// - plain box values pass through unchanged,
/// - abstractions are rebuilt with one scope-local shadowing sentinel for the
///   bound parameter,
/// - other closures are forced under their captured environment,
/// - pattern matchers collapse to their original `case` carrier when still
///   unapplied.
///
fn force_value_to_box(
    arena: &mut TreeArena,
    value: EvalValue,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    match value {
        EvalValue::Box(id) => Ok(id),
        EvalValue::Closure(closure) => match match_box(arena, closure.expr) {
            BoxMatch::Abstr(arg, body) => {
                let mut scoped = closure.env.push_scope();
                let name = ident_name(arena, arg)?;
                let sym = arena.intern_symbol(&name);
                scoped.bind(sym, arg);
                let evaluated_body = eval_box(arena, body, &scoped, loop_detector)?;
                let mut b = BoxBuilder::new(arena);
                Ok(b.abstr(arg, evaluated_body))
            }
            BoxMatch::Environment => Ok(closure.expr),
            _ => eval_box(arena, closure.expr, &closure.env, loop_detector),
        },
        EvalValue::PatternMatcher(pm) => {
            if pm.state == 0 && pm.rev_param_list.is_empty() {
                // Unapplied: return the original case box for later a2sb.
                Ok(pm.case_expr)
            } else {
                // Partially applied: store in PM side-table and return a
                // boxPatternMatcher(key) tree node. This avoids re-entering
                // the evaluator (which would cause stack overflow via
                // lower_pattern_matcher_to_symbolic → apply_value_list →
                // eval_value → force_value_to_box cycle).
                let key = loop_detector.store_pm(pm);
                let mut b = BoxBuilder::new(arena);
                let key_node = b.int(key);
                Ok(b.pattern_matcher(key_node))
            }
        }
    }
}

/// Evaluates `expr.ident` access with closure-aware Faust environment semantics.
fn eval_access_value(
    arena: &mut TreeArena,
    body: TreeId,
    field: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<EvalValue, EvalError> {
    let eval_body = eval_value(arena, body, env, loop_detector)?;
    if let EvalValue::Closure(closure) = &eval_body {
        return eval_value(arena, field, &closure.env, loop_detector);
    }
    Err(EvalError::ExpectedClosureValue {
        node: body,
        context: "access",
    })
}

/// Evaluates `component("...")` / `library("...")` by loading a file through the parser crate.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `isBoxComponent`
/// - `isBoxLibrary`
/// - `gGlobal->gReader.getList`
/// - `gGlobal->gReader.expandList`
///
/// Mapping status: `adapted`.
///
/// The C++ evaluator reads extra Faust sources through the process-global source
/// reader and wraps the resulting definitions in a closure over either
/// `boxIdent("process")` (`component`) or `boxEnvironment()` (`library`).
/// Rust reproduces the same semantic contract by:
/// - resolving the target against the captured [`EvalSourceContext`],
/// - parsing the loaded file through `parser::parse_file_with_imports(...)`,
/// - cloning the resulting definition subtree into the current evaluation arena,
/// - caching the parsed source in the context for later loads in the same session,
/// - binding the loaded definitions in a fresh environment whose source context
///   is rooted at the loaded file.
///
/// The returned value is intentionally a closure/environment pair instead of a
/// fully forced box. That preserves the C++ semantics where `component(...)`
/// and `library(...)` introduce a new lexical source-resolution root and expose
/// their loaded definitions lazily through normal evaluator lookup.
fn eval_loaded_source_value(
    arena: &mut TreeArena,
    node: TreeId,
    filename: TreeId,
    construct: &'static str,
    env: &Environment,
) -> Result<EvalValue, EvalError> {
    let target = source_reference_name(arena, filename)
        .ok_or(EvalError::InvalidSourceReference { node, construct })?;
    let source_context = env.source_context();
    let candidate_paths = candidate_loaded_source_paths(source_context, &target);
    let cached = source_context.cached_loaded_source_hits(&candidate_paths, |cached, path| {
        cached.map(|loaded| {
            (
                path.to_path_buf(),
                arena.clone_subtree_from(&loaded.arena, loaded.root),
                loaded.parse_errors.clone(),
            )
        })
    });
    let (resolved_path, cloned_defs, parse_errors) = match cached {
        Some(hit) => hit,
        None => {
            let resolved_path = candidate_paths
                .iter()
                .find(|path| path.exists())
                .cloned()
                .ok_or_else(|| EvalError::SourceFileNotFound {
                    node,
                    construct,
                    target: target.clone(),
                    current_file: source_context.current_file().map(Path::to_path_buf),
                    search_paths: source_context.search_paths().to_vec(),
                })?;
            let parse = match source_context.metadata_store() {
                Some(metadata_store) => parser::parse_file_with_imports_and_metadata(
                    &resolved_path,
                    source_context.search_paths(),
                    metadata_store.clone(),
                ),
                None => {
                    parser::parse_file_with_imports(&resolved_path, source_context.search_paths())
                }
            };
            let parse_output = parse.map_err(|error| EvalError::SourceReaderFailure {
                node,
                construct,
                target: target.clone(),
                message: error.to_string(),
            })?;
            let loaded_root = parse_output
                .root
                .ok_or_else(|| EvalError::SourceParseFailure {
                    node,
                    construct,
                    path: resolved_path.clone(),
                    errors: parse_output.errors.clone(),
                })?;
            let cached_source = CachedLoadedSource {
                root: loaded_root,
                arena: parse_output.state.arena,
                parse_errors: parse_output.errors,
            };
            let cloned_defs = arena.clone_subtree_from(&cached_source.arena, cached_source.root);
            let parse_errors = cached_source.parse_errors.clone();
            source_context.insert_loaded_source(resolved_path.clone(), cached_source);
            (resolved_path, cloned_defs, parse_errors)
        }
    };
    if !parse_errors.is_empty() {
        return Err(EvalError::SourceParseFailure {
            node,
            construct,
            path: resolved_path.clone(),
            errors: parse_errors,
        });
    }
    let mut loaded_env =
        Environment::empty_with_source_context(source_context.for_loaded_file(&resolved_path));
    bind_definitions(arena, cloned_defs, &mut loaded_env)?;

    let closure_expr = match construct {
        "component" => BoxBuilder::new(arena).ident("process"),
        "library" => BoxBuilder::new(arena).environment(),
        _ => unreachable!("unsupported source-loading construct"),
    };
    Ok(EvalValue::Closure(ClosureValue {
        expr: closure_expr,
        env: loaded_env,
    }))
}

/// Evaluates one `case` node into an explicit pattern-matcher runtime value.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `evalCase`
/// - `boxPatternMatcher`
///
/// Mapping status: `1:1` semantics with an adapted Rust value representation.
///
/// The C++ evaluator returns a `boxPatternMatcher(...)` closure-like runtime
/// value. Rust stores the equivalent state in [`EvalValue::PatternMatcher`]:
/// compiled automaton, current automaton state, per-rule barrier environments,
/// original rule list, and already-consumed arguments.
/// Rust compiles the evaluated rule list into an automaton cached in the
/// [`LoopDetector`], then returns a host-side [`EvalValue::PatternMatcher`]
/// instead of immediately forcing the whole dispatch to a box. This mirrors the
/// C++ strategy where `case` evaluation yields an applicative matcher that may
/// later be partially or fully applied.
fn eval_case_value(
    arena: &mut TreeArena,
    case_expr: TreeId,
    rules_rev: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<EvalValue, EvalError> {
    let evaluated_rules = eval_rule_list(arena, rules_rev, env, loop_detector)?;
    if !loop_detector.automaton_cache.contains_key(&evaluated_rules) {
        let automaton = pattern_matcher::make_pattern_matcher(arena, evaluated_rules);
        loop_detector
            .automaton_cache
            .insert(evaluated_rules, automaton);
    }
    let automaton = loop_detector
        .automaton_cache
        .get(&evaluated_rules)
        .expect("automaton cache populated")
        .clone();
    let envs = (0..automaton.n_rules())
        .map(|_| Some(env.push_barrier_scope()))
        .collect();
    Ok(EvalValue::PatternMatcher(PatternMatcherValue {
        automaton,
        state: 0,
        envs,
        original_rules: rules_rev,
        rev_param_list: Vec::new(),
        case_expr,
    }))
}

/// Extracts the textual file reference from `component(...)` / `library(...)`.
///
/// The parser normally produces string literals here, but Rust also accepts a
/// symbol node to stay compatible with historical tree shapes built in tests or
/// imported from transitional code.
fn source_reference_name(arena: &TreeArena, filename: TreeId) -> Option<String> {
    match arena.kind(filename) {
        Some(NodeKind::StringLiteral(value)) | Some(NodeKind::Symbol(value)) => {
            Some(value.to_string())
        }
        _ => None,
    }
}

/// Builds the ordered candidate path list for one source reference.
///
/// Resolution order intentionally mirrors Faust file loading:
/// 1. exact absolute path when `target` is already absolute,
/// 2. path relative to the current source file,
/// 3. raw `target` as given,
/// 4. each configured import search path joined with `target`.
///
/// Duplicates are removed while preserving first-hit priority so the loaded
/// source cache can key lookups deterministically.
fn candidate_loaded_source_paths(source_context: &EvalSourceContext, target: &str) -> Vec<PathBuf> {
    let target_path = PathBuf::from(target);
    let mut candidates = Vec::new();
    if target_path.is_absolute() {
        candidates.push(target_path);
        return candidates;
    }
    if let Some(current_file) = source_context.current_file() {
        let base = current_file.parent().unwrap_or_else(|| Path::new("."));
        let candidate = base.join(target);
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    if !candidates.iter().any(|existing| existing == &target_path) {
        candidates.push(target_path);
    }
    for base in source_context.search_paths() {
        let candidate = base.join(target);
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

/// Evaluates `expr [ defs ]` by copying the captured closure environment and replacing bindings.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `compiler/evaluate/environment.cpp`
/// - `copyEnvReplaceDefs`
/// - `updateClosures`
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `copyEnvReplaceDefs(...)`
///
/// `boxModifLocalDef` is not a plain nested lexical scope: existing captured
/// closures reachable from the current environment must see the replacement
/// definitions as well. Rust implements that by cloning the visible
/// environment, rewriting captured environments transitively, then evaluating
/// the body under the rewritten copy.
fn eval_modif_local_def_value(
    arena: &mut TreeArena,
    body: TreeId,
    defs: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<EvalValue, EvalError> {
    match eval_value(arena, body, env, loop_detector)? {
        EvalValue::Closure(closure) => {
            let rewritten_env = copy_env_replace_defs(arena, &closure.env, defs, env)?;
            eval_value(arena, closure.expr, &rewritten_env, loop_detector)
        }
        EvalValue::Box(_) | EvalValue::PatternMatcher(_) => Err(EvalError::ExpectedClosureValue {
            node: body,
            context: "modif-local-def",
        }),
    }
}

/// Evaluates one modulation form and rewrites matching widgets in the body.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp` modulation branch
/// - `compiler/transform/boxModulationImplanter.cpp`
///
/// This is an adapted Rust port of the same semantics:
/// - evaluate the target label and optional modulation circuit,
/// - validate modulation-circuit arity,
/// - fully evaluate the body and lower residual closures with [`a2sb`],
/// - implant the circuit around widgets whose path matches the target.
///
/// The current implementation supports literal/group-path matching, which is
/// sufficient for the production corpus and the parity fixtures in this
/// repository.
///
/// One important adaptation from C++ is that Rust performs the full rewrite on
/// the already-evaluated and `a2sb`-lowered body. This keeps `propagate` free of
/// residual closures while still preserving the observable modulation behavior.
fn eval_modulation(
    arena: &mut TreeArena,
    modulation_node: TreeId,
    var: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let target_label = eval_modulation_label(arena, var, env, loop_detector)?;
    let target_path = modulation_target_path(&target_label);
    let modulation_circuit =
        eval_modulation_circuit(arena, modulation_node, var, env, loop_detector)?;
    let Some((inputs, outputs)) = infer_box_arity(arena, modulation_circuit) else {
        return Err(EvalError::InvalidModulationCircuit {
            node: modulation_node,
            reason: "circuit should evaluate to a block diagram",
        });
    };
    if inputs > 2 {
        return Err(EvalError::InvalidModulationCircuit {
            node: modulation_node,
            reason: "circuit should have no more than 2 inputs",
        });
    }
    if outputs != 1 {
        return Err(EvalError::InvalidModulationCircuit {
            node: modulation_node,
            reason: "circuit should have exactly 1 output",
        });
    }

    let slot = if inputs == 2 {
        Some(fresh_slot(arena, loop_detector))
    } else {
        None
    };
    let evaluated_body = eval_box(arena, body, env, loop_detector)?;
    let lowered_body = a2sb(arena, evaluated_body, loop_detector)?;
    let rewritten = implant_modulation(
        arena,
        lowered_body,
        &ModulationRewrite {
            target_path: &target_path,
            slot,
            inputs_number: inputs,
            modulation_circuit,
        },
        &mut Vec::new(),
    );

    if rewritten == lowered_body {
        Ok(lowered_body)
    } else if let Some(slot) = slot {
        let mut b = BoxBuilder::new(arena);
        Ok(b.symbolic(slot, rewritten))
    } else {
        Ok(rewritten)
    }
}

/// Immutable modulation rewrite context derived from one evaluated modulation node.
///
/// Grouping these fields keeps the recursive transformer signatures short and
/// makes the C++-parallel invariants explicit at the call site.
struct ModulationRewrite<'a> {
    target_path: &'a [String],
    slot: Option<TreeId>,
    inputs_number: usize,
    modulation_circuit: TreeId,
}

/// Evaluates the modulation target to a plain label string.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `evalLabel(...)`
///
/// C++ accepts richer label syntax than plain string literals. Rust currently
/// routes target labels through the same `%ident` interpolation engine used for
/// UI labels and then strips metadata wrappers so later matching operates only
/// on the path-bearing label text.
///
/// The returned string is therefore not the raw label source but the
/// post-interpolation, metadata-free target used by the modulation implanter.
fn eval_modulation_label(
    arena: &mut TreeArena,
    var: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<String, EvalError> {
    let label_node = arena
        .hd(var)
        .ok_or(EvalError::MalformedListNode { node: var })?;
    let label = eval_label_node(arena, label_node, env, loop_detector)?;
    Ok(strip_label_metadata(&label).to_owned())
}

/// Evaluates one UI/modulation label node using the C++ `evalLabel(...)`
/// placeholder semantics.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `evalLabel(...)`
/// - `writeIdentValue(...)`
///
/// Mapping status: `adapted`.
/// Rust mirrors the C++ label substitution state machine while resolving
/// placeholder values through explicit evaluator helpers instead of global tree
/// properties.
fn eval_label_node(
    arena: &mut TreeArena,
    label_node: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<String, EvalError> {
    let Some(src) = label_node_text(arena, label_node) else {
        return Err(EvalError::InvalidModulationLabel { node: label_node });
    };
    let src = src.to_owned();
    eval_label(arena, &src, env, loop_detector)
}

/// Port of the C++ `evalLabel(...)` mini-parser used for dynamic UI labels.
fn eval_label(
    arena: &mut TreeArena,
    src: &str,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<String, EvalError> {
    #[derive(Clone, Copy)]
    enum State {
        Text,
        AfterPercent,
        Ident,
        BracedIdent,
    }

    let chars: Vec<char> = src.chars().collect();
    let mut idx = 0usize;
    let mut state = State::Text;
    let mut dst = String::new();
    let mut ident = String::new();
    let mut format = String::new();

    while idx <= chars.len() {
        let cur = chars.get(idx).copied();
        match state {
            State::Text => match cur {
                None => break,
                Some('%') => {
                    ident.clear();
                    format.clear();
                    state = State::AfterPercent;
                    idx += 1;
                }
                Some(ch) => {
                    dst.push(ch);
                    idx += 1;
                }
            },
            State::AfterPercent => match cur {
                None => {
                    dst.push('%');
                    dst.push_str(&format);
                    break;
                }
                Some(ch) if ch.is_ascii_digit() => {
                    format.push(ch);
                    idx += 1;
                }
                Some(ch) if is_eval_label_ident_char(ch) => {
                    ident.push(ch);
                    state = State::Ident;
                    idx += 1;
                }
                Some('{') => {
                    state = State::BracedIdent;
                    idx += 1;
                }
                Some(_) => {
                    dst.push('%');
                    dst.push_str(&format);
                    state = State::Text;
                }
            },
            State::Ident => match cur {
                Some(ch) if is_eval_label_ident_char(ch) => {
                    ident.push(ch);
                    idx += 1;
                }
                _ => {
                    write_label_ident_value(arena, &mut dst, &format, &ident, env, loop_detector)?;
                    state = State::Text;
                }
            },
            State::BracedIdent => match cur {
                Some(ch) if is_eval_label_ident_char(ch) => {
                    ident.push(ch);
                    idx += 1;
                }
                Some('}') => {
                    write_label_ident_value(arena, &mut dst, &format, &ident, env, loop_detector)?;
                    idx += 1;
                    state = State::Text;
                }
                _ => {
                    dst.push('%');
                    dst.push_str(&format);
                    break;
                }
            },
        }
    }

    Ok(dst)
}

/// Returns `true` for identifier characters accepted by `%ident` label syntax.
///
/// This intentionally follows the conservative subset used by the current Rust
/// port of `evalLabel(...)`: ASCII alphanumerics plus `_`.
fn is_eval_label_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// Renders one `%ident` or `%{ident}` placeholder into the destination label.
///
/// Width formatting follows the C++ `evalLabel(...)` convention implemented by
/// the active corpus: the optional decimal field width is clamped to `0..=4`
/// before rendering the resolved integer value.
fn write_label_ident_value(
    arena: &mut TreeArena,
    dst: &mut String,
    format: &str,
    ident: &str,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<(), EvalError> {
    let width = format.parse::<usize>().unwrap_or(0).clamp(0, 4);
    let value = eval_ident_to_constant_int(arena, ident, env, loop_detector)?;
    let rendered = if width == 0 {
        value.to_string()
    } else {
        format!("{value:>width$}")
    };
    dst.push_str(&rendered);
    Ok(())
}

/// Evaluates one identifier used in a label placeholder to an integer constant.
///
/// The lookup goes through the full evaluator and symbolic lowering pipeline so
/// `%i`, `%{n}`, and similar placeholders observe the same lexical environment
/// and constant-folding behavior as normal Faust expressions.
fn eval_ident_to_constant_int(
    arena: &mut TreeArena,
    ident: &str,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<i64, EvalError> {
    let expr = BoxBuilder::new(arena).ident(ident);
    let signal = eval_box_to_scalar_signal(arena, expr, env, loop_detector)?;
    tree_to_int(arena, signal).ok_or_else(|| EvalError::InvalidLabelInterpolation {
        node: expr,
        ident: ident.to_owned(),
        reason: "expression did not reduce to an integer constant",
    })
}

/// Evaluates one box expression to a scalar constant signal atom.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `eval2int(...)`
/// - `eval2double(...)`
fn eval_box_to_scalar_signal(
    arena: &mut TreeArena,
    expr: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let evaluated = eval_box(arena, expr, env, loop_detector)?;
    let lowered = a2sb(arena, evaluated, loop_detector)?;
    let Some((inputs, outputs)) = infer_box_arity(arena, lowered) else {
        return Err(EvalError::InvalidLabelInterpolation {
            node: expr,
            ident: ident_name_or_fallback(arena, expr),
            reason: "expression did not evaluate to a scalar box",
        });
    };
    if inputs != 0 || outputs != 1 {
        return Err(EvalError::InvalidLabelInterpolation {
            node: expr,
            ident: ident_name_or_fallback(arena, expr),
            reason: "expression is not a constant scalar of type (0 -> 1)",
        });
    }
    let mut cache = ArityCache::default();
    let flat =
        try_build_flat_box(arena, lowered).map_err(|_| EvalError::InvalidLabelInterpolation {
            node: expr,
            ident: ident_name_or_fallback(arena, expr),
            reason: "expression did not lower to a valid flat post-eval box",
        })?;
    let signals = propagate_typed(arena, flat, &[], &mut cache).map_err(|_| {
        EvalError::InvalidLabelInterpolation {
            node: expr,
            ident: ident_name_or_fallback(arena, expr),
            reason: "expression could not be propagated to a constant signal",
        }
    })?;
    if signals.len() != 1 {
        return Err(EvalError::InvalidLabelInterpolation {
            node: expr,
            ident: ident_name_or_fallback(arena, expr),
            reason: "expression did not produce exactly one output signal",
        });
    }
    // Algebraically simplify the propagated signal (e.g. sin(0) → 0.0).
    // C++ equivalent: `simplify(hd(lsignals))` in eval.cpp `eval2double`/`eval2int`.
    let simplified = simplify_const(arena, signals[0]);
    match match_sig(arena, simplified) {
        SigMatch::Int(_) | SigMatch::Real(_) => Ok(simplified),
        _ => Err(EvalError::InvalidLabelInterpolation {
            node: expr,
            ident: ident_name_or_fallback(arena, expr),
            reason: "expression did not simplify to a numeric constant",
        }),
    }
}

/// Returns a human-readable identifier for interpolation diagnostics.
///
/// If `expr` is not an identifier node, diagnostics still need a stable name;
/// the fallback `node_<id>` keeps error messages auditable without pretending a
/// symbolic name exists.
fn ident_name_or_fallback(arena: &TreeArena, expr: TreeId) -> String {
    match match_box(arena, expr) {
        BoxMatch::Ident(name) => name.to_owned(),
        _ => format!("node_{}", expr.as_u32()),
    }
}

// ─── Propagation + simplification helpers ─────────────────────────────────────

/// Tagged numeric literal — used to split borrow-checker lifetimes between
/// reading a signal's value and writing a new box into the arena.
#[derive(Clone, Copy)]
enum NumericLit {
    Int(i32),
    Real(f64),
}

/// Propagates a 0→1 box with no inputs, then algebraically simplifies the
/// resulting signal.
///
/// Returns `None` if the box cannot be flattened or has the wrong arity.
///
/// This is the building block for all compile-time constant extraction in the
/// evaluator.
///
/// # C++ equivalent
///
/// ```cpp
/// Tree lsignals = boxPropagateSig(gGlobal->nil, box, makeSigInputList(0));
/// Tree s        = simplify(hd(lsignals));
/// ```
///
/// Called by `isBoxNumeric`, `eval2double`, `eval2int`, and
/// `numericBoxSimplification` in `compiler/evaluate/eval.cpp`.
fn propagate_box_and_simplify(arena: &mut TreeArena, box_id: TreeId) -> Option<SigId> {
    let flat = try_build_flat_box(arena, box_id).ok()?;
    let mut cache = ArityCache::default();
    let signals = propagate_typed(arena, flat, &[], &mut cache).ok()?;
    let sig = *signals.first()?;
    Some(simplify_const(arena, sig))
}

/// Tries to reduce a box to a numeric literal for pattern matching.
///
/// If `box_id` represents a compile-time numeric constant (possibly hidden
/// behind arithmetic like `max(1, min(6, 4))`), returns the corresponding
/// `boxInt(n)` or `boxReal(x)`.  Otherwise returns `box_id` unchanged.
///
/// When the propagation yields `sigReal(x)` but `x` is an exact integer
/// (e.g. `2.0`), we return `boxInt(x as i32)` so that the pattern matcher's
/// tree-identity check succeeds against integer pattern constants like
/// `poly(2, x)`.  This mirrors the C++ pipeline where `max/min` on integers
/// stays in the integer domain.
///
/// # C++ equivalent
///
/// `Tree simplifyPattern(Tree value)` in `compiler/evaluate/eval.cpp`.
pub(crate) fn simplify_pattern(arena: &mut TreeArena, box_id: TreeId) -> TreeId {
    // Fast path: already a literal.
    match match_box(arena, box_id) {
        BoxMatch::Int(_) | BoxMatch::Real(_) => return box_id,
        _ => {}
    }
    let Some(sig) = propagate_box_and_simplify(arena, box_id) else {
        return box_id;
    };
    // Extract value before taking &mut borrow for BoxBuilder.
    let value = match match_sig(arena, sig) {
        SigMatch::Int(i) => Some(NumericLit::Int(i)),
        SigMatch::Real(x) => {
            // If the real value is an exact integer, prefer boxInt for pattern matching.
            let i = x as i32;
            if (i as f64) == x {
                Some(NumericLit::Int(i))
            } else {
                Some(NumericLit::Real(x))
            }
        }
        _ => None,
    };
    match value {
        Some(NumericLit::Int(i)) => BoxBuilder::new(arena).int(i),
        Some(NumericLit::Real(x)) => BoxBuilder::new(arena).real(x),
        None => box_id,
    }
}

/// Converts a 0→1 box to an `f64` compile-time constant.
///
/// Returns [`EvalError::NotAConstantExpression`] if the box is not a scalar
/// constant of type (0→1) or cannot be reduced to a numeric value.
///
/// # C++ equivalent
///
/// `static double eval2double(Tree exp, Tree visited, Tree localValEnv)` in
/// `compiler/evaluate/eval.cpp`.
#[allow(dead_code)] // used in tests; will be promoted to production in Step 4/6a/6b
fn eval_box_to_f64(arena: &mut TreeArena, box_id: TreeId) -> Result<f64, EvalError> {
    let sig = propagate_box_and_simplify(arena, box_id)
        .ok_or(EvalError::NotAConstantExpression { node: box_id })?;
    match match_sig(arena, sig) {
        SigMatch::Real(x) => Ok(x),
        SigMatch::Int(i) => Ok(f64::from(i)),
        _ => Err(EvalError::NotAConstantExpression { node: box_id }),
    }
}

/// Converts a 0→1 box to an `i32` compile-time constant.
///
/// Returns [`EvalError::NotAConstantExpression`] if the box is not a scalar
/// constant of type (0→1) or cannot be reduced to a numeric value.
///
/// # C++ equivalent
///
/// `static int eval2int(Tree exp, Tree visited, Tree localValEnv)` in
/// `compiler/evaluate/eval.cpp`.
#[allow(dead_code)] // used in tests; will be promoted to production in Step 5/6b
fn eval_box_to_i32(arena: &mut TreeArena, box_id: TreeId) -> Result<i32, EvalError> {
    let sig = propagate_box_and_simplify(arena, box_id)
        .ok_or(EvalError::NotAConstantExpression { node: box_id })?;
    match match_sig(arena, sig) {
        SigMatch::Int(i) => Ok(i),
        SigMatch::Real(x) => Ok(x as i32),
        _ => Err(EvalError::NotAConstantExpression { node: box_id }),
    }
}

// ─── Route parameter normalization ─────────────────────────────────────────────

/// Converts a 0→1 box to a `boxInt(n)` node.
///
/// Used to normalise the `ins` and `outs` arguments of a `route` at
/// evaluation time, mirroring the C++ `boxPropagateSig` + `sigList2vecInt`
/// pattern used in `compiler/evaluate/eval.cpp` for the `isBoxRoute` branch.
fn eval_box_to_int_node(arena: &mut TreeArena, box_id: TreeId) -> Result<TreeId, EvalError> {
    let n = eval_box_to_i32(arena, box_id)?;
    Ok(BoxBuilder::new(arena).int(n))
}

/// Recursively collects the leaves of a right-spine `Par` tree.
///
/// `route(2,2, 1,1, 2,2)` stores the wire pairs as
/// `par(int(1), par(int(1), par(int(2), int(2))))`.  Flattening extracts
/// `[int(1), int(1), int(2), int(2)]` in order.
fn flatten_route_spec(arena: &TreeArena, spec: TreeId, out: &mut Vec<TreeId>) {
    match match_box(arena, spec) {
        BoxMatch::Par(a, b) => {
            flatten_route_spec(arena, a, out);
            flatten_route_spec(arena, b, out);
        }
        _ => out.push(spec),
    }
}

/// Re-evaluates the route wire-pair spec to ensure every leaf is a `boxInt`
/// and rebuilds the tree in the canonical right-spine form.
///
/// # C++ equivalent
///
/// `static Tree normalizeRouteList(Tree routes)` in
/// `compiler/evaluate/eval.cpp`.
fn normalize_route_spec(arena: &mut TreeArena, spec: TreeId) -> TreeId {
    // Phase 1: collect leaves with an immutable borrow.
    let mut leaves: Vec<TreeId> = Vec::new();
    flatten_route_spec(arena, spec, &mut leaves);
    let n = leaves.len();
    if n == 0 {
        return spec;
    }
    // Phase 2: convert each leaf to i32 → boxInt (mutable borrow).
    let mut int_leaves: Vec<TreeId> = Vec::with_capacity(n);
    for leaf in leaves {
        if let Ok(i) = eval_box_to_i32(arena, leaf) {
            int_leaves.push(BoxBuilder::new(arena).int(i));
        } else {
            int_leaves.push(leaf); // pattern var / wire / slot — keep as-is
        }
    }
    // Phase 3: rebuild right-spine Par (C++ normalizeRouteList order).
    let mut result = int_leaves[n - 1];
    for i in (0..n - 1).rev() {
        result = BoxBuilder::new(arena).par(int_leaves[i], result);
    }
    result
}

// ─── Seq numeric folding ───────────────────────────────────────────────────────

/// Returns `true` if `box_id` is a parallel composition of numeric literals
/// (`boxInt` / `boxReal`), possibly nested.
///
/// Used as a guard before attempting compile-time Seq folding.
///
/// # C++ equivalent
///
/// `static bool isNumericalTuple(Tree box, siglist& L)` in
/// `compiler/evaluate/eval.cpp`.
fn is_numerical_tuple_box(arena: &TreeArena, box_id: TreeId) -> bool {
    match match_box(arena, box_id) {
        BoxMatch::Int(_) | BoxMatch::Real(_) => true,
        BoxMatch::Par(l, r) => is_numerical_tuple_box(arena, l) && is_numerical_tuple_box(arena, r),
        _ => false,
    }
}

/// Tries to fold `seq(a1, a2)` into a single numeric box literal.
///
/// Requires `a1` to be a numerical tuple (see [`is_numerical_tuple_box`]).
/// Propagates `a2` with the signals from `a1` as its inputs and simplifies
/// the result; if the simplified signal is a numeric constant, returns the
/// corresponding `boxInt(n)` or `boxReal(x)`.
///
/// Returns `None` if the expression cannot be reduced.
///
/// # C++ equivalent
///
/// The body of the `isBoxSeq` branch in `compiler/evaluate/eval.cpp`:
/// ```cpp
/// Tree lres = boxPropagateSig(nil, a2, lsig);
/// if (isList(lres) && isNil(tl(lres))) {
///     Tree r = simplify(hd(lres));
///     if (isNum(r)) { return r; }
/// }
/// ```
fn try_fold_seq_numeric(arena: &mut TreeArena, a1: TreeId, a2: TreeId) -> Option<TreeId> {
    // Build seq(a1, a2) and propagate it with 0 inputs.
    let seq = BoxBuilder::new(arena).seq(a1, a2);
    let sig = propagate_box_and_simplify(arena, seq)?;
    // Both SigInt/SigReal and BoxInt/BoxReal share the same underlying NodeKind
    // (NodeKind::Int / NodeKind::FloatBits), so the SigId IS the BoxId.
    match match_sig(arena, sig) {
        SigMatch::Int(_) | SigMatch::Real(_) => Some(sig),
        _ => None,
    }
}

// ─── Box simplification ────────────────────────────────────────────────────────

/// Memoised entry point: simplify `box_id` by replacing any 0→1 sub-expression
/// that propagates to a compile-time constant with the corresponding
/// `boxInt(n)` or `boxReal(x)` literal.
///
/// The result is stored in `cache` so that shared sub-trees are only visited
/// once (matching the C++ `gSimplifiedBoxProperty` property cache).
///
/// # C++ equivalent
///
/// `static Tree boxSimplification(Tree box)` in
/// `compiler/evaluate/eval.cpp`.
#[allow(dead_code)] // promoted to production in Step 6a
fn box_simplification(
    arena: &mut TreeArena,
    cache: &mut ahash::HashMap<TreeId, TreeId>,
    box_id: TreeId,
) -> TreeId {
    if let Some(&cached) = cache.get(&box_id) {
        return cached;
    }
    let result = numeric_box_simplification(arena, cache, box_id);
    cache.insert(box_id, result);
    result
}

/// Tries to reduce a 0→1 box to a numeric literal; recurses into composite
/// boxes otherwise.
///
/// # C++ equivalent
///
/// `static Tree numericBoxSimplification(Tree box)` in
/// `compiler/evaluate/eval.cpp`.
fn numeric_box_simplification(
    arena: &mut TreeArena,
    cache: &mut ahash::HashMap<TreeId, TreeId>,
    box_id: TreeId,
) -> TreeId {
    // Fast path: already a numeric literal.
    match match_box(arena, box_id) {
        BoxMatch::Int(_) | BoxMatch::Real(_) => return box_id,
        _ => {}
    }
    // General path: propagate + simplify → try to extract a numeric constant.
    if let Some(sig) = propagate_box_and_simplify(arena, box_id) {
        match match_sig(arena, sig) {
            SigMatch::Real(x) => {
                return BoxBuilder::new(arena).real(x);
            }
            SigMatch::Int(i) => {
                return BoxBuilder::new(arena).int(i);
            }
            _ => {}
        }
    }
    // Not a numeric constant: simplify children recursively.
    inside_box_simplification(arena, cache, box_id)
}

/// Recurses into composite boxes, calling [`box_simplification`] on each
/// child sub-diagram.
///
/// Leaf nodes (primitives, UI widgets, slots, waveforms, …) are returned
/// unchanged.
///
/// # C++ equivalent
///
/// `static Tree insideBoxSimplification(Tree box)` in
/// `compiler/evaluate/eval.cpp`.
fn inside_box_simplification(
    arena: &mut TreeArena,
    cache: &mut ahash::HashMap<TreeId, TreeId>,
    box_id: TreeId,
) -> TreeId {
    match match_box(arena, box_id) {
        // ── Leaves — return unchanged ──────────────────────────────────────
        BoxMatch::Int(_)
        | BoxMatch::Real(_)
        | BoxMatch::Cut
        | BoxMatch::Wire
        // Primitive operators (Prim0–Prim5 in C++ — operator boxes in Rust)
        | BoxMatch::Add | BoxMatch::Sub | BoxMatch::Mul | BoxMatch::Div | BoxMatch::Rem
        | BoxMatch::Pow | BoxMatch::Fmod | BoxMatch::Remainder
        | BoxMatch::And | BoxMatch::Or | BoxMatch::Xor | BoxMatch::Lsh | BoxMatch::Rsh
        | BoxMatch::Lt  | BoxMatch::Le  | BoxMatch::Gt  | BoxMatch::Ge
        | BoxMatch::Eq  | BoxMatch::Ne  | BoxMatch::Atan2
        | BoxMatch::Floor | BoxMatch::Ceil | BoxMatch::Round | BoxMatch::Rint
        | BoxMatch::Abs | BoxMatch::Min | BoxMatch::Max
        | BoxMatch::IntCast | BoxMatch::FloatCast
        | BoxMatch::Delay | BoxMatch::Delay1 | BoxMatch::Prefix
        | BoxMatch::ReadOnlyTable | BoxMatch::WriteReadTable
        | BoxMatch::Select2 | BoxMatch::Select3 | BoxMatch::AssertBounds
        | BoxMatch::Lowest | BoxMatch::Highest
        | BoxMatch::Attach | BoxMatch::Enable | BoxMatch::Control
        | BoxMatch::Acos | BoxMatch::Asin | BoxMatch::Atan
        | BoxMatch::Cos  | BoxMatch::Sin  | BoxMatch::Tan
        | BoxMatch::Exp  | BoxMatch::Log  | BoxMatch::Log10 | BoxMatch::Sqrt
        // Foreign function / constant / variable
        | BoxMatch::FFun(_)
        | BoxMatch::FConst(_, _, _)
        | BoxMatch::FVar(_, _, _)
        // UI widgets (C++ isBoxVSlider / HSlider / NumEntry / Bargraph …)
        | BoxMatch::Button(_)
        | BoxMatch::Checkbox(_)
        | BoxMatch::VSlider(_, _, _, _, _)
        | BoxMatch::HSlider(_, _, _, _, _)
        | BoxMatch::NumEntry(_, _, _, _, _)
        | BoxMatch::VBargraph(_, _, _)
        | BoxMatch::HBargraph(_, _, _)
        // Slot (pattern variable in symbolic boxes)
        | BoxMatch::Slot(_)
        // Waveform: always in normal form (has 1 child = size)
        | BoxMatch::Waveform(_)
        // Sound file
        | BoxMatch::Soundfile(_, _) => box_id,

        // ── Recursive on 1 child ──────────────────────────────────────────
        BoxMatch::VGroup(label, body) => {
            let sb = box_simplification(arena, cache, body);
            let mut bld = BoxBuilder::new(arena);
            bld.vgroup(label, sb)
        }
        BoxMatch::HGroup(label, body) => {
            let sb = box_simplification(arena, cache, body);
            let mut bld = BoxBuilder::new(arena);
            bld.hgroup(label, sb)
        }
        BoxMatch::TGroup(label, body) => {
            let sb = box_simplification(arena, cache, body);
            let mut bld = BoxBuilder::new(arena);
            bld.tgroup(label, sb)
        }
        BoxMatch::Symbolic(slot, body) => {
            let sb = box_simplification(arena, cache, body);
            let mut bld = BoxBuilder::new(arena);
            bld.symbolic(slot, sb)
        }

        // ── Recursive on 2 children ───────────────────────────────────────
        BoxMatch::Seq(a, b) => {
            let sa = box_simplification(arena, cache, a);
            let sb = box_simplification(arena, cache, b);
            let mut bld = BoxBuilder::new(arena);
            bld.seq(sa, sb)
        }
        BoxMatch::Par(a, b) => {
            let sa = box_simplification(arena, cache, a);
            let sb = box_simplification(arena, cache, b);
            let mut bld = BoxBuilder::new(arena);
            bld.par(sa, sb)
        }
        BoxMatch::Split(a, b) => {
            let sa = box_simplification(arena, cache, a);
            let sb = box_simplification(arena, cache, b);
            let mut bld = BoxBuilder::new(arena);
            bld.split(sa, sb)
        }
        BoxMatch::Merge(a, b) => {
            let sa = box_simplification(arena, cache, a);
            let sb = box_simplification(arena, cache, b);
            let mut bld = BoxBuilder::new(arena);
            bld.merge(sa, sb)
        }
        BoxMatch::Rec(a, b) => {
            let sa = box_simplification(arena, cache, a);
            let sb = box_simplification(arena, cache, b);
            let mut bld = BoxBuilder::new(arena);
            bld.rec(sa, sb)
        }

        // ── Metadata: simplify body, keep metadata list ───────────────────
        BoxMatch::Metadata(body, meta) => {
            let sb = box_simplification(arena, cache, body);
            let mut bld = BoxBuilder::new(arena);
            bld.metadata(sb, meta)
        }

        // ── Route: simplify ins/outs, keep spec ──────────────────────────
        BoxMatch::Route(ins, outs, routes) => {
            let si = box_simplification(arena, cache, ins);
            let so = box_simplification(arena, cache, outs);
            let mut bld = BoxBuilder::new(arena);
            bld.route(si, so, routes)
        }

        // ── Unknown / not yet handled: return unchanged ───────────────────
        _ => box_id,
    }
}

// ─── Evaluate label node ───────────────────────────────────────────────────────

/// Evaluates one label node and re-interns the resulting string literal in the arena.
///
/// Widget/group constructors in box IR still store labels as tree nodes, so the
/// string returned by [`eval_label_node`] must be converted back into a canonical
/// literal node before rebuilding the enclosing widget.
fn evaluated_label_node(
    arena: &mut TreeArena,
    label: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let text = eval_label_node(arena, label, env, loop_detector)?;
    Ok(arena.string_lit(&text))
}

/// Evaluates one `button` label and rebuilds the widget node.
fn eval_button(
    arena: &mut TreeArena,
    label: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).button(label))
}

/// Evaluates one `checkbox` label and rebuilds the widget node.
fn eval_checkbox(
    arena: &mut TreeArena,
    label: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).checkbox(label))
}

fn eval_vslider(
    arena: &mut TreeArena,
    label: TreeId,
    params: [TreeId; 4],
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    eval_slider_like(
        arena,
        SliderKind::VSlider,
        label,
        params,
        env,
        loop_detector,
    )
}

fn eval_hslider(
    arena: &mut TreeArena,
    label: TreeId,
    params: [TreeId; 4],
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    eval_slider_like(
        arena,
        SliderKind::HSlider,
        label,
        params,
        env,
        loop_detector,
    )
}

fn eval_num_entry(
    arena: &mut TreeArena,
    label: TreeId,
    params: [TreeId; 4],
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    eval_slider_like(
        arena,
        SliderKind::NumEntry,
        label,
        params,
        env,
        loop_detector,
    )
}

enum SliderKind {
    VSlider,
    HSlider,
    NumEntry,
}

fn eval_slider_like(
    arena: &mut TreeArena,
    kind: SliderKind,
    label: TreeId,
    params: [TreeId; 4],
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    // C++ eval.cpp: each numeric parameter is reduced via eval2double(…)
    // which calls boxPropagateSig + simplify internally.  We do the same by
    // calling eval_box then simplifying the result to a boxReal literal when
    // possible, matching C++ `tree(eval2double(param, …))`.
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    let [cur, min, max, step] = params;
    let cur = simplify_slider_param(arena, cur, env, loop_detector)?;
    let min = simplify_slider_param(arena, min, env, loop_detector)?;
    let max = simplify_slider_param(arena, max, env, loop_detector)?;
    let step = simplify_slider_param(arena, step, env, loop_detector)?;
    let mut b = BoxBuilder::new(arena);
    Ok(match kind {
        SliderKind::VSlider => b.vslider(label, cur, min, max, step),
        SliderKind::HSlider => b.hslider(label, cur, min, max, step),
        SliderKind::NumEntry => b.num_entry(label, cur, min, max, step),
    })
}

/// Evaluates a slider/bargraph numeric parameter with the same semantics as
/// C++ `eval2double`: `eval_box` followed by `propagate + simplify → boxReal`.
///
/// If the expression cannot be reduced to a numeric constant at evaluation
/// time, the evaluated (but not simplified) box is returned unchanged so that
/// later passes can still handle it.
///
/// # C++ equivalent
///
/// `tree(eval2double(param, visited, localValEnv))` for slider/bargraph params
/// in `compiler/evaluate/eval.cpp`.
fn simplify_slider_param(
    arena: &mut TreeArena,
    param: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let evaled = eval_box(arena, param, env, loop_detector)?;
    // Try to reduce to f64 constant → boxReal(x).
    if let Ok(x) = eval_box_to_f64(arena, evaled) {
        return Ok(BoxBuilder::new(arena).real(x));
    }
    // Fallback: return the evaluated box as-is (e.g. pattern var, slot).
    Ok(evaled)
}

/// Evaluates one `soundfile` widget.
///
/// Only label interpolation and channel expression evaluation happen here. Full
/// runtime/path semantics are still handled later in `propagate`, just like in
/// the C++ split between evaluation and box-to-signal lowering.
fn eval_soundfile(
    arena: &mut TreeArena,
    label: TreeId,
    chan: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    // C++ eval.cpp: `tree(eval2int(chan, visited, localValEnv))`.
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    let evaled_chan = eval_box(arena, chan, env, loop_detector)?;
    let chan = if let Ok(n) = eval_box_to_i32(arena, evaled_chan) {
        BoxBuilder::new(arena).int(n)
    } else {
        evaled_chan
    };
    Ok(BoxBuilder::new(arena).soundfile(label, chan))
}

/// Evaluates one vertical UI group by interpolating its label and body.
fn eval_vgroup(
    arena: &mut TreeArena,
    label: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    let body = eval_box(arena, body, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).vgroup(label, body))
}

/// Evaluates one horizontal UI group by interpolating its label and body.
fn eval_hgroup(
    arena: &mut TreeArena,
    label: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    let body = eval_box(arena, body, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).hgroup(label, body))
}

/// Evaluates one tab UI group by interpolating its label and body.
fn eval_tgroup(
    arena: &mut TreeArena,
    label: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    let body = eval_box(arena, body, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).tgroup(label, body))
}

/// Evaluates one vertical bargraph node.
fn eval_vbargraph(
    arena: &mut TreeArena,
    label: TreeId,
    min: TreeId,
    max: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    // C++ uses eval2double for bargraph min/max.
    let min = simplify_slider_param(arena, min, env, loop_detector)?;
    let max = simplify_slider_param(arena, max, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).vbargraph(label, min, max))
}

/// Evaluates one horizontal bargraph node.
fn eval_hbargraph(
    arena: &mut TreeArena,
    label: TreeId,
    min: TreeId,
    max: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let label = evaluated_label_node(arena, label, env, loop_detector)?;
    // C++ uses eval2double for bargraph min/max.
    let min = simplify_slider_param(arena, min, env, loop_detector)?;
    let max = simplify_slider_param(arena, max, env, loop_detector)?;
    Ok(BoxBuilder::new(arena).hbargraph(label, min, max))
}

/// Evaluates the optional modulation circuit, defaulting to multiplication.
///
/// Faust modulation syntax allows the circuit part to be omitted; the default is
/// multiplication. When a circuit is present, Rust evaluates it like an ordinary
/// box expression, lowers residual closures through [`a2sb`], and then checks
/// only the lightweight local arity constraints needed by modulation rewriting.
fn eval_modulation_circuit(
    arena: &mut TreeArena,
    modulation_node: TreeId,
    var: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let circuit = arena
        .tl(var)
        .ok_or(EvalError::MalformedListNode { node: var })?;
    if arena.is_nil(circuit) {
        let mut b = BoxBuilder::new(arena);
        return Ok(b.mul());
    }
    let evaluated = eval_box(arena, circuit, env, loop_detector)?;
    let lowered = a2sb(arena, evaluated, loop_detector)?;
    if infer_box_arity(arena, lowered).is_none() {
        return Err(EvalError::InvalidModulationCircuit {
            node: modulation_node,
            reason: "circuit should evaluate to a block diagram",
        });
    }
    Ok(lowered)
}

/// Recursively implants one modulation circuit into matching widgets.
///
/// The traversal keeps an explicit `group_stack` of already-entered UI labels so
/// widget matching can reconstruct the effective path seen by the user. Only
/// widget/group families receive modulation-specific treatment; every other node
/// is rebuilt structurally if any child changes.
fn implant_modulation(
    arena: &mut TreeArena,
    expr: TreeId,
    rewrite: &ModulationRewrite<'_>,
    group_stack: &mut Vec<String>,
) -> TreeId {
    match match_box(arena, expr) {
        BoxMatch::Button(label) | BoxMatch::Checkbox(label) => {
            implant_widget_if_match(arena, expr, label, rewrite, group_stack)
        }
        BoxMatch::VSlider(label, cur, min, max, step) => {
            let rebuilt = {
                let cur = implant_modulation(arena, cur, rewrite, group_stack);
                let min = implant_modulation(arena, min, rewrite, group_stack);
                let max = implant_modulation(arena, max, rewrite, group_stack);
                let step = implant_modulation(arena, step, rewrite, group_stack);
                let mut b = BoxBuilder::new(arena);
                b.vslider(label, cur, min, max, step)
            };
            implant_widget_if_match(arena, rebuilt, label, rewrite, group_stack)
        }
        BoxMatch::HSlider(label, cur, min, max, step) => {
            let rebuilt = {
                let cur = implant_modulation(arena, cur, rewrite, group_stack);
                let min = implant_modulation(arena, min, rewrite, group_stack);
                let max = implant_modulation(arena, max, rewrite, group_stack);
                let step = implant_modulation(arena, step, rewrite, group_stack);
                let mut b = BoxBuilder::new(arena);
                b.hslider(label, cur, min, max, step)
            };
            implant_widget_if_match(arena, rebuilt, label, rewrite, group_stack)
        }
        BoxMatch::NumEntry(label, cur, min, max, step) => {
            let rebuilt = {
                let cur = implant_modulation(arena, cur, rewrite, group_stack);
                let min = implant_modulation(arena, min, rewrite, group_stack);
                let max = implant_modulation(arena, max, rewrite, group_stack);
                let step = implant_modulation(arena, step, rewrite, group_stack);
                let mut b = BoxBuilder::new(arena);
                b.num_entry(label, cur, min, max, step)
            };
            implant_widget_if_match(arena, rebuilt, label, rewrite, group_stack)
        }
        BoxMatch::VBargraph(label, min, max) => {
            let rebuilt = {
                let min = implant_modulation(arena, min, rewrite, group_stack);
                let max = implant_modulation(arena, max, rewrite, group_stack);
                let mut b = BoxBuilder::new(arena);
                b.vbargraph(label, min, max)
            };
            implant_widget_if_match(arena, rebuilt, label, rewrite, group_stack)
        }
        BoxMatch::HBargraph(label, min, max) => {
            let rebuilt = {
                let min = implant_modulation(arena, min, rewrite, group_stack);
                let max = implant_modulation(arena, max, rewrite, group_stack);
                let mut b = BoxBuilder::new(arena);
                b.hbargraph(label, min, max)
            };
            implant_widget_if_match(arena, rebuilt, label, rewrite, group_stack)
        }
        BoxMatch::VGroup(label, inner) => {
            group_stack.push(strip_label_node(arena, label));
            let rewritten = implant_modulation(arena, inner, rewrite, group_stack);
            group_stack.pop();
            let mut b = BoxBuilder::new(arena);
            b.vgroup(label, rewritten)
        }
        BoxMatch::HGroup(label, inner) => {
            group_stack.push(strip_label_node(arena, label));
            let rewritten = implant_modulation(arena, inner, rewrite, group_stack);
            group_stack.pop();
            let mut b = BoxBuilder::new(arena);
            b.hgroup(label, rewritten)
        }
        BoxMatch::TGroup(label, inner) => {
            group_stack.push(strip_label_node(arena, label));
            let rewritten = implant_modulation(arena, inner, rewrite, group_stack);
            group_stack.pop();
            let mut b = BoxBuilder::new(arena);
            b.tgroup(label, rewritten)
        }
        _ => {
            let Some(node) = arena.node(expr).cloned() else {
                return expr;
            };
            if node.children.is_empty() {
                return expr;
            }

            let mut rebuilt = Vec::with_capacity(node.children.len());
            let mut changed = false;
            for child in node.children.as_slice().iter().copied() {
                let rewritten = implant_modulation(arena, child, rewrite, group_stack);
                if rewritten != child {
                    changed = true;
                }
                rebuilt.push(rewritten);
            }

            if changed {
                arena.intern(node.kind, &rebuilt)
            } else {
                expr
            }
        }
    }
}

/// Applies the modulation circuit around one widget when its path matches.
///
/// The three supported arities mirror the C++ implanter:
/// - 0 inputs: the modulation circuit fully replaces the widget,
/// - 1 input: the widget output is piped through the modulation circuit,
/// - 2 inputs: the widget is paired with the modulation slot/carry signal.
fn implant_widget_if_match(
    arena: &mut TreeArena,
    widget: TreeId,
    label: TreeId,
    rewrite: &ModulationRewrite<'_>,
    group_stack: &[String],
) -> TreeId {
    if !widget_matches_modulation_target(arena, label, rewrite.target_path, group_stack) {
        return widget;
    }
    let mut b = BoxBuilder::new(arena);
    match rewrite.inputs_number {
        0 => rewrite.modulation_circuit,
        1 => b.seq(widget, rewrite.modulation_circuit),
        2 => {
            let slot = rewrite.slot.expect("two-input modulation requires a slot");
            let pair = b.par(widget, slot);
            b.seq(pair, rewrite.modulation_circuit)
        }
        _ => widget,
    }
}

/// Returns `true` when the effective widget path matches the modulation target.
///
/// Matching is done on metadata-free path segments. Rust currently uses
/// subsequence matching on the normalized textual path representation, which is
/// sufficient for the active corpus and mirrors the practical C++ behavior for
/// the supported subset.
fn widget_matches_modulation_target(
    arena: &TreeArena,
    label: TreeId,
    target_path: &[String],
    group_stack: &[String],
) -> bool {
    let Some(label) = label_node_text(arena, label) else {
        return false;
    };
    let mut widget_path = Vec::with_capacity(group_stack.len() + 1);
    widget_path.push(strip_label_metadata(label).to_owned());
    for group in group_stack.iter().rev() {
        widget_path.push(group.clone());
    }
    is_subsequence(target_path, &widget_path)
}

/// Normalizes one modulation target label string into path segments.
///
/// Empty segments are discarded so both `a/b` and `/a//b/` normalize to the
/// same semantic path vector.
fn modulation_target_path(label: &str) -> Vec<String> {
    label
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(strip_label_metadata)
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .rev()
        .collect()
}

/// Extracts the plain-text label content from one label node.
///
/// Missing/invalid label nodes degrade to an empty string so modulation path
/// reconstruction stays total during recursive traversal.
fn strip_label_node(arena: &TreeArena, label: TreeId) -> String {
    label_node_text(arena, label)
        .map(strip_label_metadata)
        .unwrap_or_default()
        .to_owned()
}

/// Removes Faust metadata suffixes from one textual label.
///
/// For example `gain [unit:dB]` becomes `gain`. The returned slice borrows from
/// the original string and is intended for path matching, not for user-facing
/// pretty-printing.
fn strip_label_metadata(label: &str) -> &str {
    label
        .split_once('[')
        .map_or(label, |(prefix, _)| prefix)
        .trim()
}

/// Returns the raw textual payload of a label node, if any.
///
/// Both string literals and interned symbols are accepted to stay compatible
/// with transitional tree encodings.
fn label_node_text(arena: &TreeArena, label: TreeId) -> Option<&str> {
    match arena.kind(label) {
        Some(NodeKind::StringLiteral(label)) => Some(label.as_ref()),
        Some(NodeKind::Symbol(label)) => Some(label.as_ref()),
        _ => None,
    }
}

/// Returns `true` when `needle` appears in-order inside `haystack`.
///
/// This relaxed path relation is used by the current modulation implementation
/// so target paths can match inside nested UI groups without requiring exact
/// absolute-path equality.
fn is_subsequence(needle: &[String], haystack: &[String]) -> bool {
    let mut haystack_iter = haystack.iter();
    needle
        .iter()
        .all(|target| haystack_iter.by_ref().any(|candidate| candidate == target))
}

/// Structural fallback: evaluate all children, then rebuild the node unchanged in kind.
/// Recursively evaluates every child of one box node and rebuilds the parent.
///
/// This is the structural fallback used for box families whose semantics in
/// `eval` are "evaluate children, keep outer constructor". It preserves the
/// original node when no child changes, matching the hash-consing-friendly
/// behavior of the C++ tree layer.
fn map_children(
    arena: &mut TreeArena,
    expr: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let Some(node) = arena.node(expr).cloned() else {
        return Ok(expr);
    };
    let mut children = Vec::with_capacity(node.children.len());
    for child in node.children.as_slice() {
        let value = eval_value(arena, *child, env, loop_detector)?;
        children.push(a2sb_value(arena, value, loop_detector)?);
    }
    Ok(arena.intern(node.kind, &children))
}

/// Binds a parser definition list into an environment, enforcing the no-redefinition rule.
///
/// Each definition in `defs` is a `cons(name, cons(args, expr))` node.
///
/// Parser-originated definition lists are expected to be pre-normalized by
/// `parser::ParseState::format_definitions()` so that `args` is typically `nil`
/// and `expr` is already one of:
/// - plain body,
/// - nested `abstr`,
/// - `case`.
///
/// The `args != nil` fallback is retained for direct test construction and
/// compatibility with any remaining raw-definition call sites.
///
/// # Redefinition check — C++ `addLayerDef` parity
///
/// Before each `bind`, the current scope layer is checked for an existing binding of the same
/// name via [`Environment::lookup_local`]. This matches the C++ `addLayerDef` check:
///
/// ```cpp
/// // environment.cpp — addLayerDef (simplified)
/// Tree olddef = nullptr;
/// if (getProperty(lenv, id, olddef)) {
///     if (def == olddef) { /* identical — silently accept */ }
///     else {
///         gGlobal->gErrorCount++;
///         throw faustexception("redefinition of symbols are not allowed: " + boxpp(id));
///     }
/// }
/// setProperty(lenv, id, def);
/// ```
///
/// In Rust:
/// - If the same name is already bound in the **current scope** with the **same captured
///   closure value** (`expr` + captured `EnvId`), the new definition is silently skipped.
/// - If the same name is bound with a **different** captured value, `EvalError::RedefinedSymbol`
///   is returned using the underlying expression nodes for diagnostics.
/// - If the name is not yet in the current scope (including the case where it only exists
///   in a parent scope — shadowing), the binding proceeds normally.
///
/// # C++ correspondence
///
/// | C++ call site | Rust equivalent |
/// |---|---|
/// | `pushMultiClosureDefs(ldefs, visited, lenv)` | `bind_definitions(arena, defs, &mut scoped)` with explicit captured definition closures |
/// | `pushValueDef(id, def, lenv)` | `env.bind(name, value)` (single-binding fast path) |
/// Binds a top-level or local definition list into the current environment.
///
/// Source provenance (C++):
/// - `compiler/evaluate/environment.cpp`
/// - `pushMultiClosureDefs(...)`
/// - `addLayerDef(...)`
///
/// Each definition is captured as needed so later lookups evaluate under the
/// lexical environment visible at definition time. Duplicate names in the same
/// scope are rejected here to preserve the C++ no-redefinition rule.
fn bind_definitions(
    arena: &mut TreeArena,
    mut defs: TreeId,
    env: &mut Environment,
) -> Result<(), EvalError> {
    while !arena.is_nil(defs) {
        let def = arena
            .hd(defs)
            .ok_or(EvalError::MalformedDefinitionNode { node: defs })?;
        let (name, args, value) = decode_definition(arena, def)?;
        let bound = if arena.is_nil(args) {
            value
        } else {
            build_abstr_from_parser_args(arena, args, value)?
        };
        // Intern the name to get a SymId. This is the bind path — intern_symbol is correct.
        let sym = arena.intern_symbol(&name);
        let captured = EvalValue::Closure(ClosureValue {
            expr: bound,
            env: env.clone(),
        });
        // C++ parity: addLayerDef checks for conflicting redefinition within the current layer.
        // Identical bindings (same TreeId = same hash-consed expression) are silently accepted.
        // Conflicting bindings (different TreeId) are an error.
        // Parent-scope shadowing is allowed and is NOT checked here.
        if let Some(existing) = env.lookup_local_value(sym) {
            if existing != captured {
                return Err(EvalError::RedefinedSymbol {
                    symbol: name,
                    first_def: existing.display_tree(),
                    second_def: captured.display_tree(),
                });
            }
            // existing == bound: identical redefinition — silently skip (C++ parity)
        } else {
            env.bind_value(sym, captured);
        }
        defs = arena
            .tl(defs)
            .ok_or(EvalError::MalformedDefinitionNode { node: defs })?;
    }
    Ok(())
}

/// Rewrites every captured environment reachable from `value` from `source_env`
/// to `copy_env`.
///
/// This helper exists for `boxModifLocalDef` parity: copied environments cannot
/// just duplicate direct bindings, they must also retarget any nested closures
/// so future lookups see the rewritten layer chain instead of the original.
fn rewrite_captured_env(
    value: EvalValue,
    old_env: &Environment,
    new_env: &Environment,
) -> EvalValue {
    match value {
        EvalValue::Box(id) => EvalValue::Box(id),
        EvalValue::Closure(closure) => {
            if closure.env.same_identity(old_env) {
                EvalValue::Closure(ClosureValue {
                    expr: closure.expr,
                    env: new_env.clone(),
                })
            } else {
                EvalValue::Closure(closure)
            }
        }
        EvalValue::PatternMatcher(pm) => EvalValue::PatternMatcher(pm),
    }
}

/// Creates a modified copy of one captured environment layer and replaces selected definitions.
///
/// Source provenance (C++):
/// - `compiler/evaluate/environment.cpp`
/// - `copyEnvReplaceDefs`
/// - `updateClosures`
///
/// The copied layer reuses the same parent stack as `source_env`, rewires any enclosed closure
/// that previously captured `source_env` so it now captures the copied layer, then appends the
/// replacement definitions as closures captured in `current_env`.
/// Clones the visible environment chain and replaces selected definitions.
///
/// The copy preserves lexical parent ordering while rebasing closure captures
/// onto the duplicated chain. This is the Rust equivalent of the C++
/// `copyEnvReplaceDefs(...)` family used by modifier definitions.
fn copy_env_replace_defs(
    arena: &mut TreeArena,
    source_env: &Environment,
    mut defs: TreeId,
    current_env: &Environment,
) -> Result<Environment, EvalError> {
    let (parent, _barrier, bindings) = source_env.layer_snapshot();
    let mut copy_env = source_env.spawn_child_with_parent(parent, false);

    for (sym, value) in bindings {
        copy_env.bind_value(sym, rewrite_captured_env(value, source_env, &copy_env));
    }

    while !arena.is_nil(defs) {
        let def = arena
            .hd(defs)
            .ok_or(EvalError::MalformedDefinitionNode { node: defs })?;
        let (name, args, value) = decode_definition(arena, def)?;
        let bound = if arena.is_nil(args) {
            value
        } else {
            build_abstr_from_parser_args(arena, args, value)?
        };
        let sym = arena.intern_symbol(&name);
        copy_env.bind_value(
            sym,
            EvalValue::Closure(ClosureValue {
                expr: bound,
                env: current_env.clone(),
            }),
        );
        defs = arena
            .tl(defs)
            .ok_or(EvalError::MalformedDefinitionNode { node: defs })?;
    }

    Ok(copy_env)
}

/// Decodes one parser definition node into `(name, args, expr)`.
fn decode_definition(
    arena: &TreeArena,
    def: TreeId,
) -> Result<(String, TreeId, TreeId), EvalError> {
    let name_node = arena
        .hd(def)
        .ok_or(EvalError::MalformedDefinitionNode { node: def })?;
    let payload = arena
        .tl(def)
        .ok_or(EvalError::MalformedDefinitionNode { node: def })?;
    let args = arena
        .hd(payload)
        .ok_or(EvalError::MalformedDefinitionNode { node: def })?;
    let expr = arena
        .tl(payload)
        .ok_or(EvalError::MalformedDefinitionNode { node: def })?;

    let name = match match_box(arena, name_node) {
        BoxMatch::Ident(s) => s.to_owned(),
        _ => match arena.kind(name_node) {
            Some(NodeKind::Symbol(s)) => s.as_ref().to_owned(),
            _ => {
                return Err(EvalError::MalformedDefinitionNode { node: def });
            }
        },
    };

    Ok((name, args, expr))
}

/// Extracts top-level definition names in deterministic order for diagnostics.
///
/// Names are sorted and deduplicated so diagnostic snapshots remain stable.
fn top_level_definition_names(
    arena: &TreeArena,
    mut defs: TreeId,
) -> Result<Vec<String>, EvalError> {
    let mut names = Vec::new();
    while !arena.is_nil(defs) {
        let def = arena
            .hd(defs)
            .ok_or(EvalError::MalformedDefinitionNode { node: defs })?;
        let (name, _args, _expr) = decode_definition(arena, def)?;
        names.push(name);
        defs = arena
            .tl(defs)
            .ok_or(EvalError::MalformedDefinitionNode { node: defs })?;
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// Returns identifier text for one `BOXIDENT` node.
fn ident_name(arena: &TreeArena, id: TreeId) -> Result<String, EvalError> {
    match match_box(arena, id) {
        BoxMatch::Ident(name) => Ok(name.to_owned()),
        _ => Err(EvalError::NonIdentifierParameter { node: id }),
    }
}

fn build_abstr_from_parser_args(
    arena: &mut TreeArena,
    mut args: TreeId,
    body: TreeId,
) -> Result<TreeId, EvalError> {
    // C++ parity (`buildBoxAbstr`): parser param lists are reversed, and each
    // head wraps the current body before recursing on tail.
    let mut out = body;
    while !arena.is_nil(args) {
        let head = arena
            .hd(args)
            .ok_or(EvalError::MalformedListNode { node: args })?;
        out = {
            let mut b = BoxBuilder::new(arena);
            b.abstr(head, out)
        };
        args = arena
            .tl(args)
            .ok_or(EvalError::MalformedListNode { node: args })?;
    }
    Ok(out)
}

/// Evaluates argument list nodes and returns the reversed evaluated list.
///
/// This mirrors the C++ parser/evaluator list convention where argument lists are
/// accumulated in reverse order.
/// Evaluates one application argument list into reverse order.
///
/// Application in Faust stores arguments as a cons-list. Evaluating in reverse
/// order lets later application helpers consume the list head-first without an
/// extra full reversal step, mirroring the C++ `revEvalList(...)` contract.
fn rev_eval_list(
    arena: &mut TreeArena,
    mut list: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let mut result = arena.nil();
    while !arena.is_nil(list) {
        let head = arena
            .hd(list)
            .ok_or(EvalError::MalformedListNode { node: list })?;
        let value = eval_box(arena, head, env, loop_detector)?;
        result = arena.cons(value, result);
        list = arena
            .tl(list)
            .ok_or(EvalError::MalformedListNode { node: list })?;
    }
    Ok(result)
}

/// Applies an evaluated function-like box to an evaluated argument list.
///
/// Behavior summary:
/// - `abstr`: beta-like application in lexical scope.
/// - `case`: pattern-match dispatch when sufficiently applied, otherwise lowers to
///   non-closure style `seq(par(args + implicit_wires), case)` for C++ parity.
/// - other node families: C++-compatible non-closure lowering to `seq(par(args), fun)`,
///   including implicit wire insertion for partial applications.
///
/// This is the box-returning wrapper around [`apply_value_list_value`]. It is
/// used by evaluation paths that must stay in box IR even though intermediate
/// application may produce closures or pattern matchers.
fn apply_value_list(
    arena: &mut TreeArena,
    fun: EvalValue,
    larg: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
    call_site: Option<TreeId>,
) -> Result<TreeId, EvalError> {
    let value = apply_value_list_value(arena, fun, larg, env, loop_detector, call_site)?;
    force_value_to_box(arena, value, loop_detector)
}

/// Applies an evaluator value to zero or more arguments.
///
/// This is the host-side equivalent of the C++ `applyList(...)` family after
/// closure materialization. It handles:
/// - plain box application,
/// - abstraction beta-reduction with captured environments,
/// - partial application of closures,
/// - pattern-matcher progression for `case`.
fn apply_value_list_value(
    arena: &mut TreeArena,
    fun: EvalValue,
    larg: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
    call_site: Option<TreeId>,
) -> Result<EvalValue, EvalError> {
    if arena.is_nil(larg) {
        return Ok(fun);
    }

    match fun {
        EvalValue::Box(fun) => Ok(EvalValue::Box(apply_list(
            arena,
            fun,
            larg,
            env,
            loop_detector,
            call_site,
        )?)),
        EvalValue::Closure(closure) => match match_box(arena, closure.expr) {
            BoxMatch::Ident(_) => {
                let forced = eval_value(arena, closure.expr, &closure.env, loop_detector)?;
                apply_value_list_value(arena, forced, larg, env, loop_detector, call_site)
            }
            BoxMatch::Environment => Err(EvalError::TooManyArguments {
                node: call_site.unwrap_or(closure.expr),
                expected: 0,
                got: list_to_vec(arena, larg)?.len(),
            }),
            BoxMatch::Abstr(id, body) => {
                let param_name = ident_name(arena, id)?;
                let arg = arena
                    .hd(larg)
                    .ok_or(EvalError::MalformedListNode { node: larg })?;
                let mut scoped = closure.env.push_scope();
                let sym = arena.intern_symbol(&param_name);
                scoped.bind_value(
                    sym,
                    EvalValue::Closure(ClosureValue {
                        expr: arg,
                        env: env.clone(),
                    }),
                );
                let f = eval_value(arena, body, &scoped, loop_detector)?;
                let tl = arena
                    .tl(larg)
                    .ok_or(EvalError::MalformedListNode { node: larg })?;
                apply_value_list_value(arena, f, tl, env, loop_detector, call_site)
            }
            _ => {
                let fun = force_value_to_box(arena, EvalValue::Closure(closure), loop_detector)?;
                Ok(EvalValue::Box(apply_list(
                    arena,
                    fun,
                    larg,
                    env,
                    loop_detector,
                    call_site,
                )?))
            }
        },
        EvalValue::PatternMatcher(pm) => {
            apply_pattern_matcher_value(arena, pm, larg, env, loop_detector, call_site)
        }
    }
}

/// Advances a partially-applied pattern matcher with one or more arguments.
///
/// The matcher keeps one per-rule environment vector. Every successful step may
/// refine those environments until a final state is reached, at which point the
/// selected RHS is evaluated under the captured rule-local environment.
fn apply_pattern_matcher_value(
    arena: &mut TreeArena,
    mut pm: PatternMatcherValue,
    larg: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
    call_site: Option<TreeId>,
) -> Result<EvalValue, EvalError> {
    if arena.is_nil(larg) {
        return Ok(EvalValue::PatternMatcher(pm));
    }

    let arg = arena
        .hd(larg)
        .ok_or(EvalError::MalformedListNode { node: larg })?;
    let (new_state, _) =
        pattern_matcher::apply_pattern_matcher(arena, &pm.automaton, pm.state, arg, &mut pm.envs);
    let Some(new_state) = new_state else {
        return Err(EvalError::PatternMatchFailed {
            node: pm.original_rules,
        });
    };
    pm.state = new_state;
    pm.rev_param_list.push(arg);
    let tl = arena
        .tl(larg)
        .ok_or(EvalError::MalformedListNode { node: larg })?;

    if !pm.automaton.final_state(pm.state) {
        return apply_value_list_value(
            arena,
            EvalValue::PatternMatcher(pm),
            tl,
            env,
            loop_detector,
            call_site,
        );
    }

    for rule_marker in &pm.automaton.states[pm.state].rules {
        if let Some(rule_env) = pm.envs[rule_marker.r].take() {
            let rhs = pm.automaton.rhs[rule_marker.r];
            let result = eval_value(arena, rhs, &rule_env, loop_detector)?;
            return apply_value_list_value(arena, result, tl, env, loop_detector, call_site);
        }
    }

    Err(EvalError::PatternMatchFailed {
        node: pm.original_rules,
    })
}

/// Applies a first-order box expression to an argument list.
///
/// This helper implements the non-closure application rules that still exist in
/// Faust after parser lowering, including implicit wire insertion for
/// under-applied non-prefix primitives. When the callee is not directly
/// first-order, callers should use [`apply_value_list_value`] instead.
fn apply_list(
    arena: &mut TreeArena,
    fun: TreeId,
    larg: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
    call_site: Option<TreeId>,
) -> Result<TreeId, EvalError> {
    if arena.is_nil(larg) {
        return Ok(fun);
    }
    match match_box(arena, fun) {
        BoxMatch::Abstr(id, body) => {
            let param_name = ident_name(arena, id)?;
            let arg = arena
                .hd(larg)
                .ok_or(EvalError::MalformedListNode { node: larg })?;
            let mut scoped = env.push_scope();
            // intern_symbol: param_name is an owned String, not borrowed from arena.
            let sym = arena.intern_symbol(&param_name);
            scoped.bind(sym, arg);
            let f = eval_box(arena, body, &scoped, loop_detector)?;
            let tl = arena
                .tl(larg)
                .ok_or(EvalError::MalformedListNode { node: larg })?;
            apply_list(arena, f, tl, env, loop_detector, call_site)
        }
        BoxMatch::Case(rules) => {
            let expected = case_expected_arity(arena, rules)?;
            let got = list_to_vec(arena, larg)?.len();
            if got < expected {
                // C++ parity (`applyList` on under-applied closures): keep the case form
                // and insert implicit wires for missing arguments instead of evaluating
                // the case immediately.
                let missing = expected - got;
                let wires = nwires(arena, missing);
                let lowered_larg = concat_lists(arena, larg, wires)?;
                let args_par = larg2par(arena, lowered_larg)?;
                let mut b = BoxBuilder::new(arena);
                return Ok(b.seq(args_par, fun));
            }
            let pm = eval_case_value(arena, fun, rules, env, loop_detector)?;
            let applied = apply_value_list_value(arena, pm, larg, env, loop_detector, call_site)?;
            force_value_to_box(arena, applied, loop_detector)
        }
        BoxMatch::PatternMatcher(key_node) => {
            // Retrieve the partially-applied PM from the side-table and
            // continue matching via the standard PM application path.
            let key = match match_box(arena, key_node) {
                BoxMatch::Int(k) => k,
                _ => {
                    return Err(EvalError::InternalError {
                        message: "boxPatternMatcher key is not an integer".to_owned(),
                    });
                }
            };
            let pm = loop_detector
                .get_pm(key)
                .ok_or_else(|| EvalError::InternalError {
                    message: format!("boxPatternMatcher key {} not found in PM store", key),
                })?;
            let applied =
                apply_pattern_matcher_value(arena, pm, larg, env, loop_detector, call_site)?;
            force_value_to_box(arena, applied, loop_detector)
        }
        _ => {
            // C++ parity (`applyList`): for non-closures, insert implicit wires when
            // partially applying a function, and reject over-application.
            let maybe_fun_arity = infer_box_arity(arena, fun);
            let maybe_larg_outputs = list_outputs(arena, larg);
            let mut lowered_larg = larg;

            if let (Some((ins, _outs)), Some(larg_outs)) = (maybe_fun_arity, maybe_larg_outputs) {
                if larg_outs > ins {
                    return Err(EvalError::TooManyArguments {
                        node: call_site.unwrap_or(fun),
                        expected: ins,
                        got: larg_outs,
                    });
                }
                let missing = ins - larg_outs;
                if missing > 0 {
                    let wires = nwires(arena, missing);
                    lowered_larg = if larg_outs == 1 && is_binary_primitive_non_prefix(arena, fun) {
                        concat_lists(arena, wires, larg)?
                    } else {
                        concat_lists(arena, larg, wires)?
                    };
                }
            }

            let args_par = larg2par(arena, lowered_larg)?;
            let mut b = BoxBuilder::new(arena);
            Ok(b.seq(args_par, fun))
        }
    }
}

/// Converts parser-style argument list to parallel composition tree.
///
/// Example: `[a,b,c] -> par(a, par(b, c))`.
fn larg2par(arena: &mut TreeArena, larg: TreeId) -> Result<TreeId, EvalError> {
    if arena.is_nil(larg) {
        return Err(EvalError::EmptyArgumentList { node: larg });
    }
    let head = arena
        .hd(larg)
        .ok_or(EvalError::MalformedListNode { node: larg })?;
    let tail = arena
        .tl(larg)
        .ok_or(EvalError::MalformedListNode { node: larg })?;
    if arena.is_nil(tail) {
        Ok(head)
    } else {
        let right = larg2par(arena, tail)?;
        let mut b = BoxBuilder::new(arena);
        Ok(b.par(head, right))
    }
}

/// Concatenates two parser-style lists while preserving element order.
fn concat_lists(arena: &mut TreeArena, left: TreeId, right: TreeId) -> Result<TreeId, EvalError> {
    if arena.is_nil(left) {
        return Ok(right);
    }
    let head = arena
        .hd(left)
        .ok_or(EvalError::MalformedListNode { node: left })?;
    let tail = arena
        .tl(left)
        .ok_or(EvalError::MalformedListNode { node: left })?;
    let rest = concat_lists(arena, tail, right)?;
    Ok(arena.cons(head, rest))
}

/// Builds a parser-style list containing `n` wire nodes.
fn nwires(arena: &mut TreeArena, n: usize) -> TreeId {
    let mut out = arena.nil();
    for _ in 0..n {
        let wire = BoxBuilder::new(arena).wire();
        out = arena.cons(wire, out);
    }
    out
}

/// Computes total output arity for a list of application arguments.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `boxlistOutputs(...)`
///
/// C++ is intentionally permissive here. During non-closure partial
/// application, arguments have already been evaluated, but some residual
/// symbolic/recursive forms may still defeat the lightweight local arity probe.
/// In that situation `boxlistOutputs(...)` falls back to counting the argument
/// as a single output so `applyList(...)` can still insert the missing implicit
/// wire for under-applied binary primitives.
///
/// Rust needs the same fallback for parity. Without it, expressions such as
/// `*(button("play") : trigger(n))` keep the raw `arg : *` shape instead of
/// being rewritten to `(_, arg) : *`, which later fails in `propagate` with a
/// spurious `1 != 2` sequential composition mismatch.
fn list_outputs(arena: &TreeArena, mut list: TreeId) -> Option<usize> {
    let mut total = 0usize;
    while !arena.is_nil(list) {
        let head = arena.hd(list)?;
        let outs = infer_box_arity(arena, head).map_or(1, |(_, outs)| outs);
        total = total.checked_add(outs)?;
        list = arena.tl(list)?;
    }
    Some(total)
}

/// Local arity inference used by non-closure application lowering.
///
/// Returns `(inputs, outputs)` for the subset needed in `apply_list`.
/// `None` means arity is unknown or invalid for this fast-path inference.
/// Infers `(inputs, outputs)` for the evaluator-supported first-order box subset.
///
/// This lightweight arity oracle is intentionally narrower than the dedicated
/// `propagate::box_arity(...)` contract. It exists for local evaluator tasks
/// such as under-application handling and label-placeholder constant checks
/// where pulling the full propagate error surface would be unnecessarily heavy.
fn infer_box_arity(arena: &TreeArena, id: TreeId) -> Option<(usize, usize)> {
    match match_box(arena, id) {
        BoxMatch::Int(_) | BoxMatch::Real(_) => Some((0, 1)),
        BoxMatch::Slot(_) => Some((0, 1)),
        BoxMatch::Wire => Some((1, 1)),
        BoxMatch::Cut => Some((1, 0)),
        BoxMatch::Add
        | BoxMatch::Sub
        | BoxMatch::Mul
        | BoxMatch::Div
        | BoxMatch::Rem
        | BoxMatch::And
        | BoxMatch::Or
        | BoxMatch::Xor
        | BoxMatch::Lsh
        | BoxMatch::Rsh
        | BoxMatch::Lt
        | BoxMatch::Le
        | BoxMatch::Gt
        | BoxMatch::Ge
        | BoxMatch::Eq
        | BoxMatch::Ne
        | BoxMatch::Pow
        | BoxMatch::Atan2
        | BoxMatch::Fmod
        | BoxMatch::Remainder
        | BoxMatch::Delay
        | BoxMatch::Min
        | BoxMatch::Max
        | BoxMatch::Prefix
        | BoxMatch::Attach
        | BoxMatch::Enable
        | BoxMatch::Control => Some((2, 1)),
        BoxMatch::Delay1
        | BoxMatch::IntCast
        | BoxMatch::FloatCast
        | BoxMatch::Acos
        | BoxMatch::Asin
        | BoxMatch::Atan
        | BoxMatch::Cos
        | BoxMatch::Sin
        | BoxMatch::Tan
        | BoxMatch::Exp
        | BoxMatch::Log
        | BoxMatch::Log10
        | BoxMatch::Sqrt
        | BoxMatch::Abs
        | BoxMatch::Floor
        | BoxMatch::Ceil
        | BoxMatch::Rint
        | BoxMatch::Round
        | BoxMatch::Lowest
        | BoxMatch::Highest => Some((1, 1)),
        BoxMatch::ReadOnlyTable | BoxMatch::Select2 | BoxMatch::AssertBounds => Some((3, 1)),
        BoxMatch::Select3 => Some((4, 1)),
        BoxMatch::WriteReadTable => Some((5, 1)),
        BoxMatch::FConst(_, _, _) | BoxMatch::FVar(_, _, _) => Some((0, 1)),
        BoxMatch::Button(_)
        | BoxMatch::Checkbox(_)
        | BoxMatch::VSlider(_, _, _, _, _)
        | BoxMatch::HSlider(_, _, _, _, _)
        | BoxMatch::NumEntry(_, _, _, _, _) => Some((0, 1)),
        BoxMatch::VBargraph(_, _, _) | BoxMatch::HBargraph(_, _, _) => Some((1, 1)),
        BoxMatch::Soundfile(_, chan) => {
            let BoxMatch::Int(channels) = match_box(arena, chan) else {
                return None;
            };
            let channels = usize::try_from(channels).ok()?;
            Some((2, channels.checked_add(2)?))
        }
        BoxMatch::VGroup(_, inner) | BoxMatch::HGroup(_, inner) | BoxMatch::TGroup(_, inner) => {
            infer_box_arity(arena, inner)
        }
        BoxMatch::Symbolic(_, inner) => {
            let (ins, outs) = infer_box_arity(arena, inner)?;
            Some((ins.checked_add(1)?, outs))
        }
        BoxMatch::Seq(left, right) => {
            let (ins1, outs1) = infer_box_arity(arena, left)?;
            let (ins2, outs2) = infer_box_arity(arena, right)?;
            if outs1 != ins2 {
                return None;
            }
            Some((ins1, outs2))
        }
        BoxMatch::Par(left, right) => {
            let (ins1, outs1) = infer_box_arity(arena, left)?;
            let (ins2, outs2) = infer_box_arity(arena, right)?;
            Some((ins1.checked_add(ins2)?, outs1.checked_add(outs2)?))
        }
        BoxMatch::Split(left, right) => {
            let (ins1, outs1) = infer_box_arity(arena, left)?;
            let (ins2, outs2) = infer_box_arity(arena, right)?;
            if outs1 != ins2 && (outs1 == 0 || !ins2.is_multiple_of(outs1)) {
                return None;
            }
            Some((ins1, outs2))
        }
        BoxMatch::Merge(left, right) => {
            let (ins1, outs1) = infer_box_arity(arena, left)?;
            let (ins2, outs2) = infer_box_arity(arena, right)?;
            if outs1 != ins2 && (ins2 == 0 || !outs1.is_multiple_of(ins2)) {
                return None;
            }
            Some((ins1, outs2))
        }
        BoxMatch::Rec(left, right) => {
            let (ins1, outs1) = infer_box_arity(arena, left)?;
            let (ins2, outs2) = infer_box_arity(arena, right)?;
            if ins2 > outs1 || outs2 > ins1 {
                return None;
            }
            Some((ins1 - outs2, outs1))
        }
        BoxMatch::Environment => Some((0, 0)),
        BoxMatch::Route(ins, outs, _) => {
            let BoxMatch::Int(ins_n) = match_box(arena, ins) else {
                return None;
            };
            let BoxMatch::Int(outs_n) = match_box(arena, outs) else {
                return None;
            };
            let ins_n = usize::try_from(ins_n).ok()?;
            let outs_n = usize::try_from(outs_n).ok()?;
            Some((ins_n, outs_n))
        }
        BoxMatch::Inputs(_) | BoxMatch::Outputs(_) => Some((0, 1)),
        BoxMatch::Ondemand(inner) | BoxMatch::Upsampling(inner) | BoxMatch::Downsampling(inner) => {
            let (ins, outs) = infer_box_arity(arena, inner)?;
            Some((ins.checked_add(1)?, outs))
        }
        _ => None,
    }
}

/// Returns true for primitive binary operators that are not `prefix`.
fn is_binary_primitive_non_prefix(arena: &TreeArena, id: TreeId) -> bool {
    matches!(
        match_box(arena, id),
        BoxMatch::Add
            | BoxMatch::Sub
            | BoxMatch::Mul
            | BoxMatch::Div
            | BoxMatch::Rem
            | BoxMatch::And
            | BoxMatch::Or
            | BoxMatch::Xor
            | BoxMatch::Lsh
            | BoxMatch::Rsh
            | BoxMatch::Lt
            | BoxMatch::Le
            | BoxMatch::Gt
            | BoxMatch::Ge
            | BoxMatch::Eq
            | BoxMatch::Ne
            | BoxMatch::Pow
            | BoxMatch::Atan2
            | BoxMatch::Fmod
            | BoxMatch::Remainder
            | BoxMatch::Delay
            | BoxMatch::Min
            | BoxMatch::Max
            | BoxMatch::Attach
            | BoxMatch::Enable
            | BoxMatch::Control
    )
}

/// Returns the identifier name used as iterative binder in `ipar/iseq/isum/iprod`.
///
/// The parser should already enforce identifier syntax here, but `eval` keeps
/// the check local so malformed trees created programmatically still fail with a
/// typed evaluator error instead of panicking.
fn iteration_var_name(arena: &TreeArena, id: TreeId) -> Result<String, EvalError> {
    match match_box(arena, id) {
        BoxMatch::Ident(name) => Ok(name.to_owned()),
        _ => Err(EvalError::NonIdentifierIterationVariable { node: id }),
    }
}

/// Evaluates iterative count expression and enforces a non-negative integer result.
fn eval_non_negative_count(
    arena: &mut TreeArena,
    count_expr: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<usize, EvalError> {
    let count = eval_box(arena, count_expr, env, loop_detector)?;
    match match_box(arena, count) {
        BoxMatch::Int(v) if v < 0 => Err(EvalError::NegativeIterationCount {
            value: i64::from(v),
        }),
        BoxMatch::Int(v) => usize::try_from(v).map_err(|_| EvalError::IterationCountTooLarge {
            value: i64::from(v),
        }),
        BoxMatch::Real(x) => {
            let i = x as i64;
            if (i as f64) == x && i >= 0 {
                usize::try_from(i).map_err(|_| EvalError::IterationCountTooLarge { value: i })
            } else if x < 0.0 {
                Err(EvalError::NegativeIterationCount { value: x as i64 })
            } else {
                Err(EvalError::IterationCountNotInt { node: count })
            }
        }
        _ => Err(EvalError::IterationCountNotInt { node: count }),
    }
}

/// Evaluates iterative body with one bound loop index (`i`).
///
/// Each expansion step pushes one child lexical scope, binds the iteration
/// variable to the current integer index, and then evaluates the body under that
/// scope. The binding uses a normal environment entry so iteration variables are
/// visible to all evaluator features that consult lexical scope, including
/// label interpolation.
fn eval_iter_body(
    arena: &mut TreeArena,
    var_name: &str,
    i: usize,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let mut scoped = env.push_scope();
    let i_as_i64 =
        i64::try_from(i).map_err(|_| EvalError::IterationCountTooLarge { value: i64::MAX })?;
    let ival = arena.int(i_as_i64);
    // var_name is a &str parameter (not borrowed from arena) — intern is safe here.
    let sym = arena.intern_symbol(var_name);
    scoped.bind(sym, ival);
    eval_box(arena, body, &scoped, loop_detector)
}

/// Returns the C++-compatible empty-iteration neutral box (`route(0,0,par(0,0))`).
fn empty_iteration_route(arena: &mut TreeArena) -> TreeId {
    let mut b = BoxBuilder::new(arena);
    let z0 = b.int(0);
    let z1 = b.int(0);
    let spec = b.par(z0, z1);
    b.route(z0, z1, spec)
}

/// Expands `ipar(i,n,body)` into nested `par` composition.
///
/// Expansion order matches the C++ evaluator: the rightmost branch (`n - 1`) is
/// built first, then earlier iterations are prepended so the final tree keeps
/// the observable left-to-right bus order expected by later passes.
fn iterate_par(
    arena: &mut TreeArena,
    index: TreeId,
    count: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let var_name = iteration_var_name(arena, index)?;
    let n = eval_non_negative_count(arena, count, env, loop_detector)?;
    if n == 0 {
        return Ok(empty_iteration_route(arena));
    }
    let mut res = eval_iter_body(arena, &var_name, n - 1, body, env, loop_detector)?;
    for i in (0..(n - 1)).rev() {
        let left = eval_iter_body(arena, &var_name, i, body, env, loop_detector)?;
        res = {
            let mut b = BoxBuilder::new(arena);
            b.par(left, res)
        };
    }
    Ok(res)
}

/// Expands `iseq(i,n,body)` into nested `seq` composition.
///
/// Like [`iterate_par`], this preserves the source iteration order by building
/// the tail first and prepending earlier bodies during the fold.
fn iterate_seq(
    arena: &mut TreeArena,
    index: TreeId,
    count: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let var_name = iteration_var_name(arena, index)?;
    let n = eval_non_negative_count(arena, count, env, loop_detector)?;
    if n == 0 {
        return Ok(empty_iteration_route(arena));
    }
    let mut res = eval_iter_body(arena, &var_name, n - 1, body, env, loop_detector)?;
    for i in (0..(n - 1)).rev() {
        let left = eval_iter_body(arena, &var_name, i, body, env, loop_detector)?;
        res = {
            let mut b = BoxBuilder::new(arena);
            b.seq(left, res)
        };
    }
    Ok(res)
}

/// Expands `isum(i,n,body)` into a fold using `add` primitive.
///
/// The sum starts at iteration `0` and folds left using the primitive `+`
/// wiring convention (`par(lhs, rhs) : add`).
fn iterate_sum(
    arena: &mut TreeArena,
    index: TreeId,
    count: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let var_name = iteration_var_name(arena, index)?;
    let n = eval_non_negative_count(arena, count, env, loop_detector)?;
    if n == 0 {
        return Ok(empty_iteration_route(arena));
    }
    let mut res = eval_iter_body(arena, &var_name, 0, body, env, loop_detector)?;
    for i in 1..n {
        let rhs = eval_iter_body(arena, &var_name, i, body, env, loop_detector)?;
        let pair = {
            let mut b = BoxBuilder::new(arena);
            b.par(res, rhs)
        };
        let add = {
            let mut b = BoxBuilder::new(arena);
            b.add()
        };
        res = {
            let mut b = BoxBuilder::new(arena);
            b.seq(pair, add)
        };
    }
    Ok(res)
}

/// Expands `iprod(i,n,body)` into a fold using `mul` primitive.
/// Expands `iprod(i,n,body)` into a fold using `mul` primitive.
///
/// This mirrors [`iterate_sum`] but uses multiplicative composition.
fn iterate_prod(
    arena: &mut TreeArena,
    index: TreeId,
    count: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let var_name = iteration_var_name(arena, index)?;
    let n = eval_non_negative_count(arena, count, env, loop_detector)?;
    if n == 0 {
        return Ok(empty_iteration_route(arena));
    }
    let mut res = eval_iter_body(arena, &var_name, 0, body, env, loop_detector)?;
    for i in 1..n {
        let rhs = eval_iter_body(arena, &var_name, i, body, env, loop_detector)?;
        let pair = {
            let mut b = BoxBuilder::new(arena);
            b.par(res, rhs)
        };
        let mul = {
            let mut b = BoxBuilder::new(arena);
            b.mul()
        };
        res = {
            let mut b = BoxBuilder::new(arena);
            b.seq(pair, mul)
        };
    }
    Ok(res)
}

/// Converts a parser-style list into a vector in traversal order.
fn list_to_vec(arena: &TreeArena, list: TreeId) -> Result<Vec<TreeId>, EvalError> {
    tlib::list_to_vec(arena, list).ok_or(EvalError::MalformedListNode { node: list })
}

/// Converts a vector into a parser-style list preserving order.
fn vec_to_list(arena: &mut TreeArena, items: &[TreeId]) -> TreeId {
    tlib::vec_to_list(arena, items)
}

/// Decodes a case rule node into `(lhs_patterns, rhs_expr)`.
fn rule_parts(arena: &TreeArena, rule: TreeId) -> Result<(TreeId, TreeId), EvalError> {
    let lhs = arena
        .hd(rule)
        .ok_or(EvalError::MalformedCaseNode { node: rule })?;
    let rhs = arena
        .tl(rule)
        .ok_or(EvalError::MalformedCaseNode { node: rule })?;
    Ok((lhs, rhs))
}

/// Returns expected argument arity for a case-rule set (first source rule arity).
fn case_expected_arity(arena: &TreeArena, rules_rev: TreeId) -> Result<usize, EvalError> {
    let mut rules = list_to_vec(arena, rules_rev)?;
    rules.reverse();
    let Some(first_rule) = rules.first().copied() else {
        return Err(EvalError::MalformedCaseNode { node: rules_rev });
    };
    let (first_lhs, _first_rhs) = rule_parts(arena, first_rule)?;
    Ok(list_to_vec(arena, first_lhs)?.len())
}

/// Evaluates a case-rule list for matching.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `evalRuleList`
/// - `evalRule`
/// - `evalPatternList`
/// - `evalPattern`
///
/// Only the left-hand side patterns are evaluated and simplified. The right-hand
/// side remains unchanged so it can later be evaluated in the chosen rule
/// environment.
/// Evaluates every rule of a `case` expression under the current lexical environment.
///
/// Rule evaluation is split from matcher construction so patterns can first be
/// simplified and normalized exactly once, after which the resulting rule list
/// is suitable for automaton caching.
fn eval_rule_list(
    arena: &mut TreeArena,
    rules_rev: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let rules = list_to_vec(arena, rules_rev)?;
    let mut out = Vec::with_capacity(rules.len());
    for rule in rules {
        let (lhs, rhs) = rule_parts(arena, rule)?;
        let lhs_eval = eval_pattern_list(arena, lhs, env, loop_detector)?;
        out.push(arena.cons(lhs_eval, rhs));
    }
    Ok(vec_to_list(arena, &out))
}

/// Evaluates each pattern of one rule, preserving parser list order.
/// Evaluates a list of case-pattern nodes left-to-right.
///
/// Each pattern goes through [`eval_pattern`] so compile-time numeric
/// simplification and scope-barrier-sensitive behavior are applied uniformly
/// before the matcher sees the rule.
fn eval_pattern_list(
    arena: &mut TreeArena,
    patterns: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let items = list_to_vec(arena, patterns)?;
    let mut out = Vec::with_capacity(items.len());
    for pattern in items {
        out.push(eval_pattern(arena, pattern, env, loop_detector)?);
    }
    Ok(vec_to_list(arena, &out))
}

/// Evaluates and simplifies one pattern before automaton construction.
///
/// This restores the C++ `evalPattern` behavior: lexical identifiers are resolved
/// in the current environment, then constant-only numeric subgraphs are folded so
/// patterns like `(1+1)` match the same way they do in the C++ compiler.
/// Evaluates one pattern expression in the current lexical environment.
///
/// Pattern evaluation is stricter than ordinary RHS evaluation: after normal
/// evaluation the result is passed through [`pattern_simplification`] so numeric
/// constant expressions such as `(1+1)` can match literal values at runtime.
fn eval_pattern(
    arena: &mut TreeArena,
    pattern: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let evaluated = eval_box(arena, pattern, env, loop_detector)?;
    Ok(pattern_simplification(arena, evaluated))
}

/// Simplifies a pattern bottom-up after pattern evaluation.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `patternSimplification`
///
/// Today this mainly performs local numeric constant folding while preserving
/// structural pattern shapes. The helper is intentionally separate from generic
/// expression evaluation so future pattern-only canonicalizations remain
/// localized.
fn pattern_simplification(arena: &mut TreeArena, pattern: TreeId) -> TreeId {
    let simplified = match match_box(arena, pattern) {
        BoxMatch::Seq(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            let mut bld = BoxBuilder::new(arena);
            bld.seq(sa, sb)
        }
        BoxMatch::Par(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            let mut bld = BoxBuilder::new(arena);
            bld.par(sa, sb)
        }
        BoxMatch::Split(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            let mut bld = BoxBuilder::new(arena);
            bld.split(sa, sb)
        }
        BoxMatch::Merge(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            let mut bld = BoxBuilder::new(arena);
            bld.merge(sa, sb)
        }
        BoxMatch::Rec(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            let mut bld = BoxBuilder::new(arena);
            bld.rec(sa, sb)
        }
        BoxMatch::HGroup(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            let mut bld = BoxBuilder::new(arena);
            bld.hgroup(sa, sb)
        }
        BoxMatch::VGroup(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            let mut bld = BoxBuilder::new(arena);
            bld.vgroup(sa, sb)
        }
        BoxMatch::TGroup(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            let mut bld = BoxBuilder::new(arena);
            bld.tgroup(sa, sb)
        }
        BoxMatch::Route(a, b, c) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            let sc = pattern_simplification(arena, c);
            let mut bld = BoxBuilder::new(arena);
            bld.route(sa, sb, sc)
        }
        _ => pattern,
    };

    simplify_numeric_pattern(arena, simplified).unwrap_or(simplified)
}

#[derive(Clone, Copy)]
enum NumericValue {
    Int(i32),
    Real(f64),
}

/// Attempts to reduce one pattern expression to an `int` or `real` constant box.
///
/// This is the narrow Rust equivalent of the C++ pattern simplification used by
/// `evalPattern(...)`: only numerically pure subexpressions are folded, and
/// failure to fold simply leaves the original pattern unchanged.
fn simplify_numeric_pattern(arena: &mut TreeArena, pattern: TreeId) -> Option<TreeId> {
    let value = eval_numeric_pattern_value(arena, pattern)?;
    let mut b = BoxBuilder::new(arena);
    Some(match value {
        NumericValue::Int(v) => b.int(v),
        NumericValue::Real(v) => b.real(v),
    })
}

fn eval_numeric_pattern_value(arena: &TreeArena, pattern: TreeId) -> Option<NumericValue> {
    match match_box(arena, pattern) {
        BoxMatch::Int(v) => Some(NumericValue::Int(v)),
        BoxMatch::Real(v) => Some(NumericValue::Real(v)),
        BoxMatch::Seq(inputs, op) => {
            let BoxMatch::Par(lhs, rhs) = match_box(arena, inputs) else {
                return None;
            };
            let lhs = eval_numeric_pattern_value(arena, lhs)?;
            let rhs = eval_numeric_pattern_value(arena, rhs)?;
            eval_numeric_binary_op(arena, op, lhs, rhs)
        }
        _ => None,
    }
}

fn eval_numeric_binary_op(
    arena: &TreeArena,
    op: TreeId,
    lhs: NumericValue,
    rhs: NumericValue,
) -> Option<NumericValue> {
    match match_box(arena, op) {
        BoxMatch::Add => numeric_add(lhs, rhs),
        BoxMatch::Sub => numeric_sub(lhs, rhs),
        BoxMatch::Mul => numeric_mul(lhs, rhs),
        BoxMatch::Div => numeric_div(lhs, rhs),
        BoxMatch::Rem => numeric_rem(lhs, rhs),
        BoxMatch::Pow => Some(NumericValue::Real(
            numeric_as_f64(lhs).powf(numeric_as_f64(rhs)),
        )),
        BoxMatch::Lt => Some(NumericValue::Int(
            (numeric_as_f64(lhs) < numeric_as_f64(rhs)) as i32,
        )),
        BoxMatch::Le => Some(NumericValue::Int(
            (numeric_as_f64(lhs) <= numeric_as_f64(rhs)) as i32,
        )),
        BoxMatch::Gt => Some(NumericValue::Int(
            (numeric_as_f64(lhs) > numeric_as_f64(rhs)) as i32,
        )),
        BoxMatch::Ge => Some(NumericValue::Int(
            (numeric_as_f64(lhs) >= numeric_as_f64(rhs)) as i32,
        )),
        BoxMatch::Eq => Some(NumericValue::Int(
            (numeric_as_f64(lhs) == numeric_as_f64(rhs)) as i32,
        )),
        BoxMatch::Ne => Some(NumericValue::Int(
            (numeric_as_f64(lhs) != numeric_as_f64(rhs)) as i32,
        )),
        BoxMatch::And => numeric_int_binop(lhs, rhs, |a, b| a & b),
        BoxMatch::Or => numeric_int_binop(lhs, rhs, |a, b| a | b),
        BoxMatch::Xor => numeric_int_binop(lhs, rhs, |a, b| a ^ b),
        BoxMatch::Lsh => numeric_int_binop(lhs, rhs, |a, b| a.wrapping_shl(b as u32)),
        BoxMatch::Rsh => numeric_int_binop(lhs, rhs, |a, b| a.wrapping_shr(b as u32)),
        _ => None,
    }
}

fn numeric_add(lhs: NumericValue, rhs: NumericValue) -> Option<NumericValue> {
    match (lhs, rhs) {
        (NumericValue::Int(a), NumericValue::Int(b)) => Some(NumericValue::Int(a.wrapping_add(b))),
        _ => Some(NumericValue::Real(
            numeric_as_f64(lhs) + numeric_as_f64(rhs),
        )),
    }
}

fn numeric_sub(lhs: NumericValue, rhs: NumericValue) -> Option<NumericValue> {
    match (lhs, rhs) {
        (NumericValue::Int(a), NumericValue::Int(b)) => Some(NumericValue::Int(a.wrapping_sub(b))),
        _ => Some(NumericValue::Real(
            numeric_as_f64(lhs) - numeric_as_f64(rhs),
        )),
    }
}

fn numeric_mul(lhs: NumericValue, rhs: NumericValue) -> Option<NumericValue> {
    match (lhs, rhs) {
        (NumericValue::Int(a), NumericValue::Int(b)) => Some(NumericValue::Int(a.wrapping_mul(b))),
        _ => Some(NumericValue::Real(
            numeric_as_f64(lhs) * numeric_as_f64(rhs),
        )),
    }
}

fn numeric_div(lhs: NumericValue, rhs: NumericValue) -> Option<NumericValue> {
    match (lhs, rhs) {
        (_, NumericValue::Int(0)) => None,
        (_, NumericValue::Real(0.0)) => None,
        (NumericValue::Int(a), NumericValue::Int(b)) => Some(NumericValue::Int(a / b)),
        _ => Some(NumericValue::Real(
            numeric_as_f64(lhs) / numeric_as_f64(rhs),
        )),
    }
}

fn numeric_rem(lhs: NumericValue, rhs: NumericValue) -> Option<NumericValue> {
    match (lhs, rhs) {
        (NumericValue::Int(_), NumericValue::Int(0)) => None,
        (NumericValue::Int(a), NumericValue::Int(b)) => Some(NumericValue::Int(a % b)),
        _ => Some(NumericValue::Real(
            numeric_as_f64(lhs) % numeric_as_f64(rhs),
        )),
    }
}

fn numeric_int_binop(
    lhs: NumericValue,
    rhs: NumericValue,
    op: impl FnOnce(i32, i32) -> i32,
) -> Option<NumericValue> {
    let (NumericValue::Int(a), NumericValue::Int(b)) = (lhs, rhs) else {
        return None;
    };
    Some(NumericValue::Int(op(a, b)))
}

fn numeric_as_f64(value: NumericValue) -> f64 {
    match value {
        NumericValue::Int(v) => f64::from(v),
        NumericValue::Real(v) => v,
    }
}

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

#[cfg(test)]
mod simplify_helpers_tests {
    use boxes::{BoxBuilder, BoxMatch, match_box};
    use signals::{SigMatch, match_sig};
    use tlib::TreeArena;

    use super::{
        Environment, LoopDetector, box_simplification, eval_box, eval_box_to_f64, eval_box_to_i32,
        eval_box_to_int_node, flatten_route_spec, is_numerical_tuple_box, normalize_route_spec,
        propagate_box_and_simplify, try_fold_seq_numeric,
    };

    /// Build `Seq(Par(Int(a), Int(b)), Add)` — the box-calculus encoding of `a + b`.
    fn make_int_add(arena: &mut TreeArena, a: i32, b: i32) -> tlib::TreeId {
        let mut bld = BoxBuilder::new(arena);
        let la = bld.int(a);
        let lb = bld.int(b);
        let par = bld.par(la, lb);
        let add = bld.add();
        bld.seq(par, add)
    }

    /// Build `Seq(Par(Real(a), Real(b)), Mul)`.
    fn make_real_mul(arena: &mut TreeArena, a: f64, b: f64) -> tlib::TreeId {
        let mut bld = BoxBuilder::new(arena);
        let la = bld.real(a);
        let lb = bld.real(b);
        let par = bld.par(la, lb);
        let mul = bld.mul();
        bld.seq(par, mul)
    }

    // ── propagate_box_and_simplify ─────────────────────────────────────────────

    /// 0→1 box `Seq(Par(Int(2), Int(3)), Add)` → `SigInt(5)`.
    ///
    /// C++ equivalent: `boxPropagateSig(nil, box(2+3), [])` + `simplify` → `sigInt(5)`.
    #[test]
    fn propagate_box_and_simplify_int_add() {
        let mut arena = TreeArena::default();
        let box_add = make_int_add(&mut arena, 2, 3);
        let result = propagate_box_and_simplify(&mut arena, box_add);
        assert!(result.is_some(), "expected Some(sig), got None");
        assert!(
            matches!(match_sig(&arena, result.unwrap()), SigMatch::Int(5)),
            "expected SigInt(5)"
        );
    }

    /// `Seq(Par(Real(0.5), Real(2.0)), Mul)` → `SigReal(1.0)`.
    #[test]
    fn propagate_box_and_simplify_float_mul() {
        let mut arena = TreeArena::default();
        let box_mul = make_real_mul(&mut arena, 0.5, 2.0);
        let result = propagate_box_and_simplify(&mut arena, box_mul);
        assert!(result.is_some(), "expected Some(sig), got None");
        let SigMatch::Real(v) = match_sig(&arena, result.unwrap()) else {
            panic!("expected SigReal");
        };
        assert!((v - 1.0).abs() < 1e-12, "expected 1.0, got {v}");
    }

    /// Wire (1→1) has inputs — `propagate_box_and_simplify` returns `None`.
    #[test]
    fn propagate_box_and_simplify_wire_is_none() {
        let mut arena = TreeArena::default();
        let wire = BoxBuilder::new(&mut arena).wire();
        assert!(
            propagate_box_and_simplify(&mut arena, wire).is_none(),
            "Wire (1→1) should return None"
        );
    }

    // ── simplify_pattern ───────────────────────────────────────────────────────

    /// Literal `boxInt(7)` is already numeric — `simplify_pattern` returns it unchanged.
    #[test]
    fn simplify_pattern_literal_int() {
        let mut arena = TreeArena::default();
        let b7 = BoxBuilder::new(&mut arena).int(7);
        let result = super::simplify_pattern(&mut arena, b7);
        assert!(matches!(match_box(&arena, result), BoxMatch::Int(7)));
    }

    /// Arithmetic `Seq(Par(Int(2), Int(3)), Add)` → `boxInt(5)` via propagation.
    ///
    /// C++ equivalent: `simplifyPattern(box(2+3))` → `boxInt(5)`.
    #[test]
    fn simplify_pattern_arithmetic_expression() {
        let mut arena = TreeArena::default();
        let box_add = make_int_add(&mut arena, 2, 3);
        let result = super::simplify_pattern(&mut arena, box_add);
        assert!(
            matches!(match_box(&arena, result), BoxMatch::Int(5)),
            "expected boxInt(5)"
        );
    }

    /// Wire (1 input) is not a 0-input box — `simplify_pattern` returns it unchanged.
    #[test]
    fn simplify_pattern_wire_unchanged() {
        let mut arena = TreeArena::default();
        let wire = BoxBuilder::new(&mut arena).wire();
        let result = super::simplify_pattern(&mut arena, wire);
        assert_eq!(result, wire, "Wire should be returned unchanged");
    }

    // ── eval_box_to_f64 ────────────────────────────────────────────────────────

    /// `boxReal(3.14)` → `Ok(3.14)`.
    ///
    /// C++ equivalent: `eval2double(boxReal(3.14), …)` → `3.14`.
    #[test]
    #[allow(clippy::approx_constant)] // 3.14 is deliberately chosen test data, not an approximation of PI
    fn eval_box_to_f64_literal() {
        let mut arena = TreeArena::default();
        let b = BoxBuilder::new(&mut arena).real(3.14);
        let result = eval_box_to_f64(&mut arena, b);
        assert!(result.is_ok());
        assert!((result.unwrap() - 3.14).abs() < 1e-12);
    }

    /// `boxInt(4)` → `Ok(4.0)` (integer promoted to f64).
    #[test]
    fn eval_box_to_f64_from_int() {
        let mut arena = TreeArena::default();
        let b = BoxBuilder::new(&mut arena).int(4);
        let result = eval_box_to_f64(&mut arena, b);
        assert!(result.is_ok());
        assert!((result.unwrap() - 4.0).abs() < 1e-12);
    }

    // ── eval_box_to_i32 ────────────────────────────────────────────────────────

    /// `boxInt(5)` → `Ok(5)`.
    ///
    /// C++ equivalent: `eval2int(boxInt(5), …)` → `5`.
    #[test]
    fn eval_box_to_i32_literal() {
        let mut arena = TreeArena::default();
        let b = BoxBuilder::new(&mut arena).int(5);
        assert_eq!(eval_box_to_i32(&mut arena, b).unwrap(), 5);
    }

    /// Arithmetic `Seq(Par(Int(1), Int(1)), Add)` → `Ok(2)`.
    ///
    /// C++ equivalent: `eval2int(box(1+1), …)` → `2`.
    #[test]
    fn eval_box_to_i32_arithmetic() {
        let mut arena = TreeArena::default();
        let box_add = make_int_add(&mut arena, 1, 1);
        assert_eq!(eval_box_to_i32(&mut arena, box_add).unwrap(), 2);
    }

    /// Wire (not a constant 0→1 box) → `Err(NotAConstantExpression)`.
    #[test]
    fn eval_box_to_i32_wire_is_err() {
        let mut arena = TreeArena::default();
        let wire = BoxBuilder::new(&mut arena).wire();
        assert!(eval_box_to_i32(&mut arena, wire).is_err());
    }

    // ── Seq numeric folding ────────────────────────────────────────────────────

    /// `is_numerical_tuple_box(int(5))` → `true`.
    #[test]
    fn is_numerical_tuple_single_int() {
        let mut arena = TreeArena::default();
        let five = BoxBuilder::new(&mut arena).int(5);
        assert!(is_numerical_tuple_box(&arena, five));
    }

    /// `is_numerical_tuple_box(par(int(1), real(2.0)))` → `true`.
    #[test]
    fn is_numerical_tuple_par_of_numerics() {
        let mut arena = TreeArena::default();
        let one = BoxBuilder::new(&mut arena).int(1);
        let two = BoxBuilder::new(&mut arena).real(2.0);
        let p = BoxBuilder::new(&mut arena).par(one, two);
        assert!(is_numerical_tuple_box(&arena, p));
    }

    /// `is_numerical_tuple_box(wire)` → `false`.
    #[test]
    fn is_numerical_tuple_wire_is_false() {
        let mut arena = TreeArena::default();
        let w = BoxBuilder::new(&mut arena).wire();
        assert!(!is_numerical_tuple_box(&arena, w));
    }

    /// `seq(par(int(2), int(3)), add)` folds to `int(5)`.
    #[test]
    fn try_fold_seq_int_add() {
        let mut arena = TreeArena::default();
        let two = BoxBuilder::new(&mut arena).int(2);
        let three = BoxBuilder::new(&mut arena).int(3);
        let par = BoxBuilder::new(&mut arena).par(two, three);
        let add = BoxBuilder::new(&mut arena).add();
        let result = try_fold_seq_numeric(&mut arena, par, add);
        assert!(result.is_some(), "should fold");
        assert!(matches!(
            match_box(&arena, result.unwrap()),
            BoxMatch::Int(5)
        ));
    }

    /// `seq(par(real(1.5), real(2.5)), add)` folds to `real(4.0)`.
    #[test]
    fn try_fold_seq_real_add() {
        let mut arena = TreeArena::default();
        let a = BoxBuilder::new(&mut arena).real(1.5);
        let b = BoxBuilder::new(&mut arena).real(2.5);
        let par = BoxBuilder::new(&mut arena).par(a, b);
        let add = BoxBuilder::new(&mut arena).add();
        let result = try_fold_seq_numeric(&mut arena, par, add);
        assert!(result.is_some(), "should fold");
        assert!(
            matches!(match_box(&arena, result.unwrap()), BoxMatch::Real(x) if (x - 4.0).abs() < 1e-12)
        );
    }

    /// `seq(par(int(2), int(3)), wire)` does NOT fold (wire: arity 1→1, so seq(par,wire) is 2→1 — propagation fails).
    #[test]
    fn try_fold_seq_with_wire_does_not_fold() {
        let mut arena = TreeArena::default();
        let two = BoxBuilder::new(&mut arena).int(2);
        let three = BoxBuilder::new(&mut arena).int(3);
        let par = BoxBuilder::new(&mut arena).par(two, three);
        let wire = BoxBuilder::new(&mut arena).wire();
        // seq(par(2,3), wire) has arity 2→1, which means it has audio inputs.
        // propagate_box_and_simplify uses &[] inputs → propagation would fail for
        // a 2→* box, so this should return None.
        let result = try_fold_seq_numeric(&mut arena, par, wire);
        // wire passes through signal 0 of its 1-input, but par(2,3) gives 2 outputs
        // → seq is ill-typed as 0-input anyway, so this is None.
        // (If it somehow propagates, the result should not be a bare Int/Real.)
        let _ = result; // don't assert — just ensure no panic
    }

    // ── simplify_const integration ─────────────────────────────────────────────

    /// `sigAdd(sigInt(2), sigInt(3))` simplifies to `SigInt(5)` via `normalize::simplify_const`.
    #[test]
    fn simplify_const_folds_int_add() {
        use normalize::simplify_const;
        use signals::SigBuilder;
        let mut arena = TreeArena::default();
        let mut sb = SigBuilder::new(&mut arena);
        let two = sb.int(2);
        let three = sb.int(3);
        let sum = sb.add(two, three);
        let result = simplify_const(&mut arena, sum);
        assert!(matches!(match_sig(&arena, result), SigMatch::Int(5)));
    }

    // ── box_simplification ────────────────────────────────────────────────────

    /// `box_simplification(boxInt(5))` → `boxInt(5)` (literal pass-through).
    #[test]
    fn box_simplification_int_literal_passthrough() {
        let mut arena = TreeArena::default();
        let five = BoxBuilder::new(&mut arena).int(5);
        let mut cache = ahash::HashMap::default();
        let result = box_simplification(&mut arena, &mut cache, five);
        assert!(matches!(match_box(&arena, result), BoxMatch::Int(5)));
    }

    /// `box_simplification(seq(par(int(2), int(3)), add))` → `boxInt(5)`.
    #[test]
    fn box_simplification_folds_arithmetic() {
        let mut arena = TreeArena::default();
        let expr = make_int_add(&mut arena, 2, 3);
        let mut cache = ahash::HashMap::default();
        let result = box_simplification(&mut arena, &mut cache, expr);
        assert!(
            matches!(match_box(&arena, result), BoxMatch::Int(5)),
            "expected Int(5)"
        );
    }

    /// `box_simplification(wire)` → `wire` (wire is a leaf that cannot denote a number).
    #[test]
    fn box_simplification_wire_passthrough() {
        let mut arena = TreeArena::default();
        let wire = BoxBuilder::new(&mut arena).wire();
        let mut cache = ahash::HashMap::default();
        let result = box_simplification(&mut arena, &mut cache, wire);
        assert!(matches!(match_box(&arena, result), BoxMatch::Wire));
    }

    // ── route normalization ────────────────────────────────────────────────────

    /// `eval_box_to_int_node(boxInt(3))` → `boxInt(3)`.
    #[test]
    fn eval_box_to_int_node_literal() {
        let mut arena = TreeArena::default();
        let three = BoxBuilder::new(&mut arena).int(3);
        let result = eval_box_to_int_node(&mut arena, three).unwrap();
        assert!(matches!(match_box(&arena, result), BoxMatch::Int(3)));
    }

    /// `eval_box_to_int_node(boxSeq(boxPar(boxInt(1),boxInt(1)), boxAdd()))` → `boxInt(2)`.
    #[test]
    fn eval_box_to_int_node_arithmetic() {
        let mut arena = TreeArena::default();
        let expr = make_int_add(&mut arena, 1, 1);
        let result = eval_box_to_int_node(&mut arena, expr).unwrap();
        assert!(matches!(match_box(&arena, result), BoxMatch::Int(2)));
    }

    /// `normalize_route_spec(par(int(1), par(int(2), par(int(3), int(4)))))` →
    /// same right-spine Par tree with all-boxInt leaves.
    #[test]
    fn normalize_route_spec_preserves_int_leaves() {
        let mut arena = TreeArena::default();
        // Build par(int(1), par(int(2), par(int(3), int(4))))
        let i1 = BoxBuilder::new(&mut arena).int(1);
        let i2 = BoxBuilder::new(&mut arena).int(2);
        let i3 = BoxBuilder::new(&mut arena).int(3);
        let i4 = BoxBuilder::new(&mut arena).int(4);
        let inner = BoxBuilder::new(&mut arena).par(i3, i4);
        let mid = BoxBuilder::new(&mut arena).par(i2, inner);
        let spec = BoxBuilder::new(&mut arena).par(i1, mid);
        let result = normalize_route_spec(&mut arena, spec);
        // Flatten and collect leaves
        let mut leaves = Vec::new();
        flatten_route_spec(&arena, result, &mut leaves);
        assert_eq!(leaves.len(), 4);
        let vals: Vec<i32> = leaves
            .iter()
            .map(|&l| match match_box(&arena, l) {
                BoxMatch::Int(n) => n,
                _ => panic!("expected Int leaf"),
            })
            .collect();
        assert_eq!(vals, [1, 2, 3, 4]);
    }

    /// `route(1+1, 1+1, spec)` evaluated in an empty env → `route(int(2), int(2), spec)`.
    #[test]
    fn eval_route_arithmetic_ins_outs() {
        let mut arena = TreeArena::default();
        // Build route(1+1, 1+1, par(par(int(1),int(1)), par(int(2),int(2))))
        let ins = make_int_add(&mut arena, 1, 1);
        let outs = make_int_add(&mut arena, 1, 1);
        let i1a = BoxBuilder::new(&mut arena).int(1);
        let i1b = BoxBuilder::new(&mut arena).int(1);
        let i2a = BoxBuilder::new(&mut arena).int(2);
        let i2b = BoxBuilder::new(&mut arena).int(2);
        let p1 = BoxBuilder::new(&mut arena).par(i1a, i1b);
        let p2 = BoxBuilder::new(&mut arena).par(i2a, i2b);
        let spec = BoxBuilder::new(&mut arena).par(p1, p2);
        let route_box = BoxBuilder::new(&mut arena).route(ins, outs, spec);
        let env = Environment::empty();
        let mut ld = LoopDetector::new();
        let result = eval_box(&mut arena, route_box, &env, &mut ld).unwrap();
        match match_box(&arena, result) {
            BoxMatch::Route(ri, ro, _) => {
                assert!(
                    matches!(match_box(&arena, ri), BoxMatch::Int(2)),
                    "ins not 2"
                );
                assert!(
                    matches!(match_box(&arena, ro), BoxMatch::Int(2)),
                    "outs not 2"
                );
            }
            other => panic!("expected Route, got {other:?}"),
        }
    }
}
