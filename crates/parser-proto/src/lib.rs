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
//! - Implements parser slices 1..3 with real semantic actions (`program/definition`, infix core,
//!   UI/iterative subset used by prototype corpus).
//! - Routes expression constructors through `boxes` over `tlib::TreeArena` (no parser-local stubs).
//! - Keeps production `crates/parser` untouched until Gate B decision.

use cfgrammar::Span;
use lrlex::lrlex_mod;
use lrlex::{DefaultLexerTypes, LRNonStreamingLexerDef};
use lrpar::lrpar_mod;
use lrpar::{LexError, Lexeme, Lexer, NonStreamingLexer};
use std::cell::RefCell;
use tlib::{NodeKind, TreeArena, TreeId};

pub mod context;

pub use context::{DiagnosticSeverity, ParserCtx, ParserDiagnostic, SourceLocation};

/// Primitive operator subset used by parser Slice 2.
#[derive(Clone, Copy, Debug)]
pub enum PrimitiveOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    Xor,
    Lsh,
    Rsh,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    Pow,
    Delay,
    Delay1,
}

/// Parser state shared with grammar actions via `%parse-param`.
#[derive(Debug)]
pub struct ParseState {
    pub arena: TreeArena,
    pub ctx: ParserCtx,
    source_file: Box<str>,
}

impl ParseState {
    /// Creates parser state bound to one source file name/path.
    #[must_use]
    pub fn new(source_file: &str) -> Self {
        Self {
            arena: TreeArena::new(),
            ctx: ParserCtx::new(),
            source_file: source_file.into(),
        }
    }

    /// Equivalent to parser-level `nil` list root in C++ actions.
    #[must_use]
    pub fn nil(&mut self) -> TreeId {
        self.arena.nil()
    }

    /// Equivalent to C++ `cons(head, tail)` in parser actions.
    #[must_use]
    pub fn cons(&mut self, head: TreeId, tail: TreeId) -> TreeId {
        self.arena.cons(head, tail)
    }

    /// Prototype equivalent to C++ `formatDefinitions`.
    ///
    /// In Slice 1 we keep definition order as parser-built cons-list for deterministic checks.
    #[must_use]
    pub fn format_definitions(&mut self, defs: TreeId) -> TreeId {
        defs
    }

    /// Prepends non-`nil` statement in parser list order.
    #[must_use]
    pub fn prepend_statement(&mut self, list: TreeId, stmt: TreeId) -> TreeId {
        if self.arena.is_nil(stmt) {
            list
        } else {
            self.arena.cons(stmt, list)
        }
    }

    /// Builds one definition node shape compatible with C++ parser (`cons(name, cons(args, expr))`).
    #[must_use]
    pub fn make_definition(&mut self, name: TreeId, args: TreeId, expr: TreeId) -> TreeId {
        let pair = self.arena.cons(args, expr);
        self.arena.cons(name, pair)
    }

    /// Marks one recovered statement and returns `nil` placeholder.
    #[must_use]
    pub fn recovery_statement(&mut self, message: &str) -> TreeId {
        self.ctx.note_recovery();
        self.ctx.error(message);
        self.arena.nil()
    }

    /// Sets definition property at current cursor position.
    pub fn mark_def_at_cursor(&mut self, sym: TreeId) {
        self.ctx.set_def_prop_at_cursor(sym);
    }

    /// Builds `boxIdent` from a token and optionally marks use property.
    #[must_use]
    pub fn ident_from_token<'lexer, 'input: 'lexer>(
        &mut self,
        lexer: &'lexer dyn NonStreamingLexer<'input, DefaultLexerTypes<u32>>,
        tok: Result<lrlex::DefaultLexeme<u32>, lrlex::DefaultLexeme<u32>>,
        mark_use: bool,
    ) -> TreeId {
        let span = token_span(&tok);
        self.update_cursor_from_span(lexer, span);
        let ident = boxes::box_ident(&mut self.arena, lexer.span_str(span));
        if mark_use {
            self.ctx.set_use_prop_at_cursor(ident);
        }
        ident
    }

    /// Parses one integer literal token to `boxInt`.
    #[must_use]
    pub fn int_from_token<'lexer, 'input: 'lexer>(
        &mut self,
        lexer: &'lexer dyn NonStreamingLexer<'input, DefaultLexerTypes<u32>>,
        tok: Result<lrlex::DefaultLexeme<u32>, lrlex::DefaultLexeme<u32>>,
    ) -> TreeId {
        let span = token_span(&tok);
        self.update_cursor_from_span(lexer, span);
        let raw = lexer.span_str(span);
        match raw.parse::<i64>() {
            Ok(value) => boxes::box_int(&mut self.arena, value),
            Err(_) => {
                self.ctx.error("invalid INT literal");
                boxes::box_int(&mut self.arena, 0)
            }
        }
    }

    /// Parses one float literal token to `boxReal`.
    #[must_use]
    pub fn float_from_token<'lexer, 'input: 'lexer>(
        &mut self,
        lexer: &'lexer dyn NonStreamingLexer<'input, DefaultLexerTypes<u32>>,
        tok: Result<lrlex::DefaultLexeme<u32>, lrlex::DefaultLexeme<u32>>,
    ) -> TreeId {
        let span = token_span(&tok);
        self.update_cursor_from_span(lexer, span);
        let raw = lexer.span_str(span);
        let normalized = raw.strip_suffix('f').unwrap_or(raw);
        match normalized.parse::<f64>() {
            Ok(value) => boxes::box_real(&mut self.arena, value),
            Err(_) => {
                self.ctx.error("invalid FLOAT literal");
                boxes::box_real(&mut self.arena, 0.0)
            }
        }
    }

    /// Parses one quoted string token and removes outer quotes.
    #[must_use]
    pub fn uqstring_from_token<'lexer, 'input: 'lexer>(
        &mut self,
        lexer: &'lexer dyn NonStreamingLexer<'input, DefaultLexerTypes<u32>>,
        tok: Result<lrlex::DefaultLexeme<u32>, lrlex::DefaultLexeme<u32>>,
    ) -> TreeId {
        let span = token_span(&tok);
        self.update_cursor_from_span(lexer, span);
        let raw = lexer.span_str(span);
        let stripped = raw
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(raw);
        self.arena.string_lit(stripped)
    }

    fn string_node_text(&self, node: TreeId) -> Option<&str> {
        match self.arena.kind(node) {
            Some(NodeKind::StringLiteral(value)) => Some(value.as_ref()),
            Some(NodeKind::Symbol(value)) => Some(value.as_ref()),
            _ => None,
        }
    }

    /// Records one import statement and returns `nil` statement placeholder.
    #[must_use]
    pub fn import_statement(&mut self, path_node: TreeId) -> TreeId {
        match self.string_node_text(path_node).map(str::to_owned) {
            Some(path) => self.ctx.note_import(&path),
            None => self.ctx.error("invalid import path literal"),
        }
        self.nil()
    }

    /// Records one `declare key value;` statement and returns `nil`.
    #[must_use]
    pub fn declare_metadata_from_token<'lexer, 'input: 'lexer>(
        &mut self,
        lexer: &'lexer dyn NonStreamingLexer<'input, DefaultLexerTypes<u32>>,
        key_tok: Result<lrlex::DefaultLexeme<u32>, lrlex::DefaultLexeme<u32>>,
        value_node: TreeId,
    ) -> TreeId {
        let key_span = token_span(&key_tok);
        self.update_cursor_from_span(lexer, key_span);
        let key = lexer.span_str(key_span);
        match self.string_node_text(value_node).map(str::to_owned) {
            Some(value) => self.ctx.note_declared_metadata(key, &value),
            None => self.ctx.error("invalid declare metadata value"),
        }
        self.nil()
    }

    /// Records one `declare def key value;` statement and returns `nil`.
    #[must_use]
    pub fn declare_definition_metadata_from_tokens<'lexer, 'input: 'lexer>(
        &mut self,
        lexer: &'lexer dyn NonStreamingLexer<'input, DefaultLexerTypes<u32>>,
        def_tok: Result<lrlex::DefaultLexeme<u32>, lrlex::DefaultLexeme<u32>>,
        key_tok: Result<lrlex::DefaultLexeme<u32>, lrlex::DefaultLexeme<u32>>,
        value_node: TreeId,
    ) -> TreeId {
        let def_span = token_span(&def_tok);
        self.update_cursor_from_span(lexer, def_span);
        let def = lexer.span_str(def_span);
        let key = lexer.span_str(token_span(&key_tok));
        match self.string_node_text(value_node).map(str::to_owned) {
            Some(value) => self.ctx.note_declared_definition_metadata(def, key, &value),
            None => self.ctx.error("invalid declare definition metadata value"),
        }
        self.nil()
    }

    /// Parses one signed integer literal token to `boxInt`.
    #[must_use]
    pub fn signed_int_from_token<'lexer, 'input: 'lexer>(
        &mut self,
        lexer: &'lexer dyn NonStreamingLexer<'input, DefaultLexerTypes<u32>>,
        tok: Result<lrlex::DefaultLexeme<u32>, lrlex::DefaultLexeme<u32>>,
        sign: i64,
    ) -> TreeId {
        let span = token_span(&tok);
        self.update_cursor_from_span(lexer, span);
        let raw = lexer.span_str(span);
        match raw.parse::<i64>() {
            Ok(value) => boxes::box_int(&mut self.arena, value.saturating_mul(sign)),
            Err(_) => {
                self.ctx.error("invalid signed INT literal");
                boxes::box_int(&mut self.arena, 0)
            }
        }
    }

    /// Parses one signed float literal token to `boxReal`.
    #[must_use]
    pub fn signed_float_from_token<'lexer, 'input: 'lexer>(
        &mut self,
        lexer: &'lexer dyn NonStreamingLexer<'input, DefaultLexerTypes<u32>>,
        tok: Result<lrlex::DefaultLexeme<u32>, lrlex::DefaultLexeme<u32>>,
        sign: f64,
    ) -> TreeId {
        let span = token_span(&tok);
        self.update_cursor_from_span(lexer, span);
        let raw = lexer.span_str(span);
        let normalized = raw.strip_suffix('f').unwrap_or(raw);
        match normalized.parse::<f64>() {
            Ok(value) => boxes::box_real(&mut self.arena, value * sign),
            Err(_) => {
                self.ctx.error("invalid signed FLOAT literal");
                boxes::box_real(&mut self.arena, 0.0)
            }
        }
    }

    /// Encodes C++ infix primitive lowering: `a op b` -> `boxSeq(boxPar(a,b), boxOp())`.
    #[must_use]
    pub fn binary_prim(&mut self, left: TreeId, right: TreeId, op: PrimitiveOp) -> TreeId {
        let pair = boxes::box_par(&mut self.arena, left, right);
        let prim = self.prim_box(op);
        boxes::box_seq(&mut self.arena, pair, prim)
    }

    /// Encodes postfix primitive lowering: `a op` -> `boxSeq(a, boxOp())`.
    #[must_use]
    pub fn postfix_prim(&mut self, expr: TreeId, op: PrimitiveOp) -> TreeId {
        let prim = self.prim_box(op);
        boxes::box_seq(&mut self.arena, expr, prim)
    }

    /// Equivalent to C++ `buildBoxAppl` prototype behavior (`boxAppl(fun, revarglist)`).
    #[must_use]
    pub fn apply_box(&mut self, fun: TreeId, rev_arg_list: TreeId) -> TreeId {
        boxes::box_appl(&mut self.arena, fun, rev_arg_list)
    }

    /// Equivalent to C++ `boxAccess`.
    #[must_use]
    pub fn access_box(&mut self, expr: TreeId, ident: TreeId) -> TreeId {
        boxes::box_access(&mut self.arena, expr, ident)
    }

    fn prim_box(&mut self, op: PrimitiveOp) -> TreeId {
        match op {
            PrimitiveOp::Add => boxes::box_add(&mut self.arena),
            PrimitiveOp::Sub => boxes::box_sub(&mut self.arena),
            PrimitiveOp::Mul => boxes::box_mul(&mut self.arena),
            PrimitiveOp::Div => boxes::box_div(&mut self.arena),
            PrimitiveOp::Rem => boxes::box_rem(&mut self.arena),
            PrimitiveOp::And => boxes::box_and(&mut self.arena),
            PrimitiveOp::Or => boxes::box_or(&mut self.arena),
            PrimitiveOp::Xor => boxes::box_xor(&mut self.arena),
            PrimitiveOp::Lsh => boxes::box_lsh(&mut self.arena),
            PrimitiveOp::Rsh => boxes::box_rsh(&mut self.arena),
            PrimitiveOp::Lt => boxes::box_lt(&mut self.arena),
            PrimitiveOp::Le => boxes::box_le(&mut self.arena),
            PrimitiveOp::Gt => boxes::box_gt(&mut self.arena),
            PrimitiveOp::Ge => boxes::box_ge(&mut self.arena),
            PrimitiveOp::Eq => boxes::box_eq(&mut self.arena),
            PrimitiveOp::Ne => boxes::box_ne(&mut self.arena),
            PrimitiveOp::Pow => boxes::box_pow(&mut self.arena),
            PrimitiveOp::Delay => boxes::box_delay(&mut self.arena),
            PrimitiveOp::Delay1 => boxes::box_delay1(&mut self.arena),
        }
    }

    fn update_cursor_from_span<'lexer, 'input: 'lexer>(
        &mut self,
        lexer: &'lexer dyn NonStreamingLexer<'input, DefaultLexerTypes<u32>>,
        span: Span,
    ) {
        let ((line, _), _) = lexer.line_col(span);
        let line = u32::try_from(line).unwrap_or(u32::MAX);
        self.ctx.set_cursor(&self.source_file, line);
    }
}

fn token_span(tok: &Result<lrlex::DefaultLexeme<u32>, lrlex::DefaultLexeme<u32>>) -> Span {
    match tok {
        Ok(lexeme) | Err(lexeme) => lexeme.span(),
    }
}

/// Executes one mutable operation against parser state passed through `%parse-param`.
pub fn with_state<T>(state: &RefCell<ParseState>, f: impl FnOnce(&mut ParseState) -> T) -> T {
    let mut state = state.borrow_mut();
    f(&mut state)
}

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

/// Parse output containing parser state for structural checks.
#[derive(Debug)]
pub struct ParseOutput {
    pub root: Option<TreeId>,
    pub errors: Vec<String>,
    pub state: ParseState,
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

/// Parses one input with Slice-1 grammar and returns parser state.
#[must_use]
pub fn parse_program(input: &str, source_file: &str) -> ParseOutput {
    let lexerdef = lexerdef();
    let lexer = lexerdef.lexer(input);
    let state = RefCell::new(ParseState::new(source_file));
    let (root, errors) = faustparser_y::parse(&lexer, &state);
    let mut state = state.into_inner();

    let mut rendered_errors = Vec::with_capacity(errors.len());
    for err in errors {
        let message = err.pp(&lexer, &faustparser_y::token_epp).to_string();
        state.ctx.error(&message);
        rendered_errors.push(message);
    }

    ParseOutput {
        root,
        errors: rendered_errors,
        state,
    }
}

/// Parses the minimal prototype sentence `process = _;`.
#[must_use]
pub fn parse_minimal(input: &str) -> bool {
    let output = parse_program(input, "<memory>");
    output.root.is_some() && output.errors.is_empty()
}

/// Updates parser cursor from one lexed token, then tags `sym` as use-site at that location.
pub fn set_use_prop_from_token(ctx: &mut ParserCtx, sym: TreeId, file: &str, token: &LexedToken) {
    ctx.set_cursor(file, token.start_line);
    ctx.set_use_prop_at_cursor(sym);
}
