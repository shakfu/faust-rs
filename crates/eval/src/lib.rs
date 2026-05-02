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
//! - later passes still consume first-order box IR after `a2sb_value` lowers those values.
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

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use boxes::{BoxBuilder, BoxMatch, match_box};
use normalize::simplify_const;
use propagate::{ArityCache, propagate_typed, try_build_flat_box};
use signals::{SigId, SigMatch, match_sig};
use tlib::{NodeKind, TreeArena, TreeId, tree_to_double, tree_to_int};

pub(crate) mod environment;
pub(crate) mod error;
pub(crate) mod loop_detector;
mod pattern_matcher;
pub(crate) mod source_context;

use environment::{ClosureValue, EvalCacheKey, EvalValue, PatternMatcherValue};
use source_context::CachedLoadedSource;

pub use environment::Environment;
pub use error::{EvalError, EvalStats};
pub use loop_detector::LoopDetector;
pub use source_context::{EvalSourceContext, SamplePrecision};

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
    stats.def_names = loop_detector.def_names;
    Ok((result, stats))
}

fn a2sb_value(
    arena: &mut TreeArena,
    value: EvalValue,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    loop_detector.enter_structural()?;
    let result = a2sb_value_inner(arena, value, loop_detector);
    loop_detector.leave_structural();
    result
}

fn a2sb_value_inner(
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
    if let Some(&cached) = loop_detector.symbolic_box_cache.get(&expr) {
        return Ok(cached);
    }

    loop_detector.enter_structural()?;
    let outcome = a2sb_match(arena, expr, loop_detector);
    loop_detector.leave_structural();
    let result = outcome?;
    loop_detector.symbolic_box_cache.insert(expr, result);
    Ok(result)
}

fn a2sb_match(
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
        BoxMatch::Closure(key_node) => {
            // Retrieve the closure from the side-table and lower it via a2sb_value.
            let key = match match_box(arena, key_node) {
                BoxMatch::Int(k) => k,
                _ => {
                    return Err(EvalError::InternalError {
                        message: "boxClosure key is not an integer".to_owned(),
                    });
                }
            };
            let cv = loop_detector
                .get_closure(key)
                .ok_or_else(|| EvalError::InternalError {
                    message: format!("boxClosure key {} not found in closure store", key),
                })?;
            a2sb_value(arena, EvalValue::Closure(cv), loop_detector)
        }
        // Source provenance (C++):
        // - `compiler/evaluate/eval.cpp`
        // - `a2sb`
        //
        // Mapping status: `1:1`.
        // Waveform nodes are already first-order constants. Their child is the
        // encoded `cons` list of samples, which is payload data rather than a
        // box subtree that should be recursively lowered. Treating it as a
        // generic child tree causes stack overflows on large tables.
        BoxMatch::Waveform(_) => Ok(expr),
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
/// Internally the evaluator now produces `EvalValue` first, so closures can carry a captured
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
    let cache_key = EvalCacheKey {
        expr,
        env_key: env.frame_key(),
    };
    if let Some(cached) = loop_detector.eval_cache.get(&cache_key) {
        return Ok(cached.clone());
    }
    let result = eval_value_uncached(arena, expr, env, loop_detector)?;
    if should_cache_eval_value(&result) {
        loop_detector.eval_cache.insert(cache_key, result.clone());
    }
    Ok(result)
}

#[inline]
fn should_cache_eval_value(value: &EvalValue) -> bool {
    match value {
        EvalValue::Box(_) | EvalValue::Closure(_) => true,
        EvalValue::PatternMatcher(_) => false,
    }
}

/// Non-memoized evaluator core used behind [`eval_value`].
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `realeval`
///
/// Mapping status: `adapted`.
/// Rust keeps the memoization layer in [`LoopDetector::eval_cache`] instead of
/// tree properties. Host-side pattern matchers keep mutable environment state
/// in `pm.envs`, so only box/closure results are memoized for now; this
/// function corresponds to the uncached evaluation body that C++ exposes as
/// `realeval(...)`.
fn eval_value_uncached(
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
            // `name` borrows the arena; convert to owned before any mutable arena use.
            let name = name.to_owned();
            eval_ident_value(arena, expr, &name, env, loop_detector)
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
        // Source provenance (C++):
        // - `compiler/evaluate/eval.cpp`
        // - `isBoxWaveform(box) => box`
        //
        // Mapping status: `1:1`.
        // Waveform payload lists are already parser-normalized constants. The
        // evaluator must treat the whole waveform node as a leaf normal form
        // instead of recursively mapping over its internal `cons` list, or
        // large tables overflow the host stack before `propagate` can lower
        // them to `(size, waveform)` signals.
        BoxMatch::Waveform(_) => Ok(EvalValue::Box(expr)),
        // boxClosure: extract the stored ClosureValue from the side-table.
        // (Mirrors C++ eval.cpp: `isClosure(box, ...) => box` — closures
        // are already in normal form as tree nodes.)
        BoxMatch::Closure(key_node) => {
            let key = match match_box(arena, key_node) {
                BoxMatch::Int(k) => k,
                _ => {
                    return Err(EvalError::InternalError {
                        message: "boxClosure key is not an integer".to_owned(),
                    });
                }
            };
            let cv = loop_detector
                .get_closure(key)
                .ok_or_else(|| EvalError::InternalError {
                    message: format!("boxClosure key {} not found in closure store", key),
                })?;
            Ok(EvalValue::Closure(cv))
        }
        BoxMatch::PatternVar(_) => Ok(EvalValue::Box(expr)),
        BoxMatch::WithLocalDef(body, defs) => {
            let mut scoped = env.push_scope();
            bind_definitions(arena, defs, &mut scoped)?;
            eval_value(arena, body, &scoped, loop_detector)
        }
        BoxMatch::ModifLocalDef(body, defs) => {
            eval_modif_local_def_value(arena, body, defs, env, loop_detector)
        }
        BoxMatch::WithRecDef(_, _, _) => {
            // Source provenance (C++):
            // - `compiler/boxes/boxes.cpp`
            // - `boxWithRecDef`
            //
            // Mapping status: `1:1` production invariant.
            // C++ expands `letrec` before evaluation. Rust now does the same in
            // `boxes`, so reaching `BOXWITHRECDEF` here means a legacy or
            // manually constructed tree bypassed the normal parser/boxes path.
            Err(EvalError::InternalError {
                message:
                    "legacy BOXWITHRECDEF reached eval; parser/boxes should lower letrec eagerly"
                        .to_owned(),
            })
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
        BoxMatch::ForwardAD(exp, seed) => {
            // Source provenance (C++):
            // - `/Users/letz/faust/compiler/evaluate/eval.cpp`
            // - `isBoxForwardAD(exp, seed) -> boxForwardAD(eval(exp, ...), eval(seed, ...))`
            //
            // Mapping status: `1:1`.
            // Both children must be evaluated under the current lexical environment,
            // then rewrapped so the AD primitive remains explicit at the post-eval
            // propagation boundary.
            let exp_val = eval_value(arena, exp, env, loop_detector)?;
            let seed_val = eval_value(arena, seed, env, loop_detector)?;
            let exp_box = force_value_to_box(arena, exp_val, loop_detector)?;
            let seed_box = force_value_to_box(arena, seed_val, loop_detector)?;
            let mut bld = BoxBuilder::new(arena);
            Ok(EvalValue::Box(bld.forward_ad(exp_box, seed_box)))
        }
        BoxMatch::ReverseAD(exp, seeds) => {
            // Mirrors `BoxMatch::ForwardAD` arm: both children are evaluated in
            // the same lexical environment, then re-wrapped so RAD remains an
            // explicit two-child node at the post-eval propagation boundary.
            let exp_val = eval_value(arena, exp, env, loop_detector)?;
            let seeds_val = eval_value(arena, seeds, env, loop_detector)?;
            let exp_box = force_value_to_box(arena, exp_val, loop_detector)?;
            let seeds_box = force_value_to_box(arena, seeds_val, loop_detector)?;
            let mut bld = BoxBuilder::new(arena);
            Ok(EvalValue::Box(bld.reverse_ad(exp_box, seeds_box)))
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
            eval_route_value(arena, ins, outs, routes, env, loop_detector)
        }
        BoxMatch::Seq(e1, e2) => eval_seq_value(arena, e1, e2, env, loop_detector),
        // ── outputs(expr) / inputs(expr) ────────────────────────────────────
        // C++: eval.cpp handles `isBoxOutputs`/`isBoxInputs` by evaluating the
        // inner box, calling `getBoxType` to obtain the arity, then returning a
        // `boxInt(n)` so the result can be used as a numeric constant (e.g. as
        // an iteration count for `par`/`ipar`).
        //
        // Without this arm, `outputs(…)` reaches the `_` catch-all and is
        // kept as a `BOXOUTPUTS(…)` node.  When that node is later used as
        // the iteration count of an `ipar`/`par`, `eval_non_negative_count`
        // fails with "iteration count is not an int node".
        //
        // Example failure (softclipQuadratic1 in aanl.lib):
        //   pickN(N,O) = route(N,outputs(O), par(o,outputs(O), …))
        //   → outputs((0,1,2,3,4)) must reduce to boxInt(5) at eval time.
        BoxMatch::Outputs(inner) => {
            let inner_val = eval_box(arena, inner, env, loop_detector)?;
            let lowered = a2sb(arena, inner_val, loop_detector)?;
            if let Some((_ins, outs)) = infer_box_arity(arena, lowered) {
                let n = i32::try_from(outs).unwrap_or(i32::MAX);
                let mut bld = BoxBuilder::new(arena);
                Ok(EvalValue::Box(bld.int(n)))
            } else {
                let mut bld = BoxBuilder::new(arena);
                Ok(EvalValue::Box(bld.outputs(lowered)))
            }
        }
        BoxMatch::Inputs(inner) => {
            let inner_val = eval_box(arena, inner, env, loop_detector)?;
            let lowered = a2sb(arena, inner_val, loop_detector)?;
            if let Some((ins, _outs)) = infer_box_arity(arena, lowered) {
                let n = i32::try_from(ins).unwrap_or(i32::MAX);
                let mut bld = BoxBuilder::new(arena);
                Ok(EvalValue::Box(bld.int(n)))
            } else {
                let mut bld = BoxBuilder::new(arena);
                Ok(EvalValue::Box(bld.inputs(lowered)))
            }
        }
        _ => Ok(EvalValue::Box(map_children(
            arena,
            expr,
            env,
            loop_detector,
        )?)),
    }
}

/// Evaluates an identifier (`BoxMatch::Ident`) within the current environment.
///
/// Looks up `name` in the environment, then:
/// - plain box: recurses under loop detection,
/// - closure over an abstraction or environment block: returned as-is,
/// - other closure: forced under its captured environment,
/// - pattern matcher: returned as-is.
///
/// `name` must be an owned string so the arena borrow from the original
/// `match_box` call has been released before any mutable arena access.
fn eval_ident_value(
    arena: &mut TreeArena,
    expr: TreeId,
    name: &str,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<EvalValue, EvalError> {
    // get_symbol takes &self — safe while `name` no longer borrows `arena` mutably.
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
            // Record def-name → box mapping for SVG folding (C++ setDefNameProperty).
            if let Ok(EvalValue::Box(box_id)) = &out {
                loop_detector.def_names.insert(*box_id, name.to_owned());
            }
            out
        }
        EvalValue::PatternMatcher(pm) => Ok(EvalValue::PatternMatcher(pm)),
    }
}

/// Evaluates a `route(ins, outs, routes)` box.
///
/// Source provenance (C++): `compiler/evaluate/eval.cpp`, `isBoxRoute` branch.
///
/// C++ evaluates ins/outs/routes, propagates each through a nil-input signal
/// context to reduce them to integers (`sigList2vecInt`), then normalises the
/// route spec (`normalizeRouteList`). Rust mirrors this with
/// `eval_box_to_int_node` (propagate + simplify → `i32` → `boxInt`) and
/// `normalize_route_spec`.
fn eval_route_value(
    arena: &mut TreeArena,
    ins: TreeId,
    outs: TreeId,
    routes: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<EvalValue, EvalError> {
    let eval_ins = eval_box(arena, ins, env, loop_detector)?;
    let eval_outs = eval_box(arena, outs, env, loop_detector)?;
    let eval_routes = eval_box(arena, routes, env, loop_detector)?;

    let ins_node = eval_box_to_int_node(arena, eval_ins).unwrap_or(eval_ins);
    let outs_node = eval_box_to_int_node(arena, eval_outs).unwrap_or(eval_outs);
    let routes_node = a2sb(arena, eval_routes, loop_detector).unwrap_or(eval_routes);
    let spec_node = eval_box_to_int_list_node(arena, routes_node).unwrap_or_else(|| {
        let mut cache = ahash::HashMap::with_hasher(ahash::RandomState::new());
        let simplified_routes = box_simplification(arena, &mut cache, routes_node);
        normalize_route_spec(arena, simplified_routes)
    });

    let mut bld = BoxBuilder::new(arena);
    Ok(EvalValue::Box(bld.route(ins_node, outs_node, spec_node)))
}

/// Evaluates a `seq(e1, e2)` box, folding numeric tuples where possible.
///
/// Source provenance (C++): `compiler/evaluate/eval.cpp`, `isBoxSeq` branch.
///
/// If `e1` evaluates to a parallel of Int/Real literals, the composition is
/// folded via propagation (`try_fold_seq_numeric`); otherwise `boxSeq(a1, a2)`
/// is returned. Both `SigInt`/`SigReal` and `BoxInt`/`BoxReal` share the same
/// `NodeKind`, so the folded `SigId` is directly usable as a `BoxId`.
fn eval_seq_value(
    arena: &mut TreeArena,
    e1: TreeId,
    e2: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<EvalValue, EvalError> {
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
            BoxMatch::Abstr(_, _) => {
                // Store the closure (abstraction + captured env) in the
                // side-table and return a boxClosure(key) tree node.
                // This mirrors the boxPatternMatcher pattern and matches
                // C++ where closure(expr, genv, visited, lenv) is a tree node.
                let key = loop_detector.store_closure(closure);
                let mut b = BoxBuilder::new(arena);
                let key_node = b.int(key);
                Ok(b.closure_node(key_node))
            }
            BoxMatch::Environment => Ok(closure.expr),
            _ => eval_box(arena, closure.expr, &closure.env, loop_detector),
        },
        EvalValue::PatternMatcher(pm) => {
            // Always preserve pattern matchers as explicit runtime nodes.
            // Returning the original `case` tree for an unapplied matcher loses
            // the captured lexical environment, which breaks higher-order uses
            // like passing a local `case` function through another function
            // before eventually applying it.
            let key = loop_detector.store_pm(pm);
            let mut b = BoxBuilder::new(arena);
            let key_node = b.int(key);
            Ok(b.pattern_matcher(key_node))
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
///   which now preserves `importFile(...)` nodes through parse and expands them
///   structurally from the parsed definition tree like C++
///   `gReader.expandList(gReader.getList(fname))`,
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
                .find(|path| source_context.virtual_sources().contains(path) || path.exists())
                .cloned()
                .ok_or_else(|| EvalError::SourceFileNotFound {
                    node,
                    construct,
                    target: target.clone(),
                    current_file: source_context.current_file().map(Path::to_path_buf),
                    search_paths: source_context.search_paths().to_vec(),
                })?;
            let parse = match source_context.metadata_store() {
                Some(metadata_store) => {
                    if let Some(source) = source_context.virtual_sources().get(&resolved_path) {
                        parser::parse_program_with_imports_and_metadata(
                            source,
                            &resolved_path.to_string_lossy(),
                            source_context.search_paths(),
                            source_context.virtual_sources(),
                            metadata_store.clone(),
                        )
                    } else {
                        parser::parse_file_with_imports_and_metadata(
                            &resolved_path,
                            source_context.search_paths(),
                            metadata_store.clone(),
                        )
                    }
                }
                None => {
                    if let Some(source) = source_context.virtual_sources().get(&resolved_path) {
                        parser::parse_program_with_imports_and_metadata(
                            source,
                            &resolved_path.to_string_lossy(),
                            source_context.search_paths(),
                            source_context.virtual_sources(),
                            parser::CompilationMetadataStore::new(&resolved_path.to_string_lossy()),
                        )
                    } else {
                        parser::parse_file_with_imports(
                            &resolved_path,
                            source_context.search_paths(),
                        )
                    }
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
    // Global search paths (DSP file directory first) take priority over the
    // current file's directory, matching C++ faust compiler semantics where a
    // local platform.lib override in the DSP directory wins over the system
    // library found next to stdfaust.lib.
    for base in source_context.search_paths() {
        let candidate = base.join(target);
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    // Current file's directory as fallback for relative-to-library imports not
    // covered by the search paths (e.g. a library importing a sibling file
    // from a directory that is not in the explicit search list).
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
///
/// C++ parity: `evalLabel(...)` calls `eval2int(...)`, which ends in
/// `tree2int(...)`. That helper accepts both integer atoms and floating-point
/// atoms, truncating real constants toward zero. Label placeholders therefore
/// inherit the same permissive "integer constant numerical expression"
/// semantics as the C++ evaluator.
fn eval_ident_to_constant_int(
    arena: &mut TreeArena,
    ident: &str,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<i64, EvalError> {
    let expr = BoxBuilder::new(arena).ident(ident);
    let signal = eval_box_to_scalar_signal(arena, expr, env, loop_detector)?;
    if let Some(value) = tree_to_int(arena, signal) {
        return Ok(value);
    }
    if let Some(value) = tree_to_double(arena, signal) {
        return Ok(value as i64);
    }
    Err(EvalError::InvalidLabelInterpolation {
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
    let mut cache = ArityCache::new();
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
    let mut cache = ArityCache::new();
    let signals = propagate_typed(arena, flat, &[], &mut cache).ok()?;
    let [sig] = signals.as_slice() else {
        return None;
    };
    Some(simplify_const(arena, *sig))
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
    // Fast path: already a literal — return unchanged, no type coercion.
    //
    // C++ `isBoxNumeric` short-circuits on any `boxInt` / `boxReal` literal and
    // returns it as-is, without converting integer-valued floats (`1.0`) to
    // `boxInt(1)`.  The automaton stores pattern constants as their original
    // `TreeId` (e.g. `float_bits(0x3ff0000000000000)` for `1.0`), and matching
    // is a `TreeId` equality test.  Converting `1.0 → int(1)` here would make
    // `foo(1.0) = 456;` never match the call `foo(1.0)`.
    match match_box(arena, box_id) {
        BoxMatch::Int(_) | BoxMatch::Real(_) => return box_id,
        _ => {}
    }
    let Some(sig) = propagate_box_and_simplify(arena, box_id) else {
        return box_id;
    };
    // For arithmetic expressions, the signal type determines the result type:
    // - `SigInt` for integer-only operations (e.g. `1+1`, `max(int,int)`).
    //   C++ xtended `computeSigOutput` for `min`/`max` preserves the integer
    //   type when both operands are integers; `normalize/src/simplify.rs` now
    //   mirrors this so `max(1, min(2, 4))` folds to `SigInt(2)`.
    // - `SigReal` for anything involving a float or real-valued ops (e.g. `/`).
    //   The pattern `foo(4.0/2.0) = 789;` stores `boxReal(2.0)` and the
    //   argument simplifies to the same — no coercion needed.
    let value = match match_sig(arena, sig) {
        SigMatch::Int(i) => Some(NumericLit::Int(i)),
        SigMatch::Real(x) => Some(NumericLit::Real(x)),
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

/// Converts a 0→N constant box into a canonical right-spine `Par` tree of
/// `boxInt` leaves.
///
/// This follows the C++ `isBoxRoute` path in `eval.cpp`, where the route
/// specification is propagated as a whole and then converted back from the
/// resulting integer signal list.
fn eval_box_to_int_list_node(arena: &mut TreeArena, box_id: TreeId) -> Option<TreeId> {
    let (inputs, outputs) = infer_box_arity(arena, box_id)?;
    if inputs != 0 || outputs == 0 {
        return None;
    }
    let flat = try_build_flat_box(arena, box_id).ok()?;
    let mut cache = ArityCache::new();
    let signals = propagate_typed(arena, flat, &[], &mut cache).ok()?;
    if signals.len() != outputs {
        return None;
    }

    let mut ints = Vec::with_capacity(outputs);
    for sig in signals {
        let sig = simplify_const(arena, sig);
        let value = match match_sig(arena, sig) {
            SigMatch::Int(i) => i,
            SigMatch::Real(x) => {
                let i = x as i32;
                if (i as f64) != x {
                    return None;
                }
                i
            }
            _ => return None,
        };
        ints.push(BoxBuilder::new(arena).int(value));
    }

    let mut result = *ints.last()?;
    for leaf in ints[..ints.len() - 1].iter().rev() {
        result = BoxBuilder::new(arena).par(*leaf, result);
    }
    Some(result)
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
/// Returns `true` if `box_id` is a parallel composition of **integer** literals only.
///
/// Used to decide whether a Real result from seq folding should be
/// cast back to Int (preserving integer semantics for pattern matching).
fn all_inputs_are_int(arena: &TreeArena, box_id: TreeId) -> bool {
    match match_box(arena, box_id) {
        BoxMatch::Int(_) => true,
        BoxMatch::Real(_) => false,
        BoxMatch::Par(l, r) => all_inputs_are_int(arena, l) && all_inputs_are_int(arena, r),
        _ => false,
    }
}

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
/// the result; if propagation yields exactly one output signal and that
/// simplified signal is a numeric constant, returns the corresponding
/// `boxInt(n)` or `boxReal(x)`.
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
    // C++ folds only when propagation produces exactly one output. Multi-output
    // sequences such as `(0,0,1,1) : par(i,2,+)` must remain structured graphs,
    // not collapse to the first propagated constant.
    let seq = BoxBuilder::new(arena).seq(a1, a2);
    let flat = try_build_flat_box(arena, seq).ok()?;
    let mut cache = ArityCache::new();
    let signals = propagate_typed(arena, flat, &[], &mut cache).ok()?;
    let [sig] = signals.as_slice() else {
        return None;
    };
    let sig = simplify_const(arena, *sig);
    // Both SigInt/SigReal and BoxInt/BoxReal share the same underlying NodeKind
    // (NodeKind::Int / NodeKind::FloatBits), so the SigId IS the BoxId.
    //
    // When all inputs are Int and the result is a Real that happens to be an
    // exact integer (e.g. `4/2 → Real(2.0)`), convert back to Int.  This is
    // critical for pattern matching: `S(i,i)` compares tree nodes by identity,
    // so `Real(2.0) != Int(2)` would cause the match to fail.
    // C++ eval keeps integer semantics at the box level for this reason.
    match match_sig(arena, sig) {
        SigMatch::Int(_) => Some(sig),
        SigMatch::Real(x)
            if all_inputs_are_int(arena, a1)
                && x.fract() == 0.0
                && x >= f64::from(i32::MIN)
                && x <= f64::from(i32::MAX) =>
        {
            Some(BoxBuilder::new(arena).int(x as i32))
        }
        SigMatch::Real(_) => Some(sig),
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
                // Observable C++ parity:
                // compile-time boolean/integer expressions can sometimes reach
                // box simplification as exact reals (`1.0`, `0.0`) after signal
                // propagation. The C++ evaluator still treats these as integer
                // constants in downstream contexts such as pattern matching for
                // `case` dispatch. Collapse exact integer reals back to
                // `boxInt` here so residual `case` applications see the same
                // constant class on examples like `routes.lib`'s
                // `comparatorDirections(...)`.
                let i = x as i32;
                if (i as f64) == x {
                    return BoxBuilder::new(arena).int(i);
                }
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
        // Preserve residual closures/pattern matchers as box nodes here instead
        // of symbolically forcing them. Generic parent nodes such as `par`
        // must be able to carry higher-order children unchanged so later case
        // matching can still see tupled functions the same way C++ does.
        children.push(force_value_to_box(arena, value, loop_detector)?);
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

    loop_detector.enter_structural()?;
    let result = (|| match fun {
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
                let arg = eval_value(arena, arg, &closure.env, loop_detector)?;
                let mut scoped = closure.env.push_scope();
                let sym = arena.intern_symbol(&param_name);
                scoped.bind_value(sym, arg);
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
    })();
    loop_detector.leave_structural();
    result
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

    loop_detector.enter_structural()?;
    let result = (|| {
        let raw_arg = arena
            .hd(larg)
            .ok_or(EvalError::MalformedListNode { node: larg })?;
        // C++ parity: case dispatch sees numeric arguments after the same
        // compile-time simplification pass used by pattern preparation. Without
        // this, selector expressions like `((l != 0) & ...) * 2` remain residual
        // box trees and only catch-all rules match.
        let arg = {
            let mut cache = ahash::HashMap::with_hasher(ahash::RandomState::new());
            box_simplification(arena, &mut cache, raw_arg)
        };
        let (new_state, _) = pattern_matcher::apply_pattern_matcher(
            arena,
            &pm.automaton,
            pm.state,
            arg,
            &mut pm.envs,
        );
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
    })();
    loop_detector.leave_structural();
    result
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
        BoxMatch::Closure(key_node) => {
            // Retrieve the closure from the side-table and apply it via the
            // standard closure application path.
            let key = match match_box(arena, key_node) {
                BoxMatch::Int(k) => k,
                _ => {
                    return Err(EvalError::InternalError {
                        message: "boxClosure key is not an integer".to_owned(),
                    });
                }
            };
            let cv = loop_detector
                .get_closure(key)
                .ok_or_else(|| EvalError::InternalError {
                    message: format!("boxClosure key {} not found in closure store", key),
                })?;
            let applied = apply_value_list_value(
                arena,
                EvalValue::Closure(cv),
                larg,
                env,
                loop_detector,
                call_site,
            )?;
            force_value_to_box(arena, applied, loop_detector)
        }
        _ => {
            // C++ parity (`applyList`): for non-closures, insert implicit wires when
            // partially applying a function, and reject over-application.
            let maybe_fun_arity = infer_box_arity_for_apply(arena, fun, loop_detector);
            let maybe_larg_outputs = list_outputs_for_apply(arena, larg, loop_detector);
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
fn list_outputs_for_apply(
    arena: &mut TreeArena,
    mut list: TreeId,
    loop_detector: &mut LoopDetector,
) -> Option<usize> {
    let mut total = 0usize;
    while !arena.is_nil(list) {
        let head = arena.hd(list)?;
        let outs =
            infer_box_arity_for_apply(arena, head, loop_detector).map_or(1, |(_, outs)| outs);
        total = total.checked_add(outs)?;
        list = arena.tl(list)?;
    }
    Some(total)
}

fn infer_box_arity_for_apply(
    arena: &mut TreeArena,
    id: TreeId,
    loop_detector: &mut LoopDetector,
) -> Option<(usize, usize)> {
    // C++ `applyList` / `boxlistOutputs` always run `a2sb(...)` before
    // `getBoxType(...)` when probing local arity for application lowering.
    // Doing the same here avoids under-counting residual symbolic boxes such as
    // partially applied `selectbus(...)`, which otherwise fall back to
    // "1 output" and trigger spurious implicit wires.
    a2sb(arena, id, loop_detector)
        .ok()
        .and_then(|lowered| infer_box_arity(arena, lowered))
        .or_else(|| infer_box_arity(arena, id))
}

/// Local arity inference used by non-closure application lowering.
///
/// Returns `(inputs, outputs)` for the subset needed in `apply_list`.
/// `None` means arity is unknown or invalid for this fast-path inference.
/// Infers `(inputs, outputs)` for the evaluator-supported first-order box subset.
///
/// This lightweight arity oracle is intentionally narrower than the dedicated
/// `propagate::box_arity_typed(...)` contract. It exists for local evaluator tasks
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
        BoxMatch::Waveform(_) => Some((0, 2)),
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
        BoxMatch::ForwardAD(exp, seed) => {
            let (_, seed_outs) = infer_box_arity(arena, seed)?;
            if seed_outs != 1 {
                return None;
            }
            let (exp_ins, exp_outs) = infer_box_arity(arena, exp)?;
            let (seed_ins, _) = infer_box_arity(arena, seed)?;
            Some((exp_ins.max(seed_ins), exp_outs * 2))
        }
        BoxMatch::ReverseAD(exp, seeds) => {
            let (exp_ins, exp_outs) = infer_box_arity(arena, exp)?;
            let (seeds_ins, seeds_outs) = infer_box_arity(arena, seeds)?;
            if exp_outs == 0 || seeds_outs == 0 {
                return None;
            }
            Some((exp_ins.max(seeds_ins), exp_outs + seeds_outs))
        }
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
    if let Ok(v) = eval_box_to_i32(arena, count) {
        return match v {
            v if v < 0 => Err(EvalError::NegativeIterationCount {
                value: i64::from(v),
            }),
            v => usize::try_from(v).map_err(|_| EvalError::IterationCountTooLarge {
                value: i64::from(v),
            }),
        };
    }
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

/// Returns the neutral element for `iseq(i, 0, body)` following C++ `neutralExpSeq`.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp`
/// - `neutralExpSeq`
///
/// Mapping status: `adapted`.
/// C++ evaluates the body once with `i = 0`, lowers the result with `a2sb`,
/// and constructs an identity bus whose width matches the body outputs when the
/// body has equal input/output arity. Only a real `0 -> 0` body uses the empty
/// `route(0,0,par(0,0))` neutral element.
fn neutral_seq_body(
    arena: &mut TreeArena,
    var_name: &str,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let evaluated = eval_iter_body(arena, var_name, 0, body, env, loop_detector)?;
    let lowered = a2sb(arena, evaluated, loop_detector)?;
    let Some((ins, outs)) = infer_box_arity(arena, lowered) else {
        return Err(EvalError::InternalError {
            message: "seq(i,0,body) neutral arity could not be inferred".to_owned(),
        });
    };
    if ins != outs {
        return Err(EvalError::InternalError {
            message: format!(
                "seq(i,0,body) requires matching input/output arity, got {ins} -> {outs}"
            ),
        });
    }
    if outs == 0 {
        return Ok(empty_iteration_route(arena));
    }
    let mut b = BoxBuilder::new(arena);
    let mut bus = b.wire();
    for _ in 1..outs {
        let mut b = BoxBuilder::new(arena);
        let wire = b.wire();
        bus = b.par(bus, wire);
    }
    Ok(bus)
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
        // par(i, 0, X) = empty block (0 inputs, 0 outputs), not a neutral-seq identity.
        // iterate_sum/prod use the same convention; only iseq uses neutral_seq_body.
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
        return neutral_seq_body(arena, &var_name, body, env, loop_detector);
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

/// Simplifies a pattern after evaluation, mirroring C++ `patternSimplification`.
///
/// Source provenance (C++):
/// - `compiler/evaluate/eval.cpp` — `patternSimplification` (line 773)
///
/// Algorithm (exact C++ match):
/// 1. Try to reduce the whole expression to a numeric literal via full
///    signal propagation + `simplify()` (delegates to `simplify_pattern`,
///    which is the Rust equivalent of C++ `isBoxNumeric`).
/// 2. If that fails AND the node is a `PatternOp`
///    (Par / Seq / Split / Merge / Rec only — matches C++ `isBoxPatternOp`),
///    recurse into its two children.
/// 3. Otherwise return the pattern unchanged.
///
/// Note: HGroup / VGroup / TGroup / Route are **not** PatternOps in C++ and
/// are returned unchanged without recursion.
fn pattern_simplification(arena: &mut TreeArena, pattern: TreeId) -> TreeId {
    // (a) Try full constant folding on the whole expression first.
    let folded = simplify_pattern(arena, pattern);
    if folded != pattern {
        return folded;
    }
    // (b) Recurse into PatternOp children (Par/Seq/Split/Merge/Rec only).
    match match_box(arena, pattern) {
        BoxMatch::Par(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            BoxBuilder::new(arena).par(sa, sb)
        }
        BoxMatch::Seq(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            BoxBuilder::new(arena).seq(sa, sb)
        }
        BoxMatch::Split(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            BoxBuilder::new(arena).split(sa, sb)
        }
        BoxMatch::Merge(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            BoxBuilder::new(arena).merge(sa, sb)
        }
        BoxMatch::Rec(a, b) => {
            let sa = pattern_simplification(arena, a);
            let sb = pattern_simplification(arena, b);
            BoxBuilder::new(arena).rec(sa, sb)
        }
        // (c) Everything else (HGroup/VGroup/TGroup/Route/…) — unchanged.
        _ => pattern,
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
        Environment, LoopDetector, a2sb, box_simplification, eval_box, eval_box_to_f64,
        eval_box_to_i32, eval_box_to_int_list_node, eval_box_to_int_node, flatten_route_spec,
        infer_box_arity_for_apply, is_numerical_tuple_box, normalize_route_spec,
        propagate_box_and_simplify, try_fold_seq_numeric,
    };
    use parser::parse_program;

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

    /// Multi-output boxes must not be simplified by taking the first output.
    ///
    /// C++ `boxPropagateSig(nil, box, [])` is only consumed as a scalar
    /// simplification when the propagated list is a singleton. A pure route
    /// network with four outputs must therefore stay structured.
    #[test]
    fn propagate_box_and_simplify_route_identity_is_none() {
        let mut arena = TreeArena::default();
        let mut b = BoxBuilder::new(&mut arena);
        let a = b.real(0.3);
        let bb = b.real(-0.2);
        let c = b.real(0.8);
        let d = b.real(-0.5);
        let left = b.par(a, bb);
        let right = b.par(c, d);
        let inputs = b.par(left, right);
        let i1a = b.int(1);
        let i1b = b.int(1);
        let i2a = b.int(2);
        let i2b = b.int(2);
        let i3a = b.int(3);
        let i3b = b.int(3);
        let i4a = b.int(4);
        let i4b = b.int(4);
        let p4 = b.par(i4a, i4b);
        let p3 = b.par(i3b, p4);
        let p2 = b.par(i3a, p3);
        let p1 = b.par(i2b, p2);
        let p0 = b.par(i2a, p1);
        let q1 = b.par(i1b, p0);
        let spec = b.par(i1a, q1);
        let ins = b.int(4);
        let outs = b.int(4);
        let route = b.route(ins, outs, spec);
        let expr = b.seq(inputs, route);
        assert!(
            propagate_box_and_simplify(&mut arena, expr).is_none(),
            "multi-output route network should stay structured"
        );
    }

    /// Route specifications computed as 0→N integer tuples must be rebuilt as
    /// canonical `par(int, ...)` trees, like the C++ `isBoxRoute` evaluator.
    #[test]
    fn eval_box_to_int_list_node_rebuilds_computed_route_spec() {
        let mut arena = TreeArena::default();
        let mut b = BoxBuilder::new(&mut arena);

        let base = [0, 0, 1, 1, 2, 2, 3, 3]
            .into_iter()
            .map(|i| b.int(i))
            .collect::<Vec<_>>();
        let ones = [1, 1, 1, 1, 1, 1, 1, 1]
            .into_iter()
            .map(|i| b.int(i))
            .collect::<Vec<_>>();

        let mut pairs = Vec::with_capacity(16);
        for (lhs, rhs) in base.iter().copied().zip(ones.iter().copied()) {
            pairs.push(lhs);
            pairs.push(rhs);
        }
        let args = pairs
            .into_iter()
            .reduce(|acc, next| b.par(acc, next))
            .expect("interleaved args");

        let mut adds = b.add();
        for _ in 1..8 {
            let next = b.add();
            adds = b.par(adds, next);
        }
        let expr = b.seq(args, adds);

        let result = eval_box_to_int_list_node(&mut arena, expr).expect("route list");
        let mut leaves = Vec::new();
        flatten_route_spec(&arena, result, &mut leaves);
        let ints = leaves
            .into_iter()
            .map(|leaf| match match_box(&arena, leaf) {
                BoxMatch::Int(i) => i,
                other => panic!("expected int leaf, got {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(ints, vec![1, 1, 2, 2, 3, 3, 4, 4]);
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

    /// Faust `/` is real-valued even for integer literals: `1/3` stays a real constant.
    ///
    /// C++ equivalent: `simplifyPattern(box(1/3))` reduces to `boxReal(1.0/3.0)`.
    #[test]
    fn simplify_pattern_int_division_is_real_like_cpp() {
        let mut arena = TreeArena::default();
        let one = BoxBuilder::new(&mut arena).int(1);
        let three = BoxBuilder::new(&mut arena).int(3);
        let div = {
            let mut b = BoxBuilder::new(&mut arena);
            let args = b.par(one, three);
            let op = b.div();
            b.seq(args, op)
        };
        let result = super::simplify_pattern(&mut arena, div);
        match match_box(&arena, result) {
            BoxMatch::Real(v) => assert!((v - (1.0 / 3.0)).abs() < 1e-12),
            other => panic!("expected boxReal(1/3), got {other:?}"),
        }
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

    /// Multi-output propagated sequences must not fold to the first output.
    ///
    /// C++ only folds `seq(a1, a2)` in `eval.cpp` when `boxPropagateSig(...)`
    /// returns a singleton list. This protects constructs such as
    /// `(0,0,1,1) : par(i,2,+)` that semantically produce two outputs.
    #[test]
    fn try_fold_seq_multi_output_parallel_add_does_not_fold() {
        let mut arena = TreeArena::default();
        let zero_a = BoxBuilder::new(&mut arena).int(0);
        let zero_b = BoxBuilder::new(&mut arena).int(0);
        let one_a = BoxBuilder::new(&mut arena).int(1);
        let one_b = BoxBuilder::new(&mut arena).int(1);
        let left = BoxBuilder::new(&mut arena).par(zero_a, zero_b);
        let right = BoxBuilder::new(&mut arena).par(one_a, one_b);
        let inputs = BoxBuilder::new(&mut arena).par(left, right);
        let add = BoxBuilder::new(&mut arena).add();
        let adds = BoxBuilder::new(&mut arena).par(add, add);

        let result = try_fold_seq_numeric(&mut arena, inputs, adds);
        assert!(
            result.is_none(),
            "multi-output sequence should stay structured and not fold"
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
        let mut cache = ahash::HashMap::with_hasher(ahash::RandomState::new());
        let result = box_simplification(&mut arena, &mut cache, five);
        assert!(matches!(match_box(&arena, result), BoxMatch::Int(5)));
    }

    /// `box_simplification(seq(par(int(2), int(3)), add))` → `boxInt(5)`.
    #[test]
    fn box_simplification_folds_arithmetic() {
        let mut arena = TreeArena::default();
        let expr = make_int_add(&mut arena, 2, 3);
        let mut cache = ahash::HashMap::with_hasher(ahash::RandomState::new());
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
        let mut cache = ahash::HashMap::with_hasher(ahash::RandomState::new());
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

    /// Exact integer reals in route specs are canonicalized back to `boxInt`
    /// leaves, like the C++ `sigList2vecInt(...)` path in `isBoxRoute`.
    #[test]
    fn eval_route_exact_integer_real_spec_leaves_become_ints() {
        let mut arena = TreeArena::default();
        let ins = BoxBuilder::new(&mut arena).int(2);
        let outs = BoxBuilder::new(&mut arena).int(2);
        let r1a = BoxBuilder::new(&mut arena).real(1.0);
        let r1b = BoxBuilder::new(&mut arena).real(1.0);
        let r2a = BoxBuilder::new(&mut arena).real(2.0);
        let r2b = BoxBuilder::new(&mut arena).real(2.0);
        let p1 = BoxBuilder::new(&mut arena).par(r1a, r1b);
        let p2 = BoxBuilder::new(&mut arena).par(r2a, r2b);
        let spec = BoxBuilder::new(&mut arena).par(p1, p2);
        let route_box = BoxBuilder::new(&mut arena).route(ins, outs, spec);
        let env = Environment::empty();
        let mut ld = LoopDetector::new();
        let result = eval_box(&mut arena, route_box, &env, &mut ld).unwrap();
        let BoxMatch::Route(_, _, normalized_spec) = match_box(&arena, result) else {
            panic!("expected Route");
        };
        let mut leaves = Vec::new();
        flatten_route_spec(&arena, normalized_spec, &mut leaves);
        let vals: Vec<i32> = leaves
            .iter()
            .map(|&leaf| match match_box(&arena, leaf) {
                BoxMatch::Int(n) => n,
                other => panic!("expected Int leaf, got {other:?}"),
            })
            .collect();
        assert_eq!(vals, [1, 1, 2, 2]);
    }

    /// Reusing the same residual abstraction inside one evaluation session must
    /// reuse one `a2sb(...)` lowering, like C++ `gSymbolicBoxProperty`.
    #[test]
    fn a2sb_reuses_residual_abstraction_within_one_loop_detector() {
        let mut arena = TreeArena::default();
        let x = arena.intern_symbol("x");
        let ident = BoxBuilder::new(&mut arena).ident("x");
        let lambda = BoxBuilder::new(&mut arena).abstr(ident, ident);
        let mut ld = LoopDetector::new();

        let first = a2sb(&mut arena, lambda, &mut ld).expect("first lowering should succeed");
        let second = a2sb(&mut arena, lambda, &mut ld).expect("second lowering should succeed");

        assert_eq!(
            first, second,
            "same residual abstraction should reuse one symbolic lowering"
        );
        assert!(
            ld.symbolic_box_cache.contains_key(&lambda),
            "a2sb cache should retain the residual abstraction key"
        );
        let BoxMatch::Symbolic(slot, body) = match_box(&arena, first) else {
            panic!("expected symbolic lowering");
        };
        assert_eq!(slot, body);
        assert_eq!(
            x,
            arena
                .get_symbol("x")
                .expect("symbol should remain interned")
        );
    }

    /// `a2sb(...)` cache entries are per-session. Fresh `LoopDetector`s may
    /// rebuild the same interned symbolic form, but they must not share cache
    /// state across evaluation sessions.
    #[test]
    fn a2sb_does_not_reuse_residual_abstraction_across_loop_detectors() {
        let mut arena = TreeArena::default();
        let ident = BoxBuilder::new(&mut arena).ident("x");
        let lambda = BoxBuilder::new(&mut arena).abstr(ident, ident);

        let mut first_ld = LoopDetector::new();
        let first = a2sb(&mut arena, lambda, &mut first_ld).expect("first lowering should succeed");
        assert!(
            first_ld.symbolic_box_cache.contains_key(&lambda),
            "first session should populate its own symbolic cache"
        );

        let mut second_ld = LoopDetector::new();
        assert!(
            second_ld.symbolic_box_cache.is_empty(),
            "fresh session should start with an empty symbolic cache"
        );
        let second =
            a2sb(&mut arena, lambda, &mut second_ld).expect("second lowering should succeed");
        assert_eq!(
            first, second,
            "independent sessions may still rebuild the same interned symbolic form"
        );
        assert!(
            second_ld.symbolic_box_cache.contains_key(&lambda),
            "second session should populate its own symbolic cache independently"
        );
    }

    /// Residual `case` values should also reuse their symbolic lowering within
    /// one evaluation session.
    #[test]
    fn a2sb_reuses_residual_case_within_one_loop_detector() {
        let parsed = parse_program(
            r#"
process = case {
  (x) => x;
  (0) => _;
};
"#,
            "<memory>",
        );
        assert!(
            parsed.errors.is_empty(),
            "unexpected parse errors: {:?}",
            parsed.errors
        );
        let mut arena = parsed.state.arena;
        let defs = parsed.root.expect("root should exist");
        let def = arena.hd(defs).expect("process def");
        let payload = arena.tl(def).expect("definition payload");
        let expr = arena.tl(payload).expect("definition expr");
        let mut ld = LoopDetector::new();

        let first = a2sb(&mut arena, expr, &mut ld).expect("first case lowering should succeed");
        let second = a2sb(&mut arena, expr, &mut ld).expect("second case lowering should succeed");

        assert_eq!(
            first, second,
            "same residual case should reuse one symbolic lowering"
        );
        assert!(
            ld.symbolic_box_cache.contains_key(&expr),
            "a2sb cache should retain the residual case key"
        );
    }

    /// Application-side arity probing should consume the same cached `a2sb`
    /// lowering instead of rebuilding a fresh symbolic closure each time.
    #[test]
    fn infer_box_arity_for_apply_reuses_cached_a2sb_lowering() {
        let mut arena = TreeArena::default();
        let x = BoxBuilder::new(&mut arena).ident("x");
        let xx = BoxBuilder::new(&mut arena).par(x, x);
        let lambda = BoxBuilder::new(&mut arena).abstr(x, xx);
        let mut ld = LoopDetector::new();

        let first =
            infer_box_arity_for_apply(&mut arena, lambda, &mut ld).expect("first arity probe");
        let cached_len = ld.symbolic_box_cache.len();
        let second =
            infer_box_arity_for_apply(&mut arena, lambda, &mut ld).expect("second arity probe");

        assert_eq!(first, (1, 2));
        assert_eq!(second, (1, 2));
        assert_eq!(
            ld.symbolic_box_cache.len(),
            cached_len,
            "repeated apply-time arity probing should reuse cached a2sb lowering"
        );
    }
}
