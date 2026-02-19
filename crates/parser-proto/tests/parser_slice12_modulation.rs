//! Integration tests for `parser_slice12_modulation`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

use parser_proto::parse_program;
use tlib::{NodeKind, TreeArena, TreeId};

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

fn modulation_parts(arena: &TreeArena, node: TreeId) -> (TreeId, TreeId) {
    let children = arena.children(node).expect("node should exist");
    assert_eq!(children.len(), 2, "modulation should have arity 2");
    match arena.kind(node) {
        Some(NodeKind::Tag(tag_id)) => assert_eq!(arena.tag_name(*tag_id), Some("BOXMODULATION")),
        _ => panic!("expected BOXMODULATION tag"),
    }
    (children[0], children[1])
}

fn entry_label(arena: &TreeArena, entry: TreeId) -> String {
    let label = list_head(arena, entry);
    match arena.kind(label) {
        Some(NodeKind::StringLiteral(s)) => s.to_string(),
        Some(NodeKind::Symbol(s)) => s.to_string(),
        _ => panic!("modulation entry label should be string-like"),
    }
}

#[test]
fn supports_bracket_modulation_form() {
    let output = parse_program(r#"process = ["gain" : _ -> _];"#, "slice12_mod_single.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);
    let (entry, body) = modulation_parts(&output.state.arena, expr);
    assert_eq!(entry_label(&output.state.arena, entry), "gain");
    assert!(!output.state.arena.is_nil(body));
}

#[test]
fn modulation_entry_order_matches_cpp_buildboxmodulation() {
    let output = parse_program(
        r#"process = ["a" : _, "b" : _ -> _];"#,
        "slice12_mod_order.dsp",
    );
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);

    let (outer_entry, nested_body) = modulation_parts(&output.state.arena, expr);
    assert_eq!(entry_label(&output.state.arena, outer_entry), "a");
    let (inner_entry, _) = modulation_parts(&output.state.arena, nested_body);
    assert_eq!(entry_label(&output.state.arena, inner_entry), "b");
}
