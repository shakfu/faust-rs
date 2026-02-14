#![doc = "Scaffold crate for backend-cpp in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-cpp";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
