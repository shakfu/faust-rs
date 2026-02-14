#![doc = "Scaffold crate for interval in the faust-rs workspace."]

pub const CRATE_NAME: &str = "interval";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
