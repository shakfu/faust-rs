#![doc = "Scaffold crate for backend-julia in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-julia";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
