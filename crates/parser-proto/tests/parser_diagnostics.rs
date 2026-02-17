use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use errors::Stage;
use parser_proto::{DiagnosticSeverity, parse_program};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RustClass {
    Ok,
    Recovered,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CppClass {
    Ok,
    ParseError,
    OtherError,
}

#[derive(Debug, Clone, Copy)]
struct MalformedCase {
    name: &'static str,
    source: &'static str,
    expected_error_line: u32,
}

fn rust_class_for(source: &str, source_name: &str) -> RustClass {
    let out = parse_program(source, source_name);
    if out.root.is_none() {
        RustClass::Error
    } else if out.state.ctx.parse_error_count() == 0 && out.errors.is_empty() {
        RustClass::Ok
    } else {
        RustClass::Recovered
    }
}

fn cpp_bin() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FAUST_CPP_BIN") {
        return Some(PathBuf::from(path));
    }
    let default = PathBuf::from("/usr/local/bin/faust");
    if default.exists() {
        Some(default)
    } else {
        None
    }
}

fn cpp_class_for(cpp_bin: &Path, source: &str, case_name: &str) -> Result<CppClass, String> {
    let mut input_path = std::env::temp_dir();
    input_path.push(format!(
        "faust_rs_parser_diag_{}_{}.dsp",
        std::process::id(),
        case_name
    ));
    let mut out_path = std::env::temp_dir();
    out_path.push(format!(
        "faust_rs_parser_diag_{}_{}.c",
        std::process::id(),
        case_name
    ));
    fs::write(&input_path, source).map_err(|e| format!("write input failed: {e}"))?;

    let output = Command::new(cpp_bin)
        .arg(&input_path)
        .arg("-lang")
        .arg("c")
        .arg("-o")
        .arg(&out_path)
        .output()
        .map_err(|e| format!("failed to run {}: {e}", cpp_bin.display()))?;

    let _ = fs::remove_file(&input_path);
    let _ = fs::remove_file(&out_path);

    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let text_lc = text.to_lowercase();

    if output.status.success() {
        Ok(CppClass::Ok)
    } else if text_lc.contains("error") {
        Ok(CppClass::ParseError)
    } else {
        Ok(CppClass::OtherError)
    }
}

#[test]
fn malformed_suite_tracks_rust_class_and_location() {
    let cases = [
        MalformedCase {
            name: "missing_rhs_line1",
            source: "process = ;\nprocess = _;\n",
            expected_error_line: 1,
        },
        MalformedCase {
            name: "missing_rhs_line2",
            source: "a = _;\nb = ;\nprocess = _;\n",
            expected_error_line: 2,
        },
        MalformedCase {
            name: "missing_rpar",
            source: "process = hslider(\"g\", 0.5, 0.0, 1.0, 0.01;\n",
            expected_error_line: 1,
        },
        MalformedCase {
            name: "declare_missing_value",
            source: "declare author ;\nprocess = _;\n",
            expected_error_line: 1,
        },
        MalformedCase {
            name: "modulation_missing_rcroc",
            source: "process = [\"gain\" : _ -> _;\n",
            expected_error_line: 1,
        },
    ];

    for case in cases {
        let out = parse_program(case.source, case.name);
        let rust_class = rust_class_for(case.source, case.name);
        assert_ne!(
            rust_class,
            RustClass::Ok,
            "malformed case {} unexpectedly parsed as Ok",
            case.name
        );
        assert!(
            out.state.ctx.parse_error_count() > 0 || !out.errors.is_empty(),
            "malformed case {} should emit parser errors",
            case.name
        );
        assert!(
            !out.diagnostics.is_empty(),
            "malformed case {} should emit structured diagnostics",
            case.name
        );
        assert!(
            out.diagnostics.as_slice().iter().any(|d| {
                d.stage == Stage::Parser && d.code.0.starts_with("FRS-PARSE-")
            }),
            "malformed case {} should include parser diagnostic code family",
            case.name
        );
        assert!(
            out.state.ctx.recovery_count() > 0
                || out
                    .errors
                    .iter()
                    .any(|e| e.to_ascii_lowercase().contains("error")),
            "malformed case {} should hit either recovery path or lrpar error path",
            case.name
        );

        let has_location = out.state.ctx.diagnostics().iter().any(|d| {
            d.severity == DiagnosticSeverity::Error
                && d.location.as_ref().is_some_and(|loc| {
                    loc.file() == case.name && loc.line() == case.expected_error_line
                })
        });
        assert!(
            has_location,
            "missing error diagnostic location for case {} at line {}",
            case.name, case.expected_error_line
        );
        assert!(
            out.diagnostics.as_slice().iter().any(|d| {
                d.labels.iter().any(|label| {
                    label.span.file.to_string_lossy() == case.name
                        && label.span.line == case.expected_error_line
                        && label.span.col >= 1
                        && label.span.end_line >= label.span.line
                        && label.span.end_col >= label.span.col
                })
            }),
            "missing structured parser range for case {} at line {}",
            case.name,
            case.expected_error_line
        );
    }
}

#[test]
fn malformed_suite_matches_cpp_error_envelope_when_available() {
    let Some(cpp_bin) = cpp_bin() else {
        eprintln!(
            "Skipping C++ malformed envelope check: FAUST_CPP_BIN not set and /usr/local/bin/faust not found"
        );
        return;
    };
    if !cpp_bin.exists() {
        eprintln!(
            "Skipping C++ malformed envelope check: C++ binary not found at {}",
            cpp_bin.display()
        );
        return;
    }

    let cases = [
        ("missing_rhs_line1", "process = ;\nprocess = _;\n"),
        ("missing_rhs_line2", "a = _;\nb = ;\nprocess = _;\n"),
        (
            "missing_rpar",
            "process = hslider(\"g\", 0.5, 0.0, 1.0, 0.01;\n",
        ),
        ("declare_missing_value", "declare author ;\nprocess = _;\n"),
        (
            "modulation_missing_rcroc",
            "process = [\"gain\" : _ -> _;\n",
        ),
    ];

    for (name, source) in cases {
        let rust_class = rust_class_for(source, name);
        let cpp_class =
            cpp_class_for(&cpp_bin, source, name).unwrap_or_else(|e| panic!("C++ run failed: {e}"));
        assert_ne!(
            rust_class,
            RustClass::Ok,
            "malformed case {} unexpectedly Rust=Ok",
            name
        );
        assert_ne!(
            cpp_class,
            CppClass::Ok,
            "malformed case {} unexpectedly C++=Ok",
            name
        );
    }
}
