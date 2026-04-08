//! Integration tests for `signal_fir_lane`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

use codegen::backends::interp::{FbcDspInstance, InterpOptions, read_fbc};
use compiler::{Compiler, RealType, SignalFirLane};
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
    assert!(cpp.contains("class mydsp : public dsp"));
}

#[test]
fn fastlane_cpp_honors_explicit_class_name_option() {
    let cpp = compile_cpp_with_class_name(
        "rep_56_noise_smoo_slider.dsp",
        SignalFirLane::TransformFastLane,
        "customdsp",
    );
    assert!(cpp.contains("class customdsp : public dsp"));
    assert!(cpp.contains("#define FAUSTCLASS customdsp"));
    assert!(!cpp.contains("class mydsp : public dsp"));
}

#[test]
fn fastlane_cpp_honors_explicit_super_class_name_option() {
    let cpp = compile_cpp_with_names(
        "rep_56_noise_smoo_slider.dsp",
        SignalFirLane::TransformFastLane,
        "customdsp",
        "faust_dsp",
    );
    assert!(cpp.contains("class customdsp : public faust_dsp"));
    assert!(!cpp.contains("class customdsp : public dsp"));
}

#[test]
fn fastlane_c_honors_explicit_class_name_option() {
    let c_code = compile_c_with_class_name(
        "rep_56_noise_smoo_slider.dsp",
        SignalFirLane::TransformFastLane,
        "customdsp",
    );
    assert!(c_code.contains("} customdsp;"));
    assert!(c_code.contains("void computecustomdsp(customdsp* dsp"));
    assert!(!c_code.contains("} mydsp;"));
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

fn compile_cpp_source_with_lane(source_name: &str, source: &str, lane: SignalFirLane) -> String {
    let compiler = Compiler::new();
    compiler
        .compile_source_to_cpp_with_lane(
            source_name,
            source,
            &codegen::backends::cpp::CppOptions::default(),
            lane,
        )
        .unwrap_or_else(|e| panic!("{source_name} C++ compilation failed for lane {lane:?}: {e}"))
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

fn compile_cpp_with_class_name(file: &str, lane: SignalFirLane, class_name: &str) -> String {
    compile_cpp_with_names(file, lane, class_name, "dsp")
}

fn compile_cpp_with_names(
    file: &str,
    lane: SignalFirLane,
    class_name: &str,
    super_class_name: &str,
) -> String {
    let compiler = Compiler::new();
    let path = corpus_path(file);
    let options = codegen::backends::cpp::CppOptions {
        class_name: Some(class_name.to_owned()),
        super_class_name: Some(super_class_name.to_owned()),
        ..codegen::backends::cpp::CppOptions::default()
    };
    compiler
        .compile_file_default_to_cpp_with_lane(&path, &options, lane)
        .unwrap_or_else(|e| {
            panic!(
                "{file} C++ compilation failed for lane {lane:?}, class name {class_name}, super class name {super_class_name}: {e}"
            )
        })
}

fn compile_c_with_class_name(file: &str, lane: SignalFirLane, class_name: &str) -> String {
    let compiler = Compiler::new();
    let path = corpus_path(file);
    let options = codegen::backends::c::COptions {
        class_name: Some(class_name.to_owned()),
        ..codegen::backends::c::COptions::default()
    };
    compiler
        .compile_file_default_to_c_with_lane(&path, &options, lane)
        .unwrap_or_else(|e| {
            panic!("{file} C compilation failed for lane {lane:?} and class name {class_name}: {e}")
        })
}

fn compile_cpp_with_lane_and_real_type(
    file: &str,
    lane: SignalFirLane,
    real_type: RealType,
) -> String {
    let compiler = Compiler::new().with_real_type(real_type);
    let path = corpus_path(file);
    compiler
        .compile_file_default_to_cpp_with_lane(
            &path,
            &codegen::backends::cpp::CppOptions::default(),
            lane,
        )
        .unwrap_or_else(|e| {
            panic!(
                "{file} C++ compilation failed for lane {lane:?} and real type {real_type:?}: {e}"
            )
        })
}

fn compile_c_with_lane_and_real_type(
    file: &str,
    lane: SignalFirLane,
    real_type: RealType,
) -> String {
    let compiler = Compiler::new().with_real_type(real_type);
    let path = corpus_path(file);
    compiler
        .compile_file_default_to_c_with_lane(
            &path,
            &codegen::backends::c::COptions::default(),
            lane,
        )
        .unwrap_or_else(|e| {
            panic!("{file} C compilation failed for lane {lane:?} and real type {real_type:?}: {e}")
        })
}

#[test]
fn legacy_and_fastlane_both_compile_lowpass_feedback_fixture() {
    let legacy = compile_cpp_with_lane("rep_05_one_pole_lowpass.dsp", SignalFirLane::LegacyBridge);
    let fast = compile_cpp_with_lane(
        "rep_05_one_pole_lowpass.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(legacy.contains("class mydsp : public dsp"));
    assert!(fast.contains("class mydsp : public dsp"));
    assert!(fast.contains("void compute("));
}

#[test]
fn fastlane_delay_echo_uses_circular_delay_line_and_iota_in_c_and_cpp() {
    let fast_cpp = compile_cpp_with_lane("rep_04_delay_echo.dsp", SignalFirLane::TransformFastLane);
    assert!(fast_cpp.contains("class mydsp : public dsp"));
    assert!(fast_cpp.contains("int fIOTA;"));
    assert!(fast_cpp.contains("fVec"));
    assert!(
        fast_cpp.contains("[(fIOTA & 4095)]"),
        "C++ fast-lane should mask the delay-line write index"
    );
    // `mem` (Delay1) now uses the Shift strategy (2-element buffer, no fIOTA masking)
    // when max_copy_delay >= 1 (default 16). The large @(2205) line still uses fIOTA.
    assert!(
        !fast_cpp.contains("[(fIOTA & 1)]"),
        "C++ fast-lane should use 2-element shift buffer for delay1, not fIOTA & 1"
    );
    assert!(
        fast_cpp.contains("[((fIOTA - 2205) & 4095)]"),
        "C++ fast-lane should read the delay line through a masked circular index"
    );
    assert!(
        fast_cpp.contains("fIOTA = (fIOTA + 1);"),
        "C++ fast-lane should increment fIOTA once per sample"
    );
    assert!(
        fast_cpp.contains("for (int lDelay") && fast_cpp.contains("< 4096; ++lDelay"),
        "C++ fast-lane should zero the fixed-size delay line in instanceClear"
    );

    let fast_c = compile_c_with_lane("rep_04_delay_echo.dsp", SignalFirLane::TransformFastLane);
    assert!(fast_c.contains("int fIOTA;"));
    assert!(fast_c.contains("fVec"));
    assert!(
        fast_c.contains("[(dsp->fIOTA & 4095)]"),
        "C fast-lane should mask the delay-line write index"
    );
    // `mem` (Delay1) now uses the Shift strategy (2-element buffer, no fIOTA masking).
    assert!(
        !fast_c.contains("[(dsp->fIOTA & 1)]"),
        "C fast-lane should use 2-element shift buffer for delay1, not fIOTA & 1"
    );
    assert!(
        fast_c.contains("[((dsp->fIOTA - 2205) & 4095)]"),
        "C fast-lane should read the delay line through a masked circular index"
    );
    assert!(
        fast_c.contains("dsp->fIOTA = (dsp->fIOTA + 1);"),
        "C fast-lane should increment fIOTA once per sample"
    );
    assert!(
        fast_c.contains("for (int lDelay") && fast_c.contains("< 4096;") && fast_c.contains("= 0;"),
        "C fast-lane should zero the fixed-size delay line in instanceClear"
    );
}

#[test]
fn fastlane_interp_delay_lines_do_not_overrun_after_ring_wrap() {
    let compiler = Compiler::new();
    let path = corpus_path("rep_55_sine_phasor_echo_feedback.dsp");
    let fbc = compiler
        .compile_file_default_to_interp_with_lane(
            &path,
            &InterpOptions::default(),
            SignalFirLane::TransformFastLane,
        )
        .unwrap_or_else(|e| panic!("fast-lane interp compilation failed: {e}"));

    let mut reader = std::io::Cursor::new(fbc);
    let mut factory = read_fbc::<f32>(&mut reader)
        .unwrap_or_else(|e| panic!("interp bytecode parse failed: {e}"));
    let mut instance = FbcDspInstance::new(&mut factory);
    instance.init(48_000);

    let frame_count = 5_000;
    let mut out0 = vec![0.0_f32; frame_count];
    let mut out1 = vec![0.0_f32; frame_count];
    let mut outputs: [&mut [f32]; 2] = [&mut out0, &mut out1];

    instance
        .try_compute(frame_count as i32, &[], &mut outputs)
        .unwrap_or_else(|e| panic!("interp execution should survive delay-ring wrap: {e}"));

    assert!(
        outputs[0].iter().all(|sample| sample.is_finite()),
        "output0 should stay finite across the delay-ring wrap"
    );
    assert!(
        outputs[1].iter().all(|sample| sample.is_finite()),
        "output1 should stay finite across the delay-ring wrap"
    );
}

#[test]
fn legacy_and_fastlane_both_compile_feedback_projection_fixture() {
    let legacy = compile_cpp_with_lane("rep_23_feedback_simple.dsp", SignalFirLane::LegacyBridge);
    let fast = compile_cpp_with_lane(
        "rep_23_feedback_simple.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(legacy.contains("class mydsp : public dsp"));
    assert!(fast.contains("class mydsp : public dsp"));
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
    assert!(legacy.contains("class mydsp : public dsp"));
    assert!(fast.contains("class mydsp : public dsp"));
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
    assert!(legacy.contains("class mydsp : public dsp"));
    assert!(fast.contains("class mydsp : public dsp"));
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
    assert!(legacy.contains("class mydsp : public dsp"));
    assert!(fast.contains("class mydsp : public dsp"));
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
    assert!(fast.contains("class mydsp : public dsp"));
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
fn legacy_and_fastlane_both_compile_sine_phasor_fixture() {
    let legacy = compile_cpp_with_lane("rep_38_sine_phasor.dsp", SignalFirLane::LegacyBridge);
    let fast = compile_cpp_with_lane("rep_38_sine_phasor.dsp", SignalFirLane::TransformFastLane);
    assert!(legacy.contains("class mydsp : public dsp"));
    assert!(fast.contains("class mydsp : public dsp"));
    assert!(fast.contains("void compute("));
    assert!(!fast.contains("frs_"));
    assert!(fast.contains("fHslider"));
    assert!(!fast.contains("fUiCtl"));
    assert!(fast.contains("ui_interface->openVerticalBox(\"rep_38_sine_phasor\");"));
    assert!(fast.contains("ui_interface->closeBox();"));
    assert_eq!(
        fast.matches("void instanceResetUserInterface() {").count(),
        1,
        "instanceResetUserInterface should be emitted once"
    );
    assert_eq!(
        fast.matches("void instanceClear() {").count(),
        1,
        "instanceClear should be emitted once"
    );
    assert!(
        fast.contains("float fRec") && fast.contains("[2];"),
        "fast lane should lower phasor recursion to a 2-slot float array"
    );
    // Simple 2-slot recursion is now aligned with Faust C++:
    // `fRec[0] = ... + fRec[1]; out = fRec[0]; fRec[1] = fRec[0];`
    // Keep accepting the older circular-buffer form as well for robustness in
    // case future merged-delay recursion paths appear in this fixture.
    let has_direct_two_slot = fast.contains("[0] = (fRec")
        && fast.contains("[1] +")
        && fast.contains("float fTemp")
        && fast.contains("[1] = fTemp");
    let has_inline_circ =
        fast.contains("[(fIOTA & 1)] = (fRec") && fast.contains("[((fIOTA - 1) & 1)] +");
    let has_cse_circ =
        fast.contains("fIOTA & 1") && fast.contains("(fIOTA - 1) & 1") && fast.contains("fTemp");
    assert!(
        has_direct_two_slot || has_inline_circ || has_cse_circ,
        "fast lane should lower phasor recursion to either direct 2-slot or circular-buffer form"
    );

    let legacy_c = compile_c_with_lane("rep_38_sine_phasor.dsp", SignalFirLane::LegacyBridge);
    let fast_c = compile_c_with_lane("rep_38_sine_phasor.dsp", SignalFirLane::TransformFastLane);
    assert!(legacy_c.contains("void computemydsp("));
    assert!(fast_c.contains("void computemydsp("));
    assert!(!fast_c.contains("frs_"));
    assert!(fast_c.contains("fHslider"));
    assert!(!fast_c.contains("fUiCtl"));
    assert!(
        fast_c.contains("float fRec") && fast_c.contains("[2];"),
        "fast lane C backend should keep recursion as a 2-slot array"
    );
    assert!(fast_c.contains(
        "ui_interface->openVerticalBox(ui_interface->uiInterface, \"rep_38_sine_phasor\");"
    ));
    assert!(fast_c.contains("ui_interface->closeBox(ui_interface->uiInterface);"));
}

#[test]
fn fastlane_cpp_root_group_prefers_declared_name_metadata() {
    let fast = compile_cpp_with_lane(
        "rep_40_metadata_master.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(fast.contains("ui_interface->openVerticalBox(\"main\");"));
}

#[test]
fn fastlane_cpp_preserves_metadata_bearing_ui_labels() {
    let fast = compile_cpp_with_lane(
        "rep_56_noise_smoo_slider.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(fast.contains("ui_interface->openVerticalBox(\"rep_56_noise_smoo_slider\");"));
    assert!(fast.contains("ui_interface->declare(&fHslider0, \"style\", \"knob\");"));
    assert!(fast.contains("ui_interface->addHorizontalSlider(\"gain\", &fHslider0"));
    assert!(fast.contains("ui_interface->closeBox();"));
}

#[test]
fn fastlane_c_preserves_metadata_bearing_ui_labels() {
    let fast = compile_c_with_lane(
        "rep_56_noise_smoo_slider.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(fast.contains(
        "ui_interface->openVerticalBox(ui_interface->uiInterface, \"rep_56_noise_smoo_slider\");"
    ));
    assert!(fast.contains(
        "ui_interface->declare(ui_interface->uiInterface, &dsp->fHslider0, \"style\", \"knob\");"
    ));
    assert!(fast.contains(
        "ui_interface->addHorizontalSlider(ui_interface->uiInterface, \"gain\", &dsp->fHslider0"
    ));
    assert!(fast.contains("ui_interface->closeBox(ui_interface->uiInterface);"));
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
    assert!(fast.contains("void computemydsp("));
}

#[test]
fn fastlane_cpp_double_keeps_faustfloat_interface_and_uses_double_internal_ops() {
    let single = compile_cpp_with_lane_and_real_type(
        "rep_01_passthrough.dsp",
        SignalFirLane::TransformFastLane,
        RealType::Float32,
    );
    let double = compile_cpp_with_lane_and_real_type(
        "rep_01_passthrough.dsp",
        SignalFirLane::TransformFastLane,
        RealType::Float64,
    );

    assert!(single.contains("#define FAUSTFLOAT float"));
    assert!(double.contains("#define FAUSTFLOAT float"));
    assert!(single.contains("output0[i0] = ((FAUSTFLOAT)(((float)(input0[i0]))));"));
    assert!(double.contains("output0[i0] = ((FAUSTFLOAT)(((double)(input0[i0]))));"));
}

#[test]
fn fastlane_c_double_keeps_faustfloat_interface_and_uses_double_internal_ops() {
    let single = compile_c_with_lane_and_real_type(
        "rep_01_passthrough.dsp",
        SignalFirLane::TransformFastLane,
        RealType::Float32,
    );
    let double = compile_c_with_lane_and_real_type(
        "rep_01_passthrough.dsp",
        SignalFirLane::TransformFastLane,
        RealType::Float64,
    );

    assert!(single.contains("#define FAUSTFLOAT float"));
    assert!(double.contains("#define FAUSTFLOAT float"));
    assert!(single.contains("output0[i0] = ((FAUSTFLOAT)(((float)(input0[i0]))));"));
    assert!(double.contains("output0[i0] = ((FAUSTFLOAT)(((double)(input0[i0]))));"));
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
fn fastlane_cpp_compiles_noise_smoo_slider_fixture() {
    let cpp = compile_cpp_with_lane(
        "rep_56_noise_smoo_slider.dsp",
        SignalFirLane::TransformFastLane,
    );
    assert!(cpp.contains("class mydsp : public dsp"));
    assert!(cpp.contains("void compute("));
    assert!(cpp.contains("int iRec") && cpp.contains("[2];"));
    assert!(cpp.contains("fSampleRate"));
    assert!(
        !cpp.contains("float fRec"),
        "fast-lane C++ should keep the recursive noise carrier in integer state"
    );
}

#[test]
fn fastlane_cpp_keeps_integer_recursive_min_feedback_in_int_state() {
    let cpp = compile_cpp_source_with_lane(
        "rec_int_min.dsp",
        "process = 1 : (+ : min(3)) ~ _;",
        SignalFirLane::TransformFastLane,
    );
    assert!(cpp.contains("class mydsp : public dsp"));
    assert!(cpp.contains("int iRec") && cpp.contains("[2];"));
    assert!(
        !cpp.contains("float[2] fRec") && !cpp.contains("double[2] fRec"),
        "integer recursive min should keep recursion state in integer arrays"
    );
    assert!(
        cpp.contains("std::min<int>("),
        "integer recursive min should stay an explicit integer min function call"
    );
}

#[test]
fn fastlane_cpp_keeps_integer_recursive_abs_feedback_in_int_state() {
    let cpp = compile_cpp_source_with_lane(
        "rec_int_abs.dsp",
        "process = 1 : (+ : abs) ~ _;",
        SignalFirLane::TransformFastLane,
    );
    assert!(cpp.contains("class mydsp : public dsp"));
    assert!(cpp.contains("int iRec") && cpp.contains("[2];"));
    assert!(
        !cpp.contains("float[2] fRec") && !cpp.contains("double[2] fRec"),
        "integer recursive abs should keep recursion state in integer arrays"
    );
    assert!(
        cpp.contains("std::abs("),
        "integer recursive abs should stay an explicit integer abs function call"
    );
}

#[test]
fn fastlane_interp_compiles_noise_smoo_slider_fixture() {
    let compiler = Compiler::new();
    let path = corpus_path("rep_56_noise_smoo_slider.dsp");
    let fbc = compiler
        .compile_file_default_to_interp_with_lane(
            &path,
            &InterpOptions::default(),
            SignalFirLane::TransformFastLane,
        )
        .unwrap_or_else(|e| {
            panic!("rep_56_noise_smoo_slider.dsp fast-lane interp compilation failed: {e}")
        });
    assert!(
        !fbc.is_empty(),
        "rep_56_noise_smoo_slider.dsp fast-lane interp compilation should produce bytecode"
    );
}

#[test]
fn default_interp_api_uses_fastlane_runtime_lowering() {
    let compiler = Compiler::new();
    let path = corpus_path("rep_56_noise_smoo_slider.dsp");

    let default_fbc = compiler
        .compile_file_default_to_interp(&path, &InterpOptions::default())
        .unwrap_or_else(|e| panic!("default interp compilation failed: {e}"));
    let explicit_fast_fbc = compiler
        .compile_file_default_to_interp_with_lane(
            &path,
            &InterpOptions::default(),
            SignalFirLane::TransformFastLane,
        )
        .unwrap_or_else(|e| panic!("explicit fast-lane interp compilation failed: {e}"));

    assert_eq!(
        default_fbc, explicit_fast_fbc,
        "default interp API should follow the executable fast-lane lowering"
    );
}

#[test]
fn fastlane_c_lifecycle_order_matches_faust_instance_init_flow() {
    let fast = compile_c_with_lane(
        "rep_10_two_in_two_out_ui.dsp",
        SignalFirLane::TransformFastLane,
    );
    let instance_init_sig = "void instanceInitmydsp(mydsp* dsp, int sample_rate) {";
    let instance_init_start = fast
        .find(instance_init_sig)
        .expect("instanceInit signature should be present");
    let instance_init_body = &fast[instance_init_start..];
    let constants_i = instance_init_body
        .find("instanceConstantsmydsp(dsp, sample_rate);")
        .expect("instanceConstants call should be present");
    let reset_i = instance_init_body
        .find("instanceResetUserInterfacemydsp(dsp);")
        .expect("instanceResetUserInterface call should be present");
    let clear_i = instance_init_body
        .find("instanceClearmydsp(dsp);")
        .expect("instanceClear call should be present");
    assert!(
        constants_i < reset_i && reset_i < clear_i,
        "instanceInit should call constants -> resetUI -> clear in order"
    );
}
