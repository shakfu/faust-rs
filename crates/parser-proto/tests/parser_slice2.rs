#[path = "support/node_match_helpers.rs"]
mod node_match_helpers;
use boxes::dump_box;
use node_match_helpers::*;
use parser_proto::parse_program;
use tlib::{TreeArena, TreeId};

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
fn infix_precedence_mul_before_add_matches_cxx_shape() {
    let output = parse_program("process = 1 + 2 * 3;", "slice2_prec.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let arena = &output.state.arena;
    let def = list_head(arena, root);
    let expr = definition_expr(arena, def);

    assert_eq!(
        dump_box(arena, expr),
        "BOXSEQ(BOXPAR(int(1), BOXSEQ(BOXPAR(int(2), int(3)), BOXMUL())), BOXADD())"
    );
}

#[test]
fn postfix_delay1_and_access_forms_are_supported() {
    let delay = parse_program("process = _';", "slice2_delay.dsp");
    assert!(
        delay.errors.is_empty(),
        "unexpected parse errors: {:?}",
        delay.errors
    );
    let delay_root = delay.root.expect("root should be present");
    let delay_expr = definition_expr(
        &delay.state.arena,
        list_head(&delay.state.arena, delay_root),
    );
    assert_eq!(
        dump_box(&delay.state.arena, delay_expr),
        "BOXSEQ(BOXWIRE(), BOXDELAY1())"
    );

    let access = parse_program("process = foo.bar;", "slice2_access.dsp");
    assert!(
        access.errors.is_empty(),
        "unexpected parse errors: {:?}",
        access.errors
    );
    let access_root = access.root.expect("root should be present");
    let access_expr = definition_expr(
        &access.state.arena,
        list_head(&access.state.arena, access_root),
    );
    assert_eq!(
        dump_box(&access.state.arena, access_expr),
        "BOXACCESS(BOXIDENT(sym(\"foo\")), BOXIDENT(sym(\"bar\")))"
    );
}

#[test]
fn application_uses_reversed_argument_list_like_cpp_buildboxappl() {
    let output = parse_program("process = foo(1, 2);", "slice2_appl.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let arena = &output.state.arena;
    let def = list_head(arena, root);
    let expr = definition_expr(arena, def);

    assert_eq!(
        dump_box(arena, expr),
        "BOXAPPL(BOXIDENT(sym(\"foo\")), cons(int(2), cons(int(1), nil)))"
    );
}

#[test]
fn unary_minus_identifier_lowers_to_sub_from_zero() {
    let output = parse_program("process = -foo;", "slice2_unary.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let arena = &output.state.arena;
    let def = list_head(arena, root);
    let name = definition_name(arena, def);
    let expr = definition_expr(arena, def);

    assert_eq!(node_ident_name(arena, name), Some("process"));
    assert_eq!(
        dump_box(arena, expr),
        "BOXSEQ(BOXPAR(int(0), BOXIDENT(sym(\"foo\"))), BOXSUB())"
    );
}
