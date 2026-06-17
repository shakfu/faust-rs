//! Unit tests for extracted `xtask` workflow helpers.
//!
//! These tests stay in a separate module so `main.rs` can remain a small command
//! facade while still exercising option parsing, trace serialization, and ABI
//! export validation helpers.

use super::*;

#[test]
fn trace_scenario_parse_accepts_known_names() {
    assert_eq!(TraceScenario::parse("zeros").unwrap(), TraceScenario::Zeros);
    assert_eq!(
        TraceScenario::parse("impulse").unwrap(),
        TraceScenario::Impulse
    );
    assert_eq!(TraceScenario::parse("ramp").unwrap(), TraceScenario::Ramp);
    assert_eq!(TraceScenario::parse("sine").unwrap(), TraceScenario::Sine);
}

#[test]
fn trace_lane_parse_accepts_fast_aliases() {
    assert_eq!(TraceLane::parse("fast").unwrap(), TraceLane::Fast);
    assert_eq!(TraceLane::parse("fast-lane").unwrap(), TraceLane::Fast);
    assert_eq!(TraceLane::parse("transform").unwrap(), TraceLane::Fast);
}

#[test]
fn parse_interp_trace_dump_defaults_and_required_case() {
    let mut args = vec![
        "--case".to_string(),
        "tests/corpus/rep_31_extended_primitives.dsp".to_string(),
    ]
    .into_iter();
    let opts = parse_interp_trace_dump_options(&mut args).unwrap();
    assert_eq!(opts.scenario, TraceScenario::Zeros);
    assert_eq!(opts.lane, TraceLane::Fast);
    assert_eq!(opts.sample_rate, 48_000);
    assert_eq!(opts.block_size, 64);
    assert_eq!(opts.num_blocks, 4);
    assert!(!opts.strict_fir_types);
}

#[test]
fn parse_interp_trace_dump_accepts_strict_fir_types_flag() {
    let mut args = vec![
        "--case".to_string(),
        "tests/runtime_corpus/trace_01_passthrough.dsp".to_string(),
        "--strict-fir-types".to_string(),
    ]
    .into_iter();
    let opts = parse_interp_trace_dump_options(&mut args).unwrap();
    assert!(opts.strict_fir_types);
}

#[test]
fn parse_interp_trace_batch_defaults() {
    let mut args = std::iter::empty::<String>();
    let opts = parse_interp_trace_batch_options(&mut args).unwrap();
    assert_eq!(opts.case, None);
    assert_eq!(opts.lane, TraceLane::Fast);
    assert_eq!(opts.sample_rate, 48_000);
    assert_eq!(opts.block_size, 64);
    assert_eq!(opts.num_blocks, 4);
    assert!(!opts.strict_fir_types);
}

#[test]
fn parse_interp_trace_batch_accepts_strict_fir_types_flag() {
    let mut args = vec!["--strict-fir-types".to_string()].into_iter();
    let opts = parse_interp_trace_batch_options(&mut args).unwrap();
    assert!(opts.strict_fir_types);
}

#[test]
fn fir_type_diagnostic_code_filter_matches_expected_groups() {
    assert!(is_fir_type_diagnostic_code("FIR-B03"));
    assert!(is_fir_type_diagnostic_code("FIR-U02"));
    assert!(is_fir_type_diagnostic_code("FIR-C01"));
    assert!(is_fir_type_diagnostic_code("FIR-FC03"));
    assert!(is_fir_type_diagnostic_code("FIR-T02"));
    assert!(is_fir_type_diagnostic_code("FIR-MA04"));
    assert!(is_fir_type_diagnostic_code("FIR-L03"));
    assert!(is_fir_type_diagnostic_code("FIR-SW01"));
    assert!(!is_fir_type_diagnostic_code("FIR-M07"));
    assert!(!is_fir_type_diagnostic_code("FIR-SC01"));
}

#[test]
fn runtime_trace_scenario_mapping_for_typed_primitives() {
    let scenarios = trace_scenarios_for_runtime_case(Path::new(
        "tests/runtime_corpus/trace_31_extended_primitives_typed.dsp",
    ))
    .unwrap();
    assert_eq!(scenarios, vec![TraceScenario::Zeros]);
}

#[test]
fn runtime_trace_scenario_mapping_for_int_plus_one() {
    let scenarios = trace_scenarios_for_runtime_case(Path::new(
        "tests/runtime_corpus/trace_40_int_plus_one.dsp",
    ))
    .unwrap();
    assert_eq!(scenarios, vec![TraceScenario::Ramp]);
}

#[test]
fn runtime_trace_snapshot_path_uses_case_and_scenario() {
    let path = runtime_trace_snapshot_path("trace_01_passthrough", TraceScenario::Impulse);
    let expected = runtime_trace_snapshot_root()
        .join("trace_01_passthrough")
        .join("impulse.json");
    assert_eq!(path, expected);
}

#[test]
fn generate_impulse_inputs_sets_first_sample_only() {
    let inputs = generate_trace_inputs(TraceScenario::Impulse, 2, 5, 48_000);
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0], vec![1.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(inputs[1], vec![1.0, 0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn render_runtime_trace_json_contains_expected_keys() {
    let trace = RuntimeTrace {
        dsp_path: "tests/corpus/example.dsp".into(),
        lane: "fast-lane".into(),
        scenario: "zeros".into(),
        sample_rate: 48_000,
        block_size: 64,
        num_blocks: 1,
        num_inputs: 1,
        num_outputs: 1,
        outputs: vec![vec![0.0, 1.0]],
    };
    let json = render_runtime_trace_json(&trace);
    assert!(json.contains("\"backend\": \"interp\""));
    assert!(json.contains("\"signal_fir_lane\": \"fast-lane\""));
    assert!(json.contains("\"scenario\""));
    assert!(json.contains("\"outputs\""));
}

#[test]
fn parse_runtime_trace_json_roundtrip() {
    let trace = RuntimeTrace {
        dsp_path: "tests/runtime_corpus/trace_01_passthrough.dsp".into(),
        lane: "fast-lane".into(),
        scenario: "impulse".into(),
        sample_rate: 48_000,
        block_size: 64,
        num_blocks: 1,
        num_inputs: 1,
        num_outputs: 1,
        outputs: vec![vec![1.0, 0.0]],
    };
    let parsed = parse_runtime_trace_json(&render_runtime_trace_json(&trace)).unwrap();
    assert_eq!(parsed, trace);
}

#[test]
fn compare_runtime_traces_tolerates_small_float_delta() {
    let a = RuntimeTrace {
        dsp_path: "x".into(),
        lane: "normalized".into(),
        scenario: "zeros".into(),
        sample_rate: 48_000,
        block_size: 64,
        num_blocks: 1,
        num_inputs: 0,
        num_outputs: 1,
        outputs: vec![vec![1.0]],
    };
    let mut b = a.clone();
    b.outputs[0][0] = 1.0 + 1.0e-7;
    assert!(compare_runtime_traces(&a, &b, TraceCompareTolerances::default()).is_ok());
}

#[test]
fn compare_runtime_traces_reports_large_float_delta() {
    let a = RuntimeTrace {
        dsp_path: "x".into(),
        lane: "normalized".into(),
        scenario: "zeros".into(),
        sample_rate: 48_000,
        block_size: 64,
        num_blocks: 1,
        num_inputs: 0,
        num_outputs: 1,
        outputs: vec![vec![1.0]],
    };
    let mut b = a.clone();
    b.outputs[0][0] = 1.1;
    let mismatch = compare_runtime_traces(&a, &b, TraceCompareTolerances::default()).unwrap_err();
    assert_eq!(mismatch.field, "outputs");
    assert_eq!(mismatch.channel, Some(0));
    assert_eq!(mismatch.sample, Some(0));
}

#[test]
fn interp_trace_opt_level_diff_matches_on_passthrough_case() {
    let case = workspace_root().join("tests/runtime_corpus/trace_01_passthrough.dsp");
    interp_trace_diff_opt_levels_cases(&[case], false).unwrap();
}

#[test]
fn parse_faustwasm_compiler_module_options_defaults_to_release() {
    let options = parse_faustwasm_compiler_module_options(std::iter::empty::<String>()).unwrap();
    assert!(options.release);
}

#[test]
fn parse_faustwasm_compiler_module_options_accepts_debug_flag() {
    let options =
        parse_faustwasm_compiler_module_options(vec!["--debug".to_owned()].into_iter()).unwrap();
    assert!(!options.release);
}

#[test]
fn verify_wasm_ffi_exports_accepts_expected_surface() {
    let bytes = wat::parse_str(
        r#"
            (module
              (memory (export "memory") 1)
              (func (export "faust_wasm_alloc"))
              (func (export "faust_wasm_dealloc"))
              (func (export "faust_wasm_version_ptr"))
              (func (export "faust_wasm_version_len"))
              (func (export "faust_wasm_compile_dsp"))
              (func (export "faust_wasm_result_is_ok"))
              (func (export "faust_wasm_result_wasm_ptr"))
              (func (export "faust_wasm_result_wasm_len"))
              (func (export "faust_wasm_result_json_ptr"))
              (func (export "faust_wasm_result_json_len"))
              (func (export "faust_wasm_result_compile_options_ptr"))
              (func (export "faust_wasm_result_compile_options_len"))
              (func (export "faust_wasm_result_error_ptr"))
              (func (export "faust_wasm_result_error_len"))
              (func (export "faust_wasm_result_free"))
              (func (export "faust_wasm_get_info"))
              (func (export "faust_wasm_expand_dsp"))
              (func (export "faust_wasm_generate_aux_files"))
              (func (export "faust_wasm_generate_aux_files_json"))
              (func (export "faust_wasm_text_result_is_ok"))
              (func (export "faust_wasm_text_result_ptr"))
              (func (export "faust_wasm_text_result_len"))
              (func (export "faust_wasm_text_result_free"))
            )
            "#,
    )
    .unwrap();

    verify_wasm_ffi_exports(&bytes).unwrap();
}

#[test]
fn verify_wasm_ffi_exports_rejects_missing_exports() {
    let bytes = wat::parse_str(
        r#"
            (module
              (memory (export "memory") 1)
              (func (export "faust_wasm_alloc"))
            )
            "#,
    )
    .unwrap();

    let error = verify_wasm_ffi_exports(&bytes).unwrap_err().to_string();
    assert!(error.contains("faust_wasm_compile_dsp"));
    assert!(error.contains("faust_wasm_text_result_free"));
}
