#![doc = "Scaffold crate for algebra in the faust-rs workspace."]

pub const CRATE_NAME: &str = "algebra";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
