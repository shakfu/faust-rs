//! `cranelift` backend scaffold (Phase 1).
//!
//! # Role
//! - Planned native-code backend lowering Faust FIR to machine code via
//!   Cranelift, with a companion `cranelift_dsp` C/C++ export layer.
//!
//! # C++ provenance note
//! - There is no direct C++ `cranelift` backend in upstream Faust.
//! - This backend is a Rust-native extension and follows parity requirements
//!   documented in `porting/cranelift-backend-plan-en.md` for exported runtime
//!   behavior (`llvm_dsp` / `interpreter_dsp`-style API strategy).
//!
//! # Current status
//! - Phase 1 scaffold only: stable backend identifier, options, and typed error
//!   surface are defined.
//! - FIR lowering and JIT/object emission are not implemented yet and return a
//!   typed placeholder error.

use fir::{FirId, FirStore};

/// Stable backend identifier used by tooling and future CLI wiring.
pub const BACKEND_NAME: &str = "cranelift";

#[must_use]
/// Returns the stable backend identifier (`"cranelift"`).
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}

/// Cranelift optimization level (backend-local configuration surface).
///
/// API mapping status: `adapted` (no direct C++ Cranelift backend exists).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CraneliftOptLevel {
    /// Fastest compile time.
    None,
    /// Balanced mode (default planned mode for validation bring-up).
    #[default]
    Speed,
    /// Highest optimization effort.
    SpeedAndSize,
}

/// Options controlling Cranelift backend compilation (scaffold).
///
/// This mirrors the shape planned in `porting/cranelift-backend-plan-en.md`,
/// but no codegen semantics are implemented yet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CraneliftOptions {
    /// Optimization level requested for Cranelift.
    pub opt_level: CraneliftOptLevel,
    /// Optional explicit target triple (string form for portability at the
    /// facade boundary; parsed later by the backend implementation).
    pub target_triple: Option<String>,
    /// Enable deterministic NaN canonicalization when supported.
    pub enable_nan_canonicalization: bool,
    /// Emit backend debug IR dumps once implemented.
    pub debug_ir_dump: bool,
}

/// Stable error codes for the Cranelift backend scaffold and future lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CraneliftBackendErrorCode {
    /// Backend is scaffolded but not implemented yet.
    NotImplemented,
}

impl CraneliftBackendErrorCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::NotImplemented => "FRS-CGEN-CLIF-0001",
        }
    }
}

/// Typed Cranelift backend error (scaffold).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CraneliftBackendError {
    /// Machine-readable stable backend error code.
    pub code: CraneliftBackendErrorCode,
    /// Human-readable message.
    pub message: String,
}

impl CraneliftBackendError {
    fn not_implemented(message: impl Into<String>) -> Self {
        Self {
            code: CraneliftBackendErrorCode::NotImplemented,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CraneliftBackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for CraneliftBackendError {}

/// Compiled JIT module handle (placeholder).
///
/// Planned responsibility (v1+): own compiled code, metadata, and callable
/// trampolines used by Rust and `cranelift-ffi`.
#[derive(Debug, Default)]
pub struct JitDspModule {
    _private: (),
}

/// Compiles a FIR module to a Cranelift JIT module (scaffold placeholder).
///
/// # Current behavior
/// Returns a typed `NotImplemented` error until the backend lowering pipeline is
/// implemented in later phases.
pub fn compile_fir_to_cranelift_jit(
    _store: &FirStore,
    _module: FirId,
    _options: &CraneliftOptions,
) -> Result<JitDspModule, CraneliftBackendError> {
    Err(CraneliftBackendError::not_implemented(
        "Cranelift backend scaffold only: FIR lowering/JIT is not implemented yet",
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        BACKEND_NAME, CraneliftBackendErrorCode, CraneliftOptions, backend_id,
        compile_fir_to_cranelift_jit,
    };

    #[test]
    fn backend_id_is_stable() {
        assert_eq!(BACKEND_NAME, "cranelift");
        assert_eq!(backend_id(), "cranelift");
    }

    #[test]
    fn compile_placeholder_returns_typed_not_implemented() {
        let mut store = fir::FirStore::new();
        let root = {
            let mut b = fir::FirBuilder::new(&mut store);
            b.int32(0)
        };
        let err = compile_fir_to_cranelift_jit(&store, root, &CraneliftOptions::default())
            .expect_err("scaffold backend should not compile FIR yet");
        assert_eq!(err.code, CraneliftBackendErrorCode::NotImplemented);
        assert!(err.to_string().contains("FRS-CGEN-CLIF-0001"));
    }
}
