#![doc = "Scaffold module for the llvm backend in codegen."]

pub const BACKEND_NAME: &str = "llvm";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
