//! Checked vector (`-vec`) pipeline: authoritative artifact-flow map.
//!
//! Every stage produces a versioned artifact wrapped in an opaque
//! `Verified*` type that only its producer/checker boundary can construct.
//! **A checker never calls its producer, never reuses a producer cache, and
//! never accepts a producer-derived expected result as evidence** — it
//! re-derives the facts it validates. This intentional duplication is the
//! assurance boundary; do not "deduplicate" it.
//!
//! # Stage map
//!
//! | Stage (module) | Input | Output artifact | Independent check |
//! |---|---|---|---|
//! | [`analysis`] | `VerifiedPreparedSignals` | execution conditions (DNF), dependency/occurrence views, effect summaries, use tables | source-aligned decoration recomputation ([`super::decoration_verify`]) |
//! | [`plan`] | analysis decorations | `VectorPlan`: signal→loop placement, transports, epochs, fused serial groups | [`verify`] re-validates coverage, order, effects, witnesses with its own reachability |
//! | [`schedule`] | verified plan | strategy-dependent epoch serialization (`-ss 0..3`) | deterministic-order audit; plan certificate is strategy-independent |
//! | [`state`] | plan + decorations | `VectorStatePlan`: delay/recursion `pre/exec/post` phases, C++ copy/ring words | independent geometry/phase-coverage validation + executable simulators |
//! | [`clock_ad`] | plan + fresh clock envs | `VectorClockAdPlan`: OD/US/DS islands, AD policy, fire-time cursors | independent source/domain alignment check; shared admission guards stay on both paths (plan §4.8) |
//! | [`route`] | plan + state/clock policies | `VerifiedRoutedFir`: region-local definitions, transport materialization | independent FIR-evidence inspection |
//! | [`lower`] | routed regions | `VerifiedPureVectorProgram`: lowered signal closures, per-region CSE | final bodies re-checked against route evidence |
//! | [`events`] | routed chunk | `VerifiedEventOrderCertificate`: scalar/vector dynamic orders, `FissionSafe` obligation | independent event/dependency reconstruction |
//! | [`assemble`] | lowered program + state/clock plans | `VerifiedVectorFirAssembly`: loops, state words, islands | independent exact-coverage FIR inspection |
//! | `module` | assembly | final FIR module: outputs, lifecycle, `-lv 0/1` chunk drivers | FIR checker + vector-specific final checks; maps rejections to fallback reasons |
//! | [`lockstep`] | prepared groups | instance-vectorization isomorphism candidates | witness-based independent gate |
//! | `ui` | `UiProgram` | UI zone/effect facts for planning | (vocabulary; UI programs currently fall back) |
//!
//! Semantic contracts frozen across refactors: deterministic ordering of
//! every schedule and serialized certificate; exact schema versions and
//! stable diagnostic codes; fail-closed fallback reasons; scalar/vector
//! bit-exactness; `ZeroPad` fire-index gating; history-served cross-group
//! back-edges; `attach` ordering edges; `UnadoptedStatefulRead` rejection.
//!
//! # C++ provenance
//! The pipeline is an adapted decomposition of the C++
//! `DAGInstructionsCompiler` (`compiler/generator/dag_instructions_compiler.cpp`,
//! `compile_vect.cpp`): C++ discovers facts while lowering; this port freezes
//! each fact family into a checked artifact first, then lowers. Per-stage
//! provenance lives in each module header.

use super::{cse, decoration_verify, recursion, siggen};

pub mod analysis;
pub mod assemble;
pub mod clock_ad;
pub mod events;
pub mod lockstep;
pub mod lower;
pub(crate) mod module;
pub mod plan;
pub mod route;
pub mod schedule;
pub mod state;
pub(crate) mod ui;
pub mod verify;

// Keep the old internal names available while callers migrate to the grouped
// `signal_fir::vector::*` namespace.
use analysis as vector_analysis;
use assemble as vector_assemble;
use clock_ad as vector_clock_ad;
use events as vector_events;
use lower as vector_lower;
use plan as vector_plan;
use route as vector_route;
use schedule as vector_schedule;
use state as vector_state;
use ui as vector_ui;
use verify as vector_verify;
