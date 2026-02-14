#![doc = "Scaffold module for the rust backend in codegen."]

pub const BACKEND_NAME: &str = "rust";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
