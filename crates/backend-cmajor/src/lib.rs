#![doc = "Scaffold crate for backend-cmajor in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-cmajor";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
