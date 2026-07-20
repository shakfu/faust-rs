//! Final-module vocabulary: the fail-closed failure type carried by the
//! production selector when any stage of the checked vector pipeline, or
//! the terminal `check::verify_final_module` guard, refuses the candidate
//! module. Kept separate from `build.rs` so no `check.rs`/`outputs.rs`/
//! `lifecycle.rs` user needs to import a producer entry-point file just to
//! name this type.

use crate::signal_fir::VectorFallbackReason;
use std::fmt;

/// Failure stage retained by the production selector as an observable fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VectorModuleFailure {
    pub reason: VectorFallbackReason,
    pub detail: String,
}
impl VectorModuleFailure {
    pub(super) fn new(reason: VectorFallbackReason, detail: impl Into<String>) -> Self {
        Self {
            reason,
            detail: detail.into(),
        }
    }
}
impl fmt::Display for VectorModuleFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.reason.code(), self.detail)
    }
}
