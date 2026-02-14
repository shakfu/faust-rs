#![doc = "Scaffold crate for errors in the faust-rs workspace."]

pub const CRATE_NAME: &str = "errors";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
