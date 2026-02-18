//! Integration tests for parser_slice9_lambda_groups.rs.

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
fn supports_lambda_form() {
    let output = parse_program(r#"process = \(x, y).(x);"#, "slice9_lambda.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );
    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);
    let (_a0, inner) = is_node_abstr(&output.state.arena, expr).expect("outer abstraction");
    assert!(
        is_node_abstr(&output.state.arena, inner).is_some(),
        "expected nested abstraction for two lambda args"
    );
}

#[test]
fn supports_ui_groups_and_soundfile_forms() {
    let output = parse_program(
        r#"process = vgroup("g", _), hgroup("h", _), tgroup("t", _), soundfile("sf", 0);"#,
        "slice9_groups_soundfile.dsp",
    );
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
    assert_eq!(elems.len(), 4);
    assert!(is_node_vgroup(&output.state.arena, elems[0]).is_some());
    assert!(is_node_hgroup(&output.state.arena, elems[1]).is_some());
    assert!(is_node_tgroup(&output.state.arena, elems[2]).is_some());
    assert!(is_node_soundfile(&output.state.arena, elems[3]).is_some());
}

#[test]
fn supports_stream_wrapper_forms() {
    let output = parse_program(
        "process = inputs(_), outputs(_), ondemand(_), upsampling(_), downsampling(_);",
        "slice9_wrappers.dsp",
    );
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
    assert_eq!(elems.len(), 5);
    assert!(is_node_inputs(&output.state.arena, elems[0]).is_some());
    assert!(is_node_outputs(&output.state.arena, elems[1]).is_some());
    assert!(is_node_ondemand(&output.state.arena, elems[2]).is_some());
    assert!(is_node_upsampling(&output.state.arena, elems[3]).is_some());
    assert!(is_node_downsampling(&output.state.arena, elems[4]).is_some());
}
