//! `tlib` foundation crate for Faust tree-like compiler data.
//!
//! # Source provenance (C++)
//! - `compiler/tlib/tree.hh`, `compiler/tlib/tree.cpp`
//! - `compiler/tlib/list.hh`, `compiler/tlib/list.cpp`
//! - `compiler/tlib/property.hh`
//! - `compiler/tlib/node.hh`, `compiler/tlib/symbol.hh`
//!
//! # Parity invariants
//! - Structural hash-consing: identical `(NodeKind, children)` must map to the same `TreeId`.
//! - `nil/cons` list behavior follows Faust C++ conventions (`cons`, `hd`, `tl`, `isNil`, `isList`).
//! - Properties are node-keyed and support fast keyed access for parser/evaluator hot paths.
//!
//! # Recursive trees (Phase 5)
//! The crate now exposes explicit recursive-tree helpers for de Bruijn and symbolic forms:
//! - De Bruijn builders/matchers: [`de_bruijn_rec`], [`de_bruijn_ref`],
//!   [`match_de_bruijn_rec`], [`match_de_bruijn_ref`]
//! - Symbolic builders/matchers: [`sym_rec`], [`sym_ref`], [`match_sym_rec`], [`match_sym_ref`]
//! - Conversion and analysis helpers: [`de_bruijn_to_sym`], [`de_bruijn_aperture`],
//!   [`is_de_bruijn_closed`], [`lift_de_bruijn`], [`lift_de_bruijn_n`]
//!
//! Mapping note:
//! - `deBruijn2Sym(Tree)` from C++ is ported as [`de_bruijn_to_sym`] with explicit
//!   [`RecursionError`] returns instead of process-global assertions.
//!
//! Pipeline contract note:
//! - current fast-lane (`transform::signal_fir`) still consumes de Bruijn recursion directly;
//!   normalization-level symbolic conversion remains opt-in at call-site boundaries.

mod arena;
mod property;
mod recursion;

pub use arena::{ChildList, NodeKind, TreeArena, TreeId, TreeNode};
pub use property::{PropertyKey, PropertyStore};
pub use recursion::{
    DEBRUIJN_TAG, DEBRUIJNREF_TAG, RecursionError, SYMREC_TAG, SYMREF_TAG, de_bruijn_aperture,
    de_bruijn_rec, de_bruijn_ref, de_bruijn_to_sym, is_de_bruijn_closed, lift_de_bruijn,
    lift_de_bruijn_n, match_de_bruijn_rec, match_de_bruijn_ref, match_sym_rec, match_sym_ref,
    sym_rec, sym_ref,
};

/// Stable crate identifier used by workspace tooling and diagnostics.
pub const CRATE_NAME: &str = "tlib";

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

/// Converts one tree node to a compact textual atom representation.
///
/// This helper is intentionally shallow (single node, not recursive pretty-print):
/// - `Symbol`/`StringLiteral` -> raw string payload
/// - `Int`/`FloatBits` -> numeric text
/// - `Tag` -> interned tag name when available, empty string otherwise
/// - `Nil`/`Cons` -> `"nil"` / `"cons"`
/// - unknown id -> empty string
#[must_use]
pub fn tree_to_string(arena: &TreeArena, id: TreeId) -> String {
    match arena.kind(id) {
        Some(NodeKind::Symbol(s)) => s.to_string(),
        Some(NodeKind::StringLiteral(s)) => s.to_string(),
        Some(NodeKind::Int(v)) => v.to_string(),
        Some(NodeKind::FloatBits(bits)) => f64::from_bits(*bits).to_string(),
        Some(NodeKind::Tag(t)) => arena.tag_name(*t).unwrap_or("").to_owned(),
        Some(NodeKind::Nil) => "nil".to_owned(),
        Some(NodeKind::Cons) => "cons".to_owned(),
        None => String::new(),
    }
}

/// Extracts an integer atom value from one tree node.
///
/// Returns `None` when the node is not an `Int`.
#[must_use]
pub fn tree_to_int(arena: &TreeArena, id: TreeId) -> Option<i64> {
    match arena.kind(id) {
        Some(NodeKind::Int(v)) => Some(*v),
        _ => None,
    }
}

/// Extracts a floating-point atom value from one tree node.
///
/// Returns `None` when the node is not a `FloatBits`.
#[must_use]
pub fn tree_to_double(arena: &TreeArena, id: TreeId) -> Option<f64> {
    match arena.kind(id) {
        Some(NodeKind::FloatBits(bits)) => Some(f64::from_bits(*bits)),
        _ => None,
    }
}
