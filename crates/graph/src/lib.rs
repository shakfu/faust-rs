#![doc = "Scaffold crate for graph in the faust-rs workspace."]

pub const CRATE_NAME: &str = "graph";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
