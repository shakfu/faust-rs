use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use parser_proto::parse_program;

const CPP_SOURCE_ROOT: &str = "/Users/letz/Developpements/RUST/faust";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RustClass {
    Ok,
    Recovered,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CppClass {
    Ok,
    ParseError,
    OtherError,
}

#[derive(Debug)]
struct DiffRow {
    name: String,
    rust: RustClass,
    cpp: CppClass,
    rust_errors: usize,
    rust_recoveries: u32,
    cpp_status: i32,
}

#[derive(Debug)]
struct Case {
    name: String,
    source: String,
    expect_valid: bool,
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus")
}

fn rust_class_for(source: &str, source_name: &str) -> (RustClass, usize, u32) {
    let out = parse_program(source, source_name);
    let parse_errors = out.state.ctx.parse_error_count();
    let recoveries = out.state.ctx.recovery_count();
    let rust_class = if out.root.is_none() {
        RustClass::Error
    } else if parse_errors == 0 && out.errors.is_empty() {
        RustClass::Ok
    } else {
        RustClass::Recovered
    };
    (rust_class, out.errors.len(), recoveries)
}

fn cpp_class_for(cpp_bin: &Path, source: &str, case_name: &str) -> Result<(CppClass, i32), String> {
    let mut input_path = std::env::temp_dir();
    input_path.push(format!(
        "faust_rs_parser_diff_{}_{}.dsp",
        std::process::id(),
        case_name
    ));

    let mut out_path = std::env::temp_dir();
    out_path.push(format!(
        "faust_rs_parser_diff_{}_{}.c",
        std::process::id(),
        case_name
    ));

    fs::write(&input_path, source).map_err(|e| format!("write input failed: {e}"))?;

    let output = Command::new(cpp_bin)
        .arg(&input_path)
        .arg("-lang")
        .arg("c")
        .arg("-o")
        .arg(&out_path)
        .output()
        .map_err(|e| format!("failed to run {}: {e}", cpp_bin.display()))?;

    let _ = fs::remove_file(&input_path);
    let _ = fs::remove_file(&out_path);

    let status_code = output.status.code().unwrap_or(-1);
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let text_lc = text.to_lowercase();

    let class = if output.status.success() {
        CppClass::Ok
    } else if text_lc.contains("error") {
        CppClass::ParseError
    } else {
        CppClass::OtherError
    };

    Ok((class, status_code))
}

fn cpp_bin() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FAUST_CPP_BIN") {
        return Some(PathBuf::from(path));
    }

    let default = PathBuf::from("/usr/local/bin/faust");
    if default.exists() {
        Some(default)
    } else {
        None
    }
}

fn load_cases() -> Result<Vec<Case>, String> {
    let corpus = corpus_dir();
    let mut files: Vec<PathBuf> = fs::read_dir(&corpus)
        .map_err(|e| format!("cannot read corpus dir {}: {e}", corpus.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dsp"))
        })
        .collect();
    files.sort();

    let mut cases = Vec::new();
    for path in files {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("invalid corpus filename: {}", path.display()))?
            .to_owned();
        let source = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read corpus file {}: {e}", path.display()))?;
        cases.push(Case {
            name,
            source,
            expect_valid: true,
        });
    }

    cases.push(Case {
        name: "malformed_empty_rhs".to_owned(),
        source: "process = ;\n".to_owned(),
        expect_valid: false,
    });
    cases.push(Case {
        name: "malformed_missing_rpar".to_owned(),
        source: "process = hslider(\"g\", 0.5, 0.0, 1.0, 0.01;\n".to_owned(),
        expect_valid: false,
    });
    cases.push(Case {
        name: "declare_metadata".to_owned(),
        source: "declare author \"letz\";\nprocess = _;\n".to_owned(),
        expect_valid: true,
    });
    cases.push(Case {
        name: "declare_definition_metadata".to_owned(),
        source: "declare proc category \"ui\";\nprocess = _;\n".to_owned(),
        expect_valid: true,
    });
    cases.push(Case {
        name: "malformed_declare_missing_value".to_owned(),
        source: "declare author ;\nprocess = _;\n".to_owned(),
        expect_valid: false,
    });
    cases.push(Case {
        name: "doc_notice_listing_metadata".to_owned(),
        source: concat!(
            "<mdoc>",
            "<notice/>",
            "<listingdependencies=\"true\"/>",
            "<metadata>author</metadata>",
            "</mdoc>",
            "process = _;\n",
        )
        .to_owned(),
        expect_valid: true,
    });

    Ok(cases)
}

fn git_head_short(path: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("rev-parse")
        .arg("--short")
        .arg("HEAD")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let txt = String::from_utf8(out.stdout).ok()?;
    Some(txt.trim().to_owned())
}

#[test]
fn differential_parse_recovery_against_cpp_reference() {
    let Some(cpp_bin) = cpp_bin() else {
        eprintln!(
            "Skipping differential test: FAUST_CPP_BIN not set and /usr/local/bin/faust not found"
        );
        return;
    };

    if !cpp_bin.exists() {
        eprintln!(
            "Skipping differential test: C++ binary not found at {}",
            cpp_bin.display()
        );
        return;
    }

    let cpp_commit = git_head_short(CPP_SOURCE_ROOT).unwrap_or_else(|| "unknown".to_owned());
    let mut rows = Vec::new();
    let cases = load_cases().expect("cases should load");

    for case in &cases {
        let (rust, rust_errors, rust_recoveries) = rust_class_for(&case.source, &case.name);
        let (cpp, cpp_status) = cpp_class_for(&cpp_bin, &case.source, &case.name)
            .unwrap_or_else(|e| panic!("C++ run failed for {}: {e}", case.name));

        rows.push(DiffRow {
            name: case.name.clone(),
            rust,
            cpp,
            rust_errors,
            rust_recoveries,
            cpp_status,
        });
    }

    eprintln!(
        "Differential parser run with C++ source root {} @ {} and binary {}",
        CPP_SOURCE_ROOT,
        cpp_commit,
        cpp_bin.display()
    );
    for row in &rows {
        eprintln!(
            "{:30} rust={:?} (errs={}, rec={}) cpp={:?} (status={})",
            row.name, row.rust, row.rust_errors, row.rust_recoveries, row.cpp, row.cpp_status
        );
    }

    let mut mismatches = Vec::new();
    for (case, row) in cases.iter().zip(rows.iter()) {
        if case.expect_valid {
            if !(row.rust == RustClass::Ok && row.cpp == CppClass::Ok) {
                mismatches.push(format!(
                    "{} expected valid but got rust={:?} cpp={:?}",
                    row.name, row.rust, row.cpp
                ));
            }
        } else if row.rust == RustClass::Ok || row.cpp == CppClass::Ok {
            mismatches.push(format!(
                "{} expected malformed but got rust={:?} cpp={:?}",
                row.name, row.rust, row.cpp
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "Differential parse mismatches:\n{}",
        mismatches.join("\n")
    );
}
