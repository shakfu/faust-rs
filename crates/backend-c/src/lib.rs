#![doc = "Scaffold crate for backend-c in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-c";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
