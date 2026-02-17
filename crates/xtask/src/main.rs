use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;

const USAGE: &str = "\
Usage:
  cargo run -p xtask -- golden-check
  cargo run -p xtask -- golden-check-cpp
  cargo run -p xtask -- golden-gen-rust
  cargo run -p xtask -- golden-gen-cpp [-- <extra args passed to FAUST_CPP_BIN>]
  cargo run -p xtask -- parser-parity-report
  cargo run -p xtask -- corpus-status-report
  cargo run -p xtask -- cpp-backend-diff-report
\nEnvironment for golden-gen-cpp:
  FAUST_CPP_BIN   Path to reference C++ faust binary
\nEnvironment for golden-check:
  GOLDEN_REF      rust (default) or cpp
";

const CPP_SOURCE_ROOT: &str = "/Users/letz/Developpements/RUST/faust";
const PARITY_REPORT_REL_PATH: &str = "porting/phases/phase-3-parser-parity-report-en.md";
const CORPUS_STATUS_REPORT_REL_PATH: &str =
    "porting/phases/phase-4-corpus-status-diff-report-en.md";
const CPP_BACKEND_DIFF_REPORT_REL_PATH: &str =
    "porting/phases/phase-6-cpp-backend-diff-report-en.md";

fn main() {
    if let Err(err) = run() {
        eprintln!("xtask error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print!("{USAGE}");
        return Ok(());
    };

    match command.as_str() {
        "golden-check" => golden_check(None)?,
        "golden-check-cpp" => golden_check(Some(GoldenRef::Cpp))?,
        "golden-gen-rust" => golden_gen_rust()?,
        "golden-gen-cpp" => {
            let mut passthrough: Vec<OsString> = Vec::new();
            let mut separator_seen = false;
            for arg in args {
                if separator_seen {
                    passthrough.push(OsString::from(arg));
                } else if arg == "--" {
                    separator_seen = true;
                }
            }
            golden_gen_cpp(&passthrough)?;
        }
        "parser-parity-report" => parser_parity_report()?,
        "corpus-status-report" => corpus_status_report()?,
        "cpp-backend-diff-report" => cpp_backend_diff_report()?,
        _ => {
            print!("{USAGE}");
        }
    }

    Ok(())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .to_path_buf()
        })
}

fn corpus_files() -> Result<Vec<PathBuf>, io::Error> {
    let root = workspace_root();
    let corpus_dir = root.join("tests/corpus");
    let mut files = Vec::new();

    for entry in fs::read_dir(corpus_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "dsp") {
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

fn case_name(path: &Path) -> Result<String, io::Error> {
    path.file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid corpus filename"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GoldenRef {
    Rust,
    Cpp,
}

impl GoldenRef {
    fn as_dir_name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Cpp => "cpp",
        }
    }
}

fn golden_file_for_ref(case: &str, golden_ref: GoldenRef) -> PathBuf {
    workspace_root()
        .join("tests/golden")
        .join(golden_ref.as_dir_name())
        .join(case)
        .join("compiler_stdout.txt")
}

fn normalize(text: &str) -> String {
    let mut normalized = text.replace("\r\n", "\n");
    let mut lines: Vec<String> = normalized
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect();

    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }

    normalized = lines.join("\n");
    normalized.push('\n');
    normalized
}

fn render_rust_snapshot(input: &Path) -> Result<String, io::Error> {
    let source = fs::read_to_string(input)?;
    let name = input
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid input filename"))?;
    Ok(compiler::golden_snapshot(name, &source))
}

fn golden_gen_rust() -> Result<(), Box<dyn std::error::Error>> {
    let files = corpus_files()?;
    for file in files {
        let case = case_name(&file)?;
        let output = golden_file_for_ref(&case, GoldenRef::Rust);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let snapshot = normalize(&render_rust_snapshot(&file)?);
        fs::write(&output, snapshot)?;
        println!("updated {}", output.display());
    }
    Ok(())
}

fn golden_gen_cpp(extra_args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let cpp_bin = std::env::var_os("FAUST_CPP_BIN").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "FAUST_CPP_BIN is not set. Example: FAUST_CPP_BIN=/path/to/faust",
        )
    })?;

    let files = corpus_files()?;
    for file in files {
        let case = case_name(&file)?;
        let output = golden_file_for_ref(&case, GoldenRef::Cpp);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut cmd = Command::new(&cpp_bin);
        cmd.arg(&file);
        for arg in extra_args {
            cmd.arg(arg);
        }

        let result = cmd.output()?;
        if !result.status.success() {
            return Err(format!(
                "C++ reference command failed for {} with status {}",
                file.display(),
                result.status
            )
            .into());
        }

        let stdout = String::from_utf8(result.stdout)?;
        fs::write(&output, normalize(&stdout))?;
        println!("updated {}", output.display());
    }

    Ok(())
}

fn golden_ref_from_env() -> Result<GoldenRef, Box<dyn std::error::Error>> {
    let Some(raw) = std::env::var_os("GOLDEN_REF") else {
        return Ok(GoldenRef::Rust);
    };
    let value = raw
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid GOLDEN_REF value"))?;
    match value {
        "rust" => Ok(GoldenRef::Rust),
        "cpp" => Ok(GoldenRef::Cpp),
        _ => Err(format!("invalid GOLDEN_REF={value}; expected rust or cpp").into()),
    }
}

fn golden_check(forced: Option<GoldenRef>) -> Result<(), Box<dyn std::error::Error>> {
    let golden_ref = match forced {
        Some(value) => value,
        None => golden_ref_from_env()?,
    };

    let files = corpus_files()?;
    let mut failures = 0usize;

    for file in files {
        let case = case_name(&file)?;
        let expected_path = golden_file_for_ref(&case, golden_ref);
        let expected = fs::read_to_string(&expected_path).map_err(|err| {
            io::Error::new(
                err.kind(),
                format!(
                    "missing golden file {} (run golden-gen-rust or golden-gen-cpp): {err}",
                    expected_path.display()
                ),
            )
        })?;

        let actual = normalize(&render_rust_snapshot(&file)?);
        let expected = normalize(&expected);

        if actual != expected {
            failures += 1;
            println!("[FAIL] {case}");
            println!("  expected: {}", expected_path.display());
            println!("  first diff:");
            print_first_diff(&expected, &actual);
        } else {
            println!("[OK] {case}");
        }
    }

    if failures > 0 {
        return Err(format!("golden-check failed: {failures} case(s) differ").into());
    }

    Ok(())
}

fn print_first_diff(expected: &str, actual: &str) {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let max = expected_lines.len().max(actual_lines.len());

    for idx in 0..max {
        let e = expected_lines.get(idx).copied().unwrap_or("<missing>");
        let a = actual_lines.get(idx).copied().unwrap_or("<missing>");
        if e != a {
            println!("    line {}", idx + 1);
            println!("      expected: {e}");
            println!("      actual:   {a}");
            return;
        }
    }
}

fn parser_parity_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let cpp_root = PathBuf::from(CPP_SOURCE_ROOT);

    let cpp_parser = cpp_root.join("compiler/parser/faustparser.y");
    let cpp_lexer = cpp_root.join("compiler/parser/faustlexer.l");
    let rust_parser = root.join("crates/parser-proto/src/grammar/faustparser.y");
    let rust_lexer = root.join("crates/parser-proto/src/grammar/faustlexer.l");
    let report_path = root.join(PARITY_REPORT_REL_PATH);

    for path in [&cpp_parser, &cpp_lexer, &rust_parser, &rust_lexer] {
        if !path.exists() {
            return Err(format!("missing input file for parity report: {}", path.display()).into());
        }
    }

    let cpp_parser_src = fs::read_to_string(&cpp_parser)?;
    let cpp_lexer_src = fs::read_to_string(&cpp_lexer)?;
    let rust_parser_src = fs::read_to_string(&rust_parser)?;
    let rust_lexer_src = fs::read_to_string(&rust_lexer)?;

    let cpp_parser_tokens = extract_parser_tokens(&cpp_parser_src);
    let rust_parser_tokens = extract_parser_tokens(&rust_parser_src);
    let cpp_lexer_tokens = extract_cpp_lexer_emitted_tokens(&cpp_lexer_src);
    let rust_lexer_tokens = extract_rust_lexer_emitted_tokens(&rust_lexer_src);
    let cpp_lexer_states = extract_lexer_states(&cpp_lexer_src);
    let rust_lexer_states = extract_lexer_states(&rust_lexer_src);
    let cpp_nonterms = extract_cpp_nonterminals(&cpp_parser_src);
    let rust_nonterms = extract_rust_nonterminals(&rust_parser_src);

    let parser_token_extra = diff_sorted(&rust_parser_tokens, &cpp_parser_tokens);
    let parser_token_missing_exact = diff_sorted(&cpp_parser_tokens, &rust_parser_tokens);
    let (parser_token_alias_covered, parser_token_missing_unresolved) = partition_with_aliases(
        &parser_token_missing_exact,
        &rust_parser_tokens,
        token_aliases,
    );

    let lexer_state_extra = diff_sorted(&rust_lexer_states, &cpp_lexer_states);
    let lexer_state_missing = diff_sorted(&cpp_lexer_states, &rust_lexer_states);

    let nonterm_extra = diff_sorted(&rust_nonterms, &cpp_nonterms);
    let nonterm_missing_exact = diff_sorted(&cpp_nonterms, &rust_nonterms);
    let (nonterm_alias_covered, nonterm_missing_unresolved) =
        partition_with_aliases(&nonterm_missing_exact, &rust_nonterms, nonterminal_aliases);

    let cpp_declared_not_lexed = diff_sorted(&cpp_parser_tokens, &cpp_lexer_tokens);
    let rust_declared_not_lexed = diff_sorted(&rust_parser_tokens, &rust_lexer_tokens);
    let cpp_lexed_not_declared = diff_sorted(&cpp_lexer_tokens, &cpp_parser_tokens);
    let rust_lexed_not_declared = diff_sorted(&rust_lexer_tokens, &rust_parser_tokens);

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 3 Parser/Lexer Parity Coverage Report (Auto-generated)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- parser-parity-report`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Inputs")?;
    writeln!(&mut out, "- C++ parser: `{}`", cpp_parser.display())?;
    writeln!(&mut out, "- C++ lexer: `{}`", cpp_lexer.display())?;
    writeln!(&mut out, "- Rust parser: `{}`", rust_parser.display())?;
    writeln!(&mut out, "- Rust lexer: `{}`", rust_lexer.display())?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(
        &mut out,
        "- Parser token coverage: C++ declared `{}` / Rust declared `{}` / unresolved missing `{}`",
        cpp_parser_tokens.len(),
        rust_parser_tokens.len(),
        parser_token_missing_unresolved.len()
    )?;
    writeln!(
        &mut out,
        "- Lexer state coverage: C++ `{}` / Rust `{}` / unresolved missing `{}`",
        cpp_lexer_states.len(),
        rust_lexer_states.len(),
        lexer_state_missing.len()
    )?;
    writeln!(
        &mut out,
        "- Grammar nonterminal coverage (name-based): C++ `{}` / Rust `{}` / unresolved missing `{}`",
        cpp_nonterms.len(),
        rust_nonterms.len(),
        nonterm_missing_unresolved.len()
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Parser Tokens (C++ `%token` vs Rust `%token`)")?;
    writeln!(
        &mut out,
        "_Note: `exact name` mismatches below are not necessarily missing functionality; they can be covered by explicit alias mapping._"
    )?;
    render_list(
        &mut out,
        "Exact-name mismatch candidates (C++ name not present as-is in Rust)",
        &parser_token_missing_exact,
    )?;
    render_alias_list(
        &mut out,
        "Exact-name mismatches covered by explicit alias mapping (no action required)",
        &parser_token_alias_covered,
    )?;
    render_list(
        &mut out,
        "Unresolved missing after alias mapping (action required)",
        &parser_token_missing_unresolved,
    )?;
    render_list(&mut out, "Extra in Rust", &parser_token_extra)?;

    writeln!(&mut out)?;
    writeln!(&mut out, "## Lexer States (`%x`/`%s`)")?;
    render_list(
        &mut out,
        "Missing in Rust lexer state declarations",
        &lexer_state_missing,
    )?;
    render_list(
        &mut out,
        "Extra in Rust lexer state declarations",
        &lexer_state_extra,
    )?;

    writeln!(&mut out)?;
    writeln!(&mut out, "## Grammar Nonterminals (name-based)")?;
    writeln!(
        &mut out,
        "_Note: `exact name` mismatches below are not necessarily missing functionality; they can be covered by explicit alias mapping (for example dedicated C++ rules grouped under `Primitive` in Rust)._"
    )?;
    render_list(
        &mut out,
        "Exact-name mismatch candidates (C++ nonterminal not present as-is in Rust)",
        &nonterm_missing_exact,
    )?;
    render_alias_list(
        &mut out,
        "Exact-name mismatches covered by explicit alias mapping (no action required)",
        &nonterm_alias_covered,
    )?;
    render_list(
        &mut out,
        "Unresolved missing after alias mapping (action required)",
        &nonterm_missing_unresolved,
    )?;
    render_list(&mut out, "Extra in Rust", &nonterm_extra)?;

    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "## Parser/Lexer Internal Consistency (declared tokens vs lexer emissions)"
    )?;
    render_list(
        &mut out,
        "C++ parser-declared tokens not emitted by C++ lexer",
        &cpp_declared_not_lexed,
    )?;
    render_list(
        &mut out,
        "Rust parser-declared tokens not emitted by Rust lexer",
        &rust_declared_not_lexed,
    )?;
    render_list(
        &mut out,
        "C++ lexer-emitted tokens not declared in C++ parser",
        &cpp_lexed_not_declared,
    )?;
    render_list(
        &mut out,
        "Rust lexer-emitted tokens not declared in Rust parser",
        &rust_lexed_not_declared,
    )?;

    let unresolved_total = parser_token_missing_unresolved.len()
        + lexer_state_missing.len()
        + nonterm_missing_unresolved.len();
    let consistency_issues_total = cpp_declared_not_lexed.len()
        + rust_declared_not_lexed.len()
        + cpp_lexed_not_declared.len()
        + rust_lexed_not_declared.len();

    writeln!(&mut out)?;
    writeln!(&mut out, "## Next Actions")?;
    if unresolved_total == 0 {
        writeln!(
            &mut out,
            "- Unresolved missing items after alias mapping are `0` for parser tokens, lexer states, and grammar nonterminals."
        )?;
    } else {
        writeln!(
            &mut out,
            "- Resolve all items listed in `Unresolved missing after alias mapping (action required)` for tokens and nonterminals."
        )?;
    }
    if consistency_issues_total > 0 {
        writeln!(
            &mut out,
            "- Triage items listed under `Parser/Lexer Internal Consistency` (C++ or Rust declared/emitted token mismatches)."
        )?;
    }
    writeln!(
        &mut out,
        "- Keep this report regenerated at each parser/lexer migration increment to track closure toward 100% parity."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

#[derive(Clone, Debug)]
struct CaseStatus {
    ok: bool,
    stage: &'static str,
    reason: String,
}

fn corpus_status_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let report_path = root.join(CORPUS_STATUS_REPORT_REL_PATH);
    let files = corpus_files()?;
    let compiler = compiler::Compiler::new();
    let (cpp_bin, cpp_bin_is_fallback) = resolve_cpp_faust_bin();

    let mut total = 0usize;
    let mut ok_ok = 0usize;
    let mut err_err = 0usize;
    let mut ok_err = 0usize;
    let mut err_ok = 0usize;

    let mut rows = Vec::with_capacity(files.len());
    for file in files {
        let case = case_name(&file)?;
        let cpp = cpp_case_status(&cpp_bin, &file)?;
        let rust = rust_case_status(&compiler, &file);
        total = total.saturating_add(1);

        match (cpp.ok, rust.ok) {
            (true, true) => ok_ok = ok_ok.saturating_add(1),
            (false, false) => err_err = err_err.saturating_add(1),
            (true, false) => ok_err = ok_err.saturating_add(1),
            (false, true) => err_ok = err_ok.saturating_add(1),
        }

        rows.push((case, cpp, rust));
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 4 Corpus C++ vs Rust Status Differential Report (Auto-generated)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- corpus-status-report`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Inputs")?;
    writeln!(&mut out, "- Corpus: `tests/corpus/*.dsp`")?;
    writeln!(&mut out, "- C++ binary: `{}`", cpp_bin.display())?;
    if cpp_bin_is_fallback {
        writeln!(
            &mut out,
            "- Note: fallback to `faust` from PATH because `{}/build/bin/faust` was not found.",
            CPP_SOURCE_ROOT
        )?;
        writeln!(
            &mut out,
            "- Action: set `FAUST_CPP_BIN` explicitly to the source-of-truth C++ binary when available."
        )?;
    }
    writeln!(
        &mut out,
        "- Rust path: `compiler::Compiler::compile_file_default_to_signals`"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(&mut out, "- Total cases: `{total}`")?;
    writeln!(&mut out, "- `OK/OK`: `{ok_ok}`")?;
    writeln!(&mut out, "- `ERR/ERR`: `{err_err}`")?;
    writeln!(&mut out, "- `OK/ERR` (C++ ok, Rust err): `{ok_err}`")?;
    writeln!(&mut out, "- `ERR/OK` (C++ err, Rust ok): `{err_ok}`")?;
    writeln!(&mut out)?;

    writeln!(&mut out, "## Parity Mismatches")?;
    writeln!(
        &mut out,
        "| Case | Class | C++ | Rust stage | Rust reason | C++ reason |"
    )?;
    writeln!(
        &mut out,
        "|------|-------|-----|------------|-------------|------------|"
    )?;
    for (case, cpp, rust) in &rows {
        let class = match (cpp.ok, rust.ok) {
            (true, false) => "OK/ERR",
            (false, true) => "ERR/OK",
            _ => continue,
        };
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` | `{}` | `{}` |",
            case,
            class,
            status_cell(cpp),
            rust.stage,
            markdown_escape(&rust.reason),
            markdown_escape(&cpp.reason),
        )?;
    }
    writeln!(&mut out)?;

    writeln!(&mut out, "## Full Matrix")?;
    writeln!(&mut out, "| Case | C++ | Rust | Rust stage | Rust reason |")?;
    writeln!(&mut out, "|------|-----|------|------------|-------------|")?;
    for (case, cpp, rust) in &rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` | `{}` |",
            case,
            status_cell(cpp),
            status_cell(rust),
            rust.stage,
            markdown_escape(&rust.reason),
        )?;
    }
    writeln!(&mut out)?;
    writeln!(&mut out, "## Next Actions")?;
    writeln!(
        &mut out,
        "- Treat all `OK/ERR` and `ERR/OK` rows as parity tasks in parser/eval/propagate."
    )?;
    writeln!(
        &mut out,
        "- Re-run this report after each parity fix touching `tests/corpus` behavior."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ShellSignature {
    faustclass: Option<String>,
    class_decl: Option<String>,
    has_restrict_define: bool,
    has_exp10_aliases: bool,
}

#[derive(Clone, Debug)]
struct CppDiffRow {
    case: String,
    class: &'static str,
    rust_reason: String,
    cpp_reason: String,
}

fn cpp_backend_diff_report() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let report_path = root.join(CPP_BACKEND_DIFF_REPORT_REL_PATH);
    let compiler = compiler::Compiler::new();
    let (cpp_bin, cpp_bin_is_fallback) = resolve_cpp_faust_bin();
    let options = codegen::backends::cpp::CppOptions {
        class_name: Some("mydsp".to_owned()),
        ..codegen::backends::cpp::CppOptions::default()
    };

    let representative = [
        "rep_01_passthrough.dsp",
        "rep_05_one_pole_lowpass.dsp",
        "rep_09_ui_slider.dsp",
        "rep_17_ui_groups.dsp",
        "rep_20_environment_waveform.dsp",
        "rep_22_parallel_mix.dsp",
        "rep_28_nested_ui_groups.dsp",
        "rep_31_extended_primitives.dsp",
    ];

    let mut rows = Vec::with_capacity(representative.len());
    let mut ok = 0usize;
    let mut diff = 0usize;
    let mut unsupported = 0usize;

    for case in representative {
        let path = root.join("tests").join("corpus").join(case);
        let rust_output = compiler.compile_file_default_to_cpp(&path, &options);
        let cpp_output = Command::new(&cpp_bin).arg(&path).output()?;

        let row = match (rust_output, cpp_output.status.success()) {
            (Ok(rust_text), true) => {
                let cpp_text = String::from_utf8(cpp_output.stdout)?;
                let rust_sig = extract_shell_signature(&rust_text);
                let cpp_sig = extract_shell_signature(&cpp_text);
                if rust_sig == cpp_sig {
                    ok = ok.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "OK",
                        rust_reason: "shell signature matches".to_owned(),
                        cpp_reason: "ok".to_owned(),
                    }
                } else {
                    diff = diff.saturating_add(1);
                    CppDiffRow {
                        case: case.to_owned(),
                        class: "DIFF",
                        rust_reason: format!("rust={rust_sig:?}"),
                        cpp_reason: format!("cpp={cpp_sig:?}"),
                    }
                }
            }
            (Ok(_), false) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: "Rust path ok".to_owned(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
            (Err(err), true) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: "C++ path ok".to_owned(),
                }
            }
            (Err(err), false) => {
                unsupported = unsupported.saturating_add(1);
                CppDiffRow {
                    case: case.to_owned(),
                    class: "UNSUPPORTED",
                    rust_reason: err.to_string(),
                    cpp_reason: first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stderr))
                        .or_else(|| {
                            first_non_empty_line(&String::from_utf8_lossy(&cpp_output.stdout))
                        })
                        .unwrap_or_else(|| format!("failed with status {}", cpp_output.status)),
                }
            }
        };
        rows.push(row);
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "# Phase 6 C++ Backend Differential Report (Module-First, Shell-Normalized)"
    )?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "Generated by: `cargo run -p xtask -- cpp-backend-diff-report`"
    )?;
    writeln!(&mut out, "- C++ binary: `{}`", cpp_bin.display())?;
    if cpp_bin_is_fallback {
        writeln!(
            &mut out,
            "- Note: fallback to `faust` from PATH because `{}/build/bin/faust` was not found.",
            CPP_SOURCE_ROOT
        )?;
    }
    writeln!(
        &mut out,
        "- Normalization: compare module-shell signature only"
    )?;
    writeln!(&mut out, "  - `#define FAUSTCLASS <name>`")?;
    writeln!(&mut out, "  - `class <name> : public dsp`")?;
    writeln!(
        &mut out,
        "  - presence of `RESTRICT` and Apple `exp10` aliases"
    )?;
    writeln!(&mut out)?;
    writeln!(&mut out, "## Summary")?;
    writeln!(&mut out, "- Cases: `{}`", rows.len())?;
    writeln!(&mut out, "- `OK`: `{ok}`")?;
    writeln!(&mut out, "- `DIFF`: `{diff}`")?;
    writeln!(&mut out, "- `UNSUPPORTED`: `{unsupported}`")?;
    writeln!(&mut out)?;
    writeln!(
        &mut out,
        "| Case | Status | Rust detail | C++ detail |\n|------|--------|-------------|------------|"
    )?;
    for row in &rows {
        writeln!(
            &mut out,
            "| `{}` | `{}` | `{}` | `{}` |",
            row.case,
            row.class,
            markdown_escape(&row.rust_reason),
            markdown_escape(&row.cpp_reason)
        )?;
    }
    writeln!(&mut out)?;
    writeln!(&mut out, "## Notes")?;
    writeln!(
        &mut out,
        "- This report tracks module-shell parity while full production signal->FIR lowering is still in progress."
    )?;
    writeln!(
        &mut out,
        "- `DIFF` rows are expected to shrink as statement/value lowering and orchestration parity advance."
    )?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, out)?;
    println!("updated {}", report_path.display());
    Ok(())
}

fn extract_shell_signature(text: &str) -> ShellSignature {
    let mut faustclass = None::<String>;
    let mut class_decl = None::<String>;
    let mut has_restrict_define = false;
    let mut has_exp10f_alias = false;
    let mut has_exp10_alias = false;

    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("#define FAUSTCLASS ") {
            faustclass = Some(rest.trim().to_owned());
        }
        if let Some(rest) = line.strip_prefix("class ")
            && let Some((name, _)) = rest.split_once(" : public dsp")
        {
            class_decl = Some(name.trim().to_owned());
        }
        if line.contains("#define RESTRICT") {
            has_restrict_define = true;
        }
        if line == "#define exp10f __exp10f" {
            has_exp10f_alias = true;
        }
        if line == "#define exp10 __exp10" {
            has_exp10_alias = true;
        }
    }

    ShellSignature {
        faustclass,
        class_decl,
        has_restrict_define,
        has_exp10_aliases: has_exp10f_alias && has_exp10_alias,
    }
}

fn resolve_cpp_faust_bin() -> (PathBuf, bool) {
    if let Some(path) = std::env::var_os("FAUST_CPP_BIN") {
        return (PathBuf::from(path), false);
    }
    let built = Path::new(CPP_SOURCE_ROOT).join("build/bin/faust");
    if built.exists() {
        return (built, false);
    }
    (PathBuf::from("faust"), true)
}

fn cpp_case_status(cpp_bin: &Path, input: &Path) -> Result<CaseStatus, Box<dyn std::error::Error>> {
    let status = Command::new(cpp_bin)
        .arg(input)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        return Ok(CaseStatus {
            ok: true,
            stage: "ok",
            reason: "ok".to_owned(),
        });
    }

    let output = Command::new(cpp_bin).arg(input).output()?;
    let reason = first_non_empty_line(&String::from_utf8_lossy(&output.stderr))
        .or_else(|| first_non_empty_line(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_else(|| format!("failed with status {}", output.status));
    Ok(CaseStatus {
        ok: false,
        stage: "error",
        reason,
    })
}

fn rust_case_status(compiler: &compiler::Compiler, input: &Path) -> CaseStatus {
    match compiler.compile_file_default_to_signals(input) {
        Ok(_) => CaseStatus {
            ok: true,
            stage: "ok",
            reason: "ok".to_owned(),
        },
        Err(err) => {
            let (stage, reason) = match &err {
                compiler::CompilerError::Import(_) => ("import", err.to_string()),
                compiler::CompilerError::Parse { .. } => ("parse", err.to_string()),
                compiler::CompilerError::Eval { .. } => ("eval", err.to_string()),
                compiler::CompilerError::Propagate { .. } => ("propagate", err.to_string()),
                compiler::CompilerError::Codegen { .. } => ("codegen", err.to_string()),
                compiler::CompilerError::MissingRoot { .. } => ("parse", err.to_string()),
            };
            CaseStatus {
                ok: false,
                stage,
                reason,
            }
        }
    }
}

fn status_cell(status: &CaseStatus) -> &'static str {
    if status.ok { "OK" } else { "ERR" }
}

fn first_non_empty_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn markdown_escape(value: &str) -> String {
    value.replace('|', "\\|").replace(['\n', '\r'], " ")
}

fn extract_parser_tokens(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let rest = if let Some(rest) = trimmed.strip_prefix("%token") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("%left") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("%right") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("%nonassoc") {
            rest
        } else {
            continue;
        };
        let rest = rest.trim();
        for raw in rest.split_whitespace() {
            let part = raw.trim_matches(|c: char| c == ',' || c == ';');
            if part.starts_with('<') || part.starts_with("/*") || part.starts_with("//") {
                continue;
            }
            if is_token_name(part) {
                set.insert(part.to_owned());
            }
        }
    }
    set
}

fn extract_cpp_nonterminals(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in grammar_section(source).lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('%') || trimmed.starts_with('|') {
            continue;
        }
        let Some((head, _)) = trimmed.split_once(':') else {
            continue;
        };
        let head = head.trim();
        if is_ident_name(head) {
            set.insert(head.to_ascii_lowercase());
        }
    }
    set
}

fn extract_rust_nonterminals(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in grammar_section(source).lines() {
        let trimmed = line.trim_start();
        let Some((head, _)) = trimmed.split_once("->") else {
            continue;
        };
        let head = head.trim();
        if is_ident_name(head) {
            set.insert(head.to_ascii_lowercase());
        }
    }
    set
}

fn extract_lexer_states(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let rest = if let Some(rest) = trimmed.strip_prefix("%x") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("%s") {
            rest
        } else {
            continue;
        };
        for state in rest.split_whitespace() {
            let state = state.trim_matches(|c: char| c == ';');
            if is_ident_name(state) {
                set.insert(state.to_ascii_lowercase());
            }
        }
    }
    set
}

fn extract_cpp_lexer_emitted_tokens(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        let mut rest = line;
        while let Some(idx) = rest.find("return ") {
            let after = &rest[idx + "return ".len()..];
            if let Some(token) = scan_token_name(after) {
                set.insert(token);
            }
            rest = &after[after.char_indices().nth(1).map_or(after.len(), |(i, _)| i)..];
        }
    }
    set
}

fn extract_rust_lexer_emitted_tokens(source: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in source.lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] != '\'' {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '\'' {
                j += 1;
            }
            if j >= chars.len() {
                break;
            }
            let candidate: String = chars[i + 1..j].iter().collect();
            if is_token_name(&candidate) {
                set.insert(candidate);
            }
            // Move one character forward so overlapping quotes are still discovered.
            i += 1;
        }
    }
    set
}

fn grammar_section(source: &str) -> &str {
    let mut marks = source.match_indices("%%");
    let Some((first, _)) = marks.next() else {
        return source;
    };
    let Some((second, _)) = marks.next() else {
        return &source[first + 2..];
    };
    &source[first + 2..second]
}

fn is_token_name(s: &str) -> bool {
    let mut has_upper = false;
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            has_upper = true;
        } else if !(c.is_ascii_digit() || c == '_') {
            return false;
        }
    }
    has_upper
}

fn is_ident_name(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn scan_token_name(source: &str) -> Option<String> {
    let mut start = None;
    for (idx, c) in source.char_indices() {
        if c.is_ascii_uppercase() || c == '_' {
            start = Some(idx);
            break;
        }
    }
    let start = start?;
    let token: String = source[start..]
        .chars()
        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
        .collect();
    if is_token_name(&token) {
        Some(token)
    } else {
        None
    }
}

fn diff_sorted(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Vec<String> {
    left.difference(right).cloned().collect()
}

fn token_aliases(cpp_name: &str) -> &'static [&'static str] {
    match cpp_name {
        "VIRG" => &["PAR"],
        "LISTING" => &["BLST"],
        _ => &[],
    }
}

fn nonterminal_aliases(cpp_name: &str) -> &'static [&'static str] {
    match cpp_name {
        "params" => &["paramlist"],
        "recinition" => &["recdefinition"],
        "ident" => &["identexpr"],
        "fun" => &["funname"],
        "string" => &["rawstring", "uqstring", "fstring"],
        "doc" => &["doccontent"],
        "doctxt" | "doceqn" | "docdgm" | "docmtd" | "doclst" | "docntc" => &["docelem"],
        "lstattrdef" => &["lstattr"],
        "lstattrval" => &["lstattrvalue"],
        "ffunction" | "fconst" | "fvariable" | "fpar" | "fseq" | "fsum" | "fprod" | "finputs"
        | "foutputs" | "fondemand" | "fupsampling" | "fdownsampling" | "button" | "checkbox"
        | "vslider" | "hslider" | "nentry" | "vgroup" | "hgroup" | "tgroup" | "vbargraph"
        | "hbargraph" | "soundfile" => &["primitive"],
        _ => &[],
    }
}

fn partition_with_aliases(
    missing_exact: &[String],
    rust_set: &BTreeSet<String>,
    aliases: impl Fn(&str) -> &'static [&'static str],
) -> (Vec<(String, Vec<String>)>, Vec<String>) {
    let mut covered = Vec::new();
    let mut unresolved = Vec::new();

    for item in missing_exact {
        let mapped_hits = aliases(item)
            .iter()
            .copied()
            .filter(|candidate| rust_set.contains(*candidate))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if mapped_hits.is_empty() {
            unresolved.push(item.clone());
        } else {
            covered.push((item.clone(), mapped_hits));
        }
    }
    (covered, unresolved)
}

fn render_list(out: &mut String, title: &str, items: &[String]) -> Result<(), std::fmt::Error> {
    writeln!(out, "### {title}")?;
    if items.is_empty() {
        writeln!(out, "- (none)")?;
    } else {
        for item in items {
            writeln!(out, "- `{item}`")?;
        }
    }
    Ok(())
}

fn render_alias_list(
    out: &mut String,
    title: &str,
    items: &[(String, Vec<String>)],
) -> Result<(), std::fmt::Error> {
    writeln!(out, "### {title}")?;
    if items.is_empty() {
        writeln!(out, "- (none)")?;
        return Ok(());
    }
    for (source, targets) in items {
        let mapped = targets
            .iter()
            .map(|v| format!("`{v}`"))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(out, "- `{source}` -> {mapped}")?;
    }
    Ok(())
}
