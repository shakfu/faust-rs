//! Integration tests for `compiler::enrobage` search/open helpers.
//!
//! Scope:
//! - Validates search precedence for architecture files.
//! - Validates `fopen_search` full-path and import-dir enrichment side effects.

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use compiler::enrobage::{fopen_search, open_arch_stream};

fn temp_root(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock drift")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "faust_rs_enrobage_{test_name}_{}_{}",
        std::process::id(),
        nanos
    ));
    fs::create_dir_all(&root).expect("create temp root");
    root
}

fn read_file_text(mut file: fs::File) -> String {
    let mut out = String::new();
    file.read_to_string(&mut out)
        .expect("read opened architecture text");
    out
}

#[test]
fn open_arch_stream_uses_declared_architecture_dir_order() {
    let root = temp_root("open_arch_stream_order");
    let arch_a = root.join("arch_a");
    let arch_b = root.join("arch_b");
    fs::create_dir_all(&arch_a).expect("create arch_a");
    fs::create_dir_all(&arch_b).expect("create arch_b");

    let file_name = "priority_arch_fixture.cpp";
    fs::write(arch_a.join(file_name), "A\n").expect("write arch_a fixture");
    fs::write(arch_b.join(file_name), "B\n").expect("write arch_b fixture");

    let file = open_arch_stream(file_name, &[arch_b.clone(), arch_a.clone()])
        .expect("open_arch_stream should find file in first directory");
    let text = read_file_text(file);
    assert_eq!(
        text, "B\n",
        "search order must follow the declared architecture-dir order"
    );

    fs::remove_dir_all(root).expect("cleanup temp root");
}

#[test]
fn fopen_search_direct_open_enriches_import_dir_list() {
    let root = temp_root("fopen_search_direct");
    let src = root.join("direct_source.lib");
    fs::write(&src, "component(\"direct\");\n").expect("write direct file");

    let mut import_dirs = vec![root.join("unused_search")];
    let before = import_dirs.len();
    let res = fopen_search(src.to_string_lossy().as_ref(), &mut import_dirs)
        .expect("fopen_search should open direct absolute filename");

    assert_eq!(res.full_path, src);
    assert_eq!(
        import_dirs.len(),
        before + 1,
        "direct open should append source dirname to import_dirs"
    );
    assert_eq!(
        import_dirs.last().expect("last import dir"),
        &root,
        "last import_dir must be the containing directory of direct-open file"
    );

    fs::remove_dir_all(root).expect("cleanup temp root");
}

#[test]
fn fopen_search_import_dir_lookup_sets_full_path_without_enrichment() {
    let root = temp_root("fopen_search_import_dirs");
    let search = root.join("search");
    fs::create_dir_all(&search).expect("create search dir");
    let file_name = "search_target.lib";
    let target = search.join(file_name);
    fs::write(&target, "component(\"search\");\n").expect("write search target");

    let mut import_dirs = vec![search.clone()];
    let before = import_dirs.clone();
    let res = fopen_search(file_name, &mut import_dirs)
        .expect("fopen_search should open file from import_dirs");

    assert_eq!(
        res.full_path, target,
        "full_path should point to the matched import-dir candidate"
    );
    assert_eq!(
        import_dirs, before,
        "import_dirs should not be enriched when file is found through search list"
    );

    fs::remove_dir_all(root).expect("cleanup temp root");
}
