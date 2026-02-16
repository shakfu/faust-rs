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
            loop_detector.enter(value)?;
            let out = eval_box(arena, value, env, loop_detector);
            loop_detector.leave();
            out
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
            if let BoxMatch::Ident(name) = match_box(arena, arg) {
                // Parameter shadows outer binding, preserving lambda scope.
                scoped.bind(name, arg);
            }
            let evaluated_body = eval_box(arena, body, &scoped, loop_detector)?;
            let mut b = BoxBuilder::new(arena);
            Ok(b.abstr(arg, evaluated_body))
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
            let mut b = BoxBuilder::new(arena);
            b.build_abstr(args, value)
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

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
