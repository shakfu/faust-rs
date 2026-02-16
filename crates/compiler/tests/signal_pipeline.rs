use std::path::PathBuf;

use compiler::Compiler;
use signals::{BinOp, SigMatch, match_sig};
use tlib::{TreeArena, TreeId};

fn corpus_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("corpus")
        .join(file)
}

fn compile_corpus(file: &str) -> compiler::SignalCompileOutput {
    let compiler = Compiler::new();
    let path = corpus_path(file);
    compiler
        .compile_file_default_to_signals(&path)
        .unwrap_or_else(|e| panic!("failed to compile {} to signals: {e}", path.display()))
}

#[test]
fn corpus_passthrough_maps_to_input_signal() {
    let out = compile_corpus("rep_01_passthrough.dsp");
    assert_eq!(out.process_arity.inputs, 1);
    assert_eq!(out.process_arity.outputs, 1);
    assert_eq!(out.signals.len(), 1);
    assert_eq!(
        match_sig(&out.parse.state.arena, out.signals[0]),
        SigMatch::Input(0)
    );
}

#[test]
fn corpus_gain_bias_lowers_to_add_mul_and_constant() {
    let out = compile_corpus("rep_02_gain_bias.dsp");
    assert_eq!(out.process_arity.inputs, 1);
    assert_eq!(out.process_arity.outputs, 1);
    assert_eq!(out.signals.len(), 1);

    let SigMatch::BinOp(BinOp::Add, lhs, rhs) = match_sig(&out.parse.state.arena, out.signals[0])
    else {
        panic!("rep_02 should lower to one add signal");
    };
    assert_mul_input_const(&out.parse.state.arena, lhs, 0);
    assert!(matches!(
        match_sig(&out.parse.state.arena, rhs),
        SigMatch::Real(_) | SigMatch::Int(_)
    ));
}

#[test]
fn corpus_operator_precedence_structure_is_stable() {
    let out = compile_corpus("rep_21_operator_precedence.dsp");
    assert_eq!(out.process_arity.inputs, 0);
    assert_eq!(out.process_arity.outputs, 1);
    assert_eq!(out.signals.len(), 1);

    let SigMatch::BinOp(BinOp::Sub, lhs, rhs) = match_sig(&out.parse.state.arena, out.signals[0])
    else {
        panic!("rep_21 should lower to one sub signal");
    };
    assert!(matches!(
        match_sig(&out.parse.state.arena, rhs),
        SigMatch::Int(4)
    ));
    let SigMatch::BinOp(BinOp::Add, add_lhs, add_rhs) = match_sig(&out.parse.state.arena, lhs)
    else {
        panic!("rep_21 left branch should be add");
    };
    assert!(matches!(
        match_sig(&out.parse.state.arena, add_lhs),
        SigMatch::Int(1)
    ));
    let SigMatch::BinOp(BinOp::Mul, mul_lhs, mul_rhs) = match_sig(&out.parse.state.arena, add_rhs)
    else {
        panic!("rep_21 nested branch should be mul");
    };
    assert!(matches!(
        match_sig(&out.parse.state.arena, mul_lhs),
        SigMatch::Int(2)
    ));
    assert!(matches!(
        match_sig(&out.parse.state.arena, mul_rhs),
        SigMatch::Int(3)
    ));
}

#[test]
fn corpus_feedback_simple_exposes_recursive_projection() {
    let out = compile_corpus("rep_23_feedback_simple.dsp");
    assert_eq!(out.process_arity.inputs, 1);
    assert_eq!(out.process_arity.outputs, 1);
    assert_eq!(out.signals.len(), 1);
    assert!(matches!(
        match_sig(&out.parse.state.arena, out.signals[0]),
        SigMatch::Proj(_, _)
    ));
}

fn assert_mul_input_const(arena: &TreeArena, sig: TreeId, expected_input: i64) {
    let SigMatch::BinOp(BinOp::Mul, a, b) = match_sig(arena, sig) else {
        panic!("branch should be Mul");
    };
    let am = match_sig(arena, a);
    let bm = match_sig(arena, b);
    let ok = matches!(
        (am, bm),
        (SigMatch::Input(i), SigMatch::Real(_))
            | (SigMatch::Real(_), SigMatch::Input(i))
            | (SigMatch::Input(i), SigMatch::Int(_))
            | (SigMatch::Int(_), SigMatch::Input(i))
            if i == expected_input
    );
    assert!(
        ok,
        "mul branch should combine input({expected_input}) with constant"
    );
}
