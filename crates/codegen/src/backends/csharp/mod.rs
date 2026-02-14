#![doc = "Scaffold module for the csharp backend in codegen."]

pub const BACKEND_NAME: &str = "csharp";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
