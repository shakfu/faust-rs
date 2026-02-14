#![doc = "Scaffold module for the cpp backend in codegen."]

pub const BACKEND_NAME: &str = "cpp";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
