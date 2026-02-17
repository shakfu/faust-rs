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

use boxes::{BoxId, BoxMatch, dump_box, match_box};
use errors::{Diagnostic, DiagnosticBundle, IntoDiagnostic, Label, LabelStyle, SourceSpan};
use parser::{ParseOutput, SourceReaderError};
use propagate::{BoxArity, PropagateError};
use signals::SigId;

/// Parse + eval + propagate output package.
#[derive(Debug)]
pub struct SignalCompileOutput {
    pub parse: ParseOutput,
    pub process_box: BoxId,
    pub process_arity: BoxArity,
    pub signals: Vec<SigId>,
}

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

    /// Parses one source file using its parent directory as default import search path.
    pub fn compile_file_default(&self, path: &Path) -> Result<ParseOutput, CompilerError> {
        let search_base = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        self.compile_file(path, std::slice::from_ref(&search_base))
    }

    /// Parses, evaluates `process`, then propagates boxes to output signals.
    pub fn compile_source_to_signals(
        &self,
        source_name: &str,
        source: &str,
    ) -> Result<SignalCompileOutput, CompilerError> {
        let output = self.compile_source(source_name, source)?;
        self.pipeline_to_signals(source_name, output)
    }

    /// Parses one file, evaluates `process`, then propagates boxes to output signals.
    pub fn compile_file_to_signals(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
    ) -> Result<SignalCompileOutput, CompilerError> {
        let output = self.compile_file(path, search_paths)?;
        self.pipeline_to_signals(&path.display().to_string(), output)
    }

    /// Parses one file with default import search path, then runs eval+propagate.
    pub fn compile_file_default_to_signals(
        &self,
        path: &Path,
    ) -> Result<SignalCompileOutput, CompilerError> {
        let search_base = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        self.compile_file_to_signals(path, std::slice::from_ref(&search_base))
    }

    fn pipeline_to_signals(
        &self,
        source: &str,
        mut output: ParseOutput,
    ) -> Result<SignalCompileOutput, CompilerError> {
        let root = output.root.ok_or_else(|| CompilerError::MissingRoot {
            source: source.into(),
        })?;

        let process_box = eval::eval_process(&mut output.state.arena, root).map_err(|error| {
            let mut diagnostic = error.clone().into_diagnostic();
            if let Some(node) = eval_error_node(&error) {
                diagnostic = diagnostic.with_note(format!("node_id={}", node.as_u32()));
                diagnostic = diagnostic.with_note(format!(
                    "box_expr={}",
                    compact_box_preview(&output.state.arena, node)
                ));
                diagnostic = maybe_add_source_label(
                    diagnostic,
                    &output.state.ctx,
                    &output.state.arena,
                    root,
                    node,
                );
            }
            let diagnostics = bundle_from_diagnostic(diagnostic);
            CompilerError::Eval {
                source: source.into(),
                error,
                diagnostics,
            }
        })?;
        let process_arity =
            propagate::box_arity(&output.state.arena, process_box).map_err(|error| {
                let mut diagnostic = error.clone().into_diagnostic();
                if let Some(node) = propagate_error_node(&error) {
                    diagnostic = diagnostic.with_note(format!("node_id={}", node.as_u32()));
                    diagnostic = diagnostic.with_note(format!(
                        "box_expr={}",
                        compact_box_preview(&output.state.arena, node)
                    ));
                    diagnostic = maybe_add_source_label(
                        diagnostic,
                        &output.state.ctx,
                        &output.state.arena,
                        root,
                        node,
                    );
                }
                let diagnostics = bundle_from_diagnostic(diagnostic);
                CompilerError::Propagate {
                    source: source.into(),
                    error,
                    diagnostics,
                }
            })?;
        let inputs = propagate::make_sig_input_list(&mut output.state.arena, process_arity.inputs);
        let signals = propagate::propagate(&mut output.state.arena, process_box, &inputs).map_err(
            |error| {
                let mut diagnostic = error.clone().into_diagnostic();
                if let Some(node) = propagate_error_node(&error) {
                    diagnostic = diagnostic.with_note(format!("node_id={}", node.as_u32()));
                    diagnostic = diagnostic.with_note(format!(
                        "box_expr={}",
                        compact_box_preview(&output.state.arena, node)
                    ));
                    diagnostic = maybe_add_source_label(
                        diagnostic,
                        &output.state.ctx,
                        &output.state.arena,
                        root,
                        node,
                    );
                }
                let diagnostics = bundle_from_diagnostic(diagnostic);
                CompilerError::Propagate {
                    source: source.into(),
                    error,
                    diagnostics,
                }
            },
        )?;

        Ok(SignalCompileOutput {
            parse: output,
            process_box,
            process_arity,
            signals,
        })
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
    MissingRoot {
        source: Box<str>,
    },
    Parse {
        source: Box<str>,
        parse_errors: usize,
        recoveries: u32,
        diagnostics: DiagnosticBundle,
    },
    Eval {
        source: Box<str>,
        error: eval::EvalError,
        diagnostics: DiagnosticBundle,
    },
    Propagate {
        source: Box<str>,
        error: PropagateError,
        diagnostics: DiagnosticBundle,
    },
}

impl std::fmt::Display for CompilerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Import(err) => write!(f, "{err}"),
            Self::MissingRoot { source } => write!(f, "parse returned no root for {source}"),
            Self::Parse {
                source,
                parse_errors,
                recoveries,
                diagnostics,
            } => write!(
                f,
                "parse failed for {source}: errors={parse_errors}, recoveries={recoveries}, diagnostics={}",
                diagnostics.len()
            ),
            Self::Eval { source, error, .. } => {
                write!(f, "evaluation failed for {source}: {error}")
            }
            Self::Propagate { source, error, .. } => {
                write!(f, "propagation failed for {source}: {error}")
            }
        }
    }
}

impl std::error::Error for CompilerError {}

impl CompilerError {
    /// Returns structured diagnostics when this error variant carries them.
    #[must_use]
    pub fn diagnostics(&self) -> Option<&DiagnosticBundle> {
        match self {
            Self::Parse { diagnostics, .. } => Some(diagnostics),
            Self::Eval { diagnostics, .. } => Some(diagnostics),
            Self::Propagate { diagnostics, .. } => Some(diagnostics),
            _ => None,
        }
    }
}

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
            diagnostics: output.diagnostics,
        })
    }
}

fn bundle_from_diagnostic(diagnostic: Diagnostic) -> DiagnosticBundle {
    let mut diagnostics = DiagnosticBundle::new();
    diagnostics.push(diagnostic);
    diagnostics
}

/// Returns the offending node id for eval errors that carry one.
fn eval_error_node(error: &eval::EvalError) -> Option<BoxId> {
    match error {
        eval::EvalError::MalformedDefinitionNode { node }
        | eval::EvalError::MalformedListNode { node }
        | eval::EvalError::MalformedCaseNode { node }
        | eval::EvalError::NonIdentifierParameter { node }
        | eval::EvalError::NonIdentifierIterationVariable { node }
        | eval::EvalError::IterationCountNotInt { node }
        | eval::EvalError::LoopDetected { node } => Some(*node),
        _ => None,
    }
}

/// Returns the offending node id for propagate errors that carry one.
fn propagate_error_node(error: &PropagateError) -> Option<BoxId> {
    match error {
        PropagateError::UnsupportedBox { node, .. }
        | PropagateError::InvalidIntegerValue { node, .. }
        | PropagateError::InputArityMismatch { node, .. }
        | PropagateError::OutputArityMismatch { node, .. }
        | PropagateError::SeqArityMismatch { node, .. }
        | PropagateError::SplitArityMismatch { node, .. }
        | PropagateError::MergeArityMismatch { node, .. }
        | PropagateError::RecArityMismatch { node, .. } => Some(*node),
        _ => None,
    }
}

/// Compacts one box subtree dump to a bounded single-line preview for diagnostics notes.
fn compact_box_preview(arena: &tlib::TreeArena, node: BoxId) -> String {
    let preview = dump_box(arena, node);
    let mut one_line = preview.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 180;
    if one_line.chars().count() > MAX_CHARS {
        one_line = one_line.chars().take(MAX_CHARS).collect::<String>() + "...";
    }
    one_line
}

/// Attaches a primary source label when parser metadata can be resolved for `node`.
fn maybe_add_source_label(
    diagnostic: Diagnostic,
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    node: BoxId,
) -> Diagnostic {
    let span = source_span_for_definition_of_expr(ctx, arena, defs_root, node)
        .or_else(|| source_span_from_node_or_descendant(ctx, arena, node))
        .or_else(|| source_span_for_process_binding_target(ctx, arena, defs_root))
        .or_else(|| source_span_for_process_definition(ctx, arena, defs_root));
    let Some(span) = span else {
        return diagnostic;
    };
    diagnostic.with_label(Label::new(LabelStyle::Primary, span, "related source"))
}

/// Resolves one source span from the node itself, then falls back to labeled descendants.
fn source_span_from_node_or_descendant(
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    node: BoxId,
) -> Option<SourceSpan> {
    if let Some(span) = source_span_for_node(ctx, node) {
        return Some(span);
    }

    let mut stack = vec![node];
    let mut visited = 0usize;
    while let Some(cur) = stack.pop() {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }

        if let Some(span) = source_span_for_node(ctx, cur) {
            return Some(span);
        }

        if let Some(children) = arena.children(cur) {
            for child in children.iter().rev() {
                stack.push(*child);
            }
        }
    }
    None
}

/// Resolves one source span for a node from parser `use_prop` / `def_prop`.
fn source_span_for_node(ctx: &parser::ParserCtx, node: BoxId) -> Option<SourceSpan> {
    let loc = ctx.use_prop(node).or_else(|| ctx.def_prop(node))?;
    Some(SourceSpan::new(
        loc.file(),
        loc.line(),
        loc.col(),
        loc.end_line(),
        loc.end_col(),
    ))
}

/// Resolves one source span for a definition node, preferring `def_prop`.
///
/// This is used for alias fallback (`process = foo;`) where we want the location
/// of the defining equation, not the use-site of `foo`.
fn source_span_for_definition_node(ctx: &parser::ParserCtx, node: BoxId) -> Option<SourceSpan> {
    let loc = ctx.def_prop(node).or_else(|| ctx.use_prop(node))?;
    Some(SourceSpan::new(
        loc.file(),
        loc.line(),
        loc.col(),
        loc.end_line(),
        loc.end_col(),
    ))
}

/// Fallback source span for the `process` definition identifier.
///
/// Used when the offending propagated/evaluated node cannot be mapped to a more
/// specific source location.
fn source_span_for_process_definition(
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
) -> Option<SourceSpan> {
    let mut defs = defs_root;
    let mut visited = 0usize;
    while !arena.is_nil(defs) {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }
        let def = arena.hd(defs)?;
        let name = arena.hd(def)?;
        if let BoxMatch::Ident("process") = match_box(arena, name) {
            return source_span_for_node(ctx, name);
        }
        defs = arena.tl(defs)?;
    }
    None
}

/// Fallback source span for direct process aliases (`process = <ident>;`).
///
/// When `process` is a direct identifier alias, this resolves the target definition
/// location (for example `foo = ...; process = foo;` -> label on `foo = ...`).
fn source_span_for_process_binding_target(
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
) -> Option<SourceSpan> {
    let (_process_name, process_expr) = find_definition_name_and_expr(arena, defs_root, "process")?;
    let BoxMatch::Ident(target_name) = match_box(arena, process_expr) else {
        return None;
    };
    let (target_def_name, _target_expr) =
        find_definition_name_and_expr(arena, defs_root, target_name)?;
    source_span_for_definition_node(ctx, target_def_name)
}

/// Finds one `(definition_name, definition_expr)` pair by identifier name
/// in the parser root definitions list.
fn find_definition_name_and_expr(
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    wanted: &str,
) -> Option<(BoxId, BoxId)> {
    let mut defs = defs_root;
    let mut visited = 0usize;
    while !arena.is_nil(defs) {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }
        let def = arena.hd(defs)?;
        let name = arena.hd(def)?;
        let args_expr = arena.tl(def)?;
        let expr = arena.tl(args_expr)?;
        if let BoxMatch::Ident(name_str) = match_box(arena, name) {
            if name_str == wanted {
                return Some((name, expr));
            }
        }
        defs = arena.tl(defs)?;
    }
    None
}

/// Fallback source span from a definition whose expression matches (or contains) `node`.
///
/// This covers alias chains such as:
/// `foo = <bad>; bar = foo; process = bar,bar;`
/// where the failing node belongs to `foo` but `process` is not a direct identifier alias.
fn source_span_for_definition_of_expr(
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    node: BoxId,
) -> Option<SourceSpan> {
    let mut defs = defs_root;
    let mut visited = 0usize;
    while !arena.is_nil(defs) {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }
        let def = arena.hd(defs)?;
        let name = arena.hd(def)?;
        let args_expr = arena.tl(def)?;
        let expr = arena.tl(args_expr)?;
        if expr == node || subtree_contains_node(arena, expr, node) {
            return source_span_for_definition_node(ctx, name);
        }
        defs = arena.tl(defs)?;
    }
    None
}

fn subtree_contains_node(arena: &tlib::TreeArena, root: BoxId, needle: BoxId) -> bool {
    if root == needle {
        return true;
    }
    let mut stack = vec![root];
    let mut visited = 0usize;
    while let Some(cur) = stack.pop() {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }
        if cur == needle {
            return true;
        }
        if let Some(children) = arena.children(cur) {
            for child in children.iter().rev() {
                stack.push(*child);
            }
        }
    }
    false
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

    use boxes::BoxBuilder;
    use signals::SigMatch;
    use tlib::TreeArena;

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
        match err {
            CompilerError::Parse {
                parse_errors,
                diagnostics,
                ..
            } => {
                assert!(parse_errors >= 1);
                assert!(!diagnostics.is_empty());
                assert!(
                    diagnostics
                        .as_slice()
                        .iter()
                        .any(|d| d.code.0.starts_with("FRS-PARSE-"))
                );
            }
            _ => panic!("expected parse error"),
        }
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

    #[test]
    fn compiler_compile_file_default_uses_parent_dir_for_imports() {
        let root = make_temp_root("default_search");
        let main = root.join("main.dsp");
        let lib = root.join("ops.lib");
        fs::write(&main, "import(\"ops.lib\");\nprocess = gain;\n")
            .expect("main should be written");
        fs::write(&lib, "gain = _;\n").expect("lib should be written");

        let compiler = Compiler::new();
        let out = compiler
            .compile_file_default(&main)
            .expect("default search path should parse local import");
        assert!(out.root.is_some());
        assert!(out.errors.is_empty());

        fs::remove_dir_all(root).expect("temp root should be removable");
    }

    #[test]
    fn compiler_compile_source_to_signals_pass_through() {
        let compiler = Compiler::new();
        let out = compiler
            .compile_source_to_signals("pass.dsp", "process = _;")
            .expect("pass-through should compile to signals");
        assert_eq!(out.process_arity.inputs, 1);
        assert_eq!(out.process_arity.outputs, 1);
        assert_eq!(out.signals.len(), 1);
        assert_eq!(
            signals::match_sig(&out.parse.state.arena, out.signals[0]),
            SigMatch::Input(0)
        );
    }

    #[test]
    fn compiler_compile_source_to_signals_recursive_case() {
        let compiler = Compiler::new();
        let out = compiler
            .compile_source_to_signals("rec.dsp", "process = + ~ _;")
            .expect("recursive process should compile to signals");
        assert_eq!(out.process_arity.inputs, 1);
        assert_eq!(out.process_arity.outputs, 1);
        assert_eq!(out.signals.len(), 1);
        assert!(matches!(
            signals::match_sig(&out.parse.state.arena, out.signals[0]),
            SigMatch::Proj(_, _)
        ));
    }

    #[test]
    fn compiler_compile_source_to_signals_reports_eval_error() {
        let compiler = Compiler::new();
        let err = compiler
            .compile_source_to_signals("missing_process.dsp", "foo = _;")
            .expect_err("missing process should fail evaluation");
        assert!(matches!(
            err,
            CompilerError::Eval {
                error: eval::EvalError::MissingProcessDefinition,
                ..
            }
        ));
        let diagnostics = err
            .diagnostics()
            .expect("eval failure should expose structured diagnostics");
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|d| d.code.0.starts_with("FRS-EVAL-"))
        );
    }

    #[test]
    fn compiler_compile_source_to_signals_reports_propagate_error() {
        let compiler = Compiler::new();
        let err = compiler
            .compile_source_to_signals("prop_mismatch.dsp", "process = _,_ <: _,_,_;")
            .expect_err("invalid split arity should fail propagation");
        assert!(matches!(err, CompilerError::Propagate { .. }));
        let diagnostics = err
            .diagnostics()
            .expect("propagate failure should expose structured diagnostics");
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|d| d.code.0.starts_with("FRS-PROP-"))
        );
        let first = diagnostics
            .as_slice()
            .first()
            .expect("propagate error bundle should not be empty");
        assert!(first.notes.iter().any(|n| n.starts_with("node_id=")));
        assert!(first.notes.iter().any(|n| n.starts_with("box_expr=")));
    }

    #[test]
    fn source_span_lookup_finds_direct_node_property() {
        let mut arena = TreeArena::new();
        let ident = BoxBuilder::new(&mut arena).ident("x");
        let mut ctx = parser::ParserCtx::new();
        ctx.set_use_prop(ident, "fixture.dsp", 7);

        let span = super::source_span_from_node_or_descendant(&ctx, &arena, ident)
            .expect("direct property should resolve to source span");
        assert_eq!(span.file.display().to_string(), "fixture.dsp");
        assert_eq!(span.line, 7);
        assert_eq!(span.col, 1);
    }

    #[test]
    fn source_span_lookup_finds_descendant_property() {
        let mut arena = TreeArena::new();
        let (parent, child) = {
            let mut bb = BoxBuilder::new(&mut arena);
            let wire = bb.wire();
            let ident = bb.ident("x");
            let seq = bb.seq(wire, ident);
            (seq, ident)
        };
        let mut ctx = parser::ParserCtx::new();
        ctx.set_use_prop(child, "desc.dsp", 19);

        let span = super::source_span_from_node_or_descendant(&ctx, &arena, parent)
            .expect("descendant property should resolve to source span");
        assert_eq!(span.file.display().to_string(), "desc.dsp");
        assert_eq!(span.line, 19);
    }

    #[test]
    fn process_definition_span_fallback_resolves_when_node_has_no_property() {
        let mut arena = TreeArena::new();
        let (defs, process_name, expr) = {
            let mut bb = BoxBuilder::new(&mut arena);
            let process_name = bb.ident("process");
            let wire = bb.wire();
            let cut = bb.cut();
            let expr = bb.seq(wire, cut);
            let nil = arena.nil();
            let args_expr = arena.cons(nil, expr);
            let def = arena.cons(process_name, args_expr);
            let defs = arena.cons(def, nil);
            (defs, process_name, expr)
        };

        let mut ctx = parser::ParserCtx::new();
        ctx.set_def_prop(process_name, "fallback.dsp", 11);

        let span = super::source_span_for_process_definition(&ctx, &arena, defs)
            .expect("process definition should provide fallback span");
        assert_eq!(span.file.display().to_string(), "fallback.dsp");
        assert_eq!(span.line, 11);

        let diag = errors::Diagnostic::new(
            errors::Severity::Error,
            errors::Stage::Propagate,
            errors::codes::PROP_ARITY_MISMATCH,
            "mismatch",
        );
        let labeled = super::maybe_add_source_label(diag, &ctx, &arena, defs, expr);
        assert!(!labeled.labels.is_empty());
        assert_eq!(
            labeled.labels[0].span.file.display().to_string(),
            "fallback.dsp"
        );
    }

    #[test]
    fn process_binding_target_span_preferred_over_process_line() {
        let mut arena = TreeArena::new();
        let (defs, process_name, foo_name, bad_node) = {
            let mut bb = BoxBuilder::new(&mut arena);
            let foo_name = bb.ident("foo");
            let wire_a = bb.wire();
            let wire_b = bb.wire();
            let left = bb.par(wire_a, wire_b);
            let wire_c = bb.wire();
            let wire_d = bb.wire();
            let wire_e = bb.wire();
            let right_tail = bb.par(wire_d, wire_e);
            let right = bb.par(wire_c, right_tail);
            let foo_expr = bb.split(left, right);

            let process_name = bb.ident("process");
            let process_expr = bb.ident("foo");

            let nil = arena.nil();
            let foo_args_expr = arena.cons(nil, foo_expr);
            let foo_def = arena.cons(foo_name, foo_args_expr);
            let process_args_expr = arena.cons(nil, process_expr);
            let process_def = arena.cons(process_name, process_args_expr);
            let tail_defs = arena.cons(process_def, nil);
            let defs = arena.cons(foo_def, tail_defs);

            (defs, process_name, foo_name, foo_expr)
        };

        let mut ctx = parser::ParserCtx::new();
        ctx.set_def_prop(foo_name, "foo_file.dsp", 1);
        ctx.set_def_prop(process_name, "foo_file.dsp", 4);

        let direct = super::source_span_for_process_binding_target(&ctx, &arena, defs)
            .expect("process binding target should resolve to foo definition");
        assert_eq!(direct.file.display().to_string(), "foo_file.dsp");
        assert_eq!(direct.line, 1);

        let diag = errors::Diagnostic::new(
            errors::Severity::Error,
            errors::Stage::Propagate,
            errors::codes::PROP_ARITY_MISMATCH,
            "mismatch",
        );
        let labeled = super::maybe_add_source_label(diag, &ctx, &arena, defs, bad_node);
        assert!(!labeled.labels.is_empty());
        assert_eq!(
            labeled.labels[0].span.file.display().to_string(),
            "foo_file.dsp"
        );
        assert_eq!(labeled.labels[0].span.line, 1);
    }

    #[test]
    fn definition_of_expr_fallback_handles_alias_chain() {
        let mut arena = TreeArena::new();
        let (defs, process_name, bar_name, foo_name, bad_node) = {
            let mut bb = BoxBuilder::new(&mut arena);
            let foo_name = bb.ident("foo");
            let wire_a = bb.wire();
            let wire_b = bb.wire();
            let left = bb.par(wire_a, wire_b);
            let wire_c = bb.wire();
            let wire_d = bb.wire();
            let wire_e = bb.wire();
            let right_tail = bb.par(wire_d, wire_e);
            let right = bb.par(wire_c, right_tail);
            let foo_expr = bb.split(left, right);

            let bar_name = bb.ident("bar");
            let bar_expr = bb.ident("foo");

            let process_name = bb.ident("process");
            let process_bar_l = bb.ident("bar");
            let process_bar_r = bb.ident("bar");
            let process_rhs = bb.par(process_bar_l, process_bar_r);

            let nil = arena.nil();
            let foo_args_expr = arena.cons(nil, foo_expr);
            let foo_def = arena.cons(foo_name, foo_args_expr);
            let bar_args_expr = arena.cons(nil, bar_expr);
            let bar_def = arena.cons(bar_name, bar_args_expr);
            let process_args_expr = arena.cons(nil, process_rhs);
            let process_def = arena.cons(process_name, process_args_expr);
            let defs_tail = arena.cons(process_def, nil);
            let defs_tail = arena.cons(bar_def, defs_tail);
            let defs = arena.cons(foo_def, defs_tail);

            (defs, process_name, bar_name, foo_name, foo_expr)
        };

        let mut ctx = parser::ParserCtx::new();
        ctx.set_def_prop(foo_name, "chain.dsp", 1);
        ctx.set_def_prop(bar_name, "chain.dsp", 2);
        ctx.set_def_prop(process_name, "chain.dsp", 3);

        let span = super::source_span_for_definition_of_expr(&ctx, &arena, defs, bad_node)
            .expect("definition-of-expression fallback should resolve to foo definition");
        assert_eq!(span.file.display().to_string(), "chain.dsp");
        assert_eq!(span.line, 1);
    }
}
