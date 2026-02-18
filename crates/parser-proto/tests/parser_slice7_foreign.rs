//! Integration tests for parser_slice7_foreign.rs.

#[path = "support/node_match_helpers.rs"]
mod node_match_helpers;
use node_match_helpers::*;
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

#[test]
fn supports_ffunction_signature_forms() {
    let output = parse_program(
        r#"process = ffunction(float sinhf|sinh|sinhl(float), <math.h>, "");"#,
        "slice7_ffunction.dsp",
    );
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );
    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);
    let ff = is_node_ffun(&output.state.arena, expr).expect("expected BOXFFUN");
    let (signature, incfile, libfile) =
        is_ffunction(&output.state.arena, ff).expect("expected FFUN payload");

    // Signature layout matches C++ contract: cons(ret_type, cons(names4, arg_types)).
    let ret = output.state.arena.hd(signature).expect("ret type");
    assert!(matches!(
        output.state.arena.kind(ret),
        Some(NodeKind::Int(1))
    ));
    let payload = output.state.arena.tl(signature).expect("signature payload");
    let names = output.state.arena.hd(payload).expect("names");
    assert!(!output.state.arena.is_nil(names));
    let arg_types = output.state.arena.tl(payload).expect("arg types");
    assert!(!output.state.arena.is_nil(arg_types));

    assert!(matches!(
        output.state.arena.kind(incfile),
        Some(NodeKind::Symbol(s)) if s.as_ref() == "<math.h>"
    ));
    assert!(matches!(
        output.state.arena.kind(libfile),
        Some(NodeKind::Symbol(s)) if s.as_ref() == "\"\""
    ));
}

#[test]
fn supports_fconstant_and_fvariable_forms() {
    let output = parse_program(
        "a = fconstant(int fSamplingFreq, <math.h>);\nprocess = fvariable(int count, <math.h>);",
        "slice7_fconst_fvar.dsp",
    );
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );
    let root = output.root.expect("root should be present");
    let first = list_head(&output.state.arena, root);
    let second = list_head(&output.state.arena, list_tail(&output.state.arena, root));
    let expr_a = definition_expr(&output.state.arena, first);
    let expr_b = definition_expr(&output.state.arena, second);

    let (fconst_expr, fvar_expr) = if is_node_fconst(&output.state.arena, expr_a).is_some() {
        (expr_a, expr_b)
    } else {
        (expr_b, expr_a)
    };

    let (t0, _n0, _f0) = is_node_fconst(&output.state.arena, fconst_expr).expect("fconst expected");
    let (t1, _n1, _f1) = is_node_fvar(&output.state.arena, fvar_expr).expect("fvar expected");
    assert!(matches!(
        output.state.arena.kind(t0),
        Some(NodeKind::Int(0))
    ));
    assert!(matches!(
        output.state.arena.kind(t1),
        Some(NodeKind::Int(0))
    ));
}
