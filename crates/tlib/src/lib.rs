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
