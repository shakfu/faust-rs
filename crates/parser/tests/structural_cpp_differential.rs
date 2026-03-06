//! Structural parser differential checks against C++ acceptance on representative cases.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use boxes::{BoxMatch, match_box};
use parser::{parse_file_with_imports, parse_program};
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

fn definition_name(arena: &TreeArena, def: TreeId) -> String {
    let name = list_head(arena, def);
    match match_box(arena, name) {
        BoxMatch::Ident(text) => text.to_owned(),
        other => panic!("expected definition name ident, got {:?}", other),
    }
}

fn case_rules(arena: &TreeArena, expr: TreeId) -> TreeId {
    match match_box(arena, expr) {
        BoxMatch::Case(rules) => rules,
        other => panic!("expected BOXCASE, got {:?}", other),
    }
}

fn rule_first_pattern(arena: &TreeArena, rule: TreeId) -> TreeId {
    let lhs = list_head(arena, rule);
    list_head(arena, lhs)
}

fn find_definition_expr(arena: &TreeArena, mut defs: TreeId, expected: &str) -> Option<TreeId> {
    while !arena.is_nil(defs) {
        let def = arena.hd(defs)?;
        if definition_name(arena, def) == expected {
            return Some(definition_expr(arena, def));
        }
        defs = arena.tl(defs)?;
    }
    None
}

fn count_definitions_named(arena: &TreeArena, mut defs: TreeId, expected: &str) -> usize {
    let mut count = 0usize;
    while !arena.is_nil(defs) {
        let Some(def) = arena.hd(defs) else {
            break;
        };
        if definition_name(arena, def) == expected {
            count = count.saturating_add(1);
        }
        defs = arena.tl(defs).unwrap_or_else(|| arena.nil());
    }
    count
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
    default.exists().then_some(default)
}

fn cpp_accepts_source(cpp_bin: &Path, source: &str, case_name: &str) -> Result<bool, String> {
    let mut input_path = std::env::temp_dir();
    input_path.push(format!(
        "faust_rs_parser_structural_{}_{}.dsp",
        std::process::id(),
        case_name
    ));
    let mut out_path = std::env::temp_dir();
    out_path.push(format!(
        "faust_rs_parser_structural_{}_{}.c",
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
fn production_parser_structural_shapes_align_with_cpp_acceptance() {
    let cases = [
        ("prec", "process = 1 + 2 * 3;"),
        ("unary", "process = -1;"),
        ("appl", "process = abs(1);"),
    ];

    let cpp = cpp_bin();
    for (name, source) in cases {
        if let Some(cpp_bin) = &cpp {
            let cpp_ok = cpp_accepts_source(cpp_bin, source, name)
                .unwrap_or_else(|e| panic!("C++ run failed for {name}: {e}"));
            assert!(cpp_ok, "C++ should accept structural case {name}");
        }

        let (arena, expr) = parse_process_expr(source, &format!("structural_{name}.dsp"));
        match name {
            "prec" => {
                let (lhs, rhs) = match match_box(&arena, expr) {
                    BoxMatch::Seq(lhs, rhs) => (lhs, rhs),
                    other => panic!("expected top BOXSEQ for prec, got {:?}", other),
                };
                assert!(matches!(match_box(&arena, rhs), BoxMatch::Add));
                let (one, mul_chain) = match match_box(&arena, lhs) {
                    BoxMatch::Par(one, mul_chain) => (one, mul_chain),
                    other => panic!("expected BOXPAR for prec lhs, got {:?}", other),
                };
                assert!(matches!(arena.kind(one), Some(NodeKind::Int(1))));
                let (_mul_inputs, mul_op) = match match_box(&arena, mul_chain) {
                    BoxMatch::Seq(mul_inputs, mul_op) => (mul_inputs, mul_op),
                    other => panic!("expected mul chain BOXSEQ, got {:?}", other),
                };
                assert!(matches!(match_box(&arena, mul_op), BoxMatch::Mul));
            }
            "unary" => match match_box(&arena, expr) {
                BoxMatch::Seq(_lhs, rhs) => {
                    assert!(matches!(match_box(&arena, rhs), BoxMatch::Sub));
                }
                BoxMatch::Int(v) => {
                    assert_eq!(v, -1);
                }
                other => panic!("expected unary lowering shape, got {:?}", other),
            },
            "appl" => {
                let (_callee, args) = match match_box(&arena, expr) {
                    BoxMatch::Appl(callee, args) => (callee, args),
                    other => panic!("expected BOXAPPL, got {:?}", other),
                };
                assert!(
                    !arena.is_nil(args),
                    "application must carry at least one argument"
                );
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn production_parser_import_heavy_fixture_matches_cpp_acceptance() {
    let root = {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "faust_rs_parser_structural_imports_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should move forward")
                .as_nanos()
        ));
        fs::create_dir_all(&path).expect("temp root should be created");
        path
    };

    let main = root.join("src/main.dsp");
    let lib = root.join("libs/gain.lib");
    let core = root.join("libs/core/base.lib");
    fs::create_dir_all(main.parent().expect("main parent")).expect("create src");
    fs::create_dir_all(lib.parent().expect("lib parent")).expect("create libs");
    fs::create_dir_all(core.parent().expect("core parent")).expect("create core");

    fs::write(&main, "import(\"gain.lib\");\nprocess = gain + gain;\n")
        .expect("main should be written");
    fs::write(&lib, "import(\"core/base.lib\");\ngain = base;\n").expect("lib should be written");
    fs::write(&core, "base = _;\n").expect("core should be written");

    let out = parse_file_with_imports(&main, &[root.join("libs")]).expect("parse should succeed");
    assert!(out.root.is_some(), "root should be present");
    assert!(
        out.errors.is_empty(),
        "unexpected parser errors for import-heavy fixture: {:?}",
        out.errors
    );

    if let Some(cpp_bin) = cpp_bin() {
        let out_c = root.join("out.c");
        let output = Command::new(cpp_bin)
            .arg(&main)
            .arg("-lang")
            .arg("c")
            .arg("-o")
            .arg(&out_c)
            .arg("-I")
            .arg(root.join("libs"))
            .output()
            .expect("failed to run c++ reference parser");
        assert!(
            output.status.success(),
            "C++ should accept import-heavy fixture, status={:?}, stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fs::remove_dir_all(root).expect("temp root should be removable");
}

#[test]
fn production_parser_groups_patterned_and_multi_clause_definitions_like_cpp() {
    let cases = [
        (
            "patterned_constant_clause",
            "foo(0) = _;\nfoo(x) = x;\nprocess = foo;\n",
        ),
        (
            "grouped_identifier_clauses",
            "foo(x) = x;\nfoo(y) = y;\nprocess = foo;\n",
        ),
    ];

    let cpp = cpp_bin();
    for (name, source) in cases {
        if let Some(cpp_bin) = &cpp {
            let cpp_ok = cpp_accepts_source(cpp_bin, source, name)
                .unwrap_or_else(|e| panic!("C++ run failed for {name}: {e}"));
            assert!(cpp_ok, "C++ should accept grouped-definition case {name}");
        }

        let output = parse_program(source, &format!("structural_{name}.dsp"));
        assert!(
            output.errors.is_empty(),
            "unexpected parse errors for {}: {:?}",
            name,
            output.errors
        );
        let root = output.root.expect("root should be present");
        let foo_expr = find_definition_expr(&output.state.arena, root, "foo")
            .unwrap_or_else(|| panic!("missing foo definition for {name}"));
        assert!(
            matches!(match_box(&output.state.arena, foo_expr), BoxMatch::Case(_)),
            "foo should be normalized to BOXCASE for {name}, got {:?}",
            match_box(&output.state.arena, foo_expr)
        );
        assert_eq!(
            count_definitions_named(&output.state.arena, root, "foo"),
            1,
            "foo should be grouped into one definition for {name}"
        );

        let rules = case_rules(&output.state.arena, foo_expr);
        let mut cursor = rules;
        let mut saw_pattern_var = false;
        let mut saw_zero_literal = false;
        while !output.state.arena.is_nil(cursor) {
            let rule = list_head(&output.state.arena, cursor);
            let pattern = rule_first_pattern(&output.state.arena, rule);
            saw_pattern_var |= matches!(
                match_box(&output.state.arena, pattern),
                BoxMatch::PatternVar(_)
            );
            saw_zero_literal |= matches!(output.state.arena.kind(pattern), Some(NodeKind::Int(0)));
            cursor = list_tail(&output.state.arena, cursor);
        }
        assert!(
            saw_pattern_var,
            "at least one clause should be wrapped as BOXPATVAR for {name}"
        );

        if name == "patterned_constant_clause" {
            assert!(
                saw_zero_literal,
                "constant clause should stay literal for {name}"
            );
        }
    }
}

#[test]
fn production_parser_recognizes_modif_local_def_like_cpp() {
    let source = "e = environment { a = _; };\nprocess = (e [ a = 1; ]).a;\n";

    if let Some(cpp_bin) = cpp_bin() {
        let cpp_ok = cpp_accepts_source(cpp_bin.as_path(), source, "modif_local_def")
            .unwrap_or_else(|e| panic!("C++ run failed for modif_local_def: {e}"));
        assert!(cpp_ok, "C++ should accept modif-local-def syntax");
    }

    let output = parse_program(source, "structural_modif_local_def.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors for modif-local-def: {:?}",
        output.errors
    );
    let root = output.root.expect("root should be present");
    let process_expr = find_definition_expr(&output.state.arena, root, "process")
        .expect("missing process definition");
    let (modif_expr, field) = match match_box(&output.state.arena, process_expr) {
        BoxMatch::Access(body, field) => (body, field),
        other => panic!("expected BOXACCESS over BOXMODIFLOCALDEF, got {:?}", other),
    };
    assert!(
        matches!(match_box(&output.state.arena, field), BoxMatch::Ident("a")),
        "access field should remain `a`"
    );
    let (body, defs) = match match_box(&output.state.arena, modif_expr) {
        BoxMatch::ModifLocalDef(body, defs) => (body, defs),
        other => panic!("expected BOXMODIFLOCALDEF, got {:?}", other),
    };
    assert!(
        matches!(match_box(&output.state.arena, body), BoxMatch::Ident("e")),
        "modif-local-def body should remain the source expression"
    );
    assert!(
        !output.state.arena.is_nil(defs),
        "modif-local-def should carry formatted definitions"
    );
}
