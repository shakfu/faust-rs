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
    let mut current_outputs = outputs.to_vec();

    loop {
        let mut visited = HashSet::new();
        let mut symrecs = Vec::new();
        for &out in &current_outputs {
            collect_symrec_nodes(arena, out, &mut visited, &mut symrecs);
        }

        if symrecs.len() < 2 {
            break;
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
            break;
        }

        let mut cache = HashMap::new();
        current_outputs = current_outputs
            .iter()
            .map(|&root| apply_symrec_substitution(arena, root, &rec_map, &ref_map, &mut cache))
            .collect();
    }

    current_outputs
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


#[cfg(test)]
mod tests {
use super::*;
use signals::{SigBuilder, SigMatch, match_sig};
use tlib::{sym_rec, sym_ref, vec_to_list, list_to_vec, TreeArena};
use std::collections::HashMap;

fn arena() -> TreeArena {
    TreeArena::new()
}

fn make_single_rec(a: &mut TreeArena, name: &str, build_body: impl Fn(&mut SigBuilder, SigId) -> SigId) -> SigId {
    let var = a.symbol(name);
    let r = sym_ref(a, var);
    let mut b = SigBuilder::new(a);
    let body = build_body(&mut b, r);
    let list = vec_to_list(a, &[body]);
    sym_rec(a, var, list)
}

#[test]
fn merge_identical_single_output_symrecs() {
    let mut a = arena();
    let rec0 = make_single_rec(&mut a, "W0", |b, rf| { let c = b.int(1); b.sub(c, rf) });
    let rec2 = make_single_rec(&mut a, "W2", |b, rf| { let c = b.int(1); b.sub(c, rf) });

    let outputs = vec![rec0, rec2];
    let merged = merge_isomorphic_symrec_groups(&mut a, &outputs);

    assert_eq!(merged[0], merged[1], "Isomorphic SYMRECs must merge to canonical representative");
}

#[test]
fn merge_identical_multi_output_symrecs() {
    let mut a = arena();
    let build = |a: &mut TreeArena, name: &str| {
        let var = a.symbol(name);
        let r = sym_ref(a, var);
        let mut b = SigBuilder::new(a);
        let b1 = { let c = b.int(1); b.add(r, c) };
        let b2 = { let c = b.int(2); b.mul(r, c) };
        let list = vec_to_list(a, &[b1, b2]);
        sym_rec(a, var, list)
    };

    let rec0 = build(&mut a, "W0");
    let rec2 = build(&mut a, "W2");

    let outputs = vec![rec0, rec2];
    let merged = merge_isomorphic_symrec_groups(&mut a, &outputs);

    assert_eq!(merged[0], merged[1]);
}

#[test]
fn merge_does_not_unify_distinct_symrecs() {
    let mut a = arena();
    let rec0 = make_single_rec(&mut a, "W0", |b, rf| { let c = b.int(1); b.add(rf, c) });
    let rec2 = make_single_rec(&mut a, "W2", |b, rf| { let c = b.int(1); b.mul(rf, c) }); 

    let outputs = vec![rec0, rec2];
    let merged = merge_isomorphic_symrec_groups(&mut a, &outputs);

    assert_ne!(merged[0], merged[1]);
}

#[test]
fn merge_followed_by_simplify_gives_zero() {
    let mut a = arena();
    let rec0 = make_single_rec(&mut a, "W0", |b, rf| { let c = b.int(1); b.add(rf, c) });
    let rec2 = make_single_rec(&mut a, "W2", |b, rf| { let c = b.int(1); b.add(rf, c) });

    let mut b = SigBuilder::new(&mut a);
    let proj0 = b.proj(0, rec0);
    let proj2 = b.proj(0, rec2);
    let sub = b.sub(proj0, proj2);

    let merged_outputs = merge_isomorphic_symrec_groups(&mut a, &[sub]);
    let merged_sub = merged_outputs[0];

    // After merging, proj0 and proj2 refer to the same SYMREC group.
    let simplified = crate::simplify::simplify_const(&mut a, merged_sub);
    
    match match_sig(&a, simplified) {
        SigMatch::Int(0) => {}
        _ => panic!("Expected subtraction of merged isomorphic groups to simplify to 0"),
    }
}

#[test]
fn merge_nested_symrec_groups() {
    let mut a = arena();
    let inner1 = make_single_rec(&mut a, "W10", |b, rf| { let c = b.int(1); b.add(rf, c) });
    let inner2 = make_single_rec(&mut a, "W12", |b, rf| { let c = b.int(1); b.add(rf, c) });

    let outer1 = make_single_rec(&mut a, "W0", |b, rf| b.mul(inner1, rf));
    let outer2 = make_single_rec(&mut a, "W2", |b, rf| b.mul(inner2, rf));

    let outputs = vec![outer1, outer2];
    let merged = merge_isomorphic_symrec_groups(&mut a, &outputs);

    assert_eq!(merged[0], merged[1]);
}

#[test]
fn merge_is_idempotent() {
    let mut a = arena();
    let rec0 = make_single_rec(&mut a, "W0", |b, rf| { let c = b.int(1); b.sub(c, rf) });
    let rec2 = make_single_rec(&mut a, "W2", |b, rf| { let c = b.int(1); b.sub(c, rf) });

    let outputs = vec![rec0, rec2];
    let merged1 = merge_isomorphic_symrec_groups(&mut a, &outputs);
    let merged2 = merge_isomorphic_symrec_groups(&mut a, &merged1);

    assert_eq!(merged1, merged2);
}

#[test]
fn open_symrec_replaces_only_own_symref() {
    let mut a = arena();
    let var1 = a.symbol("W1");
    let ref1 = sym_ref(&mut a, var1);
    
    let hole = a.symbol("__hole__");
    let rec = make_single_rec(&mut a, "W0", |b, rf| b.add(rf, ref1));
    let (var0, body_list) = tlib::match_sym_rec(&a, rec).unwrap();
    
    let mut cache = HashMap::new();
    let opened = super::open_symrec(&mut a, body_list, var0, hole, &mut cache);
    
    let bodies = list_to_vec(&a, opened).unwrap();
    match match_sig(&a, bodies[0]) {
        SigMatch::BinOp(_, left, right) => {
            assert_eq!(left, hole, "Own symref should be replaced by hole");
            assert_eq!(right, ref1, "Other symref should be untouched");
        }
        _ => panic!("Expected BinOp"),
    }
}
}
