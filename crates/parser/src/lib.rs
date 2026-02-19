//! Production parser crate entry point.
//!
//! # Source provenance (C++)
//! - `compiler/parser/faustparser.y`
//! - `compiler/parser/faustlexer.l`
//! - `compiler/parser/sourcereader.hh/.cpp`
//!
//! # Current integration status
//! - This crate exposes the parser APIs consumed by upper crates.
//! - The implementation is delegated to `parser-proto` while Gate B migration
//!   is being integrated into production boundaries.
//!
//! # Role in pipeline
//! - Production parser facade used by `compiler` and CLI flows.
//! - Owns stable entry points for:
//!   - in-memory source parsing,
//!   - file parsing with import expansion,
//!   - tokenization helpers used by parser tests/tooling.
//!
//! # API mapping status
//! - Public parsing functions are `adapted`: behavior follows Faust parser
//!   semantics while exposing Rust-native `Result`/typed error surfaces.
//!
//! # Stability note
//! - Even while internals evolve (`parser-proto` slices), this crate is the
//!   compatibility boundary intended for upper-layer consumption.

use std::path::{Path, PathBuf};

pub use parser_proto::{
    DiagnosticSeverity, LexedToken, ParseOutput, ParserCtx, ParserDiagnostic, SourceLocation,
    SourceReader, SourceReaderError, lex_tokens,
};

/// Parses one Faust source string.
#[must_use]
pub fn parse_program(input: &str, source_file: &str) -> ParseOutput {
    parser_proto::parse_program(input, source_file)
}

/// Parses one source file after recursive local import expansion.
pub fn parse_file_with_imports(
    path: &Path,
    search_paths: &[PathBuf],
) -> Result<ParseOutput, SourceReaderError> {
    parser_proto::parse_file_with_imports(path, search_paths)
}

/// Minimal parser smoke helper (`process = _;` shape).
#[must_use]
pub fn parse_minimal(input: &str) -> bool {
    parser_proto::parse_minimal(input)
}
