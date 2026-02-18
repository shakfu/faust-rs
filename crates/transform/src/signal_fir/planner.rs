//! Resource-planning front slice for the signal->FIR fast-lane.
//!
//! Step 1A intentionally keeps this planner minimal and deterministic:
//! it only validates static contract inputs and records basic shape metadata.

use super::error::{SignalFirError, SignalFirErrorCode};
use super::SignalFirOptions;
use signals::SigId;

/// Minimal deterministic planning output for Step 1A.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalFirPlan {
    /// Number of output signals requested for compilation.
    pub signal_count: usize,
    /// Number of DSP input channels.
    pub num_inputs: usize,
    /// Number of DSP output channels.
    pub num_outputs: usize,
}

/// Validates fast-lane inputs and produces a minimal planning snapshot.
///
/// This is intentionally conservative in Step 1A: semantics/resource details
/// (delay lines/tables/UI) are added in Step 2+.
pub fn plan_signals(
    signals: &[SigId],
    num_inputs: usize,
    num_outputs: usize,
    options: &SignalFirOptions,
) -> Result<SignalFirPlan, SignalFirError> {
    if options.module_name.trim().is_empty() {
        return Err(SignalFirError::new(
            SignalFirErrorCode::InvalidOptions,
            "module_name must not be empty",
        ));
    }
    if signals.is_empty() {
        return Err(SignalFirError::new(
            SignalFirErrorCode::EmptySignalList,
            "at least one signal is required",
        ));
    }
    if num_outputs != signals.len() {
        return Err(SignalFirError::new(
            SignalFirErrorCode::OutputArityMismatch,
            format!(
                "num_outputs ({num_outputs}) must equal signal count ({}) in Step 1A",
                signals.len()
            ),
        ));
    }

    Ok(SignalFirPlan {
        signal_count: signals.len(),
        num_inputs,
        num_outputs,
    })
}
