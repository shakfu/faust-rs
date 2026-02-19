//! Signal/box normalization crate placeholder.
//!
//! # Source provenance (C++)
//! - Normalization and canonicalization logic is historically spread across
//!   `compiler/normalize/*` and pass-specific simplification code.
//!
//! # Intended role in pipeline
//! - Enforce canonical IR shapes before typing/interval/transform passes.
//! - Centralize rewrite phases that must be deterministic for golden parity.
//!
//! # Current status
//! - Scaffold only. No public normalization pipeline is stabilized yet.
//!
//! # API mapping status
//! - `crate_id()` is `adapted` utility metadata (no direct C++ counterpart).

pub const CRATE_NAME: &str = "normalize";

#[must_use]
/// Returns the stable crate identifier.
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
