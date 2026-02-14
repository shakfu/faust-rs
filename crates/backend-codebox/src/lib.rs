#![doc = "Scaffold crate for backend-codebox in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-codebox";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
