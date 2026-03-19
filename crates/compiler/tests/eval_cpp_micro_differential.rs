//! Differential `eval` micro-fixtures against Faust C++ acceptance.
//!
//! These cases are intentionally small and target the `eval -> propagate`
//! boundary directly, so parity regressions are caught before they reappear in
//! large library DSPs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use compiler::Compiler;
use signals::dump_sig_readable;

#[derive(Clone, Copy, Debug)]
struct EvalFixture {
    file: &'static str,
    expect_inputs: usize,
    expect_outputs: usize,
    expected_fragments: &'static [&'static str],
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("eval_micro_fixtures")
}

fn fixture_path(file: &str) -> PathBuf {
    fixture_dir().join(file)
}

fn cpp_bin() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FAUST_CPP_BIN") {
        return Some(PathBuf::from(path));
    }
    let default = PathBuf::from("/usr/local/bin/faust");
    default.exists().then_some(default)
}

fn cpp_accepts_norm_and_cpp(cpp_bin: &Path, input: &Path) -> Result<(), String> {
    let norm = Command::new(cpp_bin)
        .arg(input)
        .arg("-norm")
        .output()
        .map_err(|e| format!("failed to run {} -norm: {e}", cpp_bin.display()))?;
    let mut norm_text = String::new();
    norm_text.push_str(&String::from_utf8_lossy(&norm.stdout));
    norm_text.push('\n');
    norm_text.push_str(&String::from_utf8_lossy(&norm.stderr));
    let norm_lc = norm_text.to_lowercase();
    let norm_ok = norm.status.success() || norm_lc.contains("dump normal form finished");
    if !norm_ok {
        return Err(format!(
            "C++ -norm failed for {}:\n{}",
            input.display(),
            norm_text
        ));
    }

    let mut out_path = std::env::temp_dir();
    out_path.push(format!(
        "faust_rs_eval_micro_{}_{}.cpp",
        std::process::id(),
        input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("fixture")
    ));
    let cpp = Command::new(cpp_bin)
        .arg(input)
        .arg("-lang")
        .arg("cpp")
        .arg("-o")
        .arg(&out_path)
        .output()
        .map_err(|e| format!("failed to run {} -lang cpp: {e}", cpp_bin.display()))?;
    let _ = fs::remove_file(&out_path);
    if !cpp.status.success() {
        return Err(format!(
            "C++ -lang cpp failed for {}:\n{}",
            input.display(),
            String::from_utf8_lossy(&cpp.stderr)
        ));
    }
    Ok(())
}

#[test]
fn eval_micro_fixtures_match_cpp_acceptance_and_rust_signal_shapes() {
    let fixtures = [
        EvalFixture {
            file: "eval_01_inputs_residual_closure.dsp",
            expect_inputs: 1,
            expect_outputs: 1,
            expected_fragments: &["SIGINPUT(int(0))"],
        },
        EvalFixture {
            file: "eval_02_waveform_rdtable_leaf.dsp",
            expect_inputs: 0,
            expect_outputs: 1,
            expected_fragments: &["SIGRDTBL(", "SIGWAVEFORM("],
        },
        EvalFixture {
            file: "eval_03_seq_zero_neutral.dsp",
            expect_inputs: 2,
            expect_outputs: 3,
            expected_fragments: &["SIGINPUT(int(0))", "SIGINPUT(int(1))"],
        },
        EvalFixture {
            file: "eval_04_case_exact_integer_real_match.dsp",
            expect_inputs: 2,
            expect_outputs: 2,
            expected_fragments: &["SIGMAX(", "SIGMIN("],
        },
        EvalFixture {
            file: "eval_05_route_arithmetic_params.dsp",
            expect_inputs: 2,
            expect_outputs: 2,
            expected_fragments: &["SIGINPUT(int(0))", "SIGINPUT(int(1))"],
        },
    ];

    let compiler = Compiler::new();
    let cpp = cpp_bin();

    for fixture in fixtures {
        let path = fixture_path(fixture.file);
        if let Some(cpp_bin) = &cpp {
            cpp_accepts_norm_and_cpp(cpp_bin, &path)
                .unwrap_or_else(|e| panic!("C++ parity fixture {} failed: {e}", fixture.file));
        }

        let out = compiler
            .compile_file_default_to_signals(&path)
            .unwrap_or_else(|e| panic!("Rust signal lowering failed for {}: {e}", path.display()));

        assert_eq!(
            out.process_arity.inputs, fixture.expect_inputs,
            "unexpected input arity for {}",
            fixture.file
        );
        assert_eq!(
            out.process_arity.outputs, fixture.expect_outputs,
            "unexpected output arity for {}",
            fixture.file
        );
        assert_eq!(
            out.signals.len(),
            fixture.expect_outputs,
            "signal count should match process outputs for {}",
            fixture.file
        );

        let dumps: Vec<String> = out
            .signals
            .iter()
            .map(|&sig| dump_sig_readable(&out.parse.state.arena, sig))
            .collect();
        let joined = dumps.join("\n");
        for expected in fixture.expected_fragments {
            assert!(
                joined.contains(expected),
                "fixture {} should contain `{expected}` in Rust dump-sig output, got:\n{joined}",
                fixture.file
            );
        }
    }
}
