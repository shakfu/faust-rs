//! Integration tests for `parser_slice2`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

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

    let (lhs, rhs) = match match_box(arena, expr) {
        BoxMatch::Seq(lhs, rhs) => (lhs, rhs),
        other => panic!("expected top BOXSEQ, got {:?}", other),
    };
    assert!(matches!(match_box(arena, rhs), BoxMatch::Add));
    let (one, mul_chain) = match match_box(arena, lhs) {
        BoxMatch::Par(one, mul_chain) => (one, mul_chain),
        other => panic!("expected BOXPAR for + lhs, got {:?}", other),
    };
    assert!(matches!(arena.kind(one), Some(NodeKind::Int(1))));
    let (mul_inputs, mul_op) = match match_box(arena, mul_chain) {
        BoxMatch::Seq(mul_inputs, mul_op) => (mul_inputs, mul_op),
        other => panic!("expected mul chain BOXSEQ, got {:?}", other),
    };
    assert!(matches!(match_box(arena, mul_op), BoxMatch::Mul));
    let (two, three) = match match_box(arena, mul_inputs) {
        BoxMatch::Par(two, three) => (two, three),
        other => panic!("expected BOXPAR for * inputs, got {:?}", other),
    };
    assert!(matches!(arena.kind(two), Some(NodeKind::Int(2))));
    assert!(matches!(arena.kind(three), Some(NodeKind::Int(3))));
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
    let (lhs, rhs) = match match_box(&delay.state.arena, delay_expr) {
        BoxMatch::Seq(lhs, rhs) => (lhs, rhs),
        other => panic!("expected delay BOXSEQ, got {:?}", other),
    };
    assert!(matches!(match_box(&delay.state.arena, lhs), BoxMatch::Wire));
    assert!(matches!(
        match_box(&delay.state.arena, rhs),
        BoxMatch::Delay1
    ));

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
    let (head, field) = match match_box(&access.state.arena, access_expr) {
        BoxMatch::Access(head, field) => (head, field),
        other => panic!("expected BOXACCESS, got {:?}", other),
    };
    assert_eq!(node_ident_name(&access.state.arena, head), Some("foo"));
    assert_eq!(node_ident_name(&access.state.arena, field), Some("bar"));
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

    let (callee, args) = match match_box(arena, expr) {
        BoxMatch::Appl(callee, args) => (callee, args),
        other => panic!("expected BOXAPPL, got {:?}", other),
    };
    assert_eq!(node_ident_name(arena, callee), Some("foo"));
    let first = arena.hd(args).expect("arg list head");
    let rest = arena.tl(args).expect("arg list tail");
    let second = arena.hd(rest).expect("arg list second");
    assert!(matches!(arena.kind(first), Some(NodeKind::Int(2))));
    assert!(matches!(arena.kind(second), Some(NodeKind::Int(1))));
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
    let (lhs, rhs) = match match_box(arena, expr) {
        BoxMatch::Seq(lhs, rhs) => (lhs, rhs),
        other => panic!("expected unary BOXSEQ, got {:?}", other),
    };
    assert!(matches!(match_box(arena, rhs), BoxMatch::Sub));
    let (zero, ident) = match match_box(arena, lhs) {
        BoxMatch::Par(zero, ident) => (zero, ident),
        other => panic!("expected unary BOXPAR, got {:?}", other),
    };
    assert!(matches!(arena.kind(zero), Some(NodeKind::Int(0))));
    assert_eq!(node_ident_name(arena, ident), Some("foo"));
}
