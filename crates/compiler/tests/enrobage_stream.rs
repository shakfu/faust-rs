//! Integration tests for `compiler::enrobage` stream-copy/injection helpers.
//!
//! Scope:
//! - Golden text parity checks for architecture stream-copy steps.
//! - Include injection, replacement rules, and recoverable missing-include flow.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use compiler::enrobage::{
    StreamCopyConfig, StreamCopyState, stream_copy_license, stream_copy_until,
    stream_copy_until_end,
};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("enrobage")
}

fn fixture_arch(file: &str) -> PathBuf {
    fixture_root().join("arch").join(file)
}

fn fixture_corpus(file: &str) -> PathBuf {
    fixture_root().join("corpus").join(file)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn default_stream_cfg() -> StreamCopyConfig {
    StreamCopyConfig {
        class_name: "customdsp".to_owned(),
        super_class_name: "faust_dsp".to_owned(),
        inline_arch_switch: true,
        architecture_dirs: vec![fixture_root().join("arch")],
    }
}

#[test]
fn stream_copy_license_keeps_header_without_exception_tag() {
    let src_text = read(&fixture_arch("license_keep.cpp"));
    let mut src = Cursor::new(src_text.into_bytes());
    let mut out = Vec::<u8>::new();
    stream_copy_license(&mut src, &mut out, "FAUST COMPILER EXCEPTION")
        .expect("stream_copy_license should not fail");
    let rendered = String::from_utf8(out).expect("utf8 output");
    let expected = "/*\n * Architecture header without removal tag.\n */\n";
    assert_eq!(rendered, expected);
}

#[test]
fn stream_copy_license_removes_header_with_exception_tag() {
    let src_text = read(&fixture_arch("license_remove.cpp"));
    let mut src = Cursor::new(src_text.into_bytes());
    let mut out = Vec::<u8>::new();
    stream_copy_license(&mut src, &mut out, "FAUST COMPILER EXCEPTION")
        .expect("stream_copy_license should not fail");
    let rendered = String::from_utf8(out).expect("utf8 output");
    assert_eq!(rendered, "");
}

#[test]
fn stream_copy_until_matches_golden_with_includes_and_class_replacement() {
    let src_text = read(&fixture_arch("wrapper.cpp"));
    let expected = read(&fixture_corpus("wrapper_until_includeclass.expected.cpp"));
    let mut src = Cursor::new(src_text.into_bytes());
    let mut out = Vec::<u8>::new();
    let mut state = StreamCopyState::default();
    let cfg = default_stream_cfg();
    stream_copy_until(&mut src, &mut out, "<<includeclass>>", &cfg, &mut state)
        .expect("stream_copy_until should not fail");
    let rendered = String::from_utf8(out).expect("utf8 output");
    assert_eq!(rendered, expected);
    assert_eq!(state.already_included.len(), 2);
    assert!(state.already_included.contains("faust/injected_one.inc"));
    assert!(state.already_included.contains("faust/injected_two.inc"));
}

#[test]
fn stream_copy_until_end_matches_golden() {
    let src_text = read(&fixture_arch("wrapper.cpp"));
    let expected = read(&fixture_corpus("wrapper_until_end.expected.cpp"));
    let mut src = Cursor::new(src_text.into_bytes());
    let mut out = Vec::<u8>::new();
    let mut state = StreamCopyState::default();
    let cfg = default_stream_cfg();
    stream_copy_until_end(&mut src, &mut out, &cfg, &mut state)
        .expect("stream_copy_until_end should not fail");
    let rendered = String::from_utf8(out).expect("utf8 output");
    assert_eq!(rendered, expected);
}

#[test]
fn stream_copy_until_end_deduplicates_injected_architecture_files() {
    let source = "#include <faust/injected_one.inc>\n#include <faust/injected_one.inc>\n";
    let mut src = Cursor::new(source.as_bytes());
    let mut out = Vec::<u8>::new();
    let mut state = StreamCopyState::default();
    let cfg = default_stream_cfg();
    stream_copy_until_end(&mut src, &mut out, &cfg, &mut state)
        .expect("stream copy should succeed");
    let rendered = String::from_utf8(out).expect("utf8 output");
    assert_eq!(rendered, "// injected_one\n#define ENROBAGE_ONE 1\n");
    assert_eq!(state.already_included.len(), 1);
}

#[test]
fn stream_copy_until_end_records_missing_include_error_and_continues() {
    let source = "#include <faust/does_not_exist.inc>\nmydsp* value = new mydsp();\n";
    let mut src = Cursor::new(source.as_bytes());
    let mut out = Vec::<u8>::new();
    let mut state = StreamCopyState::default();
    let cfg = default_stream_cfg();
    stream_copy_until_end(&mut src, &mut out, &cfg, &mut state)
        .expect("stream copy should succeed even when include is missing");
    let rendered = String::from_utf8(out).expect("utf8 output");
    assert_eq!(rendered, "customdsp* value = new customdsp();\n");
    assert_eq!(
        state.last_error.as_deref(),
        Some("ERROR : faust/does_not_exist.inc not found\n")
    );
}
