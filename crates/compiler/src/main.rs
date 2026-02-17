use boxes::dump_box;
use compiler::{Compiler, CompilerError, golden_snapshot_from_file};
use errors::{DiagnosticBundle, LabelStyle, Severity, Stage};
use serde_json::json;
use signals::dump_sig_readable;
use std::path::PathBuf;

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
        } else {
            out.push_str(&format!("{severity} [{}] {}\n", diag.code.0, diag.message));
        }
    }
    out
}

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
}
