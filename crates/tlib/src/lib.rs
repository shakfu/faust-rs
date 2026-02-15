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

mod arena;
mod property;

pub use arena::{ChildList, NodeKind, TreeArena, TreeId, TreeNode};
pub use property::{PropertyKey, PropertyStore};

pub const CRATE_NAME: &str = "tlib";

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
