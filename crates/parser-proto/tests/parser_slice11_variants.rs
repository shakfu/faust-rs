//! Integration tests for `parser_slice11_variants`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

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

fn definition_name(arena: &TreeArena, def: TreeId) -> TreeId {
    list_head(arena, def)
}

fn definition_expr(arena: &TreeArena, def: TreeId) -> TreeId {
    let payload = list_tail(arena, def);
    list_tail(arena, payload)
}

fn collect_definition_names(arena: &TreeArena, mut defs: TreeId) -> Vec<String> {
    let mut out = Vec::new();
    while !arena.is_nil(defs) {
        let def = arena.hd(defs).expect("definition should exist");
        let name = definition_name(arena, def);
        let ident = node_ident_name(arena, name).expect("definition name should be BOXIDENT");
        out.push(ident.to_owned());
        defs = arena
            .tl(defs)
            .expect("definition list should be well-formed");
    }
    out
}

#[test]
fn variantlist_filters_statements_by_default_single_mode() {
    let output = parse_program(
        "doubleprecision foo = _; process = _;",
        "slice11_filter.dsp",
    );
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let names = collect_definition_names(&output.state.arena, root);
    assert_eq!(names, vec!["process"]);
}

#[test]
fn variantlist_accepts_singleprecision_prefixed_definition() {
    let output = parse_program(
        "singleprecision foo = _; process = foo;",
        "slice11_single.dsp",
    );
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let names = collect_definition_names(&output.state.arena, root);
    assert_eq!(names, vec!["process", "foo"]);
}

#[test]
fn variantlist_applies_inside_local_definition_lists() {
    let output = parse_program(
        "process = _ with { doubleprecision a = _; singleprecision b = _; };",
        "slice11_local_defs.dsp",
    );
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let root = output.root.expect("root should be present");
    let process_def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, process_def);
    let (_body, local_defs) =
        is_node_with_local_def(&output.state.arena, expr).expect("expected local-def expression");

    let names = collect_definition_names(&output.state.arena, local_defs);
    assert_eq!(names, vec!["b"]);
}
