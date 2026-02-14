#![doc = "Scaffold crate for utils in the faust-rs workspace."]

pub const CRATE_NAME: &str = "utils";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
