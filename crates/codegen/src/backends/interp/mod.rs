//! `interp` backend module placeholder.
//!
//! # Intended role
//! - Host `interp` code generation entry points from FIR once this backend is
//!   scheduled in the parity roadmap.
//!
//! # Current status
//! - Scaffold only: no emitter implementation yet; stable backend identifier
//!   is kept for tooling/report wiring.

pub const BACKEND_NAME: &str = "interp";

#[must_use]
/// Returns the stable backend identifier.
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
