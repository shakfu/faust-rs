#![doc = "Scaffold crate for backend-csharp in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-csharp";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
