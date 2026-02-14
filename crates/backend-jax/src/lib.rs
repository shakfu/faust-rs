#![doc = "Scaffold crate for backend-jax in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-jax";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
