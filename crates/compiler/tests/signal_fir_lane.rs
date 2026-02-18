use compiler::{Compiler, SignalFirLane};
use std::path::PathBuf;

fn corpus_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("corpus")
        .join(file)
}

#[test]
fn dump_cpp_fastlane_compiles_fixture() {
    let compiler = Compiler::new();
    let path = corpus_path("rep_01_passthrough.dsp");
    let cpp = compiler
        .compile_file_default_to_cpp_with_lane(
            &path,
            &codegen::backends::cpp::CppOptions::default(),
            SignalFirLane::TransformFastLane,
        )
        .unwrap_or_else(|e| panic!("fast-lane C++ compilation failed: {e}"));
    assert!(cpp.contains("class rep_01_passthrough : public dsp"));
}
