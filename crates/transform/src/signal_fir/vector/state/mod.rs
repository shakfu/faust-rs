//! Verified state-transition plans for vector delays and recursion
//! (`pre/exec/post` phases, C++ copy/ring storage words).
//!
//! # C++ provenance and adaptation
//! The storage equations mirror
//! `DAGInstructionsCompiler::generateDlineLoop` and
//! `DAGInstructionsCompiler::generateDelayAccess` in
//! `compiler/generator/dag_instructions_compiler.cpp`: short delay lines use
//! `_tmp`/`_perm` storage with a four-sample-rounded history, while long delay
//! lines use a power-of-two ring plus `_idx`/`_idx_save`. `CodeLoop`'s
//! `fPreCode`, `fExecCode`, and `fPostCode` become explicit phase actions.
//!
//! Recursion follows the simultaneous `RecStep` rule from the vector port
//! plan: every projection of one symbolic group is owned by one serial
//! `LoopKind::Recursive` loop and advances once per sample. Prefix cells and
//! cycling waveform indexes carry explicit lifecycle and update transitions;
//! zero-history delay effects are explicit no-ops. The artifact is derived
//! from the verified prepared forest, checked P4.3b decorations, and the
//! checked P4.4 vector plan; it does not inspect FIR statements. P6.6 composes that plan
//! with the checked P6.2 clock artifact: state local to an OD/US/DS island uses
//! one persistent ring cursor per domain and advances in fire time. Reverse
//! time and AD state remain fail-closed.

pub mod build;
pub mod check;
pub mod model;
pub mod simulation;

pub use build::*;
pub use check::*;
pub use model::*;
pub use simulation::*;

#[cfg(test)]
mod tests;
