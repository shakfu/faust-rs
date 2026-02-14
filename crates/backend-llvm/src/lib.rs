#![doc = "Scaffold crate for backend-llvm in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-llvm";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
