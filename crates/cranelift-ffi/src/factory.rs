//! Factory lifecycle and source-compilation entry points (scaffold).
//!
//! # Planned behavior
//! - Compile from file/string through the `compiler` facade and the Cranelift
//!   backend (`codegen::backends::cranelift`).
//! - Expose cache-aware factory creation/deletion APIs with parity-target
//!   semantics matching `llvm_dsp` / `interpreter_dsp`.

/// Factory scaffold status string.
#[must_use]
pub fn factory_status() -> &'static str {
    "cranelift-ffi factory scaffold"
}

#[cfg(test)]
mod tests {
    use super::factory_status;

    #[test]
    fn factory_scaffold_status_is_stable() {
        assert_eq!(factory_status(), "cranelift-ffi factory scaffold");
    }
}
