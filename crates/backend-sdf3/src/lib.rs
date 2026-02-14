#![doc = "Scaffold crate for backend-sdf3 in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-sdf3";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
