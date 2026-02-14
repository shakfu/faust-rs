#![doc = "Scaffold crate for doc in the faust-rs workspace."]

pub const CRATE_NAME: &str = "doc";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
