//! DSP instance lifecycle and execution entry points (scaffold).
//!
//! # Planned behavior
//! - Instance creation/deletion from a Cranelift factory.
//! - `init`, `compute`, `buildUserInterface`, and `metadata` entry points with
//!   callback dispatch parity-target behavior.

/// Instance scaffold status string.
#[must_use]
pub fn instance_status() -> &'static str {
    "cranelift-ffi instance scaffold"
}

#[cfg(test)]
mod tests {
    use super::instance_status;

    #[test]
    fn instance_scaffold_status_is_stable() {
        assert_eq!(instance_status(), "cranelift-ffi instance scaffold");
    }
}
