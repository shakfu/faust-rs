//! Shared utility crate placeholder.
//!
//! # Intended role
//! - Provide small, dependency-light helpers reused across crates (formatting,
//!   path helpers, stable ordering helpers, etc.).
//! - Keep cross-cutting helpers out of domain crates to preserve boundaries.
//!
//! # Current status
//! - Scaffold only. No shared helper API is stabilized yet.
//!
//! # API mapping status
//! - `crate_id()` is `adapted` utility metadata (no direct C++ counterpart).

pub const CRATE_NAME: &str = "utils";

#[must_use]
/// Returns the stable crate identifier.
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
