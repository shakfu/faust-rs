//! Parser migration prototype crate (`lrpar`/`lrlex`) kept isolated from `crates/parser`.
//!
//! # Source provenance (C++)
//! - `compiler/parser/faustparser.y`
//! - `compiler/parser/faustlexer.l`
//! - `compiler/errors/errormsg.hh` / `compiler/errors/errormsg.cpp` (`setDefProp`/`setUseProp`)
//! - `compiler/global.hh` (`gWaveForm`, `gResult`)
//!
//! # Scope in this step
//! - Provides `ParserCtx` for parser-local state and property hooks.
//! - Builds a minimal compile-time lexer/parser pair to validate the `lrlex/lrpar` toolchain.
//! - Keeps production `crates/parser` untouched until Gate B decision.

use lrlex::lrlex_mod;
use lrpar::lrpar_mod;

pub mod context;

pub use context::{DiagnosticSeverity, ParserCtx, ParserDiagnostic, SourceLocation};

lrlex_mod!("grammar/faustlexer.l");
lrpar_mod!("grammar/faustparser.y");

/// Parses the minimal prototype sentence `process = _;`.
///
/// This validates that `build.rs` generated lexer/parser artifacts are usable at runtime.
#[must_use]
pub fn parse_minimal(input: &str) -> bool {
    let lexerdef = faustlexer_l::lexerdef();
    let lexer = lexerdef.lexer(input);
    let (result, errors) = faustparser_y::parse(&lexer);
    result.is_some() && errors.is_empty()
}
