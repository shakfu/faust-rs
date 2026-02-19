//! Integration tests for `compiler::enrobage` path/output helpers.
//!
//! Scope:
//! - Locks C++-parity edge behavior for `fileBasename`/`fileDirname`/`stripEnd`.
//! - Verifies Rust `PathBuf` adaptation for `make_output_file`.

use std::path::{Path, PathBuf};

use compiler::enrobage::{file_basename, file_dirname, make_output_file, strip_end};

#[test]
fn file_basename_handles_unix_and_windows_forms() {
    assert_eq!(file_basename("/tmp/faust/test.dsp"), "test.dsp");
    assert_eq!(file_basename(r"C:\tmp\faust\test.dsp"), "test.dsp");
    assert_eq!(file_basename("test.dsp"), "test.dsp");
}

#[test]
fn file_dirname_matches_cpp_fallback_rules() {
    assert_eq!(file_dirname("test.dsp"), ".");
    assert_eq!(file_dirname("/test.dsp"), "/");
    assert_eq!(file_dirname("/tmp/faust/test.dsp"), "/tmp/faust");
    assert_eq!(file_dirname(r"C:\tmp\faust\test.dsp"), r"C:\tmp\faust");
}

#[test]
fn strip_end_keeps_cpp_style_min_length_guard() {
    assert_eq!(strip_end("noise.dsp", ".dsp"), "noise");
    assert_eq!(strip_end("noise.dsp", ".cpp"), "noise.dsp");
    assert_eq!(strip_end("a.c", ".c"), "a.c");
}

#[test]
fn make_output_file_uses_optional_output_dir() {
    assert_eq!(
        make_output_file(None, "noise.cpp"),
        PathBuf::from("noise.cpp")
    );
    assert_eq!(
        make_output_file(Some(Path::new("")), "noise.cpp"),
        PathBuf::from("noise.cpp")
    );
    assert_eq!(
        make_output_file(Some(Path::new("build/out")), "noise.cpp"),
        PathBuf::from("build/out/noise.cpp")
    );
}
