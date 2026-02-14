#![doc = "Scaffold crate for boxes in the faust-rs workspace."]

pub const CRATE_NAME: &str = "boxes";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
