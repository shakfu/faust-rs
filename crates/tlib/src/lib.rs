#![doc = "Tree/library core for Faust structures in the faust-rs workspace."]

mod arena;
mod property;

pub use arena::{NodeKind, TreeArena, TreeId, TreeNode};
pub use property::PropertyStore;

pub const CRATE_NAME: &str = "tlib";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
