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
