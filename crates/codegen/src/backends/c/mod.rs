#![doc = "Scaffold module for the c backend in codegen."]

pub const BACKEND_NAME: &str = "c";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
