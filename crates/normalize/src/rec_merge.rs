//! Pre-simplification pass that detects structurally isomorphic `SYMREC` groups
//! and unifies them to a single canonical representative.
//!
//! This pass enables downstream algebraic simplification (e.g., `x - x → 0`)
//! across signal trees produced by independent constructions like manual versus
//! macroeconomic recursive forms.
//!
//! Exposes `merge_isomorphic_symrec_groups` which must run after `de_bruijn_to_sym`
//! and prior to algebraic simplification passes.

use std::collections::{HashMap, HashSet};

use signals::SigId;
use tlib::{TreeArena, match_sym_rec, match_sym_ref, list_to_vec};

/// Returns a unique sentinel hole token used to track self references in `SYMREC`.
fn gen_hole(arena: &mut TreeArena) -> SigId {
    arena.symbol("__rec_hole__")
}

/// Merges structurally isomorphic `SYMREC` groups into canonical representatives.
///
/// Converts duplicate `SYMREC` groups generating identical output lists
/// (after masking recursive loops within their bodies) into references
/// corresponding to a single, stable canonical `SYMREC`.
pub fn merge_isomorphic_symrec_groups(
    arena: &mut TreeArena,
    outputs: &[SigId],
) -> Vec<SigId> {
    let mut visited = HashSet::new();
    let mut symrecs = Vec::new();
    for &out in outputs {
        collect_symrec_nodes(arena, out, &mut visited, &mut symrecs);
    }

    if symrecs.len() < 2 {
        return outputs.to_vec();
    }

    let hole = gen_hole(arena);
    let mut signatures = HashMap::new();
    
    for &rec in &symrecs {
        if let Some((var, body_list)) = match_sym_rec(arena, rec) {
            let mut cache = HashMap::new();
            let opened = open_symrec(arena, body_list, var, hole, &mut cache);
            signatures.insert(rec, opened);
        }
    }

    let mut signature_groups: HashMap<SigId, Vec<SigId>> = HashMap::new();
    for (rec, sig_id) in signatures {
        signature_groups.entry(sig_id).or_default().push(rec);
    }

    let (rec_map, ref_map) = build_symrec_substitution(arena, &signature_groups);

    if rec_map.is_empty() {
        return outputs.to_vec();
    }

    let mut cache = HashMap::new();
    outputs
        .iter()
        .map(|&root| apply_symrec_substitution(arena, root, &rec_map, &ref_map, &mut cache))
        .collect()
}

/// Recursively identify all reachable `SYMREC` nodes across the tree.
fn collect_symrec_nodes(
    arena: &TreeArena,
    sig: SigId,
    visited: &mut HashSet<SigId>,
    symrecs: &mut Vec<SigId>,
) {
    if !visited.insert(sig) {
        return;
    }
    
    if let Some((_, body)) = match_sym_rec(arena, sig) {
        symrecs.push(sig);
        if let Some(items) = list_to_vec(arena, body) {
            for item in items {
                collect_symrec_nodes(arena, item, visited, symrecs);
            }
        }
    } else if let Some(node) = arena.node(sig) {
        for &child in node.children.as_slice() {
            collect_symrec_nodes(arena, child, visited, symrecs);
        }
    }
}

/// Normalizes a `SYMREC` by tracking references bound to `var` as `hole`s.
fn open_symrec(
    arena: &mut TreeArena,
    sig: SigId,
    var: SigId,
    hole: SigId,
    cache: &mut HashMap<SigId, SigId>,
) -> SigId {
    if let Some(&cached) = cache.get(&sig) {
        return cached;
    }

    let result = if match_sym_ref(arena, sig) == Some(var) {
        hole
    } else if let Some(node) = arena.node(sig) {
        let kind = node.kind.clone();
        let children = node.children.as_slice().to_vec();

        let mut new_children = Vec::with_capacity(children.len());
        for &c in &children {
            new_children.push(open_symrec(arena, c, var, hole, cache));
        }
        arena.intern(kind, &new_children)
    } else {
        sig
    };

    cache.insert(sig, result);
    result
}

/// Picks canonical delegates mapping each duplicate node.
fn build_symrec_substitution(
    arena: &mut TreeArena,
    groups: &HashMap<SigId, Vec<SigId>>,
) -> (HashMap<SigId, SigId>, HashMap<SigId, SigId>) {
    let mut rec_map = HashMap::new();
    let mut ref_map = HashMap::new();

    for members in groups.values() {
        if members.len() > 1 {
            let mut sorted = members.clone();
            sorted.sort_by_key(|id| id.as_u32());
            let canon = sorted[0];

            let canon_var = match_sym_rec(arena, canon).unwrap().0;
            let canon_ref = tlib::sym_ref(arena, canon_var);

            for &dup in &sorted[1..] {
                rec_map.insert(dup, canon);

                let dup_var = match_sym_rec(arena, dup).unwrap().0;
                let dup_ref = tlib::sym_ref(arena, dup_var);
                ref_map.insert(dup_ref, canon_ref);
            }
        }
    }

    (rec_map, ref_map)
}

/// Replaces non-canonical identifiers consistently referencing one source of truth.
fn apply_symrec_substitution(
    arena: &mut TreeArena,
    sig: SigId,
    rec_map: &HashMap<SigId, SigId>,
    ref_map: &HashMap<SigId, SigId>,
    cache: &mut HashMap<SigId, SigId>,
) -> SigId {
    if let Some(&cached) = cache.get(&sig) {
        return cached;
    }

    if let Some(&canon_rec) = rec_map.get(&sig) {
        cache.insert(sig, canon_rec);
        return canon_rec;
    }

    if let Some(&canon_ref) = ref_map.get(&sig) {
        cache.insert(sig, canon_ref);
        return canon_ref;
    }

    let result = if let Some(node) = arena.node(sig) {
        let kind = node.kind.clone();
        let children = node.children.as_slice().to_vec();

        let mut new_children = Vec::with_capacity(children.len());
        for c in children {
            new_children.push(apply_symrec_substitution(
                arena, c, rec_map, ref_map, cache,
            ));
        }
        arena.intern(kind, &new_children)
    } else {
        sig
    };

    cache.insert(sig, result);
    result
}
