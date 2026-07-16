//! CSE (Common Subexpression Elimination) materialization pass for FIR.
//!
//! **Phase 2** of the FIR runtime optimization plan.  Runs after variability-driven
//! statement placement (Phase 1) and before FIR module assembly.
//!
//! For each execution-tier bucket (`constants_statements`, `control_statements`,
//! `sample_statements`), this pass:
//! 1. counts how many times each `FirId` value node is referenced as a child,
//! 2. wraps multi-referenced non-trivial expressions in `DeclareVar` + `LoadVar`,
//! 3. operates on the `FirStore` so all backends (C++, WASM, Cranelift, FBC) benefit.
//!
//! See [Phase 2 of the FIR runtime optimization plan](../../porting/fir-cse-runtime-optimizations-plan-2026-04-03-en.md#3-phase-2--cse-materialization).

use std::collections::{HashMap, HashSet};

use fir::{AccessType, FirBuilder, FirId, FirMatch, FirStore, FirType, match_fir};

struct TypedCounters<'a> {
    float_prefix: &'a str,
    float_counter: u32,
    int_prefix: &'a str,
    int_counter: u32,
}

struct RewriteState<'a> {
    ref_counts: &'a HashMap<FirId, usize>,
    materialized: HashMap<FirId, (String, FirType)>,
    temp_decls: Vec<FirId>,
}

// ─── Reference counting ─────────────────────────────────────────────────────

/// Counts how many times each `FirId` appears as a value child across a bucket.
///
/// `ref_counts[id]` = number of distinct parent references to `id`.  Children
/// are descended only once per unique `FirId` (the `descended` set), so the
/// count reflects fan-out, not tree depth.
pub(super) fn count_fir_value_uses(store: &FirStore, roots: &[FirId]) -> HashMap<FirId, usize> {
    let mut ref_counts: HashMap<FirId, usize> = HashMap::new();
    let mut descended: HashSet<FirId> = HashSet::new();

    for &root in roots {
        count_refs_stmt(store, root, &mut ref_counts, &mut descended);
    }
    ref_counts
}

/// Enters a statement node and counts references to its value children.
fn count_refs_stmt(
    store: &FirStore,
    stmt: FirId,
    ref_counts: &mut HashMap<FirId, usize>,
    descended: &mut HashSet<FirId>,
) {
    for child in value_children_of(store, stmt) {
        count_refs(store, child, ref_counts, descended);
    }
}

/// Recursively counts value-node references.
fn count_refs(
    store: &FirStore,
    node: FirId,
    ref_counts: &mut HashMap<FirId, usize>,
    descended: &mut HashSet<FirId>,
) {
    *ref_counts.entry(node).or_insert(0) += 1;

    if !descended.insert(node) {
        return; // already visited children of this node
    }
    for child in value_children_of(store, node) {
        count_refs(store, child, ref_counts, descended);
    }
}

// ─── Value-child extraction ─────────────────────────────────────────────────

/// Returns the immediate FirId children of `node` that are value expressions.
///
/// For **value nodes** this returns operands (lhs/rhs, value, args, index, …).
/// For **statement nodes** this returns embedded values (the stored value, the
/// table index, the DeclareVar initializer, …) but *not* structural children
/// such as block bodies or loop bodies — those are separate execution scopes
/// and should not be traversed by intra-bucket CSE.
fn value_children_of(store: &FirStore, node: FirId) -> Vec<FirId> {
    match match_fir(store, node) {
        // ── Value nodes with value operands ──
        FirMatch::BinOp { lhs, rhs, .. } => vec![lhs, rhs],
        FirMatch::Neg { value, .. }
        | FirMatch::Cast { value, .. }
        | FirMatch::Bitcast { value, .. } => vec![value],
        FirMatch::Select2 {
            cond,
            then_value,
            else_value,
            ..
        } => vec![cond, then_value, else_value],
        FirMatch::FunCall { args, .. } => args,
        FirMatch::LoadTable { index, .. } => vec![index],
        FirMatch::TeeVar { value, .. } => vec![value],
        FirMatch::ValueArray { values, .. } => values,
        FirMatch::LoadSoundfileBuffer {
            chan, part, idx, ..
        } => vec![chan, part, idx],
        FirMatch::LoadSoundfileLength { part, .. } | FirMatch::LoadSoundfileRate { part, .. } => {
            vec![part]
        }

        // ── Statement nodes — only embedded value children ──
        FirMatch::StoreVar { value, .. } => vec![value],
        FirMatch::StoreTable { index, value, .. } => vec![index, value],
        FirMatch::DeclareVar {
            init: Some(init), ..
        } => vec![init],
        FirMatch::Drop(value) => vec![value],
        FirMatch::If { cond, .. } | FirMatch::Control { cond, .. } => vec![cond],
        FirMatch::Return(Some(value)) => vec![value],

        // ── Leaf / trivial / structural-only nodes ──
        _ => Vec::new(),
    }
}

// ─── Trivial-node filter ────────────────────────────────────────────────────

/// Returns `true` for nodes that should never be materialized into a temp
/// variable because they are already free to duplicate or because hoisting
/// would be order-sensitive across mutable stores.
fn is_trivial_value(store: &FirStore, node: FirId) -> bool {
    matches!(
        match_fir(store, node),
        FirMatch::Int32 { .. }
            | FirMatch::Int64 { .. }
            | FirMatch::Float32 { .. }
            | FirMatch::Float64 { .. }
            | FirMatch::Bool { .. }
            | FirMatch::LoadVar { .. }
            | FirMatch::LoadVarAddress { .. }
            | FirMatch::LoadTable { .. }
            | FirMatch::NullValue { .. }
    )
}

// ─── Conservative scalar table-effect summary ───────────────────────────────

/// Canonical table subscript accepted by the scalar load-reuse proof.
///
/// Only literal `Int32` subscripts are exact in the initial implementation.
/// Any other expression is deliberately `Unknown`: treating two dynamic
/// indices as different when they can alias would change recursive DSP output,
/// whereas treating them as aliases only misses a reuse opportunity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CanonicalTableIndex {
    Constant(i32),
    Unknown,
}

/// One table location in the private scalar FIR effect model.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TableLocation {
    name: String,
    access: AccessType,
    index: CanonicalTableIndex,
}

/// Effects that can invalidate a straight-line scalar state-load cache.
///
/// This is intentionally private to scalar FIR CSE. It is not a general FIR
/// effect system: its only contract is to avoid reusing a table read after a
/// potentially aliasing write or an unsupported effect. C++ provenance is the
/// ordered `LoadVarInst` / `StoreVarInst` instruction stream emitted by
/// `InstructionsCompiler`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ScalarLoadEffect {
    ReadsTable(TableLocation),
    WritesTable(TableLocation),
    UnknownBarrier,
}

/// Returns the scalar cache key portion that can be proven from `index`.
fn canonical_table_index(store: &FirStore, index: FirId) -> CanonicalTableIndex {
    match match_fir(store, index) {
        FirMatch::Int32 { value, .. } => CanonicalTableIndex::Constant(value),
        _ => CanonicalTableIndex::Unknown,
    }
}

/// Returns whether evaluating `value` has an effect that the local load cache
/// does not model. Foreign calls and `TeeVar` writes are barriers by default.
fn has_unknown_value_effect(store: &FirStore, value: FirId) -> bool {
    match match_fir(store, value) {
        FirMatch::FunCall { .. } | FirMatch::TeeVar { .. } | FirMatch::NewDsp { .. } => true,
        _ => value_children_of(store, value)
            .into_iter()
            .any(|child| has_unknown_value_effect(store, child)),
    }
}

/// Summarizes the effects of one straight-line statement for scalar load CSE.
///
/// Nested control flow is a scope boundary and therefore an unknown barrier.
/// `StoreVar` is also a barrier: although ordinary local writes do not alias a
/// table today, this conservative rule prevents the cache from becoming an
/// accidental data-flow/scheduling pass.
fn scalar_load_effects(store: &FirStore, stmt: FirId) -> Vec<ScalarLoadEffect> {
    let matched = match_fir(store, stmt);
    match matched {
        FirMatch::DeclareVar {
            init: Some(init), ..
        }
        | FirMatch::Drop(init)
        | FirMatch::Return(Some(init)) => {
            if has_unknown_value_effect(store, init) {
                vec![ScalarLoadEffect::UnknownBarrier]
            } else if let FirMatch::LoadTable {
                name,
                access,
                index,
                ..
            } = match_fir(store, init)
            {
                vec![ScalarLoadEffect::ReadsTable(TableLocation {
                    name,
                    access,
                    index: canonical_table_index(store, index),
                })]
            } else {
                Vec::new()
            }
        }
        FirMatch::StoreTable {
            name,
            access,
            index,
            value,
        } => {
            let mut effects = Vec::new();
            if has_unknown_value_effect(store, index) || has_unknown_value_effect(store, value) {
                effects.push(ScalarLoadEffect::UnknownBarrier);
            }
            effects.push(ScalarLoadEffect::WritesTable(TableLocation {
                name,
                access,
                index: canonical_table_index(store, index),
            }));
            effects
        }
        FirMatch::ShiftArrayVar { name, access, .. } => {
            vec![ScalarLoadEffect::WritesTable(TableLocation {
                name,
                access,
                index: CanonicalTableIndex::Unknown,
            })]
        }
        FirMatch::StoreVar { .. }
        | FirMatch::If { .. }
        | FirMatch::Control { .. }
        | FirMatch::Block(_)
        | FirMatch::ForLoop { .. }
        | FirMatch::SimpleForLoop { .. }
        | FirMatch::IteratorForLoop { .. }
        | FirMatch::WhileLoop { .. }
        | FirMatch::DeclareFun { .. } => vec![ScalarLoadEffect::UnknownBarrier],
        _ => Vec::new(),
    }
}

// ─── CSE materialization ────────────────────────────────────────────────────

/// Materializes multi-referenced value nodes into temporary variables, **per
/// execution scope**.
///
/// `statements` is the flat statement list of one scope (a tier bucket, a block
/// body, a loop body, …).  Non-trivial expressions referenced ≥ 2 times *within
/// this scope* are wrapped in typed `DeclareVar(prefix<N>)` + `LoadVar(prefix<N>)`.
///
/// Statements that carry a nested body (`If`/`Control`/loops/`Block`) are
/// recursed into as their **own** scopes, so a shared value inside a guarded
/// `ondemand` block is materialized locally to that block — the case the old
/// flat pass silently missed, which made in-block FFTs emit as O(N²·ᐟ) inlined
/// trees instead of the O(N log N) shared DAG.  See
/// `porting/fft-scalability-cse-in-clocked-blocks-2026-07-09-en.md`.
///
/// Declarations are inserted **at the point of first use** (preserving
/// sequential data dependencies), and the `prefix`/counter pair is threaded
/// through the whole scope tree so temp names stay unique.  `float_start_counter`
/// / `int_start_counter` let the naming pick up where a prior pass left off.
pub(super) fn materialize_shared_values(
    store: &mut FirStore,
    statements: &mut Vec<FirId>,
    float_prefix: &str,
    float_start_counter: u32,
    int_prefix: &str,
    int_start_counter: u32,
) {
    let mut counters = TypedCounters {
        float_prefix,
        float_counter: float_start_counter,
        int_prefix,
        int_counter: int_start_counter,
    };
    materialize_scope(store, statements, &mut counters);
}

/// CSE-materializes one execution scope (a flat statement list), recursing into
/// any nested block/loop bodies as independent scopes.
fn materialize_scope(
    store: &mut FirStore,
    statements: &mut Vec<FirId>,
    counters: &mut TypedCounters<'_>,
) {
    // Reference counts are computed over *this* scope only: `value_children_of`
    // does not descend into nested bodies, so a node shared inside a guarded
    // block is not counted here — it is counted (and materialized) when we
    // recurse into that block below.
    let ref_counts = count_fir_value_uses(store, statements);
    let mut state = RewriteState {
        ref_counts: &ref_counts,
        materialized: HashMap::new(),
        temp_decls: Vec::new(),
    };
    let mut result = Vec::with_capacity(statements.len());

    for &stmt in statements.iter() {
        state.temp_decls.clear();
        let rewritten = rewrite_stmt(store, stmt, &mut state, counters);
        // Insert declarations immediately before the statement that first
        // triggers them, so they see all prior state stores.
        result.extend(state.temp_decls.iter().copied());
        result.push(rewritten);
    }

    *statements = result;
}

/// Recurses CSE into a nested body (a `Block` or a single statement), returning
/// the rewritten body.  Temporaries materialized here are declared **inside**
/// the body, so they are correctly scoped to it (never hoisted across the
/// guarding condition).
fn rewrite_scope_body(
    store: &mut FirStore,
    body: FirId,
    counters: &mut TypedCounters<'_>,
) -> FirId {
    match match_fir(store, body) {
        FirMatch::Block(mut stmts) => {
            materialize_scope(store, &mut stmts, counters);
            FirBuilder::new(store).block(&stmts)
        }
        // A single (non-block) statement as its own one-element scope. If CSE
        // adds a declaration, the body must become a block to hold it.
        _ => {
            let mut stmts = vec![body];
            materialize_scope(store, &mut stmts, counters);
            if stmts.len() == 1 {
                stmts[0]
            } else {
                FirBuilder::new(store).block(&stmts)
            }
        }
    }
}

/// Rewrites a statement node by rewriting its value children.
///
/// Statements themselves are never CSE candidates — only their embedded value
/// sub-expressions are.  This function decodes the statement, rewrites each
/// value child via [`rewrite_value`], and rebuilds the statement if any child
/// changed.
fn rewrite_stmt(
    store: &mut FirStore,
    stmt: FirId,
    state: &mut RewriteState<'_>,
    counters: &mut TypedCounters<'_>,
) -> FirId {
    let matched = match_fir(store, stmt);
    match matched {
        FirMatch::StoreVar {
            name,
            access,
            value,
        } => {
            let nv = rewrite_value(store, value, state, counters);
            if nv == value {
                return stmt;
            }
            FirBuilder::new(store).store_var(name, access, nv)
        }
        FirMatch::StoreTable {
            name,
            access,
            index,
            value,
        } => {
            let ni = rewrite_value(store, index, state, counters);
            let nv = rewrite_value(store, value, state, counters);
            if ni == index && nv == value {
                return stmt;
            }
            FirBuilder::new(store).store_table(name, access, ni, nv)
        }
        FirMatch::DeclareVar {
            name,
            typ,
            access,
            init: Some(init),
        } => {
            let ni = rewrite_value(store, init, state, counters);
            if ni == init {
                return stmt;
            }
            FirBuilder::new(store).declare_var(name, typ, access, Some(ni))
        }
        FirMatch::Drop(value) => {
            let nv = rewrite_value(store, value, state, counters);
            if nv == value {
                return stmt;
            }
            FirBuilder::new(store).drop_(nv)
        }

        // ── Nested scopes: recurse CSE into their bodies (Phase A). ──
        // The guard condition / loop header are left untouched (they belong to
        // the enclosing scope and, for loops, may be re-evaluated); only the
        // bodies are rewritten, each as its own materialization scope.
        FirMatch::If {
            cond,
            then_block,
            else_block,
        } => {
            let nthen = rewrite_scope_body(store, then_block, counters);
            let nelse = else_block.map(|e| rewrite_scope_body(store, e, counters));
            FirBuilder::new(store).if_(cond, nthen, nelse)
        }
        FirMatch::Control { cond, stmt: inner } => {
            let ninner = rewrite_scope_body(store, inner, counters);
            FirBuilder::new(store).control(cond, ninner)
        }
        FirMatch::Block(_) => rewrite_scope_body(store, stmt, counters),
        FirMatch::SimpleForLoop {
            var,
            upper,
            body,
            is_reverse,
        } => {
            let nbody = rewrite_scope_body(store, body, counters);
            FirBuilder::new(store).simple_for_loop(var, upper, nbody, is_reverse)
        }
        FirMatch::ForLoop {
            var,
            init,
            end,
            step,
            body,
            is_reverse,
        } => {
            let nbody = rewrite_scope_body(store, body, counters);
            FirBuilder::new(store).for_loop(var, init, end, step, nbody, is_reverse)
        }
        FirMatch::IteratorForLoop {
            iterators,
            is_reverse,
            body,
        } => {
            let nbody = rewrite_scope_body(store, body, counters);
            let iter_refs: Vec<&str> = iterators.iter().map(String::as_str).collect();
            FirBuilder::new(store).iterator_for_loop(&iter_refs, is_reverse, nbody)
        }
        FirMatch::WhileLoop { cond, body } => {
            let nbody = rewrite_scope_body(store, body, counters);
            FirBuilder::new(store).while_loop(cond, nbody)
        }

        // Statements without rewritable value children pass through unchanged.
        _ => stmt,
    }
}

/// Rewrites a value node bottom-up, materializing multi-referenced sub-trees.
///
/// If `node` was already materialized in a prior encounter, returns a `LoadVar`
/// reference.  Otherwise rewrites children first (bottom-up), then checks
/// whether `node` itself qualifies for materialization (ref_count ≥ 2 and
/// non-trivial).
fn rewrite_value(
    store: &mut FirStore,
    node: FirId,
    state: &mut RewriteState<'_>,
    counters: &mut TypedCounters<'_>,
) -> FirId {
    // Already materialized → return LoadVar reference.
    if let Some((name, typ)) = state.materialized.get(&node).cloned() {
        return FirBuilder::new(store).load_var(name, AccessType::Stack, typ);
    }

    // Rewrite children first (bottom-up).
    let rewritten = rewrite_value_children(store, node, state, counters);

    // Candidate for materialization?
    if state.ref_counts.get(&node).copied().unwrap_or(0) >= 2 && !is_trivial_value(store, node) {
        let typ = store.value_type(rewritten).unwrap_or(FirType::Void);
        let (prefix, counter) = typed_prefix_for(&typ, counters);
        let name = format!("{prefix}{counter}");
        *counter += 1;
        let decl = FirBuilder::new(store).declare_var(
            &name,
            typ.clone(),
            AccessType::Stack,
            Some(rewritten),
        );
        state.temp_decls.push(decl);

        state.materialized.insert(node, (name.clone(), typ.clone()));
        return FirBuilder::new(store).load_var(name, AccessType::Stack, typ);
    }

    rewritten
}

/// Rewrites the value children of a value node and rebuilds it if any changed.
fn rewrite_value_children(
    store: &mut FirStore,
    node: FirId,
    state: &mut RewriteState<'_>,
    counters: &mut TypedCounters<'_>,
) -> FirId {
    let matched = match_fir(store, node);
    match matched {
        FirMatch::BinOp {
            op, lhs, rhs, typ, ..
        } => {
            let nl = rewrite_value(store, lhs, state, counters);
            let nr = rewrite_value(store, rhs, state, counters);
            if nl == lhs && nr == rhs {
                return node;
            }
            FirBuilder::new(store).binop(op, nl, nr, typ)
        }
        FirMatch::Neg { value, typ } => {
            let nv = rewrite_value(store, value, state, counters);
            if nv == value {
                return node;
            }
            FirBuilder::new(store).neg(nv, typ)
        }
        FirMatch::Cast { typ, value } => {
            let nv = rewrite_value(store, value, state, counters);
            if nv == value {
                return node;
            }
            FirBuilder::new(store).cast(typ, nv)
        }
        FirMatch::Bitcast { typ, value } => {
            let nv = rewrite_value(store, value, state, counters);
            if nv == value {
                return node;
            }
            FirBuilder::new(store).bitcast(typ, nv)
        }
        FirMatch::Select2 {
            cond,
            then_value,
            else_value,
            typ,
        } => {
            let nc = rewrite_value(store, cond, state, counters);
            let nt = rewrite_value(store, then_value, state, counters);
            let ne = rewrite_value(store, else_value, state, counters);
            if nc == cond && nt == then_value && ne == else_value {
                return node;
            }
            FirBuilder::new(store).select2(nc, nt, ne, typ)
        }
        FirMatch::FunCall { name, args, typ } => {
            let mut changed = false;
            let new_args: Vec<FirId> = args
                .iter()
                .map(|&a| {
                    let na = rewrite_value(store, a, state, counters);
                    if na != a {
                        changed = true;
                    }
                    na
                })
                .collect();
            if !changed {
                return node;
            }
            FirBuilder::new(store).fun_call(name, &new_args, typ)
        }
        FirMatch::LoadTable {
            name,
            access,
            index,
            typ,
        } => {
            let ni = rewrite_value(store, index, state, counters);
            if ni == index {
                return node;
            }
            FirBuilder::new(store).load_table(name, access, ni, typ)
        }
        FirMatch::TeeVar {
            name,
            access,
            value,
            typ,
        } => {
            let nv = rewrite_value(store, value, state, counters);
            if nv == value {
                return node;
            }
            FirBuilder::new(store).tee_var(name, access, nv, typ)
        }
        // Leaf / trivial nodes — no children to rewrite.
        _ => node,
    }
}

/// Returns the typed prefix and counter slot for one materialized value.
fn typed_prefix_for<'a>(
    typ: &FirType,
    counters: &'a mut TypedCounters<'_>,
) -> (&'a str, &'a mut u32) {
    if matches!(typ, FirType::Int32 | FirType::Int64 | FirType::Bool) {
        (counters.int_prefix, &mut counters.int_counter)
    } else {
        (counters.float_prefix, &mut counters.float_counter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fir::{FirBinOp, match_fir};

    /// Builds the straight-line state-history shape used by the load-reuse
    /// fixtures.  It is intentionally FIR-only: the safety contract is about
    /// ordered table reads and writes, rather than about one corpus DSP.
    fn recursive_history_fixture(store: &mut FirStore) -> Vec<FirId> {
        let mut b = FirBuilder::new(store);
        let zero = b.int32(0);
        let one = b.int32(1);
        let two = b.int32(2);
        let read_previous = b.load_table("state", AccessType::Struct, one, FirType::Float32);
        let first = b.declare_var(
            "fTemp0",
            FirType::Float32,
            AccessType::Stack,
            Some(read_previous),
        );
        let update_current = b.store_table("state", AccessType::Struct, zero, read_previous);
        let second_read = b.load_table("state", AccessType::Struct, one, FirType::Float32);
        let second = b.declare_var(
            "fTemp1",
            FirType::Float32,
            AccessType::Stack,
            Some(second_read),
        );
        let shift_history = b.store_table("state", AccessType::Struct, two, read_previous);
        let commit_current = b.store_table("state", AccessType::Struct, one, read_previous);
        vec![first, update_current, second, shift_history, commit_current]
    }

    #[test]
    fn integer_shared_value_uses_itemp_prefix() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let one = b.int32(1);
        let sum = b.binop(FirBinOp::Add, one, one, FirType::Int32);
        let product = b.binop(FirBinOp::Mul, sum, sum, FirType::Int32);
        let stmt = b.drop_(product);
        let mut statements = vec![stmt];

        materialize_shared_values(&mut store, &mut statements, "fTemp", 0, "iTemp", 0);

        assert_eq!(
            statements.len(),
            2,
            "CSE should insert one temp declaration"
        );
        assert!(matches!(
            match_fir(&store, statements[0]),
            FirMatch::DeclareVar {
                ref name,
                access: AccessType::Stack,
                typ: FirType::Int32,
                ..
            } if name == "iTemp0"
        ));
    }

    #[test]
    fn recursive_history_fixture_keeps_ordered_state_shift_visible() {
        let mut store = FirStore::new();
        let mut statements = recursive_history_fixture(&mut store);

        // The ordinary expression CSE pass must not reorder or hide state
        // history updates.  The later state-aware pass is allowed to reuse the
        // two reads only after proving that the write targets index zero.
        materialize_shared_values(&mut store, &mut statements, "fTemp", 2, "iTemp", 0);

        assert_eq!(statements.len(), 5);
        assert!(matches!(
            match_fir(&store, statements[3]),
            FirMatch::StoreTable { ref name, index, .. }
                if name == "state" && matches!(match_fir(&store, index), FirMatch::Int32 { value: 2, .. })
        ));
        assert!(matches!(
            match_fir(&store, statements[4]),
            FirMatch::StoreTable { ref name, index, .. }
                if name == "state" && matches!(match_fir(&store, index), FirMatch::Int32 { value: 1, .. })
        ));
    }

    #[test]
    fn scalar_load_effects_distinguish_exact_and_dynamic_table_writes() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let one = b.int32(1);
        let dynamic = b.load_var("i", AccessType::Stack, FirType::Int32);
        let read = b.load_table("state", AccessType::Struct, one, FirType::Float32);
        let read_stmt = b.declare_var("fTemp0", FirType::Float32, AccessType::Stack, Some(read));
        let exact_write = b.store_table("state", AccessType::Struct, one, read);
        let dynamic_write = b.store_table("state", AccessType::Struct, dynamic, read);

        assert_eq!(
            scalar_load_effects(&store, read_stmt),
            vec![ScalarLoadEffect::ReadsTable(TableLocation {
                name: "state".to_string(),
                access: AccessType::Struct,
                index: CanonicalTableIndex::Constant(1),
            })]
        );
        assert_eq!(
            scalar_load_effects(&store, exact_write),
            vec![ScalarLoadEffect::WritesTable(TableLocation {
                name: "state".to_string(),
                access: AccessType::Struct,
                index: CanonicalTableIndex::Constant(1),
            })]
        );
        assert_eq!(
            scalar_load_effects(&store, dynamic_write),
            vec![ScalarLoadEffect::WritesTable(TableLocation {
                name: "state".to_string(),
                access: AccessType::Struct,
                index: CanonicalTableIndex::Unknown,
            })]
        );
    }

    #[test]
    fn scalar_load_effects_treat_calls_and_nested_control_as_barriers() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let call = b.fun_call("foreign", &[], FirType::Float32);
        let call_stmt = b.drop_(call);
        let cond = b.bool_(true);
        let body = b.null_statement();
        let guarded = b.if_(cond, body, None);

        assert_eq!(
            scalar_load_effects(&store, call_stmt),
            vec![ScalarLoadEffect::UnknownBarrier]
        );
        assert_eq!(
            scalar_load_effects(&store, guarded),
            vec![ScalarLoadEffect::UnknownBarrier]
        );
    }
}
