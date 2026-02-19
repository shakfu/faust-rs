//! Interval-analysis crate placeholder.
//!
//! # Source provenance (C++)
//! - Interval/type analyses are historically implemented in
//!   `compiler/interval/*` and related typing passes.
//!
//! # Intended role in pipeline
//! - Compute value bounds and domain constraints over normalized signal forms.
//! - Provide deterministic interval facts to downstream transforms/backends.
//!
//! # Current status
//! - Scaffold only. No interval lattice or transfer-function API exposed yet.
//!
//! # API mapping status
//! - `crate_id()` is `adapted` utility metadata (no direct C++ counterpart).

pub const CRATE_NAME: &str = "interval";

#[must_use]
/// Returns the stable crate identifier.
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
