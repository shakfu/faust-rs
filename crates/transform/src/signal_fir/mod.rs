//! Experimental signal->FIR fast-lane (Step 2A/2B/2C/2D/2E/2F/2G/2H slices).
//!
//! # Status
//! This module currently provides an **executable base slice**:
//! - contract validation (`Step 1A`),
//! - lowering for `SIGINPUT`, numeric constants, `SIGBINOP`, and `SIGOUTPUT`
//!   passthrough (`Step 2A`),
//! - core math and control/state bootstrap nodes (`Step 2B`),
//! - explicit state lowering for `delay`-family nodes, including fixed-size
//!   circular FIR delay lines for constant `SIGDELAY` amounts (`Step 2C` slice),
//! - first breadth coverage for extended primitives, waveform/table/UI families
//!   (`Step 2D`),
//! - first shim-reduction pass replacing several `frs_*` calls with native FIR
//!   lowering (`Step 2E`),
//! - critical shim elimination (`Step 2F`): no `frs_*` calls remain in fast-lane
//!   generated C++,
//! - first FIR-native table lowering (`Step 2G`) for
//!   `SIGWAVEFORM` / `SIGRDTBL` / `SIGWRTBL`,
//! - non-trivial table slice (`Step 2H`) for `SIGWRTBL(size, gen(..), ..)` with
//!   constant size and deterministic generator expansion.
//! - pre-lowering staging (`Preparation Step 1`): clone the output forest into a
//!   private arena and run forest-wide `de_bruijn_to_sym` before FIR emission.
//! - prepared typing/promotion (`Preparation Step 2/3/4`): consume the reduced
//!   `signal_prepare` type map so FIR lowering keeps integer delay/recursion/table
//!   carriers instead of defaulting every internal value to `real_ty`.
//! - **RAD Phase B3**: tape-free TBPTT(BS, BS) backward sweep for
//!   `SigBlockReverseAD` carriers whose body signals are trivially
//!   reverse-evaluable (no `Delay1`/stateful operands in Mul/Div/unary rules).
//! - **RAD Phase B4**: per-sample forward tape for `SigBlockReverseAD`
//!   carriers whose body contains non-trivially-re-evaluable operands in
//!   Mul/Div/unary backward rules (e.g. `x' * x`).  Forward values are stored
//!   in `fBraTapeN` struct arrays during the forward loop and loaded during the
//!   reverse loop via `load_bra_fwd_value`.
//! - **RAD Phase B5**: extended backward rules covering all remaining
//!   differentiable signal forms: `Delay(c, x)` (circular carry buffer of size
//!   `c` in struct field `fBraDelayCarryN`), `Prefix(init, x)` (scalar carry
//!   with a boundary condition at sample 0), all smooth unary ops
//!   (`Tan`, `Asin`, `Acos`, `Atan`, `Log10`, `Abs`, `Pow`, `Atan2`), binary
//!   `Min`/`Max` (subgradient via `Select2`), `Floor`/`Ceil`/`Rint`/`Round`
//!   (zero gradient), and discrete `BinOp` variants (zero gradient).
//!
//! Current `Step 2H` scope still excludes complex generator forms depending on
//! runtime context/loop variables; those are reported as typed
//! `UnsupportedSignalNode` errors.
//!
//! General `SIGDELAY` parity remains intentionally partial: the fast-lane now
//! supports constant integer delay amounts through fixed-size circular buffers,
//! and variable delays where the amount comes from a UI control with a bounded
//! interval (slider/numentry). Delays with unbounded intervals are currently
//! rejected as unsupported.
//!
//! Other signal families still return typed `FRS-SFIR-*` errors until the
//! remaining lowering slices are implemented.
//!
//! # Crate boundary contract
//! - `transform` owns signal->FIR lowering entrypoints.
//! - `fir` owns FIR node model, builder, and matcher.
//! - `codegen` consumes resulting FIR modules.
//! - `compiler` chooses whether to route requests to this fast-lane.

mod block_reverse_ad;
mod cse;
mod delay;
mod error;
mod module;
mod placement;
mod planner;
mod recursion;
mod siggen;

pub use error::{SignalFirError, SignalFirErrorCode};

use fir::{FirId, FirStore, FirType};
use signals::SigId;
use tlib::TreeArena;
use ui::UiProgram;

use crate::signal_prepare::prepare_signals_for_fir_verified;

/// Internal DSP computation precision used when lowering signals to FIR.
///
/// Controls the type of internal state variables, arithmetic results, math
/// function signatures, waveform table element types, and real-constant nodes
/// in the generated FIR module.
///
/// **External interface types are not affected**: audio buffer samples
/// (`FAUSTFLOAT**` in `compute`) and UI zone variables (sliders, bargraphs,
/// buttons) always use `FirType::FaustFloat` regardless of this setting.
///
/// Corresponds to Faust's `-double` compilation flag and `gFLoatSize`:
/// - `Float32` → C++ `float` (default),
/// - `Float64` → C++ `double`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RealType {
    /// 32-bit single-precision float (`float` in C++). Default.
    #[default]
    Float32,
    /// 64-bit double-precision float (`double` in C++).
    Float64,
}

impl RealType {
    /// Returns the [`FirType`] that represents this precision in FIR lowering.
    #[must_use]
    pub fn as_fir_type(self) -> FirType {
        match self {
            Self::Float32 => FirType::Float32,
            Self::Float64 => FirType::Float64,
        }
    }
}

/// Configuration options for [`compile_signals_to_fir_fastlane_with_ui`].
///
/// These options describe the externally visible module contract.
/// Resource planning and lowering policies stay internal to the fast-lane until
/// more slices are promoted to stable configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalFirOptions {
    /// FIR module name to emit.
    pub module_name: String,
    /// Internal DSP computation precision (default: [`RealType::Float32`]).
    ///
    /// Controls the FIR type used for internal arithmetic, state variables,
    /// math calls, waveform table elements, and real constants.
    ///
    /// External interface types (`FaustFloat`) are **not** affected: audio
    /// buffers (`FAUSTFLOAT** inputs/outputs` in `compute`) and UI zone
    /// variables (sliders, bargraphs, buttons) always use `FaustFloat`.
    ///
    /// Implicit casts between the internal real type and `FaustFloat` are
    /// emitted automatically at the DSP boundary (input sample load and output
    /// sample store) and at UI zone reads/writes.
    pub real_type: RealType,
    /// Maximum delay threshold before switching from shift/copy to a circular
    /// ring buffer.
    ///
    /// Mirrors Faust's `-mcd N` option as observed in the C++ backend:
    /// standalone `Delay1` stays special-cased, and general shifted delay
    /// lines are used for delays `< max_copy_delay`. Delays at or above the
    /// threshold switch to ring-buffer strategies. Default: 16.
    pub max_copy_delay: u32,
    /// Delay threshold at which the if-based wrapping strategy replaces the
    /// default power-of-two circular buffer.
    ///
    /// Mirrors Faust's `-dlt N` option as observed in the C++ backend:
    /// delays `< delay_line_threshold` use the circular-pow2 strategy, while
    /// delays at or above the threshold use an exact-size buffer with a
    /// per-line counter variable. Default: `u32::MAX` (disabled; all delays at
    /// or above `max_copy_delay` use circular-pow2).
    pub delay_line_threshold: u32,
}

impl Default for SignalFirOptions {
    fn default() -> Self {
        Self {
            module_name: "mydsp".to_owned(),
            real_type: RealType::Float32,
            max_copy_delay: 16,
            delay_line_threshold: u32::MAX,
        }
    }
}

/// Output bundle produced by [`compile_signals_to_fir_fastlane_with_ui`].
///
/// The fast-lane returns ownership of the FIR store together with the module
/// root so downstream backends can keep using normal `fir` builder/matcher APIs
/// without relying on hidden global state.
#[derive(Debug)]
pub struct SignalFirOutput {
    /// FIR storage arena.
    pub store: FirStore,
    /// Root node id of the generated FIR module.
    pub module: FirId,
}

/// Compiles propagated signals plus canonical grouped UI into a FIR module.
///
/// This is the grouped-UI-aware fast-lane entry point used by the compiler
/// facade once `propagate` owns explicit `UiProgram` output.
///
/// Callers that intentionally compile UI-free signal forests must pass an
/// explicit placeholder [`UiProgram`] such as [`UiProgram::empty()`] or a
/// root-only synthetic program constructed by the owning integration layer.
///
/// # Current behavior (Step 2A/2B/2C/2D/2E/2F/2G/2H)
/// - validates options and top-level signal/arity contract,
/// - builds a deterministic planning snapshot,
/// - prepares and verifies the whole output forest in a private staging arena,
/// - lowers one executable bootstrap signal slice to FIR using the prepared
///   reduced type annotations for state/table/result type selection.
///
/// # Errors
/// Returns [`SignalFirError`] when options are invalid or the top-level
/// signal/arity contract is inconsistent.
///
/// # Ownership contract
/// - `signals` must already be the propagated DSP outputs for the same source
///   program that produced `ui`,
/// - `ui` is the sole source of truth for grouped layout, labels, and UI
///   metadata,
/// - signal leaf widgets are expected to carry only stable `ControlId`
///   references back into `ui`.
pub fn compile_signals_to_fir_fastlane_with_ui(
    _arena: &TreeArena,
    signals: &[SigId],
    num_inputs: usize,
    num_outputs: usize,
    ui: &UiProgram,
    options: &SignalFirOptions,
) -> Result<SignalFirOutput, SignalFirError> {
    let plan = planner::plan_signals(signals, num_inputs, num_outputs, options)?;
    let prepared = prepare_signals_for_fir_verified(_arena, signals, ui).map_err(|err| {
        SignalFirError::new(
            SignalFirErrorCode::UnsupportedSignalNode,
            format!("signal preparation failed: {err}"),
        )
    })?;
    module::build_module(
        &plan,
        options.module_name.as_str(),
        prepared.arena(),
        prepared.outputs(),
        ui,
        prepared.types_map(),
        prepared.sig_types_map(),
        options.real_type.as_fir_type(),
        options.max_copy_delay,
        options.delay_line_threshold,
    )
}

#[cfg(test)]
mod tests;
