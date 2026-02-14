#![doc = "Code generation crate for the faust-rs workspace."]

pub mod backends;

pub const CRATE_NAME: &str = "codegen";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
