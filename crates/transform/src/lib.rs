#![doc = "Scaffold crate for transform in the faust-rs workspace."]

pub mod signal_fir;

pub const CRATE_NAME: &str = "transform";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
