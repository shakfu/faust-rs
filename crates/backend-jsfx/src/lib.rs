#![doc = "Scaffold crate for backend-jsfx in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-jsfx";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
