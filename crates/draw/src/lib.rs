//! Graph/diagram rendering crate placeholder.
//!
//! # Intended role
//! - Host visualization helpers for internal IRs (boxes/signals/FIR), including
//!   deterministic debug views used by parity investigations.
//! - Provide reusable drawing backends for tooling and docs generation.
//!
//! # Current status
//! - Scaffold only. No graph rendering API is exposed yet.
//!
//! # API mapping status
//! - `crate_id()` is `adapted` utility metadata (no direct C++ counterpart).

pub const CRATE_NAME: &str = "draw";

#[must_use]
/// Returns the stable crate identifier.
pub fn crate_id() -> &'static str {
    CRATE_NAME
}
