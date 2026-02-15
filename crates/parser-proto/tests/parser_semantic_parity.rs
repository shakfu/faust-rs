use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use boxes::{
    dump_box, is_box_attach, is_box_case, is_box_control, is_box_environment, is_box_ffun,
    is_box_float_cast, is_box_inputs, is_box_int_cast, is_box_ipar, is_box_pattern_var,
    is_box_read_only_table, is_box_real, is_box_route, is_box_vgroup, is_box_waveform,
    is_box_with_local_def, is_box_with_rec_def,
};
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
    if let Some((left, right)) = boxes::is_box_par(arena, expr) {
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
    assert_eq!(
        dump_box(&arena_prec, expr_prec),
        "BOXSEQ(BOXPAR(int(1), BOXSEQ(BOXPAR(int(2), int(3)), BOXMUL())), BOXADD())"
    );

    let (arena_unary, expr_unary) = parse_process_expr("process = -foo;", "semantic_unary.dsp");
    assert_eq!(
        dump_box(&arena_unary, expr_unary),
        "BOXSEQ(BOXPAR(int(0), BOXIDENT(sym(\"foo\"))), BOXSUB())"
    );

    let (arena_delay, expr_delay) = parse_process_expr("process = _';", "semantic_delay.dsp");
    assert_eq!(
        dump_box(&arena_delay, expr_delay),
        "BOXSEQ(BOXWIRE(), BOXDELAY1())"
    );
}

#[test]
fn application_access_and_route_follow_cxx_action_formulas() {
    let (arena_appl, expr_appl) = parse_process_expr("process = foo(1, 2);", "semantic_appl.dsp");
    assert_eq!(
        dump_box(&arena_appl, expr_appl),
        "BOXAPPL(BOXIDENT(sym(\"foo\")), cons(int(2), cons(int(1), nil)))"
    );

    let (arena_access, expr_access) =
        parse_process_expr("process = foo.bar;", "semantic_access.dsp");
    assert_eq!(
        dump_box(&arena_access, expr_access),
        "BOXACCESS(BOXIDENT(sym(\"foo\")), BOXIDENT(sym(\"bar\")))"
    );

    let (arena_route, expr_route) =
        parse_process_expr("process = route(_, _);", "semantic_route.dsp");
    let (_n, _m, fake_spec) = is_box_route(&arena_route, expr_route).expect("expected BOXROUTE");
    assert_eq!(dump_box(&arena_route, fake_spec), "BOXPAR(int(0), int(0))");
}

#[test]
fn scoped_forms_and_family_matrix_match_constructor_mapping() {
    let (arena_letrec, expr_letrec) =
        parse_process_expr("process = _ letrec { 'x = _; };", "semantic_letrec.dsp");
    let (_body, rec_defs, where_defs) =
        is_box_with_rec_def(&arena_letrec, expr_letrec).expect("expected BOXWITHRECDEF");
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
            .any(|id| is_box_vgroup(&arena, id).is_some())
    );
    assert!(
        elems
            .iter()
            .copied()
            .any(|id| is_box_ipar(&arena, id).is_some())
    );
    assert!(
        elems
            .iter()
            .copied()
            .any(|id| is_box_inputs(&arena, id).is_some())
    );
    assert!(any_match(&arena, &elems, is_box_read_only_table));
    assert!(any_match(&arena, &elems, is_box_int_cast));
    assert!(any_match(&arena, &elems, is_box_float_cast));
    assert!(any_match(&arena, &elems, is_box_attach));
    assert!(any_match(&arena, &elems, is_box_control));

    let env_local = elems
        .iter()
        .copied()
        .find(|id| is_box_with_local_def(&arena, *id).is_some())
        .expect("environment should lower to local-def");
    let (env, _defs) =
        is_box_with_local_def(&arena, env_local).expect("expected local-def environment");
    assert!(is_box_environment(&arena, env));

    let waveform_box = elems
        .iter()
        .copied()
        .find(|id| is_box_waveform(&arena, *id).is_some())
        .expect("expected waveform form");
    let wave_list = is_box_waveform(&arena, waveform_box).expect("expected BOXWAVEFORM");
    let v0 = arena.hd(wave_list).expect("waveform v0");
    let t1 = arena.tl(wave_list).expect("waveform tail1");
    let v1 = arena.hd(t1).expect("waveform v1");
    let t2 = arena.tl(t1).expect("waveform tail2");
    let v2 = arena.hd(t2).expect("waveform v2");
    assert!(matches!(arena.kind(v0), Some(NodeKind::Int(1))));
    assert!(matches!(arena.kind(v1), Some(NodeKind::Int(-2))));
    assert!(is_box_real(&arena, v2));
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

    assert!(is_box_ffun(&arena, elems[0]).is_some());
    let rules = is_box_case(&arena, elems[1]).expect("expected BOXCASE");
    let rule = arena.hd(rules).expect("rule");
    let lhs = arena.hd(rule).expect("lhs");
    let first_pat = arena.hd(lhs).expect("first pattern");
    assert!(
        is_box_pattern_var(&arena, first_pat).is_some(),
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
