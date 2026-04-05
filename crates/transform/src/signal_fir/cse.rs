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

use fir::{match_fir, AccessType, FirBuilder, FirId, FirMatch, FirStore, FirType};

// ─── Reference counting ─────────────────────────────────────────────────────

/// Counts how many times each `FirId` appears as a value child across a bucket.
///
/// `ref_counts[id]` = number of distinct parent references to `id`.  Children
/// are descended only once per unique `FirId` (the `descended` set), so the
/// count reflects fan-out, not tree depth.
pub(super) fn count_fir_value_uses(
    store: &FirStore,
    roots: &[FirId],
) -> HashMap<FirId, usize> {
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
        FirMatch::LoadSoundfileLength { part, .. }
        | FirMatch::LoadSoundfileRate { part, .. } => vec![part],

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
/// variable because they are already free to duplicate (literals, variable
/// loads, null values).
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
            | FirMatch::NullValue { .. }
    )
}

// ─── CSE materialization ────────────────────────────────────────────────────

/// Materializes multi-referenced value nodes into temporary variables within
/// one statement bucket.
///
/// Processes `statements` bottom-up: non-trivial expressions referenced ≥ 2
/// times are wrapped in `DeclareVar(prefix<N>)` + `LoadVar(prefix<N>)`.
///
/// Declarations are inserted **at the point of first use** rather than
/// prepended before all statements.  This preserves sequential data
/// dependencies between statements (e.g. in `constants_statements` where
/// `fConst1` may depend on a struct field stored by the statement that
/// initializes `fConst0`).
///
/// `start_counter` allows the naming to pick up where a prior pass (e.g.
/// variability placement) left off, avoiding name collisions when using the
/// same prefix (e.g. `fConst` / `fSlow`).
pub(super) fn materialize_shared_values(
    store: &mut FirStore,
    statements: &mut Vec<FirId>,
    ref_counts: &HashMap<FirId, usize>,
    prefix: &str,
    start_counter: u32,
) {
    let mut materialized: HashMap<FirId, (String, FirType)> = HashMap::new();
    let mut counter = start_counter;
    let mut result = Vec::with_capacity(statements.len());

    for &stmt in statements.iter() {
        let mut new_decls: Vec<FirId> = Vec::new();
        let rewritten = rewrite_stmt(
            store,
            stmt,
            ref_counts,
            &mut materialized,
            &mut new_decls,
            prefix,
            &mut counter,
        );
        // Insert declarations immediately before the statement that first
        // triggers them, so they see all prior state stores.
        result.extend(new_decls);
        result.push(rewritten);
    }

    *statements = result;
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
    ref_counts: &HashMap<FirId, usize>,
    materialized: &mut HashMap<FirId, (String, FirType)>,
    temp_decls: &mut Vec<FirId>,
    prefix: &str,
    counter: &mut u32,
) -> FirId {
    let matched = match_fir(store, stmt);
    match matched {
        FirMatch::StoreVar {
            name,
            access,
            value,
        } => {
            let nv = rewrite_value(store, value, ref_counts, materialized, temp_decls, prefix, counter);
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
            let ni = rewrite_value(store, index, ref_counts, materialized, temp_decls, prefix, counter);
            let nv = rewrite_value(store, value, ref_counts, materialized, temp_decls, prefix, counter);
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
            let ni = rewrite_value(store, init, ref_counts, materialized, temp_decls, prefix, counter);
            if ni == init {
                return stmt;
            }
            FirBuilder::new(store).declare_var(name, typ, access, Some(ni))
        }
        FirMatch::Drop(value) => {
            let nv = rewrite_value(store, value, ref_counts, materialized, temp_decls, prefix, counter);
            if nv == value {
                return stmt;
            }
            FirBuilder::new(store).drop_(nv)
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
    ref_counts: &HashMap<FirId, usize>,
    materialized: &mut HashMap<FirId, (String, FirType)>,
    temp_decls: &mut Vec<FirId>,
    prefix: &str,
    counter: &mut u32,
) -> FirId {
    // Already materialized → return LoadVar reference.
    if let Some((name, typ)) = materialized.get(&node).cloned() {
        return FirBuilder::new(store).load_var(name, AccessType::Stack, typ);
    }

    // Rewrite children first (bottom-up).
    let rewritten = rewrite_value_children(store, node, ref_counts, materialized, temp_decls, prefix, counter);

    // Candidate for materialization?
    if ref_counts.get(&node).copied().unwrap_or(0) >= 2 && !is_trivial_value(store, node) {
        let name = format!("{prefix}{counter}");
        *counter += 1;

        let typ = store.value_type(rewritten).unwrap_or(FirType::Void);
        let decl =
            FirBuilder::new(store).declare_var(&name, typ.clone(), AccessType::Stack, Some(rewritten));
        temp_decls.push(decl);

        materialized.insert(node, (name.clone(), typ.clone()));
        return FirBuilder::new(store).load_var(name, AccessType::Stack, typ);
    }

    rewritten
}

/// Rewrites the value children of a value node and rebuilds it if any changed.
fn rewrite_value_children(
    store: &mut FirStore,
    node: FirId,
    ref_counts: &HashMap<FirId, usize>,
    materialized: &mut HashMap<FirId, (String, FirType)>,
    temp_decls: &mut Vec<FirId>,
    prefix: &str,
    counter: &mut u32,
) -> FirId {
    let matched = match_fir(store, node);
    match matched {
        FirMatch::BinOp {
            op, lhs, rhs, typ, ..
        } => {
            let nl = rewrite_value(store, lhs, ref_counts, materialized, temp_decls, prefix, counter);
            let nr = rewrite_value(store, rhs, ref_counts, materialized, temp_decls, prefix, counter);
            if nl == lhs && nr == rhs {
                return node;
            }
            FirBuilder::new(store).binop(op, nl, nr, typ)
        }
        FirMatch::Neg { value, typ } => {
            let nv = rewrite_value(store, value, ref_counts, materialized, temp_decls, prefix, counter);
            if nv == value {
                return node;
            }
            FirBuilder::new(store).neg(nv, typ)
        }
        FirMatch::Cast { typ, value } => {
            let nv = rewrite_value(store, value, ref_counts, materialized, temp_decls, prefix, counter);
            if nv == value {
                return node;
            }
            FirBuilder::new(store).cast(typ, nv)
        }
        FirMatch::Bitcast { typ, value } => {
            let nv = rewrite_value(store, value, ref_counts, materialized, temp_decls, prefix, counter);
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
            let nc = rewrite_value(store, cond, ref_counts, materialized, temp_decls, prefix, counter);
            let nt =
                rewrite_value(store, then_value, ref_counts, materialized, temp_decls, prefix, counter);
            let ne =
                rewrite_value(store, else_value, ref_counts, materialized, temp_decls, prefix, counter);
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
                    let na =
                        rewrite_value(store, a, ref_counts, materialized, temp_decls, prefix, counter);
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
            let ni = rewrite_value(store, index, ref_counts, materialized, temp_decls, prefix, counter);
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
            let nv = rewrite_value(store, value, ref_counts, materialized, temp_decls, prefix, counter);
            if nv == value {
                return node;
            }
            FirBuilder::new(store).tee_var(name, access, nv, typ)
        }
        // Leaf / trivial nodes — no children to rewrite.
        _ => node,
    }
}
