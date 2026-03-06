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
//! ## Rust model: imperative `Vec`-based scoped environment with direct bindings
//!
//! The Rust environment is an **imperative `Vec<(SymId, TreeId)>` with an optional parent
//! pointer**. Definitions are stored as **bare `TreeId`s** (32-bit handles into the `TreeArena`),
//! with no closure wrapping. Lexical scoping is implemented explicitly by constructing a child
//! scope (`push_scope()`) before evaluating any sub-expression that introduces new bindings, then
//! threading the child scope down through recursive calls.
//!
//! ```text
//! Rust env chain:
//!
//!   Environment { bindings: [("f", id_expr_f)], parent: Some(→) }
//!        │ parent
//!   Environment { bindings: [("x", id_expr_x)], parent: Some(→) }
//!        │ parent
//!   Environment { bindings: [("process", id_expr_p)], parent: None }
//! ```
//!
//! ## Why no closures are needed in Rust
//!
//! The C++ closure model is required because C++ evaluation is *demand-driven*: closures are
//! evaluated lazily when their symbol is first looked up. The closure must carry the environment
//! at definition time to ensure lexical scoping.
//!
//! In Rust, evaluation is *eager and explicit*: `eval_box` always receives the current `env`
//! parameter. When entering a `with` block, `push_scope()` creates a child scope containing the
//! new bindings, and that child scope is passed to all recursive calls. This statically guarantees
//! that every sub-expression sees exactly the bindings that were in scope at its definition site.
//!
//! **Semantic equivalence**: For any well-formed Faust program, `eval_cpp(P)` and `eval_rust(P)`
//! produce identical box trees, because:
//! 1. `push_scope()` is always called before introducing new bindings.
//! 2. Faust has no imperative mutation of environments — scopes are always additive.
//! 3. `with`/`letrec` bodies are evaluated in the scope that includes their own definitions
//!    (enabling mutual reference), both in C++ and in Rust.
//! 4. Pattern-matching bindings are collected in a barrier child scope, so repeated pattern
//!    variables only see bindings introduced by the current rule while normal RHS evaluation
//!    still sees the outer lexical environment.
//!
//! ## Divergences from C++ (intentional)
//!
//! | Feature | C++ | Rust | Notes |
//! |---|---|---|---|
//! | Value stored | `closure(expr, genv, visited, lenv)` | Bare `TreeId` | Not needed (eager eval) |
//! | Barrier mechanism | `pushEnvBarrier` / `searchIdDef` | `push_barrier_scope()` + `lookup_until_barrier()` | Same semantics |
//! | `copyEnvReplaceDefs` | Present (letrec env rewiring) | Not needed | Flat `push_scope` is equivalent |
//! | Redefinition check | `addLayerDef` throws on conflict | `bind_definitions` returns `EvalError::RedefinedSymbol` | Same semantics, typed error |
//! | Profiling | `gGlobal->gStats` (global mutable) | `EvalStats` (returned value) | Safer, composable |
//!
//! # Performance comparison — C++ vs Rust
//!
//! | Operation | C++ implementation | C++ cost | Rust implementation | Rust cost |
//! |---|---|---|---|---|
//! | **Scope push** | `tree(unique("ENV_LAYER"), lenv)` — alloc in hash-cons pool | O(1) amortized + hash | `Vec::new()` + `Box::new(parent.clone())` | O(parent_size) clone |
//! | **Bind one symbol** | `setProperty(node, id, def)` — hash map insert on tree node | O(1) amortized | `Vec::push((sym, id))` | O(1) amortized |
//! | **Lookup (found at depth d)** | Walk d layers, `getProperty` hash probe per layer | O(d) hash probes | Reverse `u32` scan per layer O(n_local), recurse O(d) | O(d × n_local) — O(1) per compare |
//! | **Value size per binding** | `Tree*` pointer to closure node (~64 bytes closure) | Large | `(u32, u32)` — 8 bytes (`SymId` + `TreeId`) | Excellent |
//! | **Cache locality** | Pointer-chased linked list through hash-cons pool | Poor (pointer indirection) | Contiguous `Vec<(u32,u32)>` — SIMD-scannable | Excellent (sequential) |
//! | **Concurrency** | Global `gGlobal` state, not thread-safe | N/A | Fully `Send`/`Sync`, no global state | Thread-safe |
//!
//! **In practice**: for typical Faust programs (< 200 top-level definitions, scope depth ≤ 5,
//! ≤ 30 bindings per scope), the Rust `Vec<(u32,u32)>` scan is **faster** than C++ hash-table
//! probes due to cache locality and SIMD-friendly layout. Each comparison is O(1) — two `u32`
//! integer compares — matching the cost of C++ hash-cons pointer equality.
//!
//! **Remaining Rust opportunity**: the `push_scope()` clone cost is the most significant overhead
//! for deep scope chains. Replacing `Option<Box<Environment>>` with an index into a scope arena
//! would eliminate all cloning, at the cost of a more complex API.
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

use std::fmt::{Display, Formatter};

use boxes::{BoxBuilder, BoxMatch, match_box};
use errors::codes;
use errors::{Diagnostic, IntoDiagnostic, Severity, Stage};
use tlib::{NodeKind, TreeArena, TreeId};

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
/// | Values stored | **Closures**: `closure(expr, genv, visited, lenv)` capturing the scope at definition time | **Bare `TreeId`** — no scope capture needed |
/// | Lookup | `searchIdDef`: walks layers calling `getProperty` (hash probe per layer) | `lookup`: reverse linear scan of `Vec`, then recurse to `parent` |
/// | Scope push | `pushNewLayer(lenv)` — allocates a unique tree node | `push_scope()` — `Vec::new()` + `Box::new(parent.clone())` |
/// | Redefinition | `addLayerDef` throws `faustexception` on conflicting rebind | `bind_definitions` returns `EvalError::RedefinedSymbol` |
/// | Barrier | `pushEnvBarrier` / `isEnvBarrier` — stops pattern-matcher lookup | `push_barrier_scope()` / `lookup_until_barrier()` |
/// | Env copy/rewire | `copyEnvReplaceDefs` + `updateClosures` — for letrec env fixup | Not needed — `push_scope` + flat binding is equivalent |
/// | Profiling | `gGlobal->gStats.fEnvLayersPushed/fEnvLookups/fEnvLookupTotalDepth` | [`EvalStats`] returned from [`eval_process_with_stats`] |
///
/// # Performance
///
/// For typical Faust programs (scope depth ≤ 5, ≤ 30 bindings/scope):
/// - **Lookup**: O(d × n) where d = depth, n = bindings/scope. Each compare is O(1) — `u32`
///   integer equality. In practice ~30–150 comparisons — fits entirely in L1 cache.
/// - **Bind**: `Vec::push` — amortized O(1), no hashing, no pointer chasing.
/// - **Push scope**: O(parent_size) due to `clone`. This is the dominant cost for deeply
///   nested scopes. A scope-arena design would eliminate it entirely.
/// - **Memory per binding**: **8 bytes** (`u32` SymId + `u32` TreeId) vs C++'s ~64 bytes per
///   closure node. SIMD-scannable `Vec<(u32, u32)>` layout — 4 bindings per 32-byte cache line.
#[derive(Clone, Debug, Default)]
pub struct Environment {
    bindings: Vec<(SymId, TreeId)>,
    parent: Option<Box<Environment>>,
    barrier: bool,
}

impl Environment {
    /// Creates an empty root environment with no bindings and no parent.
    ///
    /// **C++ equivalent**: the initial `gGlobal->nil` environment passed to the first
    /// `pushMultiClosureDefs` call in `eval.cpp`.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            bindings: Vec::new(),
            parent: None,
            barrier: false,
        }
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
        self.bindings.push((sym, value));
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
        for (s, value) in self.bindings.iter().rev() {
            if *s == sym {
                return Some(*value);
            }
        }
        self.parent.as_ref().and_then(|p| p.lookup(sym))
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
        for (s, value) in self.bindings.iter().rev() {
            if *s == sym {
                return Some(*value);
            }
        }
        if self.barrier {
            return None;
        }
        self.parent
            .as_ref()
            .and_then(|p| p.lookup_until_barrier(sym))
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
        for (s, value) in self.bindings.iter().rev() {
            if *s == sym {
                return Some(*value);
            }
        }
        None
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
    /// **Cost**: `O(parent_size)` due to cloning the parent environment. For deeply nested
    /// scopes this is the dominant allocation cost. A scope-arena design (indexing into a
    /// flat pool instead of boxing) would make this O(1).
    #[must_use]
    pub fn push_scope(&self) -> Self {
        Self {
            bindings: Vec::new(),
            parent: Some(Box::new(self.clone())),
            barrier: false,
        }
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
        Self {
            bindings: Vec::new(),
            parent: Some(Box::new(self.clone())),
            barrier: true,
        }
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
        let mut out = self
            .bindings
            .iter()
            .filter_map(|(sym, _)| arena.symbol_name(*sym).map(str::to_owned))
            .collect::<Vec<_>>();
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
        let mut out = self.local_names(arena);
        if let Some(parent) = &self.parent {
            out.extend(parent.visible_names(arena));
        }
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
        match &self.parent {
            Some(parent) => parent.top_level_names(arena),
            None => self.local_names(arena),
        }
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
/// `Vec<u32>` is **faster** than the C++ O(log n) set probe due to SIMD-friendly contiguous
/// memory layout. The `set` approach would be preferable only for depths > 1000.
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
    call_stack: Vec<TreeId>,
    max_depth: usize,
    /// Compiled automata keyed by the `TreeId` of the evaluated `Case` rule-list.
    automaton_cache: pattern_matcher::AutomatonCache,
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
            automaton_cache: pattern_matcher::AutomatonCache::default(),
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
            automaton_cache: pattern_matcher::AutomatonCache::default(),
            next_slot_id: 0,
        }
    }

    fn enter(&mut self, id: TreeId) -> Result<(), EvalError> {
        if self.call_stack.contains(&id) {
            return Err(EvalError::LoopDetected { node: id });
        }
        if self.call_stack.len() >= self.max_depth {
            return Err(EvalError::RecursionDepthExceeded {
                max_depth: self.max_depth,
            });
        }
        self.call_stack.push(id);
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
/// - **`env_lookups / nodes_evaluated`**: average lookups per evaluated node. High values (> 3)
///   indicate deeply bound symbols that might benefit from flattening or interning.
/// - **`env_lookup_total_depth / env_lookups`**: average scope depth traversed per lookup.
///   Values > 3 indicate deep scope chains where caching or scope-arena would help.
/// - **`env_layers_pushed / nodes_evaluated`**: scope-push frequency. High values for iterative
///   forms (`ipar`/`iseq`) are expected and can be reduced with a scope-arena.
#[derive(Clone, Debug, Default)]
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
pub enum EvalError {
    MissingProcessDefinition {
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
    InvalidModulationCircuit {
        node: TreeId,
        reason: &'static str,
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
}

impl Display for EvalError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingProcessDefinition { .. } => write!(f, "missing `process` definition"),
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
            Self::InvalidModulationCircuit { reason, .. } => {
                write!(f, "invalid modulation circuit: {reason}")
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
            Self::MissingProcessDefinition { available_defs, .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_MISSING_PROCESS,
                message,
            )
            .with_note("cause: required top-level `process` definition is missing")
            .with_note("entrypoint contract: one top-level `process = ...;` definition is required")
            .with_note(format!(
                "available top-level definitions: {}",
                if available_defs.is_empty() {
                    "<none>".to_owned()
                } else {
                    available_defs.join(", ")
                }
            ))
            .with_help("define `process = ...;` in the top-level definitions")
            .with_help("template: process = _;"),
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
            Self::InvalidModulationCircuit { reason, .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_GENERIC_FAILURE,
                message,
            )
            .with_note("cause: modulation circuit violates Faust box-arity constraints")
            .with_note(format!("computed: {reason}"))
            .with_help("use a modulation circuit with at most 2 inputs and exactly 1 output"),
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
    let mut env = Environment::empty();
    let mut stats = EvalStats::default();
    bind_definitions(arena, definitions, &mut env)?;
    stats.env_layers_pushed += 1; // root scope
    let available_defs = top_level_definition_names(arena, definitions)?;
    // Use get_symbol (no alloc, &self) — if "process" was never interned it was never bound.
    let process = arena
        .get_symbol("process")
        .and_then(|sym| env.lookup(sym))
        .ok_or(EvalError::MissingProcessDefinition {
            definitions,
            available_defs,
        })?;
    stats.env_lookups += 1;
    let mut loop_detector = LoopDetector::new();
    let result = eval_box(arena, process, &env, &mut loop_detector)?;
    let result = a2sb(arena, result, &mut loop_detector)?;
    stats.loop_detector_max_depth = loop_detector.call_stack.len();
    Ok((result, stats))
}

/// Lowers residual abstractions and case closures into symbolic boxes.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `a2sb`
/// - `real_a2sb`
///
/// The C++ evaluator applies `a2sb(eval(...))` before the propagation phase so
/// `propagate` never receives raw closures or pattern matchers. Rust does not
/// materialize explicit closure nodes, so this helper operates directly on the
/// residual evaluated `BoxMatch::Abstr` and `BoxMatch::Case` shapes:
///
/// - `abstr(x, body)` becomes `symbolic(slot, lowered(body[x := slot]))`
/// - `case { ... }` becomes one nested `symbolic(slot_i, ...)` per expected
///   argument, after fully applying the case node to fresh slots
///
/// This is an **adapted** internal representation, not a byte-for-byte port of
/// C++ closure objects. The semantic contract is the same: later passes observe
/// only first-order symbolic boxes, never unapplied evaluator-only forms.
fn a2sb(
    arena: &mut TreeArena,
    expr: TreeId,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    match match_box(arena, expr) {
        BoxMatch::Abstr(_, _) => lower_abstraction_to_symbolic(arena, expr, loop_detector),
        BoxMatch::Case(_) => lower_case_to_symbolic(arena, expr, loop_detector),
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

/// Adapts one residual `abstr` closure into a first-order symbolic box.
///
/// The abstraction is applied to a fresh `boxSlot` using the same evaluator
/// machinery as C++ `a2sb`, then the resulting body is recursively lowered.
fn lower_abstraction_to_symbolic(
    arena: &mut TreeArena,
    abstraction: TreeId,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let slot = fresh_slot(arena, loop_detector);
    let args = vec_to_list(arena, &[slot]);
    let applied = apply_list(
        arena,
        abstraction,
        args,
        &Environment::empty(),
        loop_detector,
        Some(abstraction),
    )?;
    let lowered_body = a2sb(arena, applied, loop_detector)?;
    let mut b = BoxBuilder::new(arena);
    Ok(b.symbolic(slot, lowered_body))
}

/// Adapts one residual `case` node into nested symbolic boxes.
///
/// C++ lowers pattern matchers by applying them to fresh slots one argument at a
/// time. Rust keeps raw `case` nodes until this point, so the adapted lowering
/// applies the case to **all** required slots in one step, then wraps the
/// resulting body in nested `symbolic` nodes. This preserves the same external
/// meaning while avoiding the under-application placeholder path of
/// [`apply_list`].
fn lower_case_to_symbolic(
    arena: &mut TreeArena,
    case_expr: TreeId,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let BoxMatch::Case(rules) = match_box(arena, case_expr) else {
        return Ok(case_expr);
    };
    let arity = case_expected_arity(arena, rules)?;
    let slots: Vec<_> = (0..arity)
        .map(|_| fresh_slot(arena, loop_detector))
        .collect();
    let slot_args = vec_to_list(arena, &slots);
    let applied = apply_list(
        arena,
        case_expr,
        slot_args,
        &Environment::empty(),
        loop_detector,
        Some(case_expr),
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

/// Evaluates one box expression in the provided lexical environment.
///
/// This is the core recursive evaluator. It dispatches on the `BoxMatch` of `expr` and:
/// - **Resolves identifiers** (`Ident`) by lookup in `env` and recursive evaluation.
/// - **Beta-reduces applications** (`Appl`) by evaluating function and arguments, then calling
///   `apply_list`.
/// - **Evaluates `with`/`letrec` scopes** (`WithLocalDef`, `WithRecDef`) by pushing a child
///   scope, binding the local definitions, and evaluating the body in the new scope.
/// - **Evaluates abstractions** (`Abstr`) by binding the parameter in a child scope and
///   evaluating the body — effectively normalizing the abstraction body at eval time.
/// - **Expands iterative forms** (`IPar`, `ISeq`, `ISum`, `IProd`) into equivalent box
///   compositions, fully unrolled for the given count.
/// - **Maps over children** for all other nodes (`Unknown`, structural primitives) without
///   reducing the node itself.
///
/// # C++ correspondence
///
/// Corresponds to `eval(Tree exp, int numInputs, int numOutputs)` in `eval.cpp`. The Rust
/// version does not carry `numInputs`/`numOutputs` because arity inference is done lazily in
/// `apply_list` via `infer_box_arity` — the C++ counterpart threads arity through the
/// evaluator for immediate wire-insertion decisions.
///
/// The key structural difference: C++ `eval` calls `evalClosure(exp, …)` when it encounters a
/// closure, which evaluates `expr` in the **captured** environment. Rust `eval_box` on `Ident`
/// looks up the bare `TreeId` and evaluates it in the **current** `env` — semantically equivalent
/// because `push_scope()` always establishes the correct lexical scope before any binding.
pub fn eval_box(
    arena: &mut TreeArena,
    expr: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    match match_box(arena, expr) {
        BoxMatch::Unknown => map_children(arena, expr, env, loop_detector),
        BoxMatch::Ident(name) => {
            // get_symbol takes &self — safe to call while `name: &str` borrows `arena`.
            // If the name was never interned (never bound), it cannot be in the env.
            let value = arena
                .get_symbol(name)
                .and_then(|sym| env.lookup(sym))
                .ok_or_else(|| EvalError::UndefinedSymbol {
                    symbol: name.to_owned(),
                    node: expr,
                    local_scope: env.local_names(arena),
                    visible_scope: env.visible_names(arena),
                    top_level_scope: env.top_level_names(arena),
                })?;
            if value == expr {
                // Shadowing sentinel used for lambda parameters in lexical scopes.
                return Ok(expr);
            }
            loop_detector.enter(value)?;
            let out = eval_box(arena, value, env, loop_detector);
            loop_detector.leave();
            out
        }
        BoxMatch::Appl(fun, arg) => {
            let efun = eval_box(arena, fun, env, loop_detector)?;
            let rev_args = rev_eval_list(arena, arg, env, loop_detector)?;
            apply_list(arena, efun, rev_args, env, loop_detector, Some(fun))
        }
        BoxMatch::Access(body, field) => eval_access(arena, body, field, env, loop_detector),
        BoxMatch::Case(_) => Ok(expr),
        BoxMatch::PatternVar(_) => Ok(expr),
        BoxMatch::WithLocalDef(body, defs) => {
            let mut scoped = env.push_scope();
            bind_definitions(arena, defs, &mut scoped)?;
            eval_box(arena, body, &scoped, loop_detector)
        }
        BoxMatch::WithRecDef(body, rec_defs, where_defs) => {
            let mut scoped = env.push_scope();
            bind_definitions(arena, rec_defs, &mut scoped)?;
            bind_definitions(arena, where_defs, &mut scoped)?;
            eval_box(arena, body, &scoped, loop_detector)
        }
        BoxMatch::Abstr(arg, body) => {
            let mut scoped = env.push_scope();
            let name = ident_name(arena, arg)?;
            // intern_symbol is safe here: `name` is an owned String, not borrowed from arena.
            let sym = arena.intern_symbol(&name);
            // Parameter shadows outer binding in body capture.
            scoped.bind(sym, arg);
            let evaluated_body = eval_box(arena, body, &scoped, loop_detector)?;
            let mut b = BoxBuilder::new(arena);
            Ok(b.abstr(arg, evaluated_body))
        }
        BoxMatch::Modulation(var, body) => {
            eval_modulation(arena, expr, var, body, env, loop_detector)
        }
        BoxMatch::IPar(index, count, body) => {
            iterate_par(arena, index, count, body, env, loop_detector)
        }
        BoxMatch::ISeq(index, count, body) => {
            iterate_seq(arena, index, count, body, env, loop_detector)
        }
        BoxMatch::ISum(index, count, body) => {
            iterate_sum(arena, index, count, body, env, loop_detector)
        }
        BoxMatch::IProd(index, count, body) => {
            iterate_prod(arena, index, count, body, env, loop_detector)
        }
        _ => map_children(arena, expr, env, loop_detector),
    }
}

/// Evaluates `expr.ident` access with Faust environment semantics.
///
/// When `expr` evaluates to an environment-like form, `ident` is resolved inside
/// this scoped environment. Otherwise the access node is reconstructed on evaluated
/// children.
fn eval_access(
    arena: &mut TreeArena,
    body: TreeId,
    field: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    match match_box(arena, body) {
        BoxMatch::WithLocalDef(inner, defs) => {
            let mut scoped = env.push_scope();
            bind_definitions(arena, defs, &mut scoped)?;
            let inner_eval = eval_box(arena, inner, &scoped, loop_detector)?;
            if matches!(match_box(arena, inner_eval), BoxMatch::Environment) {
                return eval_box(arena, field, &scoped, loop_detector);
            }
        }
        BoxMatch::WithRecDef(inner, rec_defs, where_defs) => {
            let mut scoped = env.push_scope();
            bind_definitions(arena, rec_defs, &mut scoped)?;
            bind_definitions(arena, where_defs, &mut scoped)?;
            let inner_eval = eval_box(arena, inner, &scoped, loop_detector)?;
            if matches!(match_box(arena, inner_eval), BoxMatch::Environment) {
                return eval_box(arena, field, &scoped, loop_detector);
            }
        }
        _ => {}
    }

    let eval_body = eval_box(arena, body, env, loop_detector)?;
    if matches!(match_box(arena, eval_body), BoxMatch::Environment) {
        return eval_box(arena, field, env, loop_detector);
    }
    let eval_field = eval_box(arena, field, env, loop_detector)?;
    let mut b = BoxBuilder::new(arena);
    Ok(b.access(eval_body, eval_field))
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
/// C++ supports richer `%` substitutions via `evalLabel(...)`. The Rust port
/// currently implements the literal-label subset required by the active corpus.
fn eval_modulation_label(
    arena: &mut TreeArena,
    var: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<String, EvalError> {
    let label_node = arena
        .hd(var)
        .ok_or(EvalError::MalformedListNode { node: var })?;
    let evaluated = eval_box(arena, label_node, env, loop_detector)?;
    let Some(label) = label_node_text(arena, evaluated) else {
        return Err(EvalError::InvalidModulationLabel { node: evaluated });
    };
    Ok(strip_label_metadata(label).to_owned())
}

/// Evaluates the optional modulation circuit, defaulting to multiplication.
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

fn strip_label_node(arena: &TreeArena, label: TreeId) -> String {
    label_node_text(arena, label)
        .map(strip_label_metadata)
        .unwrap_or_default()
        .to_owned()
}

fn strip_label_metadata(label: &str) -> &str {
    label
        .split_once('[')
        .map_or(label, |(prefix, _)| prefix)
        .trim()
}

fn label_node_text(arena: &TreeArena, label: TreeId) -> Option<&str> {
    match arena.kind(label) {
        Some(NodeKind::StringLiteral(label)) => Some(label.as_ref()),
        Some(NodeKind::Symbol(label)) => Some(label.as_ref()),
        _ => None,
    }
}

fn is_subsequence(needle: &[String], haystack: &[String]) -> bool {
    let mut haystack_iter = haystack.iter();
    needle
        .iter()
        .all(|target| haystack_iter.by_ref().any(|candidate| candidate == target))
}

/// Structural fallback: evaluate all children, then rebuild the node unchanged in kind.
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
        children.push(eval_box(arena, *child, env, loop_detector)?);
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
/// - If the same name is already bound in the **current scope** with the **same** `TreeId`,
///   the new definition is silently skipped (structural identity via hash-consed `TreeId`).
/// - If the same name is bound with a **different** `TreeId`, `EvalError::RedefinedSymbol`
///   is returned.
/// - If the name is not yet in the current scope (including the case where it only exists
///   in a parent scope — shadowing), the binding proceeds normally.
///
/// # C++ correspondence
///
/// | C++ call site | Rust equivalent |
/// |---|---|
/// | `pushMultiClosureDefs(ldefs, visited, lenv)` | `bind_definitions(arena, defs, &mut scoped)` |
/// | `pushValueDef(id, def, lenv)` | `env.bind(name, value)` (single-binding fast path) |
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
        // C++ parity: addLayerDef checks for conflicting redefinition within the current layer.
        // Identical bindings (same TreeId = same hash-consed expression) are silently accepted.
        // Conflicting bindings (different TreeId) are an error.
        // Parent-scope shadowing is allowed and is NOT checked here.
        if let Some(existing) = env.lookup_local(sym) {
            if existing != bound {
                return Err(EvalError::RedefinedSymbol {
                    symbol: name,
                    first_def: existing,
                    second_def: bound,
                });
            }
            // existing == bound: identical redefinition — silently skip (C++ parity)
        } else {
            env.bind(sym, bound);
        }
        defs = arena
            .tl(defs)
            .ok_or(EvalError::MalformedDefinitionNode { node: defs })?;
    }
    Ok(())
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
            apply_case_rules(arena, rules, larg, env, loop_detector, call_site)
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

/// Computes total output arity for a list of argument boxes.
fn list_outputs(arena: &TreeArena, mut list: TreeId) -> Option<usize> {
    let mut total = 0usize;
    while !arena.is_nil(list) {
        let head = arena.hd(list)?;
        let (_, outs) = infer_box_arity(arena, head)?;
        total = total.checked_add(outs)?;
        list = arena.tl(list)?;
    }
    Some(total)
}

/// Local arity inference used by non-closure application lowering.
///
/// Returns `(inputs, outputs)` for the subset needed in `apply_list`.
/// `None` means arity is unknown or invalid for this fast-path inference.
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
        _ => Err(EvalError::IterationCountNotInt { node: count }),
    }
}

/// Evaluates iterative body with one bound loop index (`i`).
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
fn list_to_vec(arena: &TreeArena, mut list: TreeId) -> Result<Vec<TreeId>, EvalError> {
    let mut out = Vec::new();
    while !arena.is_nil(list) {
        let head = arena
            .hd(list)
            .ok_or(EvalError::MalformedListNode { node: list })?;
        out.push(head);
        list = arena
            .tl(list)
            .ok_or(EvalError::MalformedListNode { node: list })?;
    }
    Ok(out)
}

/// Converts a vector into a parser-style list preserving order.
fn vec_to_list(arena: &mut TreeArena, items: &[TreeId]) -> TreeId {
    let mut out = arena.nil();
    for id in items.iter().rev() {
        out = arena.cons(*id, out);
    }
    out
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

/// Applies case rules to a given argument list using the compiled tree automaton.
///
/// # Algorithm
///
/// 1. Evaluate and simplify all rule LHS patterns in the current environment.
/// 2. Determine expected arity from the first evaluated rule's LHS pattern count.
/// 3. Compile all evaluated rules into a [`pattern_matcher::Automaton`] via
///    [`pattern_matcher::make_pattern_matcher`] (Graef incremental algorithm).
/// 4. Initialise one barrier child [`Environment`] per rule.
/// 5. Feed each consumed argument through the automaton one at a time via
///    [`pattern_matcher::apply_pattern_matcher`], which advances the state machine
///    and accumulates variable bindings into per-rule environments.
/// 6. In the final state, pick the first rule whose environment survived all
///    nonlinearity checks, evaluate its RHS, and apply any remaining arguments.
///
/// # C++ correspondence
///
/// `evalCase()` in `compiler/evaluate/eval.cpp`.
fn apply_case_rules(
    arena: &mut TreeArena,
    rules_rev: TreeId,
    larg: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
    call_site: Option<TreeId>,
) -> Result<TreeId, EvalError> {
    let args = list_to_vec(arena, larg)?;
    let evaluated_rules = eval_rule_list(arena, rules_rev, env, loop_detector)?;

    // Determine expected arity from the first rule's LHS.
    let mut probe = list_to_vec(arena, evaluated_rules)?;
    probe.reverse();
    let Some(first_rule) = probe.first().copied() else {
        return Err(EvalError::MalformedCaseNode { node: rules_rev });
    };
    let (first_lhs, _) = rule_parts(arena, first_rule)?;
    let expected = list_to_vec(arena, first_lhs)?.len();
    if args.len() < expected {
        return Err(EvalError::PatternArityMismatch {
            node: rules_rev,
            expected,
            got: args.len(),
        });
    }
    let consumed = args[..expected].to_vec();
    let rest = args[expected..].to_vec();

    // Compile evaluated rules into a tree automaton — or retrieve a cached copy.
    if !loop_detector.automaton_cache.contains_key(&evaluated_rules) {
        let automaton = pattern_matcher::make_pattern_matcher(arena, evaluated_rules);
        loop_detector
            .automaton_cache
            .insert(evaluated_rules, automaton);
    }
    let automaton = loop_detector.automaton_cache.get(&evaluated_rules).unwrap();
    let n = automaton.n_rules();

    // Per-rule environments: each starts as a barrier child scope of the current env.
    let mut envs: Vec<Option<Environment>> =
        (0..n).map(|_| Some(env.push_barrier_scope())).collect();

    // Feed each consumed argument through the automaton state machine, one at a time.
    // C++ parallel: `for each arg: s = apply_pattern_matcher(A, s, arg, C, E)`.
    let mut state: usize = 0;
    for arg in &consumed {
        let (new_state, _) =
            pattern_matcher::apply_pattern_matcher(arena, automaton, state, *arg, &mut envs);
        let Some(new_state) = new_state else {
            return Err(EvalError::PatternMatchFailed { node: rules_rev });
        };
        state = new_state;
    }

    // Final state: pick the first rule whose environment survived nonlinearity checks.
    if !automaton.final_state(state) {
        return Err(EvalError::PatternMatchFailed { node: rules_rev });
    }
    for rule_marker in &automaton.states[state].rules {
        if let Some(rule_env) = envs[rule_marker.r].take() {
            let rhs = automaton.rhs[rule_marker.r];
            let result = eval_box(arena, rhs, &rule_env, loop_detector)?;
            if rest.is_empty() {
                return Ok(result);
            }
            let rest_list = vec_to_list(arena, &rest);
            return apply_list(arena, result, rest_list, env, loop_detector, call_site);
        }
    }

    Err(EvalError::PatternMatchFailed { node: rules_rev })
}

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
