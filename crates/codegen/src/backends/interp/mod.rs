#![doc = "Scaffold module for the interp backend in codegen."]

pub const BACKEND_NAME: &str = "interp";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
