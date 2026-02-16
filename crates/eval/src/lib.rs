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

use std::fmt::{Display, Formatter};

use boxes::{BoxBuilder, BoxMatch, match_box};
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
    MissingProcessDefinition,
    UndefinedSymbol { symbol: String },
    MalformedDefinitionNode { node: TreeId },
    MalformedListNode { node: TreeId },
    EmptyArgumentList,
    NonIdentifierParameter { node: TreeId },
    NonIdentifierIterationVariable { node: TreeId },
    IterationCountNotInt { node: TreeId },
    IterationCountTooLarge { value: i64 },
    NegativeIterationCount { value: i64 },
    LoopDetected { node: TreeId },
    RecursionDepthExceeded { max_depth: usize },
}

impl Display for EvalError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingProcessDefinition => write!(f, "missing `process` definition"),
            Self::UndefinedSymbol { symbol } => write!(f, "undefined symbol `{symbol}`"),
            Self::MalformedDefinitionNode { node } => {
                write!(f, "malformed definition node {}", node.as_u32())
            }
            Self::MalformedListNode { node } => {
                write!(f, "malformed list node {}", node.as_u32())
            }
            Self::EmptyArgumentList => write!(f, "empty argument list"),
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

/// Evaluates one Faust program root list and returns the resolved `process` expression.
///
/// `definitions` is expected to be the parser root list where each item is:
/// `cons(name, cons(args, expr))`.
pub fn eval_process(arena: &mut TreeArena, definitions: TreeId) -> Result<TreeId, EvalError> {
    let mut env = Environment::empty();
    bind_definitions(arena, definitions, &mut env)?;
    let process = env
        .lookup("process")
        .ok_or(EvalError::MissingProcessDefinition)?;
    let mut loop_detector = LoopDetector::new();
    eval_box(arena, process, &env, &mut loop_detector)
}

/// Complete evaluation of a box expression in an environment.
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
            apply_list(arena, efun, rev_args, env, loop_detector)
        }
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

fn apply_list(
    arena: &mut TreeArena,
    fun: TreeId,
    larg: TreeId,
    env: &Environment,
    loop_detector: &mut LoopDetector,
) -> Result<TreeId, EvalError> {
    if arena.is_nil(larg) {
        return Ok(fun);
    }
    if let BoxMatch::Abstr(id, body) = match_box(arena, fun) {
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
        apply_list(arena, f, tl, env, loop_detector)
    } else {
        let args_par = larg2par(arena, larg)?;
        let mut b = BoxBuilder::new(arena);
        Ok(b.seq(args_par, fun))
    }
}

fn larg2par(arena: &mut TreeArena, larg: TreeId) -> Result<TreeId, EvalError> {
    if arena.is_nil(larg) {
        return Err(EvalError::EmptyArgumentList);
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

fn iteration_var_name(arena: &TreeArena, id: TreeId) -> Result<String, EvalError> {
    match match_box(arena, id) {
        BoxMatch::Ident(name) => Ok(name.to_owned()),
        _ => Err(EvalError::NonIdentifierIterationVariable { node: id }),
    }
}

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

fn empty_iteration_route(arena: &mut TreeArena) -> TreeId {
    let mut b = BoxBuilder::new(arena);
    let z0 = b.int(0);
    let z1 = b.int(0);
    let spec = b.par(z0, z1);
    b.route(z0, z1, spec)
}

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

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
