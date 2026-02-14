#![doc = "Scaffold crate for codegen in the faust-rs workspace."]

pub const CRATE_NAME: &str = "codegen";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
