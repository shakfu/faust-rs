#![doc = "Scaffold module for the sdf3 backend in codegen."]

pub const BACKEND_NAME: &str = "sdf3";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
