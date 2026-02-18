use compiler::{Compiler, SignalFirLane};
use std::path::PathBuf;

fn corpus_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("corpus")
        .join(file)
}

#[test]
fn dump_cpp_fastlane_compiles_fixture() {
    let compiler = Compiler::new();
    let path = corpus_path("rep_01_passthrough.dsp");
    let cpp = compiler
        .compile_file_default_to_cpp_with_lane(
            &path,
            &codegen::backends::cpp::CppOptions::default(),
            SignalFirLane::TransformFastLane,
        )
        .unwrap_or_else(|e| panic!("fast-lane C++ compilation failed: {e}"));
    assert!(cpp.contains("class rep_01_passthrough : public dsp"));
}

fn compile_cpp_with_lane(file: &str, lane: SignalFirLane) -> String {
    let compiler = Compiler::new();
    let path = corpus_path(file);
    compiler
        .compile_file_default_to_cpp_with_lane(
            &path,
            &codegen::backends::cpp::CppOptions::default(),
            lane,
        )
        .unwrap_or_else(|e| panic!("{file} C++ compilation failed for lane {lane:?}: {e}"))
}

#[test]
fn legacy_and_fastlane_both_compile_lowpass_feedback_fixture() {
    let legacy = compile_cpp_with_lane("rep_05_one_pole_lowpass.dsp", SignalFirLane::LegacyBridge);
    let fast = compile_cpp_with_lane(
        "rep_05_one_pole_lowpass.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(legacy.contains("class rep_05_one_pole_lowpass : public dsp"));
    assert!(fast.contains("class rep_05_one_pole_lowpass : public dsp"));
    assert!(fast.contains("void compute("));
}

#[test]
fn legacy_and_fastlane_both_compile_feedback_projection_fixture() {
    let legacy = compile_cpp_with_lane("rep_23_feedback_simple.dsp", SignalFirLane::LegacyBridge);
    let fast = compile_cpp_with_lane(
        "rep_23_feedback_simple.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(legacy.contains("class rep_23_feedback_simple : public dsp"));
    assert!(fast.contains("class rep_23_feedback_simple : public dsp"));
    assert!(fast.contains("void compute("));
    assert!(
        !fast.contains("frs_proj"),
        "Step 2C.2 should remove proj placeholder names from fast-lane output"
    );
    assert!(
        !fast.contains("frs_rec"),
        "Step 2C.2 should remove rec placeholder names from fast-lane output"
    );
}

#[test]
fn legacy_and_fastlane_both_compile_environment_waveform_fixture() {
    let legacy = compile_cpp_with_lane(
        "rep_20_environment_waveform.dsp",
        SignalFirLane::LegacyBridge,
    );
    let fast = compile_cpp_with_lane(
        "rep_20_environment_waveform.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(legacy.contains("class rep_20_environment_waveform : public dsp"));
    assert!(fast.contains("class rep_20_environment_waveform : public dsp"));
    assert!(fast.contains("void compute("));
}

#[test]
fn legacy_and_fastlane_both_compile_extended_primitives_fixture() {
    let legacy = compile_cpp_with_lane(
        "rep_31_extended_primitives.dsp",
        SignalFirLane::LegacyBridge,
    );
    let fast = compile_cpp_with_lane(
        "rep_31_extended_primitives.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(legacy.contains("class rep_31_extended_primitives : public dsp"));
    assert!(fast.contains("class rep_31_extended_primitives : public dsp"));
    assert!(fast.contains("void compute("));
}

#[test]
fn legacy_and_fastlane_both_compile_nonlinear_clip_fixture() {
    let legacy = compile_cpp_with_lane("rep_07_nonlinear_clip.dsp", SignalFirLane::LegacyBridge);
    let fast = compile_cpp_with_lane(
        "rep_07_nonlinear_clip.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(legacy.contains("class rep_07_nonlinear_clip : public dsp"));
    assert!(fast.contains("class rep_07_nonlinear_clip : public dsp"));
    assert!(fast.contains("void compute("));
}

#[test]
fn fastlane_ui_fixture_uses_native_ui_path_without_slider_shims() {
    let fast = compile_cpp_with_lane(
        "rep_10_two_in_two_out_ui.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(fast.contains("class rep_10_two_in_two_out_ui : public dsp"));
    assert!(fast.contains("void buildUserInterface("));
    assert!(
        !fast.contains("frs_hslider"),
        "UI sliders should use native FIR UI instructions, not frs_* shims"
    );
    assert!(
        !fast.contains("frs_vslider"),
        "UI sliders should use native FIR UI instructions, not frs_* shims"
    );
}
