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
