//! Box construction helpers backed by `tlib::TreeArena`.
//!
//! # Source provenance (C++)
//! - `compiler/boxes/boxes.hh`
//! - `compiler/boxes/boxes.cpp`
//!
//! # Public API mapping status
//! - `1:1`: `box_ident`, `box_int`, `box_real`, `box_wire`, `box_cut`,
//!   `box_seq`, `box_par`, `box_rec`, `box_split`, `box_merge`,
//!   `box_appl`, `box_access`,
//!   `box_add`, `box_sub`, `box_mul`, `box_div`, `box_rem`,
//!   `box_and`, `box_or`, `box_xor`, `box_lsh`, `box_rsh`,
//!   `box_lt`, `box_le`, `box_gt`, `box_ge`, `box_eq`, `box_ne`,
//!   `box_pow`, `box_delay`, `box_delay1`, `box_min`, `box_max`,
//!   `box_ipar`, `box_iseq`, `box_isum`, `box_iprod`,
//!   `box_with_local_def`, `box_environment`, `box_component`, `box_library`,
//!   `box_waveform`, `box_route`,
//!   `box_button`, `box_checkbox`, `box_vslider`, `box_hslider`,
//!   `box_num_entry`, `box_vbargraph`, `box_hbargraph`
//! - `adapted`: `box_with_rec_def` (see function-level note)
//!
//! # Parity invariants
//! - Box nodes are represented as tagged trees with deterministic child order.
//! - Labels/identifiers are carried as `NodeKind::Symbol`.
//! - UI slider parameter payload keeps Faust list encoding (`list4(cur,min,max,step)`).

use std::fmt::Write;

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
const BOX_APPL_TAG: &str = "BOXAPPL";
const BOX_ACCESS_TAG: &str = "BOXACCESS";
const BOX_ADD_TAG: &str = "BOXADD";
const BOX_SUB_TAG: &str = "BOXSUB";
const BOX_MUL_TAG: &str = "BOXMUL";
const BOX_DIV_TAG: &str = "BOXDIV";
const BOX_REM_TAG: &str = "BOXREM";
const BOX_AND_TAG: &str = "BOXAND";
const BOX_OR_TAG: &str = "BOXOR";
const BOX_XOR_TAG: &str = "BOXXOR";
const BOX_LSH_TAG: &str = "BOXLSH";
const BOX_RSH_TAG: &str = "BOXRSH";
const BOX_LT_TAG: &str = "BOXLT";
const BOX_LE_TAG: &str = "BOXLE";
const BOX_GT_TAG: &str = "BOXGT";
const BOX_GE_TAG: &str = "BOXGE";
const BOX_EQ_TAG: &str = "BOXEQ";
const BOX_NE_TAG: &str = "BOXNE";
const BOX_POW_TAG: &str = "BOXPOW";
const BOX_DELAY_TAG: &str = "BOXDELAY";
const BOX_DELAY1_TAG: &str = "BOXDELAY1";
const BOX_MIN_TAG: &str = "BOXMIN";
const BOX_MAX_TAG: &str = "BOXMAX";
const BOX_IPAR_TAG: &str = "BOXIPAR";
const BOX_ISEQ_TAG: &str = "BOXISEQ";
const BOX_ISUM_TAG: &str = "BOXISUM";
const BOX_IPROD_TAG: &str = "BOXIPROD";
const BOX_WITH_LOCAL_DEF_TAG: &str = "BOXWITHLOCALDEF";
const BOX_WITH_REC_DEF_TAG: &str = "BOXWITHRECDEF";
const BOX_ENVIRONMENT_TAG: &str = "BOXENVIRONMENT";
const BOX_COMPONENT_TAG: &str = "BOXCOMPONENT";
const BOX_LIBRARY_TAG: &str = "BOXLIBRARY";
const BOX_WAVEFORM_TAG: &str = "BOXWAVEFORM";
const BOX_ROUTE_TAG: &str = "BOXROUTE";
const BOX_BUTTON_TAG: &str = "BOXBUTTON";
const BOX_CHECKBOX_TAG: &str = "BOXCHECKBOX";
const BOX_VSLIDER_TAG: &str = "BOXVSLIDER";
const BOX_HSLIDER_TAG: &str = "BOXHSLIDER";
const BOX_NUM_ENTRY_TAG: &str = "BOXNUMENTRY";
const BOX_VBARGRAPH_TAG: &str = "BOXVBARGRAPH";
const BOX_HBARGRAPH_TAG: &str = "BOXHBARGRAPH";

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

/// Equivalent to C++ `boxAppl`.
#[must_use]
pub fn box_appl(arena: &mut TreeArena, fun: BoxId, arglist: BoxId) -> BoxId {
    intern_tag(arena, BOX_APPL_TAG, &[fun, arglist])
}

/// Returns `(fun, arglist)` when `b` is `box_appl`.
#[must_use]
pub fn is_box_appl(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_APPL_TAG)
}

/// Equivalent to C++ `boxAccess`.
#[must_use]
pub fn box_access(arena: &mut TreeArena, expr: BoxId, ident: BoxId) -> BoxId {
    intern_tag(arena, BOX_ACCESS_TAG, &[expr, ident])
}

/// Returns `(expr, ident)` when `b` is `box_access`.
#[must_use]
pub fn is_box_access(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId)> {
    match_binary(arena, b, BOX_ACCESS_TAG)
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

/// Equivalent to C++ `boxAdd`.
#[must_use]
pub fn box_add(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_ADD_TAG, &[])
}

/// Equivalent to C++ `boxSub`.
#[must_use]
pub fn box_sub(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_SUB_TAG, &[])
}

/// Equivalent to C++ `boxMul`.
#[must_use]
pub fn box_mul(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_MUL_TAG, &[])
}

/// Equivalent to C++ `boxDiv`.
#[must_use]
pub fn box_div(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_DIV_TAG, &[])
}

/// Equivalent to C++ `boxRem`.
#[must_use]
pub fn box_rem(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_REM_TAG, &[])
}

/// Equivalent to C++ `boxAND`.
#[must_use]
pub fn box_and(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_AND_TAG, &[])
}

/// Equivalent to C++ `boxOR`.
#[must_use]
pub fn box_or(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_OR_TAG, &[])
}

/// Equivalent to C++ `boxXOR`.
#[must_use]
pub fn box_xor(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_XOR_TAG, &[])
}

/// Equivalent to C++ `boxLeftShift`.
#[must_use]
pub fn box_lsh(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LSH_TAG, &[])
}

/// Equivalent to C++ `boxARightShift`.
#[must_use]
pub fn box_rsh(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_RSH_TAG, &[])
}

/// Equivalent to C++ `boxLT`.
#[must_use]
pub fn box_lt(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LT_TAG, &[])
}

/// Equivalent to C++ `boxLE`.
#[must_use]
pub fn box_le(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_LE_TAG, &[])
}

/// Equivalent to C++ `boxGT`.
#[must_use]
pub fn box_gt(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_GT_TAG, &[])
}

/// Equivalent to C++ `boxGE`.
#[must_use]
pub fn box_ge(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_GE_TAG, &[])
}

/// Equivalent to C++ `boxEQ`.
#[must_use]
pub fn box_eq(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_EQ_TAG, &[])
}

/// Equivalent to C++ `boxNE`.
#[must_use]
pub fn box_ne(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_NE_TAG, &[])
}

/// Equivalent to C++ `boxPow`.
#[must_use]
pub fn box_pow(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_POW_TAG, &[])
}

/// Equivalent to C++ `boxDelay`.
#[must_use]
pub fn box_delay(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_DELAY_TAG, &[])
}

/// Equivalent to C++ `boxDelay1`.
#[must_use]
pub fn box_delay1(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_DELAY1_TAG, &[])
}

/// Equivalent to C++ `boxMin`.
#[must_use]
pub fn box_min(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_MIN_TAG, &[])
}

/// Equivalent to C++ `boxMax`.
#[must_use]
pub fn box_max(arena: &mut TreeArena) -> BoxId {
    intern_tag(arena, BOX_MAX_TAG, &[])
}

macro_rules! define_is_prim {
    ($fn_name:ident, $tag:ident) => {
        #[must_use]
        pub fn $fn_name(arena: &TreeArena, b: BoxId) -> bool {
            match_tag_arity(arena, b, $tag, 0).is_some()
        }
    };
}

define_is_prim!(is_box_add, BOX_ADD_TAG);
define_is_prim!(is_box_sub, BOX_SUB_TAG);
define_is_prim!(is_box_mul, BOX_MUL_TAG);
define_is_prim!(is_box_div, BOX_DIV_TAG);
define_is_prim!(is_box_rem, BOX_REM_TAG);
define_is_prim!(is_box_and, BOX_AND_TAG);
define_is_prim!(is_box_or, BOX_OR_TAG);
define_is_prim!(is_box_xor, BOX_XOR_TAG);
define_is_prim!(is_box_lsh, BOX_LSH_TAG);
define_is_prim!(is_box_rsh, BOX_RSH_TAG);
define_is_prim!(is_box_lt, BOX_LT_TAG);
define_is_prim!(is_box_le, BOX_LE_TAG);
define_is_prim!(is_box_gt, BOX_GT_TAG);
define_is_prim!(is_box_ge, BOX_GE_TAG);
define_is_prim!(is_box_eq, BOX_EQ_TAG);
define_is_prim!(is_box_ne, BOX_NE_TAG);
define_is_prim!(is_box_pow, BOX_POW_TAG);
define_is_prim!(is_box_delay, BOX_DELAY_TAG);
define_is_prim!(is_box_delay1, BOX_DELAY1_TAG);
define_is_prim!(is_box_min, BOX_MIN_TAG);
define_is_prim!(is_box_max, BOX_MAX_TAG);

/// Equivalent to C++ `boxIPar`.
#[must_use]
pub fn box_ipar(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_IPAR_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `box_ipar`.
#[must_use]
pub fn is_box_ipar(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_IPAR_TAG)
}

/// Equivalent to C++ `boxISeq`.
#[must_use]
pub fn box_iseq(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_ISEQ_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `box_iseq`.
#[must_use]
pub fn is_box_iseq(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_ISEQ_TAG)
}

/// Equivalent to C++ `boxISum`.
#[must_use]
pub fn box_isum(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_ISUM_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `box_isum`.
#[must_use]
pub fn is_box_isum(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_ISUM_TAG)
}

/// Equivalent to C++ `boxIProd`.
#[must_use]
pub fn box_iprod(arena: &mut TreeArena, index: BoxId, count: BoxId, body: BoxId) -> BoxId {
    intern_tag(arena, BOX_IPROD_TAG, &[index, count, body])
}

/// Returns `(index, count, body)` when `b` is `box_iprod`.
#[must_use]
pub fn is_box_iprod(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_IPROD_TAG)
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

/// Equivalent to C++ `boxComponent`.
#[must_use]
pub fn box_component(arena: &mut TreeArena, filename: BoxId) -> BoxId {
    intern_tag(arena, BOX_COMPONENT_TAG, &[filename])
}

/// Returns `filename` when `b` is `box_component`.
#[must_use]
pub fn is_box_component(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_COMPONENT_TAG)
}

/// Equivalent to C++ `boxLibrary`.
#[must_use]
pub fn box_library(arena: &mut TreeArena, filename: BoxId) -> BoxId {
    intern_tag(arena, BOX_LIBRARY_TAG, &[filename])
}

/// Returns `filename` when `b` is `box_library`.
#[must_use]
pub fn is_box_library(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_LIBRARY_TAG)
}

/// Equivalent to C++ `boxWaveform`.
///
/// Rust keeps a deterministic list payload in one child:
/// `tree(BOXWAVEFORM, cons(v0, cons(v1, ...)))`.
#[must_use]
pub fn box_waveform(arena: &mut TreeArena, values: &[BoxId]) -> BoxId {
    let mut list = arena.nil();
    for value in values.iter().rev() {
        list = arena.cons(*value, list);
    }
    intern_tag(arena, BOX_WAVEFORM_TAG, &[list])
}

/// Returns waveform list payload when `b` is `box_waveform`.
#[must_use]
pub fn is_box_waveform(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_WAVEFORM_TAG)
}

/// Equivalent to C++ `boxRoute`.
#[must_use]
pub fn box_route(arena: &mut TreeArena, n: BoxId, m: BoxId, route_spec: BoxId) -> BoxId {
    intern_tag(arena, BOX_ROUTE_TAG, &[n, m, route_spec])
}

/// Returns `(n, m, route_spec)` when `b` is `box_route`.
#[must_use]
pub fn is_box_route(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_ROUTE_TAG)
}

/// Equivalent to C++ `boxButton`.
#[must_use]
pub fn box_button(arena: &mut TreeArena, label: BoxId) -> BoxId {
    intern_tag(arena, BOX_BUTTON_TAG, &[label])
}

/// Returns `label` when `b` is `box_button`.
#[must_use]
pub fn is_box_button(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_BUTTON_TAG)
}

/// Equivalent to C++ `boxCheckbox`.
#[must_use]
pub fn box_checkbox(arena: &mut TreeArena, label: BoxId) -> BoxId {
    intern_tag(arena, BOX_CHECKBOX_TAG, &[label])
}

/// Returns `label` when `b` is `box_checkbox`.
#[must_use]
pub fn is_box_checkbox(arena: &TreeArena, b: BoxId) -> Option<BoxId> {
    match_unary(arena, b, BOX_CHECKBOX_TAG)
}

/// Equivalent to C++ `boxVSlider`.
///
/// C++ payload encoding is preserved:
/// `tree(BOXVSLIDER, label, list4(cur,min,max,step))`.
#[must_use]
pub fn box_vslider(
    arena: &mut TreeArena,
    label: BoxId,
    cur: BoxId,
    min: BoxId,
    max: BoxId,
    step: BoxId,
) -> BoxId {
    let params = list4(arena, cur, min, max, step);
    intern_tag(arena, BOX_VSLIDER_TAG, &[label, params])
}

/// Returns `(label, cur, min, max, step)` when `b` is `box_vslider`.
#[must_use]
pub fn is_box_vslider(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId, BoxId, BoxId)> {
    match_slider(arena, b, BOX_VSLIDER_TAG)
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
    match_slider(arena, b, BOX_HSLIDER_TAG)
}

/// Equivalent to C++ `boxNumEntry`.
///
/// C++ payload encoding is preserved:
/// `tree(BOXNUMENTRY, label, list4(cur,min,max,step))`.
#[must_use]
pub fn box_num_entry(
    arena: &mut TreeArena,
    label: BoxId,
    cur: BoxId,
    min: BoxId,
    max: BoxId,
    step: BoxId,
) -> BoxId {
    let params = list4(arena, cur, min, max, step);
    intern_tag(arena, BOX_NUM_ENTRY_TAG, &[label, params])
}

/// Returns `(label, cur, min, max, step)` when `b` is `box_num_entry`.
#[must_use]
pub fn is_box_num_entry(
    arena: &TreeArena,
    b: BoxId,
) -> Option<(BoxId, BoxId, BoxId, BoxId, BoxId)> {
    match_slider(arena, b, BOX_NUM_ENTRY_TAG)
}

/// Equivalent to C++ `boxVBargraph`.
#[must_use]
pub fn box_vbargraph(arena: &mut TreeArena, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
    intern_tag(arena, BOX_VBARGRAPH_TAG, &[label, min, max])
}

/// Returns `(label, min, max)` when `b` is `box_vbargraph`.
#[must_use]
pub fn is_box_vbargraph(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_VBARGRAPH_TAG)
}

/// Equivalent to C++ `boxHBargraph`.
#[must_use]
pub fn box_hbargraph(arena: &mut TreeArena, label: BoxId, min: BoxId, max: BoxId) -> BoxId {
    intern_tag(arena, BOX_HBARGRAPH_TAG, &[label, min, max])
}

/// Returns `(label, min, max)` when `b` is `box_hbargraph`.
#[must_use]
pub fn is_box_hbargraph(arena: &TreeArena, b: BoxId) -> Option<(BoxId, BoxId, BoxId)> {
    match_ternary(arena, b, BOX_HBARGRAPH_TAG)
}

/// Deterministic structural dump helper for parser differential checks.
///
/// Output is shape-and-label based and intentionally excludes arena addresses.
#[must_use]
pub fn dump_box(arena: &TreeArena, root: BoxId) -> String {
    let mut out = String::new();
    dump_node(arena, root, &mut out);
    out
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

fn match_ternary(arena: &TreeArena, b: BoxId, tag: &str) -> Option<(BoxId, BoxId, BoxId)> {
    let [a, b, c] = match_tag_arity(arena, b, tag, 3)? else {
        return None;
    };
    Some((*a, *b, *c))
}

fn match_unary(arena: &TreeArena, b: BoxId, tag: &str) -> Option<BoxId> {
    let [child] = match_tag_arity(arena, b, tag, 1)? else {
        return None;
    };
    Some(*child)
}

fn match_slider(
    arena: &TreeArena,
    b: BoxId,
    tag: &str,
) -> Option<(BoxId, BoxId, BoxId, BoxId, BoxId)> {
    let [label, params] = match_tag_arity(arena, b, tag, 2)? else {
        return None;
    };
    let cur = list_nth(arena, *params, 0)?;
    let min = list_nth(arena, *params, 1)?;
    let max = list_nth(arena, *params, 2)?;
    let step = list_nth(arena, *params, 3)?;
    Some((*label, cur, min, max, step))
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

fn dump_node(arena: &TreeArena, id: BoxId, out: &mut String) {
    let Some(node) = arena.node(id) else {
        write!(out, "<invalid:{}>", id.as_u32()).expect("String write cannot fail");
        return;
    };

    match &node.kind {
        NodeKind::Nil => out.push_str("nil"),
        NodeKind::Cons => {
            out.push_str("cons(");
            if let Some(head) = node.children.get(0) {
                dump_node(arena, head, out);
            } else {
                out.push_str("<missing>");
            }
            out.push_str(", ");
            if let Some(tail) = node.children.get(1) {
                dump_node(arena, tail, out);
            } else {
                out.push_str("<missing>");
            }
            out.push(')');
        }
        NodeKind::Symbol(name) => {
            write!(out, "sym({name:?})").expect("String write cannot fail");
        }
        NodeKind::StringLiteral(value) => {
            write!(out, "str({value:?})").expect("String write cannot fail");
        }
        NodeKind::Int(value) => {
            write!(out, "int({value})").expect("String write cannot fail");
        }
        NodeKind::FloatBits(bits) => {
            write!(out, "float_bits(0x{bits:016x})").expect("String write cannot fail");
        }
        NodeKind::Tag(tag) => {
            write!(out, "{tag}(").expect("String write cannot fail");
            for (idx, child) in node.children.as_slice().iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                dump_node(arena, *child, out);
            }
            out.push(')');
        }
    }
}
