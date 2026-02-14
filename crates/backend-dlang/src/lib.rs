#![doc = "Scaffold crate for backend-dlang in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-dlang";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
