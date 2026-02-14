#![doc = "Scaffold module for the jax backend in codegen."]

pub const BACKEND_NAME: &str = "jax";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
