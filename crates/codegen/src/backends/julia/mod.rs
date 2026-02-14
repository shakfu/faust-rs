#![doc = "Scaffold module for the julia backend in codegen."]

pub const BACKEND_NAME: &str = "julia";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
