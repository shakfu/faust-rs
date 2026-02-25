//! UI/meta callback dispatch helpers (scaffold).
//!
//! This module will host callback trampoline helpers for `UIGlue` and
//! `MetaGlue` once the Cranelift backend emits callable `buildUserInterface`
//! and `metadata` functions.

/// UI/meta scaffold status string.
#[must_use]
pub fn ui_status() -> &'static str {
    "cranelift-ffi ui scaffold"
}

#[cfg(test)]
mod tests {
    use super::ui_status;

    #[test]
    fn ui_scaffold_status_is_stable() {
        assert_eq!(ui_status(), "cranelift-ffi ui scaffold");
    }
}
