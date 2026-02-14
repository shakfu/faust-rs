#![doc = "Scaffold crate for eval in the faust-rs workspace."]

pub const CRATE_NAME: &str = "eval";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
