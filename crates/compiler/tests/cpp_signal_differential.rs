//! Integration tests for `cpp_signal_differential`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use compiler::Compiler;

const CPP_SOURCE_ROOT: &str = "/Users/letz/Developpements/RUST/faust";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusClass {
    Ok,
    Error,
}

#[derive(Debug)]
enum CaseInput {
    CorpusFile(&'static str),
    Inline(&'static str),
}

#[derive(Debug)]
struct Case {
    name: &'static str,
    input: CaseInput,
    expect_valid: bool,
}

fn corpus_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("corpus")
        .join(file)
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

fn git_head_short(path: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("rev-parse")
        .arg("--short")
        .arg("HEAD")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let txt = String::from_utf8(out.stdout).ok()?;
    Some(txt.trim().to_owned())
}

fn temp_input_path(case_name: &str) -> PathBuf {
    let mut input_path = std::env::temp_dir();
    input_path.push(format!(
        "faust_rs_signal_diff_{}_{}.dsp",
        std::process::id(),
        case_name
    ));
    input_path
}

fn rust_status_for_case(compiler: &Compiler, case: &Case) -> StatusClass {
    let res = match case.input {
        CaseInput::CorpusFile(file) => {
            let path = corpus_path(file);
            compiler.compile_file_default_to_signals(&path)
        }
        CaseInput::Inline(source) => compiler.compile_source_to_signals(case.name, source),
    };
    if res.is_ok() {
        StatusClass::Ok
    } else {
        StatusClass::Error
    }
}

fn cpp_status_for_case(cpp_bin: &Path, case: &Case) -> Result<StatusClass, String> {
    let input_path = match case.input {
        CaseInput::CorpusFile(file) => corpus_path(file),
        CaseInput::Inline(source) => {
            let path = temp_input_path(case.name);
            fs::write(&path, source).map_err(|e| format!("cannot write temp input: {e}"))?;
            path
        }
    };

    let output = Command::new(cpp_bin)
        .arg(&input_path)
        .arg("-norm")
        .output()
        .map_err(|e| format!("failed to run {}: {e}", cpp_bin.display()))?;

    if matches!(case.input, CaseInput::Inline(_)) {
        let _ = fs::remove_file(&input_path);
    }

    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let lc = text.to_lowercase();

    let has_error = lc.contains("error :") || lc.contains("syntax error");
    let has_norm_success = lc.contains("dump normal form finished");
    Ok(if has_error {
        StatusClass::Error
    } else if has_norm_success || output.status.success() {
        StatusClass::Ok
    } else {
        StatusClass::Error
    })
}

#[test]
fn differential_signal_pipeline_status_against_cpp_reference() {
    let Some(cpp_bin) = cpp_bin() else {
        eprintln!(
            "Skipping signal differential test: FAUST_CPP_BIN not set and /usr/local/bin/faust not found"
        );
        return;
    };
    if !cpp_bin.exists() {
        eprintln!(
            "Skipping signal differential test: C++ binary not found at {}",
            cpp_bin.display()
        );
        return;
    }

    let cpp_commit = git_head_short(CPP_SOURCE_ROOT).unwrap_or_else(|| "unknown".to_owned());
    let compiler = Compiler::new();

    let cases = [
        Case {
            name: "rep_01_passthrough",
            input: CaseInput::CorpusFile("rep_01_passthrough.dsp"),
            expect_valid: true,
        },
        Case {
            name: "rep_02_gain_bias",
            input: CaseInput::CorpusFile("rep_02_gain_bias.dsp"),
            expect_valid: true,
        },
        Case {
            name: "rep_07_nonlinear_clip",
            input: CaseInput::CorpusFile("rep_07_nonlinear_clip.dsp"),
            expect_valid: true,
        },
        Case {
            name: "rep_21_operator_precedence",
            input: CaseInput::CorpusFile("rep_21_operator_precedence.dsp"),
            expect_valid: true,
        },
        Case {
            name: "rep_19_primitive_family",
            input: CaseInput::CorpusFile("rep_19_primitive_family.dsp"),
            expect_valid: true,
        },
        Case {
            name: "rep_31_extended_primitives",
            input: CaseInput::CorpusFile("rep_31_extended_primitives.dsp"),
            expect_valid: true,
        },
        Case {
            name: "rep_10_two_in_two_out_ui",
            input: CaseInput::CorpusFile("rep_10_two_in_two_out_ui.dsp"),
            expect_valid: true,
        },
        Case {
            name: "rep_51_eval_label_widget_subst",
            input: CaseInput::CorpusFile("rep_51_eval_label_widget_subst.dsp"),
            expect_valid: true,
        },
        Case {
            name: "rep_52_eval_label_group_path_subst",
            input: CaseInput::CorpusFile("rep_52_eval_label_group_path_subst.dsp"),
            expect_valid: true,
        },
        Case {
            name: "rep_53_eval_label_modulation_target_subst",
            input: CaseInput::CorpusFile("rep_53_eval_label_modulation_target_subst.dsp"),
            expect_valid: true,
        },
        Case {
            name: "rep_22_parallel_mix",
            input: CaseInput::CorpusFile("rep_22_parallel_mix.dsp"),
            expect_valid: true,
        },
        Case {
            name: "rep_20_environment_waveform",
            input: CaseInput::CorpusFile("rep_20_environment_waveform.dsp"),
            expect_valid: true,
        },
        Case {
            name: "rep_23_feedback_simple",
            input: CaseInput::CorpusFile("rep_23_feedback_simple.dsp"),
            expect_valid: true,
        },
        Case {
            name: "malformed_missing_rhs",
            input: CaseInput::Inline("process = ;\n"),
            expect_valid: false,
        },
        Case {
            name: "closure_captured_case_results_keep_distinct_environments",
            input: CaseInput::Inline(
                "make(x) = case { (0) => x; };\nprocess = make(1)(0) + make(2)(0);\n",
            ),
            expect_valid: true,
        },
    ];

    let mut mismatches = Vec::new();
    eprintln!(
        "Signal differential run with C++ source root {} @ {} and binary {}",
        CPP_SOURCE_ROOT,
        cpp_commit,
        cpp_bin.display()
    );

    for case in &cases {
        let rust = rust_status_for_case(&compiler, case);
        let cpp = cpp_status_for_case(&cpp_bin, case)
            .unwrap_or_else(|e| panic!("C++ run failed for {}: {e}", case.name));
        eprintln!("{:30} rust={:?} cpp={:?}", case.name, rust, cpp);

        if case.expect_valid {
            if rust != StatusClass::Ok || cpp != StatusClass::Ok {
                mismatches.push(format!(
                    "{} expected valid but got rust={:?} cpp={:?}",
                    case.name, rust, cpp
                ));
            }
        } else if rust != StatusClass::Error || cpp != StatusClass::Error {
            mismatches.push(format!(
                "{} expected invalid but got rust={:?} cpp={:?}",
                case.name, rust, cpp
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "Signal differential mismatches:\n{}",
        mismatches.join("\n")
    );
}
