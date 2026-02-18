//! Experimental signal->FIR fast-lane (Step 2A slice).
//!
//! # Status
//! This module currently provides an **executable base slice**:
//! - contract validation (`Step 1A`),
//! - lowering for `SIGINPUT`, numeric constants, `SIGBINOP`, and `SIGOUTPUT`
//!   passthrough (`Step 2A`).
//!
//! Other signal families still return typed `FRS-SFIR-*` errors until the
//! remaining lowering slices are implemented.
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
/// # Current behavior (Step 2A)
/// - validates options and top-level signal/arity contract,
/// - lowers one executable base signal slice to FIR.
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
    module::build_module(&plan, options.module_name.as_str(), _arena, signals)
}

#[cfg(test)]
mod tests {
    use super::{SignalFirErrorCode, SignalFirOptions, compile_signals_to_fir_fastlane};
    use fir::{FirBinOp, FirMatch, match_fir};
    use signals::{BinOp, SigBuilder};
    use tlib::TreeArena;

    #[test]
    fn non_empty_signal_list_returns_fir_module_root() {
        let mut arena = TreeArena::new();
        let sig0 = {
            let mut b = SigBuilder::new(&mut arena);
            let i0 = b.input(0);
            let c0 = b.real(0.5);
            b.binop(BinOp::Mul, i0, c0)
        };
        let out =
            compile_signals_to_fir_fastlane(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
                .expect("Step 1A should emit a module for valid top-level inputs");

        assert!(matches!(
            match_fir(&out.store, out.module),
            FirMatch::Module { .. }
        ));
        let FirMatch::Module { declarations, .. } = match_fir(&out.store, out.module) else {
            panic!("module root expected");
        };
        let FirMatch::Block(decls) = match_fir(&out.store, declarations) else {
            panic!("module declarations block expected");
        };
        let compute = decls
            .iter()
            .copied()
            .find(|id| matches!(match_fir(&out.store, *id), FirMatch::DeclareFun { .. }))
            .expect("compute declaration expected");
        let FirMatch::DeclareFun { body, .. } = match_fir(&out.store, compute) else {
            panic!("declare fun expected");
        };
        let FirMatch::Block(stmts) = match_fir(&out.store, body) else {
            panic!("compute block expected");
        };
        let drop_value = stmts
            .iter()
            .find_map(|id| match match_fir(&out.store, *id) {
                FirMatch::Drop(value) => Some(value),
                _ => None,
            })
            .expect("compute should include one output drop");
        assert!(matches!(
            match_fir(&out.store, drop_value),
            FirMatch::BinOp {
                op: FirBinOp::Mul,
                ..
            }
        ));
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

    #[test]
    fn unsupported_signal_family_returns_typed_error_code() {
        let mut arena = TreeArena::new();
        let sig0 = {
            let mut b = SigBuilder::new(&mut arena);
            let i0 = b.input(0);
            b.delay1(i0)
        };
        let err =
            compile_signals_to_fir_fastlane(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
                .expect_err("delay1 is outside Step 2A lowering slice");

        assert_eq!(err.code(), SignalFirErrorCode::UnsupportedSignalNode);
        assert_eq!(err.code().as_str(), "FRS-SFIR-0004");
    }

    #[test]
    fn input_index_out_of_range_returns_typed_error_code() {
        let mut arena = TreeArena::new();
        let sig0 = {
            let mut b = SigBuilder::new(&mut arena);
            b.input(1)
        };
        let err =
            compile_signals_to_fir_fastlane(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
                .expect_err("input(1) is invalid when num_inputs=1");

        assert_eq!(err.code(), SignalFirErrorCode::InputIndexOutOfRange);
        assert_eq!(err.code().as_str(), "FRS-SFIR-0006");
    }

    #[test]
    fn pow_min_max_and_unary_math_lower_to_fir_fun_calls() {
        let mut arena = TreeArena::new();
        let sig0 = {
            let mut b = SigBuilder::new(&mut arena);
            let i0 = b.input(0);
            let s0 = b.sin(i0);
            let c0 = b.real(0.25);
            let c1 = b.real(0.5);
            let mx = b.max(c0, c1);
            b.pow(s0, mx)
        };
        let out =
            compile_signals_to_fir_fastlane(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
                .expect("pow/min/max/unary should be supported in Step 2B.1");

        let FirMatch::Module { declarations, .. } = match_fir(&out.store, out.module) else {
            panic!("module root expected");
        };
        let FirMatch::Block(decls) = match_fir(&out.store, declarations) else {
            panic!("module declarations block expected");
        };
        let compute = decls
            .iter()
            .copied()
            .find(|id| matches!(match_fir(&out.store, *id), FirMatch::DeclareFun { .. }))
            .expect("compute declaration expected");
        let FirMatch::DeclareFun { body, .. } = match_fir(&out.store, compute) else {
            panic!("declare fun expected");
        };
        let FirMatch::Block(stmts) = match_fir(&out.store, body) else {
            panic!("compute block expected");
        };
        let drop_value = stmts
            .iter()
            .find_map(|id| match match_fir(&out.store, *id) {
                FirMatch::Drop(value) => Some(value),
                _ => None,
            })
            .expect("compute should include one output drop");
        let FirMatch::FunCall { name, args, .. } = match_fir(&out.store, drop_value) else {
            panic!("top-level pow should lower to FIR fun call");
        };
        assert_eq!(name, "std::pow");
        assert_eq!(args.len(), 2);

        let FirMatch::FunCall { name: lhs_name, .. } = match_fir(&out.store, args[0]) else {
            panic!("lhs should lower to unary fun call");
        };
        assert_eq!(lhs_name, "std::sin");
        let FirMatch::FunCall { name: rhs_name, .. } = match_fir(&out.store, args[1]) else {
            panic!("rhs should lower to min/max fun call");
        };
        assert_eq!(rhs_name, "std::fmax");
    }
}
