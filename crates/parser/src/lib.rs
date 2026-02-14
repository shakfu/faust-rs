#![doc = "Scaffold crate for parser in the faust-rs workspace."]

pub const CRATE_NAME: &str = "parser";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
