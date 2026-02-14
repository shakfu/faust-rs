#![doc = "Scaffold crate for backend-rust in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-rust";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
