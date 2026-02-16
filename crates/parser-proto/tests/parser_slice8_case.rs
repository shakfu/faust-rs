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

#[test]
fn supports_case_expression_and_prepares_pattern_variables() {
    let output = parse_program("process = case { (x) => x; (0) => _; };", "slice8_case.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );
    assert_eq!(output.state.ctx.parse_error_count(), 0);

    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);
    let rules = is_node_case(&output.state.arena, expr).expect("expected BOXCASE");

    // At least one identifier in case-lhs is converted to BOXPATVAR.
    let mut cursor = rules;
    let mut has_pattern_var = false;
    while !output.state.arena.is_nil(cursor) {
        let rule = output.state.arena.hd(cursor).expect("rule");
        let lhs = output.state.arena.hd(rule).expect("lhs");
        let mut args = lhs;
        while !output.state.arena.is_nil(args) {
            let arg = output.state.arena.hd(args).expect("arg");
            if is_node_pattern_var(&output.state.arena, arg).is_some() {
                has_pattern_var = true;
                break;
            }
            args = output.state.arena.tl(args).expect("arg tail");
        }
        if has_pattern_var {
            break;
        }
        cursor = output.state.arena.tl(cursor).expect("rules tail");
    }
    assert!(
        has_pattern_var,
        "expected at least one BOXPATVAR in case lhs"
    );
}

#[test]
fn reports_case_rule_arity_mismatch() {
    let output = parse_program(
        "process = case { (x) => x; (x, y) => x; };",
        "slice8_case_bad_arity.dsp",
    );
    assert!(output.root.is_some(), "parse should still return a root");
    assert!(
        output.state.ctx.parse_error_count() > 0,
        "arity mismatch should be recorded as parser diagnostic"
    );
}
