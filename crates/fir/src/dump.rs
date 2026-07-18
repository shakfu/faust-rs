//! Deterministic FIR structural dump support.
//!
//! Dumps are intended for diagnostics and differential guardrails. Traversal
//! follows semantic FIR children and leaves encoded type/access atoms implicit
//! because `match_fir` reconstructs them.

use super::*;
use std::collections::HashMap;

/// Deterministic structural dump helper for FIR differential checks.
///
/// The dump is rooted at `root` and recursively expands child FIR ids.
#[must_use]
pub fn dump_fir(store: &FirStore, root: FirId) -> String {
    let mut out = String::new();
    let mut seen = HashSet::new();
    dump_node(store, root, 0, &mut out, &mut seen);
    out
}

/// Returns an allocation-independent structural encoding of reachable FIR.
///
/// Unlike [`dump_fir`], this encoding traverses the complete internal tree,
/// including type and access atoms, and assigns local node numbers in ordered
/// preorder. Arena-local [`FirId`] values and interned numeric tag ids therefore
/// cannot affect the result. Sharing remains observable because repeated edges
/// reference the same local node number.
///
/// This is suitable for cache identity. Human diagnostics should continue to
/// use [`dump_fir`], whose original ids are useful when inspecting a store.
#[must_use]
pub fn canonical_fir_fingerprint(store: &FirStore, root: FirId) -> String {
    let mut out = String::new();
    let mut labels = HashMap::new();
    labels.insert(root, 0_u32);
    let mut emitted = HashSet::new();
    fingerprint_node(store, root, &mut labels, &mut emitted, &mut out);
    out
}

fn fingerprint_node(
    store: &FirStore,
    id: FirId,
    labels: &mut HashMap<FirId, u32>,
    emitted: &mut HashSet<FirId>,
    out: &mut String,
) {
    if !emitted.insert(id) {
        return;
    }
    let node = store
        .arena
        .node(id)
        .expect("FIR fingerprint root and children must belong to the store");
    let children = node.children.as_slice();
    let child_labels: Vec<u32> = children
        .iter()
        .map(|child| {
            let next = u32::try_from(labels.len()).expect("FIR fingerprint exceeds u32::MAX nodes");
            *labels.entry(*child).or_insert(next)
        })
        .collect();
    let label = labels[&id];

    let _ = write!(out, "@{label}=");
    write_canonical_kind(store, &node.kind, out);
    out.push('[');
    for (index, child_label) in child_labels.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "@{child_label}");
    }
    out.push_str("]\n");

    for child in children {
        fingerprint_node(store, *child, labels, emitted, out);
    }
}

fn write_canonical_kind(store: &FirStore, kind: &NodeKind, out: &mut String) {
    match kind {
        NodeKind::Nil => out.push_str("Nil"),
        NodeKind::Cons => out.push_str("Cons"),
        NodeKind::Symbol(value) => {
            let _ = write!(out, "Symbol({value:?})");
        }
        NodeKind::StringLiteral(value) => {
            let _ = write!(out, "StringLiteral({value:?})");
        }
        NodeKind::Int(value) => {
            let _ = write!(out, "Int({value})");
        }
        NodeKind::FloatBits(bits) => {
            let _ = write!(out, "FloatBits(0x{bits:016x})");
        }
        NodeKind::Tag(tag) => {
            let name = store
                .arena
                .tag_name(*tag)
                .expect("FIR tag id must be interned in the store");
            let _ = write!(out, "Tag({name:?})");
        }
    }
}

fn dump_node(
    store: &FirStore,
    id: FirId,
    depth: usize,
    out: &mut String,
    seen: &mut HashSet<FirId>,
) {
    let indent = "  ".repeat(depth);
    let node = match_fir(store, id);
    let _ = writeln!(out, "{indent}#{} {:?}", id.as_u32(), node);
    if !seen.insert(id) {
        return;
    }
    for child in child_ids(&node) {
        dump_node(store, child, depth + 1, out, seen);
    }
}

/// Returns the immediate FIR children that should be traversed structurally.
///
/// This is the canonical edge list used by [`dump_fir`] and similar internal
/// walkers. It follows semantic children only; encoded type/access atoms remain
/// implicit because they are reconstructed by [`match_fir`].
pub(crate) fn child_ids(node: &FirMatch) -> Vec<FirId> {
    match node {
        FirMatch::Unknown
        | FirMatch::Int32 { .. }
        | FirMatch::Int64 { .. }
        | FirMatch::Float32 { .. }
        | FirMatch::Float64 { .. }
        | FirMatch::Bool { .. }
        | FirMatch::Quad { .. }
        | FirMatch::FixedPoint { .. }
        | FirMatch::Int32Array { .. }
        | FirMatch::Float32Array { .. }
        | FirMatch::Float64Array { .. }
        | FirMatch::QuadArray { .. }
        | FirMatch::FixedPointArray { .. }
        | FirMatch::LoadVar { .. }
        | FirMatch::LoadVarAddress { .. }
        | FirMatch::NullValue { .. }
        | FirMatch::NewDsp { .. }
        | FirMatch::DeclareStructType { .. }
        | FirMatch::DeclareBufferIterators { .. }
        | FirMatch::ShiftArrayVar { .. }
        | FirMatch::NullStatement
        | FirMatch::OpenBox { .. }
        | FirMatch::CloseBox
        | FirMatch::AddButton { .. }
        | FirMatch::AddSlider { .. }
        | FirMatch::AddBargraph { .. }
        | FirMatch::AddSoundfile { .. }
        | FirMatch::AddMetaDeclare { .. }
        | FirMatch::Label(_) => Vec::new(),
        FirMatch::LoadSoundfileLength { part, .. } | FirMatch::LoadSoundfileRate { part, .. } => {
            vec![*part]
        }
        FirMatch::LoadSoundfileBuffer {
            chan, part, idx, ..
        } => vec![*chan, *part, *idx],
        FirMatch::ValueArray { values, .. }
        | FirMatch::FunCall { args: values, .. }
        | FirMatch::DeclareTable { values, .. }
        | FirMatch::Block(values) => values.clone(),
        FirMatch::LoadTable { index, .. }
        | FirMatch::TeeVar { value: index, .. }
        | FirMatch::Neg { value: index, .. }
        | FirMatch::Cast { value: index, .. }
        | FirMatch::Bitcast { value: index, .. }
        | FirMatch::StoreVar { value: index, .. }
        | FirMatch::Drop(index) => vec![*index],
        FirMatch::SimpleForLoop { upper, body, .. } => vec![*upper, *body],
        FirMatch::BinOp { lhs, rhs, .. } => vec![*lhs, *rhs],
        FirMatch::Select2 {
            cond,
            then_value,
            else_value,
            ..
        } => vec![*cond, *then_value, *else_value],
        FirMatch::DeclareVar { init, .. } => init.iter().copied().collect(),
        FirMatch::DeclareFun { body: Some(b), .. } => vec![*b],
        FirMatch::DeclareFun { body: None, .. } => vec![],
        FirMatch::StoreTable { index, value, .. } => vec![*index, *value],
        FirMatch::Return(value) => value.iter().copied().collect(),
        FirMatch::If {
            cond,
            then_block,
            else_block,
        } => {
            let mut out = vec![*cond, *then_block];
            out.extend(else_block.iter().copied());
            out
        }
        FirMatch::Control { cond, stmt } => vec![*cond, *stmt],
        FirMatch::ForLoop {
            init,
            end,
            step,
            body,
            ..
        } => vec![*init, *end, *step, *body],
        FirMatch::IteratorForLoop { body, .. } => vec![*body],
        FirMatch::WhileLoop { cond, body } => vec![*cond, *body],
        FirMatch::Switch {
            cond,
            cases,
            default,
        } => {
            let mut out = vec![*cond];
            out.extend(cases.iter().map(|(_, block)| *block));
            out.extend(default.iter().copied());
            out
        }
        FirMatch::Module {
            dsp_struct,
            globals,
            functions,
            static_decls,
            ..
        } => vec![*dsp_struct, *globals, *functions, *static_decls],
    }
}
