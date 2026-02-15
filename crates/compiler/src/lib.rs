//! Top-level compiler facade crate.
//!
//! # Source provenance (C++)
//! - `compiler/libcode.cpp` (compile entry points and orchestration)
//! - `compiler/global.cpp` (session lifecycle)
//!
//! # Current scope
//! - Exposes minimal compile-session APIs.
//! - Wires parsing through production `crates/parser` APIs.

use std::path::{Path, PathBuf};

use parser::{ParseOutput, SourceReaderError};

pub struct Compiler;

impl Compiler {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    #[must_use]
    pub fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Parses one source string through the production parser crate.
    ///
    /// Returns [`CompilerError::Parse`] when parser recovery/errors are present.
    pub fn compile_source(
        &self,
        source_name: &str,
        source: &str,
    ) -> Result<ParseOutput, CompilerError> {
        let output = parser::parse_program(source, source_name);
        ensure_parse_success(source_name, output)
    }

    /// Parses one source file and expands local imports using `search_paths`.
    ///
    /// Returns [`CompilerError::Import`] for import resolution/cycle failures.
    pub fn compile_file(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
    ) -> Result<ParseOutput, CompilerError> {
        let output =
            parser::parse_file_with_imports(path, search_paths).map_err(CompilerError::Import)?;
        ensure_parse_success(&path.display().to_string(), output)
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Compiler facade errors for parser-stage orchestration.
#[derive(Debug)]
pub enum CompilerError {
    Import(SourceReaderError),
    Parse {
        source: Box<str>,
        parse_errors: usize,
        recoveries: u32,
    },
}

impl std::fmt::Display for CompilerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Import(err) => write!(f, "{err}"),
            Self::Parse {
                source,
                parse_errors,
                recoveries,
            } => write!(
                f,
                "parse failed for {source}: errors={parse_errors}, recoveries={recoveries}"
            ),
        }
    }
}

impl std::error::Error for CompilerError {}

fn ensure_parse_success(source: &str, output: ParseOutput) -> Result<ParseOutput, CompilerError> {
    let parse_errors = usize::try_from(output.state.ctx.parse_error_count()).unwrap_or(usize::MAX);
    let recoveries = output.state.ctx.recovery_count();
    let has_root = output.root.is_some();
    if has_root && parse_errors == 0 && recoveries == 0 {
        Ok(output)
    } else {
        Err(CompilerError::Parse {
            source: source.into(),
            parse_errors,
            recoveries,
        })
    }
}

#[must_use]
pub fn golden_snapshot(source_name: &str, source: &str) -> String {
    let normalized_source = normalize_newlines(source);
    let line_count = normalized_source.lines().count();
    let byte_count = normalized_source.len();
    let hash = fnv1a64(normalized_source.as_bytes());

    format!(
        "faust-rs-golden-v1\nsource={source_name}\nbytes={byte_count}\nlines={line_count}\nfnv1a64={hash:016x}\n"
    )
}

pub fn golden_snapshot_from_file(path: &Path) -> Result<String, std::io::Error> {
    let source = std::fs::read_to_string(path)?;
    Ok(golden_snapshot(&path.display().to_string(), &source))
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0001_0000_01b3;

fn fnv1a64(input: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in input {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn normalize_newlines(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{Compiler, CompilerError, golden_snapshot};

    fn make_temp_root(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "faust_rs_compiler_{}_{}_{}",
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
    fn golden_snapshot_is_stable_for_lf_vs_crlf() {
        let lf = "process = _;\n";
        let crlf = "process = _;\r\n";
        assert_eq!(
            golden_snapshot("pass_through.dsp", lf),
            golden_snapshot("pass_through.dsp", crlf)
        );
    }

    #[test]
    fn compiler_compile_source_accepts_valid_dsp() {
        let compiler = Compiler::new();
        let out = compiler
            .compile_source("valid.dsp", "process = _;")
            .expect("valid source should parse");
        assert!(out.root.is_some());
        assert!(out.errors.is_empty());
    }

    #[test]
    fn compiler_compile_source_rejects_malformed_dsp() {
        let compiler = Compiler::new();
        let err = compiler
            .compile_source("invalid.dsp", "process = ;")
            .expect_err("malformed source should fail compile facade");
        assert!(matches!(err, CompilerError::Parse { .. }));
    }

    #[test]
    fn compiler_compile_file_parses_imported_fixture() {
        let root = make_temp_root("imports");
        let main = root.join("main.dsp");
        let lib = root.join("ops.lib");
        fs::write(&main, "import(\"ops.lib\");\nprocess = gain;\n")
            .expect("main should be written");
        fs::write(&lib, "gain = _;\n").expect("lib should be written");

        let compiler = Compiler::new();
        let out = compiler
            .compile_file(&main, std::slice::from_ref(&root))
            .expect("import fixture should parse");
        assert!(out.root.is_some());
        assert!(out.errors.is_empty());

        fs::remove_dir_all(root).expect("temp root should be removable");
    }

    #[test]
    fn compiler_compile_file_reports_missing_import() {
        let root = make_temp_root("missing_import");
        let main = root.join("main.dsp");
        fs::write(&main, "import(\"missing.lib\");\nprocess = _;\n")
            .expect("main should be written");

        let compiler = Compiler::new();
        let err = compiler
            .compile_file(&main, std::slice::from_ref(&root))
            .expect_err("missing import should fail");
        assert!(matches!(err, CompilerError::Import(_)));

        fs::remove_dir_all(root).expect("temp root should be removable");
    }
}
