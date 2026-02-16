#[path = "support/box_match_helpers.rs"]
mod box_match_helpers;
use box_match_helpers::*;
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
    if let Some((left, right)) = is_box_par(arena, expr) {
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
        "pow, prefix, int, float, rdtable, rwtable, select2, select3, ",
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
    assert_eq!(elems.len(), 14);

    assert!(is_box_pow(&output.state.arena, elems[0]));
    assert!(is_box_prefix(&output.state.arena, elems[1]));
    assert!(is_box_int_cast(&output.state.arena, elems[2]));
    assert!(is_box_float_cast(&output.state.arena, elems[3]));
    assert!(is_box_read_only_table(&output.state.arena, elems[4]));
    assert!(is_box_write_read_table(&output.state.arena, elems[5]));
    assert!(is_box_select2(&output.state.arena, elems[6]));
    assert!(is_box_select3(&output.state.arena, elems[7]));
    assert!(is_box_assert_bounds(&output.state.arena, elems[8]));
    assert!(is_box_lowest(&output.state.arena, elems[9]));
    assert!(is_box_highest(&output.state.arena, elems[10]));
    assert!(is_box_attach(&output.state.arena, elems[11]));
    assert!(is_box_enable(&output.state.arena, elems[12]));
    assert!(is_box_control(&output.state.arena, elems[13]));
}
