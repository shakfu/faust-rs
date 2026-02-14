#![doc = "Scaffold module for the vhdl backend in codegen."]

pub const BACKEND_NAME: &str = "vhdl";

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
