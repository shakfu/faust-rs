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

fn compile_c_with_lane(file: &str, lane: SignalFirLane) -> String {
    let compiler = Compiler::new();
    let path = corpus_path(file);
    compiler
        .compile_file_default_to_c_with_lane(
            &path,
            &codegen::backends::c::COptions::default(),
            lane,
        )
        .unwrap_or_else(|e| panic!("{file} C compilation failed for lane {lane:?}: {e}"))
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
    assert!(
        !fast.contains("frs_"),
        "Step 2G fast-lane output should not contain frs_* shims"
    );
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
    assert!(
        !fast.contains("frs_"),
        "Step 2F fast-lane output should not contain frs_* shims"
    );
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
    assert!(
        !fast.contains("frs_"),
        "Step 2F fast-lane output should not contain frs_* shims"
    );
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
    assert!(
        !fast.contains("frs_"),
        "Step 2F fast-lane output should not contain frs_* shims"
    );
}

#[test]
fn legacy_and_fastlane_both_compile_table_fixtures() {
    for file in [
        "rep_34_table_rdtable_readonly_const.dsp",
        "rep_35_table_rwtable_runtime_write.dsp",
        "rep_36_table_rdtable_negative_index.dsp",
        "rep_37_table_rwtable_negative_indices.dsp",
    ] {
        let legacy = compile_cpp_with_lane(file, SignalFirLane::LegacyBridge);
        let fast = compile_cpp_with_lane(file, SignalFirLane::TransformFastLane);
        assert!(
            legacy.contains("class "),
            "legacy lane should compile table fixture {file}"
        );
        assert!(
            fast.contains("class "),
            "fast lane should compile table fixture {file}"
        );
        assert!(
            !fast.contains("frs_"),
            "fast lane output should not contain frs_* shim names for {file}"
        );
    }
}

#[test]
fn fastlane_cpp_lifecycle_order_matches_faust_instance_init_flow() {
    let fast = compile_cpp_with_lane(
        "rep_10_two_in_two_out_ui.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(fast.contains("void instanceInit(int sample_rate)"));
    let instance_init_sig = "virtual void instanceInit(int sample_rate) {";
    let instance_init_start = fast
        .find(instance_init_sig)
        .expect("instanceInit signature should be present");
    let instance_init_body = &fast[instance_init_start..];
    let constants_i = instance_init_body
        .find("instanceConstants(sample_rate);")
        .expect("instanceConstants call should be present");
    let reset_i = instance_init_body
        .find("instanceResetUserInterface();")
        .expect("instanceResetUserInterface call should be present");
    let clear_i = instance_init_body
        .find("instanceClear();")
        .expect("instanceClear call should be present");
    assert!(
        constants_i < reset_i && reset_i < clear_i,
        "instanceInit should call constants -> resetUI -> clear in order"
    );
}

#[test]
fn dump_c_fastlane_compiles_fixture() {
    let fast = compile_c_with_lane("rep_01_passthrough.dsp", SignalFirLane::TransformFastLane);
    assert!(fast.contains("typedef struct {"));
    assert!(fast.contains("void computerep_01_passthrough("));
}

#[test]
fn legacy_and_fastlane_both_compile_c_table_fixtures_without_shims() {
    for file in [
        "rep_34_table_rdtable_readonly_const.dsp",
        "rep_35_table_rwtable_runtime_write.dsp",
        "rep_36_table_rdtable_negative_index.dsp",
        "rep_37_table_rwtable_negative_indices.dsp",
    ] {
        let legacy = compile_c_with_lane(file, SignalFirLane::LegacyBridge);
        let fast = compile_c_with_lane(file, SignalFirLane::TransformFastLane);
        assert!(
            legacy.contains("void compute"),
            "legacy lane should compile C fixture {file}"
        );
        assert!(
            fast.contains("void compute"),
            "fast lane should compile C fixture {file}"
        );
        assert!(
            !fast.contains("frs_"),
            "fast lane C output should not contain frs_* shim names for {file}"
        );
    }
}

#[test]
fn fastlane_c_lifecycle_order_matches_faust_instance_init_flow() {
    let fast = compile_c_with_lane(
        "rep_10_two_in_two_out_ui.dsp",
        SignalFirLane::TransformFastLane,
    );
    let instance_init_sig = "void instanceInitrep_10_two_in_two_out_ui(rep_10_two_in_two_out_ui* dsp, int sample_rate) {";
    let instance_init_start = fast
        .find(instance_init_sig)
        .expect("instanceInit signature should be present");
    let instance_init_body = &fast[instance_init_start..];
    let constants_i = instance_init_body
        .find("instanceConstantsrep_10_two_in_two_out_ui(dsp, sample_rate);")
        .expect("instanceConstants call should be present");
    let reset_i = instance_init_body
        .find("instanceResetUserInterfacerep_10_two_in_two_out_ui(dsp);")
        .expect("instanceResetUserInterface call should be present");
    let clear_i = instance_init_body
        .find("instanceClearrep_10_two_in_two_out_ui(dsp);")
        .expect("instanceClear call should be present");
    assert!(
        constants_i < reset_i && reset_i < clear_i,
        "instanceInit should call constants -> resetUI -> clear in order"
    );
}
