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
    counters: TypedCounters<'a>,
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
/// times are wrapped in typed `DeclareVar(prefix<N>)` + `LoadVar(prefix<N>)`.
///
/// Declarations are inserted **at the point of first use** rather than
/// prepended before all statements.  This preserves sequential data
/// dependencies between statements (e.g. in `constants_statements` where
/// `fConst1` may depend on a struct field stored by the statement that
/// initializes `fConst0`).
///
/// `float_start_counter` / `int_start_counter` allow the naming to pick up
/// where a prior pass (e.g. variability placement) left off, avoiding name
/// collisions when using the same prefixes (e.g. `fConst` / `iConst`).
pub(super) fn materialize_shared_values(
    store: &mut FirStore,
    statements: &mut Vec<FirId>,
    ref_counts: &HashMap<FirId, usize>,
    float_prefix: &str,
    float_start_counter: u32,
    int_prefix: &str,
    int_start_counter: u32,
) {
    let mut state = RewriteState {
        ref_counts,
        materialized: HashMap::new(),
        temp_decls: Vec::new(),
        counters: TypedCounters {
            float_prefix,
            float_counter: float_start_counter,
            int_prefix,
            int_counter: int_start_counter,
        },
    };
    let mut result = Vec::with_capacity(statements.len());

    for &stmt in statements.iter() {
        state.temp_decls.clear();
        let rewritten = rewrite_stmt(store, stmt, &mut state);
        // Insert declarations immediately before the statement that first
        // triggers them, so they see all prior state stores.
        result.extend(state.temp_decls.iter().copied());
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
fn rewrite_stmt(store: &mut FirStore, stmt: FirId, state: &mut RewriteState<'_>) -> FirId {
    let matched = match_fir(store, stmt);
    match matched {
        FirMatch::StoreVar {
            name,
            access,
            value,
        } => {
            let nv = rewrite_value(store, value, state);
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
            let ni = rewrite_value(store, index, state);
            let nv = rewrite_value(store, value, state);
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
            let ni = rewrite_value(store, init, state);
            if ni == init {
                return stmt;
            }
            FirBuilder::new(store).declare_var(name, typ, access, Some(ni))
        }
        FirMatch::Drop(value) => {
            let nv = rewrite_value(store, value, state);
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
fn rewrite_value(store: &mut FirStore, node: FirId, state: &mut RewriteState<'_>) -> FirId {
    // Already materialized → return LoadVar reference.
    if let Some((name, typ)) = state.materialized.get(&node).cloned() {
        return FirBuilder::new(store).load_var(name, AccessType::Stack, typ);
    }

    // Rewrite children first (bottom-up).
    let rewritten = rewrite_value_children(store, node, state);

    // Candidate for materialization?
    if state.ref_counts.get(&node).copied().unwrap_or(0) >= 2 && !is_trivial_value(store, node) {
        let typ = store.value_type(rewritten).unwrap_or(FirType::Void);
        let (prefix, counter) = typed_prefix_for(&typ, &mut state.counters);
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
fn rewrite_value_children(store: &mut FirStore, node: FirId, state: &mut RewriteState<'_>) -> FirId {
    let matched = match_fir(store, node);
    match matched {
        FirMatch::BinOp {
            op, lhs, rhs, typ, ..
        } => {
            let nl = rewrite_value(store, lhs, state);
            let nr = rewrite_value(store, rhs, state);
            if nl == lhs && nr == rhs {
                return node;
            }
            FirBuilder::new(store).binop(op, nl, nr, typ)
        }
        FirMatch::Neg { value, typ } => {
            let nv = rewrite_value(store, value, state);
            if nv == value {
                return node;
            }
            FirBuilder::new(store).neg(nv, typ)
        }
        FirMatch::Cast { typ, value } => {
            let nv = rewrite_value(store, value, state);
            if nv == value {
                return node;
            }
            FirBuilder::new(store).cast(typ, nv)
        }
        FirMatch::Bitcast { typ, value } => {
            let nv = rewrite_value(store, value, state);
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
            let nc = rewrite_value(store, cond, state);
            let nt = rewrite_value(store, then_value, state);
            let ne = rewrite_value(store, else_value, state);
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
                    let na = rewrite_value(
                        store,
                        a,
                        state,
                    );
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
            let ni = rewrite_value(store, index, state);
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
            let nv = rewrite_value(store, value, state);
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
fn typed_prefix_for<'a>(typ: &FirType, counters: &'a mut TypedCounters<'_>) -> (&'a str, &'a mut u32) {
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

    #[test]
    fn integer_shared_value_uses_itemp_prefix() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let one = b.int32(1);
        let sum = b.binop(FirBinOp::Add, one, one, FirType::Int32);
        let product = b.binop(FirBinOp::Mul, sum, sum, FirType::Int32);
        let stmt = b.drop_(product);
        let mut statements = vec![stmt];

        let rc = count_fir_value_uses(&store, &statements);
        materialize_shared_values(&mut store, &mut statements, &rc, "fTemp", 0, "iTemp", 0);

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
}
