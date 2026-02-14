#![doc = "Scaffold module for the jsfx backend in codegen."]

pub const BACKEND_NAME: &str = "jsfx";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
