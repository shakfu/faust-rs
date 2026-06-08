//! Backend alignment smoke workflow orchestration.
//!
//! This module ties together golden snapshot checks, runtime trace checks,
//! Cranelift subset checks, and FIR dump scans into one CI-friendly smoke
//! command. Individual validation primitives live in their focused modules.

use super::*;

// ---------------------------------------------------------------------------
// Backend alignment orchestration
// ---------------------------------------------------------------------------

/// Default runtime cases for the CI-friendly backend alignment smoke workflow.
pub(crate) const BACKEND_ALIGN_SMOKE_DEFAULT_CASES: &[&str] = &[
    "tests/runtime_corpus/trace_01_passthrough.dsp",
    "tests/runtime_corpus/trace_07_nonlinear_clip.dsp",
    "tests/runtime_corpus/trace_38_sine_phasor.dsp",
];

/// Default FIR dump cases for the CI-friendly backend alignment smoke workflow.
pub(crate) const BACKEND_ALIGN_SMOKE_FIR_CASES: &[&str] = &[
    "tests/corpus/rep_01_passthrough.dsp",
    "tests/corpus/rep_07_nonlinear_clip.dsp",
    "tests/corpus/rep_38_sine_phasor.dsp",
];

/// Returns the stable case identifier derived from a corpus path.
pub(crate) fn case_name(path: &Path) -> Result<String, io::Error> {
    path.file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid corpus filename"))
}

#[derive(Debug, Default)]
/// Parsed options for the CI-friendly backend alignment smoke workflow.
pub(crate) struct BackendAlignSmokeOptions {
    /// Explicit runtime corpus cases selected with repeated `--case`.
    cases: Vec<PathBuf>,
    /// Whether FIR type diagnostics should make runtime traces fail early.
    strict_fir_types: bool,
    /// Skip the golden snapshot check phase.
    skip_golden: bool,
    /// Skip the structural FIR dump scan phase.
    skip_fir_dump_scan: bool,
}

/// Runs the reduced backend-alignment smoke workflow used in CI.
pub(crate) fn backend_align_smoke(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_backend_align_smoke_options(&mut args)?;
    println!("backend-align-smoke: start");

    if !options.skip_golden {
        println!("backend-align-smoke: golden-check");
        golden_check(None)?;
    } else {
        println!("backend-align-smoke: skip golden-check");
    }

    let cases = backend_align_smoke_cases(&options)?;
    if cases.is_empty() {
        return Err("backend-align-smoke: no runtime cases selected".into());
    }

    println!("backend-align-smoke: cranelift-subset-strict-check");
    cranelift_subset_strict_check_cases(&cases)?;
    println!("backend-align-smoke: cranelift-ffi-runtime-diff-smoke");
    run_cranelift_ffi_runtime_diff_smoke()?;
    println!("backend-align-smoke: interp-opt-level-diff");
    interp_trace_diff_opt_levels_cases(&cases, options.strict_fir_types)?;

    for case in &cases {
        let mut trace_check_args = vec![
            "--case".to_owned(),
            case.display().to_string(),
            "--lane".to_owned(),
            "fast".to_owned(),
        ];
        if options.strict_fir_types {
            trace_check_args.push("--strict-fir-types".to_owned());
        }
        println!("backend-align-smoke: interp-trace-check {}", case.display());
        interp_trace_check(trace_check_args.into_iter())?;
    }

    if !options.skip_fir_dump_scan {
        let mut scan_args: Vec<String> = Vec::new();
        for case in backend_align_smoke_fir_cases()? {
            scan_args.push("--case".to_owned());
            scan_args.push(case.display().to_string());
        }
        scan_args.push("--lane".to_owned());
        scan_args.push("fast".to_owned());
        println!("backend-align-smoke: fir-dump-scan (fast lane corpus subset)");
        fir_dump_scan(scan_args.into_iter())?;
    } else {
        println!("backend-align-smoke: skip fir-dump-scan");
    }

    println!(
        "backend-align-smoke: OK (runtime_cases={}, strict_fir_types={}, golden={}, cranelift_strict_subset=true, interp_opt_levels=true, fir_dump_scan={})",
        cases.len(),
        options.strict_fir_types,
        !options.skip_golden,
        !options.skip_fir_dump_scan
    );
    Ok(())
}

/// Parses CLI flags for `backend-align-smoke`.
pub(crate) fn parse_backend_align_smoke_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<BackendAlignSmokeOptions, Box<dyn std::error::Error>> {
    let mut options = BackendAlignSmokeOptions::default();
    let iter = args.by_ref();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = iter.next() else {
                    return Err("--case requires a path".into());
                };
                options.cases.push(PathBuf::from(path));
            }
            "--strict-fir-types" => options.strict_fir_types = true,
            "--skip-golden" => options.skip_golden = true,
            "--skip-fir-dump-scan" => options.skip_fir_dump_scan = true,
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- backend-align-smoke [--case <tests/runtime_corpus/foo.dsp> ...] [--strict-fir-types] [--skip-golden] [--skip-fir-dump-scan]".into());
            }
            other => return Err(format!("unknown backend-align-smoke option: {other}").into()),
        }
    }
    Ok(options)
}

/// Resolves the runtime corpus subset used by `backend-align-smoke`.
///
/// When explicit `--case` flags are present they win; otherwise the baked-in
/// smoke subset is materialized under the workspace root and existence-checked.
pub(crate) fn backend_align_smoke_cases(
    options: &BackendAlignSmokeOptions,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    if !options.cases.is_empty() {
        return Ok(options.cases.clone());
    }
    let root = workspace_root();
    let mut cases = Vec::new();
    for rel in BACKEND_ALIGN_SMOKE_DEFAULT_CASES {
        let path = root.join(rel);
        if !path.exists() {
            return Err(format!(
                "backend-align-smoke default case missing: {}",
                path.display()
            )
            .into());
        }
        cases.push(path);
    }
    Ok(cases)
}

/// Resolves the FIR corpus subset scanned by `backend-align-smoke`.
///
/// This list stays separate from the runtime-trace subset because it targets
/// `dump_fir` structural coverage rather than runtime execution coverage.
pub(crate) fn backend_align_smoke_fir_cases() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let root = workspace_root();
    let mut cases = Vec::new();
    for rel in BACKEND_ALIGN_SMOKE_FIR_CASES {
        let path = root.join(rel);
        if !path.exists() {
            return Err(format!(
                "backend-align-smoke default FIR case missing: {}",
                path.display()
            )
            .into());
        }
        cases.push(path);
    }
    Ok(cases)
}

/// Verifies that each case lowers through the strict Cranelift subset path.
///
/// This intentionally enables `fail_on_subset_gap` so the nightly/smoke flows
/// catch matcher/lowerer drift instead of silently falling back.
pub(crate) fn cranelift_subset_strict_check_cases(
    cases: &[PathBuf],
) -> Result<(), Box<dyn std::error::Error>> {
    let compiler = compiler::Compiler::new();
    for case in cases {
        let fir = compiler
            .compile_file_default_to_fir_with_lane(case, compiler::SignalFirLane::TransformFastLane)
            .map_err(|e| {
                format!(
                    "Cranelift strict subset FIR compile failed for {}: {e}",
                    case.display()
                )
            })?;
        let options = codegen::backends::cranelift::CraneliftOptions {
            fail_on_subset_gap: true,
            ..codegen::backends::cranelift::CraneliftOptions::default()
        };
        codegen::backends::cranelift::generate_cranelift_module(&fir.store, fir.module, &options)
            .map_err(|e| {
            format!(
                "Cranelift strict subset check failed for {}: {e}",
                case.display()
            )
        })?;
    }
    println!(
        "cranelift-subset-strict-check: {} case(s) compiled without fallback",
        cases.len()
    );
    Ok(())
}

/// Runs the standalone `cranelift-ffi` smoke tests used by backend alignment.
pub(crate) fn run_cranelift_ffi_runtime_diff_smoke() -> Result<(), Box<dyn std::error::Error>> {
    const TESTS: [&str; 2] = [
        "cranelift_interp_runtime_diff_smoke_corpus",
        "cranelift_ui_meta_callback_smoke_path",
    ];
    for test_name in TESTS {
        let status = Command::new("cargo")
            .arg("test")
            .arg("-p")
            .arg("cranelift-ffi")
            .arg(test_name)
            .arg("--")
            .arg("--nocapture")
            .status()?;
        if !status.success() {
            return Err(format!("cranelift-ffi smoke test failed: {test_name}").into());
        }
    }
    Ok(())
}

/// Parsed options for the broader nightly backend-alignment workflow.
#[derive(Debug, Default)]
pub(crate) struct BackendAlignNightlyOptions {
    /// Whether FIR type diagnostics should make runtime traces fail early.
    strict_fir_types: bool,
    /// Skip the golden snapshot check phase.
    skip_golden: bool,
    /// Skip the structural FIR dump scan phase.
    skip_fir_dump_scan: bool,
}

/// Runs the broader nightly backend-alignment workflow.
pub(crate) fn backend_align_nightly(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_backend_align_nightly_options(&mut args)?;
    println!("backend-align-nightly: start");

    if !options.skip_golden {
        println!("backend-align-nightly: golden-check");
        golden_check(None)?;
    } else {
        println!("backend-align-nightly: skip golden-check");
    }

    let nightly_cases = runtime_corpus_files()?;
    println!("backend-align-nightly: cranelift-subset-strict-check (all runtime cases)");
    cranelift_subset_strict_check_cases(&nightly_cases)?;
    println!("backend-align-nightly: cranelift-ffi-runtime-diff-smoke");
    run_cranelift_ffi_runtime_diff_smoke()?;

    let mut trace_check_args = vec!["--lane".to_owned(), "fast".to_owned()];
    if options.strict_fir_types {
        trace_check_args.push("--strict-fir-types".to_owned());
    }
    println!("backend-align-nightly: interp-trace-check (all runtime cases, fast lane)");
    interp_trace_check(trace_check_args.into_iter())?;

    if !options.skip_fir_dump_scan {
        println!("backend-align-nightly: fir-dump-scan (all corpus cases, fast lane)");
        fir_dump_scan(["--lane".to_owned(), "fast".to_owned()].into_iter())?;
    } else {
        println!("backend-align-nightly: skip fir-dump-scan");
    }

    println!(
        "backend-align-nightly: OK (strict_fir_types={}, golden={}, cranelift_strict_subset=true, fir_dump_scan={})",
        options.strict_fir_types, !options.skip_golden, !options.skip_fir_dump_scan
    );
    Ok(())
}

/// Parses CLI flags for `backend-align-nightly`.
pub(crate) fn parse_backend_align_nightly_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<BackendAlignNightlyOptions, Box<dyn std::error::Error>> {
    let mut options = BackendAlignNightlyOptions::default();
    for arg in args.by_ref() {
        match arg.as_str() {
            "--strict-fir-types" => options.strict_fir_types = true,
            "--skip-golden" => options.skip_golden = true,
            "--skip-fir-dump-scan" => options.skip_fir_dump_scan = true,
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- backend-align-nightly [--strict-fir-types] [--skip-golden] [--skip-fir-dump-scan]".into());
            }
            other => return Err(format!("unknown backend-align-nightly option: {other}").into()),
        }
    }
    Ok(options)
}
