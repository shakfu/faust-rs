#![allow(dead_code)]

use boxes::{BoxMatch, match_box};
use tlib::{TreeArena, TreeId};

pub fn node_ident_name(arena: &TreeArena, b: TreeId) -> Option<&str> {
    match match_box(arena, b) {
        BoxMatch::Ident(name) => Some(name),
        _ => None,
    }
}

pub fn is_node_real(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Real(_))
}

pub fn is_node_environment(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Environment)
}

pub fn is_node_pow(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Pow)
}

pub fn is_node_acos(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Acos)
}

pub fn is_node_asin(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Asin)
}

pub fn is_node_atan(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Atan)
}

pub fn is_node_atan2(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Atan2)
}

pub fn is_node_cos(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Cos)
}

pub fn is_node_sin(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Sin)
}

pub fn is_node_tan(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Tan)
}

pub fn is_node_exp(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Exp)
}

pub fn is_node_log(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Log)
}

pub fn is_node_log10(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Log10)
}

pub fn is_node_sqrt(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Sqrt)
}

pub fn is_node_abs(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Abs)
}

pub fn is_node_prefix(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Prefix)
}

pub fn is_node_int_cast(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::IntCast)
}

pub fn is_node_float_cast(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::FloatCast)
}

pub fn is_node_read_only_table(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::ReadOnlyTable)
}

pub fn is_node_write_read_table(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::WriteReadTable)
}

pub fn is_node_select2(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Select2)
}

pub fn is_node_select3(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Select3)
}

pub fn is_node_assert_bounds(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::AssertBounds)
}

pub fn is_node_lowest(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Lowest)
}

pub fn is_node_highest(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Highest)
}

pub fn is_node_attach(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Attach)
}

pub fn is_node_enable(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Enable)
}

pub fn is_node_control(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Control)
}

pub fn is_node_fmod(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Fmod)
}

pub fn is_node_remainder(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Remainder)
}

pub fn is_node_floor(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Floor)
}

pub fn is_node_ceil(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Ceil)
}

pub fn is_node_rint(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Rint)
}

pub fn is_node_round(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Round)
}

pub fn is_node_par(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::Par(left, right) => Some((left, right)),
        _ => None,
    }
}

pub fn is_node_with_local_def(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::WithLocalDef(body, defs) => Some((body, defs)),
        _ => None,
    }
}

pub fn is_node_abstr(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::Abstr(arg, body) => Some((arg, body)),
        _ => None,
    }
}

pub fn is_node_vgroup(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::VGroup(label, expr) => Some((label, expr)),
        _ => None,
    }
}

pub fn is_node_hgroup(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::HGroup(label, expr) => Some((label, expr)),
        _ => None,
    }
}

pub fn is_node_tgroup(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::TGroup(label, expr) => Some((label, expr)),
        _ => None,
    }
}

pub fn is_node_soundfile(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::Soundfile(label, chan) => Some((label, chan)),
        _ => None,
    }
}

pub fn is_node_ipar(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::IPar(index, count, body) => Some((index, count, body)),
        _ => None,
    }
}

pub fn is_node_iseq(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::ISeq(index, count, body) => Some((index, count, body)),
        _ => None,
    }
}

pub fn is_node_isum(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::ISum(index, count, body) => Some((index, count, body)),
        _ => None,
    }
}

pub fn is_node_iprod(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::IProd(index, count, body) => Some((index, count, body)),
        _ => None,
    }
}

pub fn is_node_with_rec_def(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::WithRecDef(body, rec_defs, where_defs) => Some((body, rec_defs, where_defs)),
        _ => None,
    }
}

pub fn is_node_route(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::Route(n, m, spec) => Some((n, m, spec)),
        _ => None,
    }
}

pub fn is_node_fconst(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::FConst(ty, name, file) => Some((ty, name, file)),
        _ => None,
    }
}

pub fn is_node_fvar(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::FVar(ty, name, file) => Some((ty, name, file)),
        _ => None,
    }
}

pub fn is_ffunction(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::Ffunction(signature, incfile, libfile) => Some((signature, incfile, libfile)),
        _ => None,
    }
}

pub fn is_node_hslider(
    arena: &TreeArena,
    b: TreeId,
) -> Option<(TreeId, TreeId, TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::HSlider(label, cur, min, max, step) => Some((label, cur, min, max, step)),
        _ => None,
    }
}

pub fn is_node_case(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Case(rules) => Some(rules),
        _ => None,
    }
}

pub fn is_node_pattern_var(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::PatternVar(ident) => Some(ident),
        _ => None,
    }
}

pub fn is_node_component(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Component(filename) => Some(filename),
        _ => None,
    }
}

pub fn is_node_library(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Library(filename) => Some(filename),
        _ => None,
    }
}

pub fn is_node_waveform(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Waveform(values) => Some(values),
        _ => None,
    }
}

pub fn is_node_ffun(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::FFun(ff) => Some(ff),
        _ => None,
    }
}

pub fn is_node_inputs(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Inputs(expr) => Some(expr),
        _ => None,
    }
}

pub fn is_node_outputs(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Outputs(expr) => Some(expr),
        _ => None,
    }
}

pub fn is_node_ondemand(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Ondemand(expr) => Some(expr),
        _ => None,
    }
}

pub fn is_node_upsampling(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Upsampling(expr) => Some(expr),
        _ => None,
    }
}

pub fn is_node_downsampling(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Downsampling(expr) => Some(expr),
        _ => None,
    }
}
