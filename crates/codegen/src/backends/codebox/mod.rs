#![doc = "Scaffold module for the codebox backend in codegen."]

pub const BACKEND_NAME: &str = "codebox";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
