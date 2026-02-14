#![doc = "Scaffold crate for tlib in the faust-rs workspace."]

pub const CRATE_NAME: &str = "tlib";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
