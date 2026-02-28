//! Integration tests for `api_bridge`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

use std::fs;
use std::path::PathBuf;

use parser::{SourceReaderError, parse_file_with_imports, parse_minimal, parse_program};

fn make_temp_root(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "faust_rs_parser_bridge_{}_{}_{}",
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
fn bridge_exposes_minimal_parse_helper() {
    assert!(parse_minimal("process = _;"));
}

#[test]
fn bridge_exposes_parse_program() {
    let out = parse_program("process = _;", "bridge_program.dsp");
    assert!(out.root.is_some(), "root should be present");
    assert!(
        out.errors.is_empty(),
        "unexpected parse errors: {:?}",
        out.errors
    );
}

#[test]
fn bridge_exposes_file_import_parsing() {
    let root = make_temp_root("imports");
    let main = root.join("main.dsp");
    let lib = root.join("ops.lib");

    fs::write(&main, "import(\"ops.lib\");\nprocess = gain;\n").expect("main should be written");
    fs::write(&lib, "gain = _;\n").expect("lib should be written");

    let out =
        parse_file_with_imports(&main, std::slice::from_ref(&root)).expect("parse should succeed");
    assert!(out.root.is_some(), "root should be present");
    assert!(
        out.errors.is_empty(),
        "unexpected parse errors: {:?}",
        out.errors
    );
    assert_eq!(
        out.used_files.len(),
        2,
        "used_files should contain entry + imported file"
    );
    assert_eq!(
        out.used_files[0],
        main.canonicalize().expect("main should canonicalize")
    );
    assert_eq!(
        out.used_files[1],
        lib.canonicalize().expect("lib should canonicalize")
    );

    fs::remove_dir_all(root).expect("temp root should be removable");
}

#[test]
fn parse_file_with_imports_exposes_deterministic_used_files_order() {
    let root = make_temp_root("used_files_order");
    let main = root.join("main.dsp");
    let lib_a = root.join("a.lib");
    let lib_b = root.join("b.lib");

    fs::write(
        &main,
        "import(\"a.lib\");\nimport(\"b.lib\");\nprocess = a + b;\n",
    )
    .expect("main should be written");
    fs::write(&lib_a, "a = _;\n").expect("a.lib should be written");
    fs::write(&lib_b, "b = _;\n").expect("b.lib should be written");

    let out =
        parse_file_with_imports(&main, std::slice::from_ref(&root)).expect("parse should succeed");
    assert!(
        out.errors.is_empty(),
        "unexpected parse errors: {:?}",
        out.errors
    );

    let expected = vec![
        main.canonicalize().expect("main should canonicalize"),
        lib_a.canonicalize().expect("a.lib should canonicalize"),
        lib_b.canonicalize().expect("b.lib should canonicalize"),
    ];
    assert_eq!(
        out.used_files, expected,
        "used_files order should follow deterministic expansion order"
    );

    fs::remove_dir_all(root).expect("temp root should be removable");
}

#[test]
fn parse_file_with_imports_preserves_imported_file_diagnostic_origin() {
    let root = make_temp_root("import_origin");
    let main = root.join("main.dsp");
    let lib = root.join("ops.lib");

    fs::write(&main, "import(\"ops.lib\");\nprocess = gain;\n").expect("main should be written");
    fs::write(&lib, "gain = ;\n").expect("lib should be written");

    let out =
        parse_file_with_imports(&main, std::slice::from_ref(&root)).expect("parse should succeed");

    let lib_canonical = lib.canonicalize().expect("lib path should canonicalize");
    let has_label_on_imported_file = out
        .diagnostics
        .as_slice()
        .iter()
        .flat_map(|d| d.labels.iter())
        .any(|label| label.span.file == lib_canonical);
    assert!(
        has_label_on_imported_file,
        "expected at least one parser diagnostic label on imported file {}",
        lib_canonical.display()
    );

    fs::remove_dir_all(root).expect("temp root should be removable");
}

#[test]
fn parse_file_with_imports_keeps_remote_urls_out_of_scope() {
    let root = make_temp_root("remote_import_policy");
    let main = root.join("main.dsp");
    fs::write(
        &main,
        "import(\"https://example.com/stdfaust.lib\");\nprocess = _;\n",
    )
    .expect("main should be written");

    let err =
        parse_file_with_imports(&main, std::slice::from_ref(&root)).expect_err("must fail");
    match err {
        SourceReaderError::UnresolvedImport { name, from } => {
            assert_eq!(&*name, "https://example.com/stdfaust.lib");
            assert_eq!(
                from,
                main.canonicalize().expect("main should canonicalize")
            );
        }
        other => panic!("unexpected error kind for remote import policy: {other:?}"),
    }

    fs::remove_dir_all(root).expect("temp root should be removable");
}
