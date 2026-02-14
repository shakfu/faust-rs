#![doc = "Scaffold module for the cmajor backend in codegen."]

pub const BACKEND_NAME: &str = "cmajor";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
