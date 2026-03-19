//! Differential tracker for the current parser/import/loading parity gap.
//!
//! This test is intentionally about a known divergence, not a closed parity
//! case. It records the external reproducer that motivated the exact
//! `formatDefinitions(...)` / loaded-source parity plan.

use std::path::{Path, PathBuf};
use std::process::Command;

use compiler::Compiler;

const DEFAULT_FAUSTLIBRARIES_ROOT: &str = "/Users/letz/Developpements/faustlibraries";

fn cpp_bin() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FAUST_CPP_BIN") {
        return Some(PathBuf::from(path));
    }
    let default = PathBuf::from("/usr/local/bin/faust");
    default.exists().then_some(default)
}

fn faustlibraries_root() -> Option<PathBuf> {
    std::env::var_os("FAUST_RS_FAUSTLIBRARIES_ROOT")
        .map(PathBuf::from)
        .or_else(|| {
            let default = PathBuf::from(DEFAULT_FAUSTLIBRARIES_ROOT);
            default.exists().then_some(default)
        })
}

fn cpp_accepts_file(cpp_bin: &Path, input: &Path, import_root: &Path) -> Result<(), String> {
    let mut out_path = std::env::temp_dir();
    out_path.push(format!(
        "faust_rs_import_gap_{}_{}.cpp",
        std::process::id(),
        input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("fixture")
    ));
    let output = Command::new(cpp_bin)
        .arg("-pn")
        .arg("operator_test")
        .arg("-I")
        .arg(import_root)
        .arg(input)
        .arg("-lang")
        .arg("cpp")
        .arg("-o")
        .arg(&out_path)
        .output()
        .map_err(|e| format!("failed to run {}: {e}", cpp_bin.display()))?;
    let _ = std::fs::remove_file(&out_path);
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

#[test]
fn operator_test_is_still_the_known_cpp_only_import_loading_reproducer() {
    let Some(root) = faustlibraries_root() else {
        eprintln!("Skipping import/loading differential: faustlibraries root unavailable");
        return;
    };
    let Some(cpp) = cpp_bin() else {
        eprintln!("Skipping import/loading differential: Faust C++ binary unavailable");
        return;
    };

    let dsp = root.join("tests").join("dx7_tests.dsp");
    cpp_accepts_file(&cpp, &dsp, &root)
        .unwrap_or_else(|e| panic!("Faust C++ should still accept operator_test: {e}"));

    let err = Compiler::new()
        .with_process_name("operator_test")
        .compile_file_default_to_signals(&dsp)
        .expect_err("Rust should still expose the known import/loading gap on operator_test");
    let rendered = err.to_string();
    assert!(
        rendered.contains("undefined symbol `ba`"),
        "gap tracker should still point at the loaded-source alias failure, got: {rendered}"
    );
}
