//! FIR module assembly — `build_module` entry point.
//!
//! Defines [`RadReverseState`], the sub-state struct for RAD reverse-time
//! scheduling that is populated post-construction in `build_module`.
//!
//! Owns the single crate-visible function [`build_module`] that accepts
//! pre-validated planning data and a prepared signal forest and assembles a
//! self-contained [`SignalFirOutput`] with all Faust lifecycle sections in
//! deterministic order: `metadata`, `instanceConstants`,
//! `instanceResetUserInterface`, `instanceClear`, `buildUserInterface`,
//! and `compute`.
//!
//! All other submodules in `module/` provide `impl SignalToFirLower` methods
//! that are invoked from the orchestration logic here.

use super::*;
use crate::signal_fir::ComputeMode;
use crate::signal_fir::loop_graph::LoopKind;

/// RAD reverse-time scheduling state, populated post-construction in `build_module`.
#[derive(Default)]
pub(super) struct RadReverseState {
    /// Forward output lanes already computed before the reverse-time loop.
    ///
    /// Phase-E1 RAD uses the public bundle layout `[primals..., gradients...]`.
    /// This map lets coefficient-gradient terms in the reverse loop replay
    /// `Delay1(primal)` from the primal output buffer instead of reading the
    /// recursion carrier in reverse-time order.
    pub(super) forward_output_by_sig: HashMap<SigId, usize>,
    /// Same map as [`Self::forward_output_by_sig`], keyed by the prepared
    /// readable signal shape to survive equivalent but non-identical `SigId`s.
    pub(super) forward_output_by_sig_key: HashMap<String, usize>,
    /// True while lowering the reverse-time sample-loop slice.
    pub(super) lowering_reverse_loop: bool,
}

/// Emits one sample loop node's `for`. Scalar mode — and any reverse-time loop —
/// stays a single `for (i0 = 0; i0 < count; i0++)`. Vector mode (`-vec`) wraps a
/// forward loop in the chunk driver (roadmap P6, V5):
///
/// ```text
/// for (vindex = 0; vindex < count; vindex += vec_size) {
///     int vend = (vindex + vec_size < count) ? vindex + vec_size : count;
///     for (i0 = vindex; i0 < vend; i0++) { <exec> }
/// }
/// ```
///
/// It is **bit-exact vs scalar**: the inner loop keeps the *global* sample index
/// `i0` (no I/O pointer rebasing), so the body is unchanged and only the loop
/// bounds are chunked. The C compiler can auto-vectorize the inner loop when the
/// body has no loop-carried dependency; loop *separation* (splitting recursive
/// groups into their own serial loops) is a later slice.
///
/// Only `Vectorizable` (state-free) slices are chunked: a slice that carries
/// cross-sample state has a loop-carried dependency the C compiler cannot
/// vectorize as one block, so chunking it is pure overhead — it stays one plain
/// serial loop. Reverse-time loops likewise force scalar mode (chunking would
/// change the implicit TBPTT window from `count` to `vec_size`, vector doc §5).
fn emit_sample_loop(
    store: &mut FirStore,
    exec: &[FirId],
    is_reverse: bool,
    kind: LoopKind,
    compute_mode: ComputeMode,
) -> FirId {
    let vec_size = match compute_mode {
        ComputeMode::Vector { vec_size, .. } if !is_reverse && kind == LoopKind::Vectorizable => {
            vec_size.max(1)
        }
        _ => {
            let mut b = FirBuilder::new(store);
            let upper = b.load_var("count", AccessType::FunArgs, FirType::Int32);
            let body = b.block(exec);
            return b.simple_for_loop("i0", upper, body, is_reverse);
        }
    };
    let vs = i32::try_from(vec_size).unwrap_or(i32::MAX);
    let mut b = FirBuilder::new(store);

    // vend = (vindex + vs < count) ? vindex + vs : count. Comparisons are Int32.
    let vindex_r = b.load_var("vindex", AccessType::Loop, FirType::Int32);
    let vsz = b.int32(vs);
    let next = b.binop(FirBinOp::Add, vindex_r, vsz, FirType::Int32);
    let count_hi = b.load_var("count", AccessType::FunArgs, FirType::Int32);
    let cond = b.binop(FirBinOp::Lt, next, count_hi, FirType::Int32);
    let vend_val = b.select2(cond, next, count_hi, FirType::Int32);
    let vend_decl = b.declare_var("vend", FirType::Int32, AccessType::Stack, Some(vend_val));

    // Inner: for (i0 = vindex; i0 < vend; i0++) { <exec> } — body reads the
    // global `i0` exactly as scalar does. A general `ForLoop` init is the loop
    // variable's `DeclareVar`.
    let i0_start = b.load_var("vindex", AccessType::Loop, FirType::Int32);
    let i0_init = b.declare_var("i0", FirType::Int32, AccessType::Loop, Some(i0_start));
    let i0_end = b.load_var("vend", AccessType::Stack, FirType::Int32);
    let step1 = b.int32(1);
    let inner_body = b.block(exec);
    let inner = b.for_loop("i0", i0_init, i0_end, step1, inner_body, false);

    let outer_body = b.block(&[vend_decl, inner]);

    // Outer chunk loop: for (vindex = 0; vindex < count; vindex += vs).
    let zero = b.int32(0);
    let vindex_init = b.declare_var("vindex", FirType::Int32, AccessType::Loop, Some(zero));
    let count_end = b.load_var("count", AccessType::FunArgs, FirType::Int32);
    let vsz_step = b.int32(vs);
    b.for_loop(
        "vindex",
        vindex_init,
        count_end,
        vsz_step,
        outer_body,
        false,
    )
}

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
/// `module/` remains responsible for orchestration:
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
pub(crate) fn build_module<'a>(
    plan: &SignalFirPlan,
    module_name: &str,
    arena: &'a TreeArena,
    signals: &[SigId],
    ui: &'a UiProgram,
    types: &'a HashMap<SigId, SimpleSigType>,
    sig_types: &'a HashMap<SigId, SigType>,
    real_ty: FirType,
    max_copy_delay: u32,
    delay_line_threshold: u32,
    compute_mode: ComputeMode,
    clocked: Option<clocked::ClockedPlan<'a>>,
) -> Result<SignalFirOutput, SignalFirError> {
    let delay_opts = DelayOptions {
        max_copy_delay,
        delay_line_threshold,
    };
    let (sig_ref_counts, sig_at_boundary, konst_escapes) =
        analyze_signal_sharing(arena, signals, sig_types);
    let placement = setup::PlacementInfo::new(sig_ref_counts, sig_at_boundary, konst_escapes);
    let mut lower = SignalToFirLower::new(
        arena,
        ui,
        types,
        sig_types,
        plan.num_inputs,
        real_ty,
        placement,
        delay_opts,
    );
    lower.clocked = clocked.map(clocked::ClockedState::new);
    lower.ensure_sample_rate_var();
    lower.prepare_delay_lines(signals)?;
    lower.assign_clocked_delay_cursors()?;
    let reverse_time_outputs = classify_reverse_time_outputs(lower.arena, signals);
    lower.rad_reverse.forward_output_by_sig = signals
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
            .sections
            .control_statements
            .push(b.label("signal_fir_fastlane_step2a: executable base slice"));
        lower.sections.control_statements.push(b.label(format!(
            "io: inputs={} outputs={}",
            plan.num_inputs, plan.num_outputs
        )));
        lower
            .sections
            .control_statements
            .push(b.label(format!("signals: {}", plan.signal_count)));
    }

    let has_forward_outputs = reverse_time_outputs.iter().any(|is_reverse| !*is_reverse);
    let has_reverse_outputs = reverse_time_outputs.iter().any(|is_reverse| *is_reverse);
    if has_reverse_outputs {
        // Readable structural fallback keys are only needed when the RAD
        // reverse-time loop must reconnect a delayed value to a forward output.
        lower.rad_reverse.forward_output_by_sig_key = signals
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
        lower.finalize_global_cursor();
        let delay_sample_end = lower
            .delay
            .emit_sample_end_updates(&mut lower.store, lower.uses_iota);
        lower
            .regions
            .current_phases_mut()
            .sample_end
            .extend(delay_sample_end);
        sample_loops.push((false, lower.regions.current_flattened()));
        lower.reset_sample_loop_state(region::RegionKind::ReverseSampleLoop);
    }

    if has_reverse_outputs {
        // Reverse loop slice for public reverse-time outputs.  This path is
        // used when the public bundle contains gradient projections, such as
        // `process = rad(loss, params)`.  Adaptive DSPs may skip this block
        // entirely: their gradient projection can be internal to the forward
        // update and therefore scheduled by the forward slice above.
        lower.cache.clear();
        lower.rad_reverse.lowering_reverse_loop = true;
        for (signal_index, sig) in signals.iter().enumerate() {
            if reverse_time_outputs[signal_index] {
                lower.lower_output_signal(signal_index, *sig, plan.num_outputs)?;
            }
        }
        lower.rad_reverse.lowering_reverse_loop = false;
        if !has_forward_outputs {
            lower.finalize_global_cursor();
            let delay_sample_end = lower
                .delay
                .emit_sample_end_updates(&mut lower.store, lower.uses_iota);
            lower
                .regions
                .current_phases_mut()
                .sample_end
                .extend(delay_sample_end);
        }
        sample_loops.push((true, lower.regions.current_flattened()));
        lower.reset_sample_loop_state(region::RegionKind::SampleLoop);
    }
    for index in 0..plan.num_outputs {
        let mut b = FirBuilder::new(&mut lower.store);
        let chan = b.int32(i32::try_from(index).expect("validated output index fits i32"));
        let ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let load_chan_ptr = b.load_table("outputs", AccessType::FunArgs, chan, ptr_ty.clone());
        lower.sections.control_statements.push(b.declare_var(
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
        use crate::signal_fir::cse;

        cse::materialize_shared_values(
            &mut lower.store,
            &mut lower.sections.constants_statements,
            "fConst",
            lower.name_gen.fconst_counter,
            "iConst",
            lower.name_gen.iconst_counter,
        );

        cse::materialize_shared_values(
            &mut lower.store,
            &mut lower.sections.control_statements,
            "fSlow",
            lower.name_gen.fslow_counter,
            "iSlow",
            lower.name_gen.islow_counter,
        );

        for (_, sample_loop_statements) in &mut sample_loops {
            cse::materialize_shared_values(
                &mut lower.store,
                sample_loop_statements,
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
        lower
            .sections
            .constants_statements
            .insert(0, sample_rate_store);
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.sections.constants_statements)
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
    let ui_statements = lower.ui.ui_statements.clone();
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
        b.block(&lower.sections.reset_statements)
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
        b.block(&lower.sections.clear_statements)
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
        use crate::signal_fir::loop_graph::{LoopGraph, LoopKind, slice_has_persistent_state};

        // Route the per-sample slices through the loop graph (roadmap P6, V4/V5b).
        // One loop node per non-empty slice, classified `Recursive` when the
        // slice writes cross-sample state (a recursion carrier / delay line) and
        // `Vectorizable` otherwise. Emitted in insertion order via
        // `topological_order`. Scalar mode ignores the kind and stays one loop
        // per slice — bit-identical to the previous inline emission (the 190
        // goldens are the guarantee). Vector mode chunks only `Vectorizable`
        // slices (a fully-recursive slice cannot auto-vectorize as one block, so
        // chunking it is pure overhead); per-statement separation of a recursive
        // core from its vectorizable pre/post parts is a later slice.
        let mut graph = LoopGraph::new();
        for (is_reverse, sample_loop_statements) in &sample_loops {
            if sample_loop_statements.is_empty() {
                continue;
            }
            let kind = if slice_has_persistent_state(&lower.store, sample_loop_statements) {
                LoopKind::Recursive
            } else {
                LoopKind::Vectorizable
            };
            let id = graph.add_loop(kind, *is_reverse);
            graph
                .node_mut(id)
                .exec
                .extend(sample_loop_statements.iter().copied());
        }
        let order = graph
            .topological_order()
            .expect("scalar sample loop graph has no dependency edges, so no cycle");

        let mut all = Vec::new();
        all.extend(lower.sections.control_statements.iter().copied());
        for id in order {
            let node = graph.node(id);
            let is_reverse = node.is_reverse;
            let kind = node.kind;
            let pre = node.pre.clone();
            let exec = node.exec.clone();
            let post = node.post.clone();
            all.extend(pre);
            if !exec.is_empty() {
                let sample_loop =
                    emit_sample_loop(&mut lower.store, &exec, is_reverse, kind, compute_mode);
                all.push(sample_loop);
            }
            all.extend(post);
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
        if !lower.used_protos.math_ops.contains(op) {
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
        if !lower.used_protos.int_fun_names.contains(name) {
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
    for proto in lower.used_protos.foreign_fun_protos.values() {
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
    math_prototypes.extend(lower.sections.global_declarations.iter().copied());
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
        b.block(&lower.sections.struct_declarations)
    };
    let globals = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&math_prototypes)
    };
    let static_decls_block = {
        let mut b = FirBuilder::new(&mut lower.store);
        b.block(&lower.sections.static_declarations)
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
