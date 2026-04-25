//! `faust-rs` CLI entry point.
//!
//! # Role
//! - Front-end command-line interface for the workspace compiler facade
//!   ([`compiler::Compiler`]).
//! - Exposes parse/dump/compile operations used by local parity workflows and
//!   day-to-day debugging.
//!
//! # Supported modes
//! - parse diagnostics (`--parse`)
//! - box/signal/FIR dumps (`--dump-box`, `--dump-sig`, `--dump-fir`, `--dump-fir-verify`)
//! - backend text emission (`--dump-c`, `--dump-cpp`, `-lang`)
//! - golden snapshot output (`--golden`)
//!
//! # Compatibility note
//! - Legacy flag forms are normalized to `clap` options (e.g. `-lang`) to keep
//!   script compatibility while converging on typed CLI parsing.

use boxes::dump_box;
use clap::{ArgAction, Parser, ValueEnum};
use codegen::backends::c::COptions;
use codegen::backends::c::generate_c_module;
use codegen::backends::cpp::CppOptions;
use codegen::backends::cpp::generate_cpp_module;
use codegen::backends::cranelift::{
    CraneliftOptions, StructFieldKind, diagnose_cranelift_compute_subset_gap,
    generate_cranelift_module,
};
use codegen::backends::interp::{
    FbcCppOptions, InterpOptions, generate_cpp_from_fbc, generate_interp_module, read_fbc,
    write_fbc,
};
use codegen::backends::wasm::{WasmOptions, generate_wasm_module};
use codegen::fixtures::backend_test_fixtures;
use compiler::{
    Compiler, CompilerError, FirVerifyOptions, RealType, SignalFirLane,
    compile_options_json_string,
    enrobage::{EnrobageOptions, wrap_cpp_with_architecture},
    golden_snapshot_from_file,
};
use errors::{DiagnosticBundle, LabelStyle, Severity, Stage};
use fir::{checker::verify_fir_module, dump_fir};
use serde_json::json;
use signals::dump_sig_readable;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
/// Code generation language/backend selected from the CLI.
enum CliLang {
    #[value(alias = "c99")]
    C,
    #[value(alias = "cxx", alias = "c++")]
    Cpp,
    Fir,
    #[value(alias = "interp-fbc")]
    Interp,
    #[value(alias = "clif")]
    Cranelift,
    Wasm,
    #[value(alias = "wat")]
    Wast,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
/// Structured error rendering format for CLI diagnostics.
enum ErrorFormat {
    #[default]
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
/// Diagnostic verbosity level for CLI rendering.
enum ErrorVerbosity {
    #[default]
    Standard,
    Debug,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
/// Signal->FIR lane selected from the CLI.
enum CliSignalFirLane {
    #[default]
    Fast,
}

impl CliSignalFirLane {
    /// Converts the CLI lane selection into the internal [`SignalFirLane`] used
    /// by the compiler library.
    fn into_compiler_lane(self) -> SignalFirLane {
        match self {
            Self::Fast => SignalFirLane::TransformFastLane,
        }
    }
}

/// Tracks elapsed time across compilation phases and enforces a global timeout.
struct CompilationTimer {
    /// Absolute start time of the compilation run.
    start: Instant,
    /// Maximum total compilation duration.  Exceeded → `process::exit(1)`.
    timeout: Duration,
    /// When `true`, the total timing is printed to stderr. Internal compiler
    /// phase timings are reported through [`Compiler::with_timing_sink`].
    display: bool,
}

impl CompilationTimer {
    /// Creates a new timer. `timeout_secs` sets the hard limit; `display`
    /// controls whether the total timing is printed to stderr.
    fn new(timeout_secs: u64, display: bool) -> Self {
        let now = Instant::now();
        Self {
            start: now,
            timeout: Duration::from_secs(timeout_secs),
            display,
        }
    }

    /// Mark the end of a compilation phase and abort if the global timeout has
    /// been exceeded.
    fn phase(&mut self, name: &str) {
        let now = Instant::now();
        let total_elapsed = now.duration_since(self.start);
        if total_elapsed > self.timeout {
            eprintln!(
                "ERROR: compilation timeout ({:.1}s > {}s limit) after phase '{name}'",
                total_elapsed.as_secs_f64(),
                self.timeout.as_secs(),
            );
            std::process::exit(1);
        }
    }

    /// Print the total compilation time (only when `--compilation-time` is active).
    fn total(&self) {
        if self.display {
            let elapsed = self.start.elapsed();
            eprintln!("[total] {:.1}ms", elapsed.as_secs_f64() * 1000.0);
        }
    }
}

/// Command-line arguments for the compiler binary.
///
/// Legacy mode flags are intentionally kept (`--parse`, `--dump-box`, etc.)
/// to avoid breaking existing scripts while benefiting from robust `clap`
/// parsing and help generation.
#[derive(Debug, Parser)]
#[command(name = "faust-rs", disable_version_flag = true)]
/// Parsed CLI arguments for the `compiler` binary.
struct CliArgs {
    /// Generate the golden snapshot output for one DSP file.
    #[arg(long, action = ArgAction::SetTrue)]
    golden: bool,
    /// Parse one DSP file and print parser status.
    #[arg(long, action = ArgAction::SetTrue)]
    parse: bool,
    /// Parse and dump box IR.
    #[arg(long = "dump-box", action = ArgAction::SetTrue)]
    dump_box: bool,
    /// Compile to signals and dump signal IR.
    #[arg(long = "dump-sig", action = ArgAction::SetTrue)]
    dump_sig: bool,
    /// Compile to C++ and print generated code.
    #[arg(long = "dump-cpp", action = ArgAction::SetTrue)]
    dump_cpp: bool,
    /// Read interpreter `.fbc` text and emit self-contained native C++.
    #[arg(long = "dump-cpp-from-fbc", action = ArgAction::SetTrue)]
    dump_cpp_from_fbc: bool,
    /// Compile to C and print generated code.
    #[arg(long = "dump-c", action = ArgAction::SetTrue)]
    dump_c: bool,
    /// Compile to FIR and dump FIR IR.
    #[arg(long = "dump-fir", action = ArgAction::SetTrue)]
    dump_fir: bool,
    /// Run FIR verifier and dump the verification report (no codegen).
    #[arg(long = "dump-fir-verify", action = ArgAction::SetTrue)]
    dump_fir_verify: bool,
    /// Compile to interpreter bytecode and print `.fbc` text.
    #[arg(long = "dump-interp", action = ArgAction::SetTrue)]
    dump_interp: bool,
    /// Compile through the experimental Cranelift backend and print a backend report.
    #[arg(long = "dump-cranelift", action = ArgAction::SetTrue)]
    dump_cranelift: bool,
    /// Emit strict C++-style JSON description.
    #[arg(long = "json", action = ArgAction::SetTrue)]
    dump_json: bool,
    /// Select backend language (Faust-style): `-lang c`, `-lang cpp`, `-lang cranelift`, `-lang fir`, `-lang interp`, `-lang wasm`, or `-lang wast`.
    ///
    /// This option is equivalent to `--dump-c` / `--dump-cpp` / `--dump-fir`
    /// / `--dump-interp` / `--dump-cranelift` / `-lang wasm` / `-lang wast`.
    #[arg(long = "lang", value_enum, allow_hyphen_values = true)]
    lang: Option<CliLang>,
    /// Print version information and exit.
    #[arg(short = 'v', long = "version", action = ArgAction::SetTrue)]
    version: bool,
    /// Print dedicated help for diagnostic output formats and exit.
    #[arg(long = "help-error-format", action = ArgAction::SetTrue)]
    help_error_format: bool,
    /// List built-in FIR fixtures available for backend debugging and exit.
    #[arg(long = "list-fir-fixtures", action = ArgAction::SetTrue)]
    list_fir_fixtures: bool,
    /// Use a built-in FIR fixture instead of compiling a DSP input file.
    ///
    /// This is intended for backend debugging / bring-up. Combine with
    /// `-lang fir|c|cpp|interp|cranelift` (or corresponding `--dump-*` flags).
    #[arg(long = "fir-fixture")]
    fir_fixture: Option<String>,
    /// Optional DSP input file (required by operational modes).
    input: Option<PathBuf>,
    /// Optional output file. When omitted, generated text is written to stdout.
    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,
    /// Specify the DSP class name used instead of `mydsp` (`-cn <name>`,
    /// `--class-name <name>`).
    #[arg(long = "class-name")]
    class_name: Option<String>,
    /// Specify the DSP superclass name used instead of `dsp`
    /// (`-scn <name>`, `--super-class-name <name>`).
    #[arg(long = "super-class-name")]
    super_class_name: Option<String>,
    /// Override generated C++ class name for `--dump-cpp-from-fbc`.
    ///
    /// This applies only to `.fbc` -> native C++ emission, distinct from DSP generation
    /// `-cn/--class-name`.
    #[arg(long = "cpp-class-name")]
    cpp_class_name: Option<String>,
    /// Extra import search directories.
    #[arg(short = 'I', long = "import-dir")]
    import_dir: Vec<PathBuf>,
    /// Specify the top-level DSP entry-point name instead of `process`
    /// (`-pn <name>`, `--process-name <name>`).
    #[arg(long = "process-name", default_value = "process")]
    process_name: String,
    /// Wrapper architecture file (`-a` compatibility).
    #[arg(short = 'a', long = "architecture")]
    architecture: Option<PathBuf>,
    /// Additional architecture search directories.
    #[arg(short = 'A', long = "architecture-dir")]
    architecture_dir: Vec<PathBuf>,
    /// Inline `#include <faust/...>` architecture files.
    #[arg(short = 'i', long = "inline-architecture-files", action = ArgAction::SetTrue)]
    inline_architecture_files: bool,
    /// Diagnostic output format.
    #[arg(long = "error-format", value_enum, default_value_t = ErrorFormat::Human)]
    error_format: ErrorFormat,
    /// Diagnostic verbosity level.
    #[arg(
        long = "error-verbosity",
        value_enum,
        default_value_t = ErrorVerbosity::Standard
    )]
    error_verbosity: ErrorVerbosity,
    /// Signal->FIR compilation lane.
    #[arg(long = "signal-fir-lane", value_enum)]
    signal_fir_lane: Option<CliSignalFirLane>,
    /// Disable FIR verification before codegen / FIR dump.
    #[arg(long = "no-fir-verify", action = ArgAction::SetTrue)]
    no_fir_verify: bool,
    /// Treat FIR verifier warnings as fatal.
    #[arg(long = "fir-verify-strict", action = ArgAction::SetTrue)]
    fir_verify_strict: bool,
    /// Use double-precision (64-bit) floating-point for internal DSP computation.
    ///
    /// By default, single-precision (32-bit) `float` is used for internal
    /// calculations while the external DSP interface (`FAUSTFLOAT` audio
    /// buffers and UI zones) always stays at the type declared by the
    /// architecture file.  Passing `--double` switches internal arithmetic
    /// to `double`, matching the `-double` option of the reference Faust
    /// compiler.
    #[arg(long = "double", action = ArgAction::SetTrue)]
    double: bool,
    /// Maximum delay (in samples) below which the shift/copy strategy is used
    /// instead of a circular ring buffer (`-mcd N`).
    ///
    /// Delays ≤ `mcd` use a statically-shifted array (no `fIOTA`). Default: 16.
    #[arg(long = "mcd", default_value_t = 16)]
    mcd: u32,
    /// Delay-line threshold above which the if-based wrapping strategy is used
    /// instead of the default power-of-two circular buffer (`-dlt N`).
    ///
    /// Delays > `dlt` use an exact-size buffer with a per-line counter variable.
    /// Default: disabled (all delays above `mcd` use circular-pow2).
    #[arg(long = "dlt", default_value_t = u32::MAX)]
    dlt: u32,
    /// Display compilation phases timing information (`-time`).
    #[arg(long = "compilation-time", action = ArgAction::SetTrue)]
    compilation_time: bool,
    /// Maximum compilation time in seconds (default: 120).
    #[arg(long = "timeout", default_value_t = 120)]
    timeout: u64,
}

/// Normalizes legacy Faust-style flags to the current `clap` surface.
fn normalize_legacy_args(args: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut normalized = Vec::new();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        if arg == "-lang" {
            normalized.push("--lang".to_owned());
            if let Some(value) = it.next() {
                let mapped = match value.as_str() {
                    "-c" => "c".to_owned(),
                    "-cpp" => "cpp".to_owned(),
                    "-fir" => "fir".to_owned(),
                    "-interp" => "interp".to_owned(),
                    _ => value,
                };
                normalized.push(mapped);
            }
            continue;
        }
        if arg == "-pn" {
            normalized.push("--process-name".to_owned());
            if let Some(value) = it.next() {
                normalized.push(value);
            }
            continue;
        }
        if arg == "-cn" {
            normalized.push("--class-name".to_owned());
            if let Some(value) = it.next() {
                normalized.push(value);
            }
            continue;
        }
        if arg == "-scn" {
            normalized.push("--super-class-name".to_owned());
            if let Some(value) = it.next() {
                normalized.push(value);
            }
            continue;
        }
        if arg == "-double" {
            normalized.push("--double".to_owned());
            continue;
        }
        if arg == "-json" {
            normalized.push("--json".to_owned());
            continue;
        }
        if arg == "-version" {
            normalized.push("--version".to_owned());
            continue;
        }
        if arg == "-mcd" {
            normalized.push("--mcd".to_owned());
            if let Some(value) = it.next() {
                normalized.push(value);
            }
            continue;
        }
        if arg == "-dlt" {
            normalized.push("--dlt".to_owned());
            if let Some(value) = it.next() {
                normalized.push(value);
            }
            continue;
        }
        if arg == "-time" {
            normalized.push("--compilation-time".to_owned());
            continue;
        }
        if arg == "-timeout" {
            normalized.push("--timeout".to_owned());
            if let Some(value) = it.next() {
                normalized.push(value);
            }
            continue;
        }
        normalized.push(arg);
    }
    normalized
}

/// Prints structured diagnostics according to the selected CLI format.
fn print_structured_diagnostics(
    err: &CompilerError,
    format: ErrorFormat,
    verbosity: ErrorVerbosity,
) {
    let Some(bundle) = err.diagnostics() else {
        return;
    };
    match format {
        ErrorFormat::Human => match verbosity {
            ErrorVerbosity::Standard => eprint!("{}", format_diagnostics_human(bundle)),
            ErrorVerbosity::Debug => eprint!(
                "{}",
                format_diagnostics_human_with_verbosity(bundle, verbosity)
            ),
        },
        ErrorFormat::Json => match verbosity {
            ErrorVerbosity::Standard => eprintln!("{}", format_diagnostics_json(bundle)),
            ErrorVerbosity::Debug => eprintln!(
                "{}",
                format_diagnostics_json_with_verbosity(bundle, verbosity)
            ),
        },
    }
}

/// Formats diagnostics in a human-oriented form.
///
/// When a primary label is available and its source file can be read, this renderer
/// includes a source snippet line and a caret span.
fn format_diagnostics_human(bundle: &DiagnosticBundle) -> String {
    format_diagnostics_human_with_verbosity(bundle, ErrorVerbosity::Standard)
}

/// Formats diagnostics in human mode with an explicit verbosity contract.
///
/// `Standard` hides low-level internal notes while `Debug` keeps the full note
/// stream for troubleshooting/benchmark parity workflows.
fn format_diagnostics_human_with_verbosity(
    bundle: &DiagnosticBundle,
    verbosity: ErrorVerbosity,
) -> String {
    let mut out = String::new();
    for diag in bundle.as_slice() {
        let severity = match diag.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Remark => "remark",
        };
        if let Some(label) = diag.labels.first() {
            out.push_str(&format!(
                "{}:{}:{}: {} [{}] {}\n",
                label.span.file.display(),
                label.span.line,
                label.span.col,
                severity,
                diag.code.0,
                diag.message
            ));
            if let Some(line) = source_line(label.span.file.as_path(), label.span.line) {
                out.push_str(&format!("  {} | {}\n", label.span.line, line));
                out.push_str(&format!(
                    "    | {} {}\n",
                    caret_span(label.span.col, label.span.end_col),
                    label.message
                ));
            }
        } else {
            out.push_str(&format!("{severity} [{}] {}\n", diag.code.0, diag.message));
        }

        let paired = paired_context_from_notes(&diag.notes);
        if let Some(ctx) = &paired {
            out.push_str(&format!("  = note: Here  A = {}\n", ctx.a_expr));
            if let Some(arity) = &ctx.a_arity {
                out.push_str(&format!("  = note: has {arity}\n"));
            }
            out.push_str(&format!("  = note: while B = {}\n", ctx.b_expr));
            if let Some(arity) = &ctx.b_arity {
                out.push_str(&format!("  = note: has {arity}\n"));
            }
        }

        for note in filtered_notes_for_human(&diag.notes, paired.is_some(), verbosity) {
            out.push_str(&format!("  = note: {note}\n"));
        }
        for help in &diag.help {
            out.push_str(&format!("  = help: {help}\n"));
        }
    }
    out
}

/// Rendered A/B sub-expressions extracted from a binary composition diagnostic.
///
/// Faust composition errors often involve two mismatched signal processes (e.g.
/// `A : B` where A's output count ≠ B's input count).  `PairedContext` holds
/// the human-readable rendering of both sides so the CLI can emit a C++-style
/// "Here A ... / while B ..." message without baking that format into the
/// structured diagnostic schema.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PairedContext {
    /// Human-readable rendering of the left-hand (A) sub-expression.
    a_expr: String,
    /// Human-readable rendering of the right-hand (B) sub-expression.
    b_expr: String,
    /// Signal arity of A (e.g. `"2→1"`), if available.
    a_arity: Option<String>,
    /// Signal arity of B (e.g. `"1→2"`), if available.
    b_arity: Option<String>,
}

/// Extracts paired composition context (`A`/`B`) from diagnostic notes.
///
/// This enables C++-style human rendering (`Here A ... / while B ...`) without
/// changing the structured diagnostic schema.
fn paired_context_from_notes(notes: &[Box<str>]) -> Option<PairedContext> {
    let mut a_expr = None::<String>;
    let mut b_expr = None::<String>;
    let mut a_arity = None::<String>;
    let mut b_arity = None::<String>;

    for note in notes {
        if let Some(rest) = note.strip_prefix("A arity: ") {
            a_arity = Some(rest.to_owned());
            continue;
        }
        if let Some(rest) = note.strip_prefix("B arity: ") {
            b_arity = Some(rest.to_owned());
            continue;
        }
        if let Some(rest) = note.strip_prefix("A ") {
            if let Some((_, expr)) = rest.split_once(" = ") {
                a_expr = Some(expr.to_owned());
            }
            continue;
        }
        if let Some(rest) = note.strip_prefix("B ") {
            if let Some((_, expr)) = rest.split_once(" = ") {
                b_expr = Some(expr.to_owned());
            }
            continue;
        }
    }

    Some(PairedContext {
        a_expr: a_expr?,
        b_expr: b_expr?,
        a_arity,
        b_arity,
    })
}

/// Filters note lines for human rendering.
///
/// When paired context exists, low-level `A ...` / `B ...` notes are hidden from
/// direct printing because they are rendered as condensed C++-style blocks.
///
/// Internal machine-oriented notes (`node_id`, `box_expr`) are also hidden in
/// standard human mode to keep output focused on actionable diagnostics.
fn filtered_notes_for_human(
    notes: &[Box<str>],
    has_paired_context: bool,
    verbosity: ErrorVerbosity,
) -> Vec<&str> {
    let mut out = Vec::new();
    for note in notes {
        if matches!(verbosity, ErrorVerbosity::Standard)
            && (note.starts_with("node_id=") || note.starts_with("box_expr="))
        {
            continue;
        }
        if has_paired_context
            && (note.starts_with("A ")
                || note.starts_with("B ")
                || note.starts_with("A arity: ")
                || note.starts_with("B arity: "))
        {
            continue;
        }
        out.push(note.as_ref());
    }
    out
}

/// Returns one source line from a file (1-based line number).
fn source_line(path: &Path, line_number: u32) -> Option<String> {
    let source = std::fs::read_to_string(path).ok()?;
    let idx = usize::try_from(line_number.checked_sub(1)?).ok()?;
    source.lines().nth(idx).map(str::to_owned)
}

/// Builds a caret marker string from 1-based `(col, end_col)` bounds.
fn caret_span(col: u32, end_col: u32) -> String {
    let start = usize::try_from(col.saturating_sub(1)).unwrap_or(0);
    let end = usize::try_from(end_col.saturating_sub(1)).unwrap_or(start);
    let width = end.saturating_sub(start).max(1);
    format!("{}{}", " ".repeat(start), "^".repeat(width))
}

/// Formats diagnostics in a machine-oriented JSON payload.
fn format_diagnostics_json(bundle: &DiagnosticBundle) -> String {
    format_diagnostics_json_with_verbosity(bundle, ErrorVerbosity::Standard)
}

/// Formats diagnostics in JSON with optional debug-oriented enrichment.
///
/// `Standard` keeps the stable CI/IDE contract.
/// `Debug` adds extracted low-level fields under `diagnostics[*].debug`.
fn format_diagnostics_json_with_verbosity(
    bundle: &DiagnosticBundle,
    verbosity: ErrorVerbosity,
) -> String {
    let diagnostics = bundle
        .as_slice()
        .iter()
        .map(|diag| {
            let labels = diag
                .labels
                .iter()
                .map(|label| {
                    let role = label_role(label.message.as_ref());
                    json!({
                        "style": match label.style {
                            LabelStyle::Primary => "primary",
                            LabelStyle::Secondary => "secondary",
                        },
                        "role": role,
                        "file": label.span.file.display().to_string(),
                        "line": label.span.line,
                        "col": label.span.col,
                        "end_line": label.span.end_line,
                        "end_col": label.span.end_col,
                        "message": label.message,
                    })
                })
                .collect::<Vec<_>>();
            let mut payload = json!({
                "severity": match diag.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Remark => "remark",
                },
                "stage": match diag.stage {
                    Stage::SourceReader => "source_reader",
                    Stage::Lexer => "lexer",
                    Stage::Parser => "parser",
                    Stage::Eval => "eval",
                    Stage::Propagate => "propagate",
                    Stage::Normalize => "normalize",
                    Stage::Transform => "transform",
                    Stage::Fir => "fir",
                    Stage::Codegen => "codegen",
                    Stage::Compiler => "compiler",
                },
                "code": diag.code.0,
                "message": diag.message,
                "labels": labels,
                "notes": diag.notes,
                "help": diag.help,
                "context": diagnostic_context_from_notes(diag.notes.as_slice()),
            });
            if matches!(verbosity, ErrorVerbosity::Debug)
                && let Some(obj) = payload.as_object_mut()
            {
                obj.insert(
                    "debug".to_owned(),
                    diagnostic_debug_from_notes(diag.notes.as_slice()),
                );
            }
            payload
        })
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&json!({ "diagnostics": diagnostics }))
        .expect("diagnostics JSON formatting should not fail")
}

/// Maps human label messages to stable JSON role identifiers.
///
/// This keeps machine-readable output decoupled from prose used in human mode.
fn label_role(message: &str) -> Option<&'static str> {
    match message {
        "call site" => Some("call_site"),
        "definition site" => Some("definition_site"),
        _ => None,
    }
}

/// Extracts structured context fields from diagnostic notes when present.
///
/// The extraction is best-effort and intentionally tolerant: unknown notes are
/// ignored so textual diagnostics can evolve without breaking JSON consumers.
fn diagnostic_context_from_notes(notes: &[Box<str>]) -> serde_json::Value {
    let mut owner_definition = None::<String>;
    let mut binding_trace = None::<Vec<String>>;
    let mut scope_local = None::<String>;
    let mut scope_visible = None::<String>;
    let mut scope_top_level = None::<String>;

    for note in notes {
        if let Some(owner) = note.strip_prefix("error originates from definition '") {
            owner_definition = Some(owner.trim_end_matches('\'').to_owned());
            continue;
        }
        if let Some(trace) = note.strip_prefix("binding_trace=") {
            let path = trace.split(" -> ").map(str::to_owned).collect::<Vec<_>>();
            if !path.is_empty() {
                binding_trace = Some(path);
            }
            continue;
        }
        if let Some(v) = note.strip_prefix("scope.local=") {
            scope_local = Some(v.to_owned());
            continue;
        }
        if let Some(v) = note.strip_prefix("scope.visible=") {
            scope_visible = Some(v.to_owned());
            continue;
        }
        if let Some(v) = note.strip_prefix("scope.top_level=") {
            scope_top_level = Some(v.to_owned());
            continue;
        }
    }

    json!({
        "owner_definition": owner_definition,
        "binding_trace_path": binding_trace,
        "scope": {
            "local": scope_local,
            "visible": scope_visible,
            "top_level": scope_top_level,
        }
    })
}

/// Extracts debug-only fields from diagnostic notes.
///
/// This keeps internal details (`node_id`, `box_expr`) out of the default JSON
/// surface while still allowing explicit debug workflows.
fn diagnostic_debug_from_notes(notes: &[Box<str>]) -> serde_json::Value {
    let mut node_id = None::<u32>;
    let mut box_expr = None::<String>;
    for note in notes {
        if let Some(v) = note.strip_prefix("node_id=") {
            node_id = v.parse::<u32>().ok();
            continue;
        }
        if let Some(v) = note.strip_prefix("box_expr=") {
            box_expr = Some(v.to_owned());
            continue;
        }
    }
    json!({
        "node_id": node_id,
        "box_expr": box_expr
    })
}

/// Prints top-level usage and exits the process.
fn print_global_usage_and_exit() -> ! {
    eprintln!("Usage:");
    eprintln!(
        "  cargo run -p compiler -- -lang c|cpp|fir|wast <input.dsp> [-o <file>] [-I <dir> ...] [--class-name <name>] [--super-class-name <name>] [--signal-fir-lane fast] [--error-format human|json] [--error-verbosity standard|debug]"
    );
    eprintln!("                           [--no-fir-verify] [--fir-verify-strict]");
    eprintln!("  cargo run -p compiler -- --golden <input.dsp>");
    eprintln!(
        "  cargo run -p compiler -- --parse <input.dsp> [-I <dir> ...] [--error-format human|json] [--error-verbosity standard|debug]"
    );
    eprintln!(
        "  cargo run -p compiler -- --dump-box <input.dsp> [-o <file>] [-I <dir> ...] [--error-format human|json] [--error-verbosity standard|debug]"
    );
    eprintln!(
        "  cargo run -p compiler -- --dump-sig <input.dsp> [-o <file>] [-I <dir> ...] [--error-format human|json] [--error-verbosity standard|debug]"
    );
    eprintln!(
        "  cargo run -p compiler -- --dump-fir <input.dsp> [-o <file>] [-I <dir> ...] [--signal-fir-lane fast] [--error-format human|json] [--error-verbosity standard|debug]"
    );
    eprintln!(
        "  cargo run -p compiler -- --json <input.dsp> [-o <file>] [-I <dir> ...] [--signal-fir-lane fast]"
    );
    eprintln!(
        "  cargo run -p compiler -- --dump-fir-verify <input.dsp> [-o <file>] [-I <dir> ...] [--signal-fir-lane fast] [--fir-verify-strict]"
    );
    eprintln!(
        "  cargo run -p compiler -- --dump-cpp <input.dsp> [-o <file>] [-I <dir> ...] [--class-name <name>] [--super-class-name <name>] [--signal-fir-lane fast] [--error-format human|json] [--error-verbosity standard|debug]"
    );
    eprintln!(
        "  cargo run -p compiler -- --dump-cpp-from-fbc <input.fbc> [-o <file>] [--cpp-class-name <name>]"
    );
    eprintln!(
        "  cargo run -p compiler -- --dump-c <input.dsp> [-o <file>] [-I <dir> ...] [--class-name <name>] [--signal-fir-lane fast] [--error-format human|json] [--error-verbosity standard|debug]"
    );
    std::process::exit(2);
}

/// Emits the on-demand error-format help footer.
fn maybe_print_error_format_help(enabled: bool) {
    if enabled {
        println!("--error-format human|json");
        println!("--error-verbosity standard|debug");
        println!("  human: file:line:col severity [CODE] message");
        println!("  json: structured diagnostics payload for CI/IDE tooling");
        println!("  standard: concise human notes, hides internal ids");
        println!("  debug: keeps full internal notes in human mode");
        std::process::exit(0);
    }
}

/// Writes generated output either to stdout or to the requested file.
fn emit_output(content: &str, output: Option<&PathBuf>) {
    if let Some(path) = output {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && let Err(err) = std::fs::create_dir_all(parent)
        {
            eprintln!(
                "Failed to create output directory {}: {err}",
                parent.display()
            );
            std::process::exit(1);
        }
        if let Err(err) = std::fs::write(path, content) {
            eprintln!("Failed to write output file {}: {err}", path.display());
            std::process::exit(1);
        }
    } else {
        print!("{content}");
    }
}

/// Writes generated binary output either to stdout or to the requested file.
fn emit_binary_output(content: &[u8], output: Option<&PathBuf>) {
    if let Some(path) = output {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && let Err(err) = std::fs::create_dir_all(parent)
        {
            eprintln!(
                "Failed to create output directory {}: {err}",
                parent.display()
            );
            std::process::exit(1);
        }
        if let Err(err) = std::fs::write(path, content) {
            eprintln!("Failed to write output file {}: {err}", path.display());
            std::process::exit(1);
        }
    } else if let Err(err) = std::io::Write::write_all(&mut std::io::stdout(), content) {
        eprintln!("Failed to write binary output to stdout: {err}");
        std::process::exit(1);
    }
}

/// Writes a WASM binary and, when writing to a file path, the companion JSON
/// metadata file next to it using the same stem and a `.json` extension.
fn emit_wasm_output(wasm_binary: &[u8], dsp_json: &str, output: Option<&PathBuf>) {
    if let Some(path) = output {
        emit_binary_output(wasm_binary, Some(path));
        let json_path = path.with_extension("json");
        emit_output(dsp_json, Some(&json_path));
    } else {
        emit_binary_output(wasm_binary, None);
    }
}

fn render_wast_output(wasm_binary: &[u8]) -> String {
    match wasmprinter::print_bytes(wasm_binary) {
        Ok(wast) => wast,
        Err(err) => {
            eprintln!("Failed to render WAST text from generated WASM: {err}");
            std::process::exit(1);
        }
    }
}

/// Writes a JSON companion file next to an existing backend output file using
/// the same stem and a `.json` extension.
fn emit_json_companion_output(json_text: &str, output: &Path) {
    let json_path = output.with_extension("json");
    emit_output(json_text, Some(&json_path));
}

fn cli_lang_name(lang: CliLang) -> &'static str {
    match lang {
        CliLang::C => "c",
        CliLang::Cpp => "cpp",
        CliLang::Fir => "fir",
        CliLang::Interp => "interp",
        CliLang::Cranelift => "cranelift",
        CliLang::Wasm => "wasm",
        CliLang::Wast => "wast",
    }
}

fn require_companion_output_path(cli: &CliArgs) -> &PathBuf {
    cli.output.as_ref().unwrap_or_else(|| {
        eprintln!("--json used with -lang requires -o <file> so the companion JSON has a path");
        std::process::exit(2);
    })
}

/// Renders a short Cranelift backend status report for the CLI.
fn render_cranelift_report(
    compiled: &codegen::backends::cranelift::JitDspModule,
    subset_gap: Option<&str>,
) -> String {
    let layout = compiled.struct_layout();
    let mut out = String::new();
    out.push_str("backend: cranelift (experimental)\n");
    out.push_str(&format!("module: {}\n", compiled.module_name()));
    out.push_str(&format!(
        "compute_symbol: {}\n",
        compiled.compute_symbol_name()
    ));
    out.push_str(&format!(
        "compute_entry_addr: 0x{:x}\n",
        compiled.compute_entry_addr()
    ));
    out.push_str(&format!(
        "compute_body_lowered: {}\n",
        compiled.compute_body_lowered()
    ));
    if let Some(reason) = subset_gap {
        out.push_str(&format!("subset_gap: {reason}\n"));
    }
    out.push_str(&format!(
        "dsp_struct_layout: size={} align={} fields={}\n",
        layout.size_bytes(),
        layout.align_bytes(),
        layout.fields().len()
    ));
    for field in layout.fields() {
        let kind = match &field.kind {
            StructFieldKind::Scalar(typ) => format!("scalar:{typ:?}"),
            StructFieldKind::Table { elem_type, len } => {
                format!("table:{elem_type:?}[{len}]")
            }
        };
        out.push_str(&format!(
            "  - {} @{} size={} align={} {}\n",
            field.name, field.offset_bytes, field.size_bytes, field.align_bytes, kind
        ));
    }
    out
}

/// Maps CLI backend selection to the signal->FIR lane used internally.
fn selected_codegen_lane(cli: &CliArgs) -> CliSignalFirLane {
    cli.signal_fir_lane.unwrap_or(CliSignalFirLane::Fast)
}

/// Maps CLI switches to FIR verifier behavior.
fn selected_fir_verify_options(cli: &CliArgs) -> FirVerifyOptions {
    FirVerifyOptions {
        enabled: !cli.no_fir_verify,
        strict: cli.fir_verify_strict,
    }
}

/// Maps CLI precision switches to the internal DSP real type.
fn selected_real_type(cli: &CliArgs) -> RealType {
    if cli.double {
        RealType::Float64
    } else {
        RealType::Float32
    }
}

/// Builds one configured [`Compiler`] instance from parsed CLI arguments.
fn compiler_from_cli(
    cli: &CliArgs,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Compiler {
    let mut compiler = Compiler::new()
        .with_fir_verify_options(selected_fir_verify_options(cli))
        .with_process_name(cli.process_name.clone())
        .with_real_type(selected_real_type(cli))
        .with_mcd(cli.mcd)
        .with_dlt(cli.dlt);
    if let Some(flag) = cancel {
        compiler = compiler.with_cancel(flag);
    }
    if cli.compilation_time {
        compiler = compiler.with_timing_sink(|name, duration| {
            eprintln!("end {name} (duration : {:.6})", duration.as_secs_f64());
        });
    }
    compiler
}

/// Returns the configured DSP class name, or `None` when the flag was not set
/// or was set to an empty string.
fn selected_class_name(cli: &CliArgs) -> Option<String> {
    cli.class_name
        .as_ref()
        .filter(|name| !name.is_empty())
        .cloned()
}

/// Returns the configured DSP superclass name, or `None` when the flag was not
/// set or was set to an empty string.
fn selected_super_class_name(cli: &CliArgs) -> Option<String> {
    cli.super_class_name
        .as_ref()
        .filter(|name| !name.is_empty())
        .cloned()
}

/// Renders the list of built-in FIR backend fixtures for `--fir-fixture`.
fn render_fir_fixture_list() -> String {
    let mut out = String::from("Built-in FIR fixtures:\n");
    for (name, _) in backend_test_fixtures() {
        out.push_str("- ");
        out.push_str(name);
        out.push('\n');
    }
    out
}

/// Looks up one named FIR backend fixture builder.
fn find_fir_fixture(name: &str) -> Option<codegen::fixtures::FirFixtureBuilder> {
    backend_test_fixtures()
        .iter()
        .find_map(|(n, build)| (*n == name).then_some(*build))
}

/// Compiles a named FIR fixture through the interpreter backend and renders summary text.
fn compile_fixture_to_interp_text(
    store: &fir::FirStore,
    module: fir::FirId,
    options: &InterpOptions,
) -> Result<String, String> {
    let factory =
        generate_interp_module::<f32>(store, module, options).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    write_fbc(&factory, &mut buf, false).map_err(|e| e.to_string())?;
    String::from_utf8(buf).map_err(|e| e.to_string())
}

/// Compiles a named FIR fixture to strict C++-style JSON text.
fn compile_fixture_to_json_text(
    store: &fir::FirStore,
    module: fir::FirId,
    compile_options: String,
    double_precision: bool,
) -> Result<String, String> {
    let fir::FirMatch::Module {
        name,
        functions,
        num_inputs,
        num_outputs,
        ..
    } = fir::match_fir(store, module)
    else {
        return Err("JSON fixture generation expects a FIR Module root".to_owned());
    };
    let fir::FirMatch::Block(function_items) = fir::match_fir(store, functions) else {
        return Err("JSON fixture generation expects a FIR function block".to_owned());
    };
    let layout = codegen::backends::wasm::layout::WasmMemoryLayout::from_module(
        store,
        module,
        &WasmOptions {
            double_precision,
            ..WasmOptions::default()
        },
        0,
    )
    .map_err(|e| e.to_string())?;
    let json = codegen::json::build_json_description_from_fir(
        store,
        &function_items,
        codegen::json::JsonBuildOptions {
            name,
            filename: None,
            version: Some(Compiler::version().to_owned()),
            compile_options: Some(compile_options),
            library_list: Vec::new(),
            include_pathnames: Vec::new(),
            top_level_meta: Vec::new(),
            size: Some(layout.struct_size),
            inputs: num_inputs,
            outputs: num_outputs,
            sr_index: None,
        },
        |_var| None,
    )
    .map_err(|e| e.to_string())?;
    Ok(json.render())
}

fn emit_cli_json_companion_for_backend(
    compiler: &Compiler,
    cli: &CliArgs,
    input_path: &Path,
    backend_lang: CliLang,
) {
    let compile_options =
        compile_options_json_string(Some(cli_lang_name(backend_lang)), cli.double);
    let result = if cli.import_dir.is_empty() {
        compiler.compile_file_default_to_json_with_lane_and_compile_options(
            input_path,
            selected_codegen_lane(cli).into_compiler_lane(),
            compile_options,
        )
    } else {
        compiler.compile_file_to_json_with_compile_options(
            input_path,
            &cli.import_dir,
            selected_codegen_lane(cli).into_compiler_lane(),
            compile_options,
        )
    };

    match result {
        Ok(json) => emit_json_companion_output(&json, require_companion_output_path(cli)),
        Err(err) => {
            eprintln!("JSON companion pipeline failed: {err}");
            print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
            std::process::exit(1);
        }
    }
}

/// Renders a FIR verifier report in CLI-friendly text form.
fn render_fir_verify_report(store: &fir::FirStore, module: fir::FirId, strict: bool) -> String {
    let report = verify_fir_module(store, module);
    let errors = report.errors().count();
    let warnings = report.warnings().count();
    let fatal = errors > 0 || (strict && warnings > 0);
    let mut out = String::new();
    out.push_str(&format!(
        "FIR verify: errors={errors} warnings={warnings} strict={strict} status={}\n",
        if fatal { "FAIL" } else { "OK" }
    ));
    for d in &report.diagnostics {
        let sev = match d.severity {
            fir::checker::Severity::Error => "error",
            fir::checker::Severity::Warning => "warning",
        };
        out.push_str(&format!("- {sev} [{}] {}", d.code, d.message));
        if let Some(fun) = d.context.function_name.as_deref() {
            out.push_str(&format!(" (fn={fun})"));
        }
        out.push_str(&format!(" [node={}]\n", d.node.as_u32()));
    }
    out
}

fn main() {
    // The evaluator's structural-lowering pass (`a2sb`) can recurse deeply for
    // large programs (e.g. auto-panning with many channels). 64 MiB is enough
    // headroom for the 1024-frame structural depth limit while leaving the
    // default 8 MiB thread stack free for the OS and Rust runtime.
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(run_main)
        .expect("failed to spawn compiler thread")
        .join()
        .expect("compiler thread panicked");
}

fn run_main() {
    let args = normalize_legacy_args(std::env::args());
    let cli = CliArgs::parse_from(args);
    if cli.version {
        println!("faust-rs {}", Compiler::version());
        return;
    }
    maybe_print_error_format_help(cli.help_error_format);

    // Cooperative cancellation flag + CLI watchdog.
    //
    // Two-pronged timeout approach:
    // 1. The cooperative cancel flag is checked by the evaluator on every
    //    recursive call and returns `EvalError::Cancelled`. This is safe for
    //    library (libfaust) usage because it never calls `process::exit`.
    // 2. The CLI watchdog calls `process::exit(1)` as a last resort if the
    //    cancel flag didn't abort in time (e.g. hang in propagation phase
    //    where cancel checks are not yet wired). This is CLI-only and
    //    acceptable for a standalone process.
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let timeout_secs = cli.timeout;
        if timeout_secs > 0 {
            let cancel_clone = std::sync::Arc::clone(&cancel);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(timeout_secs));
                // First, try cooperative cancellation (for eval phase).
                cancel_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                // Give the cooperative path a grace period to take effect.
                std::thread::sleep(std::time::Duration::from_secs(2));
                // If still alive, force exit (for non-eval phase hangs).
                eprintln!(
                    "ERROR: compilation timeout ({}s limit exceeded)",
                    timeout_secs,
                );
                std::process::exit(1);
            });
        }
    }

    if cli.list_fir_fixtures {
        if cli.fir_fixture.is_some() || cli.input.is_some() {
            eprintln!("--list-fir-fixtures does not accept --fir-fixture or input file");
            std::process::exit(2);
        }
        emit_output(&render_fir_fixture_list(), cli.output.as_ref());
        return;
    }

    let backend_mode_count = [
        cli.golden,
        cli.parse,
        cli.dump_box,
        cli.dump_sig,
        cli.dump_cpp,
        cli.dump_cpp_from_fbc,
        cli.dump_c,
        cli.dump_fir,
        cli.dump_fir_verify,
        cli.dump_interp,
        cli.dump_cranelift,
        cli.dump_json,
        cli.lang.is_some(),
    ]
    .into_iter()
    .filter(|v| *v)
    .count();

    let json_plus_lang_only = cli.dump_json && cli.lang.is_some() && backend_mode_count == 2;
    let mode_count = if json_plus_lang_only {
        1
    } else {
        backend_mode_count
    };

    if mode_count > 1 {
        print_global_usage_and_exit();
    }

    if mode_count == 0 && cli.input.is_none() && cli.fir_fixture.is_none() {
        println!("faust-rs compiler scaffold v{}", Compiler::version());
        return;
    }
    if mode_count == 0 {
        // Default compile mode: C++ backend, aligned with Faust CLI behavior.
    }

    if cli.fir_fixture.is_some() && cli.input.is_some() {
        eprintln!("--fir-fixture is incompatible with a DSP input file");
        std::process::exit(2);
    }
    if matches!(cli.class_name.as_deref(), Some("")) {
        eprintln!("--class-name cannot be empty");
        std::process::exit(2);
    }
    if matches!(cli.super_class_name.as_deref(), Some("")) {
        eprintln!("--super-class-name cannot be empty");
        std::process::exit(2);
    }

    if (cli.dump_box || cli.dump_sig || cli.parse || cli.golden) && cli.signal_fir_lane.is_some() {
        eprintln!(
            "--signal-fir-lane is only valid with --dump-cpp/--dump-c/--dump-fir/--dump-fir-verify/--dump-cranelift"
        );
        std::process::exit(2);
    }
    if cli.dump_cpp_from_fbc {
        if cli.signal_fir_lane.is_some()
            || !cli.import_dir.is_empty()
            || cli.architecture.is_some()
            || !cli.architecture_dir.is_empty()
            || cli.inline_architecture_files
            || cli.fir_fixture.is_some()
            || cli.super_class_name.is_some()
        {
            eprintln!(
                "--dump-cpp-from-fbc is incompatible with --signal-fir-lane/--import-dir/architecture/--fir-fixture/--super-class-name"
            );
            std::process::exit(2);
        }
        if let Some(input) = cli.input.as_ref()
            && input.extension().and_then(|e| e.to_str()) != Some("fbc")
        {
            eprintln!("--dump-cpp-from-fbc expects an input file with .fbc extension");
            std::process::exit(2);
        }
    } else if cli.cpp_class_name.is_some() {
        eprintln!("--cpp-class-name is only valid with --dump-cpp-from-fbc");
        std::process::exit(2);
    }
    if cli.super_class_name.is_some()
        && (cli.dump_c || matches!(cli.lang, Some(CliLang::C)))
        && cli.architecture.is_none()
    {
        eprintln!("--super-class-name is only meaningful for C++ output or architecture wrapping");
        std::process::exit(2);
    }
    if (cli.dump_fir
        || cli.dump_json
        || cli.dump_fir_verify
        || matches!(
            cli.lang,
            Some(
                CliLang::Fir | CliLang::Interp | CliLang::Cranelift | CliLang::Wasm | CliLang::Wast
            )
        ))
        && cli.architecture.is_some()
    {
        eprintln!("--architecture is currently supported only for C/C++ output");
        std::process::exit(2);
    }
    if cli.no_fir_verify && cli.dump_fir_verify {
        eprintln!("--no-fir-verify is incompatible with --dump-fir-verify");
        std::process::exit(2);
    }
    if let Some(path) = cli.architecture_dir.iter().find(|path| path.is_file()) {
        eprintln!(
            "-A/--architecture-dir expects a directory, not a file: {}",
            path.display()
        );
        std::process::exit(2);
    }
    if cli.architecture.is_none()
        && (!cli.architecture_dir.is_empty() || cli.inline_architecture_files)
    {
        eprintln!("--architecture-dir/--inline-architecture-files require --architecture <file>");
        std::process::exit(2);
    }

    if cli.fir_fixture.is_some() {
        if cli.golden || cli.parse || cli.dump_box || cli.dump_sig {
            eprintln!(
                "--fir-fixture supports only FIR/backend dump modes (fir/c/cpp/interp/cranelift/wasm/wast/json)"
            );
            std::process::exit(2);
        }
        if cli.signal_fir_lane.is_some() {
            eprintln!("--signal-fir-lane is not applicable with --fir-fixture (already FIR)");
            std::process::exit(2);
        }
        if !cli.import_dir.is_empty() {
            eprintln!("--import-dir is not used with --fir-fixture");
            std::process::exit(2);
        }
    }

    if let Some(fixture_name) = cli.fir_fixture.as_deref() {
        let Some(build_fixture) = find_fir_fixture(fixture_name) else {
            eprintln!("Unknown FIR fixture: {fixture_name}");
            eprintln!("{}", render_fir_fixture_list());
            std::process::exit(2);
        };
        let (store, module) = build_fixture();

        if cli.dump_fir_verify {
            let rendered = render_fir_verify_report(&store, module, cli.fir_verify_strict);
            let report = verify_fir_module(&store, module);
            let fatal = report.has_errors()
                || (cli.fir_verify_strict && report.warnings().next().is_some());
            emit_output(&rendered, cli.output.as_ref());
            if fatal {
                std::process::exit(1);
            }
            return;
        }

        if cli.dump_fir || matches!(cli.lang, Some(CliLang::Fir)) {
            let mut rendered = dump_fir(&store, module);
            if !rendered.ends_with('\n') {
                rendered.push('\n');
            }
            emit_output(&rendered, cli.output.as_ref());
            if cli.dump_json {
                let output = require_companion_output_path(&cli);
                let compile_options = compile_options_json_string(Some("fir"), cli.double);
                match compile_fixture_to_json_text(&store, module, compile_options, cli.double) {
                    Ok(json) => emit_json_companion_output(&json, output),
                    Err(err) => {
                        eprintln!("JSON fixture generation failed: {err}");
                        std::process::exit(1);
                    }
                }
            }
            return;
        }

        if cli.dump_interp || matches!(cli.lang, Some(CliLang::Interp)) {
            match compile_fixture_to_interp_text(&store, module, &InterpOptions::default()) {
                Ok(fbc_text) => {
                    emit_output(&fbc_text, cli.output.as_ref());
                    if cli.dump_json {
                        let output = require_companion_output_path(&cli);
                        let compile_options =
                            compile_options_json_string(Some("interp"), cli.double);
                        match compile_fixture_to_json_text(
                            &store,
                            module,
                            compile_options,
                            cli.double,
                        ) {
                            Ok(json) => emit_json_companion_output(&json, output),
                            Err(err) => {
                                eprintln!("JSON fixture generation failed: {err}");
                                std::process::exit(1);
                            }
                        }
                    }
                }
                Err(err) => {
                    eprintln!("Interp fixture codegen failed: {err}");
                    std::process::exit(1);
                }
            }
            return;
        }

        if cli.dump_cranelift || matches!(cli.lang, Some(CliLang::Cranelift)) {
            let subset_gap = diagnose_cranelift_compute_subset_gap(&store, module)
                .map_err(|err| err.to_string());
            let compiled =
                match generate_cranelift_module(&store, module, &CraneliftOptions::default()) {
                    Ok(compiled) => compiled,
                    Err(err) => {
                        eprintln!("Cranelift fixture codegen failed: {err}");
                        std::process::exit(1);
                    }
                };
            let rendered = render_cranelift_report(&compiled, subset_gap.ok().flatten().as_deref());
            emit_output(&rendered, cli.output.as_ref());
            if cli.dump_json {
                let output = require_companion_output_path(&cli);
                let compile_options = compile_options_json_string(Some("cranelift"), cli.double);
                match compile_fixture_to_json_text(&store, module, compile_options, cli.double) {
                    Ok(json) => emit_json_companion_output(&json, output),
                    Err(err) => {
                        eprintln!("JSON fixture generation failed: {err}");
                        std::process::exit(1);
                    }
                }
            }
            return;
        }

        if matches!(cli.lang, Some(CliLang::Wasm)) {
            match generate_wasm_module(
                &store,
                module,
                &WasmOptions {
                    double_precision: cli.double,
                    ..WasmOptions::default()
                },
            ) {
                Ok(wasm) => {
                    if cli.dump_json {
                        let output = require_companion_output_path(&cli);
                        emit_wasm_output(&wasm.wasm_binary, &wasm.dsp_json, Some(output));
                    } else {
                        emit_binary_output(&wasm.wasm_binary, cli.output.as_ref());
                    }
                }
                Err(err) => {
                    eprintln!("WASM fixture codegen failed: {err}");
                    std::process::exit(1);
                }
            }
            return;
        }

        if matches!(cli.lang, Some(CliLang::Wast)) {
            match generate_wasm_module(
                &store,
                module,
                &WasmOptions {
                    double_precision: cli.double,
                    ..WasmOptions::default()
                },
            ) {
                Ok(wasm) => {
                    let wast = render_wast_output(&wasm.wasm_binary);
                    emit_output(&wast, cli.output.as_ref());
                    if cli.dump_json {
                        let output = require_companion_output_path(&cli);
                        let compile_options = compile_options_json_string(Some("wast"), cli.double);
                        match compile_fixture_to_json_text(
                            &store,
                            module,
                            compile_options,
                            cli.double,
                        ) {
                            Ok(json) => emit_json_companion_output(&json, output),
                            Err(err) => {
                                eprintln!("JSON fixture generation failed: {err}");
                                std::process::exit(1);
                            }
                        }
                    }
                }
                Err(err) => {
                    eprintln!("WAST fixture codegen failed: {err}");
                    std::process::exit(1);
                }
            }
            return;
        }

        if cli.dump_json {
            let compile_options =
                compile_options_json_string(cli.lang.map(cli_lang_name), cli.double);
            match compile_fixture_to_json_text(&store, module, compile_options, cli.double) {
                Ok(json) => {
                    if cli.lang.is_some() {
                        let output = require_companion_output_path(&cli);
                        emit_json_companion_output(&json, output);
                    } else {
                        emit_output(&json, cli.output.as_ref());
                    }
                }
                Err(err) => {
                    eprintln!("JSON fixture generation failed: {err}");
                    std::process::exit(1);
                }
            }
            return;
        }

        if cli.dump_cpp || matches!(cli.lang, Some(CliLang::Cpp)) || mode_count == 0 {
            let options = CppOptions {
                class_name: selected_class_name(&cli),
                super_class_name: selected_super_class_name(&cli),
                ..CppOptions::default()
            };
            match generate_cpp_module(&store, module, &options) {
                Ok(cpp) => {
                    let rendered = if let Some(architecture_file) = cli.architecture.as_ref() {
                        let mut options = EnrobageOptions::new(architecture_file.clone());
                        options.architecture_dirs = cli.architecture_dir.clone();
                        options.inline_arch_files = cli.inline_architecture_files;
                        if let Some(class_name) = selected_class_name(&cli) {
                            options.class_name = class_name;
                        }
                        if let Some(super_class_name) = selected_super_class_name(&cli) {
                            options.super_class_name = super_class_name;
                        }
                        let wrapped = match wrap_cpp_with_architecture(&cpp, &options) {
                            Ok(wrapped) => wrapped,
                            Err(err) => {
                                eprintln!("Architecture wrapping failed: {err}");
                                std::process::exit(1);
                            }
                        };
                        if let Some(err) = wrapped.recoverable_error.as_deref() {
                            eprintln!("{err}");
                            std::process::exit(1);
                        }
                        wrapped.code
                    } else {
                        cpp
                    };
                    emit_output(&rendered, cli.output.as_ref());
                    if cli.dump_json {
                        let output = require_companion_output_path(&cli);
                        let compile_options = compile_options_json_string(Some("cpp"), cli.double);
                        match compile_fixture_to_json_text(
                            &store,
                            module,
                            compile_options,
                            cli.double,
                        ) {
                            Ok(json) => emit_json_companion_output(&json, output),
                            Err(err) => {
                                eprintln!("JSON fixture generation failed: {err}");
                                std::process::exit(1);
                            }
                        }
                    }
                }
                Err(err) => {
                    eprintln!("C++ fixture codegen failed: {err}");
                    std::process::exit(1);
                }
            }
            return;
        }

        if cli.dump_c || matches!(cli.lang, Some(CliLang::C)) {
            let options = COptions {
                class_name: selected_class_name(&cli),
                ..COptions::default()
            };
            match generate_c_module(&store, module, &options) {
                Ok(c_code) => {
                    let rendered = if let Some(architecture_file) = cli.architecture.as_ref() {
                        let mut options = EnrobageOptions::new(architecture_file.clone());
                        options.architecture_dirs = cli.architecture_dir.clone();
                        options.inline_arch_files = cli.inline_architecture_files;
                        if let Some(class_name) = selected_class_name(&cli) {
                            options.class_name = class_name;
                        }
                        let wrapped = match wrap_cpp_with_architecture(&c_code, &options) {
                            Ok(wrapped) => wrapped,
                            Err(err) => {
                                eprintln!("Architecture wrapping failed: {err}");
                                std::process::exit(1);
                            }
                        };
                        if let Some(err) = wrapped.recoverable_error.as_deref() {
                            eprintln!("{err}");
                            std::process::exit(1);
                        }
                        wrapped.code
                    } else {
                        c_code
                    };
                    emit_output(&rendered, cli.output.as_ref());
                    if cli.dump_json {
                        let output = require_companion_output_path(&cli);
                        let compile_options = compile_options_json_string(Some("c"), cli.double);
                        match compile_fixture_to_json_text(
                            &store,
                            module,
                            compile_options,
                            cli.double,
                        ) {
                            Ok(json) => emit_json_companion_output(&json, output),
                            Err(err) => {
                                eprintln!("JSON fixture generation failed: {err}");
                                std::process::exit(1);
                            }
                        }
                    }
                }
                Err(err) => {
                    eprintln!("C fixture codegen failed: {err}");
                    std::process::exit(1);
                }
            }
            return;
        }

        print_global_usage_and_exit();
    }

    let Some(input_path) = cli.input.as_ref() else {
        print_global_usage_and_exit();
    };

    if cli.dump_cpp_from_fbc {
        let text = match std::fs::read_to_string(input_path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Cannot read .fbc file '{}': {e}", input_path.display());
                std::process::exit(1);
            }
        };
        let mut reader = std::io::BufReader::new(text.as_bytes());
        let factory = match read_fbc::<f32>(&mut reader) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to parse .fbc: {e}");
                std::process::exit(1);
            }
        };
        let opts = FbcCppOptions {
            class_name: cli.cpp_class_name.clone(),
            pragma_once: true,
            namespace: None,
        };
        match generate_cpp_from_fbc(&factory, &opts) {
            Ok(cpp) => emit_output(&cpp, cli.output.as_ref()),
            Err(e) => {
                eprintln!("Native C++ generation from FBC failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    if cli.golden {
        if !cli.import_dir.is_empty() {
            eprintln!("--import-dir is not supported with --golden");
            std::process::exit(2);
        }
        match golden_snapshot_from_file(input_path) {
            Ok(snapshot) => {
                emit_output(&snapshot, cli.output.as_ref());
            }
            Err(err) => {
                eprintln!("Failed to create golden snapshot: {err}");
                std::process::exit(1);
            }
        }
        return;
    }

    if cli.parse {
        let mut timer = CompilationTimer::new(cli.timeout, cli.compilation_time);
        let compiler = compiler_from_cli(&cli, Some(std::sync::Arc::clone(&cancel)));
        let result = if cli.import_dir.is_empty() {
            compiler.compile_file_default(input_path)
        } else {
            compiler.compile_file(input_path, &cli.import_dir)
        };
        timer.phase("parse");

        match result {
            Ok(out) => {
                println!(
                    "Parsed OK: root={:?} parse_errors={} recoveries={}",
                    out.root,
                    out.errors.len(),
                    out.state.ctx.recovery_count()
                );
            }
            Err(err) => {
                eprintln!("Parse failed: {err}");
                print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
                std::process::exit(1);
            }
        }
        timer.total();
        return;
    }

    if cli.dump_box {
        let mut timer = CompilationTimer::new(cli.timeout, cli.compilation_time);
        let compiler = compiler_from_cli(&cli, Some(std::sync::Arc::clone(&cancel)));
        let result = if cli.import_dir.is_empty() {
            compiler.compile_file_default(input_path)
        } else {
            compiler.compile_file(input_path, &cli.import_dir)
        };
        timer.phase("parse");

        match result {
            Ok(out) => {
                let Some(root) = out.root else {
                    eprintln!("Parse failed: no root node produced");
                    std::process::exit(1);
                };
                let rendered = format!("{}\n", dump_box(&out.state.arena, root));
                timer.phase("dump-box");
                emit_output(&rendered, cli.output.as_ref());
            }
            Err(err) => {
                eprintln!("Parse failed: {err}");
                print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
                std::process::exit(1);
            }
        }
        timer.total();
        return;
    }

    if cli.dump_sig {
        let mut timer = CompilationTimer::new(cli.timeout, cli.compilation_time);
        let compiler = compiler_from_cli(&cli, Some(std::sync::Arc::clone(&cancel)));
        let result = if cli.import_dir.is_empty() {
            compiler.compile_file_default_to_signals(input_path)
        } else {
            compiler.compile_file_to_signals(input_path, &cli.import_dir)
        };
        timer.phase("signals");

        match result {
            Ok(out) => {
                let mut rendered = format!(
                    "Signals OK: inputs={} outputs={}",
                    out.process_arity.inputs, out.process_arity.outputs
                );
                for (index, sig) in out.signals.iter().enumerate() {
                    rendered.push('\n');
                    rendered.push_str(&format!(
                        "[{index}] {}",
                        dump_sig_readable(&out.parse.state.arena, *sig)
                    ));
                }
                rendered.push('\n');
                emit_output(&rendered, cli.output.as_ref());
            }
            Err(err) => {
                eprintln!("Signal pipeline failed: {err}");
                print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
                std::process::exit(1);
            }
        }
        timer.total();
        return;
    }

    if cli.dump_fir_verify {
        let mut timer = CompilationTimer::new(cli.timeout, cli.compilation_time);
        let compiler = Compiler::new()
            .with_fir_verify_options(FirVerifyOptions {
                enabled: false,
                strict: false,
            })
            .with_process_name(cli.process_name.clone())
            .with_real_type(selected_real_type(&cli))
            .with_cancel(std::sync::Arc::clone(&cancel));
        let result = if cli.import_dir.is_empty() {
            compiler.compile_file_default_to_fir_with_lane(
                input_path,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        } else {
            compiler.compile_file_to_fir_with_lane(
                input_path,
                &cli.import_dir,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        };
        timer.phase("FIR");

        match result {
            Ok(out) => {
                let rendered =
                    render_fir_verify_report(&out.store, out.module, cli.fir_verify_strict);
                let report = verify_fir_module(&out.store, out.module);
                let fatal = report.has_errors()
                    || (cli.fir_verify_strict && report.warnings().next().is_some());
                timer.phase("verify");
                emit_output(&rendered, cli.output.as_ref());
                if fatal {
                    std::process::exit(1);
                }
            }
            Err(err) => {
                eprintln!("FIR pipeline failed: {err}");
                print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
                std::process::exit(1);
            }
        }
        timer.total();
        return;
    }

    if cli.dump_fir || matches!(cli.lang, Some(CliLang::Fir)) {
        let mut timer = CompilationTimer::new(cli.timeout, cli.compilation_time);
        let compiler = compiler_from_cli(&cli, Some(std::sync::Arc::clone(&cancel)));
        let result = if cli.import_dir.is_empty() {
            compiler.compile_file_default_to_fir_with_lane(
                input_path,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        } else {
            compiler.compile_file_to_fir_with_lane(
                input_path,
                &cli.import_dir,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        };
        timer.phase("FIR");

        match result {
            Ok(out) => {
                let mut rendered = dump_fir(&out.store, out.module);
                if !rendered.ends_with('\n') {
                    rendered.push('\n');
                }
                emit_output(&rendered, cli.output.as_ref());
                if cli.dump_json {
                    emit_cli_json_companion_for_backend(&compiler, &cli, input_path, CliLang::Fir);
                }
            }
            Err(err) => {
                eprintln!("FIR pipeline failed: {err}");
                print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
                std::process::exit(1);
            }
        }
        timer.total();
        return;
    }

    if cli.dump_json && cli.lang.is_none() {
        let mut timer = CompilationTimer::new(cli.timeout, cli.compilation_time);
        let compiler = compiler_from_cli(&cli, Some(std::sync::Arc::clone(&cancel)));
        let result = if cli.import_dir.is_empty() {
            compiler.compile_file_default_to_json_with_lane_and_compile_options(
                input_path,
                selected_codegen_lane(&cli).into_compiler_lane(),
                compile_options_json_string(None, cli.double),
            )
        } else {
            compiler.compile_file_to_json_with_compile_options(
                input_path,
                &cli.import_dir,
                selected_codegen_lane(&cli).into_compiler_lane(),
                compile_options_json_string(None, cli.double),
            )
        };
        timer.phase("json");

        match result {
            Ok(json) => emit_output(&json, cli.output.as_ref()),
            Err(err) => {
                eprintln!("JSON pipeline failed: {err}");
                print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
                std::process::exit(1);
            }
        }
        timer.total();
        return;
    }

    if cli.dump_interp || matches!(cli.lang, Some(CliLang::Interp)) {
        let mut timer = CompilationTimer::new(cli.timeout, cli.compilation_time);
        let compiler = compiler_from_cli(&cli, Some(std::sync::Arc::clone(&cancel)));
        let options = InterpOptions::default();
        let result = if cli.import_dir.is_empty() {
            compiler.compile_file_default_to_interp_with_lane(
                input_path,
                &options,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        } else {
            compiler.compile_file_to_interp_with_lane(
                input_path,
                &cli.import_dir,
                &options,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        };
        timer.phase("interp");

        match result {
            Ok(fbc_text) => {
                emit_output(&fbc_text, cli.output.as_ref());
                if cli.dump_json {
                    emit_cli_json_companion_for_backend(
                        &compiler,
                        &cli,
                        input_path,
                        CliLang::Interp,
                    );
                }
            }
            Err(err) => {
                eprintln!("Interp pipeline failed: {err}");
                print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
                std::process::exit(1);
            }
        }
        timer.total();
        return;
    }

    if cli.dump_cranelift || matches!(cli.lang, Some(CliLang::Cranelift)) {
        let mut timer = CompilationTimer::new(cli.timeout, cli.compilation_time);
        let compiler = compiler_from_cli(&cli, Some(std::sync::Arc::clone(&cancel)));
        let result = if cli.import_dir.is_empty() {
            compiler.compile_file_default_to_fir_with_lane(
                input_path,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        } else {
            compiler.compile_file_to_fir_with_lane(
                input_path,
                &cli.import_dir,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        };
        timer.phase("FIR");

        match result {
            Ok(out) => {
                let subset_gap = diagnose_cranelift_compute_subset_gap(&out.store, out.module)
                    .map_err(|err| err.to_string());
                let compiled = match generate_cranelift_module(
                    &out.store,
                    out.module,
                    &CraneliftOptions::default(),
                ) {
                    Ok(compiled) => compiled,
                    Err(err) => {
                        eprintln!("Cranelift pipeline failed: {err}");
                        std::process::exit(1);
                    }
                };
                timer.phase("cranelift-codegen");
                let rendered =
                    render_cranelift_report(&compiled, subset_gap.ok().flatten().as_deref());
                emit_output(&rendered, cli.output.as_ref());
                if cli.dump_json {
                    emit_cli_json_companion_for_backend(
                        &compiler,
                        &cli,
                        input_path,
                        CliLang::Cranelift,
                    );
                }
            }
            Err(err) => {
                eprintln!("Cranelift FIR pipeline failed: {err}");
                print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
                std::process::exit(1);
            }
        }
        timer.total();
        return;
    }

    if matches!(cli.lang, Some(CliLang::Wasm)) {
        let mut timer = CompilationTimer::new(cli.timeout, cli.compilation_time);
        let compiler = compiler_from_cli(&cli, Some(std::sync::Arc::clone(&cancel)));
        let options = WasmOptions {
            double_precision: cli.double,
            ..WasmOptions::default()
        };
        let result = if cli.import_dir.is_empty() {
            compiler.compile_file_default_to_wasm_with_lane(
                input_path,
                &options,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        } else {
            compiler.compile_file_to_wasm_with_lane(
                input_path,
                &cli.import_dir,
                &options,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        };
        timer.phase("wasm-codegen");

        match result {
            Ok(wasm) => {
                if cli.dump_json {
                    let output = require_companion_output_path(&cli);
                    emit_wasm_output(&wasm.wasm_binary, &wasm.dsp_json, Some(output));
                } else {
                    emit_wasm_output(&wasm.wasm_binary, &wasm.dsp_json, cli.output.as_ref());
                }
            }
            Err(err) => {
                eprintln!("WASM pipeline failed: {err}");
                print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
                std::process::exit(1);
            }
        }
        timer.total();
        return;
    }

    if matches!(cli.lang, Some(CliLang::Wast)) {
        let mut timer = CompilationTimer::new(cli.timeout, cli.compilation_time);
        let compiler = compiler_from_cli(&cli, Some(std::sync::Arc::clone(&cancel)));
        let options = WasmOptions {
            double_precision: cli.double,
            ..WasmOptions::default()
        };
        let result = if cli.import_dir.is_empty() {
            compiler.compile_file_default_to_wasm_with_lane(
                input_path,
                &options,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        } else {
            compiler.compile_file_to_wasm_with_lane(
                input_path,
                &cli.import_dir,
                &options,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        };
        timer.phase("wast-codegen");

        match result {
            Ok(wasm) => {
                let wast = render_wast_output(&wasm.wasm_binary);
                emit_output(&wast, cli.output.as_ref());
                if cli.dump_json {
                    emit_cli_json_companion_for_backend(&compiler, &cli, input_path, CliLang::Wast);
                }
            }
            Err(err) => {
                eprintln!("WAST pipeline failed: {err}");
                print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
                std::process::exit(1);
            }
        }
        timer.total();
        return;
    }

    if cli.dump_cpp || matches!(cli.lang, Some(CliLang::Cpp)) || mode_count == 0 {
        let mut timer = CompilationTimer::new(cli.timeout, cli.compilation_time);
        let compiler = compiler_from_cli(&cli, Some(std::sync::Arc::clone(&cancel)));
        let options = CppOptions {
            class_name: selected_class_name(&cli),
            super_class_name: selected_super_class_name(&cli),
            ..CppOptions::default()
        };
        let result = if cli.import_dir.is_empty() {
            compiler.compile_file_default_to_cpp_with_lane(
                input_path,
                &options,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        } else {
            compiler.compile_file_to_cpp_with_lane(
                input_path,
                &cli.import_dir,
                &options,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        };
        timer.phase("cpp-codegen");

        match result {
            Ok(cpp) => {
                let rendered = if let Some(architecture_file) = cli.architecture.as_ref() {
                    let mut options = EnrobageOptions::new(architecture_file.clone());
                    options.architecture_dirs = cli.architecture_dir.clone();
                    options.inline_arch_files = cli.inline_architecture_files;
                    if let Some(class_name) = selected_class_name(&cli) {
                        options.class_name = class_name;
                    }
                    if let Some(super_class_name) = selected_super_class_name(&cli) {
                        options.super_class_name = super_class_name;
                    }
                    let wrapped = match wrap_cpp_with_architecture(&cpp, &options) {
                        Ok(wrapped) => wrapped,
                        Err(err) => {
                            eprintln!("Architecture wrapping failed: {err}");
                            std::process::exit(1);
                        }
                    };
                    if let Some(err) = wrapped.recoverable_error.as_deref() {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }
                    wrapped.code
                } else {
                    cpp
                };
                emit_output(&rendered, cli.output.as_ref());
                if cli.dump_json {
                    emit_cli_json_companion_for_backend(&compiler, &cli, input_path, CliLang::Cpp);
                }
            }
            Err(err) => {
                eprintln!("C++ pipeline failed: {err}");
                print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
                std::process::exit(1);
            }
        }
        timer.total();
        return;
    }

    if cli.dump_c || matches!(cli.lang, Some(CliLang::C)) {
        let mut timer = CompilationTimer::new(cli.timeout, cli.compilation_time);
        let compiler = compiler_from_cli(&cli, Some(std::sync::Arc::clone(&cancel)));
        let options = COptions {
            class_name: selected_class_name(&cli),
            ..COptions::default()
        };
        let result = if cli.import_dir.is_empty() {
            compiler.compile_file_default_to_c_with_lane(
                input_path,
                &options,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        } else {
            compiler.compile_file_to_c_with_lane(
                input_path,
                &cli.import_dir,
                &options,
                selected_codegen_lane(&cli).into_compiler_lane(),
            )
        };
        timer.phase("c-codegen");

        match result {
            Ok(c_code) => {
                let rendered = if let Some(architecture_file) = cli.architecture.as_ref() {
                    let mut options = EnrobageOptions::new(architecture_file.clone());
                    options.architecture_dirs = cli.architecture_dir.clone();
                    options.inline_arch_files = cli.inline_architecture_files;
                    if let Some(class_name) = selected_class_name(&cli) {
                        options.class_name = class_name;
                    }
                    let wrapped = match wrap_cpp_with_architecture(&c_code, &options) {
                        Ok(wrapped) => wrapped,
                        Err(err) => {
                            eprintln!("Architecture wrapping failed: {err}");
                            std::process::exit(1);
                        }
                    };
                    if let Some(err) = wrapped.recoverable_error.as_deref() {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }
                    wrapped.code
                } else {
                    c_code
                };
                emit_output(&rendered, cli.output.as_ref());
                if cli.dump_json {
                    emit_cli_json_companion_for_backend(&compiler, &cli, input_path, CliLang::C);
                }
            }
            Err(err) => {
                eprintln!("C pipeline failed: {err}");
                print_structured_diagnostics(&err, cli.error_format, cli.error_verbosity);
                std::process::exit(1);
            }
        }
        timer.total();
        return;
    }

    print_global_usage_and_exit();
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use clap::{CommandFactory, Parser};
    use compiler::Compiler;
    use errors::{Diagnostic, DiagnosticBundle, DiagnosticCode, Severity, SourceSpan, Stage};
    use serde_json::Value;
    use signals::{SigMatch, match_sig};

    use super::{
        CliArgs, CliLang, ErrorVerbosity, emit_wasm_output, format_diagnostics_human,
        format_diagnostics_human_with_verbosity, format_diagnostics_json,
        format_diagnostics_json_with_verbosity, normalize_legacy_args, render_wast_output,
    };

    #[test]
    fn normalize_legacy_args_maps_dash_fir_to_lang_fir() {
        let args = vec![
            "faust-rs".to_owned(),
            "-lang".to_owned(),
            "-fir".to_owned(),
            "foo.dsp".to_owned(),
        ];
        let normalized = normalize_legacy_args(args);
        assert_eq!(
            normalized,
            vec![
                "faust-rs".to_owned(),
                "--lang".to_owned(),
                "fir".to_owned(),
                "foo.dsp".to_owned()
            ]
        );
    }

    #[test]
    fn normalize_legacy_args_maps_dash_pn_to_process_name() {
        let args = vec![
            "faust-rs".to_owned(),
            "-pn".to_owned(),
            "dsp".to_owned(),
            "foo.dsp".to_owned(),
        ];
        let normalized = normalize_legacy_args(args);
        assert_eq!(
            normalized,
            vec![
                "faust-rs".to_owned(),
                "--process-name".to_owned(),
                "dsp".to_owned(),
                "foo.dsp".to_owned()
            ]
        );
    }

    #[test]
    fn cli_parse_accepts_lang_fir() {
        let cli = CliArgs::parse_from(["faust-rs", "--lang", "fir", "foo.dsp"]);
        assert!(matches!(cli.lang, Some(CliLang::Fir)));
    }

    #[test]
    fn cli_parse_accepts_lang_cranelift() {
        let cli = CliArgs::parse_from(["faust-rs", "--lang", "cranelift", "foo.dsp"]);
        assert!(matches!(cli.lang, Some(CliLang::Cranelift)));
    }

    #[test]
    fn cli_parse_accepts_lang_wasm() {
        let cli = CliArgs::parse_from(["faust-rs", "--lang", "wasm", "foo.dsp"]);
        assert!(matches!(cli.lang, Some(CliLang::Wasm)));
    }

    #[test]
    fn cli_parse_accepts_lang_wast() {
        let cli = CliArgs::parse_from(["faust-rs", "--lang", "wast", "foo.dsp"]);
        assert!(matches!(cli.lang, Some(CliLang::Wast)));
    }

    #[test]
    fn cli_parse_accepts_json_flag() {
        let cli = CliArgs::parse_from(["faust-rs", "--json", "foo.dsp"]);
        assert!(cli.dump_json);
    }

    #[test]
    fn cli_parse_accepts_json_with_lang() {
        let cli = CliArgs::parse_from(["faust-rs", "--json", "--lang", "cpp", "foo.dsp"]);
        assert!(cli.dump_json);
        assert!(matches!(cli.lang, Some(CliLang::Cpp)));
    }

    #[test]
    fn normalize_legacy_args_maps_dash_json_to_json_flag() {
        let args = vec![
            "faust-rs".to_owned(),
            "-json".to_owned(),
            "foo.dsp".to_owned(),
        ];
        let normalized = normalize_legacy_args(args);
        assert_eq!(
            normalized,
            vec![
                "faust-rs".to_owned(),
                "--json".to_owned(),
                "foo.dsp".to_owned()
            ]
        );
    }

    #[test]
    fn emit_wasm_output_writes_companion_json_next_to_wasm_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "faust_rs_wasm_emit_test_{}_{}",
            std::process::id(),
            unique
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        let wasm_path = dir.join("voice.wasm");

        emit_wasm_output(
            b"\0asm\x01\0\0\0",
            "{\"backend\":\"wasm\"}",
            Some(&wasm_path),
        );

        let json_path = dir.join("voice.json");
        assert_eq!(
            fs::read(&wasm_path).expect("wasm output should exist"),
            b"\0asm\x01\0\0\0"
        );
        assert_eq!(
            fs::read_to_string(&json_path).expect("json output should exist"),
            "{\"backend\":\"wasm\"}"
        );

        fs::remove_dir_all(&dir).expect("temp dir should be removed");
    }

    #[test]
    fn render_wast_output_prints_valid_text_module() {
        let wast = render_wast_output(b"\0asm\x01\0\0\0");
        assert!(wast.contains("(module"));
    }

    #[test]
    fn cli_parse_accepts_dump_cranelift() {
        let cli = CliArgs::parse_from(["faust-rs", "--dump-cranelift", "foo.dsp"]);
        assert!(cli.dump_cranelift);
    }

    #[test]
    fn cli_parse_accepts_process_name() {
        let cli = CliArgs::parse_from(["faust-rs", "--process-name", "dsp", "foo.dsp"]);
        assert_eq!(cli.process_name, "dsp");
    }

    #[test]
    fn cli_parse_accepts_class_name() {
        let cli = CliArgs::parse_from(["faust-rs", "--class-name", "customdsp", "foo.dsp"]);
        assert_eq!(cli.class_name.as_deref(), Some("customdsp"));
    }

    #[test]
    fn cli_parse_accepts_super_class_name() {
        let cli = CliArgs::parse_from(["faust-rs", "--super-class-name", "faust_dsp", "foo.dsp"]);
        assert_eq!(cli.super_class_name.as_deref(), Some("faust_dsp"));
    }

    #[test]
    fn normalize_legacy_args_maps_dash_cn_to_class_name() {
        let normalized = normalize_legacy_args(vec![
            "faust-rs".to_owned(),
            "-cn".to_owned(),
            "customdsp".to_owned(),
            "foo.dsp".to_owned(),
        ]);
        assert_eq!(
            normalized,
            vec![
                "faust-rs".to_owned(),
                "--class-name".to_owned(),
                "customdsp".to_owned(),
                "foo.dsp".to_owned(),
            ]
        );
    }

    #[test]
    fn normalize_legacy_args_maps_dash_scn_to_super_class_name() {
        let normalized = normalize_legacy_args(vec![
            "faust-rs".to_owned(),
            "-scn".to_owned(),
            "faust_dsp".to_owned(),
            "foo.dsp".to_owned(),
        ]);
        assert_eq!(
            normalized,
            vec![
                "faust-rs".to_owned(),
                "--super-class-name".to_owned(),
                "faust_dsp".to_owned(),
                "foo.dsp".to_owned(),
            ]
        );
    }

    #[test]
    fn cli_parse_accepts_list_fir_fixtures() {
        let cli = CliArgs::parse_from(["faust-rs", "--list-fir-fixtures"]);
        assert!(cli.list_fir_fixtures);
    }

    #[test]
    fn cli_parse_accepts_fir_fixture_with_lang() {
        let cli =
            CliArgs::parse_from(["faust-rs", "--fir-fixture", "sine_phasor", "--lang", "cpp"]);
        assert_eq!(cli.fir_fixture.as_deref(), Some("sine_phasor"));
        assert!(matches!(cli.lang, Some(CliLang::Cpp)));
    }

    #[test]
    fn cli_parse_accepts_dump_cpp_from_fbc() {
        let cli = CliArgs::parse_from(["faust-rs", "--dump-cpp-from-fbc", "foo.fbc"]);
        assert!(cli.dump_cpp_from_fbc);
        assert_eq!(cli.input.as_deref(), Some(Path::new("foo.fbc")));
    }

    #[test]
    fn cli_parse_accepts_cpp_class_name_with_dump_cpp_from_fbc() {
        let cli = CliArgs::parse_from([
            "faust-rs",
            "--dump-cpp-from-fbc",
            "--cpp-class-name",
            "my_dsp",
            "foo.fbc",
        ]);
        assert_eq!(cli.cpp_class_name.as_deref(), Some("my_dsp"));
    }

    #[test]
    fn help_mentions_faust_naming_aliases() {
        let mut command = CliArgs::command();
        let rendered = command.render_long_help().to_string();
        assert!(rendered.contains("-cn <name>"));
        assert!(rendered.contains("-scn <name>"));
        assert!(rendered.contains("-pn <name>"));
    }

    #[test]
    fn diagnostics_human_renderer_keeps_code_and_location() {
        let mut bundle = DiagnosticBundle::new();
        bundle.push(
            Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                DiagnosticCode("FRS-EVAL-0001"),
                "missing process",
            )
            .with_label(errors::Label::new(
                errors::LabelStyle::Primary,
                SourceSpan::new("test.dsp", 3, 7, 3, 12),
                "here",
            )),
        );

        let rendered = format_diagnostics_human(&bundle);
        assert!(rendered.contains("test.dsp:3:7"));
        assert!(rendered.contains("[FRS-EVAL-0001]"));
        assert!(rendered.contains("missing process"));
    }

    #[test]
    fn diagnostics_json_renderer_exposes_structured_fields() {
        let compiler = Compiler::new();
        let err = compiler
            .compile_source_to_signals("missing_process.dsp", "foo = _;")
            .expect_err("missing process should fail");
        let diagnostics = err
            .diagnostics()
            .expect("compiler errors should expose diagnostics");

        let rendered = format_diagnostics_json(diagnostics);
        let value: Value =
            serde_json::from_str(&rendered).expect("JSON diagnostics output should be valid");
        let first = &value["diagnostics"][0];
        assert_eq!(first["severity"], "error");
        assert_eq!(first["stage"], "eval");
        let code = first["code"].as_str().expect("code should be a string");
        assert!(code.starts_with("FRS-EVAL-"));
        assert!(first["message"].is_string());
        assert!(first["labels"].is_array());
    }

    #[test]
    fn diagnostics_human_renderer_snapshot_with_snippet_and_caret() {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "faust_rs_diag_human_{}_{}.dsp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should move forward")
                .as_nanos()
        ));
        std::fs::write(&path, "process = _,_ <: _,_,_;\n").expect("fixture should be written");

        let mut bundle = DiagnosticBundle::new();
        bundle.push(
            Diagnostic::new(
                Severity::Error,
                Stage::Propagate,
                DiagnosticCode("FRS-PROP-0002"),
                "split composition mismatch",
            )
            .with_label(errors::Label::new(
                errors::LabelStyle::Primary,
                SourceSpan::new(&path, 1, 13, 1, 15),
                "related source",
            ))
            .with_note("rule: split(A, B) requires inputs(B) % outputs(A) == 0")
            .with_note("computed: 3 % 2 = 1")
            .with_help("make B input count a multiple of A output count"),
        );

        let rendered = format_diagnostics_human(&bundle);
        let path_text = path.to_string_lossy().to_string();
        let normalized = rendered.replace(&path_text, "$TMPFILE");
        let expected = "\
$TMPFILE:1:13: error [FRS-PROP-0002] split composition mismatch
  1 | process = _,_ <: _,_,_;
    |             ^^ related source
  = note: rule: split(A, B) requires inputs(B) % outputs(A) == 0
  = note: computed: 3 % 2 = 1
  = help: make B input count a multiple of A output count
";
        assert_eq!(normalized, expected);

        std::fs::remove_file(path).expect("fixture should be removed");
    }

    #[test]
    fn diagnostics_json_renderer_snapshot_shape_stable() {
        let mut bundle = DiagnosticBundle::new();
        bundle.push(
            Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                DiagnosticCode("FRS-EVAL-0003"),
                "too many arguments",
            )
            .with_note("application accepts at most 1 argument(s), got 2")
            .with_help("remove one argument"),
        );

        let rendered = format_diagnostics_json(&bundle);
        let value: Value =
            serde_json::from_str(&rendered).expect("JSON diagnostics output should be valid");
        let diag = &value["diagnostics"][0];

        assert_eq!(diag["severity"], "error");
        assert_eq!(diag["stage"], "eval");
        assert_eq!(diag["code"], "FRS-EVAL-0003");
        assert_eq!(diag["message"], "too many arguments");
        assert!(diag["labels"].is_array());
        assert_eq!(
            diag["notes"][0],
            "application accepts at most 1 argument(s), got 2"
        );
        assert_eq!(diag["help"][0], "remove one argument");
    }

    #[test]
    fn diagnostics_json_renderer_debug_mode_exposes_internal_fields() {
        let mut bundle = DiagnosticBundle::new();
        bundle.push(
            Diagnostic::new(
                Severity::Error,
                Stage::Propagate,
                DiagnosticCode("FRS-PROP-0002"),
                "split mismatch",
            )
            .with_note("node_id=42")
            .with_note("box_expr=3(1(), 1())"),
        );
        let rendered = format_diagnostics_json_with_verbosity(&bundle, ErrorVerbosity::Debug);
        let value: Value =
            serde_json::from_str(&rendered).expect("JSON diagnostics output should be valid");
        let diag = &value["diagnostics"][0];
        assert_eq!(diag["debug"]["node_id"], 42);
        assert_eq!(diag["debug"]["box_expr"], "3(1(), 1())");
    }

    #[test]
    fn diagnostics_json_renderer_standard_mode_omits_internal_debug_fields() {
        let mut bundle = DiagnosticBundle::new();
        bundle.push(
            Diagnostic::new(
                Severity::Error,
                Stage::Propagate,
                DiagnosticCode("FRS-PROP-0002"),
                "split mismatch",
            )
            .with_note("node_id=42")
            .with_note("box_expr=3(1(), 1())"),
        );
        let rendered = format_diagnostics_json_with_verbosity(&bundle, ErrorVerbosity::Standard);
        let value: Value =
            serde_json::from_str(&rendered).expect("JSON diagnostics output should be valid");
        let diag = &value["diagnostics"][0];
        assert!(diag["debug"].is_null());
    }

    #[test]
    fn diagnostics_human_renderer_renders_cpp_style_a_b_block() {
        let mut bundle = DiagnosticBundle::new();
        bundle.push(
            Diagnostic::new(
                Severity::Error,
                Stage::Propagate,
                DiagnosticCode("FRS-PROP-0002"),
                "split composition mismatch",
            )
            .with_note("A (split left) = (_, _)")
            .with_note("B (split right) = (_, (_, _))")
            .with_note("A arity: inputs=2 outputs=2")
            .with_note("B arity: inputs=3 outputs=3"),
        );

        let rendered = format_diagnostics_human(&bundle);
        assert!(rendered.contains("Here  A = (_, _)"));
        assert!(rendered.contains("while B = (_, (_, _))"));
        assert!(rendered.contains("has inputs=2 outputs=2"));
        assert!(rendered.contains("has inputs=3 outputs=3"));
        assert!(!rendered.contains("A (split left) = "));
    }

    #[test]
    fn diagnostics_human_renderer_hides_internal_machine_notes() {
        let mut bundle = DiagnosticBundle::new();
        bundle.push(
            Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                DiagnosticCode("FRS-EVAL-0002"),
                "undefined symbol `x`",
            )
            .with_note("node_id=42")
            .with_note("box_expr=0(sym(\"x\"))")
            .with_note("expr=x"),
        );
        let rendered = format_diagnostics_human(&bundle);
        assert!(
            !rendered.contains("node_id=42"),
            "human mode should hide internal node ids"
        );
        assert!(
            !rendered.contains("box_expr=0(sym(\"x\"))"),
            "human mode should hide internal box previews"
        );
        assert!(
            rendered.contains("expr=x"),
            "human mode should keep readable expression context"
        );
    }

    #[test]
    fn diagnostics_human_renderer_debug_mode_keeps_internal_machine_notes() {
        let mut bundle = DiagnosticBundle::new();
        bundle.push(
            Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                DiagnosticCode("FRS-EVAL-0002"),
                "undefined symbol `x`",
            )
            .with_note("node_id=42")
            .with_note("box_expr=0(sym(\"x\"))")
            .with_note("expr=x"),
        );
        let rendered = format_diagnostics_human_with_verbosity(&bundle, ErrorVerbosity::Debug);
        assert!(rendered.contains("node_id=42"));
        assert!(rendered.contains("box_expr=0(sym(\"x\"))"));
        assert!(rendered.contains("expr=x"));
    }

    fn corpus_path(file: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("corpus")
            .join(file)
    }

    fn note_index<'a>(notes: &'a [&'a str], prefix: &str) -> usize {
        notes
            .iter()
            .position(|note| note.starts_with(prefix))
            .unwrap_or_else(|| panic!("missing note prefix `{prefix}` in {:?}", notes))
    }

    #[test]
    fn diagnostics_human_renderer_snapshots_cover_complex_phase4_failures() {
        let fixtures = [
            (
                "err_06_propagate_split_mismatch_chain.dsp",
                vec![
                    "error [FRS-PROP-0002] split composition mismatch",
                    "error originates from definition 'foo'",
                    "binding_trace=process -> baz -> bar -> foo",
                    "Here  A = (_, _)",
                    "while B = (_, (_, _))",
                    "suggested target: set inputs(B) to 4",
                ],
            ),
            (
                "err_07_propagate_rec_mismatch_alias.dsp",
                vec![
                    "error [FRS-PROP-0003] recursive composition mismatch",
                    "error originates from definition 'foo'",
                    "binding_trace=process -> bar -> foo",
                    "Here  A = _",
                    "while B = (_, (_, _))",
                    "suggested target: set outputs(A) >= 3 and inputs(A) >= 3",
                ],
            ),
            (
                "err_08_propagate_seq_ui_mismatch.dsp",
                vec![
                    "error [FRS-PROP-0002] sequential composition mismatch",
                    "cause: sequential composition bus widths do not match",
                    "error originates from definition 'foo'",
                    "binding_trace=process -> foo",
                    "Here  A = hslider(\"gain\", 0.5, 0, 1, 0.01)",
                    "while B = ",
                    "suggested target: make outputs(A) and inputs(B) equal (common target: 2)",
                ],
            ),
            (
                "err_16_propagate_compound_with_letrec_split.dsp",
                vec![
                    "error [FRS-PROP-0002] split composition mismatch",
                    "cause: split composition divisibility rule is violated",
                    "error originates from definition 'foo'",
                    "binding_trace=process -> baz -> bar -> foo",
                    "Here  A = (_, _)",
                    "while B = (_, (_, _))",
                    "template: process = A <: B; // inputs(B) % outputs(A) == 0",
                ],
            ),
        ];

        let compiler = Compiler::new();
        for (file, expected_lines) in fixtures {
            let path = corpus_path(file);
            let err = compiler
                .compile_file_default_to_signals(&path)
                .expect_err("fixture should fail in signal pipeline");
            let diagnostics = err
                .diagnostics()
                .expect("fixture error should expose diagnostics");
            let rendered = format_diagnostics_human(diagnostics);
            let path_text = path.to_string_lossy().to_string();
            let normalized = rendered.replace(&path_text, "$FIXTURE");
            for expected in expected_lines {
                assert!(
                    normalized.contains(expected),
                    "{file} human snapshot should contain: {expected}\nrendered:\n{normalized}"
                );
            }
            let source = fs::read_to_string(&path).expect("fixture source should be readable");
            let first_line = source
                .lines()
                .next()
                .expect("fixture should contain at least one line");
            assert!(
                normalized.contains(first_line),
                "{file} human snapshot should include source snippet line"
            );
        }
    }

    #[test]
    fn diagnostics_json_renderer_snapshots_cover_complex_phase4_failures() {
        let fixtures = [
            (
                "err_06_propagate_split_mismatch_chain.dsp",
                "binding_trace=process -> baz -> bar -> foo",
                "A (split left) = ",
                "B (split right) = ",
                "error originates from definition 'foo'",
                "suggested target: set inputs(B) to 4",
            ),
            (
                "err_07_propagate_rec_mismatch_alias.dsp",
                "binding_trace=process -> bar -> foo",
                "A (rec left) = ",
                "B (rec right) = ",
                "error originates from definition 'foo'",
                "suggested target: set outputs(A) >= 3 and inputs(A) >= 3",
            ),
            (
                "err_08_propagate_seq_ui_mismatch.dsp",
                "binding_trace=process -> foo",
                "A (seq left) = ",
                "B (seq right) = ",
                "error originates from definition 'foo'",
                "suggested target: make outputs(A) and inputs(B) equal (common target: 2)",
            ),
            (
                "err_16_propagate_compound_with_letrec_split.dsp",
                "binding_trace=process -> baz -> bar -> foo",
                "A (split left) = ",
                "B (split right) = ",
                "error originates from definition 'foo'",
                "suggested target: set inputs(B) to 4",
            ),
        ];

        let compiler = Compiler::new();
        for (file, trace, left_prefix, right_prefix, owner_note, suggestion_note) in fixtures {
            let path = corpus_path(file);
            let err = compiler
                .compile_file_default_to_signals(&path)
                .expect_err("fixture should fail in signal pipeline");
            let diagnostics = err
                .diagnostics()
                .expect("fixture error should expose diagnostics");
            let rendered = format_diagnostics_json(diagnostics);
            let value: Value =
                serde_json::from_str(&rendered).expect("JSON diagnostics output should be valid");
            let diag = &value["diagnostics"][0];
            let notes = diag["notes"]
                .as_array()
                .expect("notes should be an array")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            assert!(
                notes.contains(&trace),
                "{file} json snapshot should contain trace note"
            );
            assert!(
                notes.iter().any(|note| note.starts_with(left_prefix)),
                "{file} json snapshot should contain left-side note"
            );
            assert!(
                notes.iter().any(|note| note.starts_with(right_prefix)),
                "{file} json snapshot should contain right-side note"
            );
            assert!(
                notes.contains(&owner_note),
                "{file} json snapshot should contain owner note"
            );
            assert!(
                notes.iter().any(|note| note.starts_with(suggestion_note)),
                "{file} json snapshot should contain numeric suggestion note"
            );
        }
    }

    #[test]
    fn diagnostics_human_renderer_snapshot_for_eval_undefined_symbol() {
        let compiler = Compiler::new();
        let path = corpus_path("err_09_eval_undefined_symbol.dsp");
        let err = compiler
            .compile_file_default_to_signals(&path)
            .expect_err("fixture should fail in eval stage");
        let diagnostics = err
            .diagnostics()
            .expect("fixture error should expose diagnostics");
        let rendered = format_diagnostics_human(diagnostics);
        let path_text = path.to_string_lossy().to_string();
        let normalized = rendered.replace(&path_text, "$FIXTURE");

        assert!(normalized.contains("error [FRS-EVAL-0002] undefined symbol `bar`"));
        assert!(normalized.contains("error originates from definition 'foo'"));
        assert!(normalized.contains("binding_trace=process -> foo"));
        assert!(normalized.contains("expr=bar"));
        assert!(normalized.contains("define the symbol in scope or fix the identifier name"));
    }

    #[test]
    fn diagnostics_human_renderer_snapshot_for_eval_undefined_symbol_alias_chain() {
        let compiler = Compiler::new();
        let path = corpus_path("err_13_eval_undefined_symbol_alias_chain_nested.dsp");
        let err = compiler
            .compile_file_default_to_signals(&path)
            .expect_err("fixture should fail in eval stage");
        let diagnostics = err
            .diagnostics()
            .expect("fixture error should expose diagnostics");
        let rendered = format_diagnostics_human(diagnostics);
        let path_text = path.to_string_lossy().to_string();
        let normalized = rendered.replace(&path_text, "$FIXTURE");

        assert!(normalized.contains("error [FRS-EVAL-0002] undefined symbol `z`"));
        assert!(normalized.contains("cause: unresolved identifier in current lexical scope"));
        assert!(normalized.contains("error originates from definition 'foo'"));
        assert!(normalized.contains("binding_trace=process -> baz -> bar -> foo"));
        assert!(
            normalized
                .contains("rule: referenced identifier must be present in visible lexical scope")
        );
        assert!(normalized.contains("template: z = ...; // define before use"));
    }

    #[test]
    fn diagnostics_json_renderer_snapshot_for_eval_undefined_symbol() {
        let compiler = Compiler::new();
        let path = corpus_path("err_09_eval_undefined_symbol.dsp");
        let err = compiler
            .compile_file_default_to_signals(&path)
            .expect_err("fixture should fail in eval stage");
        let diagnostics = err
            .diagnostics()
            .expect("fixture error should expose diagnostics");
        let rendered = format_diagnostics_json(diagnostics);
        let value: Value =
            serde_json::from_str(&rendered).expect("JSON diagnostics output should be valid");
        let diag = &value["diagnostics"][0];
        let notes = diag["notes"]
            .as_array()
            .expect("notes should be an array")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(diag["code"], "FRS-EVAL-0002");
        assert!(notes.iter().any(|n| n.starts_with("expr=")));
        assert!(notes.contains(&"error originates from definition 'foo'"));
        assert!(notes.contains(&"binding_trace=process -> foo"));
    }

    #[test]
    fn diagnostics_json_renderer_snapshot_for_eval_undefined_symbol_alias_chain() {
        let compiler = Compiler::new();
        let path = corpus_path("err_13_eval_undefined_symbol_alias_chain_nested.dsp");
        let err = compiler
            .compile_file_default_to_signals(&path)
            .expect_err("fixture should fail in eval stage");
        let diagnostics = err
            .diagnostics()
            .expect("fixture error should expose diagnostics");
        let rendered = format_diagnostics_json(diagnostics);
        let value: Value =
            serde_json::from_str(&rendered).expect("JSON diagnostics output should be valid");
        let diag = &value["diagnostics"][0];
        let labels = diag["labels"]
            .as_array()
            .expect("labels should be an array");
        let notes = diag["notes"]
            .as_array()
            .expect("notes should be an array")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();

        assert_eq!(diag["code"], "FRS-EVAL-0002");
        assert_eq!(labels[0]["role"], "definition_site");
        if labels.len() >= 2 {
            assert_eq!(labels[1]["role"], "call_site");
        }
        assert!(
            notes
                .iter()
                .any(|n| n.starts_with("cause: unresolved identifier"))
        );
        assert!(notes.contains(&"binding_trace=process -> baz -> bar -> foo"));

        let cause_i = note_index(&notes, "cause:");
        let rule_i = note_index(&notes, "rule:");
        let computed_i = note_index(&notes, "computed:");
        let context_i = note_index(&notes, "scope.local=");
        assert!(
            cause_i < rule_i && rule_i < computed_i && computed_i < context_i,
            "eval JSON note ordering should be cause -> rule -> computed -> context"
        );
    }

    #[test]
    fn diagnostics_json_renderer_note_order_for_propagate_split_compound() {
        let compiler = Compiler::new();
        let path = corpus_path("err_16_propagate_compound_with_letrec_split.dsp");
        let err = compiler
            .compile_file_default_to_signals(&path)
            .expect_err("fixture should fail in propagate stage");
        let diagnostics = err
            .diagnostics()
            .expect("fixture error should expose diagnostics");
        let rendered = format_diagnostics_json(diagnostics);
        let value: Value =
            serde_json::from_str(&rendered).expect("JSON diagnostics output should be valid");
        let diag = &value["diagnostics"][0];
        let notes = diag["notes"]
            .as_array()
            .expect("notes should be an array")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        let cause_i = note_index(&notes, "cause:");
        let rule_i = note_index(&notes, "rule:");
        let computed_i = note_index(&notes, "computed:");
        let context_i = note_index(&notes, "A (split left) = ");
        assert!(
            cause_i < rule_i && rule_i < computed_i && computed_i < context_i,
            "propagate JSON note ordering should be cause -> rule -> computed -> context"
        );
    }

    #[test]
    fn diagnostics_json_renderer_note_order_for_propagate_merge_alias() {
        let compiler = Compiler::new();
        let path = corpus_path("err_05_propagate_merge_mismatch_alias.dsp");
        let err = compiler
            .compile_file_default_to_signals(&path)
            .expect_err("fixture should fail in propagate stage");
        let diagnostics = err
            .diagnostics()
            .expect("fixture error should expose diagnostics");
        let rendered = format_diagnostics_json(diagnostics);
        let value: Value =
            serde_json::from_str(&rendered).expect("JSON diagnostics output should be valid");
        let diag = &value["diagnostics"][0];
        let notes = diag["notes"]
            .as_array()
            .expect("notes should be an array")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        let cause_i = note_index(&notes, "cause:");
        let rule_i = note_index(&notes, "rule:");
        let computed_i = note_index(&notes, "computed:");
        let context_i = note_index(&notes, "A (merge left) = ");
        assert!(cause_i < rule_i && rule_i < computed_i && computed_i < context_i);
    }

    #[test]
    fn diagnostics_json_renderer_note_order_for_propagate_rec_alias() {
        let compiler = Compiler::new();
        let path = corpus_path("err_07_propagate_rec_mismatch_alias.dsp");
        let err = compiler
            .compile_file_default_to_signals(&path)
            .expect_err("fixture should fail in propagate stage");
        let diagnostics = err
            .diagnostics()
            .expect("fixture error should expose diagnostics");
        let rendered = format_diagnostics_json(diagnostics);
        let value: Value =
            serde_json::from_str(&rendered).expect("JSON diagnostics output should be valid");
        let diag = &value["diagnostics"][0];
        let notes = diag["notes"]
            .as_array()
            .expect("notes should be an array")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        let cause_i = note_index(&notes, "cause:");
        let rule_i = note_index(&notes, "rule:");
        let computed_i = note_index(&notes, "computed:");
        let context_i = note_index(&notes, "A (rec left) = ");
        assert!(cause_i < rule_i && rule_i < computed_i && computed_i < context_i);
    }

    #[test]
    fn diagnostics_human_renderer_snapshot_for_pipeline_origin_fallback() {
        let compiler = Compiler::new();
        let source =
            fs::read_to_string(corpus_path("err_17_origin_fallback_missing_props_eval.dsp"))
                .expect("fixture should be readable");
        let mut parsed =
            parser::parse_program(&source, "err_17_origin_fallback_missing_props_eval.dsp");
        parsed.state.ctx = parser::ParserCtx::new();
        let err = compiler
            .compile_parsed_to_signals("err_17_origin_fallback_missing_props_eval.dsp", parsed)
            .expect_err("fixture should fail in eval stage");
        let diagnostics = err
            .diagnostics()
            .expect("fixture error should expose diagnostics");
        let rendered = format_diagnostics_human(diagnostics);
        assert!(rendered.contains("origin span unavailable; pointing to nearest call/owner site"));
    }

    #[test]
    fn diagnostics_human_renderer_compound_case_fixture_now_compiles() {
        let compiler = Compiler::new();
        let path = corpus_path("err_15_eval_compound_with_letrec_case_arity.dsp");
        let out = compiler
            .compile_file_default_to_signals(&path)
            .expect("fixture should now compile to signals");
        assert_eq!(out.process_arity.inputs, 1);
        assert_eq!(out.process_arity.outputs, 1);
        assert_eq!(out.signals.len(), 1);
        assert_eq!(
            match_sig(&out.parse.state.arena, out.signals[0]),
            SigMatch::Int(1)
        );
    }
}
