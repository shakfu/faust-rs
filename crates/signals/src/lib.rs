#![doc = "Scaffold crate for signals in the faust-rs workspace."]

pub const CRATE_NAME: &str = "signals";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
