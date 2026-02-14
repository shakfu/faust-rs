#![doc = "Scaffold crate for backend-interp in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-interp";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
