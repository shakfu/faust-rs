#[path = "support/node_match_helpers.rs"]
mod node_match_helpers;
use boxes::{BoxMatch, match_box};
use node_match_helpers::*;
use parser_proto::parse_program;
use tlib::{NodeKind, TreeArena, TreeId};

fn list_head(arena: &TreeArena, list: TreeId) -> TreeId {
    arena.hd(list).expect("list must be non-empty")
}

fn list_tail(arena: &TreeArena, list: TreeId) -> TreeId {
    arena.tl(list).expect("list tail must exist")
}

fn definition_name(arena: &TreeArena, def: TreeId) -> TreeId {
    list_head(arena, def)
}

fn definition_expr(arena: &TreeArena, def: TreeId) -> TreeId {
    let payload = list_tail(arena, def);
    list_tail(arena, payload)
}

#[test]
fn parses_process_wire_definition_and_sets_def_property() {
    let output = parse_program("process = _;", "unit.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let arena = &output.state.arena;
    let ctx = &output.state.ctx;

    let def = list_head(arena, root);
    let name = definition_name(arena, def);
    let expr = definition_expr(arena, def);

    assert_eq!(node_ident_name(arena, name), Some("process"));
    assert!(matches!(match_box(arena, expr), BoxMatch::Wire));
    assert_eq!(ctx.def_file_prop(name), Some("unit.dsp"));
    assert_eq!(ctx.def_line_prop(name), Some(1));
}

#[test]
fn error_enddef_recovery_keeps_following_definition() {
    let output = parse_program("process = ;\nprocess = _;", "recover.dsp");

    assert!(
        output.errors.is_empty(),
        "recovery rule handles this malformed definition without lrpar errors: {:?}",
        output.errors
    );
    assert!(
        output.state.ctx.recovery_count() >= 1,
        "error ENDDEF path should record recovery"
    );

    let root = output.root.expect("root should be present");
    let arena = &output.state.arena;

    let def = list_head(arena, root);
    let name = definition_name(arena, def);
    let expr = definition_expr(arena, def);

    assert_eq!(node_ident_name(arena, name), Some("process"));
    assert!(matches!(match_box(arena, expr), BoxMatch::Wire));
}

#[test]
fn parses_ipar_iterative_form() {
    let output = parse_program("process = par(i, 4, _);", "iter.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let arena = &output.state.arena;

    let def = list_head(arena, root);
    let expr = definition_expr(arena, def);

    let (index, count, body) = is_node_ipar(arena, expr).expect("expression should be BOXIPAR");
    assert_eq!(node_ident_name(arena, index), Some("i"));
    assert!(matches!(arena.kind(count), Some(NodeKind::Int(4))));
    assert!(matches!(match_box(arena, body), BoxMatch::Wire));
}

#[test]
fn records_use_property_for_identifier_expressions() {
    let output = parse_program("foo = _;\nprocess = foo;", "props.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let arena = &output.state.arena;
    let ctx = &output.state.ctx;

    let process_def = list_head(arena, root);
    let process_expr = definition_expr(arena, process_def);

    assert_eq!(node_ident_name(arena, process_expr), Some("foo"));
    assert_eq!(ctx.use_file_prop(process_expr), Some("props.dsp"));
    assert_eq!(ctx.use_line_prop(process_expr), Some(2));
}
