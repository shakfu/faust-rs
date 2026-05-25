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
use signals::{
    BinOp, SigId, SigMatch,
    ad_rules::{
        RadBinOpRule, RadBinaryMathRule, RadFormulaBuilder, RadUnaryMathRule,
        rad_binary_contributions, rad_binary_math_rule, rad_binop_contributions, rad_binop_rule,
        rad_unary_contribution, rad_unary_math_rule,
    },
    dump_sig_readable, match_sig,
};
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

mod arithmetic;
mod bra;
mod rad_formula_builder;
mod state;
mod tables;
mod ui_lowering;
use rad_formula_builder::FirRadFormulaBuilder;

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

    /// Emits the BRA reverse update for a supported unary math node.
    ///
    /// Unlike the pure Signal RAD path, BRA cannot freely rebuild every
    /// operand expression during the reverse sweep: operands may be temporal,
    /// recursive, or otherwise already materialized in forward storage. This
    /// method therefore performs the tape-aware loads first, then delegates
    /// only the pointwise algebra to `ad_rules`. For formulas that can reuse the
    /// forward node output (`exp`, `sqrt`, `abs`), `sig` is loaded as `primal`
    /// so the local transpose uses the recorded forward value rather than a
    /// second computation.
    fn propagate_bra_unary_math_adj(
        &mut self,
        rule: RadUnaryMathRule,
        sig: SigId,
        x: SigId,
        y_bar: FirId,
        adj: &mut std::collections::HashMap<SigId, FirId>,
    ) -> Result<(), SignalFirError> {
        let real_ty = self.real_ty.clone();
        let x_fir = self.load_bra_fwd_value(x)?;
        // The shared formula only sees values. For rules whose derivative can
        // reuse the forward output, pass the tape-loaded current node value so
        // the reverse sweep does not recompute non-trivial temporal operands.
        let primal = match rule {
            RadUnaryMathRule::Exp | RadUnaryMathRule::Sqrt | RadUnaryMathRule::Abs => {
                self.load_bra_fwd_value(sig)?
            }
            _ => x_fir,
        };
        let mut b = FirRadFormulaBuilder::new(self, real_ty.clone());
        let x_adj = rad_unary_contribution(&mut b, rule, x_fir, primal, y_bar);
        Self::add_to_adjoint(&mut self.store, adj, x, x_adj, real_ty);
        Ok(())
    }

    /// Emits the BRA reverse updates for a supported binary math node.
    ///
    /// This method is the FIR/BRA counterpart of `propagate_binary_math`: it
    /// loads both forward operand values from BRA storage, lets the shared
    /// `ad_rules` formula build the two local cotangents in FIR, then
    /// accumulates them into the reverse adjoint map. `pow` additionally needs
    /// the stored forward result of `sig` for its exponent contribution; other
    /// binary math rules depend only on the loaded operands and ignore the
    /// `primal` placeholder.
    fn propagate_bra_binary_math_adj(
        &mut self,
        rule: RadBinaryMathRule,
        lhs: SigId,
        rhs: SigId,
        sig: SigId,
        y_bar: FirId,
        adj: &mut std::collections::HashMap<SigId, FirId>,
    ) -> Result<(), SignalFirError> {
        let real_ty = self.real_ty.clone();
        let lhs_fir = self.load_bra_fwd_value(lhs)?;
        let rhs_fir = self.load_bra_fwd_value(rhs)?;
        // `pow` needs its forward output for the exponent derivative. Other
        // binary rules compute their local transpose from operand values only,
        // so the placeholder is intentionally ignored by the shared helper.
        let primal = match rule {
            RadBinaryMathRule::Pow => self.load_bra_fwd_value(sig)?,
            _ => lhs_fir,
        };
        let mut b = FirRadFormulaBuilder::new(self, real_ty.clone());
        let (lhs_adj, rhs_adj) =
            rad_binary_contributions(&mut b, rule, lhs_fir, rhs_fir, primal, y_bar);
        Self::add_to_adjoint(&mut self.store, adj, lhs, lhs_adj, real_ty.clone());
        Self::add_to_adjoint(&mut self.store, adj, rhs, rhs_adj, real_ty);
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

// ── Constant, UI, soundfile, and table lowering ────────────────────────────

impl<'a> SignalToFirLower<'a> {
    /// Emits one `Int32` FIR constant.
    fn lower_int32_const(&mut self, value: i32) -> FirId {
        let mut b = FirBuilder::new(&mut self.store);
        b.int32(value)
    }

    /// Helper to produce a typed unsupported-node error with readable dumped IR.
    fn unsupported_node<T>(&self, sig: SigId, detail: &str) -> Result<T, SignalFirError> {
        Err(SignalFirError::new(
            SignalFirErrorCode::UnsupportedSignalNode,
            format!("{detail} (expr={})", dump_sig_readable(self.arena, sig)),
        ))
    }
}

// ── Arithmetic, selection, and recursion projection lowering ───────────────
