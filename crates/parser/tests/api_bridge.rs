use std::fs;
use std::path::PathBuf;

use parser::{parse_file_with_imports, parse_minimal, parse_program};

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

    fs::remove_dir_all(root).expect("temp root should be removable");
}
