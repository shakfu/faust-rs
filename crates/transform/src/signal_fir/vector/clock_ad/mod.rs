//! Verified clock-island and automatic-differentiation execution policy
//! (serial OD/US/DS islands, fire-time state, AD windows).
//!
//! # C++ provenance and adaptation
//! Clock guards follow `compile_scal.cpp::generateOD` and the clocked
//! `CodeIFblock`/`SimpleForLoop` lowering: boolean on-demand executes zero or
//! one inner transition, integer on-demand and upsampling execute a counted
//! number of transitions, and downsampling executes through a persistent
//! modulo counter. Domain-owned state advances in fire time and `PermVar`
//! outputs hold their last value when a domain does not fire.
//!
//! Rust composes the already verified signal-level [`VectorPlan`](crate::signal_fir::vector::verify::VectorPlan) with the
//! propagation-owned [`propagate::ClockDomainTable`] and a freshly recomputed
//! [`crate::clk_env::ClkEnvMap`]. Every wrapper becomes one serial island with
//! explicit parentage, member signals, nested loops, and transport policy.
//! Only top-rate transports retain P5's outer-chunk indexing; domain-rate
//! transports are marked island-scalar and must be rematerialized below the
//! guard by the later FIR assembly step.
//!
//! Forward AD has no Signal-IR carrier after propagation: its primal and
//! tangent lanes are ordinary prepared signals and use the normal vector
//! plan. `ReverseTimeRec` and `BlockReverseAD`, by contrast, are certified as
//! scalar fallbacks with immutable `Forward < Reverse` epochs. This module
//! does not claim vector reverse-window semantics and cannot activate a
//! backend path by itself.
//!
//! `check.rs` never calls `build.rs`: every island, transport, and
//! reverse-fallback obligation is re-derived independently from the sources
//! before the shared terminal verify cross-checks the producer's plan.

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
