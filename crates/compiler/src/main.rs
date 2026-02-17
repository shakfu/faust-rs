use boxes::dump_box;
use compiler::{Compiler, CompilerError, golden_snapshot_from_file};
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

fn parse_input_with_import_dirs_and_format(
    mut args: impl Iterator<Item = String>,
    usage: &str,
) -> (PathBuf, Vec<PathBuf>, ErrorFormat) {
    let Some(input) = args.next() else {
        eprintln!("{usage}");
        std::process::exit(2);
    };

    let mut search_paths = Vec::new();
    let mut error_format = ErrorFormat::Human;
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
            _ => {
                eprintln!("{usage}");
                std::process::exit(2);
            }
        }
    }

    (PathBuf::from(input), search_paths, error_format)
}

fn print_structured_diagnostics(err: &CompilerError, format: ErrorFormat) {
    let Some(bundle) = err.diagnostics() else {
        return;
    };
    match format {
        ErrorFormat::Human => {
            eprint!("{}", format_diagnostics_human(bundle));
        }
        ErrorFormat::Json => {
            eprintln!("{}", format_diagnostics_json(bundle));
        }
    }
}

/// Formats diagnostics in a human-oriented form.
///
/// When a primary label is available and its source file can be read, this renderer
/// includes a source snippet line and a caret span.
fn format_diagnostics_human(bundle: &DiagnosticBundle) -> String {
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

        for note in &diag.notes {
            out.push_str(&format!("  = note: {note}\n"));
        }
        for help in &diag.help {
            out.push_str(&format!("  = help: {help}\n"));
        }
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
    let diagnostics = bundle
        .as_slice()
        .iter()
        .map(|diag| {
            let labels = diag
                .labels
                .iter()
                .map(|label| {
                    json!({
                        "style": match label.style {
                            LabelStyle::Primary => "primary",
                            LabelStyle::Secondary => "secondary",
                        },
                        "file": label.span.file.display().to_string(),
                        "line": label.span.line,
                        "col": label.span.col,
                        "end_line": label.span.end_line,
                        "end_col": label.span.end_col,
                        "message": label.message,
                    })
                })
                .collect::<Vec<_>>();
            json!({
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
            })
        })
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&json!({ "diagnostics": diagnostics }))
        .expect("diagnostics JSON formatting should not fail")
}

fn parse_dump_usage(mode: &str) -> String {
    format!(
        "Usage: cargo run -p compiler -- --{mode} <input.dsp> [-I <dir> ...] [--error-format human|json]"
    )
}

fn parse_usage() -> String {
    "Usage: cargo run -p compiler -- --parse <input.dsp> [-I <dir> ...] [--error-format human|json]"
        .to_owned()
}

fn print_global_usage_and_exit() -> ! {
    eprintln!("Usage:");
    eprintln!("  cargo run -p compiler -- --golden <input.dsp>");
    eprintln!(
        "  cargo run -p compiler -- --parse <input.dsp> [-I <dir> ...] [--error-format human|json]"
    );
    eprintln!(
        "  cargo run -p compiler -- --dump-box <input.dsp> [-I <dir> ...] [--error-format human|json]"
    );
    eprintln!(
        "  cargo run -p compiler -- --dump-sig <input.dsp> [-I <dir> ...] [--error-format human|json]"
    );
    std::process::exit(2);
}

fn maybe_print_error_format_help(args: &[String]) {
    if args.iter().any(|arg| arg == "--help-error-format") {
        println!("--error-format human|json");
        println!("  human: file:line:col severity [CODE] message");
        println!("  json: structured diagnostics payload for CI/IDE tooling");
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
            let (input_path, search_paths, error_format) =
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
                    print_structured_diagnostics(&err, error_format);
                    std::process::exit(1);
                }
            }
        }
        Some("--dump-box") => {
            let usage = parse_dump_usage("dump-box");
            let (input_path, search_paths, error_format) =
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
                    print_structured_diagnostics(&err, error_format);
                    std::process::exit(1);
                }
            }
        }
        Some("--dump-sig") => {
            let usage = parse_dump_usage("dump-sig");
            let (input_path, search_paths, error_format) =
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
                    print_structured_diagnostics(&err, error_format);
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

    use super::{format_diagnostics_human, format_diagnostics_json};

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

    fn corpus_path(file: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("corpus")
            .join(file)
    }

    #[test]
    fn diagnostics_human_renderer_snapshots_cover_complex_phase4_failures() {
        let fixtures = [
            (
                "err_06_propagate_split_mismatch_chain.dsp",
                vec![
                    "error [FRS-PROP-0002] split composition mismatch",
                    "binding_trace=process -> baz -> bar -> foo",
                    "A (split left) = (_, _)",
                    "B (split right) = (_, (_, _))",
                ],
            ),
            (
                "err_07_propagate_rec_mismatch_alias.dsp",
                vec![
                    "error [FRS-PROP-0003] recursive composition mismatch",
                    "binding_trace=process -> bar -> foo",
                    "A (rec left) = _",
                    "B (rec right) = (_, (_, _))",
                ],
            ),
            (
                "err_08_propagate_seq_ui_mismatch.dsp",
                vec![
                    "error [FRS-PROP-0002] sequential composition mismatch",
                    "binding_trace=process -> foo",
                    "A (seq left) = ",
                    "B (seq right) = ",
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
            ),
            (
                "err_07_propagate_rec_mismatch_alias.dsp",
                "binding_trace=process -> bar -> foo",
                "A (rec left) = ",
                "B (rec right) = ",
            ),
            (
                "err_08_propagate_seq_ui_mismatch.dsp",
                "binding_trace=process -> foo",
                "A (seq left) = ",
                "B (seq right) = ",
            ),
        ];

        let compiler = Compiler::new();
        for (file, trace, left_prefix, right_prefix) in fixtures {
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
                notes.iter().any(|note| *note == trace),
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
        }
    }
}
