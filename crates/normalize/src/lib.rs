#![doc = "Scaffold crate for normalize in the faust-rs workspace."]

pub const CRATE_NAME: &str = "normalize";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
