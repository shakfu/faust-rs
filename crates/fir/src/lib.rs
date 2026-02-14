#![doc = "Scaffold crate for fir in the faust-rs workspace."]

pub const CRATE_NAME: &str = "fir";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
