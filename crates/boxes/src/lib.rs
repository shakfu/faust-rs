//! Box construction helpers backed by `tlib::TreeArena`.
//!
//! # Source provenance (C++)
//! - `compiler/boxes/boxes.hh`
//! - `compiler/boxes/boxes.cpp`
//!
//! # Public API mapping status
//! - `1:1`: `box_ident`, `box_int`, `box_real`, `box_wire`, `box_cut`,
//!   `box_seq`, `box_par`, `box_rec`, `box_split`, `box_merge`,
//!   `box_ipar`, `box_with_local_def`, `box_environment`, `box_hslider`
//! - `adapted`: `box_with_rec_def` (see function-level note)
//!
//! # Parity invariants
//! - Box nodes are represented as tagged trees with deterministic child order.
//! - Labels/identifiers are carried as `NodeKind::Symbol`.
//! - UI slider parameter payload keeps Faust list encoding (`list4(cur,min,max,step)`).

use tlib::{NodeKind, TreeArena, TreeId};

pub const CRATE_NAME: &str = "boxes";

/// Box node identifier in `TreeArena`.
pub type BoxId = TreeId;

const BOX_IDENT_TAG: &str = "BOXIDENT";
const BOX_WIRE_TAG: &str = "BOXWIRE";
const BOX_CUT_TAG: &str = "BOXCUT";
const BOX_SEQ_TAG: &str = "BOXSEQ";
const BOX_PAR_TAG: &str = "BOXPAR";
const BOX_REC_TAG: &str = "BOXREC";
const BOX_SPLIT_TAG: &str = "BOXSPLIT";
const BOX_MERGE_TAG: &str = "BOXMERGE";
const BOX_IPAR_TAG: &str = "BOXIPAR";
const BOX_WITH_LOCAL_DEF_TAG: &str = "BOXWITHLOCALDEF";
const BOX_WITH_REC_DEF_TAG: &str = "BOXWITHRECDEF";
const BOX_ENVIRONMENT_TAG: &str = "BOXENVIRONMENT";
const BOX_HSLIDER_TAG: &str = "BOXHSLIDER";

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

/// Equivalent to C++ `boxIdent(const char*)`.
#[must_use]
pub fn box_ident(arena: &mut TreeArena, name: &str) -> BoxId {
    let sym = arena.symbol(name);
    intern_tag(arena, BOX_IDENT_TAG, &[sym])
}

/// Returns identifier symbol name when `b` is `box_ident`.
#[must_use]
pub fn box_ident_name(arena: &TreeArena, b: BoxId) -> Option<&str> {
    let [sym] = match_tag_arity(arena, b, BOX_IDENT_TAG, 1)? else {
        return None;
    };
    match arena.kind(*sym) {
        Some(NodeKind::Symbol(name)) => Some(name.as_ref()),
        _ => None,
    }
}

/// Equivalent to C++ `boxInt`.
#[must_use]
pub fn box_int(arena: &mut TreeArena, value: i64) -> BoxId {
    arena.int(value)
}

/// Equivalent to C++ `boxReal`.
#[must_use]
pub fn box_real(arena: &mut TreeArena, value: f64) -> BoxId {
    arena.float(value)
}

/// Predicate equivalent to C++ `isBoxInt`.
#[must_use]
pub fn is_box_int(arena: &TreeArena, b: BoxId) -> bool {
    matches!(arena.kind(b), Some(NodeKind::Int(_)))
}

/// Predicate equivalent to C++ `isBoxReal`.
#[must_use]
pub fn is_box_real(arena: &TreeArena, b: BoxId) -> bool {
    matches!(arena.kind(b), Some(NodeKind::FloatBits(_)))
}

/// Equivalent to C++ `boxWire`.
#[must_use]
pub fn box_wire(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_WIRE_TAG, &[])
}

/// Equivalent to C++ `boxCut`.
#[must_use]
pub fn box_cut(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_CUT_TAG, &[])
}

/// Predicate equivalent to C++ `isBoxWire`.
#[must_use]
pub fn is_box_wire(arena: &TreeArena, b: BoxId) -> bool {
    match_tag_arity(arena, b, BOX_WIRE_TAG, 0).is_some()
}

/// Predicate equivalent to C++ `isBoxCut`.
#[must_use]
pub fn is_box_cut(arena: &TreeArena, b: BoxId) -> bool {
    match_tag_arity(arena, b, BOX_CUT_TAG, 0).is_some()
}

/// Equivalent to C++ `boxSeq`.
#[must_use]
pub fn box_seq(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    intern_tag(arena, BOX_SEQ_TAG, &[left, right])
}

/// Equivalent to C++ `boxPar`.
#[must_use]
pub fn box_par(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    intern_tag(arena, BOX_PAR_TAG, &[left, right])
}

/// Equivalent to C++ `boxRec`.
#[must_use]
pub fn box_rec(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    intern_tag(arena, BOX_REC_TAG, &[left, right])
}

/// Equivalent to C++ `boxSplit`.
#[must_use]
pub fn box_split(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    intern_tag(arena, BOX_SPLIT_TAG, &[left, right])
}

/// Equivalent to C++ `boxMerge`.
#[must_use]
pub fn box_merge(arena: &mut TreeArena, left: BoxId, right: BoxId) -> BoxId {
    intern_tag(arena, BOX_MERGE_TAG, &[left, right])
}

/// Returns `(left, right)` when `b` is `box_seq`.
#[must_use]
pub fn is_box_seq(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_SEQ_TAG)
}

/// Returns `(left, right)` when `b` is `box_par`.
#[must_use]
pub fn is_box_par(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_PAR_TAG)
}

/// Returns `(left, right)` when `b` is `box_rec`.
#[must_use]
pub fn is_box_rec(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_REC_TAG)
}

/// Returns `(left, right)` when `b` is `box_split`.
#[must_use]
pub fn is_box_split(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_SPLIT_TAG)
}

/// Returns `(left, right)` when `b` is `box_merge`.
#[must_use]
pub fn is_box_merge(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_MERGE_TAG)
}

/// Equivalent to C++ `boxIPar`.
#[must_use]
pub fn box_ipar(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_IPAR_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `box_ipar`.
#[must_use]
pub fn is_box_ipar(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    let [index, count, body] = match_tag_arity(arena, b, BOX_IPAR_TAG, 3)? else {
        return None;
    };
    Some((*index, *count, *body))
}

/// Equivalent to C++ `boxWithLocalDef`.
#[must_use]
pub fn box_with_local_def(arena: &mut TreeArena, body: BoxId, ldef: BoxId) -> BoxId {
    intern_tag(arena, BOX_WITH_LOCAL_DEF_TAG, &[body, ldef])
}

/// Returns `(body, ldef)` when `b` is `box_with_local_def`.
#[must_use]
pub fn is_box_with_local_def(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_WITH_LOCAL_DEF_TAG)
}

/// Adapted representation for C++ `boxWithRecDef`.
///
/// C++ performs an immediate lowering/expansion into a local-definition structure.
/// For the current parser prototype, Rust stores an explicit node preserving the three
/// inputs `(body, ldef, ldef2)`. This keeps parser output deterministic and lets later
/// phases choose where lowering happens.
#[must_use]
pub fn box_with_rec_def(arena: &mut TreeArena, body: BoxId, ldef: BoxId, ldef2: BoxId) -> BoxId {
    intern_tag(arena, BOX_WITH_REC_DEF_TAG, &[body, ldef, ldef2])
}

/// Returns `(body, ldef, ldef2)` when `b` is `box_with_rec_def`.
#[must_use]
pub fn is_box_with_rec_def(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    let [body, ldef, ldef2] = match_tag_arity(arena, b, BOX_WITH_REC_DEF_TAG, 3)? else {
        return None;
    };
    Some((*body, *ldef, *ldef2))
}

/// Equivalent to C++ `boxEnvironment`.
#[must_use]
pub fn box_environment(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ENVIRONMENT_TAG, &[])
}

/// Predicate equivalent to C++ `isBoxEnvironment`.
#[must_use]
pub fn is_box_environment(arena: &TreeArena, b: BoxId) -> bool {
    match_tag_arity(arena, b, BOX_ENVIRONMENT_TAG, 0).is_some()
}

/// Equivalent to C++ `boxHSlider`.
///
/// C++ payload encoding is preserved:
/// `tree(BOXHSLIDER, label, list4(cur,min,max,step))`.
#[must_use]
pub fn box_hslider(
    arena: &mut TreeArena,
    label: BoxId,
    cur: BoxId,
    min: BoxId,
    max: BoxId,
    step: BoxId,
) -> BoxId {
    let params = list4(arena, cur, min, max, step);
    intern_tag(arena, BOX_HSLIDER_TAG, &[label, params])
}

/// Returns `(label, cur, min, max, step)` when `b` is `box_hslider`.
#[must_use]
pub fn is_box_hslider(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId, BoxId, BoxId)> {
    let [label, params] = match_tag_arity(arena, b, BOX_HSLIDER_TAG, 2)? else {
        return None;
    };
    let cur = list_nth(arena, *params, 0)?;
    let min = list_nth(arena, *params, 1)?;
    let max = list_nth(arena, *params, 2)?;
    let step = list_nth(arena, *params, 3)?;
    Some((*label, cur, min, max, step))
}

fn intern_tag(arena: &mut TreeArena, tag: &str, children: &[BoxId]) -> BoxId {
    arena.intern(NodeKind::Tag(tag.into()), children)
}

fn match_tag_arity<'a>(
    arena: &'a TreeArena,
    b: BoxId,
    tag: &str,
    arity: usize,
) -> Option<&'a [BoxId]> {
    let children = arena.children(b)?;
    if children.len() != arity {
        return None;
    }
    match arena.kind(b) {
        Some(NodeKind::Tag(actual)) if actual.as_ref() == tag => Some(children),
        _ => None,
    }
}

fn match_binary(arena: &TreeArena, b: BoxId, tag: &str) -> Option<(BoxId, BoxId)> {
    let [left, right] = match_tag_arity(arena, b, tag, 2)? else {
        return None;
    };
    Some((*left, *right))
}

fn list4(arena: &mut TreeArena, a: BoxId, b: BoxId, c: BoxId, d: BoxId) -> BoxId {
    let nil = arena.nil();
    let l3 = arena.cons(d, nil);
    let l2 = arena.cons(c, l3);
    let l1 = arena.cons(b, l2);
    arena.cons(a, l1)
}

fn list_nth(arena: &TreeArena, mut list: BoxId, mut n: usize) -> Option<BoxId> {
    loop {
        if arena.is_nil(list) {
            return None;
        }
        let node = arena.node(list)?;
        if !matches!(node.kind, NodeKind::Cons) || node.children.len() != 2 {
            return None;
        }
        let head = node.children.get(0)?;
        let tail = node.children.get(1)?;
        if n == 0 {
            return Some(head);
        }
        n -= 1;
        list = tail;
    }
}
