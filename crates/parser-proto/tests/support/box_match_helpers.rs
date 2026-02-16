#![allow(dead_code)]

use boxes::{BoxMatch, match_box};
use tlib::{TreeArena, TreeId};

pub fn box_ident_name(arena: &TreeArena, b: TreeId) -> Option<&str> {
    match match_box(arena, b) {
        BoxMatch::Ident(name) => Some(name),
        _ => None,
    }
}

pub fn is_box_real(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Real(_))
}

pub fn is_box_environment(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Environment)
}

pub fn is_box_pow(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Pow)
}

pub fn is_box_prefix(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Prefix)
}

pub fn is_box_int_cast(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::IntCast)
}

pub fn is_box_float_cast(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::FloatCast)
}

pub fn is_box_read_only_table(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::ReadOnlyTable)
}

pub fn is_box_write_read_table(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::WriteReadTable)
}

pub fn is_box_select2(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Select2)
}

pub fn is_box_select3(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Select3)
}

pub fn is_box_assert_bounds(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::AssertBounds)
}

pub fn is_box_lowest(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Lowest)
}

pub fn is_box_highest(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Highest)
}

pub fn is_box_attach(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Attach)
}

pub fn is_box_enable(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Enable)
}

pub fn is_box_control(arena: &TreeArena, b: TreeId) -> bool {
    matches!(match_box(arena, b), BoxMatch::Control)
}

pub fn is_box_par(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::Par(left, right) => Some((left, right)),
        _ => None,
    }
}

pub fn is_box_with_local_def(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::WithLocalDef(body, defs) => Some((body, defs)),
        _ => None,
    }
}

pub fn is_box_abstr(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::Abstr(arg, body) => Some((arg, body)),
        _ => None,
    }
}

pub fn is_box_vgroup(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::VGroup(label, expr) => Some((label, expr)),
        _ => None,
    }
}

pub fn is_box_hgroup(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::HGroup(label, expr) => Some((label, expr)),
        _ => None,
    }
}

pub fn is_box_tgroup(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::TGroup(label, expr) => Some((label, expr)),
        _ => None,
    }
}

pub fn is_box_soundfile(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::Soundfile(label, chan) => Some((label, chan)),
        _ => None,
    }
}

pub fn is_box_ipar(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::IPar(index, count, body) => Some((index, count, body)),
        _ => None,
    }
}

pub fn is_box_iseq(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::ISeq(index, count, body) => Some((index, count, body)),
        _ => None,
    }
}

pub fn is_box_isum(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::ISum(index, count, body) => Some((index, count, body)),
        _ => None,
    }
}

pub fn is_box_iprod(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::IProd(index, count, body) => Some((index, count, body)),
        _ => None,
    }
}

pub fn is_box_with_rec_def(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::WithRecDef(body, rec_defs, where_defs) => Some((body, rec_defs, where_defs)),
        _ => None,
    }
}

pub fn is_box_route(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::Route(n, m, spec) => Some((n, m, spec)),
        _ => None,
    }
}

pub fn is_box_fconst(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::FConst(ty, name, file) => Some((ty, name, file)),
        _ => None,
    }
}

pub fn is_box_fvar(arena: &TreeArena, b: TreeId) -> Option<(TreeId, TreeId, TreeId)> {
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

pub fn is_box_hslider(
    arena: &TreeArena,
    b: TreeId,
) -> Option<(TreeId, TreeId, TreeId, TreeId, TreeId)> {
    match match_box(arena, b) {
        BoxMatch::HSlider(label, cur, min, max, step) => Some((label, cur, min, max, step)),
        _ => None,
    }
}

pub fn is_box_case(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Case(rules) => Some(rules),
        _ => None,
    }
}

pub fn is_box_pattern_var(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::PatternVar(ident) => Some(ident),
        _ => None,
    }
}

pub fn is_box_component(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Component(filename) => Some(filename),
        _ => None,
    }
}

pub fn is_box_library(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Library(filename) => Some(filename),
        _ => None,
    }
}

pub fn is_box_waveform(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Waveform(values) => Some(values),
        _ => None,
    }
}

pub fn is_box_ffun(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::FFun(ff) => Some(ff),
        _ => None,
    }
}

pub fn is_box_inputs(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Inputs(expr) => Some(expr),
        _ => None,
    }
}

pub fn is_box_outputs(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Outputs(expr) => Some(expr),
        _ => None,
    }
}

pub fn is_box_ondemand(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Ondemand(expr) => Some(expr),
        _ => None,
    }
}

pub fn is_box_upsampling(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Upsampling(expr) => Some(expr),
        _ => None,
    }
}

pub fn is_box_downsampling(arena: &TreeArena, b: TreeId) -> Option<TreeId> {
    match match_box(arena, b) {
        BoxMatch::Downsampling(expr) => Some(expr),
        _ => None,
    }
}
