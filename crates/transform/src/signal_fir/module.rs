//! FIR module emission for the signal->FIR fast-lane.
//!
//! Step 2A..2G lowers an executable fast-lane slice:
//! - `SIGINPUT`, integer/real constants,
//! - `SIGBINOP` (arithmetic/comparison/bitwise subset),
//! - `SIGPOW`/`SIGMIN`/`SIGMAX`,
//! - core unary math nodes (`sin/cos/tan/exp/log/log10/sqrt/abs`),
//! - `SIGDELAY1`/fixed-size `SIGDELAY`/`SIGPREFIX`,
//! - `SIGSELECT2`, `SIGINTCAST`/`SIGFLOATCAST`/`SIGBITCAST`,
//! - `SIGPROJ`/`SYMREC`/`SYMREF` (real lowering for canonical recursion groups
//!   after `de_bruijn_to_sym` conversion).
//! - `SIGWAVEFORM`/`SIGRDTBL`/`SIGWRTBL` for direct waveform tables.
//! - `SIGOUTPUT` passthrough nodes.
//! - sectioned FIR module assembly (`metadata`, `instanceConstants`,
//!   `instanceResetUserInterface`, `instanceClear`, `buildUserInterface`, `compute`).
//!
//! Section placement policy (Step 3B):
//! - `instanceConstants`: table initialization and compile-time constants
//!   (`iConst*` / `fConst*` variables — [`Variability::Konst`](sigtype::Variability::Konst)).
//! - `instanceResetUserInterface`: UI zone reset values.
//! - `instanceClear`: runtime signal state reset values (delay/rec state).
//! - `compute` preamble (before sample loop): block-rate control expressions
//!   (`iSlow*` / `fSlow*` variables — [`Variability::Block`](sigtype::Variability::Block)).
//! - `compute` sample loop: sample-rate expressions (inline, no hoisting).
//!
//! Integer policy:
//! - `SIGINT`/`SIGINTCAST` and integer bitwise operations lower to FIR `Int32`
//!   nodes/types for C++ parity in the active fast-lane.
//!
//! Type duality policy (internal vs external):
//! - **Internal real type** (`real_ty`, default `FirType::Float32`): used for
//!   all internal DSP computation — state variables, arithmetic results, math
//!   call signatures, waveform table element types, and real constants.
//!   Configurable at module build time via [`super::RealType`].
//! - **Prepared reduced type map** (`signal_prepare::SimpleSigType`): used to
//!   keep integer delay/recursion/table carriers and integer arithmetic results
//!   in FIR when the prepared signal forest proves they stay integer after the
//!   reduced promotion pass.
//! - **External type** (`FirType::FaustFloat`): used exclusively for the
//!   `FAUSTFLOAT**` audio buffer parameters in `compute`, and for UI zone
//!   struct variables (sliders, bargraphs, buttons) that are read/written by
//!   the host application.
//! - Implicit casts are emitted at every boundary:
//!   - input sample load: `FaustFloat → real_ty`,
//!   - output sample store: `real_ty → FaustFloat`,
//!   - UI zone read (for computation): `FaustFloat → real_ty`,
//!   - bargraph zone write (from computation): `real_ty → FaustFloat`.
//!
//! Other signal families still return typed `FRS-SFIR-*` errors.
//!
//! # BlockReverseAD / BRA scheduling model
//!
//! `SigBlockReverseAD` arrives from `propagate::reverse_ad` as a semantic
//! carrier: it says that a `rad(...)` sub-expression must be evaluated with
//! block-local reverse-mode semantics.  It does not prescribe the final C++ loop
//! shape.  This module owns the concrete FIR schedule and storage:
//!
//! 1. **Primal projections** (`Proj(0..M-1, BlockReverseAD)`) lower by lowering
//!    the corresponding body signal in causal order.  While doing so,
//!    `ensure_bra_tape_stores` may schedule forward stores to `fBraTapeN[i0]`
//!    for values that cannot be reconstructed during the adjoint sweep.
//! 2. **Gradient projections** (`Proj(M+j, BlockReverseAD)`) call
//!    `ensure_bra_backward_sweep`, which emits the local transpose program and
//!    caches the requested seed adjoints.
//! 3. The caller’s context decides where those statements land.  If the
//!    gradient projection is a public output, `classify_reverse_time_outputs`
//!    marks it reverse-time and `build_module` creates a second loop that runs
//!    `i0 = count-1 .. 0`.  If the gradient projection is used inside another
//!    forward expression, such as an adaptive recursive update, the same sweep
//!    is emitted into the currently active forward sample phase.  In that
//!    generated program there is no separate backward loop; the forward and
//!    adjoint statements are interleaved in one causal loop.
//!
//! The inline schedule is intentional.  Classifying an outer `Proj(SYMREC)` as
//! reverse-time just because its body contains an internal RAD gradient would
//! suppress the causal recursion update and produce the wrong program shape.
//! Therefore `classify_reverse_time_outputs` stops at `SYMREC` boundaries: the
//! public recursive output remains forward-time, and any BRA work discovered
//! while lowering the body is scheduled locally.
//!
//! Do not confuse this inline schedule with the older LTI recursive transpose
//! fast path.  The LTI path is a propagation-time choice that rewrites an
//! eligible linear recursive frontier into a `ReverseTimeRec` carrier.  The BRA
//! inline case is only a lowering-time placement choice for the general
//! block-tape fallback: the derivative is still represented by
//! `BlockReverseAD`, but the first gradient projection is requested while the
//! forward recursive update is being emitted.
//!
//! Storage follows the same boundary:
//!
//! - `fBraTapeN` arrays are scratch forward tapes, written for `0..count-1`
//!   before their matching slots are read. They are not `instanceClear` state.
//! - `fBraCarryN` / `fBraDelayCarryN` fields are adjoint carries and are reset
//!   at `compute()` entry, because each host call is one independent TBPTT
//!   block.
//! - `Konst` values below a `BlockReverseAD` carrier are forced to persistent
//!   struct storage by `placement.rs`, since the synthesized sweep can create
//!   compute-time uses that are not visible as parent edges in the original
//!   signal DAG.

use std::collections::{BTreeMap, HashMap, HashSet};

use fir::{
    AccessType, BargraphType, ButtonType, FirBinOp, FirBuilder, FirId, FirMathOp, FirStore,
    FirType, NamedType, SliderRange, SliderType, UiBoxType,
};
use signals::{BinOp, SigId, SigMatch, dump_sig_readable, match_sig};
use tlib::{
    NodeKind, TreeArena, TreeId, list_to_vec, match_sym_rec, match_sym_ref, tree_to_int,
    tree_to_str,
};
use ui::{ControlId, ControlKind, UiGroupKind, UiMatch, UiProgram, match_ui};

use sigtype::{SigType, Variability};

use crate::signal_prepare::SimpleSigType;

use super::SignalFirOutput;
use super::block_reverse_ad::{collect_bra_postorder, collect_tape_needed_values};
use super::delay::{
    DelayFirCtx, DelayLineInfo, DelayLoweringCtx, DelayManager, DelayOptions, GlobalCircularCursor,
    delay_size_for_amount, emit_delay1_for_line, emit_fixed_delay_for_line,
};
use super::error::{SignalFirError, SignalFirErrorCode};
use super::placement::{Bucket, analyze_signal_sharing, is_trivial_fir};
use super::planner::SignalFirPlan;
use super::recursion::{
    RecArrayInfo, RecursionAllocCtx, RecursionCarrierRef, RecursionCurrentValueBinding,
    RecursionDelayRef, RecursionGroupProjection, RecursionLoweringCtx, RecursionState,
    RecursionStorageStrategy, decode_group_projection, match_recursion_delay_key,
    resolve_active_recursion_carrier,
};
use super::siggen::interpret_generator;

/// Explicit execution phases inside one sample-loop iteration.
///
/// The sample body is assembled in this fixed order:
///
/// 1. `immediate`: ordinary per-sample work and writes that must happen before
///    outputs are finalized
/// 2. `post_output`: updates that must observe the current sample's outputs
///    before shifting/finalizing state
/// 3. `sample_end`: generic subsystem maintenance such as delay counter bumps
#[derive(Default)]
struct SamplePhases {
    immediate: Vec<FirId>,
    post_output: Vec<FirId>,
    sample_end: Vec<FirId>,
}

impl SamplePhases {
    fn flattened(&self) -> Vec<FirId> {
        let mut all = Vec::with_capacity(
            self.immediate.len() + self.post_output.len() + self.sample_end.len(),
        );
        all.extend(self.immediate.iter().copied());
        all.extend(self.post_output.iter().copied());
        all.extend(self.sample_end.iter().copied());
        all
    }
}

/// Maximum number of samples that can be stored in a BRA forward tape array.
///
/// Tape arrays are declared as `fBraTapeN: Array(real_ty, MAX_BRA_TAPE_BLOCK_SIZE)`.
/// The host must not call `compute()` with a frame count larger than this value
/// when using a `SigBlockReverseAD` carrier; doing so would overflow the tape.
///
/// 8 192 samples is the default upper bound chosen to stay within typical L1/L2
/// cache pressure while leaving room for the usual block sizes used in practice
/// (64, 128, 256, 512, 1024 samples).
const MAX_BRA_TAPE_BLOCK_SIZE: usize = 8192;

/// Deterministic prototype emission order for math helper functions.
///
/// Keeping this order stable avoids noisy golden diffs in generated FIR/C/C++.
const MATH_PROTO_ORDER: &[FirMathOp] = &[
    FirMathOp::Pow,
    FirMathOp::Min,
    FirMathOp::Max,
    FirMathOp::Sin,
    FirMathOp::Cos,
    FirMathOp::Acos,
    FirMathOp::Asin,
    FirMathOp::Atan,
    FirMathOp::Atan2,
    FirMathOp::Tan,
    FirMathOp::Exp,
    FirMathOp::Log,
    FirMathOp::Log10,
    FirMathOp::Sqrt,
    FirMathOp::Abs,
    FirMathOp::Fmod,
    FirMathOp::Remainder,
    FirMathOp::Floor,
    FirMathOp::Ceil,
    FirMathOp::Rint,
    FirMathOp::Round,
];

/// Deterministic prototype emission order for polymorphic integer helper calls.
const INT_FUN_PROTO_ORDER: &[&str] = &["abs", "min_i", "max_i"];

/// Lowers a prepared signal forest into a complete FIR module.
///
/// Entry point for the fast-lane Step 2A–2G boundary: accepts pre-validated
/// planning data and a prepared signal forest, returns a [`SignalFirOutput`]
/// with all Faust lifecycle sections (`metadata`, `instanceConstants`,
/// `instanceResetUserInterface`, `instanceClear`, `buildUserInterface`,
/// `compute`) assembled in deterministic order.
///
/// # Promotion invariant
///
/// The `signals` forest **must** have been processed by
/// `signal_prepare::promote_signals_for_fir` (and optionally
/// `normalize::simplify`) before being passed here.  That pass guarantees:
///
/// - Every `BinOp(op, lhs, rhs)` node has operands whose signal domain
///   types are already consistent with `op`: mixed Int/Real operands are
///   wrapped in explicit `FloatCast` nodes; bitwise/shift operands in
///   `IntCast` nodes; `Div` operands are always Real.
/// - Every `Delay(_, amount)`, `RdTbl(_, index)`, `WrTbl(…, widx, _)`,
///   `Select2(selector, …)`, and `Enable(_, gate)` has its integer-context
///   operand wrapped in `IntCast`.
/// - `Delay1(x)` and `Prefix(init, x)` have `type(init) == type(x)`.
///
/// **Consequence for the lowerer**: no implicit coercion is needed inside
/// `lower_binop`, `lower_delay_state`, or `normalized_table_index`.  All
/// necessary casts appear as explicit signal-tree nodes and are handled by
/// `lower_cast` when the lowerer dispatches on `SigMatch::IntCast /
/// FloatCast`.
///
/// BRA tape lowering relies on the same invariant.  It does not run a second
/// promotion pass over synthesized `fBraTapeN` stores.  If the signal graph
/// contains an integer/discrete subgraph that feeds a real expression through a
/// `FloatCast` (for example an LCG noise recursion multiplied by a real scale),
/// the cast node is the promoted real boundary.  The integer nodes upstream of
/// that cast keep their integer semantics and are not valid real tape values.
///
/// # Recursion Boundary
///
/// Most recursion-specific mechanics now live in `recursion.rs`:
///
/// - recursion carrier/state data types
/// - active/materialized carrier resolution
/// - delayed recursion reference resolution
/// - recursive-group projection decoding/validation
/// - recursion carrier allocation helpers
/// - recursion-specific FIR helper emission
///
/// `module.rs` remains responsible for orchestration:
///
/// - `lower_signal(...)` dispatch
/// - deciding when a top-level recursion group must be materialized
/// - evaluating recursive body expressions
/// - integrating recursion writes/finalization into the sample phases
///
/// # Recursion and delay1 coupling
///
/// Recursion outputs can be consumed through delay chains rooted at
/// `Proj(i, group)`, not only through the immediate feedback form
/// `Delay1(Proj(i, group))`.
///
/// The lowering path now resolves `Delay1^k(Proj(...))` through
/// `resolve_recursion_delay_ref` and reuses the group's existing recursion
/// carrier instead of allocating a separate delay-state slot. For scalar
/// carriers this reads the previous-sample struct field directly. For size-2
/// carriers, this preserves the direct two-slot fast path; for larger carriers,
/// reads use the preplanned circular recursion array sized from accumulated
/// delay analysis.
///
/// This is why two separate state spaces exist:
///
/// - `state_name_by_node`: standalone non-recursive delay-state slots keyed by
///   delay node
/// - `self.recursion`: recursion carriers keyed by `(group, body index)`
///
/// They must never alias, even when the body signal of a recursion group
/// happens to be the same `SigId` as a `Delay1` node (the tf22 regression
/// pattern).
///
/// # Parameters
///
/// - `plan` – pre-checked I/O counts and signal statistics.
/// - `types` – per-signal [`SimpleSigType`] from `signal_prepare`; drives
///   integer-vs-real decisions for state/table element types.
/// - `sig_types` – full type-annotator map; used only for interval-based
///   variable delay sizing via [`sigtype::check_delay_interval`].
/// - `real_ty` – internal computation type (`Float32` or `Float64`).
#[allow(clippy::too_many_arguments)]
pub(super) fn build_module(
    plan: &SignalFirPlan,
    module_name: &str,
    arena: &TreeArena,
    signals: &[SigId],
    ui: &UiProgram,
    types: &HashMap<SigId, SimpleSigType>,
    sig_types: &HashMap<SigId, SigType>,
    real_ty: FirType,
    max_copy_delay: u32,
    delay_line_threshold: u32,
) -> Result<SignalFirOutput, SignalFirError> {
    let delay_opts = DelayOptions {
        max_copy_delay,
        delay_line_threshold,
    };
    let (sig_ref_counts, sig_at_boundary, konst_escapes) =
        analyze_signal_sharing(arena, signals, sig_types);
    let mut lower = SignalToFirLower::new(
        arena,
        ui,
        types,
        sig_types,
        plan.num_inputs,
        real_ty,
        sig_ref_counts,
        sig_at_boundary,
        konst_escapes,
        delay_opts,
    );
    lower.ensure_sample_rate_var();
    lower.prepare_delay_lines(signals)?;
    let reverse_time_outputs = classify_reverse_time_outputs(lower.arena, signals);
    lower.forward_output_by_sig = signals
        .iter()
        .enumerate()
        .filter_map(|(index, &sig)| (!reverse_time_outputs[index]).then_some((sig, index)))
        .collect();
    let dsp_arg_type = FirType::Ptr(Box::new(FirType::Obj));
    let dsp_arg = NamedType {
        name: "dsp".to_string(),
        typ: dsp_arg_type.clone(),
    };

    {
        let mut b = FirBuilder::new(&mut lower.store);
        lower
            .control_statements
            .push(b.label("signal_fir_fastlane_step2a: executable base slice"));
        lower.control_statements.push(b.label(format!(
            "io: inputs={} outputs={}",
            plan.num_inputs, plan.num_outputs
        )));
        lower
            .control_statements
            .push(b.label(format!("signals: {}", plan.signal_count)));
    }

    let has_forward_outputs = reverse_time_outputs.iter().any(|is_reverse| !*is_reverse);
    let has_reverse_outputs = reverse_time_outputs.iter().any(|is_reverse| *is_reverse);
    if has_reverse_outputs {
        // Readable structural fallback keys are only needed when the RAD
        // reverse-time loop must reconnect a delayed value to a forward output.
        lower.forward_output_by_sig_key = signals
            .iter()
            .enumerate()
            .filter_map(|(index, &sig)| {
                (!reverse_time_outputs[index]).then_some((dump_sig_readable(arena, sig), index))
            })
            .collect();
    }
    let mut sample_loops = Vec::new();

    if has_forward_outputs {
        // Forward loop slice.  This is not necessarily "primal only": when a
        // BRA gradient projection is consumed inside a forward-time expression
        // (for example `p_next = p - lr * grad_p` inside a recursion body),
        // `lower_output_signal` can descend into that expression and call
        // `ensure_bra_backward_sweep`.  In that case the BRA adjoint statements
        // are appended to this same forward sample phase, and no separate
        // public backward loop is required unless another top-level output was
        // classified as reverse-time below.
        for (signal_index, sig) in signals.iter().enumerate() {
            if !reverse_time_outputs[signal_index] {
                lower.lower_output_signal(signal_index, *sig, plan.num_outputs)?;
            }
        }
        let delay_sample_end = lower
            .delay
            .emit_sample_end_updates(&mut lower.store, lower.uses_iota);
        lower.sample_phases.sample_end.extend(delay_sample_end);
        sample_loops.push((false, lower.sample_phases.flattened()));
        lower.reset_sample_loop_state();
    }

    if has_reverse_outputs {
        // Reverse loop slice for public reverse-time outputs.  This path is
        // used when the public bundle contains gradient projections, such as
        // `process = rad(loss, params)`.  Adaptive DSPs may skip this block
        // entirely: their gradient projection can be internal to the forward
        // update and therefore scheduled by the forward slice above.
        lower.cache.clear();
        lower.lowering_reverse_loop = true;
        for (signal_index, sig) in signals.iter().enumerate() {
            if reverse_time_outputs[signal_index] {
                lower.lower_output_signal(signal_index, *sig, plan.num_outputs)?;
            }
        }
        lower.lowering_reverse_loop = false;
        if !has_forward_outputs {
            let delay_sample_end = lower
                .delay
                .emit_sample_end_updates(&mut lower.store, lower.uses_iota);
            lower.sample_phases.sample_end.extend(delay_sample_end);
        }
        sample_loops.push((true, lower.sample_phases.flattened()));
        lower.reset_sample_loop_state();
    }
    for index in 0..plan.num_outputs {
        let mut b = FirBuilder::new(&mut lower.store);
        let chan = b.int32(i32::try_from(index).expect("validated output index fits i32"));
        let ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let load_chan_ptr = b.load_table("outputs", AccessType::FunArgs, chan, ptr_ty.clone());
        lower.control_statements.push(b.declare_var(
            format!("output{index}"),
            ptr_ty,
            AccessType::Stack,
            Some(load_chan_ptr),
        ));
    }
    if has_reverse_outputs {
        lower.emit_reverse_time_rec_compute_resets();
    }
    // Reset BRA carry variables at the start of every compute() call.
    //
    // These carries are populated by `ensure_bra_backward_sweep` regardless of
    // whether the BRA backward sweep runs in the forward or reverse sample loop.
    // Zeroing them here treats each `compute()` call as the start of a fresh
    // TBPTT block, which is the correct interpretation for both BS=BS (reverse
    // loop) and BS=1 (forward inline) TBPTT approximations.
    //
    // `emit_bra_compute_resets` is a no-op when no BRA carry variables were
    // allocated (i.e. when no `BlockReverseAD` node appears in the program).
    lower.emit_bra_compute_resets();
    // ═══════════════════════════════════════════════════════════════════════
    // ── Phase 2: CSE Materialization per Bucket ────────────────────────────
    // ═══════════════════════════════════════════════════════════════════════
    // Deduplicate multi-referenced value sub-expressions within each
    // execution tier.  Runs after variability placement (Phase 1) has
    // finalized bucket contents, so reference counts are stable.
    {
        use super::cse;

        let rc = cse::count_fir_value_uses(&lower.store, &lower.constants_statements);
        cse::materialize_shared_values(
            &mut lower.store,
            &mut lower.constants_statements,
            &rc,
            "fConst",
            lower.fconst_counter,
            "iConst",
            lower.iconst_counter,
        );

        let rc = cse::count_fir_value_uses(&lower.store, &lower.control_statements);
        cse::materialize_shared_values(
            &mut lower.store,
            &mut lower.control_statements,
            &rc,
            "fSlow",
            lower.fslow_counter,
            "iSlow",
            lower.islow_counter,
        );

        for (_, sample_loop_statements) in &mut sample_loops {
            let rc = cse::count_fir_value_uses(&lower.store, sample_loop_statements);
            cse::materialize_shared_values(
                &mut lower.store,
                sample_loop_statements,
                &rc,
                "fTemp",
                0,
                "iTemp",
                0,
            );
        }
    }

    let metadata_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&[])
    };
    let metadata_args = [
        dsp_arg.clone(),
        NamedType {
            name: "m".to_string(),
            typ: FirType::Meta,
        },
    ];
    let metadata = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "metadata",
            FirType::Fun {
                args: vec![dsp_arg_type.clone(), FirType::Meta],
                ret: Box::new(FirType::Void),
            },
            &metadata_args,
            Some(metadata_body),
            false,
        )
    };

    let constants_body = {
        let sample_rate_store = {
            let mut b = FirBuilder::new(&mut lower.store);
            let sample_rate = b.load_var("sample_rate", AccessType::FunArgs, FirType::Int32);
            b.store_var("fSampleRate", AccessType::Struct, sample_rate)
        };
        lower.constants_statements.insert(0, sample_rate_store);
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.constants_statements)
    };
    let constants_args = [
        dsp_arg.clone(),
        NamedType {
            name: "sample_rate".to_string(),
            typ: FirType::Int32,
        },
    ];
    let instance_constants = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "instanceConstants",
            FirType::Fun {
                args: vec![dsp_arg_type.clone(), FirType::Int32],
                ret: Box::new(FirType::Void),
            },
            &constants_args,
            Some(constants_body),
            false,
        )
    };

    lower.emit_ui_program()?;
    let ui_statements = lower.ui_statements.clone();
    let ui_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&ui_statements)
    };
    let build_ui_args = [
        dsp_arg.clone(),
        NamedType {
            name: "ui_interface".to_string(),
            typ: FirType::UI,
        },
    ];
    let build_ui = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "buildUserInterface",
            FirType::Fun {
                args: vec![dsp_arg_type.clone(), FirType::UI],
                ret: Box::new(FirType::Void),
            },
            &build_ui_args,
            Some(ui_body),
            false,
        )
    };

    let reset_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.reset_statements)
    };
    let instance_reset_ui = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "instanceResetUserInterface",
            FirType::Fun {
                args: vec![dsp_arg_type.clone()],
                ret: Box::new(FirType::Void),
            },
            std::slice::from_ref(&dsp_arg),
            Some(reset_body),
            false,
        )
    };

    let clear_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.clear_statements)
    };
    let instance_clear = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "instanceClear",
            FirType::Fun {
                args: vec![dsp_arg_type.clone()],
                ret: Box::new(FirType::Void),
            },
            std::slice::from_ref(&dsp_arg),
            Some(clear_body),
            false,
        )
    };

    let compute_statements = {
        let mut all = Vec::new();
        all.extend(lower.control_statements.iter().copied());
        for (is_reverse, sample_loop_statements) in &sample_loops {
            if sample_loop_statements.is_empty() {
                continue;
            }
            let sample_loop = {
                let mut b = FirBuilder::new(&mut lower.store);
                let upper = b.load_var("count", AccessType::FunArgs, FirType::Int32);
                let body = b.block(sample_loop_statements);
                b.simple_for_loop("i0", upper, body, *is_reverse)
            };
            all.push(sample_loop);
        }
        all
    };
    let compute_body = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&compute_statements)
    };
    let compute_args = [
        dsp_arg.clone(),
        NamedType {
            name: "count".to_string(),
            typ: FirType::Int32,
        },
        NamedType {
            name: "inputs".to_string(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
        NamedType {
            name: "outputs".to_string(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
    ];
    let compute = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    dsp_arg_type,
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        )
    };

    // Math function prototypes use the internal real type for both arguments and
    // return value: `sin`, `cos`, `pow`, etc. operate on internal-precision samples.
    let math_real_ty = lower.real_ty();
    let mut math_prototypes = Vec::new();
    for op in MATH_PROTO_ORDER {
        if !lower.used_math_ops.contains(op) {
            continue;
        }
        let arity = match op {
            FirMathOp::Pow
            | FirMathOp::Min
            | FirMathOp::Max
            | FirMathOp::Atan2
            | FirMathOp::Fmod
            | FirMathOp::Remainder => 2,
            _ => 1,
        };
        let proto_args: Vec<NamedType> = (0..arity)
            .map(|i| NamedType {
                name: format!("arg{i}"),
                typ: math_real_ty.clone(),
            })
            .collect();
        let proto = {
            let mut b = FirBuilder::new(&mut lower.store);
            b.declare_fun(
                op.symbol(),
                FirType::Fun {
                    args: vec![math_real_ty.clone(); arity],
                    ret: Box::new(math_real_ty.clone()),
                },
                &proto_args,
                None,
                false,
            )
        };
        math_prototypes.push(proto);
    }
    for name in INT_FUN_PROTO_ORDER {
        if !lower.used_int_fun_names.contains(name) {
            continue;
        }
        let arity = if *name == "abs" { 1 } else { 2 };
        let proto_args: Vec<NamedType> = (0..arity)
            .map(|i| NamedType {
                name: format!("arg{i}"),
                typ: FirType::Int32,
            })
            .collect();
        let proto = {
            let mut b = FirBuilder::new(&mut lower.store);
            b.declare_fun(
                *name,
                FirType::Fun {
                    args: vec![FirType::Int32; arity],
                    ret: Box::new(FirType::Int32),
                },
                &proto_args,
                None,
                false,
            )
        };
        math_prototypes.push(proto);
    }
    for proto in lower.used_foreign_fun_protos.values() {
        let proto_args: Vec<NamedType> = proto
            .args
            .iter()
            .enumerate()
            .map(|(i, typ)| NamedType {
                name: format!("arg{i}"),
                typ: typ.clone(),
            })
            .collect();
        let decl = {
            let mut b = FirBuilder::new(&mut lower.store);
            b.declare_fun(
                proto.name.clone(),
                FirType::Fun {
                    args: proto.args.clone(),
                    ret: Box::new(proto.ret.clone()),
                },
                &proto_args,
                None,
                false,
            )
        };
        math_prototypes.push(decl);
    }
    math_prototypes.extend(lower.global_declarations.iter().copied());
    let functions = {
        let mut b = FirBuilder::new(&mut lower.store);
        let function_items = [
            metadata,
            instance_constants,
            instance_reset_ui,
            instance_clear,
            build_ui,
            compute,
        ];
        b.block(&function_items)
    };
    let dsp_struct = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.struct_declarations)
    };
    let globals = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&math_prototypes)
    };
    let static_decls_block = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.static_declarations)
    };
    let module: FirId = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.module(
            plan.num_inputs,
            plan.num_outputs,
            module_name,
            dsp_struct,
            globals,
            functions,
            static_decls_block,
        )
    };

    Ok(SignalFirOutput {
        store: lower.store,
        module,
    })
}

fn classify_reverse_time_outputs(arena: &TreeArena, signals: &[SigId]) -> Vec<bool> {
    fn contains_reverse_time_projection(
        arena: &TreeArena,
        sig: SigId,
        visited: &mut HashSet<SigId>,
    ) -> bool {
        if !visited.insert(sig) {
            return false;
        }
        // ReverseTimeRec gradient projections run in the reverse sample loop.
        if matches!(
            match_sig(arena, sig),
            SigMatch::Proj(_, group)
                if matches!(match_sig(arena, group), SigMatch::ReverseTimeRec(_))
        ) {
            return true;
        }
        // BlockReverseAD gradient projections (index ≥ primal_count) also run
        // in a reverse sample loop when they are visible as public outputs.
        // If the same projection is internal to a forward-time expression, this
        // classifier never sees it as a root; it will be lowered inline by the
        // forward slice instead.
        if let SigMatch::Proj(index, group) = match_sig(arena, sig)
            && let SigMatch::BlockReverseAD {
                primal_count,
                policy: _,
                ..
            } = match_sig(arena, group)
        {
            let pc = usize::try_from(primal_count).unwrap_or(0);
            let idx = usize::try_from(index).unwrap_or(0);
            if idx >= pc {
                return true;
            }
        }
        // Stop recursion at SYMREC boundaries.
        //
        // A `Proj(slot, SYMREC)` node is the top-level output of a `loop ~ _`
        // recursive group.  Its primal value is always computed in the FORWARD
        // sample loop (the recursion advances state in causal order).  Recursing
        // into the SYMREC body would discover BRA gradient projections that are
        // used *inside* the body (e.g. `p_next = clamp(p_prev - lr * grad_p)` in
        // `rad_filter1.dsp`) and incorrectly classify the outer output as
        // reverse-time, suppressing the forward loop entirely.  This is the
        // main reason some RAD/BRA DSPs intentionally generate no standalone
        // backward loop: their backward work is internal to the causal recursive
        // update and is emitted while lowering that forward body.
        if let SigMatch::Proj(_, group) = match_sig(arena, sig)
            && match_sym_rec(arena, group).is_some()
        {
            return false;
        }
        arena.node(sig).is_some_and(|node| {
            node.children
                .as_slice()
                .iter()
                .copied()
                .any(|child| contains_reverse_time_projection(arena, child, visited))
        })
    }

    signals
        .iter()
        .map(|&sig| contains_reverse_time_projection(arena, sig, &mut HashSet::new()))
        .collect()
}

/// Stateful lowering engine that converts a propagated signal forest into FIR.
///
/// Stateful rather than purely recursive because the FIR output has multiple
/// side channels: value expressions, per-lifecycle-section statement lists,
/// persistent state and UI declarations, waveform tables, and deferred
/// compute-time updates.  All are accumulated in the fields below and
/// assembled into a [`SignalFirOutput`] by [`build_module`].
///
/// The forest must satisfy the **promotion invariant** documented on
/// [`build_module`]: all type coercions are represented as explicit
/// `IntCast`/`FloatCast` signal-tree nodes inserted by
/// `promote_signals_for_fir`.  The lowering methods therefore never insert
/// implicit casts themselves.
struct SignalToFirLower<'a> {
    /// Read-only signal tree arena shared with the caller.
    arena: &'a TreeArena,
    /// UI descriptor tree used to resolve control ids and emit `buildUserInterface`.
    ui_program: &'a UiProgram,
    /// Reduced per-signal type map from `signal_prepare` (integer vs real vs sound).
    types: &'a HashMap<SigId, SimpleSigType>,
    /// Full type-annotator map used for interval-based variable delay sizing.
    sig_types: &'a HashMap<SigId, SigType>,
    /// Number of audio input channels for the module being compiled.
    num_inputs: usize,
    /// Internal DSP computation type (`Float32` or `Float64`).
    ///
    /// Used for arithmetic results, state variables, math call signatures,
    /// waveform table elements, and real constants.  External interface points
    /// (audio buffers, UI zones) always use [`FirType::FaustFloat`] instead.
    real_ty: FirType,
    /// FIR node store being built; owned by this lowerer and returned in the output.
    store: FirStore,
    /// Memoization cache: maps a `SigId` to its already-lowered `FirId` for DAG sharing.
    cache: HashMap<SigId, FirId>,
    /// DSP struct field declarations (arrays, scalars, UI zones).
    struct_declarations: Vec<FirId>,
    /// Constant waveform table declarations emitted at file scope (`const static`
    /// in C++/C) rather than inside the DSP struct.  These are tables whose
    /// content is fully determined at compile time (waveform literals) and is
    /// shared across all DSP instances.
    static_declarations: Vec<FirId>,
    /// Extern global variable declarations requested by `SIGFVAR` lowering.
    global_declarations: Vec<FirId>,
    /// `instanceConstants` body: table initializations and compile-time constants.
    constants_statements: Vec<FirId>,
    /// `instanceResetUserInterface` body: UI zone reset assignments.
    reset_statements: Vec<FirId>,
    /// `instanceClear` body: delay-line and recursion-state zero-init loops.
    clear_statements: Vec<FirId>,
    /// `compute` preamble: channel-pointer aliases and diagnostic labels.
    control_statements: Vec<FirId>,
    /// Explicit per-sample execution phases for the `compute` sample loop.
    sample_phases: SamplePhases,
    /// Maps each signal node to its generated state-variable name.
    state_name_by_node: HashMap<SigId, String>,
    /// Owned recursion-group state: canonical carriers plus active-group stack.
    ///
    /// Kept separate from `state_name_by_node` so a delay-node `SigId` and a
    /// `(group, index)` pair can coexist safely even when they refer to the
    /// same signal (tf22 pattern).
    recursion: RecursionState,
    /// Guards against emitting duplicate state-update stores for shared nodes.
    scheduled_state_updates: HashSet<SigId>,
    /// Delay-line exclusive state: allocated ring buffers, recursion-merge
    /// table, and write-scheduling dedup guard.  See [`DelayManager`].
    delay: DelayManager,
    /// `true` once the shared global circular cursor (`fIOTA`) has been
    /// declared; prevents duplicate declarations across delay and recursion
    /// lowering paths.
    uses_iota: bool,
    /// Maps each `ControlId` to its generated `FaustFloat` zone variable name.
    ui_controls: HashMap<ControlId, String>,
    /// Maps each soundfile `ControlId` to its generated opaque zone variable name.
    soundfiles: HashMap<ControlId, String>,
    /// Maps each waveform/table signal to its generated table variable name.
    waveform_tables: HashMap<SigId, String>,
    /// Maps each waveform/table signal to its element count.
    waveform_table_len: HashMap<SigId, usize>,
    /// Maps each waveform/table signal to the FIR storage class used for access.
    table_access_by_sig: HashMap<SigId, AccessType>,
    /// `buildUserInterface` body: open/close box and add-control calls.
    ui_statements: Vec<FirId>,
    /// Dedup guard for named struct-var declarations (prevents double-emit).
    named_struct_vars: HashSet<String>,
    /// Dedup guard for `instanceResetUserInterface` assignments.
    reset_init_seen: HashSet<String>,
    /// Dedup guard for `instanceClear` assignments and loops.
    clear_init_seen: HashSet<String>,
    /// Maps input channel index to its generated stack pointer-alias name.
    input_ptr_aliases: HashMap<usize, String>,
    /// Set of math operations used; drives prototype emission order.
    used_math_ops: HashSet<FirMathOp>,
    /// Set of integer helper function names used (`abs`, `min_i`, `max_i`).
    used_int_fun_names: HashSet<&'static str>,
    /// Extern prototypes requested by `SIGFFUN` lowering, keyed by callee name.
    used_foreign_fun_protos: BTreeMap<String, ForeignFunProto>,
    /// Extern globals requested by `SIGFVAR` lowering, keyed by symbol name.
    used_foreign_vars: BTreeMap<String, FirType>,
    /// Monotonic counter for generating unique loop-variable names.
    next_loop_var_id: usize,
    /// Monotonic counter for `fConst*` init-time float constant variable names.
    fconst_counter: u32,
    /// Monotonic counter for `iConst*` init-time integer constant variable names.
    iconst_counter: u32,
    /// Monotonic counter for `fSlow*` block-rate float variable names.
    fslow_counter: u32,
    /// Monotonic counter for `iSlow*` block-rate integer variable names.
    islow_counter: u32,
    /// Signal-level reference counts: how many parent nodes reference each `SigId`.
    ///
    /// Used by Phase 1 variability-driven placement to gate materialization:
    /// only nodes with `ref_count >= 2` are hoisted into a named variable.
    /// Single-use nodes stay inline, avoiding unnecessary temporaries.
    sig_ref_counts: HashMap<SigId, usize>,
    /// Signal nodes that sit at a variability boundary (at least one parent has
    /// strictly higher variability).  These must be materialized even if
    /// single-use, to ensure they execute in the correct bucket.
    sig_at_boundary: HashSet<SigId>,
    /// `Konst` signal nodes whose value is consumed outside `instanceConstants`.
    ///
    /// These hoists need persistent `Struct` storage; init-only `Konst` hoists
    /// can stay stack-local inside `instanceConstants()`.
    konst_escapes: HashSet<SigId>,
    /// Forward output lanes already computed before the reverse-time loop.
    ///
    /// Phase-E1 RAD uses the public bundle layout `[primals..., gradients...]`.
    /// This map lets coefficient-gradient terms in the reverse loop replay
    /// `Delay1(primal)` from the primal output buffer instead of reading the
    /// recursion carrier in reverse-time order.
    forward_output_by_sig: HashMap<SigId, usize>,
    /// Same map as [`Self::forward_output_by_sig`], keyed by the prepared
    /// readable signal shape to survive equivalent but non-identical `SigId`s.
    forward_output_by_sig_key: HashMap<String, usize>,
    /// True while lowering the reverse-time sample-loop slice.
    lowering_reverse_loop: bool,
    /// Guards against re-emitting the backward sweep for a `SigBlockReverseAD`
    /// group that has already been scheduled.  Keyed by the group `SigId`.
    bra_state_scheduled: HashSet<SigId>,
    /// Per-seed gradient `FirId` cache for emitted `SigBlockReverseAD` sweeps.
    ///
    /// Key: `(group_sig, seed_index)` where `seed_index` is the position of
    /// the seed in the carrier's seed list.  Populated by
    /// `ensure_bra_backward_sweep` and consumed by `lower_block_reverse_ad_proj`.
    bra_grad_cache: HashMap<(SigId, usize), FirId>,
    /// Carry variable names for `Delay1` nodes encountered inside a
    /// `SigBlockReverseAD` backward sweep.  Keyed by the `Delay1` node `SigId`.
    ///
    /// Each carry variable persists in the DSP struct and is zeroed by
    /// `emit_bra_compute_resets` before every reverse sample loop so that
    /// no adjoint state leaks across host `compute()` calls.
    bra_delay1_carry_vars: HashMap<SigId, String>,
    /// Carry array variable names and sizes for `Delay(c, x)` nodes (c > 1)
    /// encountered inside a `SigBlockReverseAD` backward sweep.
    ///
    /// Key: `Delay` node `SigId`.  Value: `(name, c)` where `name` is the
    /// struct-field name of the `Array(real_ty, c)` circular carry buffer.
    ///
    /// The carry implements the anti-causal adjoint: at reverse step n,
    /// `carry[n % c]` holds `adj[y][n + c]` from the previous c-th reverse
    /// step, contributing `adj[x][n] += carry[n % c]`.  The buffer is zeroed
    /// by `emit_bra_compute_resets` before each reverse sample loop.
    bra_delay_array_carry_vars: HashMap<SigId, (String, usize)>,
    /// Tape array variable names for signals recorded during the forward loop.
    ///
    /// Key: signal `SigId` whose forward value must be replayed in the reverse
    /// loop.  Value: the struct-field name of the `Array(real_ty,
    /// MAX_BRA_TAPE_BLOCK_SIZE)` used to store/load it.
    ///
    /// Populated by `ensure_bra_tape_stores` and consumed by
    /// `load_bra_fwd_value`.  Acts as a per-signal idempotency guard: a
    /// signal is never taped twice even when `ensure_bra_tape_stores` is
    /// called once per primal body slot.
    bra_tape_store_var: HashMap<SigId, String>,
}

/// One extern prototype recovered from a Faust `FFUN(...)` descriptor.
///
/// Source provenance (C++):
/// - `compiler/signals/prim2.cpp` (`ffname`, `ffrestype`, `ffargtype`)
#[derive(Clone, Debug, PartialEq)]
struct ForeignFunProto {
    name: String,
    ret: FirType,
    args: Vec<FirType>,
}

/// Matches one raw Faust `FFUN(signature, incfile, libfile)` descriptor node.
fn match_ffunction_node(arena: &TreeArena, id: SigId) -> Option<(SigId, SigId, SigId)> {
    let node = arena.node(id)?;
    let NodeKind::Tag(tag_id) = node.kind else {
        return None;
    };
    if arena.tag_name(tag_id)? != "FFUN" {
        return None;
    }
    let [signature, incfile, libfile] = node.children.as_slice() else {
        return None;
    };
    Some((*signature, *incfile, *libfile))
}

impl<'a> SignalToFirLower<'a> {
    /// Creates a fresh lowering state for one [`build_module`] call.
    #[allow(clippy::too_many_arguments)]
    fn new(
        arena: &'a TreeArena,
        ui_program: &'a UiProgram,
        types: &'a HashMap<SigId, SimpleSigType>,
        sig_types: &'a HashMap<SigId, SigType>,
        num_inputs: usize,
        real_ty: FirType,
        sig_ref_counts: HashMap<SigId, usize>,
        sig_at_boundary: HashSet<SigId>,
        konst_escapes: HashSet<SigId>,
        delay_opts: DelayOptions,
    ) -> Self {
        Self {
            arena,
            ui_program,
            types,
            sig_types,
            num_inputs,
            real_ty,
            store: FirStore::new(),
            cache: HashMap::new(),
            struct_declarations: Vec::new(),
            static_declarations: Vec::new(),
            global_declarations: Vec::new(),
            constants_statements: Vec::new(),
            reset_statements: Vec::new(),
            clear_statements: Vec::new(),
            control_statements: Vec::new(),
            sample_phases: SamplePhases::default(),
            state_name_by_node: HashMap::new(),
            recursion: RecursionState::default(),
            scheduled_state_updates: HashSet::new(),
            delay: DelayManager::new(delay_opts),
            uses_iota: false,
            ui_controls: HashMap::new(),
            soundfiles: HashMap::new(),
            waveform_tables: HashMap::new(),
            waveform_table_len: HashMap::new(),
            table_access_by_sig: HashMap::new(),
            ui_statements: Vec::new(),
            named_struct_vars: HashSet::new(),
            reset_init_seen: HashSet::new(),
            clear_init_seen: HashSet::new(),
            input_ptr_aliases: HashMap::new(),
            used_math_ops: HashSet::new(),
            used_int_fun_names: HashSet::new(),
            used_foreign_fun_protos: BTreeMap::new(),
            used_foreign_vars: BTreeMap::new(),
            next_loop_var_id: 0,
            fconst_counter: 0,
            iconst_counter: 0,
            fslow_counter: 0,
            islow_counter: 0,
            sig_ref_counts,
            sig_at_boundary,
            konst_escapes,
            forward_output_by_sig: HashMap::new(),
            forward_output_by_sig_key: HashMap::new(),
            lowering_reverse_loop: false,
            bra_state_scheduled: HashSet::new(),
            bra_grad_cache: HashMap::new(),
            bra_delay1_carry_vars: HashMap::new(),
            bra_delay_array_carry_vars: HashMap::new(),
            bra_tape_store_var: HashMap::new(),
        }
    }

    /// Ensures the canonical DSP sample-rate field is present in the FIR struct.
    ///
    /// Backends should consume this field directly instead of synthesizing their
    /// own `fSampleRate` side channel.
    fn ensure_sample_rate_var(&mut self) {
        self.ensure_named_struct_var("fSampleRate", FirType::Int32, None);
    }

    /// Pre-scans the output signal forest and allocates all delay lines before
    /// lowering begins.
    ///
    /// This preparation step now has two phases:
    ///
    /// - [`DelayManager::analyze_signals`] computes read-only accumulated delay
    ///   metadata for reachable signals and recursion outputs
    /// - [`DelayManager::scan_signals`] collects the concrete non-recursive
    ///   carried signals that still need standalone delay-line allocation
    ///
    /// Multiple `SIGDELAY(x, n)` nodes sharing the same carried signal `x`
    /// reuse one delay line sized to the largest delay seen. Standalone
    /// `Delay1(x)` nodes that use the shift strategy are included in the same
    /// pre-pass so delay-line geometry is decided exactly once up front.
    ///
    /// Recursion carriers are not allocated here directly; their size is
    /// planned by the accumulated delay analysis and consumed later by
    /// `ensure_recursion_array_for_group`.
    ///
    /// This pre-pass ensures all resource-sizing decisions are registered
    /// before reads are emitted during lowering.
    fn prepare_delay_lines(&mut self, outputs: &[SigId]) -> Result<(), SignalFirError> {
        self.delay
            .analyze_signals(self.arena, self.sig_types, outputs)?;
        let max_delays = self
            .delay
            .scan_signals(self.arena, self.sig_types, outputs)?;
        for (carried, delay) in max_delays {
            self.ensure_delay_line_decl(carried, delay)?;
        }
        Ok(())
    }

    /// Returns a clone of the internal real computation type.
    ///
    /// Use this whenever a FIR node must carry the internal scalar precision
    /// (arithmetic result, state slot, math call, real constant, …).
    /// For external interface points (audio buffer samples, UI zone variables)
    /// use `FirType::FaustFloat` directly instead.
    fn real_ty(&self) -> FirType {
        self.real_ty.clone()
    }

    // ── Variability-driven statement placement (Phase 1) ──────────────────

    /// Returns the signal-level variability for a node, if type info exists.
    ///
    /// Variability drives the execution-tier placement of the resulting FIR
    /// expression:
    /// - [`Variability::Konst`] → `constants_statements` (once at init)
    /// - [`Variability::Block`] → `control_statements` (once per `compute()`)
    /// - [`Variability::Samp`]  → sample-loop immediate phase
    fn variability_of(&self, sig: SigId) -> Option<Variability> {
        self.sig_types.get(&sig).map(|t| t.variability())
    }

    /// Returns `true` when a hoisted `Konst` value must remain persistent
    /// beyond `instanceConstants()`.
    fn konst_escapes(&self, sig: SigId) -> bool {
        self.konst_escapes.contains(&sig)
    }

    /// Returns the typed prefix used for one materialized scalar value.
    fn typed_prefix_for(bucket: Bucket, typ: &FirType) -> &'static str {
        let is_int_like = matches!(typ, FirType::Int32 | FirType::Int64 | FirType::Bool);
        match (bucket, is_int_like) {
            (Bucket::Constants, true) => "iConst",
            (Bucket::Constants, false) => "fConst",
            (Bucket::Control, true) => "iSlow",
            (Bucket::Control, false) => "fSlow",
        }
    }

    /// Returns the next numeric suffix for one typed materialization prefix.
    fn next_materialized_counter(&mut self, prefix: &str) -> u32 {
        match prefix {
            "fConst" => {
                let n = self.fconst_counter;
                self.fconst_counter += 1;
                n
            }
            "iConst" => {
                let n = self.iconst_counter;
                self.iconst_counter += 1;
                n
            }
            "fSlow" => {
                let n = self.fslow_counter;
                self.fslow_counter += 1;
                n
            }
            "iSlow" => {
                let n = self.islow_counter;
                self.islow_counter += 1;
                n
            }
            other => panic!("unsupported materialized prefix `{other}`"),
        }
    }

    /// Returns `true` when the signal is a direct `Proj(i, SYMREC)` read.
    ///
    /// The type system (after the `update_rec_types` variability-join fix)
    /// guarantees that such nodes always carry at least `Samp` variability, so
    /// they would not be hoisted by the placement logic anyway.  This guard is
    /// kept as a defensive check against future regressions.
    fn is_recursive_projection(&self, sig: SigId) -> bool {
        if let SigMatch::Proj(_, group) = match_sig(self.arena, sig) {
            let group = match match_sig(self.arena, group) {
                SigMatch::ReverseTimeRec(body) => body,
                _ => group,
            };
            match_sym_rec(self.arena, group).is_some()
                || match_sym_ref(self.arena, group).is_some()
                || tlib::match_de_bruijn_ref(self.arena, group).is_some()
        } else {
            false
        }
    }

    /// Materializes a FIR value expression into a named variable in the
    /// given execution-tier bucket.
    ///
    /// Returns a [`FirId`] for the `LoadVar` that reads the materialized
    /// variable.  The corresponding `DeclareVar` (with initializer) is
    /// appended to the appropriate lifecycle accumulator:
    ///
    /// | Bucket | Prefix | Access | Lifecycle section |
    /// |--------|--------|--------|-------------------|
    /// | `Constants` | `iConst` / `fConst` | [`AccessType::Stack`] for init-local, [`AccessType::Struct`] for escaping values | `instanceConstants` |
    /// | `Control` | `iSlow` / `fSlow` | [`AccessType::Stack`] | `compute` preamble |
    ///
    /// `Konst` variables that feed `compute()` use struct storage because they
    /// are written in `instanceConstants()` and read later; init-only `Konst`
    /// temporaries and all `Block` variables stay stack-local.
    fn materialize_in_bucket(&mut self, sig: SigId, value: FirId, bucket: Bucket) -> FirId {
        let typ = self
            .store
            .value_type(value)
            .unwrap_or_else(|| self.real_ty());
        let prefix = Self::typed_prefix_for(bucket, &typ);
        let n = self.next_materialized_counter(prefix);
        let access = match bucket {
            Bucket::Constants if self.konst_escapes(sig) => AccessType::Struct,
            Bucket::Constants | Bucket::Control => AccessType::Stack,
        };
        let name = format!("{prefix}{n}");

        match bucket {
            Bucket::Constants if access == AccessType::Struct => {
                self.ensure_named_struct_var(&name, typ.clone(), None);
                let mut b = FirBuilder::new(&mut self.store);
                self.constants_statements
                    .push(b.store_var(&name, AccessType::Struct, value));
            }
            Bucket::Constants => {
                let mut b = FirBuilder::new(&mut self.store);
                self.constants_statements.push(b.declare_var(
                    &name,
                    typ.clone(),
                    AccessType::Stack,
                    Some(value),
                ));
            }
            Bucket::Control => {
                let mut b = FirBuilder::new(&mut self.store);
                self.control_statements.push(b.declare_var(
                    &name,
                    typ.clone(),
                    AccessType::Stack,
                    Some(value),
                ));
            }
        }

        let mut b = FirBuilder::new(&mut self.store);
        b.load_var(name, access, typ)
    }

    /// Returns the reduced prepared signal type attached to one signal node.
    ///
    /// The fast-lane relies on the pre-FIR `signal_prepare` boundary to decide
    /// whether one value/state/table should stay integer or use the internal
    /// real computation type, mirroring the reduced
    /// `deBruijn2Sym -> typeAnnotation -> signalPromote` contract.
    fn simple_type(&self, sig: SigId) -> Result<SimpleSigType, SignalFirError> {
        self.types.get(&sig).copied().ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("missing prepared type for signal {}", sig.as_u32()),
            )
        })
    }

    /// Maps one prepared signal type to the FIR value type used by lowering.
    fn signal_fir_type(&self, sig: SigId) -> Result<FirType, SignalFirError> {
        match self.simple_type(sig)? {
            SimpleSigType::Int => Ok(FirType::Int32),
            SimpleSigType::Real => Ok(self.real_ty()),
            SimpleSigType::Sound => Ok(FirType::Sound),
        }
    }

    /// Returns the typed zero initializer used for state slots and table
    /// declarations.
    fn zero_value_for_signal(&mut self, sig: SigId) -> Result<FirId, SignalFirError> {
        match self.simple_type(sig)? {
            SimpleSigType::Int => Ok(self.lower_int32_const(0)),
            SimpleSigType::Real => Ok(self.float_const(0.0)),
            SimpleSigType::Sound => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "signal {} cannot use a soundfile handle as delay/table state",
                    sig.as_u32()
                ),
            )),
        }
    }
}

// ── Core signal lowering: dispatch, foreign/runtime leaves, and delays ─────

impl<'a> SignalToFirLower<'a> {
    /// Central dispatcher: lowers one signal node to a FIR value expression.
    ///
    /// Results are memoized in [`Self::cache`] for DAG sharing.  As a side
    /// effect, successful lowering may append declarations and assignments to
    /// lifecycle section accumulators (e.g. sample-loop phase statements,
    /// state declarations to
    /// [`Self::struct_declarations`]).
    ///
    /// Returns a typed `FRS-SFIR-*` error for unsupported signal families.
    fn lower_signal(&mut self, sig: SigId) -> Result<FirId, SignalFirError> {
        if let Some(id) = self.cache.get(&sig).copied() {
            return Ok(id);
        }

        let lowered = match match_sig(self.arena, sig) {
            SigMatch::Int(value) => self.lower_int32_const(value),
            // Real constant: emitted at internal precision (Float32 or Float64).
            SigMatch::Real(value) => self.float_const(value),
            SigMatch::Input(index) => self.lower_input(index)?,
            SigMatch::Output(_, inner) => self.lower_signal(inner)?,
            SigMatch::Delay1(value) => {
                // Recursion delay chains that ultimately read from an active
                // recursion carrier are lowered through that carrier directly.
                // Standalone Delay1 nodes keep using the dedicated fast path
                // when the shift strategy is enabled.
                if self.resolve_recursion_delay_ref(value)?.is_none()
                    && self.delay.max_copy_delay() >= 1
                {
                    self.lower_shift_delay1(sig, value)?
                } else {
                    let init = self.zero_value_for_signal(sig)?;
                    self.lower_delay_state(sig, value, init)?
                }
            }
            SigMatch::Delay(value, amount) => self.lower_delay(sig, value, amount)?,
            SigMatch::Prefix(init_sig, value) => {
                let init = self.initial_state_from_signal(init_sig);
                self.lower_delay_state(sig, value, init)?
            }
            SigMatch::IntCast(value) => self.lower_cast(FirType::Int32, value)?,
            // BitCast and FloatCast convert to the internal real type, not to
            // FaustFloat: they are integer↔float reinterpretation/coercion
            // operations used in internal DSP computation.
            SigMatch::BitCast(value) => self.lower_bitcast(self.real_ty(), value)?,
            SigMatch::FloatCast(value) => self.lower_cast(self.real_ty(), value)?,
            SigMatch::Select2(cond, else_value, then_value) => {
                self.lower_select2(sig, cond, then_value, else_value)?
            }
            SigMatch::Proj(index, group) => self.lower_proj(sig, index, group)?,
            SigMatch::BinOp(op, lhs, rhs) => self.lower_binop(sig, op, lhs, rhs)?,
            SigMatch::Pow(lhs, rhs) => self.lower_math2(FirMathOp::Pow, lhs, rhs)?,
            SigMatch::Min(lhs, rhs) => self.lower_minmax(sig, lhs, rhs, true)?,
            SigMatch::Max(lhs, rhs) => self.lower_minmax(sig, lhs, rhs, false)?,
            SigMatch::Sin(value) => self.lower_math1(FirMathOp::Sin, value)?,
            SigMatch::Cos(value) => self.lower_math1(FirMathOp::Cos, value)?,
            SigMatch::Acos(value) => self.lower_math1(FirMathOp::Acos, value)?,
            SigMatch::Asin(value) => self.lower_math1(FirMathOp::Asin, value)?,
            SigMatch::Atan(value) => self.lower_math1(FirMathOp::Atan, value)?,
            SigMatch::Atan2(lhs, rhs) => self.lower_math2(FirMathOp::Atan2, lhs, rhs)?,
            SigMatch::Tan(value) => self.lower_math1(FirMathOp::Tan, value)?,
            SigMatch::Exp(value) => self.lower_math1(FirMathOp::Exp, value)?,
            SigMatch::Log(value) => self.lower_math1(FirMathOp::Log, value)?,
            SigMatch::Log10(value) => self.lower_math1(FirMathOp::Log10, value)?,
            SigMatch::Sqrt(value) => self.lower_math1(FirMathOp::Sqrt, value)?,
            SigMatch::Abs(value) => self.lower_abs(sig, value)?,
            SigMatch::Fmod(lhs, rhs) => self.lower_math2(FirMathOp::Fmod, lhs, rhs)?,
            SigMatch::Remainder(lhs, rhs) => self.lower_math2(FirMathOp::Remainder, lhs, rhs)?,
            SigMatch::Floor(value) => self.lower_math1(FirMathOp::Floor, value)?,
            SigMatch::Ceil(value) => self.lower_math1(FirMathOp::Ceil, value)?,
            SigMatch::Rint(value) => self.lower_math1(FirMathOp::Rint, value)?,
            SigMatch::Round(value) => self.lower_math1(FirMathOp::Round, value)?,
            SigMatch::Lowest(value) => self.lower_signal(value)?,
            SigMatch::Highest(value) => self.lower_signal(value)?,
            SigMatch::FConst(_, name, _) => self.lower_fconst(sig, name)?,
            SigMatch::RdTbl(tbl, ridx) => self.lower_rdtbl(sig, tbl, ridx)?,
            SigMatch::WrTbl(size, generator, widx, wsig) => {
                self.lower_wrtbl(sig, size, generator, widx, wsig)?
            }
            SigMatch::Waveform(values) => self.lower_waveform(sig, values)?,
            SigMatch::Button(control) => self.lower_button(control, ButtonType::Button)?,
            SigMatch::Checkbox(control) => self.lower_button(control, ButtonType::Checkbox)?,
            SigMatch::VSlider(control) => self.lower_slider(control, SliderType::Vertical)?,
            SigMatch::HSlider(control) => self.lower_slider(control, SliderType::Horizontal)?,
            SigMatch::NumEntry(control) => self.lower_slider(control, SliderType::NumEntry)?,
            SigMatch::VBargraph(control, value) => {
                self.lower_bargraph(control, value, BargraphType::Vertical)?
            }
            SigMatch::HBargraph(control, value) => {
                self.lower_bargraph(control, value, BargraphType::Horizontal)?
            }
            SigMatch::Attach(lhs, rhs) => {
                let _ = self.lower_signal(rhs)?;
                self.lower_signal(lhs)?
            }
            SigMatch::Enable(lhs, rhs) => {
                let zero = self.zero_value_for_signal(sig)?;
                let lhs = self.lower_signal(lhs)?;
                let cond = self.lower_signal(rhs)?;
                let real_ty = self.signal_fir_type(sig)?;
                let mut b = FirBuilder::new(&mut self.store);
                b.select2(cond, lhs, zero, real_ty)
            }
            SigMatch::Control(lhs, rhs) => {
                let _ = self.lower_signal(rhs)?;
                self.lower_signal(lhs)?
            }
            SigMatch::FFun(ff, largs) => self.lower_ffun(sig, ff, largs)?,
            SigMatch::FVar(kind, name, file) => self.lower_fvar(sig, kind, name, file)?,
            SigMatch::Soundfile(control) => self.lower_soundfile(control)?,
            SigMatch::SoundfileLength(sf, part) => self.lower_soundfile_length(sf, part)?,
            SigMatch::SoundfileRate(sf, part) => self.lower_soundfile_rate(sf, part)?,
            SigMatch::SoundfileBuffer(sf, chan, part, ridx) => {
                self.lower_soundfile_buffer(sig, sf, chan, part, ridx)?
            }
            other => {
                return Err(SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!(
                        "unsupported signal node in Step 2C: {other:?} (expr={})",
                        dump_sig_readable(self.arena, sig)
                    ),
                ));
            }
        };

        // ── Variability-driven placement (Phase 1) ──────────────────────
        //
        // Non-trivial expressions whose variability is slower than sample
        // rate are hoisted into the appropriate execution-tier bucket:
        //   Konst → constants_statements (instanceConstants, once at init)
        //   Block → control_statements   (compute preamble, once per call)
        //   Samp  → stays inline in the sample loop (no action needed)
        //
        // To avoid creating unnecessary temporaries for intermediate
        // sub-expressions, only nodes referenced ≥ 2 times in the signal
        // DAG are materialized into named variables (`iConst*`/`fConst*`,
        // `iSlow*`/`fSlow*`).
        // Single-use nodes at the same variability tier stay inline inside
        // their parent's expression.  This matches C++ Faust behavior
        // where compound expressions like `fConst6 * cos(fConst7 * fSlow2)`
        // are emitted as one variable instead of three.
        //
        // However, at a **variability boundary** (Block→Samp or
        // Konst→Block/Samp), even single-use nodes must be materialized
        // to ensure they execute in the correct bucket.  Without this,
        // a single-use Block-rate sub-expression of a Samp parent would
        // be inlined into the per-sample loop body, re-evaluated every
        // sample.
        //
        // Guards:
        // - Trivial nodes (literals, loads) are never hoisted — they are
        //   free to duplicate and hoisting them wastes a variable name.
        // - Recursive projections must stay in the sample loop; the type
        //   system ensures they are always Samp, but the guard is kept as
        //   a defensive check.
        // - SIGWRTBL nodes: the type system assigns Konst variability
        //   (from `make_table_type`) reflecting the static table content,
        //   but `lower_wrtbl` returns the write signal's value which may
        //   reference Samp-rate state (e.g. `iWave*` cycling counters).
        //   Hoisting would place `LoadVar("iWave*")` inside
        //   `instanceConstants`, before `instanceClear` has initialized it.
        let sig_shared = self.sig_ref_counts.get(&sig).copied().unwrap_or(0) >= 2;
        let at_boundary = self.sig_at_boundary.contains(&sig);
        let lowered = if !is_trivial_fir(&self.store, lowered)
            && !self.is_recursive_projection(sig)
            && !matches!(match_sig(self.arena, sig), SigMatch::WrTbl(..))
            && (sig_shared || at_boundary)
        {
            match self.variability_of(sig) {
                Some(Variability::Konst) => {
                    self.materialize_in_bucket(sig, lowered, Bucket::Constants)
                }
                Some(Variability::Block) => {
                    self.materialize_in_bucket(sig, lowered, Bucket::Control)
                }
                _ => lowered,
            }
        } else {
            lowered
        };

        if !self.is_recursive_projection(sig) {
            self.cache.insert(sig, lowered);
        }
        Ok(lowered)
    }

    /// Lowers one top-level signal into the currently active sample-loop
    /// accumulator.
    ///
    /// The caller controls which sample loop is active by clearing
    /// [`Self::sample_phases`] between forward and reverse scheduling slices.
    /// Output signals are cast at the external FaustFloat boundary and stored
    /// into `outputN[i0]`; non-output surplus signals are evaluated and dropped.
    fn lower_output_signal(
        &mut self,
        signal_index: usize,
        sig: SigId,
        num_outputs: usize,
    ) -> Result<(), SignalFirError> {
        let mut value = self.lower_signal(sig)?;
        if signal_index < num_outputs {
            let needs_output_cast = self.store.value_type(value) != Some(FirType::FaustFloat);
            let mut b = FirBuilder::new(&mut self.store);
            if needs_output_cast {
                value = b.cast(FirType::FaustFloat, value);
            }
            let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
            self.sample_phases.immediate.push(b.store_table(
                format!("output{signal_index}"),
                AccessType::Stack,
                i0,
                value,
            ));
        } else {
            let mut b = FirBuilder::new(&mut self.store);
            self.sample_phases.immediate.push(b.drop_(value));
        }
        Ok(())
    }

    /// Clears per-loop scheduling state before building another sample loop.
    fn reset_sample_loop_state(&mut self) {
        self.sample_phases = SamplePhases::default();
        self.scheduled_state_updates.clear();
        self.recursion.scheduled_groups.clear();
    }

    /// Lowers supported foreign constants.
    ///
    /// Active parity slice mirrors the C++ fast-lane special-case for
    /// `fSamplingFreq`, which loads the persistent `fSampleRate` struct field.
    ///
    /// `fSamplingFreq` is typed as Int in the signal domain, so its FIR type is
    /// always `Int32`.  If it appears in a Real context the promoter wraps it in a
    /// `FloatCast` node, which is lowered separately by `lower_cast`.  No implicit
    /// cast is needed here.
    fn lower_fconst(&mut self, sig: SigId, name: SigId) -> Result<FirId, SignalFirError> {
        let name = self.label_text(name);
        if name == "fSamplingFreq" || name == "fSamplingRate" {
            // The Faust runtime stores the sample rate as a 32-bit integer
            // (`fSampleRate` struct field, type `int`).  However, when the
            // Faust signal tree uses this constant in floating-point arithmetic
            // (e.g. `si.smoo` → `tau2pole` → `exp(-2π/ma.SR)`), the prepared
            // type of the FConst node is `Real`.  Emitting a bare `Int32` load
            // there causes a FIR type-mismatch error at verify time.
            //
            // Fix: load as `Int32` and then cast to the expected FIR type when
            // the signal's prepared type is `Real`.
            let int_val = {
                let mut b = FirBuilder::new(&mut self.store);
                b.load_var("fSampleRate", AccessType::Struct, FirType::Int32)
            };
            let expected_ty = self.signal_fir_type(sig)?;
            if expected_ty == FirType::Int32 {
                return Ok(int_val);
            }
            let mut b = FirBuilder::new(&mut self.store);
            return Ok(b.cast(expected_ty, int_val));
        }
        self.unsupported_node(
            sig,
            &format!("unsupported foreign constant `{name}` in Step 2C"),
        )
    }

    /// Lowers one foreign variable load.
    ///
    /// Active parity slice mirrors `InstructionsCompiler::generateFVar`:
    /// - `count` is a special Faust runtime symbol (`fFullCount` in the C++
    ///   generator), not a normal extern. In scalar `compute(int count, ...)`
    ///   codegen it denotes the current block size, so we must lower it to the
    ///   existing FIR function argument rather than emitting a separate global.
    /// - any other foreign variable is treated as an extern global and loaded
    ///   through `AccessType::Global`, with one declaration emitted per symbol.
    ///
    /// Source provenance (C++):
    /// - `compiler/generator/instructions_compiler.cpp` (`generateFVar`)
    fn lower_fvar(
        &mut self,
        _sig: SigId,
        kind: SigId,
        name: SigId,
        _file: SigId,
    ) -> Result<FirId, SignalFirError> {
        let name = self.label_text(name);
        let typ = self.foreign_sig_type(kind);
        let mut b = FirBuilder::new(&mut self.store);

        if name == "count" {
            return Ok(b.load_var(name, AccessType::FunArgs, typ));
        }

        if !self.used_foreign_vars.contains_key(&name) {
            let decl = b.declare_var(name.to_owned(), typ.clone(), AccessType::Global, None);
            self.global_declarations.push(decl);
            self.used_foreign_vars.insert(name.to_owned(), typ.clone());
        }

        Ok(b.load_var(name, AccessType::Global, typ))
    }

    /// Lowers one foreign function call to a FIR `FunCall` plus extern prototype.
    ///
    /// Source provenance (C++):
    /// - `compiler/signals/prim2.cpp` (`ffname`, `ffrestype`, `ffargtype`)
    /// - `compiler/generator/instructions_compiler.cpp` (`generateFFun`)
    fn lower_ffun(&mut self, sig: SigId, ff: SigId, largs: SigId) -> Result<FirId, SignalFirError> {
        let proto = self.decode_foreign_fun_proto(ff)?;
        let args = list_to_vec(self.arena, largs).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "malformed SIGFFUN argument list in Step 2C (expr={})",
                    dump_sig_readable(self.arena, sig)
                ),
            )
        })?;
        if args.len() != proto.args.len() {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "foreign function `{}` arity mismatch in Step 2C: expected {}, got {}",
                    proto.name,
                    proto.args.len(),
                    args.len()
                ),
            ));
        }

        let mut lowered_args = Vec::with_capacity(args.len());
        for arg in args {
            lowered_args.push(self.lower_signal(arg)?);
        }
        self.used_foreign_fun_protos
            .entry(proto.name.clone())
            .or_insert_with(|| proto.clone());

        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.fun_call(proto.name, &lowered_args, proto.ret))
    }

    /// Decodes one Faust `FFUN(signature, incfile, libfile)` descriptor.
    /// Extracts a [`ForeignFunProto`] from a Faust `FFUN(signature, _, _)` descriptor.
    ///
    /// The `signature` list has the layout `[ret_type, [name_f32, name_f64], arg0_type, …]`:
    /// index 0 is the return type code, index 1 is the name list (0=float32 name,
    /// 1=float64 name), and indices 2+ are argument type codes.  Type codes follow
    /// `foreign_sig_type`: `0` → `Int32`, any other value → `real_ty`.
    fn decode_foreign_fun_proto(&self, ff: SigId) -> Result<ForeignFunProto, SignalFirError> {
        let Some((signature, _, _)) = match_ffunction_node(self.arena, ff) else {
            return self.unsupported_node(ff, "SIGFFUN descriptor is not an FFUNCTION node");
        };
        let items = list_to_vec(self.arena, signature).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "malformed foreign function signature list in Step 2C",
            )
        })?;
        if items.len() < 2 {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "foreign function signature list must contain return type and names",
            ));
        }
        let names = list_to_vec(self.arena, items[1]).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "malformed foreign function name list in Step 2C",
            )
        })?;
        let name_index = match self.real_ty() {
            FirType::Float32 => 0,
            FirType::Float64 => 1,
            _ => 0,
        };
        let name = names
            .get(name_index)
            .and_then(|id| tree_to_str(self.arena, *id))
            .ok_or_else(|| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    "foreign function name slot missing in Step 2C",
                )
            })?
            .to_owned();
        let ret = self.foreign_sig_type(items[0]);
        let args = items[2..]
            .iter()
            .copied()
            .map(|ty| self.foreign_sig_type(ty))
            .collect();
        Ok(ForeignFunProto { name, ret, args })
    }

    /// Decodes one Faust foreign signature type code (`0=int`, otherwise real).
    fn foreign_sig_type(&self, ty: SigId) -> FirType {
        match tree_to_int(self.arena, ty) {
            Some(0) => FirType::Int32,
            Some(_) | None => self.real_ty(),
        }
    }

    /// Lowers one input signal by materializing channel-pointer aliases once
    /// and generating a per-sample table load (`inputN[i0]`).
    fn lower_input(&mut self, index: i32) -> Result<FirId, SignalFirError> {
        let index = usize::try_from(index).map_err(|_| {
            SignalFirError::new(
                SignalFirErrorCode::InputIndexOutOfRange,
                "input index conversion overflow",
            )
        })?;
        if index >= self.num_inputs {
            return Err(SignalFirError::new(
                SignalFirErrorCode::InputIndexOutOfRange,
                format!(
                    "input index {index} is out of range for num_inputs={}",
                    self.num_inputs
                ),
            ));
        }

        let alias = if let Some(alias) = self.input_ptr_aliases.get(&index) {
            alias.clone()
        } else {
            let alias = format!("input{index}");
            let mut b = FirBuilder::new(&mut self.store);
            let chan = b.int32(i32::try_from(index).expect("validated input index fits i32"));
            let ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
            let load_chan_ptr = b.load_table("inputs", AccessType::FunArgs, chan, ptr_ty.clone());
            self.control_statements.push(b.declare_var(
                alias.clone(),
                ptr_ty,
                AccessType::Stack,
                Some(load_chan_ptr),
            ));
            self.input_ptr_aliases.insert(index, alias.clone());
            alias
        };

        // Load the sample from the external FAUSTFLOAT buffer, then cast to the
        // internal real type so all downstream computation uses real_ty.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let raw = b.load_table(alias, AccessType::Stack, i0, FirType::FaustFloat);
        Ok(b.cast(real_ty, raw))
    }

    /// Lowers general `SIGDELAY` using a fixed-size circular delay line.
    ///
    /// Source provenance (C++):
    /// - `signalFIRCompiler.cpp::compileSigDelay(...)`
    /// - `signalFIRCompiler.hh::writeReadDelay(...)`
    ///
    /// Active Rust parity slice:
    /// - constant integer amount only,
    /// - zero-delay fast path,
    /// - one typed DSP-struct array per delayed carried signal,
    /// - masked circular indexing driven by persistent `fIOTA`.
    ///
    /// For variable-rate amounts (e.g., UI sliders), the delay line is sized to
    /// the interval upper bound from `sig_types`; the runtime index expression
    /// is the lowered amount signal evaluated each sample.
    fn lower_delay(
        &mut self,
        node: SigId,
        value: SigId,
        amount: SigId,
    ) -> Result<FirId, SignalFirError> {
        match delay_size_for_amount(self.arena, self.sig_types, amount)? {
            Some(0) => self.lower_signal(value),
            Some(delay) => self.lower_fixed_delay(node, value, amount, delay),
            None => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "SIGDELAY requires a constant integer amount or a signal with a \
                     bounded non-negative interval (expr={})",
                    dump_sig_readable(self.arena, amount)
                ),
            )),
        }
    }

    /// Lowers a fixed-size `SIGDELAY(value, amount)` using the canonical delay
    /// line pre-allocated by [`Self::prepare_delay_lines`].
    ///
    /// Strategy-specific FIR emission is delegated to `delay.rs` through
    /// `emit_fixed_delay_for_line`, while this method keeps:
    ///
    /// - recursion-carrier reuse for merged `Delay1^k(Proj(...))` chains
    /// - evaluation of the runtime `amount` expression
    /// - per-carrier write scheduling
    fn lower_fixed_delay(
        &mut self,
        node: SigId,
        value: SigId,
        amount: SigId,
        delay: i32,
    ) -> Result<FirId, SignalFirError> {
        // ── Merged recursion delay ──
        //
        // When `value` is a `Delay1^k(Proj(i, active_group))` chain, the scan pass has
        // already sized the recursion array to hold the full delay chain.
        // Read directly from the recursion array at offset `amount + k`,
        // eliminating the separate fVec buffer and per-sample copy.
        if let Some(rec_delay_ref) = self.resolve_recursion_delay_ref(value)? {
            let total_delay =
                usize::try_from(delay).unwrap_or(usize::MAX) + rec_delay_ref.implicit_delay;
            match rec_delay_ref.carrier.strategy {
                RecursionStorageStrategy::Circular => {
                    // The recursion array was upsized — the merge is active.
                    // Use the runtime amount expression (which may be variable,
                    // e.g. slider-driven), not the constant sizing bound.
                    // Total offset = explicit amount + the carried implicit delay chain.
                    let amount_value = self.lower_signal(amount)?;
                    let carried_delay = self.lower_int32_const(
                        i32::try_from(rec_delay_ref.implicit_delay).unwrap_or(i32::MAX),
                    );
                    let total_offset = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.binop(FirBinOp::Add, amount_value, carried_delay, FirType::Int32)
                    };
                    let read_index = self.global_circular_delayed_index(
                        total_offset,
                        rec_delay_ref.carrier.info.size,
                    );
                    let read_ty = self.signal_fir_type(node)?;
                    let mut b = FirBuilder::new(&mut self.store);
                    return Ok(b.load_table(
                        rec_delay_ref.carrier.info.name,
                        AccessType::Struct,
                        read_index,
                        read_ty,
                    ));
                }
                RecursionStorageStrategy::SingleScalar if total_delay == 1 => {
                    let read_ty = self.signal_fir_type(node)?;
                    let mut b = FirBuilder::new(&mut self.store);
                    return Ok(b.load_var(
                        rec_delay_ref.carrier.info.name,
                        AccessType::Struct,
                        read_ty,
                    ));
                }
                RecursionStorageStrategy::ExactShift => {
                    let read_ty = self.signal_fir_type(node)?;
                    let prev_index =
                        self.lower_int32_const(i32::try_from(total_delay).unwrap_or(i32::MAX));
                    let mut b = FirBuilder::new(&mut self.store);
                    return Ok(b.load_table(
                        rec_delay_ref.carrier.info.name,
                        AccessType::Struct,
                        prev_index,
                        read_ty,
                    ));
                }
                RecursionStorageStrategy::SingleScalar => {}
            }
        }

        let line = self.delay_line_info(value)?;
        let current = self.lower_signal(value)?;
        let read_ty = self.signal_fir_type(node)?;
        let amount_value = self.lower_signal(amount)?;
        let schedule_write = self.delay.schedule_delay_write(value);
        let mut delay_ctx = DelayLoweringCtx {
            store: &mut self.store,
            immediate_statements: &mut self.sample_phases.immediate,
            post_output_statements: &mut self.sample_phases.post_output,
            next_loop_var_id: &mut self.next_loop_var_id,
        };
        Ok(emit_fixed_delay_for_line(
            &mut delay_ctx,
            &line,
            current,
            amount_value,
            read_ty,
            schedule_write,
        ))
    }

    /// Lowers one single-sample state edge (`delay1`/`prefix`).
    ///
    /// **Recursion feedback optimization**: if the carried `value` is
    /// `Proj(i, SYMREC/SYMREF)` pointing into the currently active recursion
    /// context (detected by `recursion_feedback_info`), the group's existing
    /// recursion array is reused directly — no separate state variable is
    /// allocated and no extra write is emitted.  The previous-sample value is
    /// read as `rec_array[(fIOTA - 1) & 1]`, which is always valid because the
    /// recursion body writes `rec_array[0]` earlier in the same sample and a
    /// deferred copy updates `rec_array[1]` after outputs are stored.
    ///
    /// For all other `value` signals the normal path applies:
    ///
    /// - Write: `state[fIOTA & 1] = next` (immediate, in sample body)
    /// - Read:  `state[(fIOTA - 1) & 1]`   (returns previous sample)
    fn lower_delay_state(
        &mut self,
        node: SigId,
        value: SigId,
        init: FirId,
    ) -> Result<FirId, SignalFirError> {
        if self.lowering_reverse_loop
            && let Some(replayed) =
                self.lower_forward_output_delay1_for_reverse_loop(node, value)?
        {
            return Ok(replayed);
        }
        if let Some(rec_delay_ref) = self.resolve_recursion_delay_ref(value)? {
            let out_ty = self.signal_fir_type(node)?;
            debug_assert_eq!(
                rec_delay_ref.carrier.info.typ, out_ty,
                "prepared recursion feedback type should match delay1 output type"
            );
            let total_offset = rec_delay_ref.implicit_delay.saturating_add(1);
            match rec_delay_ref.carrier.strategy {
                RecursionStorageStrategy::SingleScalar => {
                    debug_assert_eq!(
                        total_offset, 1,
                        "scalar recursion carriers must not serve delays beyond one sample"
                    );
                    let mut b = FirBuilder::new(&mut self.store);
                    return Ok(b.load_var(
                        rec_delay_ref.carrier.info.name,
                        AccessType::Struct,
                        rec_delay_ref.carrier.info.typ.clone(),
                    ));
                }
                RecursionStorageStrategy::ExactShift => {
                    let prev_index =
                        self.lower_int32_const(i32::try_from(total_offset).unwrap_or(i32::MAX));
                    let mut b = FirBuilder::new(&mut self.store);
                    return Ok(b.load_table(
                        rec_delay_ref.carrier.info.name,
                        AccessType::Struct,
                        prev_index,
                        rec_delay_ref.carrier.info.typ.clone(),
                    ));
                }
                RecursionStorageStrategy::Circular => {
                    let total_offset =
                        self.lower_int32_const(i32::try_from(total_offset).unwrap_or(i32::MAX));
                    let prev_index = self.global_circular_delayed_index(
                        total_offset,
                        rec_delay_ref.carrier.info.size,
                    );
                    let mut b = FirBuilder::new(&mut self.store);
                    return Ok(b.load_table(
                        rec_delay_ref.carrier.info.name,
                        AccessType::Struct,
                        prev_index,
                        rec_delay_ref.carrier.info.typ.clone(),
                    ));
                }
            }
        }
        let state_ty = self.signal_fir_type(value)?;
        let name = self.ensure_state_slot(node, state_ty.clone(), init);
        // Read previous value: state[(fIOTA - 1) & 1]
        let one = self.lower_int32_const(1);
        let read_index = self.global_circular_delayed_index(one, 2);
        let out = {
            let mut b = FirBuilder::new(&mut self.store);
            b.load_table(name.clone(), AccessType::Struct, read_index, state_ty)
        };
        // Write current value: state[fIOTA & 1] = next (immediate)
        if self.scheduled_state_updates.insert(node) {
            let next = self.lower_signal(value)?;
            let write_index = self.global_circular_current_index(2);
            let mut b = FirBuilder::new(&mut self.store);
            self.sample_phases.immediate.push(b.store_table(
                name,
                AccessType::Struct,
                write_index,
                next,
            ));
        }
        Ok(out)
    }

    /// Replays `Delay1(primal_output)` while lowering a reverse-time RAD loop.
    ///
    /// In split RAD bundles, forward primals are emitted before reverse
    /// gradients. A feedback-coefficient contribution such as
    /// `adjoint[n] * y[n-1]` must read the primal state at the matching forward
    /// frame, not advance a recursion carrier while iterating backward. For
    /// primals present in the public output bundle, the forward output buffer is
    /// the block-local tape: frame `0` returns the delay initializer `0`, and
    /// later frames read `output_primal[i0 - 1]`.
    fn lower_forward_output_delay1_for_reverse_loop(
        &mut self,
        node: SigId,
        value: SigId,
    ) -> Result<Option<FirId>, SignalFirError> {
        let output_index = self.forward_output_by_sig.get(&value).copied().or_else(|| {
            self.forward_output_by_sig_key
                .get(&dump_sig_readable(self.arena, value))
                .copied()
        });
        let Some(output_index) = output_index else {
            return Ok(None);
        };
        let out_ty = self.signal_fir_type(node)?;
        if !matches!(out_ty, FirType::Int32 | FirType::Float32 | FirType::Float64) {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "unsupported reverse RAD primal replay type for {}: {out_ty:?}",
                    dump_sig_readable(self.arena, node)
                ),
            ));
        }
        let mut b = FirBuilder::new(&mut self.store);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let zero_index = b.int32(0);
        let has_previous = b.binop(FirBinOp::Gt, i0, zero_index, FirType::Int32);
        let one = b.int32(1);
        let raw_previous_index = b.binop(FirBinOp::Sub, i0, one, FirType::Int32);
        let previous_index = b.binop(
            FirBinOp::Mul,
            has_previous,
            raw_previous_index,
            FirType::Int32,
        );
        let previous = b.load_table(
            format!("output{output_index}"),
            AccessType::Stack,
            previous_index,
            FirType::FaustFloat,
        );
        let previous = b.cast(out_ty.clone(), previous);
        let mask = b.cast(out_ty.clone(), has_previous);
        let masked_previous = b.binop(FirBinOp::Mul, previous, mask, out_ty);
        Ok(Some(masked_previous))
    }

    /// Lowers a standalone `Delay1(value)` node using the canonical
    /// preplanned strategy for its carried signal.
    ///
    /// When the carried signal owns a `Shift` delay line, this matches the
    /// reference C++ Faust pattern:
    /// ```text
    /// buf[0] = value;       // immediate write
    /// output = buf[1];      // read previous sample
    /// buf[1] = buf[0];      // deferred shift (after output stores)
    /// ```
    ///
    /// The same `Delay1(value)` may also reuse a preplanned `CircularPow2` or
    /// `IfWrapping` line when the carried signal shares storage with a larger
    /// `SIGDELAY(value, N)`. In all cases the concrete write/read sequence is
    /// delegated to `emit_delay1_for_line`.
    ///
    /// Only called when `max_copy_delay >= 1` and `value` is not a recursion
    /// feedback projection.
    fn lower_shift_delay1(&mut self, node: SigId, value: SigId) -> Result<FirId, SignalFirError> {
        let line = self.delay_line_info(value)?;
        let read_ty = self.signal_fir_type(node)?;
        let current = self.lower_signal(value)?;
        let schedule_write = self.delay.schedule_delay_write(value);
        let mut delay_ctx = DelayLoweringCtx {
            store: &mut self.store,
            immediate_statements: &mut self.sample_phases.immediate,
            post_output_statements: &mut self.sample_phases.post_output,
            next_loop_var_id: &mut self.next_loop_var_id,
        };
        Ok(emit_delay1_for_line(
            &mut delay_ctx,
            &line,
            current,
            read_ty,
            schedule_write,
        ))
    }
}

// ── Shared delay/recursion/state helpers used by lowering ──────────────────

impl<'a> SignalToFirLower<'a> {
    /// Returns the resolved recursion-delay reference for `value`.
    ///
    /// Examples:
    ///
    /// - `Proj(i, group)` → delay chain `0`
    /// - `Delay1(Proj(i, group))` → delay chain `1`
    /// - `Delay1(Delay1(Proj(i, group)))` → delay chain `2`
    ///
    /// Pure state-based resolution lives in `recursion.rs`; this wrapper only
    /// falls back to `lower_proj(...)` when a top-level `SYMREC` group still
    /// needs to be materialized in the current lowering pass.
    fn resolve_recursion_delay_ref(
        &mut self,
        value: SigId,
    ) -> Result<Option<RecursionDelayRef>, SignalFirError> {
        if let Some(delay_ref) = self.recursion.resolve_delay_ref(self.arena, value)? {
            return Ok(Some(delay_ref));
        }
        let Some(key) = match_recursion_delay_key(self.arena, value) else {
            return Ok(None);
        };
        let Some(rec_info) =
            self.resolve_recursion_carrier(key.proj_node, key.proj_index, key.group)?
        else {
            return Ok(None);
        };
        Ok(Some(RecursionDelayRef {
            carrier: rec_info,
            implicit_delay: key.implicit_delay,
        }))
    }

    /// Returns the canonical recursion carrier for `Proj(index, group)` whether
    /// the projection points to the active feedback reference (`SYMREF`) or to
    /// the materialized top-level recursion group (`SYMREC`).
    ///
    /// Pure active/materialized lookup lives in `recursion.rs`; this wrapper
    /// only performs top-level materialization when needed.
    fn resolve_recursion_carrier(
        &mut self,
        proj_node: SigId,
        index: i32,
        group: SigId,
    ) -> Result<Option<RecursionCarrierRef>, SignalFirError> {
        let index_usize = usize::try_from(index).map_err(|_| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("negative SIGPROJ index {index} in recursion carrier lookup"),
            )
        })?;
        if let Some(info) = self
            .recursion
            .resolve_carrier(self.arena, group, index_usize)?
        {
            return Ok(Some(info));
        }
        if match_sym_rec(self.arena, group).is_none() {
            return Ok(None);
        }

        // Ensure the group's recursion arrays and body stores are scheduled,
        // then read back the canonical carrier metadata allocated by `lower_proj`.
        let _ = self.lower_proj(proj_node, index, group)?;
        self.recursion
            .resolve_carrier(self.arena, group, index_usize)
    }

    /// Declares a stack-local current-sample binding for one scalar recursion
    /// carrier and records it under the canonical `(group, index)` key.
    fn bind_scalar_recursion_current_value(
        &mut self,
        group: SigId,
        index: usize,
        info: &RecArrayInfo,
        value: FirId,
    ) -> String {
        let prefix = if info.typ == FirType::Int32 {
            "iRecCur"
        } else {
            "fRecCur"
        };
        let name = if index == 0 {
            format!("{prefix}{}", group.as_u32())
        } else {
            format!("{prefix}{}_{}", group.as_u32(), index)
        };
        let mut b = FirBuilder::new(&mut self.store);
        self.sample_phases.immediate.push(b.declare_var(
            name.clone(),
            info.typ.clone(),
            AccessType::Stack,
            Some(value),
        ));
        self.recursion.set_current_value_binding(
            group,
            index,
            RecursionCurrentValueBinding {
                name: name.clone(),
                typ: info.typ.clone(),
            },
        );
        name
    }

    /// Loads the current-sample value of a scalar recursion carrier through its
    /// stack-local binding.
    fn load_scalar_recursion_current_value(
        &mut self,
        group: SigId,
        index: usize,
    ) -> Result<Option<FirId>, SignalFirError> {
        let Some(binding) = self
            .recursion
            .current_value_binding(self.arena, group, index)
        else {
            return Ok(None);
        };
        let mut b = FirBuilder::new(&mut self.store);
        Ok(Some(b.load_var(
            binding.name,
            AccessType::Stack,
            binding.typ.clone(),
        )))
    }

    /// Ensures a 2-element circular buffer state slot exists for `node`,
    /// idempotent.  On first call, declares `[typ; 2]` in the struct
    /// (prefixed `iRec` for `Int32`, `fRec` otherwise) and registers an
    /// `instanceClear` zeroing loop.  Returns the generated variable name.
    ///
    /// Keyed by `node` SigId in `state_name_by_node` — separate from
    /// `rec_array_by_group_index` to avoid aliasing (see `build_module` doc).
    fn ensure_state_slot(&mut self, node: SigId, typ: FirType, init: FirId) -> String {
        if let Some(name) = self.state_name_by_node.get(&node) {
            return name.clone();
        }
        let prefix = if typ == FirType::Int32 {
            "iRec"
        } else {
            "fRec"
        };
        let name = format!("{prefix}{}", node.as_u32());
        // Allocate a 2-element circular buffer (matching C++ signalFIRCompiler DelayLine).
        let array_ty = FirType::Array(Box::new(typ), 2);
        let mut b = FirBuilder::new(&mut self.store);
        let dec = b.declare_var(name.clone(), array_ty, AccessType::Struct, None);
        self.struct_declarations.push(dec);
        self.register_clear_recursion_array(name.clone(), init, 2);
        self.state_name_by_node.insert(node, name.clone());
        name
    }

    /// Declares the struct array for one circular delay line, idempotent.
    ///
    /// Thin delegate to [`DelayManager::ensure_delay_line`]; constructs a
    /// [`DelayFirCtx`] from disjoint fields via split-borrow struct literal so
    /// that `self.delay` can be borrowed simultaneously.
    fn ensure_delay_line_decl(
        &mut self,
        carried: SigId,
        delay: i32,
    ) -> Result<DelayLineInfo, SignalFirError> {
        // Explicit field-level split borrows: `self.delay` is NOT included here,
        // so it can be mutably borrowed below without conflict.
        let mut ctx = DelayFirCtx {
            store: &mut self.store,
            real_ty: self.real_ty.clone(),
            types: self.types,
            struct_declarations: &mut self.struct_declarations,
            clear_statements: &mut self.clear_statements,
            clear_init_seen: &mut self.clear_init_seen,
            next_loop_var_id: &mut self.next_loop_var_id,
            uses_iota: &mut self.uses_iota,
        };
        self.delay.ensure_delay_line(carried, delay, &mut ctx)
    }

    /// Returns the canonical pre-allocated delay line for `carried`.
    ///
    /// Delay-line strategy and geometry are chosen during
    /// [`Self::prepare_delay_lines`]. Lowering paths should only query that
    /// decision, not allocate new delay lines opportunistically.
    fn delay_line_info(&self, carried: SigId) -> Result<DelayLineInfo, SignalFirError> {
        self.delay.get_delay_line(carried).cloned().ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "internal fast-lane missing pre-allocated delay line for signal {}",
                    carried.as_u32()
                ),
            )
        })
    }

    /// Declares the shared global circular cursor state (`fIOTA`), idempotent.
    fn ensure_global_circular_cursor(&mut self) {
        let mut ctx = DelayFirCtx {
            store: &mut self.store,
            real_ty: self.real_ty.clone(),
            types: self.types,
            struct_declarations: &mut self.struct_declarations,
            clear_statements: &mut self.clear_statements,
            clear_init_seen: &mut self.clear_init_seen,
            next_loop_var_id: &mut self.next_loop_var_id,
            uses_iota: &mut self.uses_iota,
        };
        GlobalCircularCursor.ensure_state(&mut ctx);
    }

    /// Returns the masked current write index for the shared global cursor.
    fn global_circular_current_index(&mut self, size: usize) -> FirId {
        self.ensure_global_circular_cursor();
        GlobalCircularCursor.current_index(&mut self.store, size)
    }

    /// Returns the masked delayed read index for the shared global cursor.
    fn global_circular_delayed_index(&mut self, amount: FirId, size: usize) -> FirId {
        self.ensure_global_circular_cursor();
        GlobalCircularCursor.delayed_index(&mut self.store, amount, size)
    }

    /// Runs `f` with one recursion group pushed onto the active recursion stack.
    ///
    /// This centralizes the push/pop discipline for the active recursion-group
    /// stack, which must stay perfectly balanced even when lowering
    /// fails partway through a recursive body.
    fn with_active_recursion_group<R>(
        &mut self,
        var: SigId,
        arrays: Vec<RecArrayInfo>,
        f: impl FnOnce(&mut Self, &[RecArrayInfo]) -> Result<R, SignalFirError>,
    ) -> Result<R, SignalFirError> {
        self.recursion.push_active_group(var, arrays.clone());
        let result = f(self, &arrays);
        self.recursion.pop_active_group();
        result
    }

    /// Emits an `instanceClear` zeroing loop for a two-slot recursion array.
    ///
    /// Idempotent: subsequent calls for the same `name` are silently ignored.
    fn register_clear_recursion_array(&mut self, name: String, init: FirId, size: usize) {
        if !self.clear_init_seen.insert(name.clone()) {
            return;
        }
        let loop_var = self.fresh_loop_var("lRec");
        let upper = {
            let mut b = FirBuilder::new(&mut self.store);
            b.int32(i32::try_from(size).unwrap_or(i32::MAX))
        };
        let body = {
            let index = {
                let mut b = FirBuilder::new(&mut self.store);
                b.load_var(loop_var.clone(), AccessType::Loop, FirType::Int32)
            };
            let store = {
                let mut b = FirBuilder::new(&mut self.store);
                b.store_table(name, AccessType::Struct, index, init)
            };
            let mut b = FirBuilder::new(&mut self.store);
            b.block(&[store])
        };
        let mut b = FirBuilder::new(&mut self.store);
        self.clear_statements
            .push(b.simple_for_loop(loop_var, upper, body, false));
    }

    /// Generates a unique loop variable name using a monotonic counter.
    fn fresh_loop_var(&mut self, prefix: &str) -> String {
        let name = format!("{prefix}{}", self.next_loop_var_id);
        self.next_loop_var_id += 1;
        name
    }

    /// Emits `compute()`-preamble resets for `ReverseTimeRec` (LTI adjoint)
    /// recursion carriers.
    ///
    /// Dormant under the 2026-05-10 RAD dispatcher change; kept compilable for
    /// a future LTI fast-path revival.
    ///
    /// `ReverseTimeRec` has block-local adjoint semantics: the state one frame
    /// past `count - 1` is terminal-zero for every `compute()` call. Ordinary
    /// SYMREC primal carriers are only cleared by `instanceClear()` (they are
    /// persistent DSP state); only the LTI adjoint carriers belonging to
    /// `ReverseTimeRec` groups must be zeroed per-block.
    ///
    /// The distinction is made via `recursion.reverse_time_rec_group_ids`,
    /// which is populated by `allocate_group_arrays` when it sees a
    /// `SigMatch::ReverseTimeRec` group.  SYMREC carriers for BRA primal
    /// bodies are NOT in that set and are therefore skipped here.
    fn emit_reverse_time_rec_compute_resets(&mut self) {
        let reverse_ids = self.recursion.reverse_time_rec_group_ids.clone();
        let mut carriers: Vec<_> = self
            .recursion
            .rec_array_by_group_index
            .iter()
            .filter(|&(&(group_id, _), _)| reverse_ids.contains(&group_id))
            .map(|(_, info)| info.clone())
            .collect();
        carriers.sort_by(|a, b| a.name.cmp(&b.name));
        carriers.dedup_by(|a, b| a.name == b.name);

        for info in carriers {
            let init = match info.typ {
                FirType::Int32 => self.lower_int32_const(0),
                FirType::Float32 | FirType::Float64 | FirType::FaustFloat => self.float_const(0.0),
                _ => continue,
            };
            if info.size == 1 {
                let mut b = FirBuilder::new(&mut self.store);
                self.control_statements
                    .push(b.store_var(info.name, AccessType::Struct, init));
            } else {
                let loop_var = self.fresh_loop_var("lRevRec");
                let upper = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.int32(i32::try_from(info.size).unwrap_or(i32::MAX))
                };
                let body = {
                    let index = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.load_var(loop_var.clone(), AccessType::Loop, FirType::Int32)
                    };
                    let store = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.store_table(info.name, AccessType::Struct, index, init)
                    };
                    let mut b = FirBuilder::new(&mut self.store);
                    b.block(&[store])
                };
                let mut b = FirBuilder::new(&mut self.store);
                self.control_statements
                    .push(b.simple_for_loop(loop_var, upper, body, false));
            }
        }
    }

    // ── BlockReverseAD (Phase B3) ─────────────────────────────────────────

    /// Emits `compute()`-preamble resets for `SigBlockReverseAD` adjoint carry
    /// variables.
    ///
    /// Each carry variable stores the anti-causal adjoint contribution for a
    /// `Delay1` node inside the BRA body across reverse-loop samples.  Like
    /// `ReverseTimeRec` adjoint carriers, these must be zeroed before each
    /// reverse sample loop so no adjoint state leaks across host `compute()`
    /// calls.
    fn emit_bra_compute_resets(&mut self) {
        // Scalar Delay1 / Prefix carry resets.
        let mut names: Vec<String> = self.bra_delay1_carry_vars.values().cloned().collect();
        names.sort();
        for name in names {
            let zero = self.float_const(0.0);
            let mut b = FirBuilder::new(&mut self.store);
            self.control_statements
                .push(b.store_var(name, AccessType::Struct, zero));
        }
        // Array Delay(c) carry resets: zero c elements via a small for-loop.
        let mut array_entries: Vec<(String, usize)> =
            self.bra_delay_array_carry_vars.values().cloned().collect();
        array_entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, c) in array_entries {
            let zero = self.float_const(0.0);
            let loop_var = self.fresh_loop_var("lBraDlyRst");
            let upper = {
                let mut b = FirBuilder::new(&mut self.store);
                b.int32(i32::try_from(c).unwrap_or(i32::MAX))
            };
            let body = {
                let idx = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.load_var(loop_var.clone(), AccessType::Loop, FirType::Int32)
                };
                let store = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.store_table(name, AccessType::Struct, idx, zero)
                };
                let mut b = FirBuilder::new(&mut self.store);
                b.block(&[store])
            };
            let mut b = FirBuilder::new(&mut self.store);
            self.control_statements
                .push(b.simple_for_loop(loop_var, upper, body, false));
        }
    }

    /// Lowers a `Proj(index, BlockReverseAD)` node.
    ///
    /// - Slots `0 .. primal_count - 1` are **primal** outputs: the body
    ///   expression at that index is lowered directly in the forward sample
    ///   loop.
    /// - Slots `primal_count .. primal_count + seeds.len() - 1` are
    ///   **gradient** outputs: `ensure_bra_backward_sweep` is called once to
    ///   emit the TBPTT(BS, BS) adjoint sweep in the **current** sample-loop
    ///   slice, and the per-seed adjoint `FirId` is returned from the cache.
    ///
    /// “Current” is deliberate.  For a public gradient output the current slice
    /// is the reverse loop built by `build_module`.  For an internal gradient
    /// used by a forward recursive update, the current slice is the forward
    /// loop body currently being lowered.  The sweep code itself is the same;
    /// only its placement differs.
    #[allow(clippy::too_many_arguments)]
    fn lower_block_reverse_ad_proj(
        &mut self,
        _node: SigId,
        group: SigId,
        index: usize,
        primal_count: usize,
        body_sigs: &[SigId],
        seed_sigs: &[SigId],
        cotangent_sigs: &[SigId],
    ) -> Result<FirId, SignalFirError> {
        if index < primal_count {
            // Primal projection: lower the body signal and schedule tape stores
            // for signals reachable from THIS body only (Phase B4).
            //
            // Each body is lowered in its own SYMREC recursion context, so
            // `lower_signal` for body[index] works correctly under that body's
            // recursion variable.  Passing only `body_sigs[index]` ensures that
            // `ensure_bra_tape_stores` never tries to lower signals from a
            // different SYMREC group whose recursion variable is not yet on the
            // stack.  A per-signal guard inside the function prevents duplicate
            // tape declarations when bodies share sub-expressions.
            let val = self.lower_signal(body_sigs[index])?;
            self.ensure_bra_tape_stores(group, &[body_sigs[index]], seed_sigs, cotangent_sigs)?;
            return Ok(val);
        }
        let seed_index = index - primal_count;
        self.ensure_bra_backward_sweep(group, body_sigs, seed_sigs, cotangent_sigs)?;
        self.bra_grad_cache
            .get(&(group, seed_index))
            .copied()
            .ok_or_else(|| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!(
                        "BRA backward sweep did not produce gradient for seed index {seed_index}"
                    ),
                )
            })
    }

    /// Ensures the TBPTT(BS, BS) backward adjoint sweep for `group` has been
    /// emitted into the current sample-loop phase.
    ///
    /// The phase may be the explicit reverse loop for public RAD gradient
    /// outputs, or the forward loop when the gradient projection is an internal
    /// operand of a causal expression.  This function should therefore avoid
    /// assuming `self.lowering_reverse_loop == true`; it emits a local transpose
    /// program against the loop variable `i0` and lets the caller's scheduling
    /// context determine whether `i0` advances forward or backward in generated
    /// C++.
    ///
    /// The sweep is emitted **at most once** per group per loop slice; the
    /// `bra_state_scheduled` guard prevents re-emission when multiple gradient
    /// projection slots for the same carrier are lowered.
    ///
    /// # Algorithm
    ///
    /// 1. Build a unified postorder over all body roots (shared `visited` set
    ///    handles DAG-shared sub-expressions).
    /// 2. Lower each cotangent signal into a FIR value (constant `1.0` in the
    ///    all-ones B1 convention).
    ///    3a. Pre-seed recursive feedback carries (Phase B6).  For each
    ///    `Delay1(Proj(slot, SYMREF(var)))` node in the postorder, load the
    ///    corresponding carry struct field (written by the previous reverse step)
    ///    and accumulate it into `adj[body_sigs[slot]]`.  This ensures the total
    ///    TBPTT adjoint `cotangent[n] + carry_from_step_n+1` is available when
    ///    the `Proj(slot, SYMREC)` node is processed first in the reverse
    ///    postorder.
    ///    3b. Seed the adjoint map: `adj[body_sigs[k]] += cotangent_firs[k]`.
    /// 4. Walk the postorder in reverse, calling `propagate_bra_adj` for each
    ///    node to distribute its accumulated adjoint to its children.
    /// 5. Store per-seed gradient `FirId`s into `bra_grad_cache`.
    fn ensure_bra_backward_sweep(
        &mut self,
        group: SigId,
        body_sigs: &[SigId],
        seed_sigs: &[SigId],
        cotangent_sigs: &[SigId],
    ) -> Result<(), SignalFirError> {
        if !self.bra_state_scheduled.insert(group) {
            return Ok(());
        }

        // 1. Collect unified postorder.
        let mut visited = std::collections::HashSet::new();
        let mut postorder = Vec::new();
        for &body in body_sigs {
            collect_bra_postorder(self.arena, body, &mut visited, &mut postorder);
        }

        // 2. Lower cotangent signals.
        let mut cot_firs = Vec::with_capacity(cotangent_sigs.len());
        for &c in cotangent_sigs {
            cot_firs.push(self.lower_signal(c)?);
        }

        // 3. Seed the adjoint map.
        let mut adj: std::collections::HashMap<SigId, FirId> = std::collections::HashMap::new();

        // 3a. Pre-seed recursive feedback carries.
        //
        // In TBPTT the total adjoint of a recursive output `y[slot][n]` is:
        //
        //   adj[y[slot][n]] = cotangent[slot][n] + carry_from_step_n+1
        //
        // The carry from step n+1 encodes `adj[y[slot][n+1]] · ∂y[n+1]/∂y[n]`
        // and is stored in a struct field written during the previous reverse-loop
        // iteration.  We load it here — before the reverse-postorder walk — and
        // accumulate it into the matching `body_sig` so that when the Proj-SYMREC
        // node is processed first in the reverse postorder its `y_bar` already
        // includes the feedback contribution.
        //
        // `Delay1(Proj(slot, SYMREF(var)))` is the structural signal that
        // introduces the one-sample feedback delay; its carry variable represents
        // the anti-causal adjoint flowing from step n+1 back to step n.
        //
        // For circuits with multiple independent SYMREC groups (e.g., two
        // separate recursive poles), each group has its own SYMREF variable and
        // its own SYMREC variable.  We must match `SYMREF(var)` against the
        // corresponding `Proj(slot, SYMREC(var, ...))` in `body_sigs` by
        // comparing the symbolic recursion variable — NOT by using `slot` as a
        // flat index into `body_sigs` (which would be wrong when multiple groups
        // all have slot=0).
        //
        // Build: (SYMREC var TreeId, proj slot) → body_sig  from body_sigs.
        let mut var_slot_to_body_sig: HashMap<(TreeId, usize), SigId> = HashMap::new();
        for &body_sig in body_sigs {
            if let SigMatch::Proj(bslot, bgroup) = match_sig(self.arena, body_sig)
                && let Some((bvar, _)) = match_sym_rec(self.arena, bgroup)
            {
                let bslot_usize = usize::try_from(bslot).unwrap_or(usize::MAX);
                var_slot_to_body_sig.insert((bvar, bslot_usize), body_sig);
            }
        }

        for &sig in &postorder {
            if let SigMatch::Delay1(x) = match_sig(self.arena, sig)
                && let SigMatch::Proj(slot, inner_group) = match_sig(self.arena, x)
                && let Some(ref_var) = match_sym_ref(self.arena, inner_group)
            {
                let slot_usize = usize::try_from(slot).unwrap_or(usize::MAX);
                // Look up the body_sig whose SYMREC var matches this SYMREF var.
                if let Some(&proj_symrec) = var_slot_to_body_sig.get(&(ref_var, slot_usize)) {
                    let carry_name = self.ensure_bra_delay1_carry(sig, group)?;
                    let carry_load = {
                        let rt = self.real_ty();
                        let mut b = FirBuilder::new(&mut self.store);
                        b.load_var(carry_name, AccessType::Struct, rt)
                    };
                    let real_ty = self.real_ty.clone();
                    Self::add_to_adjoint(
                        &mut self.store,
                        &mut adj,
                        proj_symrec,
                        carry_load,
                        real_ty,
                    );
                }
            }
        }

        // 3b. Seed cotangent contributions.
        for (k, &body_sig) in body_sigs.iter().enumerate() {
            let cot = cot_firs[k];
            Self::add_to_adjoint(
                &mut self.store,
                &mut adj,
                body_sig,
                cot,
                self.real_ty.clone(),
            );
        }

        // 4. Backward propagation in reverse postorder.
        for &sig in postorder.iter().rev() {
            let y_bar = match adj.get(&sig).copied() {
                Some(fir) => fir,
                None => continue,
            };
            self.propagate_bra_adj(sig, y_bar, &mut adj, group)?;
        }

        // 5. Cache gradient FirIds.
        for (j, &seed) in seed_sigs.iter().enumerate() {
            let grad = adj
                .get(&seed)
                .copied()
                .unwrap_or_else(|| self.float_const(0.0));
            self.bra_grad_cache.insert((group, j), grad);
        }

        Ok(())
    }

    /// Propagates the adjoint `y_bar` of `sig` to the signal's children,
    /// updating `adj` according to the chain rule for each supported node kind.
    ///
    /// **Delay1** is anti-causal: rather than contributing directly to `adj[x]`,
    /// it reads the carry variable (written by the *next* reverse-loop step)
    /// as `adj[x]` and schedules a carry write to `post_output` for the
    /// *previous* reverse-loop step.  This matches the TBPTT(BS, BS) reference
    /// executor in `crates/compiler/tests/block_reverse_ad.rs`.
    ///
    /// **Phase B4 tape**: for `Mul`, `Div`, and unary math nodes whose operand
    /// value must be replayed from the forward pass, this method uses
    /// [`Self::load_bra_fwd_value`] instead of `lower_signal`.  When a tape
    /// array was declared by `ensure_bra_tape_stores` for that signal, the
    /// tape load is emitted; otherwise `lower_signal` is called (safe for
    /// trivially reverse-evaluable signals).
    ///
    /// Unsupported node kinds return a `SignalFirError::UnsupportedSignalNode`.
    fn propagate_bra_adj(
        &mut self,
        sig: SigId,
        y_bar: FirId,
        adj: &mut std::collections::HashMap<SigId, FirId>,
        group: SigId,
    ) -> Result<(), SignalFirError> {
        let real_ty = self.real_ty.clone();
        match match_sig(self.arena, sig) {
            // ── Leaves ─────────────────────────────────────────────────────
            SigMatch::Real(_)
            | SigMatch::Int(_)
            | SigMatch::Input(_)
            | SigMatch::HSlider(_)
            | SigMatch::VSlider(_)
            | SigMatch::NumEntry(_)
            | SigMatch::Button(_)
            | SigMatch::Checkbox(_)
            // Foreign constants (e.g. `ma.SR`, `ma.PI` pulled in via stdfaust.lib)
            // and foreign variables are external scalars with no differentiable children.
            // Gradient contribution is zero; nothing to propagate.
            | SigMatch::FConst(..)
            | SigMatch::FVar(..) => {
                // Seeds, constants, or external scalars: no children to propagate into.
            }

            // ── Casts: identity rule for real casts, gradient stop for int→real ─
            SigMatch::FloatCast(x) => {
                // `signalPromotion` inserts `FloatCast` where an Int-valued
                // signal is used in a Real context.  Example:
                //
                //   i[n]  = 1103515245*i[n-1] + 12345     // Int LCG state
                //   x[n]  = float(i[n]) * 4.656612873e-10 // Real noise sample
                //
                // RAD differentiates the Real expression starting at `x[n]`;
                // it does not reinterpret the upstream Int recurrence as Real
                // arithmetic.  Propagating a Float32 adjoint into the Int32
                // LCG subtree would both change the DSP semantics and produce
                // invalid mixed-domain FIR (`BinOp(Float32, Int32)`) in the
                // reverse sweep.  Therefore FloatCast is an identity only for
                // float-to-float casts; for int→real casts it is a gradient
                // boundary.
                let x_is_int = matches!(
                    self.signal_fir_type(x),
                    Ok(FirType::Int32) | Ok(FirType::Int64)
                );
                if !x_is_int {
                    Self::add_to_adjoint(&mut self.store, adj, x, y_bar, real_ty);
                }
            }
            SigMatch::IntCast(x) | SigMatch::BitCast(x) => {
                Self::add_to_adjoint(&mut self.store, adj, x, y_bar, real_ty);
            }

            // ── BinOp ───────────────────────────────────────────────────────
            SigMatch::BinOp(op, lhs, rhs) => match op {
                BinOp::Add => {
                    // adj[lhs] += y_bar; adj[rhs] += y_bar
                    Self::add_to_adjoint(&mut self.store, adj, lhs, y_bar, real_ty.clone());
                    Self::add_to_adjoint(&mut self.store, adj, rhs, y_bar, real_ty);
                }
                BinOp::Sub => {
                    // adj[lhs] += y_bar; adj[rhs] += -y_bar
                    Self::add_to_adjoint(&mut self.store, adj, lhs, y_bar, real_ty.clone());
                    let zero = self.float_const(0.0);
                    let neg_y_bar = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.binop(FirBinOp::Sub, zero, y_bar, real_ty.clone())
                    };
                    Self::add_to_adjoint(&mut self.store, adj, rhs, neg_y_bar, real_ty);
                }
                BinOp::Mul => {
                    // adj[lhs] += y_bar * val[rhs]; adj[rhs] += y_bar * val[lhs]
                    // Use load_bra_fwd_value so non-trivial operands (e.g.
                    // Delay1(x)) are read from the forward tape rather than
                    // re-evaluated in the reverse loop (Phase B4).
                    let rhs_val = self.load_bra_fwd_value(rhs)?;
                    let lhs_val = self.load_bra_fwd_value(lhs)?;
                    let lhs_adj = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.binop(FirBinOp::Mul, y_bar, rhs_val, real_ty.clone())
                    };
                    let rhs_adj = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.binop(FirBinOp::Mul, y_bar, lhs_val, real_ty.clone())
                    };
                    Self::add_to_adjoint(&mut self.store, adj, lhs, lhs_adj, real_ty.clone());
                    Self::add_to_adjoint(&mut self.store, adj, rhs, rhs_adj, real_ty);
                }
                BinOp::Div => {
                    // adj[lhs] += y_bar / val[rhs]
                    // adj[rhs] += -y_bar * val[lhs] / (val[rhs]^2)
                    let rhs_val = self.load_bra_fwd_value(rhs)?;
                    let lhs_val = self.load_bra_fwd_value(lhs)?;
                    let lhs_adj = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.binop(FirBinOp::Div, y_bar, rhs_val, real_ty.clone())
                    };
                    Self::add_to_adjoint(&mut self.store, adj, lhs, lhs_adj, real_ty.clone());
                    let rhs_sq = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.binop(FirBinOp::Mul, rhs_val, rhs_val, real_ty.clone())
                    };
                    let zero = self.float_const(0.0);
                    let neg_num = {
                        let neg_y_bar = {
                            let mut b = FirBuilder::new(&mut self.store);
                            b.binop(FirBinOp::Sub, zero, y_bar, real_ty.clone())
                        };
                        let mut b = FirBuilder::new(&mut self.store);
                        b.binop(FirBinOp::Mul, neg_y_bar, lhs_val, real_ty.clone())
                    };
                    let rhs_adj = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.binop(FirBinOp::Div, neg_num, rhs_sq, real_ty.clone())
                    };
                    Self::add_to_adjoint(&mut self.store, adj, rhs, rhs_adj, real_ty);
                }
                // Discrete / integer ops: gradient is zero for both operands.
                BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::And
                | BinOp::Or
                | BinOp::Xor
                | BinOp::Lsh
                | BinOp::ARsh
                | BinOp::LRsh
                | BinOp::Rem => {}
            },

            // ── Unary math ──────────────────────────────────────────────────
            SigMatch::Sin(x) => {
                // adj[x] += y_bar * cos(x)
                let x_fir = self.load_bra_fwd_value(x)?;
                self.used_math_ops.insert(FirMathOp::Cos);
                let rt = self.real_ty();
                let cos_x = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.math_call(FirMathOp::Cos, &[x_fir], rt)
                };
                let x_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Mul, y_bar, cos_x, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
            }
            SigMatch::Cos(x) => {
                // adj[x] += -y_bar * sin(x)
                let x_fir = self.load_bra_fwd_value(x)?;
                self.used_math_ops.insert(FirMathOp::Sin);
                let rt = self.real_ty();
                let sin_x = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.math_call(FirMathOp::Sin, &[x_fir], rt)
                };
                let zero = self.float_const(0.0);
                let neg_y_bar = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Sub, zero, y_bar, real_ty.clone())
                };
                let x_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Mul, neg_y_bar, sin_x, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
            }
            SigMatch::Exp(x) => {
                // adj[x] += y_bar * exp(x)  [= y_bar * val[sig]]
                // Tape-needed: val[sig] = exp(x); loaded from tape when x is
                // not trivially re-evaluable (Phase B4).
                let exp_val = self.load_bra_fwd_value(sig)?;
                let x_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Mul, y_bar, exp_val, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
            }
            SigMatch::Log(x) => {
                // adj[x] += y_bar / x
                let x_fir = self.load_bra_fwd_value(x)?;
                let x_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Div, y_bar, x_fir, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
            }
            SigMatch::Sqrt(x) => {
                // adj[x] += y_bar / (2 * sqrt(x))  [= y_bar / (2 * val[sig])]
                // Tape-needed: val[sig] = sqrt(x); loaded from tape when x is
                // not trivially re-evaluable (Phase B4).
                let sqrt_val = self.load_bra_fwd_value(sig)?;
                let two = self.float_const(2.0);
                let denom = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Mul, two, sqrt_val, real_ty.clone())
                };
                let x_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Div, y_bar, denom, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
            }

            // ── Delay1: anti-causal carry ───────────────────────────────────
            SigMatch::Delay1(x) => {
                // y[n] = x[n-1].  Adjoint: adj[x][n-1] += adj[y][n].
                //
                // In the reverse sample loop at step n:
                //   carry_load  = struct field written at step n+1  → adj[x] contribution
                //   carry_store = y_bar → struct field for step n-1 to read
                //
                // Ordering: `immediate` runs before `post_output` within one
                // iteration, so the load always reads the value stored at n+1.
                //
                // Special case — `Delay1(Proj(slot, SYMREF(var)))`:
                //   This is the one-sample feedback in a recursive body.  The carry
                //   load was already emitted during the pre-scan in
                //   `ensure_bra_backward_sweep` (step 3a) and accumulated into
                //   `adj[body_sigs[slot]]` so that the total TBPTT adjoint
                //   `cotangent[n] + carry_from_n+1` is set before the Proj-SYMREC
                //   node is processed in the reverse postorder.  Here we only need
                //   to store the new carry for step n-1.
                let is_recursive_feedback =
                    if let SigMatch::Proj(_slot, inner_group) = match_sig(self.arena, x) {
                        match_sym_ref(self.arena, inner_group).is_some()
                    } else {
                        false
                    };
                let carry_name = self.ensure_bra_delay1_carry(sig, group)?;
                let carry_store = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.store_var(carry_name.clone(), AccessType::Struct, y_bar)
                };
                self.sample_phases.post_output.push(carry_store);
                if !is_recursive_feedback {
                    let carry_load = {
                        let rt = self.real_ty();
                        let mut b = FirBuilder::new(&mut self.store);
                        b.load_var(carry_name, AccessType::Struct, rt)
                    };
                    Self::add_to_adjoint(&mut self.store, adj, x, carry_load, real_ty);
                }
            }

            // ── Tan, Asin, Acos, Atan, Log10 ───────────────────────────────
            SigMatch::Tan(x) => {
                // adj[x] += y_bar / cos(x)^2  = y_bar * (1 + tan(x)^2)
                let x_fir = self.load_bra_fwd_value(x)?;
                self.used_math_ops.insert(FirMathOp::Cos);
                let rt = self.real_ty();
                let cos_x = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.math_call(FirMathOp::Cos, &[x_fir], rt)
                };
                let cos2 = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Mul, cos_x, cos_x, real_ty.clone())
                };
                let x_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Div, y_bar, cos2, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
            }
            SigMatch::Asin(x) => {
                // adj[x] += y_bar / sqrt(1 - x^2)
                let x_fir = self.load_bra_fwd_value(x)?;
                self.used_math_ops.insert(FirMathOp::Sqrt);
                let one = self.float_const(1.0);
                let x2 = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Mul, x_fir, x_fir, real_ty.clone())
                };
                let denom_sq = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Sub, one, x2, real_ty.clone())
                };
                let rt = self.real_ty();
                let denom = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.math_call(FirMathOp::Sqrt, &[denom_sq], rt)
                };
                let x_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Div, y_bar, denom, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
            }
            SigMatch::Acos(x) => {
                // adj[x] += -y_bar / sqrt(1 - x^2)
                let x_fir = self.load_bra_fwd_value(x)?;
                self.used_math_ops.insert(FirMathOp::Sqrt);
                let one = self.float_const(1.0);
                let x2 = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Mul, x_fir, x_fir, real_ty.clone())
                };
                let denom_sq = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Sub, one, x2, real_ty.clone())
                };
                let rt = self.real_ty();
                let denom = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.math_call(FirMathOp::Sqrt, &[denom_sq], rt)
                };
                let zero = self.float_const(0.0);
                let neg_y_bar = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Sub, zero, y_bar, real_ty.clone())
                };
                let x_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Div, neg_y_bar, denom, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
            }
            SigMatch::Atan(x) => {
                // adj[x] += y_bar / (1 + x^2)
                let x_fir = self.load_bra_fwd_value(x)?;
                let one = self.float_const(1.0);
                let x2 = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Mul, x_fir, x_fir, real_ty.clone())
                };
                let denom = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Add, one, x2, real_ty.clone())
                };
                let x_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Div, y_bar, denom, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
            }
            SigMatch::Log10(x) => {
                // adj[x] += y_bar / (x * ln(10))
                let x_fir = self.load_bra_fwd_value(x)?;
                let ln10 = self.float_const(std::f64::consts::LN_10);
                let denom = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Mul, x_fir, ln10, real_ty.clone())
                };
                let x_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Div, y_bar, denom, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
            }

            // ── Abs: subgradient (sign function) ────────────────────────────
            SigMatch::Abs(x) => {
                // adj[x] += y_bar * sign(x)  [subgradient; 0 at x=0]
                // sign(x) = val[sig] / |val[sig]| when x != 0; we use
                // the fwd value of sig divided by the abs value.
                let abs_val = self.load_bra_fwd_value(sig)?;
                let x_fir = self.load_bra_fwd_value(x)?;
                let x_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    // y_bar * x / abs(x)  — matches sign(x)
                    let num = b.binop(FirBinOp::Mul, y_bar, x_fir, real_ty.clone());
                    b.binop(FirBinOp::Div, num, abs_val, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
            }

            // ── Floor / Ceil / Rint / Round: zero gradient ──────────────────
            SigMatch::Floor(x) | SigMatch::Ceil(x) | SigMatch::Rint(x) | SigMatch::Round(x) => {
                let _ = (x, y_bar); // Rounding ops: gradient is 0 almost everywhere.
            }

            // ── Pow(x, y): power rule ───────────────────────────────────────
            SigMatch::Pow(lhs, rhs) => {
                // d/dx x^y = y * x^(y-1);  d/dy x^y = x^y * ln(x)
                // adj[x] += y_bar * y * x^(y-1)         [uses pow(x, y-1)]
                // adj[y] += y_bar * val[sig] * ln(x)
                //
                // NOTE: do NOT use the equivalent form `y * x^y / x` — that
                // divides by x and produces NaN when x = 0 (e.g. when the
                // loss of a learning system reaches zero at convergence).
                // `pow(x, y-1)` is numerically safe: pow(0, 1) = 0 for y=2.
                self.used_math_ops.insert(FirMathOp::Pow);
                self.used_math_ops.insert(FirMathOp::Log);
                let pow_val = self.load_bra_fwd_value(sig)?;
                let x_fir = self.load_bra_fwd_value(lhs)?;
                let y_fir = self.load_bra_fwd_value(rhs)?;
                let lhs_adj = {
                    // y_bar * y * pow(x, y - 1)
                    let one = self.float_const(1.0);
                    let y_minus_1 = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.binop(FirBinOp::Sub, y_fir, one, real_ty.clone())
                    };
                    let rt = self.real_ty();
                    let pow_x_ym1 = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.math_call(FirMathOp::Pow, &[x_fir, y_minus_1], rt)
                    };
                    let mut b = FirBuilder::new(&mut self.store);
                    let t = b.binop(FirBinOp::Mul, y_bar, y_fir, real_ty.clone());
                    b.binop(FirBinOp::Mul, t, pow_x_ym1, real_ty.clone())
                };
                let rt = self.real_ty();
                let ln_x = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.math_call(FirMathOp::Log, &[x_fir], rt)
                };
                let rhs_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    let t = b.binop(FirBinOp::Mul, y_bar, pow_val, real_ty.clone());
                    b.binop(FirBinOp::Mul, t, ln_x, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, lhs, lhs_adj, real_ty.clone());
                Self::add_to_adjoint(&mut self.store, adj, rhs, rhs_adj, real_ty);
            }

            // ── Atan2(y, x): angle of (x, y) vector ─────────────────────────
            SigMatch::Atan2(lhs, rhs) => {
                // lhs = y, rhs = x in atan2(y, x).
                // d/dy = x / (x^2 + y^2);  d/dx = -y / (x^2 + y^2)
                let y_fir = self.load_bra_fwd_value(lhs)?;
                let x_fir = self.load_bra_fwd_value(rhs)?;
                let x2 = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Mul, x_fir, x_fir, real_ty.clone())
                };
                let y2 = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Mul, y_fir, y_fir, real_ty.clone())
                };
                let denom = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Add, x2, y2, real_ty.clone())
                };
                let lhs_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    let num = b.binop(FirBinOp::Mul, y_bar, x_fir, real_ty.clone());
                    b.binop(FirBinOp::Div, num, denom, real_ty.clone())
                };
                let zero = self.float_const(0.0);
                let neg_y_bar = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Sub, zero, y_bar, real_ty.clone())
                };
                let rhs_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    let num = b.binop(FirBinOp::Mul, neg_y_bar, y_fir, real_ty.clone());
                    b.binop(FirBinOp::Div, num, denom, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, lhs, lhs_adj, real_ty.clone());
                Self::add_to_adjoint(&mut self.store, adj, rhs, rhs_adj, real_ty);
            }

            // ── Min / Max: subgradient ──────────────────────────────────────
            SigMatch::Min(lhs, rhs) => {
                // adj[lhs] += y_bar if lhs <= rhs, else 0
                // adj[rhs] += y_bar if rhs < lhs, else 0
                let lhs_v = self.load_bra_fwd_value(lhs)?;
                let rhs_v = self.load_bra_fwd_value(rhs)?;
                let cond = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Le, lhs_v, rhs_v, FirType::Int32)
                };
                let zero_r = self.float_const(0.0);
                let lhs_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.select2(cond, y_bar, zero_r, real_ty.clone())
                };
                let zero_r2 = self.float_const(0.0);
                let rhs_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.select2(cond, zero_r2, y_bar, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, lhs, lhs_adj, real_ty.clone());
                Self::add_to_adjoint(&mut self.store, adj, rhs, rhs_adj, real_ty);
            }
            SigMatch::Max(lhs, rhs) => {
                // adj[lhs] += y_bar if lhs >= rhs, else 0
                // adj[rhs] += y_bar if rhs > lhs, else 0
                let lhs_v = self.load_bra_fwd_value(lhs)?;
                let rhs_v = self.load_bra_fwd_value(rhs)?;
                let cond = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Ge, lhs_v, rhs_v, FirType::Int32)
                };
                let zero_r = self.float_const(0.0);
                let lhs_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.select2(cond, y_bar, zero_r, real_ty.clone())
                };
                let zero_r2 = self.float_const(0.0);
                let rhs_adj = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.select2(cond, zero_r2, y_bar, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, lhs, lhs_adj, real_ty.clone());
                Self::add_to_adjoint(&mut self.store, adj, rhs, rhs_adj, real_ty);
            }

            // ── Delay(c, x): anti-causal carry with circular buffer ──────────
            SigMatch::Delay(sig_inner, amount) => {
                // Forward: y[n] = x[n-c].  Backward: adj[x][n] += adj[y][n+c].
                //
                // At reverse step n:
                //   carry[n % c] holds adj[y][n+c] written c steps ago.
                //   We load it → adj[sig_inner] += carry[n%c].
                //   We store y_bar to carry[n%c] for step n-c to read.
                let c_raw = tree_to_int(self.arena, amount).unwrap_or(0);
                let c = usize::try_from(c_raw).unwrap_or(0);
                if c == 0 {
                    // Zero delay: y = x.
                    Self::add_to_adjoint(&mut self.store, adj, sig_inner, y_bar, real_ty);
                } else {
                    let carry_name = self.ensure_bra_delay_array_carry(sig, c)?;
                    let c_fir = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.int32(i32::try_from(c).unwrap_or(i32::MAX))
                    };
                    let i0 = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.load_var("i0", AccessType::Loop, FirType::Int32)
                    };
                    let slot = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.binop(FirBinOp::Rem, i0, c_fir, FirType::Int32)
                    };
                    let rt = self.real_ty();
                    let carry_load = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.load_table(carry_name.clone(), AccessType::Struct, slot, rt)
                    };
                    let carry_store = {
                        let mut b = FirBuilder::new(&mut self.store);
                        b.store_table(carry_name, AccessType::Struct, slot, y_bar)
                    };
                    self.sample_phases.post_output.push(carry_store);
                    Self::add_to_adjoint(&mut self.store, adj, sig_inner, carry_load, real_ty);
                }
            }

            // ── Prefix(init, sig): Delay1 semantics + init contribution ─────
            SigMatch::Prefix(init, sig_inner) => {
                // Forward: y[0] = init, y[n] = x[n-1] for n ≥ 1.
                // Backward (same as Delay1 for x):
                //   adj[sig_inner][n] += adj[y][n+1]  (anti-causal carry)
                //   adj[init]         += adj[y][0]    (only at frame 0)
                //
                // The i0==0 condition for the init contribution is emitted as
                // a FIR Select2: contrib = y_bar * (i0 == 0 ? 1 : 0).
                let carry_name = self.ensure_bra_delay1_carry(sig, sig)?;
                let rt = self.real_ty();
                let carry_load = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.load_var(carry_name.clone(), AccessType::Struct, rt)
                };
                let carry_store = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.store_var(carry_name, AccessType::Struct, y_bar)
                };
                self.sample_phases.post_output.push(carry_store);
                Self::add_to_adjoint(&mut self.store, adj, sig_inner, carry_load, real_ty.clone());
                // Conditional init contribution: y_bar when i0 == 0, else 0.
                let i0 = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.load_var("i0", AccessType::Loop, FirType::Int32)
                };
                let zero_i = self.lower_int32_const(0);
                let is_frame0 = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.binop(FirBinOp::Eq, i0, zero_i, FirType::Int32)
                };
                let zero_r = self.float_const(0.0);
                let init_contrib = {
                    let mut b = FirBuilder::new(&mut self.store);
                    // Select2(cond, y_bar, 0.0): when is_frame0 != 0, use y_bar
                    b.select2(is_frame0, y_bar, zero_r, real_ty.clone())
                };
                Self::add_to_adjoint(&mut self.store, adj, init, init_contrib, real_ty);
            }

            // ── Proj(slot, SYMREC/SYMREF) — recursive carrier projection ────────
            //
            // Two symbolic Proj forms appear after `de_bruijn_to_sym`:
            //
            // • `Proj(slot, SYMREC(var, body_list))` — the top-level recursive
            //   output.  Its primal value equals `body_list[slot]`, so the adjoint
            //   flows identically to that body (identity Jacobian = 1).
            //   The pre-scan in `ensure_bra_backward_sweep` (step 3a) already
            //   accumulated the feedback carry into `adj[this_node]` before the
            //   reverse-postorder walk, so `y_bar` here is the full TBPTT adjoint
            //   `cotangent[n] + carry_from_step_n+1`.
            //
            // • `Proj(slot, SYMREF(var))` — a back-reference inside the recursive
            //   body.  This always appears as `Delay1(Proj(slot, SYMREF))`, and
            //   its adjoint carry was pre-loaded into `adj[body_sigs[slot]]` during
            //   the pre-scan.  The `Delay1` arm above stores the new carry to the
            //   struct field (for step n-1).  Nothing more to propagate here.
            SigMatch::Proj(slot, group_sig) => {
                if let Some((_var, body_list)) = match_sym_rec(self.arena, group_sig) {
                    // SYMREC top-level output: propagate adjoint to body[slot].
                    let slot_usize = usize::try_from(slot).map_err(|_| {
                        SignalFirError::new(
                            SignalFirErrorCode::UnsupportedSignalNode,
                            format!(
                                "negative Proj slot {slot} in BlockReverseAD backward pass (B6)"
                            ),
                        )
                    })?;
                    let bodies = list_to_vec(self.arena, body_list).ok_or_else(|| {
                        SignalFirError::new(
                            SignalFirErrorCode::UnsupportedSignalNode,
                            "malformed SYMREC body list in BlockReverseAD backward pass (B6)"
                                .to_string(),
                        )
                    })?;
                    let &body = bodies.get(slot_usize).ok_or_else(|| {
                        SignalFirError::new(
                            SignalFirErrorCode::UnsupportedSignalNode,
                            format!(
                                "Proj slot {slot_usize} out of range (SYMREC body count \
                                 {}) in BlockReverseAD backward pass (B6)",
                                bodies.len()
                            ),
                        )
                    })?;
                    Self::add_to_adjoint(&mut self.store, adj, body, y_bar, real_ty);
                } else if match_sym_ref(self.arena, group_sig).is_some() {
                    // SYMREF back-reference: carry pre-loaded in pre-scan; nothing to do.
                } else {
                    return Err(SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        format!(
                            "Proj over non-SYMREC/SYMREF group ({:?}) not supported in \
                             BlockReverseAD backward pass (B6)",
                            match_sig(self.arena, group_sig)
                        ),
                    ));
                }
            }

            other => {
                return Err(SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!("signal {other:?} not supported in BlockReverseAD backward pass (B6)"),
                ));
            }
        }
        Ok(())
    }

    /// Declares and returns the name of the adjoint carry variable for a
    /// `Delay1` node encountered inside a `SigBlockReverseAD` backward sweep.
    ///
    /// The carry is stored as a real-typed DSP struct field named
    /// `fBraCarryN` where N comes from the monotonic loop-var counter.  It is
    /// zeroed by `emit_bra_compute_resets` before each reverse sample loop so
    /// no adjoint state leaks across host `compute()` calls.
    ///
    /// Idempotent: subsequent calls for the same `delay1_node` return the same
    /// name without emitting a second declaration.
    fn ensure_bra_delay1_carry(
        &mut self,
        delay1_node: SigId,
        _group: SigId,
    ) -> Result<String, SignalFirError> {
        if let Some(name) = self.bra_delay1_carry_vars.get(&delay1_node) {
            return Ok(name.clone());
        }
        let name = format!("fBraCarry{}", self.next_loop_var_id);
        self.next_loop_var_id += 1;
        let real_ty = self.real_ty.clone();
        // Declare the struct field without a reset-time init: BRA carry variables
        // are internal DSP state, not UI-controlled parameters, and must NOT appear
        // in `instanceResetUserInterface`.  Only `instanceClear` zeroes them (below).
        self.ensure_named_struct_var(&name, real_ty, None);
        // Register a clear-time zero init for `instanceClear`.
        let zero2 = self.float_const(0.0);
        self.register_clear_init(name.clone(), zero2);
        self.bra_delay1_carry_vars.insert(delay1_node, name.clone());
        Ok(name)
    }

    /// Declares and returns the name of the circular carry buffer for a
    /// `Delay(c, x)` node encountered inside a `SigBlockReverseAD` backward
    /// sweep, where `c > 1` is the constant delay amount.
    ///
    /// The buffer is a `Array(real_ty, c)` struct field named `fBraDelayCarryN`.
    /// At reverse step n, slot `n % c` holds the adjoint contribution from step
    /// `n + c` (written c iterations ago), implementing the anti-causal rule
    /// `adj_x[n] += adj_y[n + c]`.
    ///
    /// Idempotent: subsequent calls for the same `delay_node` return the same
    /// name without emitting a second declaration.
    fn ensure_bra_delay_array_carry(
        &mut self,
        delay_node: SigId,
        c: usize,
    ) -> Result<String, SignalFirError> {
        if let Some((name, _)) = self.bra_delay_array_carry_vars.get(&delay_node) {
            return Ok(name.clone());
        }
        let name = format!("fBraDelayCarry{}", self.next_loop_var_id);
        self.next_loop_var_id += 1;
        let real_ty = self.real_ty.clone();
        let arr_ty = FirType::Array(Box::new(real_ty), c);
        self.ensure_named_struct_var(&name, arr_ty, None);
        self.bra_delay_array_carry_vars
            .insert(delay_node, (name.clone(), c));
        Ok(name)
    }

    /// Schedules forward-tape stores for tape-needed signals reachable from
    /// the given `body_sigs` roots.
    ///
    /// Called from `lower_block_reverse_ad_proj` once per primal slot, with
    /// only the body for that slot.  This ensures that `lower_signal` is
    /// called exclusively within the SYMREC recursion context that is active
    /// for the current primal slot — signals from a different SYMREC group
    /// (with a different recursion variable on the stack) must **not** be
    /// lowered here.
    ///
    /// Idempotency is maintained per-signal via `bra_tape_store_var`: if a
    /// signal has already been taped (e.g. because it is shared across bodies),
    /// a second call for a different body silently skips it.
    ///
    /// # Steps
    ///
    /// 1. Build the postorder for the supplied `body_sigs` roots.
    /// 2. Call [`collect_tape_needed_values`] to determine which forward values
    ///    require a tape.
    /// 3. For each tape-needed signal `v` not yet in `bra_tape_store_var`:
    ///    a. Allocate a fresh struct-field name `fBraTapeN`.
    ///    b. Declare the field as `Array(real_ty, MAX_BRA_TAPE_BLOCK_SIZE)`.
    ///    c. Lower `v` via `lower_signal` (runs in the forward loop context).
    ///    d. Emit `store_table(fBraTapeN, Struct, i0, v_fir)` to
    ///    `sample_phases.immediate` so it captures the forward value
    ///    **before** `post_output` updates delay/state variables (placing
    ///    it in `sample_end` would read post-update state and produce the
    ///    wrong tape entry for signals like `Delay1`).
    ///    e. Record the mapping `v → fBraTapeN` in `bra_tape_store_var`.
    ///
    /// In the split public-output schedule these stores appear in the forward
    /// loop and the matching loads appear in a later reverse loop.  In the
    /// inline adaptive schedule both the stores and the adjoint statements can
    /// be emitted into the same forward loop body.  The phase ordering still
    /// matters: tape stores are pushed to `immediate`, before state updates and
    /// before any later BRA sweep statements for the same carrier can consume
    /// the recorded values.
    ///
    /// # Interaction with `signalPromotion`
    ///
    /// The input signal forest has already been promoted before FIR lowering.
    /// BRA therefore must not perform ad-hoc integer-to-real promotion by
    /// casting values at the tape store.  The tape is a backend object, not a
    /// Signal-IR node, so such a cast would bypass normalform's `signalPromotion`
    /// rules and could hide a missing promotion bug.
    ///
    /// `collect_tape_needed_values` is intentionally conservative and
    /// structural: it may see integer/discrete nodes that are present upstream
    /// of a promoted `FloatCast`.  Those upstream nodes keep their original
    /// integer semantics (for instance the LCG recurrence used to generate
    /// pseudo-noise) and no adjoint rule crosses the int→real cast.  They are
    /// skipped here.  The promoted real `FloatCast` result, or a real expression
    /// derived from it, is the value that may be taped and later loaded by the
    /// reverse sweep.
    fn ensure_bra_tape_stores(
        &mut self,
        _group: SigId,
        body_sigs: &[SigId],
        _seed_sigs: &[SigId],
        _cotangent_sigs: &[SigId],
    ) -> Result<(), SignalFirError> {
        // 1. Build postorder over the supplied body roots.
        let mut visited = std::collections::HashSet::new();
        let mut postorder = Vec::new();
        for &body in body_sigs {
            collect_bra_postorder(self.arena, body, &mut visited, &mut postorder);
        }

        // 2. Determine which values need to be taped.
        let tape_needed = collect_tape_needed_values(self.arena, &postorder);
        if tape_needed.is_empty() {
            return Ok(());
        }

        // 3. Emit tape stores in deterministic (postorder) order.
        let mut tape_sigs: Vec<SigId> = tape_needed.into_iter().collect();
        // Sort by SigId for deterministic emission.
        tape_sigs.sort();
        for v in tape_sigs {
            // Per-signal idempotency: skip signals already taped by a prior call
            // (e.g. a signal shared between two SYMREC bodies).
            if self.bra_tape_store_var.contains_key(&v) {
                continue;
            }
            let real_ty = self.real_ty.clone();
            let v_ty = self.signal_fir_type(v)?;
            if v_ty != real_ty {
                // `collect_tape_needed_values` is structural: it walks the full
                // body postorder and can see integer islands below a
                // `FloatCast`, notably LCG-style noise recursions.  Those
                // integer subgraphs are not differentiable and
                // `propagate_bra_adj` stops at the int->float cast, so no
                // reverse rule will ever load them from a BRA tape.  The
                // real-valued use site must already be represented by a
                // promoted `FloatCast` node; that node is the candidate to tape
                // when needed.  Skip non-real candidates here rather than
                // silently casting and hiding a missing Signal-level promotion.
                continue;
            }
            let tape_name = format!("fBraTape{}", self.next_loop_var_id);
            self.next_loop_var_id += 1;
            // Declare as a fixed-size array struct field.
            let tape_ty = FirType::Array(Box::new(real_ty.clone()), MAX_BRA_TAPE_BLOCK_SIZE);
            self.ensure_named_struct_var(&tape_name, tape_ty, None);
            // Lower the value in the current (forward) loop context.
            // BRA tapes are homogeneous `real_ty` arrays because the reverse
            // rules consume recorded forward values in real adjoint arithmetic.
            let v_fir = self.lower_signal(v)?;
            if self.store.value_type(v_fir) != Some(real_ty.clone()) {
                let sig_text = dump_sig_readable(self.arena, v);
                let got = self.store.value_type(v_fir);
                return Err(SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!(
                        "BlockReverseAD real tape-needed signal {sig_text} lowered to FIR type {got:?}, expected {real_ty:?}; integer/real promotion must be resolved before FIR lowering"
                    ),
                ));
            }
            // Tape stores go in `immediate` so they capture the forward value
            // BEFORE `post_output` updates delay/state variables.  Placing them
            // in `sample_end` would re-read post-update state (e.g. the updated
            // Delay1 register) and produce the wrong tape entry.
            let i0 = {
                let mut b = FirBuilder::new(&mut self.store);
                b.load_var("i0", AccessType::Loop, FirType::Int32)
            };
            let store_stmt = {
                let mut b = FirBuilder::new(&mut self.store);
                b.store_table(tape_name.clone(), AccessType::Struct, i0, v_fir)
            };
            self.sample_phases.immediate.push(store_stmt);
            self.bra_tape_store_var.insert(v, tape_name);
        }
        Ok(())
    }

    /// Returns the FIR value for `sig` in the **reverse** sample loop.
    ///
    /// - If `sig` has a tape array (recorded by `ensure_bra_tape_stores`),
    ///   emits `load_table(fBraTapeN, Struct, i0)` and returns that value.
    /// - Otherwise falls back to `lower_signal(sig)`, which is correct when
    ///   `sig` is trivially reverse-evaluable (stateless leaf or pure
    ///   combinator of leaves).
    ///
    /// The loop variable `i0` used for the tape load is the same reverse-loop
    /// counter driven by the outer `build_module` reverse iteration; loading
    /// tape[i0] during the backward sweep at step `n` retrieves the forward
    /// value stored at forward step `n`.
    fn load_bra_fwd_value(&mut self, sig: SigId) -> Result<FirId, SignalFirError> {
        if let Some(tape_name) = self.bra_tape_store_var.get(&sig).cloned() {
            let real_ty = self.real_ty();
            let i0 = {
                let mut b = FirBuilder::new(&mut self.store);
                b.load_var("i0", AccessType::Loop, FirType::Int32)
            };
            let load = {
                let mut b = FirBuilder::new(&mut self.store);
                b.load_table(tape_name, AccessType::Struct, i0, real_ty)
            };
            Ok(load)
        } else {
            self.lower_signal(sig)
        }
    }

    /// Accumulates `new_term` into the adjoint of `sig`, building an `Add`
    /// node when a prior term already exists.
    ///
    /// This is the FIR-level equivalent of `adj[sig] += new_term` in the
    /// scalar BPTT executor.
    fn add_to_adjoint(
        store: &mut FirStore,
        adj: &mut std::collections::HashMap<SigId, FirId>,
        sig: SigId,
        new_term: FirId,
        real_ty: FirType,
    ) {
        let entry = adj.entry(sig);
        match entry {
            std::collections::hash_map::Entry::Occupied(mut e) => {
                let old = *e.get();
                let sum = {
                    let mut b = FirBuilder::new(store);
                    b.binop(FirBinOp::Add, old, new_term, real_ty)
                };
                *e.get_mut() = sum;
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(new_term);
            }
        }
    }

    /// Emits one floating-point constant at the internal real precision.
    ///
    /// Uses `Float32` or `Float64` depending on `real_ty`.  Never emits
    /// `FaustFloat` — that type is reserved for external interface points.
    fn float_const(&mut self, value: f64) -> FirId {
        let mut b = FirBuilder::new(&mut self.store);
        match self.real_ty {
            FirType::Float64 => b.float64(value),
            _ => b.float32(value as f32),
        }
    }

    /// Derives an initial state value from a signal if constant, otherwise `0`.
    fn initial_state_from_signal(&mut self, sig: SigId) -> FirId {
        match match_sig(self.arena, sig) {
            SigMatch::Int(v) => self.lower_int32_const(v),
            SigMatch::Real(v) => self.float_const(v),
            _ => self.float_const(0.0),
        }
    }
}

// ── Constant, UI, soundfile, and table lowering ────────────────────────────

impl<'a> SignalToFirLower<'a> {
    /// Emits one `Int32` FIR constant.
    fn lower_int32_const(&mut self, value: i32) -> FirId {
        let mut b = FirBuilder::new(&mut self.store);
        b.int32(value)
    }

    /// Declares the `FaustFloat` struct zone variable for a button or checkbox, idempotent.
    fn ensure_button_zone(
        &mut self,
        control: ControlId,
        typ: ButtonType,
    ) -> Result<String, SignalFirError> {
        if let Some(var) = self.ui_controls.get(&control).cloned() {
            return Ok(var);
        }
        let spec = self.control_spec(control)?;
        let expected_kind = match typ {
            ButtonType::Button => ControlKind::Button,
            ButtonType::Checkbox => ControlKind::Checkbox,
        };
        if spec.kind != expected_kind {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "control id {control} kind mismatch: expected {expected_kind:?}, got {:?}",
                    spec.kind
                ),
            ));
        }
        let var = self.ui_control_var_name(
            control,
            match typ {
                ButtonType::Button => "fButton",
                ButtonType::Checkbox => "fCheckbox",
            },
        );
        let init = self.float_const(0.0);
        self.ensure_named_struct_var(&var, FirType::FaustFloat, Some(init));
        self.ui_controls.insert(control, var.clone());
        Ok(var)
    }

    /// Lowers button/checkbox UI controls as zone-backed struct variables.
    fn lower_button(
        &mut self,
        control: ControlId,
        typ: ButtonType,
    ) -> Result<FirId, SignalFirError> {
        let var = self.ensure_button_zone(control, typ)?;
        if self.ui_controls.contains_key(&control) {
            // UI zone variable is FaustFloat (external); cast to real_ty for computation.
            let real_ty = self.real_ty();
            let mut b = FirBuilder::new(&mut self.store);
            let load = b.load_var(var, AccessType::Struct, FirType::FaustFloat);
            return Ok(b.cast(real_ty, load));
        }
        unreachable!("button zone should be inserted before loading")
    }

    /// Lowers slider-style UI controls and records metadata in
    /// `buildUserInterface`.
    fn lower_slider(
        &mut self,
        control: ControlId,
        typ: SliderType,
    ) -> Result<FirId, SignalFirError> {
        let var = self.ensure_slider_zone(control, typ)?;
        if self.ui_controls.contains_key(&control) {
            // UI zone variable is FaustFloat (external); cast to real_ty for computation.
            let real_ty = self.real_ty();
            let mut b = FirBuilder::new(&mut self.store);
            let load = b.load_var(var, AccessType::Struct, FirType::FaustFloat);
            return Ok(b.cast(real_ty, load));
        }
        unreachable!("slider zone should be inserted before loading")
    }

    /// Lowers bargraph UI nodes by creating UI descriptors and storing incoming
    /// runtime value in a dedicated control zone.
    fn lower_bargraph(
        &mut self,
        control: ControlId,
        value: SigId,
        typ: BargraphType,
    ) -> Result<FirId, SignalFirError> {
        let _ = self.ensure_bargraph_zone(control, typ)?;
        // The incoming signal value is computed at internal real precision; cast
        // it to FaustFloat before writing to the external zone variable.
        let value = self.lower_signal(value)?;
        let var = self
            .ui_controls
            .get(&control)
            .cloned()
            .expect("bargraph variable should exist after declaration");
        let mut b = FirBuilder::new(&mut self.store);
        let faust_value = b.cast(FirType::FaustFloat, value);
        self.sample_phases
            .immediate
            .push(b.store_var(var, AccessType::Struct, faust_value));
        Ok(value)
    }

    /// Lowers a soundfile declaration into UI-only registration and an opaque
    /// struct-backed runtime handle.
    fn lower_soundfile(&mut self, control: ControlId) -> Result<FirId, SignalFirError> {
        let var = self.ensure_soundfile_zone(control)?;
        if self.soundfiles.contains_key(&control) {
            let mut b = FirBuilder::new(&mut self.store);
            return Ok(b.load_var(var, AccessType::Struct, FirType::Sound));
        }
        unreachable!("soundfile zone should be inserted before loading")
    }

    /// Extracts the var name from a `SIGSOUNDFILE` signal node.
    fn soundfile_var_from_signal(&mut self, sf: SigId) -> Result<String, SignalFirError> {
        match match_sig(self.arena, sf) {
            SigMatch::Soundfile(control) => self.ensure_soundfile_zone(control),
            _ => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "expected SIGSOUNDFILE node, got {}",
                    dump_sig_readable(self.arena, sf)
                ),
            )),
        }
    }

    /// Lowers `SIGSOUNDFILELENGTH(sf, part)` → `fSoundN->fLength[part]`.
    fn lower_soundfile_length(&mut self, sf: SigId, part: SigId) -> Result<FirId, SignalFirError> {
        let var = self.soundfile_var_from_signal(sf)?;
        let part = self.lower_signal(part)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_soundfile_length(var, part))
    }

    /// Lowers `SIGSOUNDFILERATE(sf, part)` → `fSoundN->fSR[part]`.
    fn lower_soundfile_rate(&mut self, sf: SigId, part: SigId) -> Result<FirId, SignalFirError> {
        let var = self.soundfile_var_from_signal(sf)?;
        let part = self.lower_signal(part)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_soundfile_rate(var, part))
    }

    /// Lowers `SIGSOUNDFILEBUFFER(sf, chan, part, ridx)` →
    /// `((FAUSTFLOAT**)fSoundN->fBuffers)[chan][fSoundN->fOffset[part] + ridx]`.
    fn lower_soundfile_buffer(
        &mut self,
        node: SigId,
        sf: SigId,
        chan: SigId,
        part: SigId,
        ridx: SigId,
    ) -> Result<FirId, SignalFirError> {
        let var = self.soundfile_var_from_signal(sf)?;
        let chan = self.lower_signal(chan)?;
        let part = self.lower_signal(part)?;
        let idx = self.lower_signal(ridx)?;
        let typ = self.signal_fir_type(node)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_soundfile_buffer(var, chan, part, idx, typ))
    }

    /// Lowers a `SIGWAVEFORM` node used as a direct signal output.
    ///
    /// Emits a cycling integer state slot `iWave{N}` (cleared to 0 in
    /// `instanceClear`) that advances by 1 mod `len` each sample, producing the
    /// correct sequential value from the waveform table.
    ///
    /// Contrast with `lower_rdtbl`: when a waveform is used as a read-table
    /// source (via `SIGWRTBL`/`SIGGEN`), the table is filled once in
    /// `ensure_wrtbl_table` and accessed with an arbitrary external index.
    fn lower_waveform(&mut self, node: SigId, values: &[SigId]) -> Result<FirId, SignalFirError> {
        let table_name = self.ensure_waveform_table(node, values)?;
        if values.is_empty() {
            return self.unsupported_node(node, "SIGWAVEFORM cannot be empty");
        }
        let n = i32::try_from(values.len()).unwrap_or(i32::MAX);
        let idx_name = format!("iWave{}", node.as_u32());
        if self.named_struct_vars.insert(idx_name.clone()) {
            let mut b = FirBuilder::new(&mut self.store);
            let dec = b.declare_var(idx_name.clone(), FirType::Int32, AccessType::Struct, None);
            self.struct_declarations.push(dec);
            let zero = self.lower_int32_const(0);
            self.register_clear_init(idx_name.clone(), zero);
            // Compute update: iWave = (iWave + 1) % N
            let iwave_load = {
                let mut b = FirBuilder::new(&mut self.store);
                b.load_var(idx_name.clone(), AccessType::Struct, FirType::Int32)
            };
            let one = self.lower_int32_const(1);
            let size = self.lower_int32_const(n);
            let next = {
                let mut b = FirBuilder::new(&mut self.store);
                let sum = b.binop(FirBinOp::Add, iwave_load, one, FirType::Int32);
                b.binop(FirBinOp::Rem, sum, size, FirType::Int32)
            };
            let update = {
                let mut b = FirBuilder::new(&mut self.store);
                b.store_var(idx_name.clone(), AccessType::Struct, next)
            };
            self.sample_phases.post_output.push(update);
        }
        let index = {
            let mut b = FirBuilder::new(&mut self.store);
            b.load_var(idx_name, AccessType::Struct, FirType::Int32)
        };
        let real_ty = self.signal_fir_type(node)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_table(table_name, AccessType::Static, index, real_ty))
    }

    /// Lowers one table read by resolving the table producer and normalizing
    /// the runtime read index according to table length.
    fn lower_rdtbl(
        &mut self,
        node: SigId,
        tbl: SigId,
        ridx: SigId,
    ) -> Result<FirId, SignalFirError> {
        // Keep C++ `compileSigRDTbl` evaluation order: evaluate table first so
        // pending `wrtbl` side-effects are emitted before read access.
        let _ = self.lower_signal(tbl)?;
        let (table_name, table_len, access) = self.resolve_table(tbl)?;
        if table_len == 0 {
            return self.unsupported_node(node, "SIGRDTBL cannot read an empty table");
        }
        let ridx_sig = ridx;
        let ridx = self.lower_signal(ridx)?;
        let index = self.table_index_with_bounds(ridx, ridx_sig, table_len);
        let real_ty = self.signal_fir_type(node)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_table(table_name, access, index, real_ty))
    }

    /// Lowers one table write producer (`SIGWRTBL`) and returns the table alias.
    ///
    /// Current scope supports deterministic constant-size tables with generator
    /// expansion handled by [`Self::expand_generator_values`].
    fn lower_wrtbl(
        &mut self,
        node: SigId,
        _size: SigId,
        generator: SigId,
        widx: SigId,
        wsig: SigId,
    ) -> Result<FirId, SignalFirError> {
        let (table_name, table_len, access) = self.resolve_table(node)?;
        if table_len == 0 {
            return self.unsupported_node(generator, "SIGWRTBL cannot write an empty table");
        }
        if self.arena.is_nil(widx) {
            if self.arena.is_nil(wsig) {
                return self.zero_value_for_signal(node);
            }
            return self.lower_signal(wsig);
        }
        if self.arena.is_nil(wsig) {
            return self.unsupported_node(node, "SIGWRTBL write requires wsig when widx is set");
        }
        let wsig_value = self.lower_signal(wsig)?;
        let widx = self.lower_signal(widx)?;
        let index = self.normalized_table_index(widx, table_len);
        let mut b = FirBuilder::new(&mut self.store);
        self.sample_phases
            .immediate
            .push(b.store_table(table_name, access, index, wsig_value));
        Ok(wsig_value)
    }

    /// Resolves a table-producing signal into `(table_name, table_len, access)`.
    ///
    /// Three cases are handled:
    /// - `SIGWAVEFORM`: static constant table (`AccessType::Static`).
    /// - `SIGWRTBL(size, gen, nil, nil)`: read-only generated table, expanded
    ///   at compile-time (`AccessType::Static`).
    /// - `SIGWRTBL(size, gen, widx, wsig)`: writable runtime table; written
    ///   per-sample and read with (`AccessType::Struct`).
    fn resolve_table(&mut self, sig: SigId) -> Result<(String, usize, AccessType), SignalFirError> {
        if let Some(name) = self.waveform_tables.get(&sig).cloned() {
            let len = self.waveform_table_len.get(&sig).copied().unwrap_or(0);
            let access = self
                .table_access_by_sig
                .get(&sig)
                .copied()
                .unwrap_or(AccessType::Static);
            return Ok((name, len, access));
        }
        match match_sig(self.arena, sig) {
            SigMatch::Waveform(values) => {
                let name = self.ensure_waveform_table(sig, values)?;
                Ok((name, values.len(), AccessType::Static))
            }
            SigMatch::WrTbl(size, generator, widx, wsig) => {
                if self.arena.is_nil(widx) && self.arena.is_nil(wsig) {
                    let (name, len) = self.ensure_readonly_table(sig, size, generator)?;
                    Ok((name, len, AccessType::Static))
                } else {
                    let (name, len) = self.ensure_wrtbl_table(sig, size, generator)?;
                    Ok((name, len, AccessType::Struct))
                }
            }
            _ => self.unsupported_node(
                sig,
                "table access currently supports SIGWAVEFORM and SIGWRTBL forms in Step 2H",
            ),
        }
    }

    /// Ensures one waveform table declaration is emitted exactly once.
    fn ensure_waveform_table(
        &mut self,
        sig: SigId,
        values: &[SigId],
    ) -> Result<String, SignalFirError> {
        if let Some(name) = self.waveform_tables.get(&sig).cloned() {
            return Ok(name);
        }
        let mut lowered_values = Vec::with_capacity(values.len());
        for value in values {
            lowered_values.push(self.lower_signal(*value)?);
        }
        let elem_ty = self.signal_fir_type(sig)?;
        let prefix = if elem_ty == FirType::Int32 {
            "iTbl"
        } else {
            "fTbl"
        };
        let name = format!("{prefix}{}", sig.as_u32());
        let mut b = FirBuilder::new(&mut self.store);
        let decl = b.declare_table(name.clone(), AccessType::Static, elem_ty, &lowered_values);
        self.static_declarations.push(decl);
        self.waveform_tables.insert(sig, name.clone());
        self.waveform_table_len.insert(sig, values.len());
        self.table_access_by_sig.insert(sig, AccessType::Static);
        Ok(name)
    }

    /// Ensures one read-only `rdtable`-style declaration is emitted exactly once.
    ///
    /// Unlike `ensure_waveform_table` (literal constant values), this expands
    /// the generator at compile-time via `expand_generator_values`.  The
    /// resulting array is declared `Static` — no per-instance write is needed.
    fn ensure_readonly_table(
        &mut self,
        sig: SigId,
        size_sig: SigId,
        generator_sig: SigId,
    ) -> Result<(String, usize), SignalFirError> {
        let size = self.table_size_from_sig(size_sig)?;
        let elem_ty = self.signal_fir_type(sig)?;
        let generated = self.expand_generator_values(generator_sig, size, &elem_ty)?;
        let prefix = if elem_ty == FirType::Int32 {
            "iTbl"
        } else {
            "fTbl"
        };
        let name = format!("{prefix}{}", sig.as_u32());
        let mut b = FirBuilder::new(&mut self.store);
        let decl = b.declare_table(name.clone(), AccessType::Static, elem_ty, &generated);
        self.static_declarations.push(decl);
        self.waveform_tables.insert(sig, name.clone());
        self.waveform_table_len.insert(sig, size);
        self.table_access_by_sig.insert(sig, AccessType::Static);
        Ok((name, size))
    }

    /// Ensures one writable `rwtable` declaration and per-instance
    /// initialization are emitted exactly once.
    ///
    /// The table lives in the DSP struct (`AccessType::Struct`) so it can be
    /// written at runtime.  The generator is expanded at compile-time and
    /// registered in `instanceConstants` to seed initial values; per-sample
    /// writes are emitted by `lower_wrtbl` into the sample loop immediate phase.
    fn ensure_wrtbl_table(
        &mut self,
        sig: SigId,
        size_sig: SigId,
        generator_sig: SigId,
    ) -> Result<(String, usize), SignalFirError> {
        let size = self.table_size_from_sig(size_sig)?;
        let elem_ty = self.signal_fir_type(sig)?;
        let generated = self.expand_generator_values(generator_sig, size, &elem_ty)?;
        let prefix = if elem_ty == FirType::Int32 {
            "iTbl"
        } else {
            "fTbl"
        };
        let name = format!("{prefix}{}", sig.as_u32());
        let mut b = FirBuilder::new(&mut self.store);
        let decl = b.declare_table(
            name.clone(),
            AccessType::Struct,
            elem_ty.clone(),
            &generated,
        );
        self.struct_declarations.push(decl);
        self.register_constant_table_init(name.clone(), AccessType::Struct, &generated);
        self.waveform_tables.insert(sig, name.clone());
        self.waveform_table_len.insert(sig, size);
        self.table_access_by_sig.insert(sig, AccessType::Struct);
        Ok((name, size))
    }

    /// Evaluates table-size signal to a positive `usize`.
    fn table_size_from_sig(&self, size_sig: SigId) -> Result<usize, SignalFirError> {
        match match_sig(self.arena, size_sig) {
            SigMatch::Int(v) if v > 0 => usize::try_from(v).map_err(|_| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!("SIGWRTBL size conversion overflow: {v}"),
                )
            }),
            SigMatch::Int(v) => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("SIGWRTBL size must be > 0, got {v}"),
            )),
            _ => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "SIGWRTBL currently requires constant integer size in Step 2H",
            )),
        }
    }

    /// Expands a table generator signal into concrete initializer values.
    ///
    /// Only generator shapes that can be fully resolved at compile-time are
    /// accepted in the current fast-lane slice.
    fn expand_generator_values(
        &mut self,
        generator_sig: SigId,
        size: usize,
        elem_ty: &FirType,
    ) -> Result<Vec<FirId>, SignalFirError> {
        let init_sig = if let SigMatch::Gen(inner) = match_sig(self.arena, generator_sig) {
            inner
        } else {
            generator_sig
        };
        match match_sig(self.arena, init_sig) {
            SigMatch::Waveform(values) => {
                if values.is_empty() {
                    return Err(SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "SIGGEN waveform cannot be empty in Step 2H",
                    ));
                }
                let mut out = Vec::with_capacity(size);
                for index in 0..size {
                    let item = values[index % values.len()];
                    out.push(self.lower_signal(item)?);
                }
                Ok(out)
            }
            SigMatch::Int(_) | SigMatch::Real(_) => {
                let v = self.lower_signal(init_sig)?;
                Ok(vec![v; size])
            }
            _ => {
                // Computed generator: interpret at compile time.
                // This is the compile-time equivalent of C++'s signal2Container
                // approach — since SIGGEN generators are always 0-input
                // deterministic DSP, we can evaluate them directly.
                let values = interpret_generator(self.arena, init_sig, size)?;
                let mut out = Vec::with_capacity(size);
                for v in values {
                    out.push(self.fir_const_for_table_value(v, elem_ty)?);
                }
                Ok(out)
            }
        }
    }

    /// Converts one compile-time generator sample into the declared FIR table
    /// element type, preserving integer tables as `Int32` and real tables at
    /// the current internal precision.
    fn fir_const_for_table_value(
        &mut self,
        value: f64,
        elem_ty: &FirType,
    ) -> Result<FirId, SignalFirError> {
        let mut b = FirBuilder::new(&mut self.store);
        match elem_ty {
            FirType::Int32 => Ok(b.int32(value as i32)),
            FirType::Float32 => Ok(b.float32(value as f32)),
            FirType::Float64 => Ok(b.float64(value)),
            other => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("unsupported table element type for generator expansion: {other:?}"),
            )),
        }
    }

    /// Normalizes one table index to `[0, table_len)` with integer modulo.
    /// Wraps a table index with `((index % size) + size) % size` to produce a
    /// non-negative in-bounds `Int32` offset.
    ///
    /// The promoter guarantees that all table index signals are Int-typed
    /// (wrapped in `IntCast` if necessary), so `index` is already `Int32` at the
    /// FIR level when this function is called.  No cast is needed.
    fn normalized_table_index(&mut self, index: FirId, table_len: usize) -> FirId {
        let size = {
            let mut b = FirBuilder::new(&mut self.store);
            b.int32(i32::try_from(table_len).unwrap_or(i32::MAX))
        };
        let rem = {
            let mut b = FirBuilder::new(&mut self.store);
            b.binop(FirBinOp::Rem, index, size, FirType::Int32)
        };
        let rem_plus_size = {
            let mut b = FirBuilder::new(&mut self.store);
            b.binop(FirBinOp::Add, rem, size, FirType::Int32)
        };
        let mut b = FirBuilder::new(&mut self.store);
        b.binop(FirBinOp::Rem, rem_plus_size, size, FirType::Int32)
    }

    /// Selects the appropriate index bounds strategy based on the interval of
    /// `index_sig`:
    ///
    /// - **[lo, hi] ⊆ [0, N-1]**: the interval proves the index is always
    ///   in-bounds → emit direct access (no bounds check).
    /// - **[lo, hi] with lo ≥ 0, hi finite but > N-1**: non-negative but may
    ///   overflow → clamp to `min_i(N-1, index)`.
    /// - **[lo, hi] finite with lo < 0**: signed bounds → full clamp
    ///   `min_i(N-1, max_i(0, index))`.
    /// - **Unknown / infinite interval**: fall back to modular wrapping
    ///   `((index % N) + N) % N`.
    ///
    /// This mirrors the C++ reference compiler's interval-driven access
    /// strategy and avoids the systematic over-conservatism of always applying
    /// modular wrapping.
    fn table_index_with_bounds(
        &mut self,
        index_fir: FirId,
        index_sig: SigId,
        table_len: usize,
    ) -> FirId {
        let n = i32::try_from(table_len).unwrap_or(i32::MAX);
        let iv = self.sig_types.get(&index_sig).map(|ty| ty.interval());

        if let Some(iv) = iv {
            let lo = iv.lo();
            let hi = iv.hi();
            if lo.is_finite() && hi.is_finite() {
                let lo_i = lo as i64;
                let hi_i = hi as i64;
                let n_i = n as i64;
                if lo_i >= 0 && hi_i >= 0 && hi_i < n_i {
                    // Index is already provably in [0, N-1] — direct access.
                    return index_fir;
                }
                if lo_i >= 0 {
                    // Non-negative but hi may exceed N-1 — upper clamp only.
                    let upper = self.lower_int32_const(n - 1);
                    self.used_int_fun_names.insert("min_i");
                    let mut b = FirBuilder::new(&mut self.store);
                    return b.fun_call("min_i", &[index_fir, upper], FirType::Int32);
                }
                // Signed bounds — full clamp max(0, min(N-1, x)).
                let zero = self.lower_int32_const(0);
                let upper = self.lower_int32_const(n - 1);
                self.used_int_fun_names.insert("min_i");
                self.used_int_fun_names.insert("max_i");
                let clamped = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.fun_call("min_i", &[upper, index_fir], FirType::Int32)
                };
                let mut b = FirBuilder::new(&mut self.store);
                return b.fun_call("max_i", &[clamped, zero], FirType::Int32);
            }
        }
        // No interval info or infinite bounds — full modular wrapping.
        self.normalized_table_index(index_fir, table_len)
    }

    /// Declares one named struct variable once.
    fn ensure_named_struct_var(&mut self, name: &str, typ: FirType, init: Option<FirId>) {
        if self.named_struct_vars.contains(name) {
            return;
        }
        let mut b = FirBuilder::new(&mut self.store);
        let dec = b.declare_var(name.to_owned(), typ, AccessType::Struct, None);
        self.struct_declarations.push(dec);
        self.named_struct_vars.insert(name.to_owned());
        if let Some(init) = init {
            self.register_reset_init(name.to_owned(), init);
        }
    }

    /// Registers one reset-time assignment for UI controls (`instanceResetUserInterface`).
    fn register_reset_init(&mut self, name: String, init: FirId) {
        if !self.reset_init_seen.insert(name.clone()) {
            return;
        }
        let mut b = FirBuilder::new(&mut self.store);
        self.reset_statements
            .push(b.store_var(name, AccessType::Struct, init));
    }

    /// Registers one clear-time assignment for runtime state (`instanceClear`).
    fn register_clear_init(&mut self, name: String, init: FirId) {
        if !self.clear_init_seen.insert(name.clone()) {
            return;
        }
        let mut b = FirBuilder::new(&mut self.store);
        self.clear_statements
            .push(b.store_var(name, AccessType::Struct, init));
    }

    /// Registers one per-instance table initialization block for
    /// `instanceConstants`.
    fn register_constant_table_init(&mut self, name: String, access: AccessType, values: &[FirId]) {
        if values.is_empty() {
            return;
        }
        let mut stores = Vec::with_capacity(values.len());
        for (index, value) in values.iter().enumerate() {
            let idx = {
                let mut b = FirBuilder::new(&mut self.store);
                b.int32(i32::try_from(index).unwrap_or(i32::MAX))
            };
            let store = {
                let mut b = FirBuilder::new(&mut self.store);
                b.store_table(name.clone(), access, idx, *value)
            };
            stores.push(store);
        }
        let mut b = FirBuilder::new(&mut self.store);
        self.constants_statements.push(b.block(&stores));
    }

    /// Helper to produce a typed unsupported-node error with readable dumped IR.
    fn unsupported_node<T>(&self, sig: SigId, detail: &str) -> Result<T, SignalFirError> {
        Err(SignalFirError::new(
            SignalFirErrorCode::UnsupportedSignalNode,
            format!("{detail} (expr={})", dump_sig_readable(self.arena, sig)),
        ))
    }

    /// Converts a label signal node to UTF-8 text fallback used by foreign refs.
    fn label_text(&self, label: SigId) -> String {
        match self.arena.kind(label) {
            Some(NodeKind::Symbol(s)) => s.to_string(),
            Some(NodeKind::StringLiteral(s)) => s.to_string(),
            Some(NodeKind::Int(v)) => v.to_string(),
            Some(NodeKind::FloatBits(bits)) => f64::from_bits(*bits).to_string(),
            _ => "ui".to_owned(),
        }
    }

    /// Stable generated UI zone variable naming policy.
    fn ui_control_var_name(&self, control: ControlId, prefix: &str) -> String {
        format!("{prefix}{control}")
    }

    /// Looks up the `ControlSpec` for `control`, returning an error if missing.
    fn control_spec(&self, control: ControlId) -> Result<&ui::ControlSpec, SignalFirError> {
        self.ui_program.control(control).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("missing UiProgram control spec for control id {control}"),
            )
        })
    }

    /// Returns the numeric range for `control`, returning an error if absent.
    ///
    /// `kind_name` is included in the error message for diagnostics only.
    fn control_range(
        &self,
        control: ControlId,
        kind_name: &str,
    ) -> Result<ui::ControlRange, SignalFirError> {
        self.control_spec(control)?.range.ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("missing UI range for {kind_name} control id {control}"),
            )
        })
    }

    /// Emits `addMetaDeclare(var, key, value)` calls for each metadata pair.
    fn emit_ui_metadata_for_target(&mut self, var: &str, metadata: &[(String, String)]) {
        for (key, value) in metadata {
            let mut b = FirBuilder::new(&mut self.store);
            self.ui_statements
                .push(b.add_meta_declare(var, key.clone(), value.clone()));
        }
    }

    fn control_metadata_value(
        &self,
        control: ControlId,
        key: &str,
    ) -> Result<Option<String>, SignalFirError> {
        Ok(self
            .control_spec(control)?
            .metadata
            .iter()
            .find_map(|(entry_key, entry_value)| (entry_key == key).then(|| entry_value.clone())))
    }

    /// Emits `addMetaDeclare` calls for every metadata entry attached to `control`.
    fn emit_control_ui_metadata(
        &mut self,
        control: ControlId,
        var: &str,
    ) -> Result<(), SignalFirError> {
        let metadata = self.control_spec(control)?.metadata.clone();
        self.emit_ui_metadata_for_target(var, &metadata);
        Ok(())
    }

    /// Declares the `FaustFloat` struct zone variable for a slider or numentry, idempotent.
    fn ensure_slider_zone(
        &mut self,
        control: ControlId,
        typ: SliderType,
    ) -> Result<String, SignalFirError> {
        if let Some(var) = self.ui_controls.get(&control).cloned() {
            return Ok(var);
        }
        let spec = self.control_spec(control)?;
        let expected_kind = match typ {
            SliderType::Horizontal => ControlKind::HSlider,
            SliderType::Vertical => ControlKind::VSlider,
            SliderType::NumEntry => ControlKind::NumEntry,
        };
        if spec.kind != expected_kind {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "control id {control} kind mismatch: expected {expected_kind:?}, got {:?}",
                    spec.kind
                ),
            ));
        }
        let var = self.ui_control_var_name(
            control,
            match typ {
                SliderType::Horizontal => "fHslider",
                SliderType::Vertical => "fVslider",
                SliderType::NumEntry => "fEntry",
            },
        );
        let range = self.control_range(
            control,
            match typ {
                SliderType::Horizontal => "hslider",
                SliderType::Vertical => "vslider",
                SliderType::NumEntry => "numentry",
            },
        )?;
        let init = self.float_const(range.init);
        self.ensure_named_struct_var(&var, FirType::FaustFloat, Some(init));
        self.ui_controls.insert(control, var.clone());
        Ok(var)
    }

    /// Declares the `FaustFloat` struct zone variable for a bargraph, idempotent.
    fn ensure_bargraph_zone(
        &mut self,
        control: ControlId,
        typ: BargraphType,
    ) -> Result<String, SignalFirError> {
        if let Some(var) = self.ui_controls.get(&control).cloned() {
            return Ok(var);
        }
        let spec = self.control_spec(control)?;
        let expected_kind = match typ {
            BargraphType::Horizontal => ControlKind::HBargraph,
            BargraphType::Vertical => ControlKind::VBargraph,
        };
        if spec.kind != expected_kind {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "control id {control} kind mismatch: expected {expected_kind:?}, got {:?}",
                    spec.kind
                ),
            ));
        }
        let var = self.ui_control_var_name(
            control,
            match typ {
                BargraphType::Horizontal => "fHbargraph",
                BargraphType::Vertical => "fVbargraph",
            },
        );
        let init = self.float_const(0.0);
        self.ensure_named_struct_var(&var, FirType::FaustFloat, Some(init));
        self.ui_controls.insert(control, var.clone());
        Ok(var)
    }

    /// Declares the opaque `Sound` struct zone variable for a soundfile, idempotent.
    fn ensure_soundfile_zone(&mut self, control: ControlId) -> Result<String, SignalFirError> {
        if let Some(var) = self.soundfiles.get(&control).cloned() {
            return Ok(var);
        }
        let spec = self.control_spec(control)?;
        if spec.kind != ControlKind::Soundfile {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "control id {control} kind mismatch: expected {:?}, got {:?}",
                    ControlKind::Soundfile,
                    spec.kind
                ),
            ));
        }
        let var = format!("fSound{control}");
        self.ensure_named_struct_var(&var, FirType::Sound, None);
        self.soundfiles.insert(control, var.clone());
        Ok(var)
    }

    /// Drives emission of the entire `buildUserInterface` body from the root UI node.
    ///
    /// Clears any previous `ui_statements` accumulator before walking the tree.
    fn emit_ui_program(&mut self) -> Result<(), SignalFirError> {
        if self.ui_program.is_empty() {
            self.ui_statements.clear();
            return Ok(());
        }
        self.ui_statements.clear();
        self.emit_ui_node(self.ui_program.root)
    }

    /// Recursively emits FIR UI calls for one UI tree node.
    ///
    /// Dispatches on group containers (open/close box), input controls
    /// (button, checkbox, slider, numentry), output controls (bargraph),
    /// and soundfile declarations.
    fn emit_ui_node(&mut self, node: ui::UiId) -> Result<(), SignalFirError> {
        match match_ui(&self.ui_program.arena, node) {
            UiMatch::Group {
                kind,
                label,
                metadata,
                children,
            } => {
                let typ = match kind {
                    UiGroupKind::Vertical => UiBoxType::Vertical,
                    UiGroupKind::Horizontal => UiBoxType::Horizontal,
                    UiGroupKind::Tab => UiBoxType::Tab,
                };
                self.emit_ui_metadata_for_target("0", &metadata);
                let mut b = FirBuilder::new(&mut self.store);
                self.ui_statements.push(b.open_box(typ, label));
                for child in children {
                    self.emit_ui_node(child)?;
                }
                let mut b = FirBuilder::new(&mut self.store);
                self.ui_statements.push(b.close_box());
                Ok(())
            }
            UiMatch::InputControl(control) => {
                let spec = self.control_spec(control)?;
                let kind = spec.kind;
                let label = spec.label.clone();
                match kind {
                    ControlKind::Button => {
                        let var = self.ensure_button_zone(control, ButtonType::Button)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui_statements
                            .push(b.add_button(ButtonType::Button, label, var));
                    }
                    ControlKind::Checkbox => {
                        let var = self.ensure_button_zone(control, ButtonType::Checkbox)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui_statements
                            .push(b.add_button(ButtonType::Checkbox, label, var));
                    }
                    ControlKind::VSlider => {
                        let range = self.control_range(control, "vslider")?;
                        let var = self.ensure_slider_zone(control, SliderType::Vertical)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui_statements.push(b.add_slider(
                            SliderType::Vertical,
                            label,
                            var,
                            SliderRange {
                                init: range.init,
                                lo: range.min,
                                hi: range.max,
                                step: range.step,
                            },
                        ));
                    }
                    ControlKind::HSlider => {
                        let range = self.control_range(control, "hslider")?;
                        let var = self.ensure_slider_zone(control, SliderType::Horizontal)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui_statements.push(b.add_slider(
                            SliderType::Horizontal,
                            label,
                            var,
                            SliderRange {
                                init: range.init,
                                lo: range.min,
                                hi: range.max,
                                step: range.step,
                            },
                        ));
                    }
                    ControlKind::NumEntry => {
                        let range = self.control_range(control, "numentry")?;
                        let var = self.ensure_slider_zone(control, SliderType::NumEntry)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui_statements.push(b.add_slider(
                            SliderType::NumEntry,
                            label,
                            var,
                            SliderRange {
                                init: range.init,
                                lo: range.min,
                                hi: range.max,
                                step: range.step,
                            },
                        ));
                    }
                    other => {
                        return Err(SignalFirError::new(
                            SignalFirErrorCode::UnsupportedSignalNode,
                            format!("input UI leaf points to non-input control kind {other:?}"),
                        ));
                    }
                }
                Ok(())
            }
            UiMatch::OutputControl(control) => {
                let spec = self.control_spec(control)?;
                let kind = spec.kind;
                let label = spec.label.clone();
                match kind {
                    ControlKind::VBargraph => {
                        let range = self.control_range(control, "vbargraph")?;
                        let var = self.ensure_bargraph_zone(control, BargraphType::Vertical)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui_statements.push(b.add_bargraph(
                            BargraphType::Vertical,
                            label,
                            var,
                            range.min,
                            range.max,
                        ));
                    }
                    ControlKind::HBargraph => {
                        let range = self.control_range(control, "hbargraph")?;
                        let var = self.ensure_bargraph_zone(control, BargraphType::Horizontal)?;
                        self.emit_control_ui_metadata(control, &var)?;
                        let mut b = FirBuilder::new(&mut self.store);
                        self.ui_statements.push(b.add_bargraph(
                            BargraphType::Horizontal,
                            label,
                            var,
                            range.min,
                            range.max,
                        ));
                    }
                    other => {
                        return Err(SignalFirError::new(
                            SignalFirErrorCode::UnsupportedSignalNode,
                            format!("output UI leaf points to non-bargraph control kind {other:?}"),
                        ));
                    }
                }
                Ok(())
            }
            UiMatch::Soundfile(control) => {
                let label = self.control_spec(control)?.label.clone();
                let url = self
                    .control_metadata_value(control, "url")?
                    .unwrap_or_default();
                let var = self.ensure_soundfile_zone(control)?;
                let mut b = FirBuilder::new(&mut self.store);
                self.ui_statements
                    .push(b.add_soundfile_with_url(label, url, var));
                Ok(())
            }
            UiMatch::Unknown => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "malformed UiProgram node".to_owned(),
            )),
        }
    }
}

// ── Arithmetic, selection, and recursion projection lowering ───────────────

impl<'a> SignalToFirLower<'a> {
    /// Lowers one binary signal operator to FIR binop.
    ///
    /// Relies on the promoter invariant: every `BinOp` operand already has the
    /// correct domain type (mixed Int/Real pairs wrapped in `FloatCast`; bitwise
    /// and shift operands in `IntCast`; `Div` operands always Real).
    /// Comparisons keep same-typed numeric operands and produce `Int32` results
    /// for C++ parity.  No implicit coercion is performed here.
    fn lower_binop(
        &mut self,
        node: SigId,
        op: BinOp,
        lhs_sig: SigId,
        rhs_sig: SigId,
    ) -> Result<FirId, SignalFirError> {
        let result_ty = self.signal_fir_type(node)?;
        let lhs = self.lower_signal(lhs_sig)?;
        let rhs = self.lower_signal(rhs_sig)?;
        let (fir_op, typ) = map_binop(op, result_ty).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedBinOp,
                format!("unsupported SIGBINOP operator `{}` in Step 2A", op.name()),
            )
        })?;
        let lhs_ty = self.store.value_type(lhs).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedBinOp,
                format!(
                    "missing FIR type for left operand of `{}` in Step 2A",
                    op.name()
                ),
            )
        })?;
        let rhs_ty = self.store.value_type(rhs).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedBinOp,
                format!(
                    "missing FIR type for right operand of `{}` in Step 2A",
                    op.name()
                ),
            )
        })?;
        let operands_ok = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                lhs_ty == typ && rhs_ty == typ
            }
            BinOp::And | BinOp::Or | BinOp::Xor | BinOp::Lsh | BinOp::ARsh | BinOp::LRsh => {
                lhs_ty == FirType::Int32 && rhs_ty == FirType::Int32
            }
            BinOp::Gt | BinOp::Lt | BinOp::Ge | BinOp::Le | BinOp::Eq | BinOp::Ne => {
                lhs_ty == rhs_ty
                    && matches!(lhs_ty, FirType::Int32 | FirType::Float32 | FirType::Float64)
            }
        };
        if !operands_ok {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedBinOp,
                format!(
                    "prepared SIGBINOP operands for `{}` violate fast-lane typing contract: lhs={lhs_ty:?}, rhs={rhs_ty:?}, result={typ:?} (expr={})",
                    op.name(),
                    dump_sig_readable(self.arena, node)
                ),
            ));
        }
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.binop(fir_op, lhs, rhs, typ))
    }

    /// Lowers one unary math intrinsic call.
    fn lower_math1(&mut self, op: FirMathOp, value: SigId) -> Result<FirId, SignalFirError> {
        let value = self.lower_signal(value)?;
        self.used_math_ops.insert(op);
        // Math calls operate on and return the internal real type.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.math_call(op, &[value], real_ty))
    }

    /// Lowers one binary math intrinsic call.
    fn lower_math2(
        &mut self,
        op: FirMathOp,
        lhs: SigId,
        rhs: SigId,
    ) -> Result<FirId, SignalFirError> {
        let lhs = self.lower_signal(lhs)?;
        let rhs = self.lower_signal(rhs)?;
        self.used_math_ops.insert(op);
        // Math calls operate on and return the internal real type.
        let real_ty = self.real_ty();
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.math_call(op, &[lhs, rhs], real_ty))
    }

    /// Lowers `min`/`max`, preserving integer recursion/state when the reduced
    /// typer kept both operands in the integer domain.
    ///
    /// Source provenance (C++):
    /// - `compiler/extended/minprim.hh`
    /// - `compiler/extended/maxprim.hh`
    ///
    /// Integer `min/max` remain explicit FIR function calls (`min_i` / `max_i`)
    /// so backends can apply the same target-local renaming policy as the C++
    /// compiler instead of hardwiring a branch synthesis here.
    fn lower_minmax(
        &mut self,
        node: SigId,
        lhs_sig: SigId,
        rhs_sig: SigId,
        is_min: bool,
    ) -> Result<FirId, SignalFirError> {
        let result_ty = self.signal_fir_type(node)?;
        if result_ty == FirType::Int32 {
            let lhs = self.lower_signal(lhs_sig)?;
            let rhs = self.lower_signal(rhs_sig)?;
            self.used_int_fun_names
                .insert(if is_min { "min_i" } else { "max_i" });
            let mut b = FirBuilder::new(&mut self.store);
            return Ok(b.fun_call(
                if is_min { "min_i" } else { "max_i" },
                &[lhs, rhs],
                FirType::Int32,
            ));
        }
        self.lower_math2(
            if is_min {
                FirMathOp::Min
            } else {
                FirMathOp::Max
            },
            lhs_sig,
            rhs_sig,
        )
    }

    /// Lowers `abs`, preserving integer recursion/state when the reduced typer
    /// kept the operand in the integer domain.
    ///
    /// Source provenance (C++):
    /// - `compiler/extended/absprim.hh`
    ///
    /// Integer `abs` stays an explicit function call so backends can preserve
    /// the target-local parity spelling and overflow contract.
    fn lower_abs(&mut self, node: SigId, value_sig: SigId) -> Result<FirId, SignalFirError> {
        let result_ty = self.signal_fir_type(node)?;
        if result_ty == FirType::Int32 {
            let value = self.lower_signal(value_sig)?;
            self.used_int_fun_names.insert("abs");
            let mut b = FirBuilder::new(&mut self.store);
            return Ok(b.fun_call("abs", &[value], FirType::Int32));
        }
        self.lower_math1(FirMathOp::Abs, value_sig)
    }

    /// Lowers one numeric cast.
    fn lower_cast(&mut self, typ: FirType, value: SigId) -> Result<FirId, SignalFirError> {
        let value = self.lower_signal(value)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.cast(typ, value))
    }

    /// Lowers one bitcast operation.
    fn lower_bitcast(&mut self, typ: FirType, value: SigId) -> Result<FirId, SignalFirError> {
        let value = self.lower_signal(value)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.bitcast(typ, value))
    }

    /// Lowers `select2` with explicit result-type selection.
    fn lower_select2(
        &mut self,
        node: SigId,
        cond: SigId,
        then_value: SigId,
        else_value: SigId,
    ) -> Result<FirId, SignalFirError> {
        let cond = self.lower_signal(cond)?;
        let then_value = self.lower_signal(then_value)?;
        let else_value = self.lower_signal(else_value)?;
        let real_ty = self.signal_fir_type(node)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.select2(cond, then_value, else_value, real_ty))
    }

    /// Lowers recursion projection nodes after the mandatory
    /// `de_bruijn_to_sym` preparation step.
    ///
    /// Expects symbolic recursion payloads (`SYMREC` / `SYMREF`) — the normal
    /// fast-lane input form produced by `signal_prepare`.
    ///
    /// **Deferred body evaluation**: on the first `SIGPROJ` encountered for a
    /// group, this method allocates 2-slot arrays for all output bodies, pushes
    /// the group onto `recursion_stack`, lowers every body signal (emitting
    /// stores into the sample loop immediate phase), then pops the stack.  Subsequent
    /// `SIGPROJ` nodes for the same group skip body evaluation entirely (the
    /// `scheduled_state_updates` dedup guard keyed by `group` SigId ensures
    /// exactly one body-lowering pass per sample).
    ///
    /// **Fast path** (active reference inside a body being lowered): when the
    /// canonical recursion-carrier resolver finds the group on the stack, the
    /// current-slot value is read directly — no recursion into `lower_signal`
    /// occurs, which breaks the cycle.
    fn lower_proj(
        &mut self,
        node: SigId,
        index: i32,
        group: SigId,
    ) -> Result<FirId, SignalFirError> {
        let index_usize = usize::try_from(index).map_err(|_| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("negative SIGPROJ index {index} in Step 2C.2"),
            )
        })?;
        // ── Fast path: active reference inside a body being lowered ──
        if let Some(rec_ref) =
            resolve_active_recursion_carrier(self.arena, &self.recursion, group, index_usize)?
        {
            let real_ty = self.signal_fir_type(node)?;
            let current_index = if rec_ref.strategy == RecursionStorageStrategy::ExactShift {
                self.lower_int32_const(0)
            } else if rec_ref.strategy == RecursionStorageStrategy::Circular {
                self.global_circular_current_index(rec_ref.info.size)
            } else {
                self.lower_int32_const(0)
            };
            let mut recursion_ctx = RecursionLoweringCtx {
                store: &mut self.store,
                immediate_statements: &mut self.sample_phases.immediate,
                post_output_statements: &mut self.sample_phases.post_output,
                next_loop_var_id: &mut self.next_loop_var_id,
            };
            return Ok(recursion_ctx.load_feedback_carrier(&rec_ref.info, current_index, real_ty));
        }

        // ── Fast path: already materialized scalar carrier current value ──
        if let Some(current_value) = self.load_scalar_recursion_current_value(group, index_usize)? {
            return Ok(current_value);
        }

        // ── Fast path: already materialized array-backed carrier ──
        if let Some(rec_ref) =
            self.recursion
                .resolve_materialized_carrier(self.arena, group, index_usize)
        {
            let real_ty = self.signal_fir_type(node)?;
            let current_index = if rec_ref.strategy == RecursionStorageStrategy::ExactShift {
                self.lower_int32_const(0)
            } else {
                self.global_circular_current_index(rec_ref.info.size)
            };
            let mut recursion_ctx = RecursionLoweringCtx {
                store: &mut self.store,
                immediate_statements: &mut self.sample_phases.immediate,
                post_output_statements: &mut self.sample_phases.post_output,
                next_loop_var_id: &mut self.next_loop_var_id,
            };
            return Ok(recursion_ctx.load_feedback_carrier(&rec_ref.info, current_index, real_ty));
        }

        // ── Fast path: SigBlockReverseAD carrier ──
        if let SigMatch::BlockReverseAD {
            body,
            primal_count,
            seeds,
            cotangents,
            policy: _,
        } = match_sig(self.arena, group)
        {
            let pc = usize::try_from(primal_count).map_err(|_| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!("negative primal_count in BlockReverseAD Proj({index})"),
                )
            })?;
            let body_sigs = list_to_vec(self.arena, body).ok_or_else(|| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    "malformed body list in BlockReverseAD".to_string(),
                )
            })?;
            let seed_sigs = list_to_vec(self.arena, seeds).ok_or_else(|| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    "malformed seed list in BlockReverseAD".to_string(),
                )
            })?;
            let cotangent_sigs = list_to_vec(self.arena, cotangents).ok_or_else(|| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    "malformed cotangent list in BlockReverseAD".to_string(),
                )
            })?;
            return self.lower_block_reverse_ad_proj(
                node,
                group,
                index_usize,
                pc,
                &body_sigs,
                &seed_sigs,
                &cotangent_sigs,
            );
        }

        // ── Decode all body signals from the group ──
        let RecursionGroupProjection {
            var,
            bodies,
            canonical_index,
        } = decode_group_projection(self.arena, node, index, group)?;

        // ── Allocate recursion arrays for ALL bodies ──
        //
        // Each output slot gets its own array keyed by `(group, index)` in the
        // recursion state, intentionally separate from `state_name_by_node` so
        // that a `lower_delay_state` call inside the body expression never
        // aliases the group's output carrier.
        let mut body_infos = Vec::with_capacity(bodies.len());
        for body in &bodies {
            let state_ty = self.signal_fir_type(*body)?;
            let init = match state_ty {
                FirType::Int32 => self.lower_int32_const(0),
                FirType::Float32 | FirType::Float64 | FirType::FaustFloat => self.float_const(0.0),
                other => {
                    return Err(SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        format!("unsupported recursive state type in Step 2C.2: {other:?}"),
                    ));
                }
            };
            body_infos.push((state_ty, init));
        }
        let group_arrays = {
            let mut ctx = RecursionAllocCtx {
                arena: self.arena,
                delay: &self.delay,
                store: &mut self.store,
                struct_declarations: &mut self.struct_declarations,
                clear_statements: &mut self.clear_statements,
                clear_init_seen: &mut self.clear_init_seen,
                next_loop_var_id: &mut self.next_loop_var_id,
                recursion: &mut self.recursion,
            };
            ctx.allocate_group_arrays(group, &body_infos)?
        };

        // ── Push group context, lower ALL bodies, emit stores ──
        // Use recursion-owned scheduling so each group's body pass runs only once.
        if self.recursion.mark_group_scheduled(group) {
            self.with_active_recursion_group(var, group_arrays.clone(), |this, active_arrays| {
                let zero = this.lower_int32_const(0);
                let mut body_values = Vec::with_capacity(bodies.len());
                let mut current_indexes = Vec::with_capacity(active_arrays.len());
                for (i, body) in bodies.iter().enumerate() {
                    body_values.push(this.lower_signal(*body)?);
                    let current_index = match active_arrays[i].storage_strategy() {
                        RecursionStorageStrategy::SingleScalar => {
                            this.bind_scalar_recursion_current_value(
                                group,
                                i,
                                &active_arrays[i],
                                body_values[i],
                            );
                            zero
                        }
                        RecursionStorageStrategy::ExactShift => zero,
                        RecursionStorageStrategy::Circular => {
                            this.global_circular_current_index(active_arrays[i].size)
                        }
                    };
                    current_indexes.push(current_index);
                }
                if active_arrays.len() > 1 {
                    // Multi-output recursion is a simultaneous update. Snapshot
                    // every body before carrier stores so one lane cannot read
                    // another lane's already-updated current slot.
                    for (i, body_value) in body_values.iter_mut().enumerate() {
                        let typ = active_arrays[i].typ.clone();
                        let prefix = if typ == FirType::Int32 {
                            "iRecBody"
                        } else {
                            "fRecBody"
                        };
                        let name = format!("{prefix}{}", this.next_loop_var_id);
                        this.next_loop_var_id += 1;
                        let declare = {
                            let mut b = FirBuilder::new(&mut this.store);
                            b.declare_var(
                                name.clone(),
                                typ.clone(),
                                AccessType::Stack,
                                Some(*body_value),
                            )
                        };
                        this.sample_phases.immediate.push(declare);
                        *body_value = {
                            let mut b = FirBuilder::new(&mut this.store);
                            b.load_var(name, AccessType::Stack, typ)
                        };
                    }
                }
                let mut recursion_ctx = RecursionLoweringCtx {
                    store: &mut this.store,
                    immediate_statements: &mut this.sample_phases.immediate,
                    post_output_statements: &mut this.sample_phases.post_output,
                    next_loop_var_id: &mut this.next_loop_var_id,
                };
                recursion_ctx.emit_group_body_updates(
                    active_arrays,
                    &body_values,
                    &current_indexes,
                );
                for (i, info) in active_arrays.iter().enumerate() {
                    if info.storage_strategy() == RecursionStorageStrategy::SingleScalar {
                        let binding = this
                            .recursion
                            .current_value_binding(this.arena, group, i)
                            .expect("scalar recursion binding should be recorded before finalize");
                        let current_value = {
                            let mut b = FirBuilder::new(&mut this.store);
                            b.load_var(binding.name, AccessType::Stack, binding.typ.clone())
                        };
                        let store_state = {
                            let mut b = FirBuilder::new(&mut this.store);
                            b.store_var(info.name.clone(), AccessType::Struct, current_value)
                        };
                        this.sample_phases.post_output.push(store_state);
                    }
                }
                Ok(())
            })?;
        }

        // ── Return the result for the requested index ──
        let info = &group_arrays[canonical_index];
        let out_ty = self.signal_fir_type(node)?;
        if info.storage_strategy() == RecursionStorageStrategy::SingleScalar {
            let current_value = self
                .load_scalar_recursion_current_value(group, canonical_index)?
                .expect("scalar recursion current value should be available after scheduling");
            debug_assert_eq!(
                info.typ, out_ty,
                "SIGPROJ type mismatch: carrier={:?}, node={:?}",
                info.typ, out_ty
            );
            return Ok(current_value);
        }
        let zero = self.lower_int32_const(0);
        let circular_index = if info.storage_strategy() == RecursionStorageStrategy::ExactShift {
            zero
        } else {
            self.global_circular_current_index(info.size)
        };
        let mut recursion_ctx = RecursionLoweringCtx {
            store: &mut self.store,
            immediate_statements: &mut self.sample_phases.immediate,
            post_output_statements: &mut self.sample_phases.post_output,
            next_loop_var_id: &mut self.next_loop_var_id,
        };
        let current_index = recursion_ctx.current_index_for_carrier(info, zero, circular_index);
        let out = recursion_ctx.load_feedback_carrier(info, current_index, info.typ.clone());
        debug_assert_eq!(
            info.typ, out_ty,
            "SIGPROJ type mismatch: array={:?}, node={:?}",
            info.typ, out_ty
        );
        Ok(out)
    }
}

/// Maps signal-level operators to FIR operators with result typing policy.
///
/// `real_ty` is the internal DSP computation type (e.g. `Float32` / `Float64`).
/// It is used for arithmetic operators whose result is a real-valued sample.
/// Comparison operators produce `Int32` in the fast-lane, matching the normal
/// C++ signal typing path where comparisons are "boolean int" values. This is
/// distinct from the optional backend-specific `SignalBool2IntPromotion` pass:
/// the fast-lane does not rely on that pass and must preserve the standard
/// signal semantics directly. Bitwise operators also produce `Int32`.
fn map_binop(op: BinOp, real_ty: FirType) -> Option<(FirBinOp, FirType)> {
    match op {
        // Arithmetic operators: result is the internal real type.
        BinOp::Add => Some((FirBinOp::Add, real_ty)),
        BinOp::Sub => Some((FirBinOp::Sub, real_ty)),
        BinOp::Mul => Some((FirBinOp::Mul, real_ty)),
        BinOp::Div => Some((FirBinOp::Div, real_ty)),
        BinOp::Rem => Some((FirBinOp::Rem, real_ty)),
        // Comparison operators: result is Int32 ("boolean int") for parity
        // with the standard C++ signal typing path.
        BinOp::Gt => Some((FirBinOp::Gt, FirType::Int32)),
        BinOp::Lt => Some((FirBinOp::Lt, FirType::Int32)),
        BinOp::Ge => Some((FirBinOp::Ge, FirType::Int32)),
        BinOp::Le => Some((FirBinOp::Le, FirType::Int32)),
        BinOp::Eq => Some((FirBinOp::Eq, FirType::Int32)),
        BinOp::Ne => Some((FirBinOp::Ne, FirType::Int32)),
        // Bitwise operators: result is Int32 — independent of real_ty.
        BinOp::And => Some((FirBinOp::And, FirType::Int32)),
        BinOp::Or => Some((FirBinOp::Or, FirType::Int32)),
        BinOp::Xor => Some((FirBinOp::Xor, FirType::Int32)),
        BinOp::Lsh => Some((FirBinOp::Lsh, FirType::Int32)),
        BinOp::ARsh => Some((FirBinOp::ARsh, FirType::Int32)),
        BinOp::LRsh => Some((FirBinOp::LRsh, FirType::Int32)),
    }
}
