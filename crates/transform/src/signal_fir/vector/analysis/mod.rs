//! Vector analysis: execution conditions, dependencies, occurrences,
//! effects, and use tables over the verified prepared forest.
//!
//! # C++ provenance
//! The dependency projection centralizes the dependency rules previously
//! embedded in the Rust Hgraph adapter. Its C++ references are
//! `compiler/Dependencies/DependenciesUtils.cpp::getSignalDependencies` and
//! `compiler/generator/occurrences.cpp::OccMarkup`. One decoded signal shape
//! produces distinct scheduling and occurrence views because the C++ rules
//! intentionally differ for FIR/IIR carriers, tables, `seq`, generators, and
//! clock wrappers.
//!
//! P4.3a adds the C++ `conditionAnnotation` DNF producer and conservative
//! effect decoration without activating either scalar or vector scheduling.
//! Effects in this table describe compute-time behavior. `Gen` remains a
//! lifecycle boundary, so table-initialization effects require a separate
//! decoration before a certificate can establish full lifecycle coverage.
//! The compute-scoped `DecorationCertificate` lives in the adjacent
//! `decoration_verify` module; the production vector plan consumes only
//! independently certified decorations.
//!
//! Development history: P4.2/P4.3a slices of
//! `porting/vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md`.

pub mod conditions;
pub mod dependencies;
pub mod effects;
pub mod uses;

pub use conditions::*;
pub use dependencies::*;
pub use effects::*;
pub use uses::*;

use crate::clk_env::ClkEnvMap;
use crate::signal_prepare::VerifiedPreparedSignals;
use signals::SigId;
use std::fmt;

/// Canonical P4.3a result coupling real execution conditions with decorated
/// signal-use facts from the same prepared forest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorSignalAnalysis {
    pub conditions: ExecutionConditionTable,
    pub uses: SignalUseTable,
}
/// Typed P4.2 analysis errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalysisError {
    /// A required list-shaped signal payload was malformed, or legacy `SIGREC`
    /// reached the symbolic-recursion-only analysis boundary.
    Malformed { sig: SigId, detail: String },
    /// The verified preparation map unexpectedly lacks a reachable signal type.
    MissingType { sig: SigId },
    /// Clock inference did not annotate a reachable signal.
    MissingClock { sig: SigId },
    /// A projection index was negative.
    InvalidRecursiveProjection {
        sig: SigId,
        index: i32,
        group: SigId,
    },
    /// A delay amount type violates the bounded nonnegative C++ contract.
    InvalidDelayInterval {
        sig: SigId,
        amount: SigId,
        detail: String,
    },
}
impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { sig, detail } => {
                write!(f, "malformed signal {}: {detail}", sig.as_u32())
            }
            Self::MissingType { sig } => write!(f, "signal {} has no prepared type", sig.as_u32()),
            Self::MissingClock { sig } => write!(
                f,
                "signal {} has no inferred clock environment",
                sig.as_u32()
            ),
            Self::InvalidRecursiveProjection { sig, index, group } => write!(
                f,
                "projection {} has invalid negative index {index} for group {}",
                sig.as_u32(),
                group.as_u32()
            ),
            Self::InvalidDelayInterval {
                sig,
                amount,
                detail,
            } => write!(
                f,
                "signal {} has invalid delay amount {}: {detail}",
                sig.as_u32(),
                amount.as_u32()
            ),
        }
    }
}
impl std::error::Error for AnalysisError {}
/// Builds the canonical P4.3a condition/effect analysis for a verified forest.
pub fn analyze_vector_signals(
    prepared: &VerifiedPreparedSignals,
    clk_envs: &ClkEnvMap,
) -> Result<VectorSignalAnalysis, AnalysisError> {
    let timing_enabled = std::env::var_os("FAUST_RS_VECTOR_TIMING").is_some();
    let started = std::time::Instant::now();
    let conditions = ExecutionConditionTable::build(prepared)?;
    if timing_enabled {
        eprintln!(
            "[vector-analysis-stage] conditions: {:.3}s",
            started.elapsed().as_secs_f64()
        );
    }
    let started = std::time::Instant::now();
    let uses = analyze_signal_uses(prepared, clk_envs, &conditions)?;
    if timing_enabled {
        eprintln!(
            "[vector-analysis-stage] uses: {:.3}s",
            started.elapsed().as_secs_f64()
        );
    }
    Ok(VectorSignalAnalysis { conditions, uses })
}

#[cfg(test)]
mod tests;
