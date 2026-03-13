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
use ui::{ControlKind, UiGroupKind, UiMatch, match_ui};

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

fn expect_ui_group(
    out: &compiler::SignalCompileOutput,
    node: ui::UiId,
    expected_kind: UiGroupKind,
    expected_label: &str,
) -> Vec<ui::UiId> {
    let UiMatch::Group {
        kind,
        label,
        metadata: _,
        children,
    } = match_ui(&out.ui.arena, node)
    else {
        panic!("expected UI group node");
    };
    assert_eq!(kind, expected_kind);
    assert_eq!(label, expected_label);
    children
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
fn inline_partial_mul_with_trigger_argument_compiles_to_signal_mul() {
    let source = r#"
upfront(x) = (x-x') > 0.0;
decay(n,x) = x - (x>0.0)/n;
release(n) = + ~ decay(n);
trigger(n) = upfront : release(n) : >(0.0);
process = *(button("play") : trigger(128));
"#;

    let out = compile_inline("partial_mul_trigger.dsp", source);
    assert_eq!(out.process_arity.inputs, 1);
    assert_eq!(out.process_arity.outputs, 1);
    assert_eq!(out.signals.len(), 1);

    let SigMatch::BinOp(BinOp::Mul, lhs, rhs) = match_sig(&out.parse.state.arena, out.signals[0])
    else {
        panic!("partial mul trigger repro should lower to one mul signal");
    };
    assert!(
        matches!(match_sig(&out.parse.state.arena, lhs), SigMatch::Input(0))
            || matches!(match_sig(&out.parse.state.arena, rhs), SigMatch::Input(0)),
        "one mul operand should be the implicit input wire"
    );
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
fn corpus_higher_order_named_direct_apply_lowers_checkbox_ui() {
    let out = compile_corpus("rep_58_higher_order_named_direct_apply.dsp");
    assert_eq!(out.process_arity.inputs, 0);
    assert_eq!(out.process_arity.outputs, 1);
    assert_eq!(out.signals.len(), 1);
    assert!(matches!(
        match_sig(&out.parse.state.arena, out.signals[0]),
        SigMatch::Checkbox(_)
    ));
    let root_children = expect_ui_group(&out, out.ui.root, UiGroupKind::Horizontal, "top");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].kind, ControlKind::Checkbox);
    assert_eq!(out.ui.controls[0].label, "c");
}

#[test]
fn corpus_higher_order_named_argument_apply_lowers_checkbox_ui() {
    let out = compile_corpus("rep_59_higher_order_named_argument_apply.dsp");
    assert_eq!(out.process_arity.inputs, 0);
    assert_eq!(out.process_arity.outputs, 1);
    assert_eq!(out.signals.len(), 1);
    assert!(matches!(
        match_sig(&out.parse.state.arena, out.signals[0]),
        SigMatch::Checkbox(_)
    ));
    let root_children = expect_ui_group(&out, out.ui.root, UiGroupKind::Horizontal, "top");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].kind, ControlKind::Checkbox);
    assert_eq!(out.ui.controls[0].label, "c");
}

#[test]
fn inline_empty_root_group_uses_source_stem_in_ui_program() {
    let out = compile_inline(
        "empty_root_group.dsp",
        r#"process = vgroup("", checkbox("c"));"#,
    );
    let root_children =
        expect_ui_group(&out, out.ui.root, UiGroupKind::Vertical, "empty_root_group");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::InputControl(0)
    );
}

#[test]
fn inline_empty_root_group_prefers_declared_name_metadata() {
    let out = compile_inline(
        "ignored_root_name.dsp",
        "declare name \"main\";\nprocess = vgroup(\"\", checkbox(\"c\"));\n",
    );
    let root_children = expect_ui_group(&out, out.ui.root, UiGroupKind::Vertical, "main");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::InputControl(0)
    );
}

#[test]
fn corpus_group_label_substitution_reaches_compiler_ui_output() {
    let out = compile_corpus("rep_52_eval_label_group_path_subst.dsp");
    let root_children = expect_ui_group(&out, out.ui.root, UiGroupKind::Vertical, "main3");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].kind, ControlKind::HSlider);
    assert_eq!(out.ui.controls[0].label, "gain");
}

#[test]
fn corpus_sine_phasor_lowers_to_gain_times_sin_of_feedback_phase() {
    let out = compile_corpus("rep_38_sine_phasor.dsp");
    assert_eq!(out.process_arity.inputs, 0);
    assert_eq!(out.process_arity.outputs, out.signals.len());
    assert!(
        !out.signals.is_empty(),
        "rep_38 should expose at least one output signal"
    );

    for sig in &out.signals {
        let SigMatch::BinOp(BinOp::Mul, gain, carrier) = match_sig(&out.parse.state.arena, *sig)
        else {
            panic!("rep_38 should lower each output to gain * sin(phase)");
        };
        assert!(matches!(
            match_sig(&out.parse.state.arena, gain),
            SigMatch::HSlider(_) | SigMatch::VSlider(_) | SigMatch::NumEntry(_) | SigMatch::Real(_)
        ));
        assert!(matches!(
            match_sig(&out.parse.state.arena, carrier),
            SigMatch::Sin(_)
        ));
    }
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

#[test]
fn corpus_eval_label_widget_substitution_reaches_ui_controls() {
    let out = compile_corpus("rep_51_eval_label_widget_subst.dsp");
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].label, "gain3");
}

#[test]
fn corpus_eval_label_group_path_substitution_reaches_ui_controls() {
    let out = compile_corpus("rep_52_eval_label_group_path_subst.dsp");
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].label, "gain");
}

#[test]
fn corpus_eval_label_modulation_target_substitution_matches_ui_control() {
    let out = compile_corpus("rep_53_eval_label_modulation_target_subst.dsp");
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].label, "gain3");
}

#[test]
fn corpus_metadata_bearing_widget_label_splits_display_label_and_ui_metadata() {
    let out = compile_corpus("rep_56_noise_smoo_slider.dsp");
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].label, "gain");
    assert_eq!(
        out.ui.controls[0].metadata,
        vec![("style".to_owned(), "knob".to_owned())]
    );
}

#[test]
fn corpus_relative_widget_path_rebases_to_parent_group() {
    let out = compile_corpus("rep_60_ui_relative_widget_path.dsp");
    let root_children = expect_ui_group(&out, out.ui.root, UiGroupKind::Horizontal, "Foo");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].kind, ControlKind::HSlider);
    assert_eq!(out.ui.controls[0].label, "volume");
}

#[test]
fn corpus_typed_widget_path_creates_typed_group() {
    let out = compile_corpus("rep_61_ui_typed_widget_path.dsp");
    let root_children = expect_ui_group(&out, out.ui.root, UiGroupKind::Horizontal, "Oscillator");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].kind, ControlKind::HSlider);
    assert_eq!(out.ui.controls[0].label, "freq");
}

#[test]
fn corpus_relative_widget_path_metadata_survives_rebase() {
    let out = compile_corpus("rep_62_ui_relative_widget_path_metadata.dsp");
    let root_children = expect_ui_group(&out, out.ui.root, UiGroupKind::Horizontal, "Foo");
    assert_eq!(root_children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, root_children[0]),
        UiMatch::InputControl(0)
    );
    assert_eq!(out.ui.controls.len(), 1);
    assert_eq!(out.ui.controls[0].label, "gain");
    assert_eq!(
        out.ui.controls[0].metadata,
        vec![("style".to_owned(), "knob".to_owned())]
    );
}

#[test]
fn inline_group_label_metadata_splits_display_label_and_group_metadata() {
    let out = compile_inline(
        "group_metadata",
        "process = hgroup(\"top [tooltip:hello]\", checkbox(\"c\"));\n",
    );
    let UiMatch::Group {
        kind,
        label,
        metadata,
        children,
    } = match_ui(&out.ui.arena, out.ui.root)
    else {
        panic!("expected root grouped UI");
    };
    assert_eq!(kind, UiGroupKind::Horizontal);
    assert_eq!(label, "top");
    assert_eq!(metadata, vec![("tooltip".to_owned(), "hello".to_owned())]);
    assert_eq!(children.len(), 1);
    assert_eq!(
        match_ui(&out.ui.arena, children[0]),
        UiMatch::InputControl(0)
    );
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
        (SigMatch::Input(i), SigMatch::HSlider(_))
            | (SigMatch::HSlider(_), SigMatch::Input(i))
            if i == expected_input
    );
    assert!(
        ok,
        "mul branch should combine input({expected_input}) with hslider"
    );
}
