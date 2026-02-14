#![doc = "Scaffold module for the wasm backend in codegen."]

pub const BACKEND_NAME: &str = "wasm";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
