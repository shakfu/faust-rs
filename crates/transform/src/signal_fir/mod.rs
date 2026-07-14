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
pub mod decoration_verify;
mod delay;
mod error;
mod loop_graph;
mod module;
mod placement;
mod planner;
pub mod pv_slice;
mod recursion;
pub mod shadow;
mod siggen;
pub mod vector_analysis;
pub mod vector_plan;
pub mod vector_route;
pub mod vector_schedule;
pub mod vector_verify;

pub use error::{SignalFirError, SignalFirErrorCode};

use fir::{FirId, FirStore, FirType};
use signals::SigId;
use tlib::TreeArena;
use ui::UiProgram;

use crate::schedule::SchedulingStrategy;
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

/// Codegen strategy for the generated `compute()` body.
///
/// `Scalar` compiles the whole signal graph into one per-sample loop. `Vector`
/// (`-vec`) restructures it into an **outer chunk loop** of `vec_size` samples
/// containing a DAG of small inner loops, so the C compiler can auto-vectorize
/// the non-recursive ones (SIMD); recursive computations stay in serial loops.
///
/// Roadmap P6, vector doc V1
/// (`porting/vector-mode-analysis-port-plan-2026-06-10-en.md`). **V1 plumbs the
/// option/CLI surface only**: `Scalar` is the sole lowering acted on today, and
/// selecting `Vector` currently falls back to scalar codegen until the
/// `LoopGraph` lowering (V2+) lands. It is threaded now so later slices have a
/// stable configuration point and CLI contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComputeMode {
    /// One scalar loop over the whole block (`for i in 0..count`).
    #[default]
    Scalar,
    /// Vector mode: outer chunk loop of `vec_size` samples, inner loop DAG.
    Vector {
        /// Chunk size (`-vs N`; Faust default 32).
        vec_size: u32,
        /// Chunk-driver variant (`-lv`, as Faust C++): `0` = fastest (default) —
        /// a constant-trip main loop over `count - count % vec_size` plus a scalar
        /// remainder (autovectorization-friendly); `1` = simple — one loop with a
        /// runtime `min(vindex + vec_size, count)` bound.
        loop_variant: u8,
    },
}

impl ComputeMode {
    /// The default Faust vector size (`-vs`) when `-vec` is given without `-vs`.
    pub const DEFAULT_VEC_SIZE: u32 = 32;

    /// Whether this mode requests vector-mode codegen.
    #[must_use]
    pub fn is_vector(self) -> bool {
        matches!(self, Self::Vector { .. })
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
    /// Codegen strategy for `compute()`: scalar (default) or vector mode
    /// (`-vec`). Roadmap P6 (V1 plumbing; `Vector` still lowers as `Scalar`
    /// until the `LoopGraph` slices land).
    pub compute_mode: ComputeMode,
    /// Signal/loop dependency scheduling policy (`-ss` /
    /// `--scheduling-strategy`). Vectorization port plan phase P2: plumbing
    /// only — the selected strategy is stored and reported but no lowering
    /// path calls [`crate::schedule::schedule`] yet (P3 activates scalar
    /// scheduling). Deliberately independent of [`ComputeMode`]: the same
    /// strategy applies to the scalar control/signal DAG and, once P5 lands,
    /// to the vector `LoopGraph` schedule (plan §2.5).
    pub scheduling_strategy: SchedulingStrategy,
}

impl Default for SignalFirOptions {
    fn default() -> Self {
        Self {
            module_name: "mydsp".to_owned(),
            real_type: RealType::Float32,
            max_copy_delay: 16,
            delay_line_threshold: u32::MAX,
            compute_mode: ComputeMode::Scalar,
            scheduling_strategy: SchedulingStrategy::DepthFirst,
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
    /// Demand-driven first-lowering order of every distinct materialized
    /// `SigId` (P3 shadow-mode diagnostic input, plan §P3 "record
    /// statement-order... differences for `-ss 0` before making it
    /// authoritative"). Observation-only: nothing in lowering or codegen
    /// reads it, and it never affects the emitted FIR. Compare it against a
    /// selected `hgraph::Hsched` with [`shadow::compare_emission_order`].
    pub emission_order: Vec<SigId>,
    /// P3 shadow-mode report: how the demand-driven [`Self::emission_order`]
    /// relates to the causality gate's selected `Hsched` (`-ss 0`). `None`
    /// when no hierarchical graph was built (a wrapper program compiled
    /// through the clock-unaware entry point). Observation-only; the FIR is
    /// identical whether or not this is computed.
    pub shadow_report: Option<shadow::ShadowReport>,
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
    compile_fastlane_inner(_arena, signals, num_inputs, num_outputs, ui, options, None)
}

/// Clock-domain-aware variant of [`compile_signals_to_fir_fastlane_with_ui`].
///
/// `clock_domains` is the propagation-owned side table
/// ([`propagate::ClockDomainTable`], roadmap P0.2). When the program contains
/// clocked wrappers, this entry runs the clock-environment inference
/// ([`crate::clk_env`]) and the hierarchical-graph validation
/// ([`crate::hgraph`]) on the prepared forest, then lowers boolean `ondemand`
/// blocks as guarded regions (roadmap P3, first slice). Programs without
/// clocked wrappers behave exactly like the plain entry point.
pub fn compile_signals_to_fir_fastlane_clocked(
    arena: &TreeArena,
    signals: &[SigId],
    num_inputs: usize,
    num_outputs: usize,
    ui: &UiProgram,
    clock_domains: &propagate::ClockDomainTable,
    options: &SignalFirOptions,
) -> Result<SignalFirOutput, SignalFirError> {
    compile_fastlane_inner(
        arena,
        signals,
        num_inputs,
        num_outputs,
        ui,
        options,
        Some(clock_domains),
    )
}

fn compile_fastlane_inner(
    arena: &TreeArena,
    signals: &[SigId],
    num_inputs: usize,
    num_outputs: usize,
    ui: &UiProgram,
    options: &SignalFirOptions,
    clock_domains: Option<&propagate::ClockDomainTable>,
) -> Result<SignalFirOutput, SignalFirError> {
    let plan = planner::plan_signals(signals, num_inputs, num_outputs, options)?;
    let prepared = prepare_signals_for_fir_verified(arena, signals, ui).map_err(|err| {
        SignalFirError::new(
            SignalFirErrorCode::UnsupportedSignalNode,
            format!("signal preparation failed: {err}"),
        )
    })?;

    // Causality gate (P3): build the hierarchical dependency graph and a
    // schedule for every prepared forest before lowering. `gate_graphs`
    // captures the `(Hgraph, Hsched)` when one is built, so the P3
    // shadow-mode diagnostic below can compare the selected schedule against
    // the demand-driven emission order — over the same prepared arena. The
    // schedule itself is still *not* authoritative over lowering: only its
    // acceptance (causality) gates, so `-ss` remains behaviorally
    // unobservable.
    let mut gate_graphs: Option<(crate::hgraph::Hgraph, crate::hgraph::Hsched)> = None;
    let clocked = match clock_domains {
        Some(domains) if !domains.is_empty() => {
            let envs = crate::clk_env::annotate(prepared.arena(), domains, prepared.outputs())
                .map_err(|err| {
                    SignalFirError::new(
                        SignalFirErrorCode::ClockAnalysis,
                        format!("clock-environment inference failed: {err}"),
                    )
                })?;
            let hgraph = crate::hgraph::build_hgraph(
                prepared.arena(),
                domains,
                &envs,
                prepared.outputs(),
                prepared.sig_types_map(),
            )
            .map_err(|err| {
                SignalFirError::new(
                    SignalFirErrorCode::ClockAnalysis,
                    format!("hierarchical dependency graph failed: {err}"),
                )
            })?;
            let hsched =
                crate::hgraph::schedule(&hgraph, crate::schedule::SchedulingStrategy::DepthFirst)
                    .map_err(|err| {
                    SignalFirError::new(
                        SignalFirErrorCode::ClockAnalysis,
                        format!("clock-domain scheduling failed: {err}"),
                    )
                })?;
            gate_graphs = Some((hgraph, hsched));
            Some(module::ClockedPlan { domains, envs })
        }
        _ => {
            // A program reaching this branch with an actual OD/US/DS wrapper
            // node was compiled through a clock-unaware entry point (no
            // `ClockDomainTable` was ever supplied): `clk_env::annotate`
            // cannot resolve a real wrapper's clock relationship from an
            // empty table, and would report a confusing
            // `ClockedViolation`-family error instead of letting
            // `module::build_module`'s own, specific `FRS-SFIR-0007`
            // ("clocked node reached without a domain table") rejection
            // fire, exactly as before this gate existed. So skip the gate
            // for those; ordinary wrapper-free programs run it.
            let has_wrapper = crate::hgraph::contains_wrapper(prepared.arena(), prepared.outputs())
                .map_err(|err| {
                    SignalFirError::new(
                        SignalFirErrorCode::ClockAnalysis,
                        format!("wrapper scan failed: {err}"),
                    )
                })?;
            if !has_wrapper {
                let empty_domains = propagate::ClockDomainTable::new();
                let envs =
                    crate::clk_env::annotate(prepared.arena(), &empty_domains, prepared.outputs())
                        .map_err(|err| {
                            SignalFirError::new(
                                SignalFirErrorCode::ClockAnalysis,
                                format!("clock-environment inference failed: {err}"),
                            )
                        })?;
                let hgraph = crate::hgraph::build_hgraph(
                    prepared.arena(),
                    &empty_domains,
                    &envs,
                    prepared.outputs(),
                    prepared.sig_types_map(),
                )
                .map_err(|err| {
                    SignalFirError::new(
                        SignalFirErrorCode::ClockAnalysis,
                        format!("hierarchical dependency graph failed: {err}"),
                    )
                })?;
                let hsched = crate::hgraph::schedule(
                    &hgraph,
                    crate::schedule::SchedulingStrategy::DepthFirst,
                )
                .map_err(|err| {
                    SignalFirError::new(
                        SignalFirErrorCode::ClockAnalysis,
                        format!("clock-domain scheduling failed: {err}"),
                    )
                })?;
                gate_graphs = Some((hgraph, hsched));
            }
            None
        }
    };

    let output = module::build_module(
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
        options.compute_mode,
        clocked,
    )?;

    // P3 shadow mode (observation-only): with the demand-driven emission
    // order and the gate's selected `Hsched` now both available over the
    // same prepared arena, record how they relate. This never alters the
    // returned FIR — it only annotates the output for diagnostics/tests, so
    // activation (making the schedule authoritative) can be judged against
    // real corpus evidence rather than assumed.
    let shadow_report = gate_graphs
        .as_ref()
        .map(|(hgraph, hsched)| shadow::compare_emission_order(hgraph, hsched, &output));

    Ok(SignalFirOutput {
        shadow_report,
        ..output
    })
}

#[cfg(test)]
mod tests;
