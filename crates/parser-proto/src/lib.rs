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
//! - Ports a first lexer subset from C++ `faustlexer.l` with token-priority tests.
//! - Builds a minimal compile-time lexer/parser pair to validate the `lrlex/lrpar` toolchain.
//! - Keeps production `crates/parser` untouched until Gate B decision.

use cfgrammar::Span;
use lrlex::lrlex_mod;
use lrlex::{DefaultLexerTypes, LRNonStreamingLexerDef};
use lrpar::lrpar_mod;
use lrpar::{LexError, Lexeme, Lexer, NonStreamingLexer};
use tlib::TreeId;

pub mod context;

pub use context::{DiagnosticSeverity, ParserCtx, ParserDiagnostic, SourceLocation};

lrlex_mod!("grammar/faustlexer.l");
lrpar_mod!("grammar/faustparser.y");

/// One lexed token with normalized name/text/location information.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexedToken {
    pub name: Box<str>,
    pub text: Box<str>,
    pub span: Span,
    pub start_line: u32,
    pub start_col: u32,
}

/// Returns the generated lexer definition for the Faust parser prototype.
#[must_use]
pub fn lexerdef() -> LRNonStreamingLexerDef<DefaultLexerTypes<u32>> {
    faustlexer_l::lexerdef()
}

/// Lexes `input` and returns named tokens with source locations.
pub fn lex_tokens(input: &str) -> Result<Vec<LexedToken>, String> {
    let lexerdef = lexerdef();
    let lexer = lexerdef.lexer(input);
    let mut out = Vec::new();
    for item in lexer.iter() {
        let lexeme = item.map_err(|err| format!("lex error at span {:?}", err.span()))?;
        let name = faustparser_y::token_epp(cfgrammar::TIdx(lexeme.tok_id())).unwrap_or("<anon>");
        let span = lexeme.span();
        let ((line, col), _) = lexer.line_col(span);
        out.push(LexedToken {
            name: name.to_owned().into_boxed_str(),
            text: lexer.span_str(span).into(),
            span,
            start_line: u32::try_from(line).unwrap_or(u32::MAX),
            start_col: u32::try_from(col).unwrap_or(u32::MAX),
        });
    }
    Ok(out)
}

/// Parses the minimal prototype sentence `process = _;`.
///
/// This validates that `build.rs` generated lexer/parser artifacts are usable at runtime.
#[must_use]
pub fn parse_minimal(input: &str) -> bool {
    let lexerdef = lexerdef();
    let lexer = lexerdef.lexer(input);
    let (result, errors) = faustparser_y::parse(&lexer);
    result.is_some() && errors.is_empty()
}

/// Updates parser cursor from one lexed token, then tags `sym` as use-site at that location.
pub fn set_use_prop_from_token(ctx: &mut ParserCtx, sym: TreeId, file: &str, token: &LexedToken) {
    ctx.set_cursor(file, token.start_line);
    ctx.set_use_prop_at_cursor(sym);
}
