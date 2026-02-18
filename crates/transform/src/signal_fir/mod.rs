//! Experimental signal->FIR fast-lane (Step 1A contract).
//!
//! # Status
//! This module currently provides a **contract-only skeleton**:
//! it validates entry arguments and emits a minimal FIR `Module` root.
//! Signal semantics/resource planning parity is intentionally deferred to
//! Step 2+ of the fast-lane plan.
//!
//! # Crate boundary contract
//! - `transform` owns signal->FIR lowering entrypoints.
//! - `fir` owns FIR node model, builder, and matcher.
//! - `codegen` consumes resulting FIR modules.
//! - `compiler` chooses whether to route requests to this fast-lane.

mod error;
mod module;
mod planner;

pub use error::{SignalFirError, SignalFirErrorCode};

use fir::{FirId, FirStore};
use signals::SigId;
use tlib::TreeArena;

/// Options for `compile_signals_to_fir_fastlane`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalFirOptions {
    /// FIR module name to emit.
    pub module_name: String,
    /// Reserved for Step 2+ strictness/profile toggles.
    pub strict_mode: bool,
}

impl Default for SignalFirOptions {
    fn default() -> Self {
        Self {
            module_name: "mydsp".to_owned(),
            strict_mode: true,
        }
    }
}

/// Output package of the fast-lane compiler.
#[derive(Debug)]
pub struct SignalFirOutput {
    /// FIR storage arena.
    pub store: FirStore,
    /// Root node id of the generated FIR module.
    pub module: FirId,
}

/// Compiles propagated signals into a FIR module using the experimental fast-lane.
///
/// # Current behavior (Step 1A)
/// - validates options and top-level signal/arity contract,
/// - emits a minimal valid FIR module with a placeholder `compute`.
///
/// # Errors
/// Returns [`SignalFirError`] when options are invalid or the top-level
/// signal/arity contract is inconsistent.
pub fn compile_signals_to_fir_fastlane(
    _arena: &TreeArena,
    signals: &[SigId],
    num_inputs: usize,
    num_outputs: usize,
    options: &SignalFirOptions,
) -> Result<SignalFirOutput, SignalFirError> {
    let plan = planner::plan_signals(signals, num_inputs, num_outputs, options)?;
    Ok(module::build_module(&plan, options.module_name.as_str()))
}

#[cfg(test)]
mod tests {
    use super::{
        SignalFirErrorCode, SignalFirOptions, compile_signals_to_fir_fastlane,
    };
    use fir::{FirMatch, match_fir};
    use signals::SigBuilder;
    use tlib::TreeArena;

    #[test]
    fn non_empty_signal_list_returns_fir_module_root() {
        let mut arena = TreeArena::new();
        let sig0 = {
            let mut b = SigBuilder::new(&mut arena);
            b.input(0)
        };
        let out = compile_signals_to_fir_fastlane(
            &arena,
            &[sig0],
            1,
            1,
            &SignalFirOptions::default(),
        )
        .expect("Step 1A should emit a module for valid top-level inputs");

        assert!(matches!(match_fir(&out.store, out.module), FirMatch::Module { .. }));
    }

    #[test]
    fn invalid_options_return_typed_error_code() {
        let mut arena = TreeArena::new();
        let sig0 = {
            let mut b = SigBuilder::new(&mut arena);
            b.input(0)
        };
        let err = compile_signals_to_fir_fastlane(
            &arena,
            &[sig0],
            1,
            1,
            &SignalFirOptions {
                module_name: "".to_owned(),
                strict_mode: true,
            },
        )
        .expect_err("empty module name should fail option validation");

        assert_eq!(err.code(), SignalFirErrorCode::InvalidOptions);
        assert_eq!(err.code().as_str(), "FRS-SFIR-0001");
    }
}
