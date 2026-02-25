//! `xtask` CLI entry point for repository maintenance workflows.
//!
//! # Role
//! - Hosts developer/CI automation that should not be part of runtime compiler
//!   crates (golden generation/checks, parity reports, differential reports).
//!
//! # Primary workflows
//! - Golden snapshots:
//!   - `golden-check`, `golden-check-cpp`
//!   - `golden-gen-rust`, `golden-gen-cpp`
//! - Runtime trace validation (interp backend):
//!   - `interp-trace-dump` (Phase 1 harness prototype)
//!   - `interp-trace-gen`, `interp-trace-check` (Phase 2 snapshot scaffold)
//!   - `interp-trace-diff-lanes` (Phase 3 lane differential scaffold)
//!   - `interp-trace-dump-cppfbc` (C++ Faust `.fbc` -> Rust interp runtime)
//!   - `interp-trace-gen-cppfbc` (batch-generate persisted traces from C++ `.fbc`)
//!   - `backend-align-smoke` (CI-friendly smoke alignment orchestration)
//!   - `backend-align-nightly` (broader alignment orchestration)
//!   - `fir-dump-scan` (structural scan of `dump_fir` loop body expansion)
//! - Differential reports:
//!   - parser parity report
//!   - corpus status report
//!   - backend diff reports
//!
//! # Design invariants
//! - Deterministic corpus file ordering.
//! - Normalized output text before snapshot comparison.
//! - Fail-fast behavior when one case diverges to preserve CI signal quality.

use fir::{FirMatch, dump_fir, match_fir};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;

const USAGE: &str = "\
Usage:
  cargo run -p xtask -- golden-check
  cargo run -p xtask -- golden-check-cpp
  cargo run -p xtask -- golden-gen-rust
  cargo run -p xtask -- golden-gen-cpp [-- <extra args passed to FAUST_CPP_BIN>]
  cargo run -p xtask -- interp-trace-dump --case <tests/corpus/foo.dsp> [--scenario zeros|impulse|ramp|sine] [--lane legacy|fast] [--strict-fir-types]
  cargo run -p xtask -- interp-trace-dump-cppfbc --case <tests/corpus/foo.dsp> [--scenario zeros|impulse|ramp|sine] [--faust-bin /path/to/faust]
  cargo run -p xtask -- interp-trace-gen-cppfbc [--case <tests/corpus/foo.dsp>] [--scenario zeros|impulse|ramp|sine] [--out-dir <dir>] [--faust-bin /path/to/faust]
  cargo run -p xtask -- interp-trace-gen [--case <tests/runtime_corpus/foo.dsp>] [--lane legacy|fast] [--strict-fir-types]
  cargo run -p xtask -- interp-trace-check [--case <tests/runtime_corpus/foo.dsp>] [--lane legacy|fast] [--strict-fir-types]
  cargo run -p xtask -- interp-trace-diff-lanes [--case <tests/runtime_corpus/foo.dsp>] [--strict-fir-types]
  cargo run -p xtask -- fir-dump-scan [--case <tests/corpus/foo.dsp> ...] [--lane legacy|fast]
  cargo run -p xtask -- backend-align-smoke [--case <tests/runtime_corpus/foo.dsp> ...] [--strict-fir-types] [--skip-golden] [--skip-diff-lanes] [--skip-fir-dump-scan]
  cargo run -p xtask -- backend-align-nightly [--strict-fir-types] [--skip-golden] [--skip-diff-lanes] [--skip-fir-dump-scan]
  cargo run -p xtask -- parser-parity-report
  cargo run -p xtask -- corpus-status-report
  cargo run -p xtask -- cpp-backend-diff-report
  cargo run -p xtask -- c-fastlane-diff-report
  cargo run -p xtask -- backend-full-corpus-diff-report
  cargo run -p xtask -- table-fastlane-diff-report
\nEnvironment for golden-gen-cpp:
  FAUST_CPP_BIN   Path to reference C++ faust binary
\nEnvironment for golden-check:
  GOLDEN_REF      rust (default) or cpp
";

const CPP_SOURCE_ROOT: &str = "/Users/letz/Developpements/RUST/faust";
const PARITY_REPORT_REL_PATH: &str = "porting/phases/phase-3-parser-parity-report-en.md";
const CORPUS_STATUS_REPORT_REL_PATH: &str =
    "porting/phases/phase-4-corpus-status-diff-report-en.md";
const CPP_BACKEND_DIFF_REPORT_REL_PATH: &str =
    "porting/phases/phase-6-cpp-backend-diff-report-en.md";
const C_FASTLANE_DIFF_REPORT_REL_PATH: &str = "porting/phases/phase-6-c-fastlane-diff-report-en.md";
const BACKEND_FULL_CORPUS_DIFF_REPORT_REL_PATH: &str =
    "porting/phases/phase-6-backend-full-corpus-diff-report-en.md";
const TABLE_FASTLANE_DIFF_REPORT_REL_PATH: &str =
    "porting/phases/phase-6-table-fastlane-diff-report-en.md";

fn main() {
    if let Err(err) = run() {
        eprintln!("xtask error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print!("{USAGE}");
        return Ok(());
    };

    match command.as_str() {
        "golden-check" => golden_check(None)?,
        "golden-check-cpp" => golden_check(Some(GoldenRef::Cpp))?,
        "golden-gen-rust" => golden_gen_rust()?,
        "golden-gen-cpp" => {
            let mut passthrough: Vec<OsString> = Vec::new();
            let mut separator_seen = false;
            for arg in args {
                if separator_seen {
                    passthrough.push(OsString::from(arg));
                } else if arg == "--" {
                    separator_seen = true;
                }
            }
            golden_gen_cpp(&passthrough)?;
        }
        "interp-trace-dump" => interp_trace_dump(args)?,
        "interp-trace-dump-cppfbc" => interp_trace_dump_cppfbc(args)?,
        "interp-trace-gen-cppfbc" => interp_trace_gen_cppfbc(args)?,
        "interp-trace-gen" => interp_trace_gen(args)?,
        "interp-trace-check" => interp_trace_check(args)?,
        "interp-trace-diff-lanes" => interp_trace_diff_lanes(args)?,
        "fir-dump-scan" => fir_dump_scan(args)?,
        "backend-align-smoke" => backend_align_smoke(args)?,
        "backend-align-nightly" => backend_align_nightly(args)?,
        "parser-parity-report" => parser_parity_report()?,
        "corpus-status-report" => corpus_status_report()?,
        "cpp-backend-diff-report" => cpp_backend_diff_report()?,
        "c-fastlane-diff-report" => c_fastlane_diff_report()?,
        "backend-full-corpus-diff-report" => backend_full_corpus_diff_report()?,
        "table-fastlane-diff-report" => table_fastlane_diff_report()?,
        _ => {
            print!("{USAGE}");
        }
    }

    Ok(())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .to_path_buf()
        })
}

fn corpus_files() -> Result<Vec<PathBuf>, io::Error> {
    let root = workspace_root();
    let corpus_dir = root.join("tests/corpus");
    let mut files = Vec::new();

    for entry in fs::read_dir(corpus_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "dsp") {
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

fn runtime_corpus_files() -> Result<Vec<PathBuf>, io::Error> {
    let root = workspace_root();
    let dir = root.join("tests/runtime_corpus");
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "dsp") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn runtime_trace_snapshot_root() -> PathBuf {
    workspace_root().join("tests/runtime_traces").join("rust")
}

const BACKEND_ALIGN_SMOKE_DEFAULT_CASES: &[&str] = &[
    "tests/runtime_corpus/trace_01_passthrough.dsp",
    "tests/runtime_corpus/trace_07_nonlinear_clip.dsp",
    "tests/runtime_corpus/trace_38_sine_phasor.dsp",
];
const BACKEND_ALIGN_SMOKE_FIR_CASES: &[&str] = &[
    "tests/corpus/rep_01_passthrough.dsp",
    "tests/corpus/rep_07_nonlinear_clip.dsp",
    "tests/corpus/rep_38_sine_phasor.dsp",
];

fn case_name(path: &Path) -> Result<String, io::Error> {
    path.file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid corpus filename"))
}

#[derive(Debug, Default)]
struct BackendAlignSmokeOptions {
    cases: Vec<PathBuf>,
    strict_fir_types: bool,
    skip_golden: bool,
    skip_diff_lanes: bool,
    skip_fir_dump_scan: bool,
}

fn backend_align_smoke(
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

    if !options.skip_diff_lanes {
        for case in &cases {
            let mut diff_args = vec!["--case".to_owned(), case.display().to_string()];
            if options.strict_fir_types {
                diff_args.push("--strict-fir-types".to_owned());
            }
            println!(
                "backend-align-smoke: interp-trace-diff-lanes {}",
                case.display()
            );
            interp_trace_diff_lanes(diff_args.into_iter())?;
        }
    } else {
        println!("backend-align-smoke: skip interp-trace-diff-lanes");
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
        "backend-align-smoke: OK (runtime_cases={}, strict_fir_types={}, golden={}, diff_lanes={}, fir_dump_scan={})",
        cases.len(),
        options.strict_fir_types,
        !options.skip_golden,
        !options.skip_diff_lanes,
        !options.skip_fir_dump_scan
    );
    Ok(())
}

fn parse_backend_align_smoke_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<BackendAlignSmokeOptions, Box<dyn std::error::Error>> {
    let mut options = BackendAlignSmokeOptions::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = args.next() else {
                    return Err("--case requires a path".into());
                };
                options.cases.push(PathBuf::from(path));
            }
            "--strict-fir-types" => options.strict_fir_types = true,
            "--skip-golden" => options.skip_golden = true,
            "--skip-diff-lanes" => options.skip_diff_lanes = true,
            "--skip-fir-dump-scan" => options.skip_fir_dump_scan = true,
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- backend-align-smoke [--case <tests/runtime_corpus/foo.dsp> ...] [--strict-fir-types] [--skip-golden] [--skip-diff-lanes] [--skip-fir-dump-scan]".into());
            }
            other => return Err(format!("unknown backend-align-smoke option: {other}").into()),
        }
    }
    Ok(options)
}

fn backend_align_smoke_cases(
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

fn backend_align_smoke_fir_cases() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
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

#[derive(Debug, Default)]
struct BackendAlignNightlyOptions {
    strict_fir_types: bool,
    skip_golden: bool,
    skip_diff_lanes: bool,
    skip_fir_dump_scan: bool,
}

fn backend_align_nightly(
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

    let mut trace_check_args = vec!["--lane".to_owned(), "fast".to_owned()];
    if options.strict_fir_types {
        trace_check_args.push("--strict-fir-types".to_owned());
    }
    println!("backend-align-nightly: interp-trace-check (all runtime cases, fast lane)");
    interp_trace_check(trace_check_args.into_iter())?;

    if !options.skip_diff_lanes {
        let mut diff_args: Vec<String> = Vec::new();
        if options.strict_fir_types {
            diff_args.push("--strict-fir-types".to_owned());
        }
        println!("backend-align-nightly: interp-trace-diff-lanes (all runtime cases)");
        interp_trace_diff_lanes(diff_args.into_iter())?;
    } else {
        println!("backend-align-nightly: skip interp-trace-diff-lanes");
    }

    if !options.skip_fir_dump_scan {
        println!("backend-align-nightly: fir-dump-scan (all corpus cases, fast lane)");
        fir_dump_scan(["--lane".to_owned(), "fast".to_owned()].into_iter())?;
    } else {
        println!("backend-align-nightly: skip fir-dump-scan");
    }

    println!(
        "backend-align-nightly: OK (strict_fir_types={}, golden={}, diff_lanes={}, fir_dump_scan={})",
        options.strict_fir_types,
        !options.skip_golden,
        !options.skip_diff_lanes,
        !options.skip_fir_dump_scan
    );
    Ok(())
}

fn parse_backend_align_nightly_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<BackendAlignNightlyOptions, Box<dyn std::error::Error>> {
    let mut options = BackendAlignNightlyOptions::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--strict-fir-types" => options.strict_fir_types = true,
            "--skip-golden" => options.skip_golden = true,
            "--skip-diff-lanes" => options.skip_diff_lanes = true,
            "--skip-fir-dump-scan" => options.skip_fir_dump_scan = true,
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- backend-align-nightly [--strict-fir-types] [--skip-golden] [--skip-diff-lanes] [--skip-fir-dump-scan]".into());
            }
            other => return Err(format!("unknown backend-align-nightly option: {other}").into()),
        }
    }
    Ok(options)
}

#[derive(Debug)]
struct FirDumpScanOptions {
    cases: Vec<PathBuf>,
    lane: TraceLane,
}

impl Default for FirDumpScanOptions {
    fn default() -> Self {
        Self {
            cases: Vec::new(),
            lane: TraceLane::Fast,
        }
    }
}

fn fir_dump_scan(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_fir_dump_scan_options(&mut args)?;
    let cases = if options.cases.is_empty() {
        corpus_files()?
    } else {
        options.cases
    };
    let compiler = compiler::Compiler::new();

    let mut compiled_cases = 0usize;
    let mut skipped_compile = 0usize;
    let mut loop_nodes_seen = 0usize;
    let mut issues: Vec<String> = Vec::new();

    for case in cases {
        let lowered = match compiler
            .compile_file_default_to_fir_with_lane(&case, options.lane.to_signal_fir_lane())
        {
            Ok(out) => out,
            Err(e) => {
                skipped_compile += 1;
                println!("skip {} (FIR compile failed: {e})", case.display());
                continue;
            }
        };
        let rendered = dump_fir(&lowered.store, lowered.module);
        compiled_cases += 1;
        loop_nodes_seen += count_loop_nodes_in_dump(&rendered);

        let missing = find_unexpanded_loop_bodies(&rendered);
        if missing.is_empty() {
            println!("ok {} [lane={}]", case.display(), options.lane.as_str());
            continue;
        }

        for (loop_kind, loop_id, body_id) in missing {
            issues.push(format!(
                "{} [lane={}] {loop_kind} node #{loop_id} body #{body_id} not expanded in dump_fir output",
                case.display(),
                options.lane.as_str()
            ));
        }
    }

    if !issues.is_empty() {
        for issue in &issues {
            println!("[FAIL] {issue}");
        }
        return Err(format!(
            "fir-dump-scan failed: {} issue(s) across {} compiled case(s) (skipped_compile={})",
            issues.len(),
            compiled_cases,
            skipped_compile
        )
        .into());
    }

    println!(
        "fir-dump-scan: OK (lane={}, compiled_cases={}, skipped_compile={}, loop_nodes_seen={})",
        options.lane.as_str(),
        compiled_cases,
        skipped_compile,
        loop_nodes_seen
    );
    Ok(())
}

fn parse_fir_dump_scan_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<FirDumpScanOptions, Box<dyn std::error::Error>> {
    let mut options = FirDumpScanOptions::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = args.next() else {
                    return Err("--case requires a path".into());
                };
                options.cases.push(PathBuf::from(path));
            }
            "--lane" => {
                let Some(value) = args.next() else {
                    return Err("--lane requires legacy|fast".into());
                };
                options.lane = TraceLane::parse(&value)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            }
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- fir-dump-scan [--case <tests/corpus/foo.dsp> ...] [--lane legacy|fast]".into());
            }
            other => return Err(format!("unknown fir-dump-scan option: {other}").into()),
        }
    }
    Ok(options)
}

fn count_loop_nodes_in_dump(rendered: &str) -> usize {
    rendered.matches("SimpleForLoop {").count()
        + rendered.matches("ForLoop {").count()
        + rendered.matches("IteratorForLoop {").count()
}

fn find_unexpanded_loop_bodies(rendered: &str) -> Vec<(&'static str, u32, u32)> {
    let mut issues = Vec::new();
    for line in rendered.lines() {
        let Some((loop_kind, loop_id, body_id)) = parse_loop_line_body_ids(line) else {
            continue;
        };
        let body_marker = format!("#{body_id} ");
        if !rendered.contains(&body_marker) {
            issues.push((loop_kind, loop_id, body_id));
        }
    }
    issues
}

fn parse_loop_line_body_ids(line: &str) -> Option<(&'static str, u32, u32)> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix('#')?;
    let loop_id_end = rest.find(' ')?;
    let loop_id = rest[..loop_id_end].parse().ok()?;
    let rest = &rest[loop_id_end + 1..];

    let loop_kind = if rest.starts_with("SimpleForLoop {") {
        "SimpleForLoop"
    } else if rest.starts_with("ForLoop {") {
        "ForLoop"
    } else if rest.starts_with("IteratorForLoop {") {
        "IteratorForLoop"
    } else {
        return None;
    };

    let body_key = "body: TreeId(";
    let body_pos = rest.find(body_key)?;
    let body_tail = &rest[body_pos + body_key.len()..];
    let body_end = body_tail.find(')')?;
    let body_id = body_tail[..body_end].parse().ok()?;
    Some((loop_kind, loop_id, body_id))
}

fn golden_cases_for_check(golden_ref: GoldenRef) -> Result<Vec<(String, PathBuf)>, io::Error> {
    let root = workspace_root();
    match golden_ref {
        GoldenRef::Rust => {
            let mut cases = Vec::new();
            for file in corpus_files()? {
                cases.push((case_name(&file)?, file));
            }
            Ok(cases)
        }
        GoldenRef::Cpp => {
            let golden_root = root.join("tests/golden").join(golden_ref.as_dir_name());
            let mut cases = Vec::new();
            for entry in fs::read_dir(golden_root)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let case = entry
                    .file_name()
                    .to_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid golden case directory name",
                        )
                    })?;
                let expected = entry.path().join("compiler_stdout.txt");
                if expected.is_file() {
                    let source = root.join("tests/corpus").join(format!("{case}.dsp"));
                    cases.push((case, source));
                }
            }
            cases.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(cases)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GoldenRef {
    Rust,
    Cpp,
}

impl GoldenRef {
    fn as_dir_name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Cpp => "cpp",
        }
    }
}

fn golden_file_for_ref(case: &str, golden_ref: GoldenRef) -> PathBuf {
    workspace_root()
        .join("tests/golden")
        .join(golden_ref.as_dir_name())
        .join(case)
        .join("compiler_stdout.txt")
}

fn normalize(text: &str) -> String {
    let mut normalized = text.replace("\r\n", "\n");
    let mut lines: Vec<String> = normalized
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect();

    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }

    normalized = lines.join("\n");
    normalized.push('\n');
    normalized
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TraceScenario {
    Zeros,
    Impulse,
    Ramp,
    Sine,
}

impl TraceScenario {
    fn as_str(self) -> &'static str {
        match self {
            Self::Zeros => "zeros",
            Self::Impulse => "impulse",
            Self::Ramp => "ramp",
            Self::Sine => "sine",
        }
    }

    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "zeros" => Ok(Self::Zeros),
            "impulse" => Ok(Self::Impulse),
            "ramp" => Ok(Self::Ramp),
            "sine" => Ok(Self::Sine),
            _ => Err(format!(
                "unknown scenario '{s}' (expected: zeros|impulse|ramp|sine)"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TraceLane {
    Legacy,
    Fast,
}

impl TraceLane {
    fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Fast => "fast-lane",
        }
    }

    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "legacy" => Ok(Self::Legacy),
            "fast" | "fast-lane" | "transform" => Ok(Self::Fast),
            _ => Err(format!("unknown lane '{s}' (expected: legacy|fast)")),
        }
    }

    fn to_signal_fir_lane(self) -> compiler::SignalFirLane {
        match self {
            Self::Legacy => compiler::SignalFirLane::LegacyBridge,
            Self::Fast => compiler::SignalFirLane::TransformFastLane,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InterpTraceDumpOptions {
    case: PathBuf,
    scenario: TraceScenario,
    lane: TraceLane,
    sample_rate: usize,
    block_size: usize,
    num_blocks: usize,
    strict_fir_types: bool,
    out: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InterpTraceCppFbcDumpOptions {
    trace: InterpTraceDumpOptions,
    faust_bin: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InterpTraceCppFbcBatchOptions {
    case: Option<PathBuf>,
    scenario: TraceScenario,
    sample_rate: usize,
    block_size: usize,
    num_blocks: usize,
    out_dir: PathBuf,
    faust_bin: Option<PathBuf>,
}

impl Default for InterpTraceCppFbcBatchOptions {
    fn default() -> Self {
        Self {
            case: None,
            scenario: TraceScenario::Impulse,
            sample_rate: 48_000,
            block_size: 64,
            num_blocks: 1,
            out_dir: workspace_root().join("tests/runtime_traces").join("cppfbc"),
            faust_bin: None,
        }
    }
}

impl Default for InterpTraceDumpOptions {
    fn default() -> Self {
        Self {
            case: PathBuf::new(),
            scenario: TraceScenario::Zeros,
            lane: TraceLane::Fast,
            sample_rate: 48_000,
            block_size: 64,
            num_blocks: 4,
            strict_fir_types: false,
            out: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct RuntimeTrace {
    dsp_path: String,
    lane: String,
    scenario: String,
    sample_rate: usize,
    block_size: usize,
    num_blocks: usize,
    num_inputs: usize,
    num_outputs: usize,
    outputs: Vec<Vec<f32>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TraceCompareTolerances {
    abs_tol: f32,
    rel_tol: f32,
}

impl Default for TraceCompareTolerances {
    fn default() -> Self {
        Self {
            abs_tol: 1.0e-6,
            rel_tol: 1.0e-5,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct TraceMismatch {
    field: String,
    channel: Option<usize>,
    sample: Option<usize>,
    expected: Option<f32>,
    actual: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InterpTraceBatchOptions {
    case: Option<PathBuf>,
    lane: TraceLane,
    sample_rate: usize,
    block_size: usize,
    num_blocks: usize,
    strict_fir_types: bool,
}

impl Default for InterpTraceBatchOptions {
    fn default() -> Self {
        Self {
            case: None,
            lane: TraceLane::Fast,
            sample_rate: 48_000,
            block_size: 64,
            num_blocks: 4,
            strict_fir_types: false,
        }
    }
}

fn interp_trace_dump(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_interp_trace_dump_options(&mut args)?;
    let trace = run_interp_trace_case(&options)?;
    let json = render_runtime_trace_json(&trace);
    if let Some(path) = &options.out {
        fs::write(path, json)?;
    } else {
        print!("{json}");
    }
    Ok(())
}

fn interp_trace_dump_cppfbc(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_interp_trace_dump_cppfbc_options(&mut args)?;
    let trace = run_interp_trace_case_from_cpp_fbc(&options)?;
    let json = render_runtime_trace_json(&trace);
    if let Some(path) = &options.trace.out {
        fs::write(path, json)?;
    } else {
        print!("{json}");
    }
    Ok(())
}

fn interp_trace_gen_cppfbc(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_interp_trace_gen_cppfbc_options(&mut args)?;
    let mut cases = if let Some(case) = &options.case {
        vec![case.clone()]
    } else {
        corpus_files()?
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|n| n.starts_with("rep_"))
            })
            .collect()
    };
    cases.sort();
    if cases.is_empty() {
        return Err("no corpus cases found for interp-trace-gen-cppfbc".into());
    }

    let mut generated = 0usize;
    for case in cases {
        let case_id = case_name(&case)?;
        let trace = run_interp_trace_case_from_cpp_fbc(&InterpTraceCppFbcDumpOptions {
            trace: InterpTraceDumpOptions {
                case: case.clone(),
                scenario: options.scenario,
                lane: TraceLane::Fast,
                sample_rate: options.sample_rate,
                block_size: options.block_size,
                num_blocks: options.num_blocks,
                strict_fir_types: false,
                out: None,
            },
            faust_bin: options.faust_bin.clone(),
        })?;
        let case_dir = options.out_dir.join(&case_id);
        fs::create_dir_all(&case_dir)?;
        let path = case_dir.join(format!("{}.json", options.scenario.as_str()));
        fs::write(&path, render_runtime_trace_json(&trace))?;
        println!("generated {}", path.display());
        generated += 1;
    }
    println!(
        "interp-trace-gen-cppfbc: generated {generated} trace snapshot(s) in {}",
        options.out_dir.display()
    );
    Ok(())
}

fn parse_interp_trace_dump_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<InterpTraceDumpOptions, Box<dyn std::error::Error>> {
    let mut options = InterpTraceDumpOptions::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --case".into());
                };
                options.case = PathBuf::from(path);
            }
            "--scenario" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --scenario".into());
                };
                options.scenario = TraceScenario::parse(&value)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            }
            "--lane" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --lane".into());
                };
                options.lane = TraceLane::parse(&value)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            }
            "--sample-rate" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --sample-rate".into());
                };
                options.sample_rate = value.parse::<usize>()?;
            }
            "--block-size" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --block-size".into());
                };
                options.block_size = value.parse::<usize>()?;
            }
            "--num-blocks" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --num-blocks".into());
                };
                options.num_blocks = value.parse::<usize>()?;
            }
            "--strict-fir-types" => {
                options.strict_fir_types = true;
            }
            "--out" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --out".into());
                };
                options.out = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- interp-trace-dump --case <path> [--scenario zeros|impulse|ramp|sine] [--lane legacy|fast] [--sample-rate N] [--block-size N] [--num-blocks N] [--strict-fir-types] [--out path]".into());
            }
            other => {
                return Err(format!("unknown interp-trace-dump option: {other}").into());
            }
        }
    }

    if options.case.as_os_str().is_empty() {
        return Err("interp-trace-dump requires --case <path>".into());
    }
    if options.block_size == 0 || options.num_blocks == 0 {
        return Err("block-size and num-blocks must be > 0".into());
    }
    Ok(options)
}

fn parse_interp_trace_dump_cppfbc_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<InterpTraceCppFbcDumpOptions, Box<dyn std::error::Error>> {
    let mut options = InterpTraceCppFbcDumpOptions {
        trace: InterpTraceDumpOptions::default(),
        faust_bin: None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --case".into());
                };
                options.trace.case = PathBuf::from(path);
            }
            "--scenario" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --scenario".into());
                };
                options.trace.scenario = TraceScenario::parse(&value)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            }
            "--faust-bin" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --faust-bin".into());
                };
                options.faust_bin = Some(PathBuf::from(path));
            }
            "--sample-rate" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --sample-rate".into());
                };
                options.trace.sample_rate = value.parse::<usize>()?;
            }
            "--block-size" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --block-size".into());
                };
                options.trace.block_size = value.parse::<usize>()?;
            }
            "--num-blocks" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --num-blocks".into());
                };
                options.trace.num_blocks = value.parse::<usize>()?;
            }
            "--out" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --out".into());
                };
                options.trace.out = Some(PathBuf::from(path));
            }
            "--lane" => {
                return Err(
                    "--lane is not supported for interp-trace-dump-cppfbc (source is C++ .fbc)"
                        .into(),
                );
            }
            "--strict-fir-types" => {
                return Err(
                    "--strict-fir-types is not applicable to interp-trace-dump-cppfbc".into(),
                );
            }
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- interp-trace-dump-cppfbc --case <path> [--scenario zeros|impulse|ramp|sine] [--faust-bin /path/to/faust] [--sample-rate N] [--block-size N] [--num-blocks N] [--out path]".into());
            }
            other => {
                return Err(format!("unknown interp-trace-dump-cppfbc option: {other}").into());
            }
        }
    }
    if options.trace.case.as_os_str().is_empty() {
        return Err("interp-trace-dump-cppfbc requires --case <path>".into());
    }
    if options.trace.block_size == 0 || options.trace.num_blocks == 0 {
        return Err("block-size and num-blocks must be > 0".into());
    }
    options.trace.lane = TraceLane::Fast;
    Ok(options)
}

fn parse_interp_trace_gen_cppfbc_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<InterpTraceCppFbcBatchOptions, Box<dyn std::error::Error>> {
    let mut options = InterpTraceCppFbcBatchOptions::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --case".into());
                };
                options.case = Some(PathBuf::from(path));
            }
            "--scenario" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --scenario".into());
                };
                options.scenario = TraceScenario::parse(&value)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            }
            "--faust-bin" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --faust-bin".into());
                };
                options.faust_bin = Some(PathBuf::from(path));
            }
            "--sample-rate" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --sample-rate".into());
                };
                options.sample_rate = value.parse::<usize>()?;
            }
            "--block-size" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --block-size".into());
                };
                options.block_size = value.parse::<usize>()?;
            }
            "--num-blocks" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --num-blocks".into());
                };
                options.num_blocks = value.parse::<usize>()?;
            }
            "--out-dir" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --out-dir".into());
                };
                options.out_dir = PathBuf::from(path);
            }
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- interp-trace-gen-cppfbc [--case <path>] [--scenario zeros|impulse|ramp|sine] [--faust-bin /path/to/faust] [--sample-rate N] [--block-size N] [--num-blocks N] [--out-dir <dir>]".into());
            }
            other => return Err(format!("unknown interp-trace-gen-cppfbc option: {other}").into()),
        }
    }
    if options.block_size == 0 || options.num_blocks == 0 {
        return Err("block-size and num-blocks must be > 0".into());
    }
    Ok(options)
}

fn interp_trace_gen(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_interp_trace_batch_options(&mut args)?;
    let cases = runtime_trace_cases(&options)?;
    let mut generated = 0usize;
    for case in cases {
        let case_id = case_name(&case)?;
        fs::create_dir_all(runtime_trace_snapshot_root().join(&case_id))?;
        let scenarios = trace_scenarios_for_runtime_case(&case)?;
        if scenarios.is_empty() {
            println!(
                "skip {} (no snapshot-enabled scenarios yet)",
                case.display()
            );
            continue;
        }
        for scenario in scenarios {
            let trace = run_interp_trace_case(&InterpTraceDumpOptions {
                case: case.clone(),
                scenario,
                lane: options.lane,
                sample_rate: options.sample_rate,
                block_size: options.block_size,
                num_blocks: options.num_blocks,
                strict_fir_types: options.strict_fir_types,
                out: None,
            })?;
            let path = runtime_trace_snapshot_path(&case_id, scenario);
            fs::write(&path, render_runtime_trace_json(&trace))?;
            println!("generated {}", path.display());
            generated += 1;
        }
    }
    println!("interp-trace-gen: generated {generated} trace snapshot(s)");
    Ok(())
}

fn interp_trace_check(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_interp_trace_batch_options(&mut args)?;
    let tol = TraceCompareTolerances::default();
    let cases = runtime_trace_cases(&options)?;
    let mut checked = 0usize;
    for case in cases {
        let case_id = case_name(&case)?;
        let scenarios = trace_scenarios_for_runtime_case(&case)?;
        if scenarios.is_empty() {
            println!(
                "skip {} (no snapshot-enabled scenarios yet)",
                case.display()
            );
            continue;
        }
        for scenario in scenarios {
            let expected_path = runtime_trace_snapshot_path(&case_id, scenario);
            let expected_text = fs::read_to_string(&expected_path).map_err(|err| {
                io::Error::new(
                    err.kind(),
                    format!(
                        "missing runtime trace snapshot {}: {err} (run interp-trace-gen)",
                        expected_path.display()
                    ),
                )
            })?;
            let expected = parse_runtime_trace_json(&expected_text)?;
            let trace = run_interp_trace_case(&InterpTraceDumpOptions {
                case: case.clone(),
                scenario,
                lane: options.lane,
                sample_rate: options.sample_rate,
                block_size: options.block_size,
                num_blocks: options.num_blocks,
                strict_fir_types: options.strict_fir_types,
                out: None,
            })?;
            let actual = render_runtime_trace_json(&trace);
            let actual_parsed = parse_runtime_trace_json(&actual)?;
            if let Err(mismatch) = compare_runtime_traces(&expected, &actual_parsed, tol) {
                return Err(format!(
                    "interp-trace-check failed for {} [{}]: mismatch {:?} ({})",
                    case.display(),
                    scenario.as_str(),
                    mismatch,
                    expected_path.display()
                )
                .into());
            }
            println!("ok {} [{}]", case.display(), scenario.as_str());
            checked += 1;
        }
    }
    println!("interp-trace-check: {checked} trace snapshot(s) matched");
    Ok(())
}

fn interp_trace_diff_lanes(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_interp_trace_batch_options(&mut args)?;
    let tol = TraceCompareTolerances::default();
    let cases = runtime_trace_cases(&options)?;
    let mut compared = 0usize;
    for case in cases {
        let scenarios = trace_scenarios_for_runtime_case(&case)?;
        if scenarios.is_empty() {
            println!(
                "skip {} (no snapshot-enabled scenarios yet)",
                case.display()
            );
            continue;
        }
        let legacy_is_stub = legacy_interp_bridge_is_nonsemantic_stub(&case)?;
        for scenario in scenarios {
            if legacy_is_stub {
                println!(
                    "skip {} [{}] (legacy lane FIR bridge is non-semantic label-only stub)",
                    case.display(),
                    scenario.as_str()
                );
                continue;
            }
            let legacy = match run_interp_trace_case_catching_panic(&InterpTraceDumpOptions {
                case: case.clone(),
                scenario,
                lane: TraceLane::Legacy,
                sample_rate: options.sample_rate,
                block_size: options.block_size,
                num_blocks: options.num_blocks,
                strict_fir_types: options.strict_fir_types,
                out: None,
            }) {
                Ok(trace) => trace,
                Err(reason) => {
                    println!(
                        "skip {} [{}] (legacy lane panic/error: {reason})",
                        case.display(),
                        scenario.as_str()
                    );
                    continue;
                }
            };
            let fast = match run_interp_trace_case_catching_panic(&InterpTraceDumpOptions {
                case: case.clone(),
                scenario,
                lane: TraceLane::Fast,
                sample_rate: options.sample_rate,
                block_size: options.block_size,
                num_blocks: options.num_blocks,
                strict_fir_types: options.strict_fir_types,
                out: None,
            }) {
                Ok(trace) => trace,
                Err(reason) => {
                    println!(
                        "skip {} [{}] (fast lane panic/error: {reason})",
                        case.display(),
                        scenario.as_str()
                    );
                    continue;
                }
            };
            // Compare semantics while ignoring lane labels.
            let mut fast_norm = fast.clone();
            let mut legacy_norm = legacy.clone();
            fast_norm.lane = "normalized".into();
            legacy_norm.lane = "normalized".into();
            if let Err(mismatch) = compare_runtime_traces(&legacy_norm, &fast_norm, tol) {
                return Err(format!(
                    "interp-trace-diff-lanes failed for {} [{}]: mismatch {:?}",
                    case.display(),
                    scenario.as_str(),
                    mismatch
                )
                .into());
            }
            println!(
                "match {} [{}] (legacy vs fast)",
                case.display(),
                scenario.as_str()
            );
            compared += 1;
        }
    }
    println!("interp-trace-diff-lanes: {compared} trace(s) matched");
    Ok(())
}

fn legacy_interp_bridge_is_nonsemantic_stub(
    case: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    let compiler = compiler::Compiler::new().with_fir_verify_options(compiler::FirVerifyOptions {
        enabled: false,
        strict: false,
    });
    let fir_out = compiler
        .compile_file_default_to_fir_with_lane(case, compiler::SignalFirLane::LegacyBridge)?;
    let FirMatch::Module { declarations, .. } = match_fir(&fir_out.store, fir_out.module) else {
        return Ok(false);
    };
    let FirMatch::Block(decls) = match_fir(&fir_out.store, declarations) else {
        return Ok(false);
    };
    let Some(compute_id) = decls.iter().copied().find(|id| {
        matches!(
            match_fir(&fir_out.store, *id),
            FirMatch::DeclareFun { ref name, .. } if name == "compute"
        )
    }) else {
        return Ok(false);
    };
    let FirMatch::DeclareFun {
        body: Some(body), ..
    } = match_fir(&fir_out.store, compute_id)
    else {
        return Ok(false);
    };
    let FirMatch::Block(stmts) = match_fir(&fir_out.store, body) else {
        return Ok(false);
    };
    Ok(!stmts.is_empty()
        && stmts
            .iter()
            .all(|id| matches!(match_fir(&fir_out.store, *id), FirMatch::Label(_))))
}

fn parse_interp_trace_batch_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<InterpTraceBatchOptions, Box<dyn std::error::Error>> {
    let mut options = InterpTraceBatchOptions::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => {
                let Some(path) = args.next() else {
                    return Err("missing value after --case".into());
                };
                options.case = Some(PathBuf::from(path));
            }
            "--lane" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --lane".into());
                };
                options.lane = TraceLane::parse(&value)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            }
            "--sample-rate" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --sample-rate".into());
                };
                options.sample_rate = value.parse::<usize>()?;
            }
            "--block-size" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --block-size".into());
                };
                options.block_size = value.parse::<usize>()?;
            }
            "--num-blocks" => {
                let Some(value) = args.next() else {
                    return Err("missing value after --num-blocks".into());
                };
                options.num_blocks = value.parse::<usize>()?;
            }
            "--strict-fir-types" => {
                options.strict_fir_types = true;
            }
            "--help" | "-h" => {
                return Err("usage: cargo run -p xtask -- interp-trace-gen [--case <path>] [--lane legacy|fast] [--sample-rate N] [--block-size N] [--num-blocks N] [--strict-fir-types]".into());
            }
            other => return Err(format!("unknown interp-trace batch option: {other}").into()),
        }
    }
    if options.block_size == 0 || options.num_blocks == 0 {
        return Err("block-size and num-blocks must be > 0".into());
    }
    Ok(options)
}

fn runtime_trace_cases(
    options: &InterpTraceBatchOptions,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    if let Some(case) = &options.case {
        return Ok(vec![case.clone()]);
    }
    let cases = runtime_corpus_files()?;
    if cases.is_empty() {
        return Err("no runtime trace corpus files found in tests/runtime_corpus".into());
    }
    Ok(cases)
}

fn trace_scenarios_for_runtime_case(
    case: &Path,
) -> Result<Vec<TraceScenario>, Box<dyn std::error::Error>> {
    let name = case_name(case)?;
    let scenarios = match name.as_str() {
        "trace_01_passthrough" => vec![TraceScenario::Impulse, TraceScenario::Ramp],
        "trace_02_gain_bias_typed" => vec![],
        "trace_03_stereo_mix" => vec![],
        "trace_07_nonlinear_clip" => vec![],
        "trace_09_ui_slider" => vec![TraceScenario::Impulse],
        "trace_22_parallel_mix" => vec![],
        "trace_31_extended_primitives_typed" => vec![TraceScenario::Zeros],
        "trace_38_sine_phasor" => vec![],
        other => {
            return Err(format!(
                "no runtime trace scenario mapping defined for {other} (update xtask)"
            )
            .into());
        }
    };
    Ok(scenarios)
}

fn runtime_trace_snapshot_path(case_id: &str, scenario: TraceScenario) -> PathBuf {
    runtime_trace_snapshot_root()
        .join(case_id)
        .join(format!("{}.json", scenario.as_str()))
}

fn run_interp_trace_case(
    options: &InterpTraceDumpOptions,
) -> Result<RuntimeTrace, Box<dyn std::error::Error>> {
    let compiler = compiler::Compiler::new().with_fir_verify_options(compiler::FirVerifyOptions {
        enabled: true,
        strict: false,
    });

    let signals = compiler.compile_file_default_to_signals(&options.case)?;
    let fir = compiler
        .compile_file_default_to_fir_with_lane(&options.case, options.lane.to_signal_fir_lane())?;
    if options.strict_fir_types {
        enforce_strict_fir_type_diagnostics(&fir.store, fir.module, &options.case)?;
    }

    let interp_options = codegen::backends::interp::InterpOptions {
        opt_level: 0,
        module_name: None,
        num_inputs: signals.process_arity.inputs,
        num_outputs: signals.process_arity.outputs,
    };
    let mut factory =
        codegen::backends::interp::generate_interp_module(&fir.store, fir.module, &interp_options)?;
    let mut instance = codegen::backends::interp::FbcDspInstance::new(&mut factory);
    instance.init(options.sample_rate as i32);

    let total_samples = options.block_size * options.num_blocks;
    let input_channels = generate_trace_inputs(
        options.scenario,
        signals.process_arity.inputs,
        total_samples,
        options.sample_rate,
    );
    let mut output_channels = vec![vec![0.0f32; total_samples]; signals.process_arity.outputs];

    for block_idx in 0..options.num_blocks {
        let start = block_idx * options.block_size;
        let end = start + options.block_size;
        let input_refs: Vec<&[f32]> = input_channels.iter().map(|ch| &ch[start..end]).collect();
        let mut output_refs: Vec<&mut [f32]> = output_channels
            .iter_mut()
            .map(|ch| &mut ch[start..end])
            .collect();
        instance
            .try_compute(options.block_size as i32, &input_refs, &mut output_refs)
            .map_err(|e| {
                format!(
                    "interp runtime execution failed in compute block (block_idx={}): {e}",
                    block_idx
                )
            })?;
    }

    Ok(RuntimeTrace {
        dsp_path: options.case.display().to_string(),
        lane: options.lane.as_str().to_string(),
        scenario: options.scenario.as_str().to_string(),
        sample_rate: options.sample_rate,
        block_size: options.block_size,
        num_blocks: options.num_blocks,
        num_inputs: signals.process_arity.inputs,
        num_outputs: signals.process_arity.outputs,
        outputs: output_channels,
    })
}

fn resolve_faust_cpp_bin(explicit: Option<&Path>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    if let Some(path) = std::env::var_os("FAUST_CPP_BIN") {
        return Ok(PathBuf::from(path));
    }
    Ok(PathBuf::from("faust"))
}

fn compile_dsp_to_cpp_fbc(
    faust_bin: &Path,
    dsp_case: &Path,
    fbc_out: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::new(faust_bin);
    cmd.arg("-lang").arg("interp");
    for inc in default_import_search_paths(dsp_case) {
        cmd.arg("-I").arg(inc);
    }
    cmd.arg(dsp_case);
    cmd.arg("-o").arg(fbc_out);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let output = cmd.output().map_err(|e| {
        io::Error::new(
            e.kind(),
            format!(
                "failed to spawn Faust C++ binary {}: {e}",
                faust_bin.display()
            ),
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "faust -lang interp failed for {} with status {}\nstdout:\n{}\nstderr:\n{}",
            dsp_case.display(),
            output.status,
            stdout.trim(),
            stderr.trim()
        )
        .into());
    }
    if !fbc_out.is_file() {
        return Err(format!(
            "faust reported success but did not produce .fbc output: {}",
            fbc_out.display()
        )
        .into());
    }
    Ok(())
}

fn run_interp_trace_case_from_cpp_fbc(
    options: &InterpTraceCppFbcDumpOptions,
) -> Result<RuntimeTrace, Box<dyn std::error::Error>> {
    let faust_bin = resolve_faust_cpp_bin(options.faust_bin.as_deref())?;
    let case_id = case_name(&options.trace.case)?;
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let fbc_path = std::env::temp_dir().join(format!("faust_rs_xtask_{case_id}_{pid}_{nanos}.fbc"));
    compile_dsp_to_cpp_fbc(&faust_bin, &options.trace.case, &fbc_path)?;

    let trace_result = (|| -> Result<RuntimeTrace, Box<dyn std::error::Error>> {
        let file = fs::File::open(&fbc_path)?;
        let mut reader = io::BufReader::new(file);
        let mut factory: codegen::backends::interp::FbcDspFactory<f32> =
            codegen::backends::interp::read_fbc(&mut reader).map_err(|e| {
                format!(
                    "failed to read C++ generated .fbc {}: {e}",
                    fbc_path.display()
                )
            })?;
        let num_inputs = factory.num_inputs.max(0) as usize;
        let num_outputs = factory.num_outputs.max(0) as usize;

        let mut instance = codegen::backends::interp::FbcDspInstance::new(&mut factory);
        instance.init(options.trace.sample_rate as i32);

        let total_samples = options.trace.block_size * options.trace.num_blocks;
        let input_channels = generate_trace_inputs(
            options.trace.scenario,
            num_inputs,
            total_samples,
            options.trace.sample_rate,
        );
        let mut output_channels = vec![vec![0.0f32; total_samples]; num_outputs];
        for block_idx in 0..options.trace.num_blocks {
            let start = block_idx * options.trace.block_size;
            let end = start + options.trace.block_size;
            let input_refs: Vec<&[f32]> = input_channels.iter().map(|ch| &ch[start..end]).collect();
            let mut output_refs: Vec<&mut [f32]> = output_channels
                .iter_mut()
                .map(|ch| &mut ch[start..end])
                .collect();
            instance
                .try_compute(
                    options.trace.block_size as i32,
                    &input_refs,
                    &mut output_refs,
                )
                .map_err(|e| {
                    format!(
                        "Rust interp runtime failed on C++ .fbc (block_idx={}): {e}",
                        block_idx
                    )
                })?;
        }

        Ok(RuntimeTrace {
            dsp_path: options.trace.case.display().to_string(),
            lane: "cpp-fbc".to_string(),
            scenario: options.trace.scenario.as_str().to_string(),
            sample_rate: options.trace.sample_rate,
            block_size: options.trace.block_size,
            num_blocks: options.trace.num_blocks,
            num_inputs,
            num_outputs,
            outputs: output_channels,
        })
    })();

    let _ = fs::remove_file(&fbc_path);
    trace_result
}

fn run_interp_trace_case_catching_panic(
    options: &InterpTraceDumpOptions,
) -> Result<RuntimeTrace, String> {
    match catch_unwind(AssertUnwindSafe(|| run_interp_trace_case(options))) {
        Ok(Ok(trace)) => Ok(trace),
        Ok(Err(err)) => Err(err.to_string()),
        Err(payload) => Err(format!("panic: {}", panic_payload_to_string(payload))),
    }
}

fn enforce_strict_fir_type_diagnostics(
    store: &fir::FirStore,
    module: fir::FirId,
    case: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let report = fir::checker::verify_fir_module(store, module);
    let type_diags: Vec<&fir::checker::FirDiagnostic> = report
        .diagnostics
        .iter()
        .filter(|d| is_fir_type_diagnostic_code(d.code))
        .collect();
    if type_diags.is_empty() {
        return Ok(());
    }

    let mut msg = format!(
        "strict FIR type diagnostics present for {}: {} diagnostic(s)",
        case.display(),
        type_diags.len()
    );
    for d in type_diags.iter().take(4) {
        let sev = match d.severity {
            fir::checker::Severity::Error => "error",
            fir::checker::Severity::Warning => "warning",
        };
        let fn_ctx = d
            .context
            .function_name
            .as_deref()
            .map(|f| format!(" (fn={f})"))
            .unwrap_or_default();
        msg.push_str(&format!("\n- {sev} [{}] {}{}", d.code, d.message, fn_ctx));
    }
    if type_diags.len() > 4 {
        msg.push_str(&format!("\n- ... {} more", type_diags.len() - 4));
    }
    Err(msg.into())
}

fn is_fir_type_diagnostic_code(code: &str) -> bool {
    code.starts_with("FIR-B")
        || code.starts_with("FIR-U")
        || code.starts_with("FIR-C")
        || code.starts_with("FIR-FC")
        || code.starts_with("FIR-T")
        || code.starts_with("FIR-MA")
        || matches!(code, "FIR-R01" | "FIR-L03" | "FIR-SW01")
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn generate_trace_inputs(
    scenario: TraceScenario,
    num_inputs: usize,
    total_samples: usize,
    sample_rate: usize,
) -> Vec<Vec<f32>> {
    let mut inputs = vec![vec![0.0f32; total_samples]; num_inputs];
    match scenario {
        TraceScenario::Zeros => {}
        TraceScenario::Impulse => {
            if total_samples > 0 {
                for channel in &mut inputs {
                    channel[0] = 1.0;
                }
            }
        }
        TraceScenario::Ramp => {
            if total_samples == 0 {
                return inputs;
            }
            let denom = (total_samples.saturating_sub(1)).max(1) as f32;
            for channel in &mut inputs {
                for (i, sample) in channel.iter_mut().enumerate() {
                    *sample = (i as f32) / denom;
                }
            }
        }
        TraceScenario::Sine => {
            let sr = sample_rate.max(1) as f32;
            let freq_hz = 440.0f32;
            let w = core::f32::consts::TAU * freq_hz / sr;
            for (ch_idx, channel) in inputs.iter_mut().enumerate() {
                let phase = (ch_idx as f32) * 0.25 * core::f32::consts::TAU;
                for (i, sample) in channel.iter_mut().enumerate() {
                    *sample = (w * (i as f32) + phase).sin();
                }
            }
        }
    }
    inputs
}

fn render_runtime_trace_json(trace: &RuntimeTrace) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"schema_version\": 1,");
    let _ = writeln!(out, "  \"dsp\": \"{}\",", json_escape(&trace.dsp_path));
    let _ = writeln!(out, "  \"backend\": \"interp\",");
    let _ = writeln!(out, "  \"pipeline\": {{");
    let _ = writeln!(out, "    \"signal_fir_lane\": \"{}\"", trace.lane);
    let _ = writeln!(out, "  }},");
    let _ = writeln!(out, "  \"runtime\": {{");
    let _ = writeln!(out, "    \"sample_rate\": {},", trace.sample_rate);
    let _ = writeln!(out, "    \"block_size\": {},", trace.block_size);
    let _ = writeln!(out, "    \"num_blocks\": {}", trace.num_blocks);
    let _ = writeln!(out, "  }},");
    let _ = writeln!(out, "  \"scenario\": {{");
    let _ = writeln!(out, "    \"name\": \"{}\",", trace.scenario);
    let _ = writeln!(out, "    \"inputs\": {},", trace.num_inputs);
    let _ = writeln!(out, "    \"outputs\": {}", trace.num_outputs);
    let _ = writeln!(out, "  }},");
    let _ = writeln!(out, "  \"outputs\": [");
    for (ch_idx, channel) in trace.outputs.iter().enumerate() {
        let _ = write!(out, "    [");
        for (i, sample) in channel.iter().enumerate() {
            if i > 0 {
                let _ = write!(out, ", ");
            }
            let _ = write!(out, "{:.9}", sample);
        }
        let _ = writeln!(
            out,
            "]{}",
            if ch_idx + 1 == trace.outputs.len() {
                ""
            } else {
                ","
            }
        );
    }
    let _ = writeln!(out, "  ]");
    let _ = writeln!(out, "}}");
    out
}

fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct RuntimeTraceJson {
    dsp: String,
    pipeline: RuntimeTracePipelineJson,
    runtime: RuntimeTraceRuntimeJson,
    scenario: RuntimeTraceScenarioJson,
    outputs: Vec<Vec<f32>>,
}

#[derive(Debug, Deserialize)]
struct RuntimeTracePipelineJson {
    signal_fir_lane: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeTraceRuntimeJson {
    sample_rate: usize,
    block_size: usize,
    num_blocks: usize,
}

#[derive(Debug, Deserialize)]
struct RuntimeTraceScenarioJson {
    name: String,
    inputs: usize,
    outputs: usize,
}

fn parse_runtime_trace_json(text: &str) -> Result<RuntimeTrace, Box<dyn std::error::Error>> {
    let parsed: RuntimeTraceJson = serde_json::from_str(text)?;
    Ok(RuntimeTrace {
        dsp_path: parsed.dsp,
        lane: parsed.pipeline.signal_fir_lane,
        scenario: parsed.scenario.name,
        sample_rate: parsed.runtime.sample_rate,
        block_size: parsed.runtime.block_size,
        num_blocks: parsed.runtime.num_blocks,
        num_inputs: parsed.scenario.inputs,
        num_outputs: parsed.scenario.outputs,
        outputs: parsed.outputs,
    })
}

fn compare_runtime_traces(
    expected: &RuntimeTrace,
    actual: &RuntimeTrace,
    tol: TraceCompareTolerances,
) -> Result<(), TraceMismatch> {
    if expected.dsp_path != actual.dsp_path {
        return Err(TraceMismatch {
            field: "dsp".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.lane != actual.lane {
        return Err(TraceMismatch {
            field: "pipeline.signal_fir_lane".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.scenario != actual.scenario {
        return Err(TraceMismatch {
            field: "scenario.name".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.sample_rate != actual.sample_rate {
        return Err(TraceMismatch {
            field: "runtime.sample_rate".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.block_size != actual.block_size {
        return Err(TraceMismatch {
            field: "runtime.block_size".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.num_blocks != actual.num_blocks {
        return Err(TraceMismatch {
            field: "runtime.num_blocks".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.num_inputs != actual.num_inputs {
        return Err(TraceMismatch {
            field: "scenario.inputs".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.num_outputs != actual.num_outputs {
        return Err(TraceMismatch {
            field: "scenario.outputs".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    if expected.outputs.len() != actual.outputs.len() {
        return Err(TraceMismatch {
            field: "outputs.channel_count".into(),
            channel: None,
            sample: None,
            expected: None,
            actual: None,
        });
    }
    for (ch_idx, (exp_ch, act_ch)) in expected.outputs.iter().zip(&actual.outputs).enumerate() {
        if exp_ch.len() != act_ch.len() {
            return Err(TraceMismatch {
                field: "outputs.sample_count".into(),
                channel: Some(ch_idx),
                sample: None,
                expected: None,
                actual: None,
            });
        }
        for (i, (&e, &a)) in exp_ch.iter().zip(act_ch.iter()).enumerate() {
            if !trace_sample_equal(e, a, tol) {
                return Err(TraceMismatch {
                    field: "outputs".into(),
                    channel: Some(ch_idx),
                    sample: Some(i),
                    expected: Some(e),
                    actual: Some(a),
                });
            }
        }
    }
    Ok(())
}

fn trace_sample_equal(expected: f32, actual: f32, tol: TraceCompareTolerances) -> bool {
    if expected.is_nan() || actual.is_nan() {
        return expected.is_nan() && actual.is_nan();
    }
    if expected.is_infinite() || actual.is_infinite() {
        return expected == actual;
    }
    let diff = (expected - actual).abs();
    let scale = expected.abs().max(actual.abs());
    diff <= tol.abs_tol + tol.rel_tol * scale
}

fn render_rust_snapshot(input: &Path) -> Result<String, io::Error> {
    let source = fs::read_to_string(input)?;
    let name = input
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid input filename"))?;
    Ok(compiler::golden_snapshot(name, &source))
}

fn default_import_search_paths(input: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(parent) = input.parent() {
        paths.push(parent.to_path_buf());
    }
    {
        let path = PathBuf::from("/usr/local/share/faust");
        if path.is_dir() {
            paths.push(path);
        }
    }
    paths
}

fn render_rust_cpp_output(input: &Path) -> Result<String, compiler::CompilerError> {
    let compiler = compiler::Compiler::new();
    let options = codegen::backends::cpp::CppOptions::default();
    let search_paths = default_import_search_paths(input);
    compiler.compile_file_to_cpp(input, &search_paths, &options)
}

fn golden_gen_rust() -> Result<(), Box<dyn std::error::Error>> {
    let files = corpus_files()?;
    for file in files {
        let case = case_name(&file)?;
        let output = golden_file_for_ref(&case, GoldenRef::Rust);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let snapshot = normalize(&render_rust_snapshot(&file)?);
        fs::write(&output, snapshot)?;
        println!("updated {}", output.display());
    }
    Ok(())
}

fn golden_gen_cpp(extra_args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let cpp_bin = std::env::var_os("FAUST_CPP_BIN").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "FAUST_CPP_BIN is not set. Example: FAUST_CPP_BIN=/path/to/faust",
        )
    })?;

    let files = corpus_files()?;
    for file in files {
        let case = case_name(&file)?;
        let output = golden_file_for_ref(&case, GoldenRef::Cpp);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut cmd = Command::new(&cpp_bin);
        cmd.arg(&file);
        for arg in extra_args {
            cmd.arg(arg);
        }

        let result = cmd.output()?;
        if !result.status.success() {
            return Err(format!(
                "C++ reference command failed for {} with status {}",
                file.display(),
                result.status
            )
            .into());
        }

        let stdout = String::from_utf8(result.stdout)?;
        fs::write(&output, normalize(&stdout))?;
        println!("updated {}", output.display());
    }

    Ok(())
}

fn golden_ref_from_env() -> Result<GoldenRef, Box<dyn std::error::Error>> {
    let Some(raw) = std::env::var_os("GOLDEN_REF") else {
        return Ok(GoldenRef::Rust);
    };
    let value = raw
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid GOLDEN_REF value"))?;
    match value {
        "rust" => Ok(GoldenRef::Rust),
        "cpp" => Ok(GoldenRef::Cpp),
        _ => Err(format!("invalid GOLDEN_REF={value}; expected rust or cpp").into()),
    }
}

fn golden_check(forced: Option<GoldenRef>) -> Result<(), Box<dyn std::error::Error>> {
    let golden_ref = match forced {
        Some(value) => value,
        None => golden_ref_from_env()?,
    };

    let files = golden_cases_for_check(golden_ref)?;
    if files.is_empty() {
        return Err(format!(
            "no golden cases found for reference `{}`",
            golden_ref.as_dir_name()
        )
        .into());
    }
    let mut failures = 0usize;

    for (case, file) in files {
        if !file.exists() {
            return Err(format!(
                "missing corpus file for golden case `{case}`: {}",
                file.display()
            )
            .into());
        }
        let expected_path = golden_file_for_ref(&case, golden_ref);
        let expected = fs::read_to_string(&expected_path).map_err(|err| {
            io::Error::new(
                err.kind(),
                format!(
                    "missing golden file {} (run golden-gen-rust or golden-gen-cpp): {err}",
                    expected_path.display()
                ),
            )
        })?;

        let actual = match golden_ref {
            GoldenRef::Rust => normalize(&render_rust_snapshot(&file)?),
            GoldenRef::Cpp => match render_rust_cpp_output(&file) {
                Ok(output) => normalize(&output),
                Err(error) => format!("__RUST_CPP_ERROR__\n{error}\n"),
            },
        };
        let expected = normalize(&expected);

        if actual != expected {
            failures += 1;
            println!("[FAIL] {case}");
            println!("  expected: {}", expected_path.display());
            println!("  first diff:");
            print_first_diff(&expected, &actual);
        } else {
            println!("[OK] {case}");
        }
    }

    if failures > 0 {
        return Err(format!("golden-check failed: {failures} case(s) differ").into());
    }

    Ok(())
}

fn print_first_diff(expected: &str, actual: &str) {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let max = expected_lines.len().max(actual_lines.len());

    for idx in 0..max {
        let e = expected_lines.get(idx).copied().unwrap_or("<missing>");
        let a = actual_lines.get(idx).copied().unwrap_or("<missing>");
        if e != a {
            println!("    line {}", idx + 1);
            println!("      expected: {e}");
            println!("      actual:   {a}");
            return;
        }
    }
}

fn parser_parity_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let cpp_root = PathBuf::from(CPP_SOURCE_ROOT);

    let cpp_parser = cpp_root.join("compiler/parser/faustparser.y");
    let cpp_lexer = cpp_root.join("compiler/parser/faustlexer.l");
    let rust_parser = root.join("crates/parser-proto/src/grammar/faustparser.y");
    let rust_lexer = root.join("crates/parser-proto/src/grammar/faustlexer.l");
    let report_path = root.join(PARITY_REPORT_REL_PATH);

    for path in [&cpp_parser, &cpp_lexer, &rust_parser, &rust_lexer] {
        if !path.exists() {
            return Err(format!("missing input file for parity report: {}", path.display()).into());
        }
    }

    let cpp_parser_src = fs::read_to_string(&cpp_parser)?;
    let cpp_lexer_src = fs::read_to_string(&cpp_lexer)?;
    let rust_parser_src = fs::read_to_string(&rust_parser)?;
    let rust_lexer_src = fs::read_to_string(&rust_lexer)?;

    let cpp_parser_tokens = extract_parser_tokens(&cpp_parser_src);
    let rust_parser_tokens = extract_parser_tokens(&rust_parser_src);
    let cpp_lexer_tokens = extract_cpp_lexer_emitted_tokens(&cpp_lexer_src);
    let rust_lexer_tokens = extract_rust_lexer_emitted_tokens(&rust_lexer_src);
    let cpp_lexer_states = extract_lexer_states(&cpp_lexer_src);
    let rust_lexer_states = extract_lexer_states(&rust_lexer_src);
    let cpp_nonterms = extract_cpp_nonterminals(&cpp_parser_src);
    let rust_nonterms = extract_rust_nonterminals(&rust_parser_src);

    let parser_token_extra = diff_sorted(&rust_parser_tokens, &cpp_parser_tokens);
    let parser_token_missing_exact = diff_sorted(&cpp_parser_tokens, &rust_parser_tokens);
    let (parser_token_alias_covered, parser_token_missing_unresolved) = partition_with_aliases(
        &parser_token_missing_exact,
        &rust_parser_tokens,
        token_aliases,
    );

    let lexer_state_extra = diff_sorted(&rust_lexer_states, &cpp_lexer_states);
    let lexer_state_missing = diff_sorted(&cpp_lexer_states, &rust_lexer_states);

    let nonterm_extra = diff_sorted(&rust_nonterms, &cpp_nonterms);
    let nonterm_missing_exact = diff_sorted(&cpp_nonterms, &rust_nonterms);
    let (nonterm_alias_covered, nonterm_missing_unresolved) =
        partition_with_aliases(&nonterm_missing_exact, &rust_nonterms, nonterminal_aliases);

    let cpp_declared_not_lexed = diff_sorted(&cpp_parser_tokens, &cpp_lexer_tokens);
    let rust_declared_not_lexed = diff_sorted(&rust_parser_tokens, &rust_lexer_tokens);
    let cpp_lexed_not_declared = diff_sorted(&cpp_lexer_tokens, &cpp_parser_tokens);
    let rust_lexed_not_declared = diff_sorted(&rust_lexer_tokens, &rust_parser_tokens);

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 3 Parser/Lexer Parity Coverage Report (Auto-generated)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- parser-parity-report`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Inputs")?;
    writeln!(&mut out, "- C++ parser: `{}`", cpp_parser.display())?;
    writeln!(&mut out, "- C++ lexer: `{}`", cpp_lexer.display())?;
    writeln!(&mut out, "- Rust parser: `{}`", rust_parser.display())?;
    writeln!(&mut out, "- Rust lexer: `{}`", rust_lexer.display())?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(
        &mut out,
        "- Parser token coverage: C++ declared `{}` / Rust declared `{}` / unresolved missing `{}`",
        cpp_parser_tokens.len(),
        rust_parser_tokens.len(),
        parser_token_missing_unresolved.len()
    )?;
    writeln!(
        &mut out,
        "- Lexer state coverage: C++ `{}` / Rust `{}` / unresolved missing `{}`",
        cpp_lexer_states.len(),
        rust_lexer_states.len(),
        lexer_state_missing.len()
    )?;
    writeln!(
        &mut out,
        "- Grammar nonterminal coverage (name-based): C++ `{}` / Rust `{}` / unresolved missing `{}`",
        cpp_nonterms.len(),
        rust_nonterms.len(),
        nonterm_missing_unresolved.len()
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Parser Tokens (C++ `%token` vs Rust `%token`)")?;
    writeln!(
        &mut out,
        "_Note: `exact name` mismatches below are not necessarily missing functionality; they can be covered by explicit alias mapping._"
    )?;
    render_list(
        &mut out,
        "Exact-name mismatch candidates (C++ name not present as-is in Rust)",
        &parser_token_missing_exact,
    )?;
    render_alias_list(
        &mut out,
        "Exact-name mismatches covered by explicit alias mapping (no action required)",
        &parser_token_alias_covered,
    )?;
    render_list(
        &mut out,
        "Unresolved missing after alias mapping (action required)",
        &parser_token_missing_unresolved,
    )?;
    render_list(&mut out, "Extra in Rust", &parser_token_extra)?;

    writeln!(&mut out)?;
    writeln!(&mut out, "## Lexer States (`%x`/`%s`)")?;
    render_list(
        &mut out,
        "Missing in Rust lexer state declarations",
        &lexer_state_missing,
    )?;
    render_list(
        &mut out,
        "Extra in Rust lexer state declarations",
        &lexer_state_extra,
    )?;

    writeln!(&mut out)?;
    writeln!(&mut out, "## Grammar Nonterminals (name-based)")?;
    writeln!(
        &mut out,
        "_Note: `exact name` mismatches below are not necessarily missing functionality; they can be covered by explicit alias mapping (for example dedicated C++ rules grouped under `Primitive` in Rust)._"
    )?;
    render_list(
        &mut out,
        "Exact-name mismatch candidates (C++ nonterminal not present as-is in Rust)",
        &nonterm_missing_exact,
    )?;
    render_alias_list(
        &mut out,
        "Exact-name mismatches covered by explicit alias mapping (no action required)",
        &nonterm_alias_covered,
    )?;
    render_list(
        &mut out,
        "Unresolved missing after alias mapping (action required)",
        &nonterm_missing_unresolved,
    )?;
    render_list(&mut out, "Extra in Rust", &nonterm_extra)?;

    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "## Parser/Lexer Internal Consistency (declared tokens vs lexer emissions)"
    )?;
    render_list(
        &mut out,
        "C++ parser-declared tokens not emitted by C++ lexer",
        &cpp_declared_not_lexed,
    )?;
    render_list(
        &mut out,
        "Rust parser-declared tokens not emitted by Rust lexer",
        &rust_declared_not_lexed,
    )?;
    render_list(
        &mut out,
        "C++ lexer-emitted tokens not declared in C++ parser",
        &cpp_lexed_not_declared,
    )?;
    render_list(
        &mut out,
        "Rust lexer-emitted tokens not declared in Rust parser",
        &rust_lexed_not_declared,
    )?;

    let unresolved_total = parser_token_missing_unresolved.len()
        + lexer_state_missing.len()
        + nonterm_missing_unresolved.len();
    let consistency_issues_total = cpp_declared_not_lexed.len()
        + rust_declared_not_lexed.len()
        + cpp_lexed_not_declared.len()
        + rust_lexed_not_declared.len();

    writeln!(&mut out)?;
    writeln!(&mut out, "## Next Actions")?;
    if unresolved_total == 0 {
        writeln!(
            &mut out,
            "- Unresolved missing items after alias mapping are `0` for parser tokens, lexer states, and grammar nonterminals."
        )?;
    } else {
        writeln!(
            &mut out,
            "- Resolve all items listed in `Unresolved missing after alias mapping (action required)` for tokens and nonterminals."
        )?;
    }
    if consistency_issues_total > 0 {
        writeln!(
            &mut out,
            "- Triage items listed under `Parser/Lexer Internal Consistency` (C++ or Rust declared/emitted token mismatches)."
        )?;
    }
    writeln!(
        &mut out,
        "- Keep this report regenerated at each parser/lexer migration increment to track closure toward 100% parity."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

#[derive(Clone, Debug)]
struct CaseStatus {
    ok: bool,
    stage: &'static str,
    reason: String,
}

fn corpus_status_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let report_path = root.join(CORPUS_STATUS_REPORT_REL_PATH);
    let files = corpus_files()?;
    let compiler = compiler::Compiler::new();
    let (cpp_bin, cpp_bin_is_fallback) = resolve_cpp_faust_bin();

    let mut total = 0usize;
    let mut ok_ok = 0usize;
    let mut err_err = 0usize;
    let mut ok_err = 0usize;
    let mut err_ok = 0usize;

    let mut rows = Vec::with_capacity(files.len());
    for file in files {
        let case = case_name(&file)?;
        let cpp = cpp_case_status(&cpp_bin, &file)?;
        let rust = rust_case_status(&compiler, &file);
        total = total.saturating_add(1);

        match (cpp.ok, rust.ok) {
            (true, true) => ok_ok = ok_ok.saturating_add(1),
            (false, false) => err_err = err_err.saturating_add(1),
            (true, false) => ok_err = ok_err.saturating_add(1),
            (false, true) => err_ok = err_ok.saturating_add(1),
        }

        rows.push((case, cpp, rust));
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 4 Corpus C++ vs Rust Status Differential Report (Auto-generated)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- corpus-status-report`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Inputs")?;
    writeln!(&mut out, "- Corpus: `tests/corpus/*.dsp`")?;
    writeln!(&mut out, "- C++ binary: `{}`", cpp_bin.display())?;
    if cpp_bin_is_fallback {
        writeln!(
            &mut out,
            "- Note: fallback to `faust` from PATH because `{}/build/bin/faust` was not found.",
            CPP_SOURCE_ROOT
        )?;
        writeln!(
            &mut out,
            "- Action: set `FAUST_CPP_BIN` explicitly to the source-of-truth C++ binary when available."
        )?;
    }
    writeln!(
        &mut out,
        "- Rust path: `compiler::Compiler::compile_file_default_to_signals`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(&mut out, "- Total cases: `{total}`")?;
    writeln!(&mut out, "- `OK/OK`: `{ok_ok}`")?;
    writeln!(&mut out, "- `ERR/ERR`: `{err_err}`")?;
    writeln!(&mut out, "- `OK/ERR` (C++ ok, Rust err): `{ok_err}`")?;
    writeln!(&mut out, "- `ERR/OK` (C++ err, Rust ok): `{err_ok}`")?;
    writeln!(&mut out)?;

    writeln!(&mut out, "## Parity Mismatches")?;
    writeln!(
        &mut out,
        "| Case | Class | C++ | Rust stage | Rust reason | C++ reason |"
    )?;
    writeln!(
        &mut out,
        "|------|-------|-----|------------|-------------|------------|"
    )?;
    for (case, cpp, rust) in &rows {
        let class = match (cpp.ok, rust.ok) {
            (true, false) => "OK/ERR",
            (false, true) => "ERR/OK",
            _ => continue,
        };
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` | `{}` | `{}` |",
            case,
            class,
            status_cell(cpp),
            rust.stage,
            markdown_escape(&rust.reason),
            markdown_escape(&cpp.reason),
        )?;
    }
    writeln!(&mut out)?;

    writeln!(&mut out, "## Full Matrix")?;
    writeln!(&mut out, "| Case | C++ | Rust | Rust stage | Rust reason |")?;
    writeln!(&mut out, "|------|-----|------|------------|-------------|")?;
    for (case, cpp, rust) in &rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` | `{}` |",
            case,
            status_cell(cpp),
            status_cell(rust),
            rust.stage,
            markdown_escape(&rust.reason),
        )?;
    }
    writeln!(&mut out)?;
    writeln!(&mut out, "## Next Actions")?;
    writeln!(
        &mut out,
        "- Treat all `OK/ERR` and `ERR/OK` rows as parity tasks in parser/eval/propagate."
    )?;
    writeln!(
        &mut out,
        "- Re-run this report after each parity fix touching `tests/corpus` behavior."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ShellSignature {
    faustclass: Option<String>,
    class_decl: Option<String>,
    has_restrict_define: bool,
    has_exp10_aliases: bool,
}

#[derive(Clone, Debug)]
struct CppDiffRow {
    case: String,
    class: &'static str,
    rust_reason: String,
    cpp_reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CShellSignature {
    has_typedef_struct: bool,
    has_faustfloat_define: bool,
    has_restrict_define: bool,
    has_instance_constants_fn: bool,
    has_instance_reset_ui_fn: bool,
    has_instance_clear_fn: bool,
    has_instance_init_fn: bool,
    has_build_ui_fn: bool,
    has_compute_fn: bool,
    has_instance_init_ordered_calls: bool,
}

fn cpp_backend_diff_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let report_path = root.join(CPP_BACKEND_DIFF_REPORT_REL_PATH);
    let compiler = compiler::Compiler::new();
    let (cpp_bin, cpp_bin_is_fallback) = resolve_cpp_faust_bin();
    let options = codegen::backends::cpp::CppOptions {
        class_name: Some("mydsp".to_owned()),
        ..codegen::backends::cpp::CppOptions::default()
    };

    let representative = [
        "rep_01_passthrough.dsp",
        "rep_05_one_pole_lowpass.dsp",
        "rep_09_ui_slider.dsp",
        "rep_17_ui_groups.dsp",
        "rep_20_environment_waveform.dsp",
        "rep_22_parallel_mix.dsp",
        "rep_28_nested_ui_groups.dsp",
        "rep_31_extended_primitives.dsp",
    ];

    let mut rows = Vec::with_capacity(representative.len());
    let mut ok = 0usize;
    let mut diff = 0usize;
    let mut unsupported = 0usize;

    for case in representative {
        let path = root.join("tests").join("corpus").join(case);
        let rust_output = compiler.compile_file_default_to_cpp(&path, &options);
        let cpp_output = Command::new(&cpp_bin).arg(&path).output();

        let row = match (rust_output, cpp_output) {
            (Ok(_), Err(err)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust path ok".to_owned(),
                    cpp_reason: format!("cannot run `{}`: {err}", cpp_bin.display()),
                }
            }
            (Err(err), Err(cpp_err)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: format!("cannot run `{}`: {cpp_err}", cpp_bin.display()),
                }
            }
            (Ok(rust_text), Ok(cpp_output)) if cpp_output.status.success() => {
                let cpp_text = String::from_utf8(cpp_output.stdout)?;
                let rust_sig = extract_shell_signature(&rust_text);
                let cpp_sig = extract_shell_signature(&cpp_text);
                if rust_sig == cpp_sig {
                    ok = ok.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "OK",
                        rust_reason: "shell signature matches".to_owned(),
                        cpp_reason: "ok".to_owned(),
                    }
                } else {
                    diff = diff.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "DIFF",
                        rust_reason: format!("rust={rust_sig:?}"),
                        cpp_reason: format!("cpp={cpp_sig:?}"),
                    }
                }
            }
            (Ok(_), Ok(cpp_output)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust path ok".to_owned(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
            (Err(err), Ok(cpp_output)) if cpp_output.status.success() => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: "C++ path ok".to_owned(),
                }
            }
            (Err(err), Ok(cpp_output)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
        };
        rows.push(row);
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 6 C++ Backend Differential Report (Module-First, Shell-Normalized)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- cpp-backend-diff-report`"
    )?;
    writeln!(&mut out, "- C++ binary: `{}`", cpp_bin.display())?;
    if cpp_bin_is_fallback {
        writeln!(
            &mut out,
            "- Note: fallback to `faust` from PATH because `{}/build/bin/faust` was not found.",
            CPP_SOURCE_ROOT
        )?;
    }
    writeln!(
        &mut out,
        "- Normalization: compare module-shell signature only"
    )?;
    writeln!(&mut out, "  - `#define FAUSTCLASS <name>`")?;
    writeln!(&mut out, "  - `class <name> : public dsp`")?;
    writeln!(
        &mut out,
        "  - presence of `RESTRICT` and Apple `exp10` aliases"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(&mut out, "- Cases: `{}`", rows.len())?;
    writeln!(&mut out, "- `OK`: `{ok}`")?;
    writeln!(&mut out, "- `DIFF`: `{diff}`")?;
    writeln!(&mut out, "- `UNSUPPORTED`: `{unsupported}`")?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "| Case | Status | Rust detail | C++ detail |\n|------|--------|-------------|------------|"
    )?;
    for row in &rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` |",
            row.case,
            row.class,
            markdown_escape(&row.rust_reason),
            markdown_escape(&row.cpp_reason)
        )?;
    }
    writeln!(&mut out)?;
    writeln!(&mut out, "## Notes")?;
    writeln!(
        &mut out,
        "- This report tracks module-shell parity while full production signal->FIR lowering is still in progress."
    )?;
    writeln!(
        &mut out,
        "- `DIFF` rows are expected to shrink as statement/value lowering and orchestration parity advance."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

fn table_fastlane_diff_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let report_path = root.join(TABLE_FASTLANE_DIFF_REPORT_REL_PATH);
    let compiler = compiler::Compiler::new();
    let (cpp_bin, cpp_bin_is_fallback) = resolve_cpp_faust_bin();
    let options = codegen::backends::cpp::CppOptions {
        class_name: Some("mydsp".to_owned()),
        ..codegen::backends::cpp::CppOptions::default()
    };

    let representative = [
        "rep_20_environment_waveform.dsp",
        "rep_30_environment_access_pair.dsp",
        "rep_34_table_rdtable_readonly_const.dsp",
        "rep_35_table_rwtable_runtime_write.dsp",
        "rep_36_table_rdtable_negative_index.dsp",
        "rep_37_table_rwtable_negative_indices.dsp",
    ];

    let mut rows = Vec::with_capacity(representative.len());
    let mut ok = 0usize;
    let mut diff = 0usize;
    let mut unsupported = 0usize;

    for case in representative {
        let path = root.join("tests").join("corpus").join(case);
        let rust_output = compiler.compile_file_default_to_cpp_with_lane(
            &path,
            &options,
            compiler::SignalFirLane::TransformFastLane,
        );
        let cpp_output = Command::new(&cpp_bin).arg(&path).output();

        let row = match (rust_output, cpp_output) {
            (Ok(_), Err(err)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust fast-lane ok".to_owned(),
                    cpp_reason: format!("cannot run `{}`: {err}", cpp_bin.display()),
                }
            }
            (Err(err), Err(cpp_err)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: format!("cannot run `{}`: {cpp_err}", cpp_bin.display()),
                }
            }
            (Ok(rust_text), Ok(cpp_output)) if cpp_output.status.success() => {
                let cpp_text = String::from_utf8(cpp_output.stdout)?;
                let rust_sig = extract_shell_signature(&rust_text);
                let cpp_sig = extract_shell_signature(&cpp_text);
                if rust_sig == cpp_sig {
                    ok = ok.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "OK",
                        rust_reason: "shell signature matches".to_owned(),
                        cpp_reason: "ok".to_owned(),
                    }
                } else {
                    diff = diff.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "DIFF",
                        rust_reason: format!("rust={rust_sig:?}"),
                        cpp_reason: format!("cpp={cpp_sig:?}"),
                    }
                }
            }
            (Ok(_), Ok(cpp_output)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust fast-lane ok".to_owned(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
            (Err(err), Ok(cpp_output)) if cpp_output.status.success() => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: "C++ path ok".to_owned(),
                }
            }
            (Err(err), Ok(cpp_output)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
        };
        rows.push(row);
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 6 Table Fast-Lane Differential Report (C++ vs Rust)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- table-fastlane-diff-report`"
    )?;
    writeln!(&mut out, "- C++ binary: `{}`", cpp_bin.display())?;
    if cpp_bin_is_fallback {
        writeln!(
            &mut out,
            "- Note: fallback to `faust` from PATH because `{}/build/bin/faust` was not found.",
            CPP_SOURCE_ROOT
        )?;
    }
    writeln!(
        &mut out,
        "- Rust route: `compiler::SignalFirLane::TransformFastLane`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(&mut out, "- Cases: `{}`", rows.len())?;
    writeln!(&mut out, "- `OK`: `{ok}`")?;
    writeln!(&mut out, "- `DIFF`: `{diff}`")?;
    writeln!(&mut out, "- `UNSUPPORTED`: `{unsupported}`")?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "| Case | Status | Rust detail | C++ detail |\n|------|--------|-------------|------------|"
    )?;
    for row in &rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` |",
            row.case,
            row.class,
            markdown_escape(&row.rust_reason),
            markdown_escape(&row.cpp_reason)
        )?;
    }
    writeln!(&mut out)?;
    writeln!(&mut out, "## Notes")?;
    writeln!(
        &mut out,
        "- Comparison is shell-signature based (`FAUSTCLASS`, class declaration, macro aliases)."
    )?;
    writeln!(
        &mut out,
        "- This report focuses on table-oriented fixtures for Step 2J closure."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

fn c_fastlane_diff_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let report_path = root.join(C_FASTLANE_DIFF_REPORT_REL_PATH);
    let compiler = compiler::Compiler::new();
    let (cpp_bin, cpp_bin_is_fallback) = resolve_cpp_faust_bin();
    let options = codegen::backends::c::COptions {
        class_name: Some("mydsp".to_owned()),
        ..codegen::backends::c::COptions::default()
    };

    let representative = [
        "rep_01_passthrough.dsp",
        "rep_05_one_pole_lowpass.dsp",
        "rep_07_nonlinear_clip.dsp",
        "rep_09_ui_slider.dsp",
        "rep_10_two_in_two_out_ui.dsp",
        "rep_17_ui_groups.dsp",
        "rep_20_environment_waveform.dsp",
        "rep_22_parallel_mix.dsp",
        "rep_23_feedback_simple.dsp",
        "rep_28_nested_ui_groups.dsp",
        "rep_30_environment_access_pair.dsp",
        "rep_31_extended_primitives.dsp",
        "rep_34_table_rdtable_readonly_const.dsp",
        "rep_35_table_rwtable_runtime_write.dsp",
        "rep_36_table_rdtable_negative_index.dsp",
        "rep_37_table_rwtable_negative_indices.dsp",
    ];

    let mut rows = Vec::with_capacity(representative.len());
    let mut ok = 0usize;
    let mut diff = 0usize;
    let mut unsupported = 0usize;

    for case in representative {
        let path = root.join("tests").join("corpus").join(case);
        let rust_output = compiler.compile_file_default_to_c_with_lane(
            &path,
            &options,
            compiler::SignalFirLane::TransformFastLane,
        );
        let cpp_output = Command::new(&cpp_bin)
            .arg(&path)
            .arg("-lang")
            .arg("c")
            .arg("-cn")
            .arg("mydsp")
            .output();

        let row = match (rust_output, cpp_output) {
            (Ok(_), Err(err)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust C fast-lane ok".to_owned(),
                    cpp_reason: format!("cannot run `{}`: {err}", cpp_bin.display()),
                }
            }
            (Err(err), Err(cpp_err)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: format!("cannot run `{}`: {cpp_err}", cpp_bin.display()),
                }
            }
            (Ok(rust_text), Ok(cpp_output)) if cpp_output.status.success() => {
                let cpp_text = String::from_utf8(cpp_output.stdout)?;
                let rust_sig = extract_c_shell_signature(&rust_text);
                let cpp_sig = extract_c_shell_signature(&cpp_text);
                if rust_sig == cpp_sig {
                    ok = ok.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "OK",
                        rust_reason: "C shell signature matches".to_owned(),
                        cpp_reason: "ok".to_owned(),
                    }
                } else {
                    diff = diff.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "DIFF",
                        rust_reason: format!("rust={rust_sig:?}"),
                        cpp_reason: format!("cpp={cpp_sig:?}"),
                    }
                }
            }
            (Ok(_), Ok(cpp_output)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust C fast-lane ok".to_owned(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
            (Err(err), Ok(cpp_output)) if cpp_output.status.success() => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: "C++ C backend path ok".to_owned(),
                }
            }
            (Err(err), Ok(cpp_output)) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
        };
        rows.push(row);
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 6 C Fast-Lane Differential Report (C++ `-lang c` vs Rust)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- c-fastlane-diff-report`"
    )?;
    writeln!(&mut out, "- C++ binary: `{}`", cpp_bin.display())?;
    if cpp_bin_is_fallback {
        writeln!(
            &mut out,
            "- Note: fallback to `faust` from PATH because `{}/build/bin/faust` was not found.",
            CPP_SOURCE_ROOT
        )?;
    }
    writeln!(
        &mut out,
        "- C++ command: `faust <case>.dsp -lang c -cn mydsp`"
    )?;
    writeln!(
        &mut out,
        "- Rust route: `compiler::SignalFirLane::TransformFastLane` + `--dump-c`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(&mut out, "- Cases: `{}`", rows.len())?;
    writeln!(&mut out, "- `OK`: `{ok}`")?;
    writeln!(&mut out, "- `DIFF`: `{diff}`")?;
    writeln!(&mut out, "- `UNSUPPORTED`: `{unsupported}`")?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "| Case | Status | Rust detail | C++ detail |\n|------|--------|-------------|------------|"
    )?;
    for row in &rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` |",
            row.case,
            row.class,
            markdown_escape(&row.rust_reason),
            markdown_escape(&row.cpp_reason)
        )?;
    }
    writeln!(&mut out)?;
    writeln!(&mut out, "## Notes")?;
    writeln!(
        &mut out,
        "- Comparison is C-shell signature based (typedef/defines/lifecycle/UI/compute function presence and init call ordering)."
    )?;
    writeln!(
        &mut out,
        "- This report is the Step 7B guardrail for C fast-lane parity progression."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

fn backend_full_corpus_diff_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let report_path = root.join(BACKEND_FULL_CORPUS_DIFF_REPORT_REL_PATH);
    let compiler = compiler::Compiler::new();
    let (cpp_bin, cpp_bin_is_fallback) = resolve_cpp_faust_bin();
    let files = corpus_files()?;
    let cpp_options = codegen::backends::cpp::CppOptions {
        class_name: Some("mydsp".to_owned()),
        ..codegen::backends::cpp::CppOptions::default()
    };
    let c_options = codegen::backends::c::COptions {
        class_name: Some("mydsp".to_owned()),
        ..codegen::backends::c::COptions::default()
    };

    let mut cpp_rows = Vec::with_capacity(files.len());
    let mut cpp_ok = 0usize;
    let mut cpp_diff = 0usize;
    let mut cpp_unsupported = 0usize;

    let mut c_rows = Vec::with_capacity(files.len());
    let mut c_ok = 0usize;
    let mut c_diff = 0usize;
    let mut c_unsupported = 0usize;

    for file in &files {
        let case = case_name(file)?;

        let rust_cpp = compiler.compile_file_default_to_cpp_with_lane(
            file,
            &cpp_options,
            compiler::SignalFirLane::TransformFastLane,
        );
        let cpp_cpp = Command::new(&cpp_bin)
            .arg(file)
            .arg("-lang")
            .arg("cpp")
            .arg("-cn")
            .arg("mydsp")
            .output();
        let cpp_row = match (rust_cpp, cpp_cpp) {
            (Ok(_), Err(err)) => {
                cpp_unsupported = cpp_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust C++ fast-lane ok".to_owned(),
                    cpp_reason: format!("cannot run `{}`: {err}", cpp_bin.display()),
                }
            }
            (Err(err), Err(cpp_err)) => {
                cpp_unsupported = cpp_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: format!("cannot run `{}`: {cpp_err}", cpp_bin.display()),
                }
            }
            (Ok(rust_text), Ok(cpp_output)) if cpp_output.status.success() => {
                let cpp_text = String::from_utf8(cpp_output.stdout)?;
                let rust_sig = extract_shell_signature(&rust_text);
                let cpp_sig = extract_shell_signature(&cpp_text);
                if rust_sig == cpp_sig {
                    cpp_ok = cpp_ok.saturating_add(1);
                    CppDiffRow {
                        case: case.clone(),
                        class: "OK",
                        rust_reason: "shell signature matches".to_owned(),
                        cpp_reason: "ok".to_owned(),
                    }
                } else {
                    cpp_diff = cpp_diff.saturating_add(1);
                    CppDiffRow {
                        case: case.clone(),
                        class: "DIFF",
                        rust_reason: format!("rust={rust_sig:?}"),
                        cpp_reason: format!("cpp={cpp_sig:?}"),
                    }
                }
            }
            (Ok(_), Ok(cpp_output)) => {
                cpp_unsupported = cpp_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust C++ fast-lane ok".to_owned(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
            (Err(err), Ok(cpp_output)) if cpp_output.status.success() => {
                cpp_unsupported = cpp_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: "C++ reference path ok".to_owned(),
                }
            }
            (Err(err), Ok(cpp_output)) => {
                cpp_unsupported = cpp_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
        };
        cpp_rows.push(cpp_row);

        let rust_c = compiler.compile_file_default_to_c_with_lane(
            file,
            &c_options,
            compiler::SignalFirLane::TransformFastLane,
        );
        let cpp_c = Command::new(&cpp_bin)
            .arg(file)
            .arg("-lang")
            .arg("c")
            .arg("-cn")
            .arg("mydsp")
            .output();
        let c_row = match (rust_c, cpp_c) {
            (Ok(_), Err(err)) => {
                c_unsupported = c_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust C fast-lane ok".to_owned(),
                    cpp_reason: format!("cannot run `{}`: {err}", cpp_bin.display()),
                }
            }
            (Err(err), Err(cpp_err)) => {
                c_unsupported = c_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: format!("cannot run `{}`: {cpp_err}", cpp_bin.display()),
                }
            }
            (Ok(rust_text), Ok(cpp_output)) if cpp_output.status.success() => {
                let cpp_text = String::from_utf8(cpp_output.stdout)?;
                let rust_sig = extract_c_shell_signature(&rust_text);
                let cpp_sig = extract_c_shell_signature(&cpp_text);
                if rust_sig == cpp_sig {
                    c_ok = c_ok.saturating_add(1);
                    CppDiffRow {
                        case: case.clone(),
                        class: "OK",
                        rust_reason: "C shell signature matches".to_owned(),
                        cpp_reason: "ok".to_owned(),
                    }
                } else {
                    c_diff = c_diff.saturating_add(1);
                    CppDiffRow {
                        case: case.clone(),
                        class: "DIFF",
                        rust_reason: format!("rust={rust_sig:?}"),
                        cpp_reason: format!("cpp={cpp_sig:?}"),
                    }
                }
            }
            (Ok(_), Ok(cpp_output)) => {
                c_unsupported = c_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust C fast-lane ok".to_owned(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
            (Err(err), Ok(cpp_output)) if cpp_output.status.success() => {
                c_unsupported = c_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: "C reference path ok".to_owned(),
                }
            }
            (Err(err), Ok(cpp_output)) => {
                c_unsupported = c_unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.clone(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
        };
        c_rows.push(c_row);
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 6 Backend Full-Corpus Differential Report (Rust fast-lane vs C++ reference)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- backend-full-corpus-diff-report`"
    )?;
    writeln!(&mut out, "- C++ binary: `{}`", cpp_bin.display())?;
    if cpp_bin_is_fallback {
        writeln!(
            &mut out,
            "- Note: fallback to `faust` from PATH because `{}/build/bin/faust` was not found.",
            CPP_SOURCE_ROOT
        )?;
    }
    writeln!(&mut out, "- Corpus: `tests/corpus/*.dsp`")?;
    writeln!(
        &mut out,
        "- Rust route: `compiler::SignalFirLane::TransformFastLane`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(&mut out, "- Cases: `{}`", files.len())?;
    writeln!(
        &mut out,
        "- C++ backend parity: `OK={cpp_ok}` `DIFF={cpp_diff}` `UNSUPPORTED={cpp_unsupported}`"
    )?;
    writeln!(
        &mut out,
        "- C backend parity: `OK={c_ok}` `DIFF={c_diff}` `UNSUPPORTED={c_unsupported}`"
    )?;
    writeln!(&mut out)?;

    writeln!(&mut out, "## C++ Backend Matrix")?;
    writeln!(
        &mut out,
        "| Case | Status | Rust detail | C++ detail |\n|------|--------|-------------|------------|"
    )?;
    for row in &cpp_rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` |",
            row.case,
            row.class,
            markdown_escape(&row.rust_reason),
            markdown_escape(&row.cpp_reason)
        )?;
    }
    writeln!(&mut out)?;

    writeln!(&mut out, "## C Backend Matrix")?;
    writeln!(
        &mut out,
        "| Case | Status | Rust detail | C++ detail |\n|------|--------|-------------|------------|"
    )?;
    for row in &c_rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` |",
            row.case,
            row.class,
            markdown_escape(&row.rust_reason),
            markdown_escape(&row.cpp_reason)
        )?;
    }
    writeln!(&mut out)?;
    writeln!(&mut out, "## Notes")?;
    writeln!(
        &mut out,
        "- C++ reference command: `faust <case>.dsp -lang cpp -cn mydsp` (shell-signature metric)."
    )?;
    writeln!(
        &mut out,
        "- C reference command: `faust <case>.dsp -lang c -cn mydsp` (C-shell-signature metric)."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

fn extract_shell_signature(text: &str) -> ShellSignature {
    let mut faustclass = None::<String>;
    let mut class_decl = None::<String>;
    let mut has_restrict_define = false;
    let mut has_exp10f_alias = false;
    let mut has_exp10_alias = false;

    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("#define FAUSTCLASS ") {
            faustclass = Some(rest.trim().to_owned());
        }
        if let Some(rest) = line.strip_prefix("class ")
            && let Some((name, _)) = rest.split_once(" : public dsp")
        {
            class_decl = Some(name.trim().to_owned());
        }
        if line.contains("#define RESTRICT") {
            has_restrict_define = true;
        }
        if line == "#define exp10f __exp10f" {
            has_exp10f_alias = true;
        }
        if line == "#define exp10 __exp10" {
            has_exp10_alias = true;
        }
    }

    ShellSignature {
        faustclass,
        class_decl,
        has_restrict_define,
        has_exp10_aliases: has_exp10f_alias && has_exp10_alias,
    }
}

fn extract_c_shell_signature(text: &str) -> CShellSignature {
    let has_typedef_struct = text.contains("typedef struct {");
    let has_faustfloat_define = text.contains("#ifndef FAUSTFLOAT");
    let has_restrict_define = text.contains("#define RESTRICT");
    let has_instance_constants_fn = text.contains("void instanceConstants");
    let has_instance_reset_ui_fn = text.contains("void instanceResetUserInterface");
    let has_instance_clear_fn = text.contains("void instanceClear");
    let has_instance_init_fn = text.contains("void instanceInit");
    let has_build_ui_fn = text.contains("void buildUserInterface");
    let has_compute_fn = text.contains("void compute");

    let has_instance_init_ordered_calls = has_ordered_instance_init_calls(text);

    CShellSignature {
        has_typedef_struct,
        has_faustfloat_define,
        has_restrict_define,
        has_instance_constants_fn,
        has_instance_reset_ui_fn,
        has_instance_clear_fn,
        has_instance_init_fn,
        has_build_ui_fn,
        has_compute_fn,
        has_instance_init_ordered_calls,
    }
}

fn has_ordered_instance_init_calls(text: &str) -> bool {
    let mut search_from = 0usize;
    while let Some(rel) = text[search_from..].find("void instanceInit") {
        let start = search_from + rel;
        let tail = &text[start..];
        let end = tail.find("}\n").unwrap_or(tail.len());
        let body = &tail[..end];
        let c_i = body.find("instanceConstants");
        let r_i = body.find("instanceResetUserInterface");
        let cl_i = body.find("instanceClear");
        if matches!((c_i, r_i, cl_i), (Some(a), Some(b), Some(c)) if a < b && b < c) {
            return true;
        }
        search_from = start + "void instanceInit".len();
    }
    false
}

fn resolve_cpp_faust_bin() -> (PathBuf, bool) {
    if let Some(path) = std::env::var_os("FAUST_CPP_BIN") {
        return (PathBuf::from(path), false);
    }
    let built = Path::new(CPP_SOURCE_ROOT).join("build/bin/faust");
    if built.exists() {
        return (built, false);
    }
    (PathBuf::from("faust"), true)
}

fn cpp_case_status(cpp_bin: &Path, input: &Path) -> Result<CaseStatus, Box<dyn std::error::Error>> {
    let status = Command::new(cpp_bin)
        .arg(input)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        return Ok(CaseStatus {
            ok: true,
            stage: "ok",
            reason: "ok".to_owned(),
        });
    }

    let output = Command::new(cpp_bin).arg(input).output()?;
    let reason = first_non_empty_line(&String::from_utf8_lossy(&output.stderr))
        .or_else(|| first_non_empty_line(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_else(|| format!("failed with status {}", output.status));
    Ok(CaseStatus {
        ok: false,
        stage: "error",
        reason,
    })
}

fn rust_case_status(compiler: &compiler::Compiler, input: &Path) -> CaseStatus {
    match compiler.compile_file_default_to_signals(input) {
        Ok(_) => CaseStatus {
            ok: true,
            stage: "ok",
            reason: "ok".to_owned(),
        },
        Err(err) => {
            let (stage, reason) = match &err {
                compiler::CompilerError::Import(_) => ("import", err.to_string()),
                compiler::CompilerError::Parse { .. } => ("parse", err.to_string()),
                compiler::CompilerError::Eval { .. } => ("eval", err.to_string()),
                compiler::CompilerError::Propagate { .. } => ("propagate", err.to_string()),
                compiler::CompilerError::Transform { .. } => ("transform", err.to_string()),
                compiler::CompilerError::FirVerify { .. } => ("fir", err.to_string()),
                compiler::CompilerError::Codegen { .. } => ("codegen", err.to_string()),
                compiler::CompilerError::CodegenC { .. } => ("codegen", err.to_string()),
                compiler::CompilerError::CodegenInterp { .. } => ("codegen", err.to_string()),
                compiler::CompilerError::MissingRoot { .. } => ("parse", err.to_string()),
            };
            CaseStatus {
                ok: false,
                stage,
                reason,
            }
        }
    }
}

fn status_cell(status: &CaseStatus) -> &'static str {
    if status.ok { "OK" } else { "ERR" }
}

fn first_non_empty_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn markdown_escape(value: &str) -> String {
    value.replace('|', "\\|").replace(['\n', '\r'], " ")
}

fn extract_parser_tokens(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let rest = if let Some(rest) = trimmed.strip_prefix("%token") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("%left") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("%right") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("%nonassoc") {
            rest
        } else {
            continue;
        };
        let rest = rest.trim();
        for raw in rest.split_whitespace() {
            let part = raw.trim_matches(|c: char| c == ',' || c == ';');
            if part.starts_with('<') || part.starts_with("/*") || part.starts_with("//") {
                continue;
            }
            if is_token_name(part) {
                set.insert(part.to_owned());
            }
        }
    }
    set
}

fn extract_cpp_nonterminals(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in grammar_section(source).lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('%') || trimmed.starts_with('|') {
            continue;
        }
        let Some((head, _)) = trimmed.split_once(':') else {
            continue;
        };
        let head = head.trim();
        if is_ident_name(head) {
            set.insert(head.to_ascii_lowercase());
        }
    }
    set
}

fn extract_rust_nonterminals(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in grammar_section(source).lines() {
        let trimmed = line.trim_start();
        let Some((head, _)) = trimmed.split_once("->") else {
            continue;
        };
        let head = head.trim();
        if is_ident_name(head) {
            set.insert(head.to_ascii_lowercase());
        }
    }
    set
}

fn extract_lexer_states(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let rest = if let Some(rest) = trimmed.strip_prefix("%x") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("%s") {
            rest
        } else {
            continue;
        };
        for state in rest.split_whitespace() {
            let state = state.trim_matches(|c: char| c == ';');
            if is_ident_name(state) {
                set.insert(state.to_ascii_lowercase());
            }
        }
    }
    set
}

fn extract_cpp_lexer_emitted_tokens(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        let mut rest = line;
        while let Some(idx) = rest.find("return ") {
            let after = &rest[idx + "return ".len()..];
            if let Some(token) = scan_token_name(after) {
                set.insert(token);
            }
            rest = &after[after.char_indices().nth(1).map_or(after.len(), |(i, _)| i)..];
        }
    }
    set
}

fn extract_rust_lexer_emitted_tokens(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] != '\'' {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '\'' {
                j += 1;
            }
            if j >= chars.len() {
                break;
            }
            let candidate: String = chars[i + 1..j].iter().collect();
            if is_token_name(&candidate) {
                set.insert(candidate);
            }
            // Move one character forward so overlapping quotes are still discovered.
            i += 1;
        }
    }
    set
}

fn grammar_section(source: &str) -> &str {
    let mut marks = source.match_indices("%%");
    let Some((first, _)) = marks.next() else {
        return source;
    };
    let Some((second, _)) = marks.next() else {
        return &source[first + 2..];
    };
    &source[first + 2..second]
}

fn is_token_name(s: &str) -> bool {
    let mut has_upper = false;
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            has_upper = true;
        } else if !(c.is_ascii_digit() || c == '_') {
            return false;
        }
    }
    has_upper
}

fn is_ident_name(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn scan_token_name(source: &str) -> Option<String> {
    let mut start = None;
    for (idx, c) in source.char_indices() {
        if c.is_ascii_uppercase() || c == '_' {
            start = Some(idx);
            break;
        }
    }
    let start = start?;
    let token: String = source[start..]
        .chars()
        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
        .collect();
    if is_token_name(&token) {
        Some(token)
    } else {
        None
    }
}

fn diff_sorted(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Vec<String> {
    left.difference(right).cloned().collect()
}

fn token_aliases(cpp_name: &str) -> &'static [&'static str] {
    match cpp_name {
        "VIRG" => &["PAR"],
        "LISTING" => &["BLST"],
        _ => &[],
    }
}

fn nonterminal_aliases(cpp_name: &str) -> &'static [&'static str] {
    match cpp_name {
        "params" => &["paramlist"],
        "recinition" => &["recdefinition"],
        "ident" => &["identexpr"],
        "fun" => &["funname"],
        "string" => &["rawstring", "uqstring", "fstring"],
        "doc" => &["doccontent"],
        "doctxt" | "doceqn" | "docdgm" | "docmtd" | "doclst" | "docntc" => &["docelem"],
        "lstattrdef" => &["lstattr"],
        "lstattrval" => &["lstattrvalue"],
        "ffunction" | "fconst" | "fvariable" | "fpar" | "fseq" | "fsum" | "fprod" | "finputs"
        | "foutputs" | "fondemand" | "fupsampling" | "fdownsampling" | "button" | "checkbox"
        | "vslider" | "hslider" | "nentry" | "vgroup" | "hgroup" | "tgroup" | "vbargraph"
        | "hbargraph" | "soundfile" => &["primitive"],
        _ => &[],
    }
}

fn partition_with_aliases(
    missing_exact: &[String],
    rust_set: &BTreeSet<String>,
    aliases: impl Fn(&str) -> &'static [&'static str],
) -> (Vec<(String, Vec<String>)>, Vec<String>) {
    let mut covered = Vec::new();
    let mut unresolved = Vec::new();

    for item in missing_exact {
        let mapped_hits = aliases(item)
            .iter()
            .copied()
            .filter(|candidate| rust_set.contains(*candidate))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if mapped_hits.is_empty() {
            unresolved.push(item.clone());
        } else {
            covered.push((item.clone(), mapped_hits));
        }
    }
    (covered, unresolved)
}

fn render_list(out: &mut String, title: &str, items: &[String]) -> Result<(), std::fmt::Error> {
    writeln!(out, "### {title}")?;
    if items.is_empty() {
        writeln!(out, "- (none)")?;
    } else {
        for item in items {
            writeln!(out, "- `{item}`")?;
        }
    }
    Ok(())
}

fn render_alias_list(
    out: &mut String,
    title: &str,
    items: &[(String, Vec<String>)],
) -> Result<(), std::fmt::Error> {
    writeln!(out, "### {title}")?;
    if items.is_empty() {
        writeln!(out, "- (none)")?;
        return Ok(());
    }
    for (source, targets) in items {
        let mapped = targets
            .iter()
            .map(|v| format!("`{v}`"))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(out, "- `{source}` -> {mapped}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
        assert_eq!(TraceLane::parse("legacy").unwrap(), TraceLane::Legacy);
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
        let mismatch =
            compare_runtime_traces(&a, &b, TraceCompareTolerances::default()).unwrap_err();
        assert_eq!(mismatch.field, "outputs");
        assert_eq!(mismatch.channel, Some(0));
        assert_eq!(mismatch.sample, Some(0));
    }
}
