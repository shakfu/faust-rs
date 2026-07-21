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

// ---------------------------------------------------------------------------
// corpus-status-query (C3): option parsing, pure classification, and full
// end-to-end checks against the real C++ reference binary.
// ---------------------------------------------------------------------------

#[test]
fn corpus_status_query_options_require_case_or_all() {
    let mut args = std::iter::empty::<String>();
    let error = parse_corpus_status_query_options(&mut args)
        .unwrap_err()
        .to_string();
    assert!(error.contains("--case") && error.contains("--all"));
}

#[test]
fn corpus_status_query_options_reject_case_and_all_together() {
    let mut args = vec![
        "--case".to_string(),
        "tests/corpus/fad_basic.dsp".to_string(),
        "--all".to_string(),
    ]
    .into_iter();
    let error = parse_corpus_status_query_options(&mut args)
        .unwrap_err()
        .to_string();
    assert!(error.contains("mutually exclusive"));
}

#[test]
fn corpus_status_query_options_accept_repeated_case_and_format() {
    let mut args = vec![
        "--case".to_string(),
        "tests/corpus/fad_basic.dsp".to_string(),
        "--case".to_string(),
        "tests/corpus/rep_01_passthrough.dsp".to_string(),
        "--format".to_string(),
        "human".to_string(),
    ]
    .into_iter();
    let options = parse_corpus_status_query_options(&mut args).unwrap();
    assert_eq!(options.cases.len(), 2);
    assert!(!options.all);
    assert_eq!(options.format, QueryFormat::Human);
}

#[test]
fn corpus_status_query_options_reject_unknown_format() {
    let mut args = vec![
        "--all".to_string(),
        "--format".to_string(),
        "yaml".to_string(),
    ]
    .into_iter();
    let error = parse_corpus_status_query_options(&mut args)
        .unwrap_err()
        .to_string();
    assert!(error.contains("yaml"));
}

/// The exact C++ reference wording for an unresolved `fad`/`rad` symbol,
/// confirmed against `porting/phases/phase-4-corpus-status-diff-report-en.md`
/// (e.g. the `fad_basic` row: `tests/corpus/fad_basic.dsp:1 : ERROR :
/// undefined symbol : fad`).
#[test]
fn is_expected_divergence_detects_fad_and_rad_undefined_symbol() {
    assert!(is_expected_divergence(
        "tests/corpus/fad_basic.dsp:1 : ERROR : undefined symbol : fad"
    ));
    assert!(is_expected_divergence(
        "tests/corpus/err_rad_delay_temporal_unsupported.dsp:5 : ERROR : undefined symbol : rad"
    ));
}

#[test]
fn is_expected_divergence_detects_ondemand_undefined_symbol() {
    // Found by measurement: every one of the 21 `real_divergence` cases in
    // the full 218-file corpus run was `undefined symbol : ondemand`, not a
    // genuine regression. See the doc comment on `EXPECTED_DIVERGENCE_SYMBOLS`.
    assert!(is_expected_divergence(
        "interleave.lib:90 : ERROR : undefined symbol : ondemand"
    ));
    assert!(is_expected_divergence(
        "tests/corpus/rep_18_stream_wrappers.dsp:1 : ERROR : undefined symbol : ondemand"
    ));
}

#[test]
fn is_expected_divergence_rejects_unrelated_undefined_symbols() {
    // A hypothetical symbol that merely starts with the same letters must not
    // match: the check stops at the first non-identifier character.
    assert!(!is_expected_divergence(
        "some.dsp:1 : ERROR : undefined symbol : radius"
    ));
    assert!(!is_expected_divergence(
        "some.dsp:1 : ERROR : undefined symbol : fadeout"
    ));
    assert!(!is_expected_divergence(
        "some.dsp:1 : ERROR : undefined symbol : ondemandish"
    ));
    assert!(!is_expected_divergence("some.dsp:1 : ERROR : syntax error"));
}

#[test]
fn classify_divergence_covers_all_four_buckets() {
    assert_eq!(classify_divergence(true, true, "ok"), DivergenceClass::OkOk);
    assert_eq!(
        classify_divergence(false, false, "some other error"),
        DivergenceClass::ErrErr
    );
    assert_eq!(
        classify_divergence(false, true, "undefined symbol : fad"),
        DivergenceClass::ExpectedDivergence
    );
    assert_eq!(
        classify_divergence(false, true, "undefined symbol : somethingelse"),
        DivergenceClass::RealDivergence
    );
    // C++ ok, Rust fails: always a real (Rust) regression, never "expected",
    // even if the C++ reason string happens to mention fad/rad incidentally.
    assert_eq!(
        classify_divergence(true, false, "undefined symbol : fad"),
        DivergenceClass::RealDivergence
    );
}

/// Best-effort guard for the end-to-end tests below: they need a working C++
/// reference binary (either `FAUST_CPP_BIN` or the checked-out build tree
/// `resolve_cpp_faust_bin` falls back to). If neither resolves to something
/// runnable, the tests are skipped rather than failed, mirroring how the
/// existing `xtask` report generators depend on an external checkout without
/// a bundled fixture binary.
fn cpp_reference_binary_available() -> bool {
    let (bin, is_fallback) = resolve_cpp_faust_bin();
    if is_fallback {
        return false;
    }
    bin.exists()
}

#[test]
fn corpus_status_query_json_response_carries_staleness_metadata() {
    if !cpp_reference_binary_available() {
        eprintln!("skipping: no C++ reference binary available (set FAUST_CPP_BIN)");
        return;
    }
    let mut args = vec![
        "--case".to_string(),
        "tests/corpus/fad_basic.dsp".to_string(),
    ]
    .into_iter();
    let options = parse_corpus_status_query_options(&mut args).unwrap();
    let response = run_corpus_status_query(&options).unwrap();

    // Round-trip through JSON: the schema must actually parse, not merely
    // serialize.
    let json = serde_json::to_string(&response).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["schema_version"], CORPUS_STATUS_QUERY_SCHEMA_VERSION);
    assert!(parsed["generated_at_unix"].as_u64().unwrap() > 0);
    assert!(parsed["corpus_file_count_seen"].as_u64().unwrap() > 0);
    assert!(!parsed["cpp_binary"]["path"].as_str().unwrap().is_empty());
    assert!(
        !parsed["cpp_binary"]["resolved_via"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    // The corpus is much larger than the one requested case: proves the
    // staleness signal (corpus size actually seen) is independent of, and
    // does not collapse into, the query scope.
    assert!(response.corpus_file_count_seen > response.cases.len());
}

#[test]
fn corpus_status_query_classifies_fad_basic_as_expected_divergence() {
    if !cpp_reference_binary_available() {
        eprintln!("skipping: no C++ reference binary available (set FAUST_CPP_BIN)");
        return;
    }
    let mut args = vec![
        "--case".to_string(),
        "tests/corpus/fad_basic.dsp".to_string(),
    ]
    .into_iter();
    let options = parse_corpus_status_query_options(&mut args).unwrap();
    let response = run_corpus_status_query(&options).unwrap();

    assert_eq!(response.cases.len(), 1);
    let case = &response.cases[0];
    assert_eq!(case.case, "fad_basic");
    assert_eq!(case.cpp_status, "ERR");
    assert_eq!(case.rust_status, "OK");
    assert_eq!(case.classification, DivergenceClass::ExpectedDivergence);
    assert_eq!(response.counts.expected_divergence, 1);
    assert_eq!(response.counts.real_divergence, 0);
}

#[test]
fn corpus_status_query_case_list_compiles_only_requested_cases() {
    if !cpp_reference_binary_available() {
        eprintln!("skipping: no C++ reference binary available (set FAUST_CPP_BIN)");
        return;
    }
    let requested = [
        "tests/corpus/fad_basic.dsp",
        "tests/corpus/rep_01_passthrough.dsp",
        "tests/corpus/rep_05_one_pole_lowpass.dsp",
    ];
    let mut args = Vec::new();
    for case in requested {
        args.push("--case".to_string());
        args.push(case.to_string());
    }
    let options = parse_corpus_status_query_options(&mut args.into_iter()).unwrap();
    let response = run_corpus_status_query(&options).unwrap();

    assert_eq!(response.query_scope, QueryScope::Cases);
    assert_eq!(response.requested_cases.len(), requested.len());
    assert_eq!(response.cases.len(), requested.len());
    assert_eq!(response.counts.total, requested.len());
    // The corpus holds far more than 3 files; a query for 3 cases must not
    // silently expand to the whole corpus.
    assert!(response.corpus_file_count_seen > requested.len());
    let names: Vec<&str> = response.cases.iter().map(|c| c.case.as_str()).collect();
    assert_eq!(
        names,
        vec!["fad_basic", "rep_01_passthrough", "rep_05_one_pole_lowpass"]
    );
}

#[test]
fn corpus_status_query_counts_are_internally_consistent() {
    if !cpp_reference_binary_available() {
        eprintln!("skipping: no C++ reference binary available (set FAUST_CPP_BIN)");
        return;
    }
    let requested = [
        "tests/corpus/fad_basic.dsp",
        "tests/corpus/rep_01_passthrough.dsp",
        "tests/corpus/rep_05_one_pole_lowpass.dsp",
        "tests/corpus/err_rad_delay_temporal_unsupported.dsp",
    ];
    let mut args = Vec::new();
    for case in requested {
        args.push("--case".to_string());
        args.push(case.to_string());
    }
    let options = parse_corpus_status_query_options(&mut args.into_iter()).unwrap();
    let response = run_corpus_status_query(&options).unwrap();

    let c = &response.counts;
    assert_eq!(
        c.total,
        c.ok_ok + c.err_err + c.expected_divergence + c.real_divergence
    );
    assert_eq!(c.total, response.cases.len());

    // The staleness field must reflect what this run actually observed, not
    // a cached or hardcoded figure: recompute the corpus size independently
    // (a fresh directory listing) and require the response to agree.
    let actual_corpus_file_count = corpus_files().unwrap().len();
    assert_eq!(response.corpus_file_count_seen, actual_corpus_file_count);
}
