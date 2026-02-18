use boxes::dump_box;
use codegen::backends::cpp::CppOptions;
use compiler::{Compiler, CompilerError, SignalFirLane, golden_snapshot_from_file};
use errors::{DiagnosticBundle, LabelStyle, Severity, Stage};
use serde_json::json;
use signals::dump_sig_readable;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ErrorFormat {
    #[default]
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ErrorVerbosity {
    #[default]
    Standard,
    Debug,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CliSignalFirLane {
    #[default]
    Legacy,
    Fast,
}

impl CliSignalFirLane {
    fn into_compiler_lane(self) -> SignalFirLane {
        match self {
            Self::Legacy => SignalFirLane::LegacyBridge,
            Self::Fast => SignalFirLane::TransformFastLane,
        }
    }
}

fn parse_input_with_import_dirs_and_format(
    mut args: impl Iterator<Item = String>,
    usage: &str,
) -> (PathBuf, Vec<PathBuf>, ErrorFormat, ErrorVerbosity) {
    let Some(input) = args.next() else {
        eprintln!("{usage}");
        std::process::exit(2);
    };

    let mut search_paths = Vec::new();
    let mut error_format = ErrorFormat::Human;
    let mut error_verbosity = ErrorVerbosity::Standard;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "-I" | "--import-dir" => {
                let Some(dir) = args.next() else {
                    eprintln!("{usage}");
                    std::process::exit(2);
                };
                search_paths.push(PathBuf::from(dir));
            }
            "--error-format" => {
                let Some(format) = args.next() else {
                    eprintln!("{usage}");
                    std::process::exit(2);
                };
                error_format = match format.as_str() {
                    "human" => ErrorFormat::Human,
                    "json" => ErrorFormat::Json,
                    _ => {
                        eprintln!("{usage}");
                        std::process::exit(2);
                    }
                };
            }
            "--error-verbosity" => {
                let Some(level) = args.next() else {
                    eprintln!("{usage}");
                    std::process::exit(2);
                };
                error_verbosity = match level.as_str() {
                    "standard" => ErrorVerbosity::Standard,
                    "debug" => ErrorVerbosity::Debug,
                    _ => {
                        eprintln!("{usage}");
                        std::process::exit(2);
                    }
                };
            }
            _ => {
                eprintln!("{usage}");
                std::process::exit(2);
            }
        }
    }

    (
        PathBuf::from(input),
        search_paths,
        error_format,
        error_verbosity,
    )
}

fn parse_dump_cpp_input(
    mut args: impl Iterator<Item = String>,
    usage: &str,
) -> (
    PathBuf,
    Vec<PathBuf>,
    CliSignalFirLane,
    ErrorFormat,
    ErrorVerbosity,
) {
    let Some(input) = args.next() else {
        eprintln!("{usage}");
        std::process::exit(2);
    };

    let mut search_paths = Vec::new();
    let mut lane = CliSignalFirLane::Legacy;
    let mut error_format = ErrorFormat::Human;
    let mut error_verbosity = ErrorVerbosity::Standard;

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "-I" | "--import-dir" => {
                let Some(dir) = args.next() else {
                    eprintln!("{usage}");
                    std::process::exit(2);
                };
                search_paths.push(PathBuf::from(dir));
            }
            "--signal-fir-lane" => {
                let Some(v) = args.next() else {
                    eprintln!("{usage}");
                    std::process::exit(2);
                };
                lane = match v.as_str() {
                    "legacy" => CliSignalFirLane::Legacy,
                    "fast" => CliSignalFirLane::Fast,
                    _ => {
                        eprintln!("{usage}");
                        std::process::exit(2);
                    }
                };
            }
            "--error-format" => {
                let Some(format) = args.next() else {
                    eprintln!("{usage}");
                    std::process::exit(2);
                };
                error_format = match format.as_str() {
                    "human" => ErrorFormat::Human,
                    "json" => ErrorFormat::Json,
                    _ => {
                        eprintln!("{usage}");
                        std::process::exit(2);
                    }
                };
            }
            "--error-verbosity" => {
                let Some(level) = args.next() else {
                    eprintln!("{usage}");
                    std::process::exit(2);
                };
                error_verbosity = match level.as_str() {
                    "standard" => ErrorVerbosity::Standard,
                    "debug" => ErrorVerbosity::Debug,
                    _ => {
                        eprintln!("{usage}");
                        std::process::exit(2);
                    }
                };
            }
            _ => {
                eprintln!("{usage}");
                std::process::exit(2);
            }
        }
    }

    (
        PathBuf::from(input),
        search_paths,
        lane,
        error_format,
        error_verbosity,
    )
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct PairedContext {
    a_expr: String,
    b_expr: String,
    a_arity: Option<String>,
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

fn parse_dump_usage(mode: &str) -> String {
    if mode == "dump-cpp" {
        "Usage: cargo run -p compiler -- --dump-cpp <input.dsp> [-I <dir> ...] [--signal-fir-lane legacy|fast] [--error-format human|json] [--error-verbosity standard|debug]"
            .to_owned()
    } else {
        format!(
            "Usage: cargo run -p compiler -- --{mode} <input.dsp> [-I <dir> ...] [--error-format human|json] [--error-verbosity standard|debug]"
        )
    }
}

fn parse_usage() -> String {
    "Usage: cargo run -p compiler -- --parse <input.dsp> [-I <dir> ...] [--error-format human|json] [--error-verbosity standard|debug]"
        .to_owned()
}

fn print_global_usage_and_exit() -> ! {
    eprintln!("Usage:");
    eprintln!("  cargo run -p compiler -- --golden <input.dsp>");
    eprintln!(
        "  cargo run -p compiler -- --parse <input.dsp> [-I <dir> ...] [--error-format human|json] [--error-verbosity standard|debug]"
    );
    eprintln!(
        "  cargo run -p compiler -- --dump-box <input.dsp> [-I <dir> ...] [--error-format human|json] [--error-verbosity standard|debug]"
    );
    eprintln!(
        "  cargo run -p compiler -- --dump-sig <input.dsp> [-I <dir> ...] [--error-format human|json] [--error-verbosity standard|debug]"
    );
    eprintln!(
        "  cargo run -p compiler -- --dump-cpp <input.dsp> [-I <dir> ...] [--signal-fir-lane legacy|fast] [--error-format human|json] [--error-verbosity standard|debug]"
    );
    std::process::exit(2);
}

fn maybe_print_error_format_help(args: &[String]) {
    if args.iter().any(|arg| arg == "--help-error-format") {
        println!("--error-format human|json");
        println!("--error-verbosity standard|debug");
        println!("  human: file:line:col severity [CODE] message");
        println!("  json: structured diagnostics payload for CI/IDE tooling");
        println!("  standard: concise human notes, hides internal ids");
        println!("  debug: keeps full internal notes in human mode");
        std::process::exit(0);
    }
}

fn main() {
    let argv = std::env::args().skip(1).collect::<Vec<_>>();
    maybe_print_error_format_help(&argv);
    let mut args = argv.into_iter();
    match args.next().as_deref() {
        Some("--golden") => {
            let Some(input) = args.next() else {
                eprintln!("Usage: cargo run -p compiler -- --golden <input.dsp>");
                std::process::exit(2);
            };

            if args.next().is_some() {
                eprintln!("Usage: cargo run -p compiler -- --golden <input.dsp>");
                std::process::exit(2);
            }

            let input_path = PathBuf::from(input);
            match golden_snapshot_from_file(&input_path) {
                Ok(snapshot) => {
                    print!("{snapshot}");
                }
                Err(err) => {
                    eprintln!("Failed to create golden snapshot: {err}");
                    std::process::exit(1);
                }
            }
        }
        Some("--parse") => {
            let usage = parse_usage();
            let (input_path, search_paths, error_format, error_verbosity) =
                parse_input_with_import_dirs_and_format(args, &usage);
            let compiler = Compiler::new();
            let result = if search_paths.is_empty() {
                compiler.compile_file_default(&input_path)
            } else {
                compiler.compile_file(&input_path, &search_paths)
            };

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
                    print_structured_diagnostics(&err, error_format, error_verbosity);
                    std::process::exit(1);
                }
            }
        }
        Some("--dump-box") => {
            let usage = parse_dump_usage("dump-box");
            let (input_path, search_paths, error_format, error_verbosity) =
                parse_input_with_import_dirs_and_format(args, &usage);
            let compiler = Compiler::new();
            let result = if search_paths.is_empty() {
                compiler.compile_file_default(&input_path)
            } else {
                compiler.compile_file(&input_path, &search_paths)
            };

            match result {
                Ok(out) => {
                    let Some(root) = out.root else {
                        eprintln!("Parse failed: no root node produced");
                        std::process::exit(1);
                    };
                    println!("{}", dump_box(&out.state.arena, root));
                }
                Err(err) => {
                    eprintln!("Parse failed: {err}");
                    print_structured_diagnostics(&err, error_format, error_verbosity);
                    std::process::exit(1);
                }
            }
        }
        Some("--dump-sig") => {
            let usage = parse_dump_usage("dump-sig");
            let (input_path, search_paths, error_format, error_verbosity) =
                parse_input_with_import_dirs_and_format(args, &usage);
            let compiler = Compiler::new();
            let result = if search_paths.is_empty() {
                compiler.compile_file_default_to_signals(&input_path)
            } else {
                compiler.compile_file_to_signals(&input_path, &search_paths)
            };

            match result {
                Ok(out) => {
                    println!(
                        "Signals OK: inputs={} outputs={}",
                        out.process_arity.inputs, out.process_arity.outputs
                    );
                    for (index, sig) in out.signals.iter().enumerate() {
                        println!(
                            "[{index}] {}",
                            dump_sig_readable(&out.parse.state.arena, *sig)
                        );
                    }
                }
                Err(err) => {
                    eprintln!("Signal pipeline failed: {err}");
                    print_structured_diagnostics(&err, error_format, error_verbosity);
                    std::process::exit(1);
                }
            }
        }
        Some("--dump-cpp") => {
            let usage = parse_dump_usage("dump-cpp");
            let (input_path, search_paths, lane, error_format, error_verbosity) =
                parse_dump_cpp_input(args, &usage);
            let compiler = Compiler::new();
            let options = CppOptions::default();
            let result = if search_paths.is_empty() {
                compiler.compile_file_default_to_cpp_with_lane(
                    &input_path,
                    &options,
                    lane.into_compiler_lane(),
                )
            } else {
                compiler.compile_file_to_cpp_with_lane(
                    &input_path,
                    &search_paths,
                    &options,
                    lane.into_compiler_lane(),
                )
            };

            match result {
                Ok(cpp) => {
                    print!("{cpp}");
                }
                Err(err) => {
                    eprintln!("C++ pipeline failed: {err}");
                    print_structured_diagnostics(&err, error_format, error_verbosity);
                    std::process::exit(1);
                }
            }
        }
        None => {
            println!("faust-rs compiler scaffold v{}", Compiler::version());
        }
        Some(_) => {
            print_global_usage_and_exit();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use compiler::Compiler;
    use errors::{Diagnostic, DiagnosticBundle, DiagnosticCode, Severity, SourceSpan, Stage};
    use serde_json::Value;

    use super::{
        ErrorVerbosity, format_diagnostics_human, format_diagnostics_human_with_verbosity,
        format_diagnostics_json, format_diagnostics_json_with_verbosity,
    };

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
    fn diagnostics_human_renderer_snapshot_for_compound_case_propagate_fallback() {
        let compiler = Compiler::new();
        let path = corpus_path("err_15_eval_compound_with_letrec_case_arity.dsp");
        let err = compiler
            .compile_file_default_to_signals(&path)
            .expect_err("fixture should fail in propagate stage");
        let diagnostics = err
            .diagnostics()
            .expect("fixture error should expose diagnostics");
        let rendered = format_diagnostics_human(diagnostics);
        let path_text = path.to_string_lossy().to_string();
        let normalized = rendered.replace(&path_text, "$FIXTURE");

        assert!(normalized.contains("error [FRS-PROP-0001] unsupported box node"));
        assert!(normalized.contains("cause: encountered box node family is not supported"));
        assert!(normalized.contains("binding_trace=process -> bar -> foo"));
    }
}
