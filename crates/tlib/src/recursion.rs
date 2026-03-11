//! Recursive-tree helpers (`DEBRUIJN`/`DEBRUIJNREF` and symbolic form).
//!
//! # Source provenance (C++)
//! - `compiler/tlib/recursive-tree.cpp` (`deBruijn2Sym`, `substitute`, `liftn`)
//! - `compiler/tlib/tree.hh` (recursive-tree API surface)
//!
//! # Public API mapping status
//! - `deBruijn2Sym(Tree)` -> [`de_bruijn_to_sym`] (`adapted`: returns `Result`)
//! - `lift/liftn(Tree, threshold)` -> [`lift_de_bruijn`] / [`lift_de_bruijn_n`] (`adapted`)
//! - symbolic recursion shape -> explicit tags [`SYMREC_TAG`]/[`SYMREF_TAG`] (`adapted`)
//!
//! # Parity invariants
//! - De Bruijn binder is `DEBRUIJN(body)`, reference is `DEBRUIJNREF(level)`.
//! - Symbolic recursion is explicit and deterministic:
//!   - `SYMREC(var, body)`
//!   - `SYMREF(var)`
//! - Conversion keeps structural sharing through memoized rebuilds.

use std::error::Error;
use std::fmt;

use ahash::AHashMap;

use crate::{NodeKind, TreeArena, TreeId, tree_to_int};

/// Tag for one de Bruijn recursive group binder: `DEBRUIJN(body)`.
pub const DEBRUIJN_TAG: &str = "DEBRUIJN";
/// Tag for one de Bruijn reference payload: `DEBRUIJNREF(level)`.
pub const DEBRUIJNREF_TAG: &str = "DEBRUIJNREF";
/// Tag for one symbolic recursive binder: `SYMREC(var, body)`.
pub const SYMREC_TAG: &str = "SYMREC";
/// Tag for one symbolic recursive reference: `SYMREF(var)`.
pub const SYMREF_TAG: &str = "SYMREF";

/// Errors returned by recursive-tree conversion utilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecursionError {
    /// The provided root contains free de Bruijn references (`aperture > 0`).
    OpenDeBruijnTree { aperture: i64 },
    /// A de Bruijn reference remained during conversion, which indicates an unbound reference.
    UnboundDeBruijnReference { node: TreeId, level: i64 },
    /// One `DEBRUIJNREF` node did not have the expected `int` payload.
    MalformedDeBruijnReference { node: TreeId },
    /// One requested `TreeId` did not exist in the arena.
    InvalidNode { node: TreeId },
}

impl fmt::Display for RecursionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenDeBruijnTree { aperture } => {
                write!(f, "tree is open in de Bruijn form (aperture={aperture})")
            }
            Self::UnboundDeBruijnReference { node, level } => {
                write!(
                    f,
                    "unbound DEBRUIJNREF(level={level}) at node {} during conversion",
                    node.as_u32()
                )
            }
            Self::MalformedDeBruijnReference { node } => {
                write!(
                    f,
                    "malformed DEBRUIJNREF payload at node {} (expected int level)",
                    node.as_u32()
                )
            }
            Self::InvalidNode { node } => write!(f, "invalid node id {}", node.as_u32()),
        }
    }
}

impl Error for RecursionError {}

/// Builds `DEBRUIJN(body)`.
#[must_use]
pub fn de_bruijn_rec(arena: &mut TreeArena, body: TreeId) -> TreeId {
    intern_tag(arena, DEBRUIJN_TAG, &[body])
}

/// Builds `DEBRUIJNREF(level)`.
#[must_use]
pub fn de_bruijn_ref(arena: &mut TreeArena, level: i64) -> TreeId {
    let level_id = arena.int(level);
    intern_tag(arena, DEBRUIJNREF_TAG, &[level_id])
}

/// Builds `SYMREC(var, body)` using the explicit symbolic shape.
#[must_use]
pub fn sym_rec(arena: &mut TreeArena, var: TreeId, body: TreeId) -> TreeId {
    intern_tag(arena, SYMREC_TAG, &[var, body])
}

/// Builds `SYMREF(var)` using the explicit symbolic shape.
#[must_use]
pub fn sym_ref(arena: &mut TreeArena, var: TreeId) -> TreeId {
    intern_tag(arena, SYMREF_TAG, &[var])
}

/// Matches `DEBRUIJN(body)`.
#[must_use]
pub fn match_de_bruijn_rec(arena: &TreeArena, id: TreeId) -> Option<TreeId> {
    let children = tag_children(arena, id, DEBRUIJN_TAG)?;
    match children {
        [body] => Some(*body),
        _ => None,
    }
}

/// Matches `DEBRUIJNREF(level)` and decodes `level`.
///
/// Returns `None` when node shape is not de Bruijn reference or payload is not `Int`.
#[must_use]
pub fn match_de_bruijn_ref(arena: &TreeArena, id: TreeId) -> Option<i64> {
    let children = tag_children(arena, id, DEBRUIJNREF_TAG)?;
    match children {
        [lvl] => tree_to_int(arena, *lvl),
        _ => None,
    }
}

/// Matches symbolic binder `SYMREC(var, body)`.
#[must_use]
pub fn match_sym_rec(arena: &TreeArena, id: TreeId) -> Option<(TreeId, TreeId)> {
    let children = tag_children(arena, id, SYMREC_TAG)?;
    match children {
        [var, body] => Some((*var, *body)),
        _ => None,
    }
}

/// Matches symbolic reference `SYMREF(var)`.
///
/// Compatibility note: one-child legacy `SYMREC(var)` is also accepted as symbolic ref.
#[must_use]
pub fn match_sym_ref(arena: &TreeArena, id: TreeId) -> Option<TreeId> {
    if let Some(children) = tag_children(arena, id, SYMREF_TAG)
        && let [var] = children
    {
        return Some(*var);
    }
    let children = tag_children(arena, id, SYMREC_TAG)?;
    match children {
        [var] => Some(*var),
        _ => None,
    }
}

/// Computes de Bruijn aperture (`<= 0` means closed).
///
/// Semantics mirror C++ `calcTreeAperture`:
/// - `DEBRUIJNREF(level)` -> `level`
/// - `DEBRUIJN(body)` -> `aperture(body) - 1`
/// - any other node -> `max(aperture(children))`
#[must_use]
pub fn de_bruijn_aperture(arena: &TreeArena, root: TreeId) -> i64 {
    let mut memo = AHashMap::new();
    aperture(arena, root, &mut memo)
}

/// Returns `true` when `root` has no free de Bruijn references.
#[must_use]
pub fn is_de_bruijn_closed(arena: &TreeArena, root: TreeId) -> bool {
    de_bruijn_aperture(arena, root) <= 0
}

/// Lifts free de Bruijn references by one level (`liftn(..., 1)`).
#[must_use]
pub fn lift_de_bruijn(arena: &mut TreeArena, root: TreeId) -> TreeId {
    lift_de_bruijn_n(arena, root, 1)
}

/// Lifts free de Bruijn references with one threshold level (`liftn` parity).
///
/// References with `level < threshold` are considered bound and unchanged.
#[must_use]
pub fn lift_de_bruijn_n(arena: &mut TreeArena, root: TreeId, threshold: i64) -> TreeId {
    let mut lifter = Lifter::new(arena);
    lifter.lift(root, threshold)
}

/// Converts one closed de Bruijn recursion tree into symbolic recursion form.
///
/// Returns `Err` when the input tree is open (`aperture > 0`), malformed, or contains
/// unbound references during conversion.
pub fn de_bruijn_to_sym(arena: &mut TreeArena, root: TreeId) -> Result<TreeId, RecursionError> {
    let aperture = de_bruijn_aperture(arena, root);
    if aperture > 0 {
        return Err(RecursionError::OpenDeBruijnTree { aperture });
    }
    let mut converter = Converter::new(arena);
    converter.convert(root)
}

/// Stateful de Bruijn-to-symbolic converter.
///
/// This helper mirrors the staged C++ algorithm:
/// 1. allocate one fresh symbolic variable per encountered `DEBRUIJN(...)`
///    binder,
/// 2. substitute `DEBRUIJNREF(1)` in the binder body with `SYMREF(var)`,
/// 3. recursively rebuild the result while preserving sharing via memoization.
///
/// Separate memos are kept for:
/// - full-node conversion,
/// - substitution at `(node, level, replacement)`,
/// - aperture queries used to prune substitution work.
struct Converter<'a> {
    arena: &'a mut TreeArena,
    next_var_index: u64,
    convert_memo: AHashMap<TreeId, TreeId>,
    substitute_memo: AHashMap<(TreeId, i64, TreeId), TreeId>,
    aperture_memo: AHashMap<TreeId, i64>,
}

impl<'a> Converter<'a> {
    /// Creates one converter scoped to a single output arena/root conversion.
    fn new(arena: &'a mut TreeArena) -> Self {
        Self {
            arena,
            next_var_index: 0,
            convert_memo: AHashMap::new(),
            substitute_memo: AHashMap::new(),
            aperture_memo: AHashMap::new(),
        }
    }

    /// Allocates one deterministic fresh symbolic variable name (`W0`, `W1`, ...).
    ///
    /// The exact prefix is an implementation detail, but the sequence is stable
    /// for a given traversal order, which keeps converted trees deterministic.
    fn fresh_var(&mut self) -> TreeId {
        let name = format!("W{}", self.next_var_index);
        self.next_var_index += 1;
        self.arena.symbol(name)
    }

    /// Converts one node from de Bruijn form to symbolic form.
    ///
    /// Non-recursive nodes are rebuilt recursively with converted children.
    /// `DEBRUIJN(...)` binders trigger the fresh-variable/substitute/rebuild
    /// sequence described on [`Converter`].
    fn convert(&mut self, id: TreeId) -> Result<TreeId, RecursionError> {
        if let Some(mapped) = self.convert_memo.get(&id) {
            return Ok(*mapped);
        }

        let Some(node) = self.arena.node(id).cloned() else {
            return Err(RecursionError::InvalidNode { node: id });
        };

        if let Some(body) = match_de_bruijn_rec(self.arena, id) {
            let var = self.fresh_var();
            let replacement = sym_ref(self.arena, var);
            let substituted = self.substitute(body, 1, replacement)?;
            let converted_body = self.convert(substituted)?;
            let out = sym_rec(self.arena, var, converted_body);
            self.convert_memo.insert(id, out);
            return Ok(out);
        }

        if let Some((var, body)) = match_sym_rec(self.arena, id) {
            let converted_body = self.convert(body)?;
            let out = sym_rec(self.arena, var, converted_body);
            self.convert_memo.insert(id, out);
            return Ok(out);
        }

        if match_sym_ref(self.arena, id).is_some() {
            self.convert_memo.insert(id, id);
            return Ok(id);
        }

        if let Some(level) = match_de_bruijn_ref(self.arena, id) {
            return Err(RecursionError::UnboundDeBruijnReference { node: id, level });
        }
        if is_de_bruijn_ref_tag(self.arena, id) {
            return Err(RecursionError::MalformedDeBruijnReference { node: id });
        }

        let mut converted_children = Vec::with_capacity(node.children.len());
        for child in node.children.as_slice() {
            converted_children.push(self.convert(*child)?);
        }
        let out = self.arena.intern(node.kind, &converted_children);
        self.convert_memo.insert(id, out);
        Ok(out)
    }

    /// Substitutes the de Bruijn reference at exact `level` with `replacement`.
    ///
    /// This mirrors the C++ `substitute` helper:
    /// - `DEBRUIJNREF(level)` matching the requested level is replaced,
    /// - binder bodies recurse with `level + 1`,
    /// - nodes whose aperture is already below `level` are reused unchanged.
    fn substitute(
        &mut self,
        id: TreeId,
        level: i64,
        replacement: TreeId,
    ) -> Result<TreeId, RecursionError> {
        let key = (id, level, replacement);
        if let Some(mapped) = self.substitute_memo.get(&key) {
            return Ok(*mapped);
        }

        if self.aperture(id)? < level {
            self.substitute_memo.insert(key, id);
            return Ok(id);
        }

        if let Some(ref_level) = match_de_bruijn_ref(self.arena, id) {
            let out = if ref_level == level { replacement } else { id };
            self.substitute_memo.insert(key, out);
            return Ok(out);
        }
        if is_de_bruijn_ref_tag(self.arena, id) {
            return Err(RecursionError::MalformedDeBruijnReference { node: id });
        }

        if let Some(body) = match_de_bruijn_rec(self.arena, id) {
            let sub = self.substitute(body, level + 1, replacement)?;
            let out = de_bruijn_rec(self.arena, sub);
            self.substitute_memo.insert(key, out);
            return Ok(out);
        }

        let Some(node) = self.arena.node(id).cloned() else {
            return Err(RecursionError::InvalidNode { node: id });
        };
        let mut out_children = Vec::with_capacity(node.children.len());
        for child in node.children.as_slice() {
            out_children.push(self.substitute(*child, level, replacement)?);
        }
        let out = self.arena.intern(node.kind, &out_children);
        self.substitute_memo.insert(key, out);
        Ok(out)
    }

    /// Computes aperture with error reporting for malformed references/invalid nodes.
    ///
    /// Unlike the public [`de_bruijn_aperture`], this variant participates in
    /// conversion and therefore reports malformed trees instead of silently
    /// treating missing payloads as ordinary nodes.
    fn aperture(&mut self, id: TreeId) -> Result<i64, RecursionError> {
        if let Some(value) = self.aperture_memo.get(&id) {
            return Ok(*value);
        }

        if let Some(level) = match_de_bruijn_ref(self.arena, id) {
            self.aperture_memo.insert(id, level);
            return Ok(level);
        }
        if is_de_bruijn_ref_tag(self.arena, id) {
            return Err(RecursionError::MalformedDeBruijnReference { node: id });
        }

        let value = if let Some(body) = match_de_bruijn_rec(self.arena, id) {
            self.aperture(body)? - 1
        } else {
            let Some(children) = self.arena.children(id).map(|ch| ch.to_vec()) else {
                return Err(RecursionError::InvalidNode { node: id });
            };
            let mut max_aperture = 0;
            for child in children {
                let child_aperture = self.aperture(child)?;
                if child_aperture > max_aperture {
                    max_aperture = child_aperture;
                }
            }
            max_aperture
        };

        self.aperture_memo.insert(id, value);
        Ok(value)
    }
}

/// Stateful helper implementing `liftn`.
///
/// `threshold` tracks how many binders must be crossed before a reference is
/// considered free and therefore needs to be incremented.
struct Lifter<'a> {
    arena: &'a mut TreeArena,
    memo: AHashMap<(TreeId, i64), TreeId>,
}

impl<'a> Lifter<'a> {
    /// Creates one memoizing lifter scoped to a single arena/root call.
    fn new(arena: &'a mut TreeArena) -> Self {
        Self {
            arena,
            memo: AHashMap::new(),
        }
    }

    /// Lifts all free references with `level >= threshold` by one.
    ///
    /// Binder bodies recurse with `threshold + 1`, which preserves binding for
    /// locally bound references while shifting only the free part of the tree.
    fn lift(&mut self, id: TreeId, threshold: i64) -> TreeId {
        let key = (id, threshold);
        if let Some(mapped) = self.memo.get(&key) {
            return *mapped;
        }

        let out = if let Some(level) = match_de_bruijn_ref(self.arena, id) {
            if level < threshold {
                id
            } else {
                de_bruijn_ref(self.arena, level + 1)
            }
        } else if let Some(body) = match_de_bruijn_rec(self.arena, id) {
            let lifted = self.lift(body, threshold + 1);
            de_bruijn_rec(self.arena, lifted)
        } else if let Some(node) = self.arena.node(id).cloned() {
            let mut children = Vec::with_capacity(node.children.len());
            for child in node.children.as_slice() {
                children.push(self.lift(*child, threshold));
            }
            self.arena.intern(node.kind, &children)
        } else {
            id
        };

        self.memo.insert(key, out);
        out
    }
}

/// Memoized aperture worker used by the public read-only aperture query.
///
/// This variant is deliberately permissive: malformed `DEBRUIJNREF` payloads
/// simply behave like ordinary nodes because the public helper returns only an
/// integer aperture, not a `Result`.
fn aperture(arena: &TreeArena, root: TreeId, memo: &mut AHashMap<TreeId, i64>) -> i64 {
    if let Some(value) = memo.get(&root) {
        return *value;
    }

    let value = if let Some(level) = match_de_bruijn_ref(arena, root) {
        level
    } else if let Some(body) = match_de_bruijn_rec(arena, root) {
        aperture(arena, body, memo) - 1
    } else {
        let mut max_aperture = 0;
        if let Some(children) = arena.children(root) {
            for child in children {
                let child_aperture = aperture(arena, *child, memo);
                if child_aperture > max_aperture {
                    max_aperture = child_aperture;
                }
            }
        }
        max_aperture
    };

    memo.insert(root, value);
    value
}

/// Returns children when `id` is a tag node with the exact expected tag name.
///
/// This small matcher centralizes the "tag name + arity checked by caller"
/// pattern shared by all public recursion matchers.
fn tag_children<'a>(arena: &'a TreeArena, id: TreeId, expected_tag: &str) -> Option<&'a [TreeId]> {
    let node = arena.node(id)?;
    let NodeKind::Tag(tag_id) = &node.kind else {
        return None;
    };
    if arena.tag_name(*tag_id)? != expected_tag {
        return None;
    }
    Some(node.children.as_slice())
}

/// Returns whether `id` is tagged as `DEBRUIJNREF`, regardless of payload shape.
///
/// This is used to distinguish a malformed reference payload from a completely
/// unrelated node kind when producing structured errors.
fn is_de_bruijn_ref_tag(arena: &TreeArena, id: TreeId) -> bool {
    let Some(node) = arena.node(id) else {
        return false;
    };
    let NodeKind::Tag(tag_id) = &node.kind else {
        return false;
    };
    arena.tag_name(*tag_id) == Some(DEBRUIJNREF_TAG)
}

/// Interns one tag node by tag name plus ordered children.
///
/// This keeps all recursion-specific builders on the same hash-consing path as
/// the rest of `TreeArena`.
fn intern_tag(arena: &mut TreeArena, tag: &str, children: &[TreeId]) -> TreeId {
    let tag_id = arena.intern_tag(tag);
    arena.intern(NodeKind::Tag(tag_id), children)
}
