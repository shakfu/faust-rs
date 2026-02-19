//! Code generation crate for backend emission from FIR.
//!
//! # Source provenance (C++)
//! - `compiler/generator/*`
//! - `compiler/generator/fir/*`
//! - backend-specific emitters under `compiler/generator/<backend>/`
//!
//! # Role in pipeline
//! - Consumes FIR (`fir::FirStore` + FIR roots) produced by compile lanes.
//! - Emits target-language source text for supported backends.
//! - Centralizes backend option structs and signature validation helpers.
//!
//! # Public surface
//! - [`backends`] exposes backend modules.
//! - [`fixtures`] provides shared FIR fixtures used by backend tests and parity
//!   checks.
//!
//! # Current status
//! - C/C++ backends are implemented for the active module-first slice.
//! - Other backend modules are scaffolded with stable identifiers and explicit
//!   placeholders for future parity work.
//!
//! # API mapping status
//! - Backend option structs and generation entry points are `adapted` APIs:
//!   they preserve C++ behavior but use Rust ownership/error types.

pub mod backends;
pub mod fixtures;

pub const CRATE_NAME: &str = "codegen";

#[must_use]
/// Returns the stable crate identifier.
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
