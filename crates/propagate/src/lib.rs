#![doc = "Scaffold crate for propagate in the faust-rs workspace."]

pub const CRATE_NAME: &str = "propagate";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
