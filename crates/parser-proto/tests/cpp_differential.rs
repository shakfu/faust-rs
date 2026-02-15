use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use parser_proto::{parse_file_with_imports, parse_program};

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
    input: CaseInput,
    expect_valid: bool,
}

#[derive(Debug)]
enum CaseInput {
    Inline(String),
    FileFixture(FileFixture),
}

#[derive(Debug)]
struct FileFixture {
    entry_rel_path: String,
    search_paths: Vec<String>,
    files: Vec<(String, String)>,
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus")
}

fn rust_class_for_inline(source: &str, source_name: &str) -> (RustClass, usize, u32) {
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

fn temp_case_root(case_name: &str) -> PathBuf {
    let mut root = std::env::temp_dir();
    root.push(format!(
        "faust_rs_parser_diff_fixture_{}_{}_{}",
        std::process::id(),
        case_name,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos()
    ));
    root
}

fn write_fixture(root: &Path, fixture: &FileFixture) -> Result<(), String> {
    for (rel, content) in &fixture.files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create fixture dir {}: {e}", parent.display()))?;
        }
        fs::write(&path, content)
            .map_err(|e| format!("cannot write fixture file {}: {e}", path.display()))?;
    }
    Ok(())
}

fn rust_class_for_fixture(fixture: &FileFixture, case_name: &str) -> (RustClass, usize, u32) {
    let root = temp_case_root(case_name);
    let result = (|| -> Result<(RustClass, usize, u32), String> {
        fs::create_dir_all(&root)
            .map_err(|e| format!("cannot create fixture root {}: {e}", root.display()))?;
        write_fixture(&root, fixture)?;
        let entry = root.join(&fixture.entry_rel_path);
        let search_paths: Vec<PathBuf> =
            fixture.search_paths.iter().map(|p| root.join(p)).collect();
        match parse_file_with_imports(&entry, &search_paths) {
            Ok(out) => {
                let parse_errors = out.state.ctx.parse_error_count();
                let recoveries = out.state.ctx.recovery_count();
                let rust_class = if out.root.is_none() {
                    RustClass::Error
                } else if parse_errors == 0 && out.errors.is_empty() {
                    RustClass::Ok
                } else {
                    RustClass::Recovered
                };
                Ok((rust_class, out.errors.len(), recoveries))
            }
            Err(_) => Ok((RustClass::Error, 0, 0)),
        }
    })();
    let _ = fs::remove_dir_all(&root);
    result.unwrap_or((RustClass::Error, 0, 0))
}

fn rust_class_for_case(case: &Case) -> (RustClass, usize, u32) {
    match &case.input {
        CaseInput::Inline(source) => rust_class_for_inline(source, &case.name),
        CaseInput::FileFixture(fixture) => rust_class_for_fixture(fixture, &case.name),
    }
}

fn classify_cpp_output(output: &std::process::Output) -> (CppClass, i32) {
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
    (class, status_code)
}

fn cpp_class_for_inline(
    cpp_bin: &Path,
    source: &str,
    case_name: &str,
) -> Result<(CppClass, i32), String> {
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

    Ok(classify_cpp_output(&output))
}

fn cpp_class_for_fixture(
    cpp_bin: &Path,
    fixture: &FileFixture,
    case_name: &str,
) -> Result<(CppClass, i32), String> {
    let root = temp_case_root(case_name);
    let result = (|| -> Result<(CppClass, i32), String> {
        fs::create_dir_all(&root)
            .map_err(|e| format!("cannot create fixture root {}: {e}", root.display()))?;
        write_fixture(&root, fixture)?;
        let entry = root.join(&fixture.entry_rel_path);
        let mut out_path = root.join("out.c");
        if out_path == entry {
            out_path = root.join("out_generated.c");
        }

        let mut cmd = Command::new(cpp_bin);
        cmd.arg(&entry)
            .arg("-lang")
            .arg("c")
            .arg("-o")
            .arg(&out_path);
        for rel in &fixture.search_paths {
            cmd.arg("-I").arg(root.join(rel));
        }
        let output = cmd
            .output()
            .map_err(|e| format!("failed to run {}: {e}", cpp_bin.display()))?;
        Ok(classify_cpp_output(&output))
    })();
    let _ = fs::remove_dir_all(&root);
    result
}

fn cpp_class_for_case(cpp_bin: &Path, case: &Case) -> Result<(CppClass, i32), String> {
    match &case.input {
        CaseInput::Inline(source) => cpp_class_for_inline(cpp_bin, source, &case.name),
        CaseInput::FileFixture(fixture) => cpp_class_for_fixture(cpp_bin, fixture, &case.name),
    }
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
            input: CaseInput::Inline(source),
            expect_valid: true,
        });
    }

    cases.push(Case {
        name: "malformed_empty_rhs".to_owned(),
        input: CaseInput::Inline("process = ;\n".to_owned()),
        expect_valid: false,
    });
    cases.push(Case {
        name: "malformed_missing_rpar".to_owned(),
        input: CaseInput::Inline("process = hslider(\"g\", 0.5, 0.0, 1.0, 0.01;\n".to_owned()),
        expect_valid: false,
    });
    cases.push(Case {
        name: "declare_metadata".to_owned(),
        input: CaseInput::Inline("declare author \"letz\";\nprocess = _;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "declare_definition_metadata".to_owned(),
        input: CaseInput::Inline("declare proc category \"ui\";\nprocess = _;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "malformed_declare_missing_value".to_owned(),
        input: CaseInput::Inline("declare author ;\nprocess = _;\n".to_owned()),
        expect_valid: false,
    });
    cases.push(Case {
        name: "doc_notice_listing_metadata".to_owned(),
        input: CaseInput::Inline(
            concat!(
                "<mdoc>",
                "<notice/>",
                "<listingdependencies=\"true\"/>",
                "<metadata>author</metadata>",
                "</mdoc>",
                "process = _;\n",
            )
            .to_owned(),
        ),
        expect_valid: true,
    });
    cases.push(Case {
        name: "with_local_def".to_owned(),
        input: CaseInput::Inline("process = _ with { a = _; };\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "letrec_basic".to_owned(),
        input: CaseInput::Inline("process = _ letrec { 'x = _; };\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "waveform_numbers".to_owned(),
        input: CaseInput::Inline("process = waveform { 1, -2, 3.5 };\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "foreign_fconstant".to_owned(),
        input: CaseInput::Inline("process = fconstant(int fSamplingFreq, <math.h>);\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "foreign_fvariable".to_owned(),
        input: CaseInput::Inline("process = fvariable(int count, <math.h>);\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "foreign_ffunction".to_owned(),
        input: CaseInput::Inline(
            "process = ffunction(float sinhf|sinh|sinhl(float), <math.h>, \"\");\n".to_owned(),
        ),
        expect_valid: true,
    });
    cases.push(Case {
        name: "case_single_rule".to_owned(),
        input: CaseInput::Inline("process = case { (x) => x; };\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "case_arity_mismatch".to_owned(),
        input: CaseInput::Inline("process = case { (x) => x; (x, y) => x; };\n".to_owned()),
        expect_valid: false,
    });
    cases.push(Case {
        name: "lambda_identity".to_owned(),
        input: CaseInput::Inline("process = \\(x).(x);\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "modulation_single".to_owned(),
        input: CaseInput::Inline("process = [\"gain\" : _ -> _];\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "modulation_chain".to_owned(),
        input: CaseInput::Inline("process = [\"a\" : _, \"b\" : _ -> _];\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "malformed_modulation_missing_rcroc".to_owned(),
        input: CaseInput::Inline("process = [\"gain\" : _ -> _;\n".to_owned()),
        expect_valid: false,
    });
    cases.push(Case {
        name: "vgroup_basic".to_owned(),
        input: CaseInput::Inline("process = vgroup(\"g\", _);\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "stream_wrappers".to_owned(),
        input: CaseInput::Inline(
            "process = inputs(_), outputs(_), ondemand(_), upsampling(_), downsampling(_);\n"
                .to_owned(),
        ),
        expect_valid: true,
    });
    cases.push(Case {
        name: "int_cast_primitive".to_owned(),
        input: CaseInput::Inline("process = _ : int;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "float_cast_primitive".to_owned(),
        input: CaseInput::Inline("process = _ : float;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "prefix_primitive".to_owned(),
        input: CaseInput::Inline("process = 0, _ : prefix;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "rdtable_primitive".to_owned(),
        input: CaseInput::Inline("process = 4, 0, _ : rdtable;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "rwtable_primitive".to_owned(),
        input: CaseInput::Inline("process = 4, 0, 0, _, _ : rwtable;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "select2_primitive".to_owned(),
        input: CaseInput::Inline("process = _, _, _ : select2;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "select3_primitive".to_owned(),
        input: CaseInput::Inline("process = _, _, _, _ : select3;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "lowest_primitive".to_owned(),
        input: CaseInput::Inline("process = _ : lowest;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "highest_primitive".to_owned(),
        input: CaseInput::Inline("process = _ : highest;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "attach_primitive".to_owned(),
        input: CaseInput::Inline("process = _, _ : attach;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "enable_primitive".to_owned(),
        input: CaseInput::Inline("process = _, _ : enable;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "control_primitive".to_owned(),
        input: CaseInput::Inline("process = _, _ : control;\n".to_owned()),
        expect_valid: true,
    });
    cases.push(Case {
        name: "import_nested_search_path".to_owned(),
        input: CaseInput::FileFixture(FileFixture {
            entry_rel_path: "src/main_ok.dsp".to_owned(),
            search_paths: vec!["libs".to_owned()],
            files: vec![
                (
                    "src/main_ok.dsp".to_owned(),
                    "import(\"gain.lib\");\nprocess = gain;\n".to_owned(),
                ),
                (
                    "libs/gain.lib".to_owned(),
                    "import(\"core/base.lib\");\ngain = base;\n".to_owned(),
                ),
                ("libs/core/base.lib".to_owned(), "base = _;\n".to_owned()),
            ],
        }),
        expect_valid: true,
    });
    cases.push(Case {
        name: "import_missing_search_path".to_owned(),
        input: CaseInput::FileFixture(FileFixture {
            entry_rel_path: "src/main_missing.dsp".to_owned(),
            search_paths: vec!["libs".to_owned()],
            files: vec![(
                "src/main_missing.dsp".to_owned(),
                "import(\"missing_gain.lib\");\nprocess = _;\n".to_owned(),
            )],
        }),
        expect_valid: false,
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
        let (rust, rust_errors, rust_recoveries) = rust_class_for_case(case);
        let (cpp, cpp_status) = cpp_class_for_case(&cpp_bin, case)
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
