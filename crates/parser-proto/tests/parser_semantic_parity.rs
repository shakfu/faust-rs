//! Integration tests for `parser_semantic_parity`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn any_match(arena: &TreeArena, elems: &[TreeId], pred: fn(&TreeArena, TreeId) -> bool) -> bool {
    elems.iter().copied().any(|id| pred(arena, id))
}

fn parse_process_expr(source: &str, source_name: &str) -> (TreeArena, TreeId) {
    let output = parse_program(source, source_name);
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors for {}: {:?}",
        source_name,
        output.errors
    );
    let root = output.root.expect("root should be present");
    let def = list_head(&output.state.arena, root);
    let expr = definition_expr(&output.state.arena, def);
    (output.state.arena, expr)
}

fn cpp_bin() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FAUST_CPP_BIN") {
        return Some(PathBuf::from(path));
    }
    let default = PathBuf::from("/usr/local/bin/faust");
    if default.exists() {
        Some(default)
    } else {
        None
    }
}

fn cpp_accepts(cpp_bin: &Path, source: &str, case_name: &str) -> Result<bool, String> {
    let mut input_path = std::env::temp_dir();
    input_path.push(format!(
        "faust_rs_parser_semantic_{}_{}.dsp",
        std::process::id(),
        case_name
    ));
    let mut out_path = std::env::temp_dir();
    out_path.push(format!(
        "faust_rs_parser_semantic_{}_{}.c",
        std::process::id(),
        case_name
    ));
    fs::write(&input_path, source).map_err(|e| format!("write input failed: {e}"))?;

    let output = Command::new(cpp_bin)
        .arg(&input_path)
        .arg("-lang")
        .arg("c")
        .arg("-o")
        .arg(&out_path)
        .output()
        .map_err(|e| format!("failed to run {}: {e}", cpp_bin.display()))?;

    let _ = fs::remove_file(&input_path);
    let _ = fs::remove_file(&out_path);
    Ok(output.status.success())
}

#[test]
fn infix_and_unary_follow_cxx_action_formulas() {
    let (arena_prec, expr_prec) = parse_process_expr("process = 1 + 2 * 3;", "semantic_prec.dsp");
    let (lhs, rhs) = match match_box(&arena_prec, expr_prec) {
        BoxMatch::Seq(lhs, rhs) => (lhs, rhs),
        other => panic!("expected top BOXSEQ, got {:?}", other),
    };
    assert!(matches!(match_box(&arena_prec, rhs), BoxMatch::Add));
    let (one, mul_chain) = match match_box(&arena_prec, lhs) {
        BoxMatch::Par(one, mul_chain) => (one, mul_chain),
        other => panic!("expected BOXPAR for + lhs, got {:?}", other),
    };
    assert!(matches!(arena_prec.kind(one), Some(NodeKind::Int(1))));
    let (mul_inputs, mul_op) = match match_box(&arena_prec, mul_chain) {
        BoxMatch::Seq(mul_inputs, mul_op) => (mul_inputs, mul_op),
        other => panic!("expected mul chain BOXSEQ, got {:?}", other),
    };
    assert!(matches!(match_box(&arena_prec, mul_op), BoxMatch::Mul));
    let (two, three) = match match_box(&arena_prec, mul_inputs) {
        BoxMatch::Par(two, three) => (two, three),
        other => panic!("expected BOXPAR for * inputs, got {:?}", other),
    };
    assert!(matches!(arena_prec.kind(two), Some(NodeKind::Int(2))));
    assert!(matches!(arena_prec.kind(three), Some(NodeKind::Int(3))));

    let (arena_unary, expr_unary) = parse_process_expr("process = -foo;", "semantic_unary.dsp");
    let (lhs, rhs) = match match_box(&arena_unary, expr_unary) {
        BoxMatch::Seq(lhs, rhs) => (lhs, rhs),
        other => panic!("expected unary BOXSEQ, got {:?}", other),
    };
    assert!(matches!(match_box(&arena_unary, rhs), BoxMatch::Sub));
    let (zero, ident) = match match_box(&arena_unary, lhs) {
        BoxMatch::Par(zero, ident) => (zero, ident),
        other => panic!("expected unary BOXPAR, got {:?}", other),
    };
    assert!(matches!(arena_unary.kind(zero), Some(NodeKind::Int(0))));
    assert_eq!(node_ident_name(&arena_unary, ident), Some("foo"));

    let (arena_delay, expr_delay) = parse_process_expr("process = _';", "semantic_delay.dsp");
    let (lhs, rhs) = match match_box(&arena_delay, expr_delay) {
        BoxMatch::Seq(lhs, rhs) => (lhs, rhs),
        other => panic!("expected delay BOXSEQ, got {:?}", other),
    };
    assert!(matches!(match_box(&arena_delay, lhs), BoxMatch::Wire));
    assert!(matches!(match_box(&arena_delay, rhs), BoxMatch::Delay1));
}

#[test]
fn application_access_and_route_follow_cxx_action_formulas() {
    let (arena_appl, expr_appl) = parse_process_expr("process = foo(1, 2);", "semantic_appl.dsp");
    let (callee, args) = match match_box(&arena_appl, expr_appl) {
        BoxMatch::Appl(callee, args) => (callee, args),
        other => panic!("expected BOXAPPL, got {:?}", other),
    };
    assert_eq!(node_ident_name(&arena_appl, callee), Some("foo"));
    let first = arena_appl.hd(args).expect("arg list head");
    let rest = arena_appl.tl(args).expect("arg list tail");
    let second = arena_appl.hd(rest).expect("arg list second");
    assert!(matches!(arena_appl.kind(first), Some(NodeKind::Int(2))));
    assert!(matches!(arena_appl.kind(second), Some(NodeKind::Int(1))));

    let (arena_access, expr_access) =
        parse_process_expr("process = foo.bar;", "semantic_access.dsp");
    let (head, field) = match match_box(&arena_access, expr_access) {
        BoxMatch::Access(head, field) => (head, field),
        other => panic!("expected BOXACCESS, got {:?}", other),
    };
    assert_eq!(node_ident_name(&arena_access, head), Some("foo"));
    assert_eq!(node_ident_name(&arena_access, field), Some("bar"));

    let (arena_route, expr_route) =
        parse_process_expr("process = route(_, _);", "semantic_route.dsp");
    let (_n, _m, fake_spec) = is_node_route(&arena_route, expr_route).expect("expected BOXROUTE");
    let (a, b) = is_node_par(&arena_route, fake_spec).expect("route fake spec should be BOXPAR");
    assert!(matches!(arena_route.kind(a), Some(NodeKind::Int(0))));
    assert!(matches!(arena_route.kind(b), Some(NodeKind::Int(0))));
}

#[test]
fn scoped_forms_and_family_matrix_match_constructor_mapping() {
    let (arena_letrec, expr_letrec) =
        parse_process_expr("process = _ letrec { 'x = _; };", "semantic_letrec.dsp");
    let (_body, rec_defs, where_defs) =
        is_node_with_rec_def(&arena_letrec, expr_letrec).expect("expected BOXWITHRECDEF");
    assert!(!arena_letrec.is_nil(rec_defs));
    assert!(arena_letrec.is_nil(where_defs));

    let source = concat!(
        "process = ",
        "vgroup(\"g\", _), ",
        "par(i, 4, _), ",
        "inputs(_), ",
        "rdtable, ",
        "int, ",
        "float, ",
        "attach, ",
        "control, ",
        "environment { a = _; }, ",
        "waveform { 1, -2, 3.5 };",
    );
    let (arena, expr) = parse_process_expr(source, "semantic_matrix.dsp");
    let mut elems = Vec::new();
    flatten_top_par(&arena, expr, &mut elems);
    assert_eq!(elems.len(), 10);

    assert!(
        elems
            .iter()
            .copied()
            .any(|id| is_node_vgroup(&arena, id).is_some())
    );
    assert!(
        elems
            .iter()
            .copied()
            .any(|id| is_node_ipar(&arena, id).is_some())
    );
    assert!(
        elems
            .iter()
            .copied()
            .any(|id| is_node_inputs(&arena, id).is_some())
    );
    assert!(any_match(&arena, &elems, is_node_read_only_table));
    assert!(any_match(&arena, &elems, is_node_int_cast));
    assert!(any_match(&arena, &elems, is_node_float_cast));
    assert!(any_match(&arena, &elems, is_node_attach));
    assert!(any_match(&arena, &elems, is_node_control));

    let env_local = elems
        .iter()
        .copied()
        .find(|id| is_node_with_local_def(&arena, *id).is_some())
        .expect("environment should lower to local-def");
    let (env, _defs) =
        is_node_with_local_def(&arena, env_local).expect("expected local-def environment");
    assert!(is_node_environment(&arena, env));

    let waveform_box = elems
        .iter()
        .copied()
        .find(|id| is_node_waveform(&arena, *id).is_some())
        .expect("expected waveform form");
    let wave_list = is_node_waveform(&arena, waveform_box).expect("expected BOXWAVEFORM");
    let v0 = arena.hd(wave_list).expect("waveform v0");
    let t1 = arena.tl(wave_list).expect("waveform tail1");
    let v1 = arena.hd(t1).expect("waveform v1");
    let t2 = arena.tl(t1).expect("waveform tail2");
    let v2 = arena.hd(t2).expect("waveform v2");
    assert!(matches!(arena.kind(v0), Some(NodeKind::Int(1))));
    assert!(matches!(arena.kind(v1), Some(NodeKind::Int(-2))));
    assert!(is_node_real(&arena, v2));
}

#[test]
fn foreign_and_case_actions_follow_cxx_families() {
    let source = concat!(
        "process = ",
        "ffunction(float sinhf|sinh|sinhl(float), <math.h>, \"\"), ",
        "case { (x) => x; };",
    );
    let (arena, expr) = parse_process_expr(source, "semantic_foreign_case.dsp");
    let mut elems = Vec::new();
    flatten_top_par(&arena, expr, &mut elems);
    assert_eq!(elems.len(), 2);

    assert!(is_node_ffun(&arena, elems[0]).is_some());
    let rules = is_node_case(&arena, elems[1]).expect("expected BOXCASE");
    let rule = arena.hd(rules).expect("rule");
    let lhs = arena.hd(rule).expect("lhs");
    let first_pat = arena.hd(lhs).expect("first pattern");
    assert!(
        is_node_pattern_var(&arena, first_pat).is_some(),
        "pattern variable should be transformed to BOXPATTERNVAR"
    );
}

#[test]
fn semantic_shape_corpus_is_accepted_by_cpp_reference() {
    let Some(cpp_bin) = cpp_bin() else {
        eprintln!(
            "Skipping C++ semantic envelope check: FAUST_CPP_BIN not set and /usr/local/bin/faust not found"
        );
        return;
    };
    if !cpp_bin.exists() {
        eprintln!(
            "Skipping C++ semantic envelope check: C++ binary not found at {}",
            cpp_bin.display()
        );
        return;
    }

    let cases = [
        ("semantic_prec", "process = 1 + 2 * 3;\n"),
        ("semantic_unary", "process = -foo;\nfoo = _;\n"),
        ("semantic_delay", "process = _';\n"),
        ("semantic_appl", "process = foo(1, 2);\nfoo(x, y) = x, y;\n"),
        ("semantic_letrec", "process = _ letrec { 'x = _; };\n"),
        (
            "semantic_foreign_case",
            concat!(
                "process = ",
                "ffunction(float sinhf|sinh|sinhl(float), <math.h>, \"\"), ",
                "case { (x) => x; };",
            ),
        ),
    ];

    let mut rejected = Vec::new();
    for (name, source) in cases {
        let ok = cpp_accepts(&cpp_bin, source, name)
            .unwrap_or_else(|e| panic!("C++ run failed for {name}: {e}"));
        if !ok {
            rejected.push(name.to_owned());
        }
    }

    assert!(
        rejected.is_empty(),
        "C++ reference rejected semantic parity cases: {}",
        rejected.join(", ")
    );
}
