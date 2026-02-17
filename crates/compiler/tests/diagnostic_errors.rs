use std::fs;
use std::path::PathBuf;

use compiler::Compiler;

fn corpus_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("corpus")
        .join(file)
}

fn read_corpus(file: &str) -> String {
    let path = corpus_path(file);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

#[test]
fn parse_error_fixture_exposes_frs_parse_code() {
    let compiler = Compiler::new();
    let source = read_corpus("err_01_parse_missing_rhs.dsp");
    let err = compiler
        .compile_source("err_01_parse_missing_rhs.dsp", &source)
        .expect_err("parse error fixture should fail parse stage");

    let diagnostics = err
        .diagnostics()
        .expect("parse error should expose diagnostics");
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|d| d.code.0.starts_with("FRS-PARSE-"))
    );
}

#[test]
fn eval_error_fixture_exposes_frs_eval_code() {
    let compiler = Compiler::new();
    let source = read_corpus("err_02_eval_missing_process.dsp");
    let err = compiler
        .compile_source_to_signals("err_02_eval_missing_process.dsp", &source)
        .expect_err("eval error fixture should fail eval stage");

    let diagnostics = err
        .diagnostics()
        .expect("eval error should expose diagnostics");
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|d| d.code.0.starts_with("FRS-EVAL-"))
    );
}

#[test]
fn propagate_error_fixture_exposes_frs_prop_code() {
    let compiler = Compiler::new();
    let source = read_corpus("err_03_propagate_split_mismatch.dsp");
    let err = compiler
        .compile_source_to_signals("err_03_propagate_split_mismatch.dsp", &source)
        .expect_err("propagate error fixture should fail propagate stage");

    let diagnostics = err
        .diagnostics()
        .expect("propagate error should expose diagnostics");
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|d| d.code.0.starts_with("FRS-PROP-"))
    );
}

#[test]
fn propagate_error_operator_span_points_to_composition_token() {
    let compiler = Compiler::new();
    let source = read_corpus("err_03_propagate_split_mismatch.dsp");
    let err = compiler
        .compile_source_to_signals("err_03_propagate_split_mismatch.dsp", &source)
        .expect_err("propagate error fixture should fail propagate stage");

    let diagnostics = err
        .diagnostics()
        .expect("propagate error should expose diagnostics");
    let first = diagnostics
        .as_slice()
        .first()
        .expect("propagate error bundle should not be empty");
    let primary = first
        .labels
        .first()
        .expect("propagate error should include one source label");

    assert_eq!(primary.span.line, 1);
    assert!(
        primary.span.col > 1,
        "operator-level span should not point to definition column 1"
    );
}

#[test]
fn propagate_error_complex_fixtures_expose_codes_and_source_labels() {
    let compiler = Compiler::new();
    let fixtures = [
        ("err_04_propagate_seq_mismatch_alias.dsp", 1u32),
        ("err_05_propagate_merge_mismatch_alias.dsp", 1u32),
        ("err_06_propagate_split_mismatch_chain.dsp", 1u32),
        ("err_07_propagate_rec_mismatch_alias.dsp", 1u32),
        ("err_08_propagate_seq_ui_mismatch.dsp", 1u32),
    ];

    for (file, expected_line) in fixtures {
        let source = read_corpus(file);
        let err = compiler
            .compile_source_to_signals(file, &source)
            .expect_err(&format!("{file} should fail in propagate stage"));

        let diagnostics = err
            .diagnostics()
            .expect(&format!("{file} should expose diagnostics"));
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|d| d.code.0.starts_with("FRS-PROP-")),
            "{file} should expose FRS-PROP-* code"
        );
        let first = diagnostics
            .as_slice()
            .first()
            .expect(&format!("{file} should produce one diagnostic"));
        let primary = first
            .labels
            .first()
            .expect(&format!("{file} should include one source label"));
        assert_eq!(
            primary.span.line, expected_line,
            "{file} should point to the expected source line"
        );
    }
}

#[test]
fn propagate_error_alias_chain_exposes_binding_trace_note() {
    let compiler = Compiler::new();
    let source = read_corpus("err_06_propagate_split_mismatch_chain.dsp");
    let err = compiler
        .compile_source_to_signals("err_06_propagate_split_mismatch_chain.dsp", &source)
        .expect_err("fixture should fail in propagate stage");

    let diagnostics = err
        .diagnostics()
        .expect("propagate error should expose diagnostics");
    let first = diagnostics
        .as_slice()
        .first()
        .expect("propagate error bundle should not be empty");
    assert!(
        first
            .notes
            .iter()
            .any(|note| note.as_ref() == "binding_trace=process -> baz -> bar -> foo"),
        "alias chain note should expose the ownership trace"
    );
}
