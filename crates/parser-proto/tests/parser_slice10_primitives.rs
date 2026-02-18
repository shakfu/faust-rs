//! Integration tests for parser_slice10_primitives.rs.

#[path = "support/node_match_helpers.rs"]
mod node_match_helpers;
use node_match_helpers::*;
use parser_proto::parse_program;
use tlib::{TreeArena, TreeId};

fn list_head(arena: &TreeArena, list: TreeId) -> TreeId {
    arena.hd(list).expect("list must be non-empty")
}

fn list_tail(arena: &TreeArena, list: TreeId) -> TreeId {
    arena.tl(list).expect("list tail must exist")
}

fn definition_expr(arena: &TreeArena, def: TreeId) -> TreeId {
    let payload = list_tail(arena, def);
    list_tail(arena, payload)
}

fn flatten_top_par(arena: &TreeArena, expr: TreeId, out: &mut Vec<TreeId>) {
    if let Some((left, right)) = is_node_par(arena, expr) {
        out.push(left);
        flatten_top_par(arena, right, out);
    } else {
        out.push(expr);
    }
}

#[test]
fn supports_extended_primitive_tokens() {
    let src = concat!(
        "process = ",
        "pow, acos, asin, atan, atan2, cos, sin, tan, exp, log, log10, sqrt, abs, ",
        "min, max, fmod, remainder, floor, ceil, rint, round, ",
        "prefix, int, float, rdtable, rwtable, select2, select3, ",
        "assertbounds, lowest, highest, attach, enable, control;",
    );
    let output = parse_program(src, "slice10_primitives.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);
    let mut elems = Vec::new();
    flatten_top_par(&output.state.arena, expr, &mut elems);
    assert_eq!(elems.len(), 34);

    assert!(is_node_pow(&output.state.arena, elems[0]));
    assert!(is_node_acos(&output.state.arena, elems[1]));
    assert!(is_node_asin(&output.state.arena, elems[2]));
    assert!(is_node_atan(&output.state.arena, elems[3]));
    assert!(is_node_atan2(&output.state.arena, elems[4]));
    assert!(is_node_cos(&output.state.arena, elems[5]));
    assert!(is_node_sin(&output.state.arena, elems[6]));
    assert!(is_node_tan(&output.state.arena, elems[7]));
    assert!(is_node_exp(&output.state.arena, elems[8]));
    assert!(is_node_log(&output.state.arena, elems[9]));
    assert!(is_node_log10(&output.state.arena, elems[10]));
    assert!(is_node_sqrt(&output.state.arena, elems[11]));
    assert!(is_node_abs(&output.state.arena, elems[12]));
    assert!(matches!(
        boxes::match_box(&output.state.arena, elems[13]),
        boxes::BoxMatch::Min
    ));
    assert!(matches!(
        boxes::match_box(&output.state.arena, elems[14]),
        boxes::BoxMatch::Max
    ));
    assert!(is_node_fmod(&output.state.arena, elems[15]));
    assert!(is_node_remainder(&output.state.arena, elems[16]));
    assert!(is_node_floor(&output.state.arena, elems[17]));
    assert!(is_node_ceil(&output.state.arena, elems[18]));
    assert!(is_node_rint(&output.state.arena, elems[19]));
    assert!(is_node_round(&output.state.arena, elems[20]));
    assert!(is_node_prefix(&output.state.arena, elems[21]));
    assert!(is_node_int_cast(&output.state.arena, elems[22]));
    assert!(is_node_float_cast(&output.state.arena, elems[23]));
    assert!(is_node_read_only_table(&output.state.arena, elems[24]));
    assert!(is_node_write_read_table(&output.state.arena, elems[25]));
    assert!(is_node_select2(&output.state.arena, elems[26]));
    assert!(is_node_select3(&output.state.arena, elems[27]));
    assert!(is_node_assert_bounds(&output.state.arena, elems[28]));
    assert!(is_node_lowest(&output.state.arena, elems[29]));
    assert!(is_node_highest(&output.state.arena, elems[30]));
    assert!(is_node_attach(&output.state.arena, elems[31]));
    assert!(is_node_enable(&output.state.arena, elems[32]));
    assert!(is_node_control(&output.state.arena, elems[33]));
}
