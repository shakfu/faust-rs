//! Integration tests for `source_reader`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

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

#[test]
fn url_imports_are_unresolved_in_parser_proto_scope() {
    let root = make_temp_root("url_import");
    let main = root.join("main.dsp");

    fs::write(
        &main,
        "import(\"https://example.com/stdfaust.lib\");\nprocess = _;\n",
    )
    .expect("main should be written");

    let mut reader = SourceReader::new(vec![root.clone()]);
    let err = reader
        .read_file(&main)
        .expect_err("URL import should remain unresolved in parser-proto scope");

    match err {
        SourceReaderError::UnresolvedImport { name, .. } => {
            assert_eq!(name.as_ref(), "https://example.com/stdfaust.lib");
        }
        other => panic!("expected UnresolvedImport, got {other}"),
    }

    fs::remove_dir_all(root).expect("temp root should be removable");
}

#[test]
fn prefers_import_from_local_directory_over_search_paths() {
    let root = make_temp_root("local_precedence");
    let src = root.join("src");
    let libs = root.join("libs");
    fs::create_dir_all(&src).expect("src dir should be created");
    fs::create_dir_all(&libs).expect("libs dir should be created");

    let main = src.join("main.dsp");
    let local_shared = src.join("shared.lib");
    let search_shared = libs.join("shared.lib");

    fs::write(&main, "import(\"shared.lib\");\nprocess = marker;\n")
        .expect("main should be written");
    fs::write(&local_shared, "marker = local_version;\n").expect("local shared should be written");
    fs::write(&search_shared, "marker = search_path_version;\n")
        .expect("search-path shared should be written");

    let mut reader = SourceReader::new(vec![libs.clone()]);
    let expanded = reader.read_file(&main).expect("read should succeed");

    assert!(expanded.contains("local_version"));
    assert!(!expanded.contains("search_path_version"));

    fs::remove_dir_all(root).expect("temp root should be removable");
}

#[test]
fn resolves_parent_relative_imports_and_keeps_used_files_unique() {
    let root = make_temp_root("parent_relative");
    let src = root.join("src");
    let lib = root.join("lib");
    fs::create_dir_all(&src).expect("src dir should be created");
    fs::create_dir_all(&lib).expect("lib dir should be created");

    let main = src.join("main.dsp");
    let mid = src.join("mid.lib");
    let common = lib.join("common.lib");

    fs::write(
        &main,
        "import(\"../lib/common.lib\");\nimport(\"mid.lib\");\nprocess = out;\n",
    )
    .expect("main should be written");
    fs::write(&mid, "import(\"../lib/common.lib\");\nout = _;\n").expect("mid should be written");
    fs::write(&common, "common = _;\n").expect("common should be written");

    let mut reader = SourceReader::new(vec![]);
    let expanded = reader.read_file(&main).expect("read should succeed");

    assert!(expanded.contains("common = _;"));
    assert!(expanded.contains("out = _;"));
    assert_eq!(reader.used_files().len(), 3);

    fs::remove_dir_all(root).expect("temp root should be removable");
}
