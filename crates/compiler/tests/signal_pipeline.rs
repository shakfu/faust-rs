//! Integration tests for `signal_pipeline`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

use std::path::PathBuf;

use compiler::Compiler;
use parser::CompilationMetadataKey;
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

fn compile_inline(name: &str, source: &str) -> compiler::SignalCompileOutput {
    Compiler::new()
        .compile_source_to_signals(name, source)
        .unwrap_or_else(|e| panic!("failed to compile {name} to signals: {e}"))
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

#[test]
fn corpus_two_in_two_out_ui_lowers_to_input_slider_muls() {
    let out = compile_corpus("rep_10_two_in_two_out_ui.dsp");
    assert_eq!(out.process_arity.inputs, 2);
    assert_eq!(out.process_arity.outputs, 2);
    assert_eq!(out.signals.len(), 2);

    assert_mul_input_ui(&out.parse.state.arena, out.signals[0], 0);
    assert_mul_input_ui(&out.parse.state.arena, out.signals[1], 1);
}

#[test]
fn corpus_parallel_mix_lowers_to_sum_of_two_scaled_inputs() {
    let out = compile_corpus("rep_22_parallel_mix.dsp");
    assert_eq!(out.process_arity.inputs, 1);
    assert_eq!(out.process_arity.outputs, 1);
    assert_eq!(out.signals.len(), 1);

    let SigMatch::BinOp(BinOp::Add, lhs, rhs) = match_sig(&out.parse.state.arena, out.signals[0])
    else {
        panic!("rep_22 should lower to one add signal");
    };
    assert_mul_input_const(&out.parse.state.arena, lhs, 0);
    assert_mul_input_const(&out.parse.state.arena, rhs, 0);
}

#[test]
fn corpus_environment_waveform_combines_access_and_waveform_outputs() {
    let out = compile_corpus("rep_20_environment_waveform.dsp");
    assert_eq!(out.process_arity.inputs, 1);
    assert_eq!(out.process_arity.outputs, 3);
    assert_eq!(out.signals.len(), 3);

    assert_eq!(
        match_sig(&out.parse.state.arena, out.signals[0]),
        SigMatch::Input(0)
    );
    assert_eq!(
        match_sig(&out.parse.state.arena, out.signals[1]),
        SigMatch::Int(3)
    );

    let SigMatch::Waveform(values) = match_sig(&out.parse.state.arena, out.signals[2]) else {
        panic!("rep_20 third output should be waveform");
    };
    assert_eq!(values.len(), 3);
    assert!(matches!(
        match_sig(&out.parse.state.arena, values[0]),
        SigMatch::Int(1)
    ));
    assert!(matches!(
        match_sig(&out.parse.state.arena, values[1]),
        SigMatch::Int(-2)
    ));
    assert!(matches!(
        match_sig(&out.parse.state.arena, values[2]),
        SigMatch::Real(_)
    ));
}

#[test]
fn corpus_nonlinear_clip_lowers_to_min_max_shape() {
    let out = compile_corpus("rep_07_nonlinear_clip.dsp");
    assert_eq!(out.process_arity.inputs, 1);
    assert_eq!(out.process_arity.outputs, 1);
    assert_eq!(out.signals.len(), 1);

    let SigMatch::Max(lo, inner) = match_sig(&out.parse.state.arena, out.signals[0]) else {
        panic!("rep_07 should lower to max(lo, ...)");
    };
    assert!(matches!(
        match_sig(&out.parse.state.arena, lo),
        SigMatch::Real(_)
    ));
    let SigMatch::Min(hi, mul) = match_sig(&out.parse.state.arena, inner) else {
        panic!("rep_07 inner should lower to min(hi, ...)");
    };
    assert!(matches!(
        match_sig(&out.parse.state.arena, hi),
        SigMatch::Real(_)
    ));
    assert_mul_input_const(&out.parse.state.arena, mul, 0);
}

#[test]
fn corpus_primitive_family_includes_pow_output() {
    let out = compile_corpus("rep_19_primitive_family.dsp");
    assert_eq!(out.process_arity.outputs, 10);
    assert_eq!(out.signals.len(), 10);
    assert!(matches!(
        match_sig(&out.parse.state.arena, out.signals[0]),
        SigMatch::Pow(_, _)
    ));
}

#[test]
fn corpus_extended_primitives_cover_unary_and_binary_signal_nodes() {
    let out = compile_corpus("rep_31_extended_primitives.dsp");
    assert_eq!(out.process_arity.inputs, 0);
    assert_eq!(out.process_arity.outputs, 20);
    assert_eq!(out.signals.len(), 20);

    let got: Vec<SigMatch<'_>> = out
        .signals
        .iter()
        .copied()
        .map(|s| match_sig(&out.parse.state.arena, s))
        .collect();

    assert!(matches!(got[0], SigMatch::Acos(_)));
    assert!(matches!(got[1], SigMatch::Asin(_)));
    assert!(matches!(got[2], SigMatch::Atan(_)));
    assert!(matches!(got[3], SigMatch::Atan2(_, _)));
    assert!(matches!(got[4], SigMatch::Cos(_)));
    assert!(matches!(got[5], SigMatch::Sin(_)));
    assert!(matches!(got[6], SigMatch::Tan(_)));
    assert!(matches!(got[7], SigMatch::Exp(_)));
    assert!(matches!(got[8], SigMatch::Log(_)));
    assert!(matches!(got[9], SigMatch::Log10(_)));
    assert!(matches!(got[10], SigMatch::Sqrt(_)));
    assert!(matches!(got[11], SigMatch::Abs(_)));
    assert!(matches!(got[12], SigMatch::Min(_, _)));
    assert!(matches!(got[13], SigMatch::Max(_, _)));
    assert!(matches!(got[14], SigMatch::Fmod(_, _)));
    assert!(matches!(got[15], SigMatch::Remainder(_, _)));
    assert!(matches!(got[16], SigMatch::Floor(_)));
    assert!(matches!(got[17], SigMatch::Ceil(_)));
    assert!(matches!(got[18], SigMatch::Rint(_)));
    assert!(matches!(got[19], SigMatch::Round(_)));
}

#[test]
fn corpus_sine_phasor_lowers_to_gain_times_sin_of_feedback_phase() {
    let out = compile_corpus("rep_38_sine_phasor.dsp");
    assert_eq!(out.process_arity.inputs, 0);
    assert_eq!(out.process_arity.outputs, 1);
    assert_eq!(out.signals.len(), 1);

    let SigMatch::BinOp(BinOp::Mul, gain, carrier) =
        match_sig(&out.parse.state.arena, out.signals[0])
    else {
        panic!("rep_38 should lower to gain * carrier");
    };
    assert!(matches!(
        match_sig(&out.parse.state.arena, gain),
        SigMatch::HSlider(_, _, _, _, _)
            | SigMatch::VSlider(_, _, _, _, _)
            | SigMatch::NumEntry(_, _, _, _, _)
            | SigMatch::Real(_)
    ));
    assert!(matches!(
        match_sig(&out.parse.state.arena, carrier),
        SigMatch::Sin(_)
    ));
}

#[test]
fn corpus_metadata_master_is_exposed_in_compilation_output() {
    let out = compile_corpus("rep_40_metadata_master.dsp");
    let values = out
        .compilation_metadata
        .entries()
        .get(&CompilationMetadataKey::global("name"))
        .expect("master metadata key should exist");
    assert!(values.contains("main"));
}

#[test]
fn corpus_metadata_import_is_prefixed_like_cpp() {
    let out = compile_corpus("rep_41_metadata_import.dsp");
    let imported = corpus_path("metadata/imported_meta.dsp")
        .canonicalize()
        .expect("imported metadata file should canonicalize");
    let values = out
        .compilation_metadata
        .entries()
        .get(&CompilationMetadataKey::scoped(
            imported.to_string_lossy().into_owned(),
            "author",
        ))
        .expect("imported metadata key should exist");
    assert!(values.contains("imported-author"));
}

#[test]
fn corpus_component_metadata_is_aggregated_through_eval_loading() {
    let out = compile_corpus("rep_42_component_metadata.dsp");
    let child = corpus_path("metadata/component_child.dsp")
        .canonicalize()
        .expect("component child should canonicalize");
    let values = out
        .compilation_metadata
        .entries()
        .get(&CompilationMetadataKey::scoped(
            child.to_string_lossy().into_owned(),
            "author",
        ))
        .expect("component metadata key should exist");
    assert!(values.contains("component-author"));
}

#[test]
fn corpus_library_metadata_is_aggregated_through_eval_loading() {
    let out = compile_corpus("rep_43_library_metadata.dsp");
    let child = corpus_path("metadata/library_child.dsp")
        .canonicalize()
        .expect("library child should canonicalize");
    let values = out
        .compilation_metadata
        .entries()
        .get(&CompilationMetadataKey::scoped(
            child.to_string_lossy().into_owned(),
            "author",
        ))
        .expect("library metadata key should exist");
    assert!(values.contains("library-author"));
}

#[test]
fn corpus_modulation_wrappers_without_matching_widgets_reduce_to_identity() {
    for file in [
        "rep_32_modulation_single.dsp",
        "rep_33_modulation_chain.dsp",
    ] {
        let out = compile_corpus(file);
        assert_eq!(out.process_arity.inputs, 1, "{file} should keep 1 input");
        assert_eq!(out.process_arity.outputs, 1, "{file} should keep 1 output");
        assert_eq!(out.signals.len(), 1, "{file} should lower to one signal");
        assert_eq!(
            match_sig(&out.parse.state.arena, out.signals[0]),
            SigMatch::Input(0),
            "{file} should reduce to input(0)"
        );
    }
}

#[test]
fn closure_captured_case_results_keep_distinct_environments() {
    let source = r#"
make(x) = case { (0) => x; };
process = make(1)(0) + make(2)(0);
"#;
    let out = compile_inline(
        "closure_captured_case_results_keep_distinct_environments.dsp",
        source,
    );
    assert_eq!(out.process_arity.inputs, 0);
    assert_eq!(out.process_arity.outputs, 1);
    assert_eq!(out.signals.len(), 1);
    let SigMatch::BinOp(BinOp::Add, lhs, rhs) = match_sig(&out.parse.state.arena, out.signals[0])
    else {
        panic!("captured case results should stay as one add signal");
    };
    assert!(matches!(
        match_sig(&out.parse.state.arena, lhs),
        SigMatch::Int(1)
    ));
    assert!(matches!(
        match_sig(&out.parse.state.arena, rhs),
        SigMatch::Int(2)
    ));
}

fn assert_mul_input_const(arena: &TreeArena, sig: TreeId, expected_input: i32) {
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

fn assert_mul_input_ui(arena: &TreeArena, sig: TreeId, expected_input: i32) {
    let SigMatch::BinOp(BinOp::Mul, a, b) = match_sig(arena, sig) else {
        panic!("branch should be Mul");
    };
    let am = match_sig(arena, a);
    let bm = match_sig(arena, b);
    let ok = matches!(
        (am, bm),
        (SigMatch::Input(i), SigMatch::HSlider(_, _, _, _, _))
            | (SigMatch::HSlider(_, _, _, _, _), SigMatch::Input(i))
            if i == expected_input
    );
    assert!(
        ok,
        "mul branch should combine input({expected_input}) with hslider"
    );
}
