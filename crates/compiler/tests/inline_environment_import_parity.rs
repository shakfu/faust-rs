//! Parity test for inline `environment { import("..."); }` handling.
//!
//! The adjacent C++ Faust compiler accepts this shape because imports survive
//! parsing as structural `importFile(...)` nodes and are expanded later from the
//! parsed definition tree. Rust used to depend on source-line import flattening,
//! which dropped inline local imports and miscompiled the reduced `chain.dsp`
//! pattern.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use codegen::backends::cpp::CppOptions;
use compiler::Compiler;

fn cpp_bin() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FAUST_CPP_BIN") {
        return Some(PathBuf::from(path));
    }
    let default = PathBuf::from("/usr/local/bin/faust");
    default.exists().then_some(default)
}

fn temp_root(test_name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock drift")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "faust_rs_compiler_{test_name}_{}_{}",
        std::process::id(),
        stamp
    ));
    fs::create_dir_all(&root).expect("create temp root");
    root
}

fn cpp_accepts_file(cpp_bin: &Path, input: &Path) -> Result<(), String> {
    let out_path = std::env::temp_dir().join(format!(
        "faust_rs_inline_env_import_{}_{}.cpp",
        std::process::id(),
        input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("fixture")
    ));
    let output = Command::new(cpp_bin)
        .arg(input)
        .arg("-lang")
        .arg("cpp")
        .arg("-o")
        .arg(&out_path)
        .output()
        .map_err(|e| format!("failed to run {}: {e}", cpp_bin.display()))?;
    let _ = fs::remove_file(&out_path);
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

#[test]
fn compiler_accepts_inline_environment_import_like_cpp() {
    let root = temp_root("inline_environment_import");
    let main = root.join("main.dsp");
    let child = root.join("child.dsp");

    fs::write(
        &main,
        "GEN = environment { import(\"child.dsp\"); }.process;\nprocess = GEN;\n",
    )
    .expect("write main");
    fs::write(&child, "process = _;\n").expect("write child");

    if let Some(cpp) = cpp_bin() {
        cpp_accepts_file(&cpp, &main)
            .unwrap_or_else(|e| panic!("Faust C++ should accept inline environment import: {e}"));
    }

    let rendered = Compiler::new()
        .compile_file_default_to_cpp(&main, &CppOptions::default())
        .expect("Rust compiler should accept inline environment import");
    assert!(
        rendered.contains("class mydsp"),
        "generated C++ should contain the DSP class declaration"
    );

    fs::remove_dir_all(root).expect("temp root should be removable");
}

/// Guards the reduced `chain.dsp` shape that originally triggered this parity
/// work: two local environments import separate files, then `process` fans the
/// generator output into a duplicated effect tuple.
#[test]
fn compiler_accepts_reduced_chain_shape_like_cpp() {
    let root = temp_root("reduced_chain_shape");
    let main = root.join("chain.dsp");
    let generator = root.join("karplus.dsp");
    let fx = root.join("freeverb.dsp");

    fs::write(
        &main,
        concat!(
            "GEN = environment { import(\"karplus.dsp\"); }.process;\n",
            "FX = environment { import(\"freeverb.dsp\"); }.process;\n",
            "process = GEN<:(FX,FX);\n",
        ),
    )
    .expect("write main");
    fs::write(&generator, "process = _;\n").expect("write generator");
    fs::write(&fx, "process = _;\n").expect("write effect");

    if let Some(cpp) = cpp_bin() {
        cpp_accepts_file(&cpp, &main)
            .unwrap_or_else(|e| panic!("Faust C++ should accept reduced chain shape: {e}"));
    }

    let rendered = Compiler::new()
        .compile_file_default_to_cpp(&main, &CppOptions::default())
        .expect("Rust compiler should accept reduced chain shape");
    assert!(
        rendered.contains("class mydsp"),
        "generated C++ should contain the DSP class declaration"
    );

    fs::remove_dir_all(root).expect("temp root should be removable");
}

/// Guards the case where the same imported file is needed in two different
/// structural scopes: once at the top level and once transitively inside a
/// nested local environment. Duplicate-import suppression must stay local to
/// each scope so the top-level alias remains visible.
#[test]
fn compiler_keeps_top_level_imports_visible_after_nested_transitive_imports() {
    let root = temp_root("top_level_and_nested_imports");
    let main = root.join("main.dsp");
    let child = root.join("child.lib");
    let shared = root.join("shared.lib");

    fs::write(
        &main,
        concat!(
            "import(\"shared.lib\");\n",
            "child = environment { import(\"child.lib\"); }.process;\n",
            "process = val;\n",
        ),
    )
    .expect("write main");
    fs::write(&child, "import(\"shared.lib\");\nprocess = val;\n").expect("write child");
    fs::write(&shared, "val = 1;\n").expect("write shared");

    if let Some(cpp) = cpp_bin() {
        cpp_accepts_file(&cpp, &main).unwrap_or_else(|e| {
            panic!("Faust C++ should accept top-level and nested shared imports: {e}")
        });
    }

    let rendered = Compiler::new()
        .compile_file_default_to_cpp(&main, &CppOptions::default())
        .expect("Rust compiler should keep top-level shared import visible");
    assert!(
        rendered.contains("class mydsp"),
        "generated C++ should contain the DSP class declaration"
    );

    fs::remove_dir_all(root).expect("temp root should be removable");
}
