use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "\
Usage:
  cargo run -p xtask -- golden-check
  cargo run -p xtask -- golden-check-cpp
  cargo run -p xtask -- golden-gen-rust
  cargo run -p xtask -- golden-gen-cpp [-- <extra args passed to FAUST_CPP_BIN>]
\nEnvironment for golden-gen-cpp:
  FAUST_CPP_BIN   Path to reference C++ faust binary
\nEnvironment for golden-check:
  GOLDEN_REF      rust (default) or cpp
";

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
