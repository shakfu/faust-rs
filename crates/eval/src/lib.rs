//! Box evaluator (Phase 4, section 2.2).
//!
//! # Source provenance (C++)
//! - `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.hh`
//! - `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.cpp`
//! - `/Users/letz/Developpements/RUST/faust/compiler/evaluate/environment.hh`
//! - `/Users/letz/Developpements/RUST/faust/compiler/evaluate/loopDetector.hh`
//!
//! # Scope of this tranche
//! - Name resolution against definition environments.
//! - Lexical scoping for `with {}` and function abstractions.
//! - Loop detection for recursive symbol expansion.
//! - Structural recursive evaluation over box trees.
//! - Function application and iterative form expansion (`ipar/iseq/isum/iprod`).
//! - Non-closure partial-application parity (`applyList`) with implicit wire insertion.
//!
//! # Execution model
//! 1. Build an environment from parser definitions (`name -> expr`).
//! 2. Resolve `process`.
//! 3. Evaluate recursively by box family:
//!    - lexical forms (`abstr`, `with`, `letrec`, `access`),
//!    - application (`appl` / `case`),
//!    - iterative forms (`ipar`, `iseq`, `isum`, `iprod`),
//!    - fallback structural map for nodes without eval-time reduction.
//!
//! The evaluator returns a normalized box tree meant to be consumed by later passes
//! (`propagate`, typing, transforms). It does not emit signals directly.

use std::fmt::{Display, Formatter};

use boxes::{BoxBuilder, BoxMatch, match_box};
use errors::codes;
use errors::{Diagnostic, IntoDiagnostic, Severity, Stage};
use tlib::{NodeKind, TreeArena, TreeId};

pub const CRATE_NAME: &str = "eval";

/// Symbol identifier used in evaluator environments.
pub type SymId = Box<str>;

/// Evaluation environment (name -> tree binding).
#[derive(Clone, Debug, Default)]
pub struct Environment {
    bindings: Vec<(SymId, TreeId)>,
    parent: Option<Box<Environment>>,
}

impl Environment {
    /// Creates an empty environment.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Binds one symbol in the current scope.
    pub fn bind(&mut self, name: impl Into<SymId>, value: TreeId) {
        self.bindings.push((name.into(), value));
    }

    /// Looks up one symbol in the current scope chain.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<TreeId> {
        for (sym, value) in self.bindings.iter().rev() {
            if sym.as_ref() == name {
                return Some(*value);
            }
        }
        self.parent.as_ref().and_then(|p| p.lookup(name))
    }

    /// Pushes one child scope.
    #[must_use]
    pub fn push_scope(&self) -> Self {
        Self {
            bindings: Vec::new(),
            parent: Some(Box::new(self.clone())),
        }
    }

    /// Returns names bound in the current lexical scope only.
    #[must_use]
    pub fn local_names(&self) -> Vec<String> {
        let mut out = self
            .bindings
            .iter()
            .map(|(sym, _)| sym.to_string())
            .collect::<Vec<_>>();
        out.sort();
        out.dedup();
        out
    }

    /// Returns names visible from current scope across lexical parents.
    #[must_use]
    pub fn visible_names(&self) -> Vec<String> {
        let mut out = self.local_names();
        if let Some(parent) = &self.parent {
            out.extend(parent.visible_names());
        }
        out.sort();
        out.dedup();
        out
    }

    /// Returns names from the root (top-level) scope.
    #[must_use]
    pub fn top_level_names(&self) -> Vec<String> {
        match &self.parent {
            Some(parent) => parent.top_level_names(),
            None => self.local_names(),
        }
    }
}

/// Infinite loop detector for recursive expansion.
#[derive(Clone, Debug)]
pub struct LoopDetector {
    call_stack: Vec<TreeId>,
    max_depth: usize,
}

impl LoopDetector {
    /// Creates a detector with default max depth.
    #[must_use]
    pub fn new() -> Self {
        Self {
            call_stack: Vec::new(),
            max_depth: 1024,
        }
    }

    /// Creates a detector with explicit max depth.
    #[must_use]
    pub fn with_max_depth(max_depth: usize) -> Self {
        Self {
            call_stack: Vec::new(),
            max_depth,
        }
    }

    fn enter(&mut self, id: TreeId) -> Result<(), EvalError> {
        if self.call_stack.contains(&id) {
            return Err(EvalError::LoopDetected { node: id });
        }
        if self.call_stack.len() >= self.max_depth {
            return Err(EvalError::RecursionDepthExceeded {
                max_depth: self.max_depth,
            });
        }
        self.call_stack.push(id);
        Ok(())
    }

    fn leave(&mut self) {
        let _ = self.call_stack.pop();
    }
}

impl Default for LoopDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Evaluator error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    MissingProcessDefinition {
        /// Parser root definitions list used for fallback source-label resolution.
        definitions: TreeId,
        /// Deterministic list of top-level definition names available in this program.
        available_defs: Vec<String>,
    },
    UndefinedSymbol {
        symbol: String,
        /// Identifier node where resolution failed.
        node: TreeId,
        /// Names bound in the immediate lexical scope.
        local_scope: Vec<String>,
        /// Names visible across lexical parents.
        visible_scope: Vec<String>,
        /// Names bound at top-level.
        top_level_scope: Vec<String>,
    },
    MalformedDefinitionNode {
        node: TreeId,
    },
    MalformedListNode {
        node: TreeId,
    },
    MalformedCaseNode {
        node: TreeId,
    },
    EmptyArgumentList {
        /// Argument-list node that was expected to contain at least one item.
        node: TreeId,
    },
    NonIdentifierParameter {
        node: TreeId,
    },
    NonIdentifierIterationVariable {
        node: TreeId,
    },
    IterationCountNotInt {
        node: TreeId,
    },
    IterationCountTooLarge {
        value: i64,
    },
    NegativeIterationCount {
        value: i64,
    },
    PatternArityMismatch {
        /// Case-rules root node used to evaluate matching.
        node: TreeId,
        expected: usize,
        got: usize,
    },
    PatternMatchFailed {
        /// Case-rules root node where no rule matched provided arguments.
        node: TreeId,
    },
    /// Non-closure application received more arguments than the function input arity.
    TooManyArguments {
        /// Function-like node receiving too many arguments.
        node: TreeId,
        expected: usize,
        got: usize,
    },
    LoopDetected {
        node: TreeId,
    },
    RecursionDepthExceeded {
        max_depth: usize,
    },
}

impl Display for EvalError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingProcessDefinition { .. } => write!(f, "missing `process` definition"),
            Self::UndefinedSymbol { symbol, .. } => write!(f, "undefined symbol `{symbol}`"),
            Self::MalformedDefinitionNode { node } => {
                write!(f, "malformed definition node {}", node.as_u32())
            }
            Self::MalformedListNode { node } => {
                write!(f, "malformed list node {}", node.as_u32())
            }
            Self::MalformedCaseNode { node } => {
                write!(f, "malformed case node {}", node.as_u32())
            }
            Self::EmptyArgumentList { .. } => write!(f, "empty argument list"),
            Self::NonIdentifierParameter { node } => {
                write!(
                    f,
                    "abstraction parameter is not an identifier: {}",
                    node.as_u32()
                )
            }
            Self::NonIdentifierIterationVariable { node } => {
                write!(
                    f,
                    "iteration variable is not an identifier: {}",
                    node.as_u32()
                )
            }
            Self::IterationCountNotInt { node } => {
                write!(f, "iteration count is not an int node: {}", node.as_u32())
            }
            Self::IterationCountTooLarge { value } => {
                write!(f, "iteration count too large for this target: {value}")
            }
            Self::NegativeIterationCount { value } => {
                write!(f, "iteration count is negative: {value}")
            }
            Self::PatternArityMismatch { expected, got, .. } => {
                write!(f, "pattern arity mismatch: expected {expected}, got {got}")
            }
            Self::PatternMatchFailed { .. } => write!(f, "no case rule matches arguments"),
            Self::TooManyArguments { expected, got, .. } => {
                write!(
                    f,
                    "too many arguments: expected at most {expected}, got {got}"
                )
            }
            Self::LoopDetected { node } => {
                write!(f, "recursive evaluation loop on node {}", node.as_u32())
            }
            Self::RecursionDepthExceeded { max_depth } => {
                write!(f, "evaluation recursion depth exceeded ({max_depth})")
            }
        }
    }
}

impl std::error::Error for EvalError {}

/// Converts one evaluator error into the workspace diagnostics model.
///
/// This keeps `EvalError` as the local phase error type while exposing
/// stable stage/code metadata for compiler-level aggregation and CLI rendering.
impl IntoDiagnostic for EvalError {
    fn into_diagnostic(self) -> Diagnostic {
        let message = self.to_string();
        match self {
            Self::MissingProcessDefinition { available_defs, .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_MISSING_PROCESS,
                message,
            )
            .with_note("cause: required top-level `process` definition is missing")
            .with_note("entrypoint contract: one top-level `process = ...;` definition is required")
            .with_note(format!(
                "available top-level definitions: {}",
                if available_defs.is_empty() {
                    "<none>".to_owned()
                } else {
                    available_defs.join(", ")
                }
            ))
            .with_help("define `process = ...;` in the top-level definitions")
            .with_help("template: process = _;"),
            Self::UndefinedSymbol {
                symbol,
                local_scope,
                visible_scope,
                top_level_scope,
                ..
            } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_UNDEFINED_SYMBOL,
                message,
            )
            .with_note("cause: unresolved identifier in current lexical scope")
            .with_note("rule: referenced identifier must be present in visible lexical scope")
            .with_note(format!(
                "computed: `{symbol}` is not present in current visible scope"
            ))
            .with_note(format!(
                "scope.local={}",
                if local_scope.is_empty() {
                    "<none>".to_owned()
                } else {
                    local_scope.join(", ")
                }
            ))
            .with_note(format!(
                "scope.visible={}",
                if visible_scope.is_empty() {
                    "<none>".to_owned()
                } else {
                    visible_scope.join(", ")
                }
            ))
            .with_note(format!(
                "scope.top_level={}",
                if top_level_scope.is_empty() {
                    "<none>".to_owned()
                } else {
                    top_level_scope.join(", ")
                }
            ))
            .with_help("define the symbol in scope or fix the identifier name")
            .with_help(format!("template: {symbol} = ...; // define before use"))
            .with_help("for top-level aliases: define target before first use"),
            Self::PatternArityMismatch { expected, got, .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_ARITY_MISMATCH,
                message,
            )
            .with_note("cause: case pattern arity does not match provided argument tuple")
            .with_note("rule: case rule arity must match provided argument tuple arity")
            .with_note(format!(
                "computed: expected={expected}, provided={got}, delta={}",
                got as i128 - expected as i128
            ))
            .with_note(format!(
                "suggested target: call case function with exactly {expected} argument(s)"
            ))
            .with_help("adapt the case pattern arity or provide the expected number of arguments")
            .with_help("template: case { (x, y) => ...; }; // 2-argument rule"),
            Self::TooManyArguments { expected, got, .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_ARITY_MISMATCH,
                message,
            )
            .with_note("cause: function application provides more arguments than accepted")
            .with_note(
                "rule: non-closure application requires provided arguments <= function input arity",
            )
            .with_note(format!(
                "computed: provided={got}, expected_max={expected}, overflow={}",
                got.saturating_sub(expected)
            ))
            .with_note(format!(
                "suggested target: remove {} extra argument(s)",
                got.saturating_sub(expected)
            ))
            .with_help("remove extra arguments or expand the function input arity")
            .with_help("template: f(a, b); // keep provided args <= function input arity"),
            Self::PatternMatchFailed { .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_GENERIC_FAILURE,
                message,
            )
            .with_note("cause: no case rule matched the provided argument tuple")
            .with_note("rule: at least one case pattern must match the provided argument tuple")
            .with_note("computed: provided tuple did not match any declared case pattern")
            .with_help("add a matching case rule or add a catch-all pattern"),
            Self::IterationCountNotInt { .. }
            | Self::IterationCountTooLarge { .. }
            | Self::NegativeIterationCount { .. } => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_ITERATION_INVALID,
                message,
            )
            .with_note("cause: iterative combinator count is not a valid non-negative integer")
            .with_note(
                "rule: iterator count must be integer, non-negative, and within supported range",
            )
            .with_help("iteration count must be a non-negative integer in target range"),
            _ => Diagnostic::new(
                Severity::Error,
                Stage::Eval,
                codes::EVAL_GENERIC_FAILURE,
                message,
            )
            .with_note("cause: evaluator reached an unsupported or malformed intermediate form"),
        }
    }
}

/// Evaluates one Faust program root list and returns the resolved `process` expression.
///
/// `definitions` is expected to be the parser root list where each item is:
/// `cons(name, cons(args, expr))`.
///
/// The returned tree is still a box IR node. It can contain high-level forms
/// when they are intentionally preserved for later passes.
pub fn eval_process(arena: &mut TreeArena, definitions: TreeId) -> Result<TreeId, EvalError> {
    let mut env = Environment::empty();
    bind_definitions(arena, definitions, &mut env)?;
    let available_defs = top_level_definition_names(arena, definitions)?;
    let process = env
        .lookup("process")
        .ok_or(EvalError::MissingProcessDefinition {
            definitions,
            available_defs,
        })?;
    let mut loop_detector = LoopDetector::new();
    eval_box(arena, process, &env, &mut loop_detector)
}

/// Evaluates one box expression in the provided lexical environment.
///
/// This function is the core recursive evaluator and may:
/// - resolve identifiers,
/// - inline/apply abstractions,
/// - evaluate `with`/`letrec` scopes,
/// - expand iterative forms,
/// - keep non-reduced nodes structurally intact.
pub fn eval_box(
    arena: &mut TreeArena,
    expr: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    match match_box(arena, expr) {
        BoxMatch::Unknown => map_children(arena, expr, env, loop_detector),
        BoxMatch::Ident(name) => {
            let value = env.lookup(name).ok_or_else(|| EvalError::UndefinedSymbol {
                symbol: name.to_owned(),
                node: expr,
                local_scope: env.local_names(),
                visible_scope: env.visible_names(),
                top_level_scope: env.top_level_names(),
            })?;
            if value == expr {
                // Shadowing sentinel used for lambda parameters in lexical scopes.
                return Ok(expr);
            }
            loop_detector.enter(value)?;
            let out = eval_box(arena, value, env, loop_detector);
            loop_detector.leave();
            out
        }
        BoxMatch::Appl(fun, arg) => {
            let efun = eval_box(arena, fun, env, loop_detector)?;
            let rev_args = rev_eval_list(arena, arg, env, loop_detector)?;
            apply_list(arena, efun, rev_args, env, loop_detector, Some(fun))
        }
        BoxMatch::Access(body, field) => eval_access(arena, body, field, env, loop_detector),
        BoxMatch::Case(_) => Ok(expr),
        BoxMatch::PatternVar(_) => Ok(expr),
        BoxMatch::WithLocalDef(body, defs) => {
            let mut scoped = env.push_scope();
            bind_definitions(arena, defs, &mut scoped)?;
            eval_box(arena, body, &scoped, loop_detector)
        }
        BoxMatch::WithRecDef(body, rec_defs, where_defs) => {
            let mut scoped = env.push_scope();
            bind_definitions(arena, rec_defs, &mut scoped)?;
            bind_definitions(arena, where_defs, &mut scoped)?;
            eval_box(arena, body, &scoped, loop_detector)
        }
        BoxMatch::Abstr(arg, body) => {
            let mut scoped = env.push_scope();
            let name = ident_name(arena, arg)?;
            // Parameter shadows outer binding in body capture.
            scoped.bind(name, arg);
            let evaluated_body = eval_box(arena, body, &scoped, loop_detector)?;
            let mut b = BoxBuilder::new(arena);
            Ok(b.abstr(arg, evaluated_body))
        }
        BoxMatch::IPar(index, count, body) => {
            iterate_par(arena, index, count, body, env, loop_detector)
        }
        BoxMatch::ISeq(index, count, body) => {
            iterate_seq(arena, index, count, body, env, loop_detector)
        }
        BoxMatch::ISum(index, count, body) => {
            iterate_sum(arena, index, count, body, env, loop_detector)
        }
        BoxMatch::IProd(index, count, body) => {
            iterate_prod(arena, index, count, body, env, loop_detector)
        }
        _ => map_children(arena, expr, env, loop_detector),
    }
}

/// Evaluates `expr.ident` access with Faust environment semantics.
///
/// When `expr` evaluates to an environment-like form, `ident` is resolved inside
/// this scoped environment. Otherwise the access node is reconstructed on evaluated
/// children.
fn eval_access(
    arena: &mut TreeArena,
    body: TreeId,
    field: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    match match_box(arena, body) {
        BoxMatch::WithLocalDef(inner, defs) => {
            let mut scoped = env.push_scope();
            bind_definitions(arena, defs, &mut scoped)?;
            let inner_eval = eval_box(arena, inner, &scoped, loop_detector)?;
            if matches!(match_box(arena, inner_eval), BoxMatch::Environment) {
                return eval_box(arena, field, &scoped, loop_detector);
            }
        }
        BoxMatch::WithRecDef(inner, rec_defs, where_defs) => {
            let mut scoped = env.push_scope();
            bind_definitions(arena, rec_defs, &mut scoped)?;
            bind_definitions(arena, where_defs, &mut scoped)?;
            let inner_eval = eval_box(arena, inner, &scoped, loop_detector)?;
            if matches!(match_box(arena, inner_eval), BoxMatch::Environment) {
                return eval_box(arena, field, &scoped, loop_detector);
            }
        }
        _ => {}
    }

    let eval_body = eval_box(arena, body, env, loop_detector)?;
    if matches!(match_box(arena, eval_body), BoxMatch::Environment) {
        return eval_box(arena, field, env, loop_detector);
    }
    let eval_field = eval_box(arena, field, env, loop_detector)?;
    let mut b = BoxBuilder::new(arena);
    Ok(b.access(eval_body, eval_field))
}

/// Structural fallback: evaluate all children, then rebuild the node unchanged in kind.
fn map_children(
    arena: &mut TreeArena,
    expr: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let Some(node) = arena.node(expr).cloned() else {
        return Ok(expr);
    };
    let mut children = Vec::with_capacity(node.children.len());
    for child in node.children.as_slice() {
        children.push(eval_box(arena, *child, env, loop_detector)?);
    }
    Ok(arena.intern(node.kind, &children))
}

/// Binds parser definition list into an environment.
///
/// Parser definitions are in `cons(name, cons(args, expr))` form.
/// When `args` is non-empty, this creates nested abstractions (`buildBoxAbstr` parity)
/// before binding.
fn bind_definitions(
    arena: &mut TreeArena,
    mut defs: TreeId,
    env: &mut Environment,
) -> Result<(), EvalError> {
    while !arena.is_nil(defs) {
        let def = arena
            .hd(defs)
            .ok_or(EvalError::MalformedDefinitionNode { node: defs })?;
        let (name, args, value) = decode_definition(arena, def)?;
        let bound = if arena.is_nil(args) {
            value
        } else {
            build_abstr_from_parser_args(arena, args, value)?
        };
        env.bind(name, bound);
        defs = arena
            .tl(defs)
            .ok_or(EvalError::MalformedDefinitionNode { node: defs })?;
    }
    Ok(())
}

/// Decodes one parser definition node into `(name, args, expr)`.
fn decode_definition(
    arena: &TreeArena,
    def: TreeId,
) -> Result<(String, TreeId, TreeId), EvalError> {
    let name_node = arena
        .hd(def)
        .ok_or(EvalError::MalformedDefinitionNode { node: def })?;
    let payload = arena
        .tl(def)
        .ok_or(EvalError::MalformedDefinitionNode { node: def })?;
    let args = arena
        .hd(payload)
        .ok_or(EvalError::MalformedDefinitionNode { node: def })?;
    let expr = arena
        .tl(payload)
        .ok_or(EvalError::MalformedDefinitionNode { node: def })?;

    let name = match match_box(arena, name_node) {
        BoxMatch::Ident(s) => s.to_owned(),
        _ => match arena.kind(name_node) {
            Some(NodeKind::Symbol(s)) => s.as_ref().to_owned(),
            _ => {
                return Err(EvalError::MalformedDefinitionNode { node: def });
            }
        },
    };

    Ok((name, args, expr))
}

/// Extracts top-level definition names in deterministic order for diagnostics.
///
/// Names are sorted and deduplicated so diagnostic snapshots remain stable.
fn top_level_definition_names(
    arena: &TreeArena,
    mut defs: TreeId,
) -> Result<Vec<String>, EvalError> {
    let mut names = Vec::new();
    while !arena.is_nil(defs) {
        let def = arena
            .hd(defs)
            .ok_or(EvalError::MalformedDefinitionNode { node: defs })?;
        let (name, _args, _expr) = decode_definition(arena, def)?;
        names.push(name);
        defs = arena
            .tl(defs)
            .ok_or(EvalError::MalformedDefinitionNode { node: defs })?;
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// Returns identifier text for one `BOXIDENT` node.
fn ident_name(arena: &TreeArena, id: TreeId) -> Result<String, EvalError> {
    match match_box(arena, id) {
        BoxMatch::Ident(name) => Ok(name.to_owned()),
        _ => Err(EvalError::NonIdentifierParameter { node: id }),
    }
}

fn build_abstr_from_parser_args(
    arena: &mut TreeArena,
    mut args: TreeId,
    body: TreeId,
) -> Result<TreeId, EvalError> {
    // C++ parity (`buildBoxAbstr`): parser param lists are reversed, and each
    // head wraps the current body before recursing on tail.
    let mut out = body;
    while !arena.is_nil(args) {
        let head = arena
            .hd(args)
            .ok_or(EvalError::MalformedListNode { node: args })?;
        out = {
            let mut b = BoxBuilder::new(arena);
            b.abstr(head, out)
        };
        args = arena
            .tl(args)
            .ok_or(EvalError::MalformedListNode { node: args })?;
    }
    Ok(out)
}

/// Evaluates argument list nodes and returns the reversed evaluated list.
///
/// This mirrors the C++ parser/evaluator list convention where argument lists are
/// accumulated in reverse order.
fn rev_eval_list(
    arena: &mut TreeArena,
    mut list: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let mut result = arena.nil();
    while !arena.is_nil(list) {
        let head = arena
            .hd(list)
            .ok_or(EvalError::MalformedListNode { node: list })?;
        let value = eval_box(arena, head, env, loop_detector)?;
        result = arena.cons(value, result);
        list = arena
            .tl(list)
            .ok_or(EvalError::MalformedListNode { node: list })?;
    }
    Ok(result)
}

/// Applies an evaluated function-like box to an evaluated argument list.
///
/// Behavior summary:
/// - `abstr`: beta-like application in lexical scope.
/// - `case`: pattern-match dispatch when sufficiently applied, otherwise lowers to
///   non-closure style `seq(par(args + implicit_wires), case)` for C++ parity.
/// - other node families: C++-compatible non-closure lowering to `seq(par(args), fun)`,
///   including implicit wire insertion for partial applications.
fn apply_list(
    arena: &mut TreeArena,
    fun: TreeId,
    larg: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
    call_site: Option<TreeId>,
) -> Result<TreeId, EvalError> {
    if arena.is_nil(larg) {
        return Ok(fun);
    }
    match match_box(arena, fun) {
        BoxMatch::Abstr(id, body) => {
            let param_name = ident_name(arena, id)?;
            let arg = arena
                .hd(larg)
                .ok_or(EvalError::MalformedListNode { node: larg })?;
            let mut scoped = env.push_scope();
            scoped.bind(param_name, arg);
            let f = eval_box(arena, body, &scoped, loop_detector)?;
            let tl = arena
                .tl(larg)
                .ok_or(EvalError::MalformedListNode { node: larg })?;
            apply_list(arena, f, tl, env, loop_detector, call_site)
        }
        BoxMatch::Case(rules) => {
            let expected = case_expected_arity(arena, rules)?;
            let got = list_to_vec(arena, larg)?.len();
            if got < expected {
                // C++ parity (`applyList` on under-applied closures): keep the case form
                // and insert implicit wires for missing arguments instead of evaluating
                // the case immediately.
                let missing = expected - got;
                let wires = nwires(arena, missing);
                let lowered_larg = concat_lists(arena, larg, wires)?;
                let args_par = larg2par(arena, lowered_larg)?;
                let mut b = BoxBuilder::new(arena);
                return Ok(b.seq(args_par, fun));
            }
            apply_case_rules(arena, rules, larg, env, loop_detector, call_site)
        }
        _ => {
            // C++ parity (`applyList`): for non-closures, insert implicit wires when
            // partially applying a function, and reject over-application.
            let maybe_fun_arity = infer_box_arity(arena, fun);
            let maybe_larg_outputs = list_outputs(arena, larg);
            let mut lowered_larg = larg;

            if let (Some((ins, _outs)), Some(larg_outs)) = (maybe_fun_arity, maybe_larg_outputs) {
                if larg_outs > ins {
                    return Err(EvalError::TooManyArguments {
                        node: call_site.unwrap_or(fun),
                        expected: ins,
                        got: larg_outs,
                    });
                }
                let missing = ins - larg_outs;
                if missing > 0 {
                    let wires = nwires(arena, missing);
                    lowered_larg = if larg_outs == 1 && is_binary_primitive_non_prefix(arena, fun) {
                        concat_lists(arena, wires, larg)?
                    } else {
                        concat_lists(arena, larg, wires)?
                    };
                }
            }

            let args_par = larg2par(arena, lowered_larg)?;
            let mut b = BoxBuilder::new(arena);
            Ok(b.seq(args_par, fun))
        }
    }
}

/// Converts parser-style argument list to parallel composition tree.
///
/// Example: `[a,b,c] -> par(a, par(b, c))`.
fn larg2par(arena: &mut TreeArena, larg: TreeId) -> Result<TreeId, EvalError> {
    if arena.is_nil(larg) {
        return Err(EvalError::EmptyArgumentList { node: larg });
    }
    let head = arena
        .hd(larg)
        .ok_or(EvalError::MalformedListNode { node: larg })?;
    let tail = arena
        .tl(larg)
        .ok_or(EvalError::MalformedListNode { node: larg })?;
    if arena.is_nil(tail) {
        Ok(head)
    } else {
        let right = larg2par(arena, tail)?;
        let mut b = BoxBuilder::new(arena);
        Ok(b.par(head, right))
    }
}

/// Concatenates two parser-style lists while preserving element order.
fn concat_lists(arena: &mut TreeArena, left: TreeId, right: TreeId) -> Result<TreeId, EvalError> {
    if arena.is_nil(left) {
        return Ok(right);
    }
    let head = arena
        .hd(left)
        .ok_or(EvalError::MalformedListNode { node: left })?;
    let tail = arena
        .tl(left)
        .ok_or(EvalError::MalformedListNode { node: left })?;
    let rest = concat_lists(arena, tail, right)?;
    Ok(arena.cons(head, rest))
}

/// Builds a parser-style list containing `n` wire nodes.
fn nwires(arena: &mut TreeArena, n: usize) -> TreeId {
    let mut out = arena.nil();
    for _ in 0..n {
        let wire = BoxBuilder::new(arena).wire();
        out = arena.cons(wire, out);
    }
    out
}

/// Computes total output arity for a list of argument boxes.
fn list_outputs(arena: &TreeArena, mut list: TreeId) -> Option<usize> {
    let mut total = 0usize;
    while !arena.is_nil(list) {
        let head = arena.hd(list)?;
        let (_, outs) = infer_box_arity(arena, head)?;
        total = total.checked_add(outs)?;
        list = arena.tl(list)?;
    }
    Some(total)
}

/// Local arity inference used by non-closure application lowering.
///
/// Returns `(inputs, outputs)` for the subset needed in `apply_list`.
/// `None` means arity is unknown or invalid for this fast-path inference.
fn infer_box_arity(arena: &TreeArena, id: TreeId) -> Option<(usize, usize)> {
    match match_box(arena, id) {
        BoxMatch::Int(_) | BoxMatch::Real(_) => Some((0, 1)),
        BoxMatch::Wire => Some((1, 1)),
        BoxMatch::Cut => Some((1, 0)),
        BoxMatch::Add
        | BoxMatch::Sub
        | BoxMatch::Mul
        | BoxMatch::Div
        | BoxMatch::Rem
        | BoxMatch::And
        | BoxMatch::Or
        | BoxMatch::Xor
        | BoxMatch::Lsh
        | BoxMatch::Rsh
        | BoxMatch::Lt
        | BoxMatch::Le
        | BoxMatch::Gt
        | BoxMatch::Ge
        | BoxMatch::Eq
        | BoxMatch::Ne
        | BoxMatch::Pow
        | BoxMatch::Atan2
        | BoxMatch::Fmod
        | BoxMatch::Remainder
        | BoxMatch::Delay
        | BoxMatch::Min
        | BoxMatch::Max
        | BoxMatch::Prefix
        | BoxMatch::Attach
        | BoxMatch::Enable
        | BoxMatch::Control => Some((2, 1)),
        BoxMatch::Delay1
        | BoxMatch::IntCast
        | BoxMatch::FloatCast
        | BoxMatch::Acos
        | BoxMatch::Asin
        | BoxMatch::Atan
        | BoxMatch::Cos
        | BoxMatch::Sin
        | BoxMatch::Tan
        | BoxMatch::Exp
        | BoxMatch::Log
        | BoxMatch::Log10
        | BoxMatch::Sqrt
        | BoxMatch::Abs
        | BoxMatch::Floor
        | BoxMatch::Ceil
        | BoxMatch::Rint
        | BoxMatch::Round
        | BoxMatch::Lowest
        | BoxMatch::Highest => Some((1, 1)),
        BoxMatch::ReadOnlyTable | BoxMatch::Select2 | BoxMatch::AssertBounds => Some((3, 1)),
        BoxMatch::Select3 => Some((4, 1)),
        BoxMatch::WriteReadTable => Some((5, 1)),
        BoxMatch::FConst(_, _, _) | BoxMatch::FVar(_, _, _) => Some((0, 1)),
        BoxMatch::Button(_)
        | BoxMatch::Checkbox(_)
        | BoxMatch::VSlider(_, _, _, _, _)
        | BoxMatch::HSlider(_, _, _, _, _)
        | BoxMatch::NumEntry(_, _, _, _, _) => Some((0, 1)),
        BoxMatch::VBargraph(_, _, _) | BoxMatch::HBargraph(_, _, _) => Some((1, 1)),
        BoxMatch::Soundfile(_, chan) => {
            let BoxMatch::Int(channels) = match_box(arena, chan) else {
                return None;
            };
            let channels = usize::try_from(channels).ok()?;
            Some((2, channels.checked_add(2)?))
        }
        BoxMatch::VGroup(_, inner) | BoxMatch::HGroup(_, inner) | BoxMatch::TGroup(_, inner) => {
            infer_box_arity(arena, inner)
        }
        BoxMatch::Seq(left, right) => {
            let (ins1, outs1) = infer_box_arity(arena, left)?;
            let (ins2, outs2) = infer_box_arity(arena, right)?;
            if outs1 != ins2 {
                return None;
            }
            Some((ins1, outs2))
        }
        BoxMatch::Par(left, right) => {
            let (ins1, outs1) = infer_box_arity(arena, left)?;
            let (ins2, outs2) = infer_box_arity(arena, right)?;
            Some((ins1.checked_add(ins2)?, outs1.checked_add(outs2)?))
        }
        BoxMatch::Split(left, right) => {
            let (ins1, outs1) = infer_box_arity(arena, left)?;
            let (ins2, outs2) = infer_box_arity(arena, right)?;
            if outs1 != ins2 && (outs1 == 0 || !ins2.is_multiple_of(outs1)) {
                return None;
            }
            Some((ins1, outs2))
        }
        BoxMatch::Merge(left, right) => {
            let (ins1, outs1) = infer_box_arity(arena, left)?;
            let (ins2, outs2) = infer_box_arity(arena, right)?;
            if outs1 != ins2 && (ins2 == 0 || !outs1.is_multiple_of(ins2)) {
                return None;
            }
            Some((ins1, outs2))
        }
        BoxMatch::Rec(left, right) => {
            let (ins1, outs1) = infer_box_arity(arena, left)?;
            let (ins2, outs2) = infer_box_arity(arena, right)?;
            if ins2 > outs1 || outs2 > ins1 {
                return None;
            }
            Some((ins1 - outs2, outs1))
        }
        BoxMatch::Environment => Some((0, 0)),
        BoxMatch::Route(ins, outs, _) => {
            let BoxMatch::Int(ins_n) = match_box(arena, ins) else {
                return None;
            };
            let BoxMatch::Int(outs_n) = match_box(arena, outs) else {
                return None;
            };
            let ins_n = usize::try_from(ins_n).ok()?;
            let outs_n = usize::try_from(outs_n).ok()?;
            Some((ins_n, outs_n))
        }
        BoxMatch::Inputs(_) | BoxMatch::Outputs(_) => Some((0, 1)),
        BoxMatch::Ondemand(inner) | BoxMatch::Upsampling(inner) | BoxMatch::Downsampling(inner) => {
            let (ins, outs) = infer_box_arity(arena, inner)?;
            Some((ins.checked_add(1)?, outs))
        }
        _ => None,
    }
}

/// Returns true for primitive binary operators that are not `prefix`.
fn is_binary_primitive_non_prefix(arena: &TreeArena, id: TreeId) -> bool {
    matches!(
        match_box(arena, id),
        BoxMatch::Add
            | BoxMatch::Sub
            | BoxMatch::Mul
            | BoxMatch::Div
            | BoxMatch::Rem
            | BoxMatch::And
            | BoxMatch::Or
            | BoxMatch::Xor
            | BoxMatch::Lsh
            | BoxMatch::Rsh
            | BoxMatch::Lt
            | BoxMatch::Le
            | BoxMatch::Gt
            | BoxMatch::Ge
            | BoxMatch::Eq
            | BoxMatch::Ne
            | BoxMatch::Pow
            | BoxMatch::Atan2
            | BoxMatch::Fmod
            | BoxMatch::Remainder
            | BoxMatch::Delay
            | BoxMatch::Min
            | BoxMatch::Max
            | BoxMatch::Attach
            | BoxMatch::Enable
            | BoxMatch::Control
    )
}

fn iteration_var_name(arena: &TreeArena, id: TreeId) -> Result<String, EvalError> {
    match match_box(arena, id) {
        BoxMatch::Ident(name) => Ok(name.to_owned()),
        _ => Err(EvalError::NonIdentifierIterationVariable { node: id }),
    }
}

/// Evaluates iterative count expression and enforces a non-negative integer result.
fn eval_non_negative_count(
    arena: &mut TreeArena,
    count_expr: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<usize, EvalError> {
    let count = eval_box(arena, count_expr, env, loop_detector)?;
    match match_box(arena, count) {
        BoxMatch::Int(v) if v < 0 => Err(EvalError::NegativeIterationCount { value: v }),
        BoxMatch::Int(v) => {
            usize::try_from(v).map_err(|_| EvalError::IterationCountTooLarge { value: v })
        }
        _ => Err(EvalError::IterationCountNotInt { node: count }),
    }
}

/// Evaluates iterative body with one bound loop index (`i`).
fn eval_iter_body(
    arena: &mut TreeArena,
    var_name: &str,
    i: usize,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let mut scoped = env.push_scope();
    let i_as_i64 =
        i64::try_from(i).map_err(|_| EvalError::IterationCountTooLarge { value: i64::MAX })?;
    let ival = arena.int(i_as_i64);
    scoped.bind(var_name.to_owned(), ival);
    eval_box(arena, body, &scoped, loop_detector)
}

/// Returns the C++-compatible empty-iteration neutral box (`route(0,0,par(0,0))`).
fn empty_iteration_route(arena: &mut TreeArena) -> TreeId {
    let mut b = BoxBuilder::new(arena);
    let z0 = b.int(0);
    let z1 = b.int(0);
    let spec = b.par(z0, z1);
    b.route(z0, z1, spec)
}

/// Expands `ipar(i,n,body)` into nested `par` composition.
fn iterate_par(
    arena: &mut TreeArena,
    index: TreeId,
    count: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let var_name = iteration_var_name(arena, index)?;
    let n = eval_non_negative_count(arena, count, env, loop_detector)?;
    if n == 0 {
        return Ok(empty_iteration_route(arena));
    }
    let mut res = eval_iter_body(arena, &var_name, n - 1, body, env, loop_detector)?;
    for i in (0..(n - 1)).rev() {
        let left = eval_iter_body(arena, &var_name, i, body, env, loop_detector)?;
        res = {
            let mut b = BoxBuilder::new(arena);
            b.par(left, res)
        };
    }
    Ok(res)
}

/// Expands `iseq(i,n,body)` into nested `seq` composition.
fn iterate_seq(
    arena: &mut TreeArena,
    index: TreeId,
    count: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let var_name = iteration_var_name(arena, index)?;
    let n = eval_non_negative_count(arena, count, env, loop_detector)?;
    if n == 0 {
        return Ok(empty_iteration_route(arena));
    }
    let mut res = eval_iter_body(arena, &var_name, n - 1, body, env, loop_detector)?;
    for i in (0..(n - 1)).rev() {
        let left = eval_iter_body(arena, &var_name, i, body, env, loop_detector)?;
        res = {
            let mut b = BoxBuilder::new(arena);
            b.seq(left, res)
        };
    }
    Ok(res)
}

/// Expands `isum(i,n,body)` into a fold using `add` primitive.
fn iterate_sum(
    arena: &mut TreeArena,
    index: TreeId,
    count: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let var_name = iteration_var_name(arena, index)?;
    let n = eval_non_negative_count(arena, count, env, loop_detector)?;
    if n == 0 {
        return Ok(empty_iteration_route(arena));
    }
    let mut res = eval_iter_body(arena, &var_name, 0, body, env, loop_detector)?;
    for i in 1..n {
        let rhs = eval_iter_body(arena, &var_name, i, body, env, loop_detector)?;
        let pair = {
            let mut b = BoxBuilder::new(arena);
            b.par(res, rhs)
        };
        let add = {
            let mut b = BoxBuilder::new(arena);
            b.add()
        };
        res = {
            let mut b = BoxBuilder::new(arena);
            b.seq(pair, add)
        };
    }
    Ok(res)
}

/// Expands `iprod(i,n,body)` into a fold using `mul` primitive.
fn iterate_prod(
    arena: &mut TreeArena,
    index: TreeId,
    count: TreeId,
    body: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    let var_name = iteration_var_name(arena, index)?;
    let n = eval_non_negative_count(arena, count, env, loop_detector)?;
    if n == 0 {
        return Ok(empty_iteration_route(arena));
    }
    let mut res = eval_iter_body(arena, &var_name, 0, body, env, loop_detector)?;
    for i in 1..n {
        let rhs = eval_iter_body(arena, &var_name, i, body, env, loop_detector)?;
        let pair = {
            let mut b = BoxBuilder::new(arena);
            b.par(res, rhs)
        };
        let mul = {
            let mut b = BoxBuilder::new(arena);
            b.mul()
        };
        res = {
            let mut b = BoxBuilder::new(arena);
            b.seq(pair, mul)
        };
    }
    Ok(res)
}

/// Converts a parser-style list into a vector in traversal order.
fn list_to_vec(arena: &TreeArena, mut list: TreeId) -> Result<Vec<TreeId>, EvalError> {
    let mut out = Vec::new();
    while !arena.is_nil(list) {
        let head = arena
            .hd(list)
            .ok_or(EvalError::MalformedListNode { node: list })?;
        out.push(head);
        list = arena
            .tl(list)
            .ok_or(EvalError::MalformedListNode { node: list })?;
    }
    Ok(out)
}

/// Converts a vector into a parser-style list preserving order.
fn vec_to_list(arena: &mut TreeArena, items: &[TreeId]) -> TreeId {
    let mut out = arena.nil();
    for id in items.iter().rev() {
        out = arena.cons(*id, out);
    }
    out
}

/// Decodes a case rule node into `(lhs_patterns, rhs_expr)`.
fn rule_parts(arena: &TreeArena, rule: TreeId) -> Result<(TreeId, TreeId), EvalError> {
    let lhs = arena
        .hd(rule)
        .ok_or(EvalError::MalformedCaseNode { node: rule })?;
    let rhs = arena
        .tl(rule)
        .ok_or(EvalError::MalformedCaseNode { node: rule })?;
    Ok((lhs, rhs))
}

/// Returns expected argument arity for a case-rule set (first source rule arity).
fn case_expected_arity(arena: &TreeArena, rules_rev: TreeId) -> Result<usize, EvalError> {
    let mut rules = list_to_vec(arena, rules_rev)?;
    rules.reverse();
    let Some(first_rule) = rules.first().copied() else {
        return Err(EvalError::MalformedCaseNode { node: rules_rev });
    };
    let (first_lhs, _first_rhs) = rule_parts(arena, first_rule)?;
    Ok(list_to_vec(arena, first_lhs)?.len())
}

/// Applies case rules to a given argument list with C++-compatible first-match semantics.
fn apply_case_rules(
    arena: &mut TreeArena,
    rules_rev: TreeId,
    larg: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
    call_site: Option<TreeId>,
) -> Result<TreeId, EvalError> {
    let args = list_to_vec(arena, larg)?;
    let mut rules = list_to_vec(arena, rules_rev)?;
    rules.reverse();
    let Some(first_rule) = rules.first().copied() else {
        return Err(EvalError::MalformedCaseNode { node: rules_rev });
    };

    let (first_lhs, _first_rhs) = rule_parts(arena, first_rule)?;
    let expected = list_to_vec(arena, first_lhs)?.len();
    if args.len() < expected {
        return Err(EvalError::PatternArityMismatch {
            node: rules_rev,
            expected,
            got: args.len(),
        });
    }
    let consumed = &args[..expected];
    let rest = &args[expected..];

    for rule in rules {
        let (lhs_rev, rhs) = rule_parts(arena, rule)?;
        let mut patterns = list_to_vec(arena, lhs_rev)?;
        patterns.reverse();
        if patterns.len() != expected {
            return Err(EvalError::MalformedCaseNode { node: rule });
        }

        let mut bindings = Environment::empty();
        let mut ok = true;
        for (pat, arg) in patterns.iter().zip(consumed.iter()) {
            let prepared_pat = eval_box(arena, *pat, env, loop_detector)?;
            if !match_pattern(arena, prepared_pat, *arg, &mut bindings)? {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }

        let mut scoped = env.push_scope();
        for (name, value) in &bindings.bindings {
            scoped.bind(name.clone(), *value);
        }
        let result = eval_box(arena, rhs, &scoped, loop_detector)?;
        if rest.is_empty() {
            return Ok(result);
        }
        let rest_list = vec_to_list(arena, rest);
        return apply_list(arena, result, rest_list, env, loop_detector, call_site);
    }

    Err(EvalError::PatternMatchFailed { node: rules_rev })
}

/// Structural pattern matching helper used by case-rule application.
fn match_pattern(
    arena: &TreeArena,
    pattern: TreeId,
    value: TreeId,
    bindings: &mut Environment,
) -> Result<bool, EvalError> {
    if let BoxMatch::PatternVar(ident_node) = match_box(arena, pattern) {
        let name = ident_name(arena, ident_node)?;
        if let Some(existing) = bindings.lookup(&name) {
            return Ok(existing == value);
        }
        bindings.bind(name, value);
        return Ok(true);
    }

    if pattern == value {
        return Ok(true);
    }

    let Some(pn) = arena.node(pattern) else {
        return Ok(false);
    };
    let Some(vn) = arena.node(value) else {
        return Ok(false);
    };
    if pn.kind != vn.kind || pn.children.len() != vn.children.len() {
        return Ok(false);
    }
    for (pc, vc) in pn
        .children
        .as_slice()
        .iter()
        .zip(vn.children.as_slice().iter())
    {
        if !match_pattern(arena, *pc, *vc, bindings)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
