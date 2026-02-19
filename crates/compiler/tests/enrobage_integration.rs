//! Integration tests for architecture wrapping assembly (`enrobage` Step E).
//!
//! Scope:
//! - Verifies wrapper orchestration around generated C++ text.
//! - Confirms marker slicing and class-name replacement in wrapped output.

use std::fs;
use std::path::{Path, PathBuf};

use compiler::enrobage::{EnrobageOptions, wrap_cpp_with_architecture};

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

#[test]
fn wrap_cpp_with_architecture_matches_expected_fixture() {
    let generated_cpp = "// GENERATED CLASS\nclass customdsp : public faust_dsp {};\n";
    let mut options = EnrobageOptions::new(fixture_arch("wrapper.cpp"));
    options.architecture_dirs = vec![fixture_root().join("arch")];
    options.class_name = "customdsp".to_owned();
    options.super_class_name = "faust_dsp".to_owned();
    options.inline_arch_files = true;

    let wrapped = wrap_cpp_with_architecture(generated_cpp, &options)
        .expect("wrap_cpp_with_architecture should succeed");
    let expected = read(&fixture_corpus("wrapper_wrapped.expected.cpp"));
    assert_eq!(wrapped.code, expected);
    assert_eq!(
        wrapped.recoverable_error, None,
        "fixture wrapper should not produce include-injection errors"
    );
}
