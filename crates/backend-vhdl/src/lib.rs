#![doc = "Scaffold crate for backend-vhdl in the faust-rs workspace."]

pub const CRATE_NAME: &str = "backend-vhdl";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
