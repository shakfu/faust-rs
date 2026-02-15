use std::fs;
use std::path::PathBuf;

use parser_proto::{SourceReader, SourceReaderError};

fn make_temp_root(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "faust_rs_parser_proto_source_reader_{}_{}_{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos()
    ));
    fs::create_dir_all(&path).expect("temp root should be created");
    path
}

#[test]
fn resolves_imports_from_search_paths() {
    let root = make_temp_root("resolve");
    let lib = root.join("stdfaust.lib");
    fs::write(&lib, "process = _;\n").expect("lib file should be written");

    let reader = SourceReader::new(vec![root.clone()]);
    let resolved = reader
        .resolve_import("stdfaust.lib")
        .expect("import should resolve");
    assert_eq!(
        resolved,
        lib.canonicalize().expect("canonical path should resolve")
    );

    fs::remove_dir_all(root).expect("temp root should be removable");
}

#[test]
fn reads_nested_imports_and_tracks_used_files() {
    let root = make_temp_root("nested");
    let a = root.join("a.dsp");
    let b = root.join("b.dsp");
    let c = root.join("c.dsp");

    fs::write(&a, "import(\"b.dsp\");\nprocess = _;\n").expect("a should be written");
    fs::write(&b, "import(\"c.dsp\");\nfoo = _;\n").expect("b should be written");
    fs::write(&c, "bar = _;\n").expect("c should be written");

    let mut reader = SourceReader::new(vec![root.clone()]);
    let expanded = reader.read_file(&a).expect("read should succeed");

    assert!(expanded.contains("bar = _;"));
    assert!(expanded.contains("foo = _;"));
    assert!(expanded.contains("process = _;"));
    assert_eq!(reader.used_files().len(), 3);

    fs::remove_dir_all(root).expect("temp root should be removable");
}

#[test]
fn detects_import_cycles() {
    let root = make_temp_root("cycle");
    let a = root.join("a.dsp");
    let b = root.join("b.dsp");

    fs::write(&a, "import(\"b.dsp\");\nprocess = _;\n").expect("a should be written");
    fs::write(&b, "import(\"a.dsp\");\nfoo = _;\n").expect("b should be written");

    let mut reader = SourceReader::new(vec![root.clone()]);
    let err = reader
        .read_file(&a)
        .expect_err("cycle should be detected and reported");

    match err {
        SourceReaderError::ImportCycle { .. } => {}
        other => panic!("expected ImportCycle, got {other}"),
    }

    fs::remove_dir_all(root).expect("temp root should be removable");
}
