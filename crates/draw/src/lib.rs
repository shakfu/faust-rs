#![doc = "Scaffold crate for draw in the faust-rs workspace."]

pub const CRATE_NAME: &str = "draw";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
