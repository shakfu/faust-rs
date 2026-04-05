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
//!   (`fConst*` variables — [`Variability::Konst`](sigtype::Variability::Konst)).
//! - `instanceResetUserInterface`: UI zone reset values.
//! - `instanceClear`: runtime signal state reset values (delay/rec state).
//! - `compute` preamble (before sample loop): block-rate control expressions
//!   (`fSlow*` variables — [`Variability::Block`](sigtype::Variability::Block)).
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

use std::collections::{BTreeMap, HashMap, HashSet};

use fir::{
    AccessType, BargraphType, ButtonType, FirBinOp, FirBuilder, FirId, FirMatch, FirMathOp,
    FirStore, FirType, NamedType, SliderRange, SliderType, UiBoxType, match_fir,
};
use signals::{BinOp, SigId, SigMatch, dump_sig_readable, match_sig};
use tlib::{
    NodeKind, TreeArena, list_to_vec, match_sym_rec, match_sym_ref, tree_to_int, tree_to_str,
};
use ui::{ControlId, ControlKind, UiGroupKind, UiMatch, UiProgram, match_ui};

use sigtype::{SigType, Variability, check_delay_interval};

use crate::signal_prepare::SimpleSigType;

use super::SignalFirOutput;
use super::error::{SignalFirError, SignalFirErrorCode};
use super::planner::SignalFirPlan;

/// Fixed-size circular delay line resource used by fast-lane `SIGDELAY`.
///
/// Source provenance (C++):
/// - `compiler/transform/signalFIRCompiler.hh` (`DelayLine`, `allocateDelayLineAux`)
/// - `compiler/transform/signalFIRCompiler.cpp` (`compileSigDelay`, `writeReadDelay`)
///
/// Rust adapts the representation to the current FIR builder by storing the
/// delay line as one DSP-struct array field plus an explicit `instanceClear`
/// zeroing loop. The semantic contract stays the same for the active subset:
/// constant integer delay amount, power-of-two size, and masked circular
/// indexing driven by `fIOTA`.
/// Planned storage for one lowered fixed-size circular delay line.
#[derive(Clone, Debug)]
struct DelayLineInfo {
    /// Generated DSP-struct array variable name (e.g. `fVec42`).
    name: String,
    /// Allocated size in elements (always a power of two ≥ 1).
    size: usize,
}

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
/// # Recursion and delay1 coupling
///
/// Every `~` recursion in Faust reaches the fast-lane preparation boundary as
/// `Delay1(Proj(i, group))`. The lowerer detects that pattern in
/// `lower_delay_state` (via `recursion_feedback_info`) and **reuses the
/// group's existing 2-slot recursion array** instead of allocating a separate
/// state variable. Reading `state[(fIOTA - 1) & 1]` from that array is
/// sufficient — no extra write is needed.
///
/// This is why two separate maps exist: `state_name_by_node` (keyed by delay
/// node) and `rec_array_by_group_index` (keyed by group + body index).  They
/// must never alias, even when the body signal of a recursion group happens to
/// be the same `SigId` as a `Delay1` node (the tf22 regression pattern).
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
pub fn build_module(
    plan: &SignalFirPlan,
    module_name: &str,
    arena: &TreeArena,
    signals: &[SigId],
    ui: &UiProgram,
    types: &HashMap<SigId, SimpleSigType>,
    sig_types: &HashMap<SigId, SigType>,
    real_ty: FirType,
) -> Result<SignalFirOutput, SignalFirError> {
    let mut lower = SignalToFirLower::new(arena, ui, types, sig_types, plan.num_inputs, real_ty);
    lower.ensure_sample_rate_var();
    lower.prepare_delay_lines(signals)?;
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

    for (signal_index, sig) in signals.iter().enumerate() {
        let mut value = lower.lower_signal(*sig)?;
        if signal_index < plan.num_outputs {
            // Internal real type → external FaustFloat boundary at output store.
            // Internal values are always Float32/Float64, never FaustFloat, so
            // this cast is always emitted. The check guards against future cases
            // where the value might already carry the external type.
            let needs_output_cast = lower.store.value_type(value) != Some(FirType::FaustFloat);
            let mut b = FirBuilder::new(&mut lower.store);
            if needs_output_cast {
                value = b.cast(FirType::FaustFloat, value);
            }
            let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
            lower.sample_statements.push(b.store_table(
                format!("output{signal_index}"),
                AccessType::Stack,
                i0,
                value,
            ));
        } else {
            let mut b = FirBuilder::new(&mut lower.store);
            lower.sample_statements.push(b.drop_(value));
        }
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
    lower
        .sample_statements
        .extend(lower.compute_updates.iter().copied());
    if lower.uses_iota {
        let bump_iota = lower.bump_iota();
        lower.sample_statements.push(bump_iota);
    }

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
            lower.const_counter,
        );

        let rc = cse::count_fir_value_uses(&lower.store, &lower.control_statements);
        cse::materialize_shared_values(
            &mut lower.store,
            &mut lower.control_statements,
            &rc,
            "fSlow",
            lower.slow_counter,
        );

        let rc = cse::count_fir_value_uses(&lower.store, &lower.sample_statements);
        cse::materialize_shared_values(
            &mut lower.store,
            &mut lower.sample_statements,
            &rc,
            "fTemp",
            0,
        );
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
        if !lower.sample_statements.is_empty() {
            let sample_loop = {
                let mut b = FirBuilder::new(&mut lower.store);
                let upper = b.load_var("count", AccessType::FunArgs, FirType::Int32);
                let body = b.block(&lower.sample_statements);
                b.simple_for_loop("i0", upper, body, false)
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

/// Execution-tier bucket for variability-driven statement placement.
///
/// Maps directly to the C++ Faust compiler's three execution tiers: init-time
/// constants (`instanceConstants`), block-rate control expressions (before
/// the sample loop in `compute`), and sample-rate expressions (inside the loop).
///
/// See [Phase 1 of the FIR runtime optimization plan](../../porting/fir-cse-runtime-optimizations-plan-2026-04-03-en.md#2-phase-1--variability-driven-statement-placement).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bucket {
    /// Init-time constants — written once in `instanceConstants`.
    Constants,
    /// Block-rate controls — written once per `compute()` call, before the loop.
    Control,
}

/// Returns `true` when a FIR value node is *trivial* — meaning it should
/// never be materialized into a named variable because it is already free
/// to duplicate (literals, variable loads, null values).
///
/// This prevents variability placement from hoisting bare constants or
/// variable references into unnecessary temporary variables.
fn is_trivial_fir(store: &FirStore, node: FirId) -> bool {
    matches!(
        match_fir(store, node),
        FirMatch::Int32 { .. }
            | FirMatch::Int64 { .. }
            | FirMatch::Float32 { .. }
            | FirMatch::Float64 { .. }
            | FirMatch::Bool { .. }
            | FirMatch::LoadVar { .. }
            | FirMatch::LoadVarAddress { .. }
            | FirMatch::NullValue { .. }
    )
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
    /// Per-sample loop body: reads, arithmetic, output stores, deferred updates.
    sample_statements: Vec<FirId>,
    /// State-update stores appended after the per-sample body (delay writes, rec shifts).
    compute_updates: Vec<FirId>,
    /// Maps each signal node to its generated state-variable name.
    state_name_by_node: HashMap<SigId, String>,
    /// Maps `(group_id, body_index)` to the recursion array allocated for that
    /// output slot of a recursion group.  **Intentionally separate from
    /// `state_name_by_node`** to prevent aliasing when `lower_delay_state`
    /// detects a `Delay1(Proj(i, group))` feedback and reuses the group's
    /// array slot instead of allocating a fresh variable.  The two maps use
    /// disjoint keys so a delay-node `SigId` and a `(group, index)` pair can
    /// coexist safely even when they refer to the same signal (tf22 pattern).
    rec_array_by_group_index: HashMap<(u32, usize), RecArrayInfo>,
    /// Guards against emitting duplicate state-update stores for shared nodes.
    /// Also used as the dedup key for recursion groups (keyed by `group` SigId)
    /// to ensure that body evaluation and the resulting state writes are
    /// scheduled exactly once per sample regardless of how many `SIGPROJ` nodes
    /// reference the same group.
    scheduled_state_updates: HashSet<SigId>,
    /// Allocated delay lines keyed by carried-signal id.
    delay_lines: HashMap<SigId, DelayLineInfo>,
    /// Guards against emitting duplicate delay-write stores for shared carried signals.
    scheduled_delay_writes: HashSet<SigId>,
    /// `true` once `fIOTA` has been declared; prevents duplicate declarations.
    uses_iota: bool,
    /// Stack of active recursion carrier groups, innermost last; used by `SIGPROJ` resolution.
    ///
    /// Each entry is a group of `RecArrayInfo`s — one per output body in a
    /// multi-output recursion group.  Single-output groups store `vec![info]`.
    recursion_stack: Vec<Vec<RecArrayInfo>>,
    /// Stack of active symbolic recursion variables matching `recursion_stack`.
    recursion_vars: Vec<SigId>,
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
    /// Monotonic counter for `fConst*` init-time constant variable names.
    ///
    /// Incremented each time [`materialize_in_bucket`] hoists a
    /// [`Variability::Konst`] expression into [`Self::constants_statements`].
    const_counter: u32,
    /// Monotonic counter for `fSlow*` block-rate variable names.
    ///
    /// Incremented each time [`materialize_in_bucket`] hoists a
    /// [`Variability::Block`] expression into [`Self::control_statements`].
    slow_counter: u32,
}

/// Two-slot carrier for one output of a recursive group (`SIGPROJ(i, SYMREC(…))`).
///
/// Each output body in a multi-output recursion group gets its own array.
/// Slot `[1]` holds the previous-sample value; slot `[0]` holds the
/// current-sample value.  After outputs are stored, the lowering emits
/// `state[1] = state[0]` to shift the window forward.
///
/// Source provenance (C++): `signalFIRCompiler.cpp` (`generateRecProj`,
/// `generateRec`), emitted as `fRecN[2]` / `iRecN[2]`.
#[derive(Clone, Debug)]
struct RecArrayInfo {
    /// Generated DSP-struct array variable name (e.g. `fRec7`).
    name: String,
    /// Element type (`Int32` for integer recursion, `Float32`/`Float64` otherwise).
    typ: FirType,
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
    fn new(
        arena: &'a TreeArena,
        ui_program: &'a UiProgram,
        types: &'a HashMap<SigId, SimpleSigType>,
        sig_types: &'a HashMap<SigId, SigType>,
        num_inputs: usize,
        real_ty: FirType,
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
            sample_statements: Vec::new(),
            compute_updates: Vec::new(),
            state_name_by_node: HashMap::new(),
            rec_array_by_group_index: HashMap::new(),
            scheduled_state_updates: HashSet::new(),
            delay_lines: HashMap::new(),
            scheduled_delay_writes: HashSet::new(),
            uses_iota: false,
            recursion_stack: Vec::new(),
            recursion_vars: Vec::new(),
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
            const_counter: 0,
            slow_counter: 0,
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
    /// Multiple `SIGDELAY(x, n)` nodes sharing the same carried signal `x`
    /// reuse one delay line sized to the largest delay seen.  This pre-pass
    /// ensures all writes are registered before any reads are emitted.
    fn prepare_delay_lines(&mut self, outputs: &[SigId]) -> Result<(), SignalFirError> {
        // Two-pass approach: first collect the maximum delay per carried signal,
        // then allocate once with the correct (max) size.  This avoids the
        // sizing-mismatch error when the same carried signal appears in multiple
        // SIGDELAY nodes with different delay amounts.
        let mut max_delays: HashMap<SigId, i32> = HashMap::new();
        let mut seen = HashSet::new();
        for output in outputs {
            self.scan_delay_lines(*output, &mut seen, &mut max_delays)?;
        }
        for (carried, delay) in max_delays {
            self.ensure_delay_line_decl(carried, delay)?;
        }
        Ok(())
    }

    /// Visits one signal node, recording the maximum delay per carried signal.
    ///
    /// Skips already-visited nodes (DAG sharing) via `seen`.  The maximum is
    /// tracked because several `SIGDELAY` nodes can share the same carried
    /// signal `x` with different delay amounts; a single delay line is
    /// allocated at the largest observed size so every read offset remains
    /// in-bounds.
    fn scan_delay_lines(
        &mut self,
        sig: SigId,
        seen: &mut HashSet<SigId>,
        max_delays: &mut HashMap<SigId, i32>,
    ) -> Result<(), SignalFirError> {
        if !seen.insert(sig) {
            return Ok(());
        }
        if let SigMatch::Delay(value, amount) = match_sig(self.arena, sig) {
            match self.delay_size_for_amount(amount)? {
                Some(0) => {}
                Some(delay) => {
                    let entry = max_delays.entry(value).or_insert(0);
                    if delay > *entry {
                        *entry = delay;
                    }
                }
                None => {
                    return self.unsupported_node(
                        sig,
                        "SIGDELAY requires a constant integer amount or a signal with a bounded non-negative interval",
                    );
                }
            }
        }
        let node = self.arena.node(sig).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("missing prepared signal node {}", sig.as_u32()),
            )
        })?;
        for child in node.children.as_slice() {
            self.scan_delay_child(*child, seen, max_delays)?;
        }
        Ok(())
    }

    /// Recurses into one child node, transparently unwrapping list spines.
    fn scan_delay_child(
        &mut self,
        child: SigId,
        seen: &mut HashSet<SigId>,
        max_delays: &mut HashMap<SigId, i32>,
    ) -> Result<(), SignalFirError> {
        if self.arena.is_list(child) {
            let mut list = child;
            while !self.arena.is_nil(list) {
                let head = self.arena.hd(list).ok_or_else(|| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "malformed prepared signal list while scanning delay lines",
                    )
                })?;
                self.scan_delay_lines(head, seen, max_delays)?;
                list = self.arena.tl(list).ok_or_else(|| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "malformed prepared signal list while scanning delay lines",
                    )
                })?;
            }
            Ok(())
        } else {
            self.scan_delay_lines(child, seen, max_delays)
        }
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
    /// - [`Variability::Samp`]  → `sample_statements`  (every sample)
    fn variability_of(&self, sig: SigId) -> Option<Variability> {
        self.sig_types.get(&sig).map(|t| t.variability())
    }

    /// Returns `true` when the signal is a direct `Proj(i, SYMREC)` read.
    ///
    /// The type system (after the `update_rec_types` variability-join fix)
    /// guarantees that such nodes always carry at least `Samp` variability, so
    /// they would not be hoisted by the placement logic anyway.  This guard is
    /// kept as a defensive check against future regressions.
    fn is_recursive_projection(&self, sig: SigId) -> bool {
        if let SigMatch::Proj(_, group) = match_sig(self.arena, sig) {
            match_sym_rec(self.arena, group).is_some()
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
    /// | `Constants` | `fConst` | [`AccessType::Struct`] | `instanceConstants` |
    /// | `Control` | `fSlow` | [`AccessType::Stack`] | `compute` preamble |
    ///
    /// `fConst` variables use struct storage because they are written in
    /// `instanceConstants()` and read in `compute()`.  `fSlow` variables
    /// use stack storage because they live within the `compute()` body.
    fn materialize_in_bucket(&mut self, value: FirId, bucket: Bucket) -> FirId {
        let (name, access) = match bucket {
            Bucket::Constants => {
                let n = self.const_counter;
                self.const_counter += 1;
                (format!("fConst{n}"), AccessType::Struct)
            }
            Bucket::Control => {
                let n = self.slow_counter;
                self.slow_counter += 1;
                (format!("fSlow{n}"), AccessType::Stack)
            }
        };

        let typ = self
            .store
            .value_type(value)
            .unwrap_or_else(|| self.real_ty());

        match bucket {
            Bucket::Constants => {
                // Struct-backed: declare the field, then assign in instanceConstants.
                self.ensure_named_struct_var(&name, typ.clone(), None);
                let mut b = FirBuilder::new(&mut self.store);
                let store_stmt = b.store_var(&name, access, value);
                self.constants_statements.push(store_stmt);
            }
            Bucket::Control => {
                // Stack-backed: use DeclareVar(Stack, init) so that backends
                // (WASM, Cranelift) discover the local during their pre-scan.
                let mut b = FirBuilder::new(&mut self.store);
                let decl = b.declare_var(&name, typ.clone(), access, Some(value));
                self.control_statements.push(decl);
            }
        }

        // Return a load reference that callers embed in downstream expressions.
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

    /// Central dispatcher: lowers one signal node to a FIR value expression.
    ///
    /// Results are memoized in [`Self::cache`] for DAG sharing.  As a side
    /// effect, successful lowering may append declarations and assignments to
    /// lifecycle section accumulators (e.g. delay writes to
    /// [`Self::compute_updates`], state declarations to
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
                let init = self.zero_value_for_signal(sig)?;
                self.lower_delay_state(sig, value, init)?
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
        let lowered = if !is_trivial_fir(&self.store, lowered)
            && !self.is_recursive_projection(sig)
            && !matches!(match_sig(self.arena, sig), SigMatch::WrTbl(..))
        {
            match self.variability_of(sig) {
                Some(Variability::Konst) => self.materialize_in_bucket(lowered, Bucket::Constants),
                Some(Variability::Block) => self.materialize_in_bucket(lowered, Bucket::Control),
                _ => lowered,
            }
        } else {
            lowered
        };

        self.cache.insert(sig, lowered);
        Ok(lowered)
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
            let mut b = FirBuilder::new(&mut self.store);
            return Ok(b.load_var("fSampleRate", AccessType::Struct, FirType::Int32));
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
        match self.delay_size_for_amount(amount)? {
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

    /// Emits the circular-buffer read for a delay whose line was pre-allocated
    /// by [`Self::prepare_delay_lines`].
    ///
    /// Schedules a write of the current sample into the ring buffer (once per
    /// carried signal) and returns a masked-index load at `fIOTA - amount`.
    fn lower_fixed_delay(
        &mut self,
        node: SigId,
        value: SigId,
        amount: SigId,
        delay: i32,
    ) -> Result<FirId, SignalFirError> {
        let line = self.ensure_delay_line_decl(value, delay)?;
        let current = self.lower_signal(value)?;
        if self.scheduled_delay_writes.insert(value) {
            let write_index = {
                let raw = self.current_iota_index();
                self.masked_delay_index(raw, line.size)
            };
            let mut b = FirBuilder::new(&mut self.store);
            self.sample_statements.push(b.store_table(
                line.name.clone(),
                AccessType::Struct,
                write_index,
                current,
            ));
        }
        let amount_value = self.lower_signal(amount)?;
        let read_index = self.delayed_iota_index(amount_value, line.size);
        let read_ty = self.signal_fir_type(node)?;
        let mut b = FirBuilder::new(&mut self.store);
        Ok(b.load_table(line.name.clone(), AccessType::Struct, read_index, read_ty))
    }

    /// Lowers one single-sample state edge (`delay1`/`prefix`) using a
    /// 2-element circular buffer indexed by `fIOTA`, matching the C++
    /// `signalFIRCompiler::writeReadDelay` pattern.
    ///
    /// **Recursion feedback optimization**: if the carried `value` is
    /// `Proj(i, SYMREC/SYMREF)` pointing into the currently active recursion
    /// context (detected by `recursion_feedback_info`), the group's existing
    /// recursion array is reused directly — no separate state variable is
    /// allocated and no extra write is emitted.  The previous-sample value is
    /// read as `rec_array[(fIOTA - 1) & 1]`, which is always valid because the
    /// recursion body writes `rec_array[fIOTA & 1]` earlier in the same sample.
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
        if let Some(rec_info) = self.recursion_feedback_info(value)? {
            let out_ty = self.signal_fir_type(node)?;
            debug_assert_eq!(
                rec_info.typ, out_ty,
                "prepared recursion feedback type should match delay1 output type"
            );
            // Read previous value from recursion array: state[(fIOTA - 1) & 1]
            self.ensure_iota_state();
            let one = self.lower_int32_const(1);
            let prev_index = self.delayed_iota_index(one, 2);
            let mut b = FirBuilder::new(&mut self.store);
            return Ok(b.load_table(
                rec_info.name,
                AccessType::Struct,
                prev_index,
                rec_info.typ.clone(),
            ));
        }
        self.ensure_iota_state();
        let state_ty = self.signal_fir_type(value)?;
        let name = self.ensure_state_slot(node, state_ty.clone(), init);
        // Read previous value: state[(fIOTA - 1) & 1]
        let one = self.lower_int32_const(1);
        let read_index = self.delayed_iota_index(one, 2);
        let out = {
            let mut b = FirBuilder::new(&mut self.store);
            b.load_table(name.clone(), AccessType::Struct, read_index, state_ty)
        };
        // Write current value: state[fIOTA & 1] = next (immediate)
        if self.scheduled_state_updates.insert(node) {
            let next = self.lower_signal(value)?;
            let write_index = {
                let iota = self.current_iota_index();
                self.masked_delay_index(iota, 2)
            };
            let mut b = FirBuilder::new(&mut self.store);
            self.sample_statements
                .push(b.store_table(name, AccessType::Struct, write_index, next));
        }
        Ok(out)
    }

    /// Returns the active recursion carrier if `value` is `SIGPROJ(i, group)`
    /// pointing into the current recursion context, otherwise `None`.
    ///
    /// Used by `lower_delay_state` to detect the canonical feedback pattern
    /// and reuse the existing recursion array slot instead of creating a
    /// separate state variable.
    fn recursion_feedback_info(
        &mut self,
        value: SigId,
    ) -> Result<Option<RecArrayInfo>, SignalFirError> {
        let SigMatch::Proj(index, group) = match_sig(self.arena, value) else {
            return Ok(None);
        };
        self.active_recursion_info(group, index as usize)
    }

    /// Resolves a symbolic recursion group reference to its active carrier
    /// at a given projection index.
    ///
    /// Walks [`Self::recursion_stack`] from innermost outward; returns `None`
    /// if `group` is not a `SYMREF` bound in the current lowering context.
    fn active_recursion_info(
        &self,
        group: SigId,
        proj_index: usize,
    ) -> Result<Option<RecArrayInfo>, SignalFirError> {
        let Some(var) = match_sym_ref(self.arena, group) else {
            return Ok(None);
        };
        let depth = self
            .recursion_vars
            .iter()
            .rposition(|bound| *bound == var)
            .map(|slot| self.recursion_vars.len() - slot)
            .ok_or_else(|| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!("unbound symbolic recursion variable {}", var.as_u32()),
                )
            })?;
        let group_arrays = &self.recursion_stack[self.recursion_stack.len() - depth];
        let canonical_index = if group_arrays.len() == 1 {
            0
        } else {
            proj_index
        };
        group_arrays.get(canonical_index).cloned().ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "projection index {proj_index} out of bounds for recursion group with {} outputs",
                    group_arrays.len()
                ),
            )
        }).map(Some)
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
        self.register_clear_recursion_array(name.clone(), init);
        self.state_name_by_node.insert(node, name.clone());
        name
    }

    /// Declares the struct array for one circular delay line, idempotent.
    ///
    /// On first call for `carried`, allocates `next_power_of_two(delay + 1)`
    /// elements, emits the struct declaration, and registers an `instanceClear`
    /// zeroing loop.  Subsequent calls for the same `carried` return the cached
    /// info; an error is returned if the cached size is smaller than required.
    fn ensure_delay_line_decl(
        &mut self,
        carried: SigId,
        delay: i32,
    ) -> Result<DelayLineInfo, SignalFirError> {
        if delay < 0 {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("SIGDELAY amount must be >= 0, got {delay}"),
            ));
        }
        let elem_type = self.signal_fir_type(carried)?;
        let required_size = self.pow2limit_for_delay(delay)?;
        if let Some(existing) = self.delay_lines.get(&carried) {
            if existing.size < required_size {
                return Err(SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!(
                        "internal fast-lane delay-line sizing mismatch for signal {}: existing size {} < required {}",
                        carried.as_u32(),
                        existing.size,
                        required_size
                    ),
                ));
            }
            return Ok(existing.clone());
        }

        self.ensure_iota_state();
        let prefix = if elem_type == FirType::Int32 {
            "iVec"
        } else {
            "fVec"
        };
        let name = format!("{prefix}{}", carried.as_u32());
        let array_ty = FirType::Array(Box::new(elem_type.clone()), required_size);
        let mut b = FirBuilder::new(&mut self.store);
        let decl = b.declare_var(name.clone(), array_ty, AccessType::Struct, None);
        self.struct_declarations.push(decl);
        self.register_clear_table(name.clone(), elem_type.clone(), required_size, carried)?;
        let info = DelayLineInfo {
            name,
            size: required_size,
        };
        self.delay_lines.insert(carried, info.clone());
        Ok(info)
    }

    /// Declares the `fIOTA` circular-buffer position counter, idempotent.
    fn ensure_iota_state(&mut self) {
        if self.uses_iota {
            return;
        }
        self.uses_iota = true;
        let zero = self.lower_int32_const(0);
        let mut b = FirBuilder::new(&mut self.store);
        let decl = b.declare_var("fIOTA", FirType::Int32, AccessType::Struct, None);
        self.struct_declarations.push(decl);
        self.register_clear_init("fIOTA".to_owned(), zero);
    }

    /// Emits a struct load of `fIOTA` (current write position in delay lines).
    fn current_iota_index(&mut self) -> FirId {
        let mut b = FirBuilder::new(&mut self.store);
        b.load_var("fIOTA", AccessType::Struct, FirType::Int32)
    }

    /// Computes the masked read index `(fIOTA - amount) & (size - 1)`.
    fn delayed_iota_index(&mut self, amount: FirId, size: usize) -> FirId {
        let iota = self.current_iota_index();
        let raw = {
            let mut b = FirBuilder::new(&mut self.store);
            b.binop(FirBinOp::Sub, iota, amount, FirType::Int32)
        };
        self.masked_delay_index(raw, size)
    }

    /// Applies the power-of-two ring-buffer mask: `index & (size - 1)`.
    fn masked_delay_index(&mut self, index: FirId, size: usize) -> FirId {
        let mask = {
            let mut b = FirBuilder::new(&mut self.store);
            b.int32(i32::try_from(size.saturating_sub(1)).unwrap_or(i32::MAX))
        };
        let mut b = FirBuilder::new(&mut self.store);
        b.binop(FirBinOp::And, index, mask, FirType::Int32)
    }

    /// Emits `fIOTA = fIOTA + 1` to advance the delay-line write pointer.
    fn bump_iota(&mut self) -> FirId {
        let next = {
            let iota = self.current_iota_index();
            let one = self.lower_int32_const(1);
            let mut b = FirBuilder::new(&mut self.store);
            b.binop(FirBinOp::Add, iota, one, FirType::Int32)
        };
        let mut b = FirBuilder::new(&mut self.store);
        b.store_var("fIOTA", AccessType::Struct, next)
    }

    /// Emits an `instanceClear` zeroing loop for a delay-line or table array.
    ///
    /// Idempotent: subsequent calls for the same `name` are silently ignored.
    fn register_clear_table(
        &mut self,
        name: String,
        elem_type: FirType,
        size: usize,
        sig: SigId,
    ) -> Result<(), SignalFirError> {
        if !self.clear_init_seen.insert(name.clone()) {
            return Ok(());
        }
        let loop_var = self.fresh_loop_var("lDelay");
        let upper = {
            let mut b = FirBuilder::new(&mut self.store);
            b.int32(i32::try_from(size).map_err(|_| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!("delay line size conversion overflow: {size}"),
                )
            })?)
        };
        let zero = match self.simple_type(sig)? {
            SimpleSigType::Int => self.lower_int32_const(0),
            SimpleSigType::Real => self.float_const(0.0),
            SimpleSigType::Sound => {
                return Err(SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!(
                        "signal {} cannot use a soundfile handle as delay-line element type",
                        sig.as_u32()
                    ),
                ));
            }
        };
        let body = {
            let index = {
                let mut b = FirBuilder::new(&mut self.store);
                b.load_var(loop_var.clone(), AccessType::Loop, FirType::Int32)
            };
            let store = {
                let mut b = FirBuilder::new(&mut self.store);
                b.store_table(name, AccessType::Struct, index, zero)
            };
            let mut b = FirBuilder::new(&mut self.store);
            b.block(&[store])
        };
        let mut b = FirBuilder::new(&mut self.store);
        self.clear_statements
            .push(b.simple_for_loop(loop_var, upper, body, false));
        let _ = elem_type;
        Ok(())
    }

    /// Emits an `instanceClear` zeroing loop for a two-slot recursion array.
    ///
    /// Idempotent: subsequent calls for the same `name` are silently ignored.
    fn register_clear_recursion_array(&mut self, name: String, init: FirId) {
        if !self.clear_init_seen.insert(name.clone()) {
            return;
        }
        let loop_var = self.fresh_loop_var("lRec");
        let upper = {
            let mut b = FirBuilder::new(&mut self.store);
            b.int32(2)
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

    /// Returns the constant integer value of `sig` if it is `SIGINT`, otherwise `None`.
    fn constant_delay_amount(&self, sig: SigId) -> Result<Option<i32>, SignalFirError> {
        match match_sig(self.arena, sig) {
            SigMatch::Int(value) => Ok(Some(value)),
            _ => Ok(None),
        }
    }

    /// Returns the interval upper-bound used to size the delay line for a
    /// variable delay amount, mirroring C++ `checkDelayInterval`.
    ///
    /// Accepts any signal whose interval is non-empty, bounded (finite `hi`),
    /// and has `hi >= 0`.  `hi == 0` is the zero-delay passthrough case.
    /// Returns `None` for signals with no type entry, unbounded or empty
    /// intervals, or strictly-negative `hi`.
    fn variable_delay_max_bound(&self, sig: SigId) -> Option<i32> {
        let ty = self.sig_types.get(&sig)?;
        if ty.interval().hi() < 0.0 {
            return None;
        }
        check_delay_interval(ty).ok()
    }

    /// Returns a structural upper bound for a delay expression when interval
    /// analysis cannot determine a finite bound.
    ///
    /// If `sig` is `SIGMIN(SigInt(n), _)` or `SIGMIN(_, SigInt(n))` with
    /// `n >= 0`, returns `n` as a conservative upper bound.  This covers the
    /// standard `de.delay(n, d, x) = x @ min(n, max(0, d))` pattern, where
    /// the first argument to `min` is an explicit compile-time ceiling.
    ///
    /// Note: with correct `FConst` typing (`Interval::new_default()` rather
    /// than `empty()`), `fSamplingFreq`-based expressions like `ma.SR`
    /// produce a finite bounded interval through standard interval algebra
    /// and do not reach this fallback.  This method acts as defence-in-depth
    /// for any remaining case where interval analysis yields an empty or
    /// unbounded result.
    fn min_const_upper_bound(&self, sig: SigId) -> Option<i32> {
        let SigMatch::Min(lhs, rhs) = match_sig(self.arena, sig) else {
            return None;
        };
        let as_nonneg_int = |id: SigId| -> Option<i32> {
            if let SigMatch::Int(n) = match_sig(self.arena, id)
                && n >= 0
            {
                return Some(n);
            }
            None
        };
        as_nonneg_int(lhs).or_else(|| as_nonneg_int(rhs))
    }

    /// Resolve the delay line allocation size for `amount`:
    ///
    /// 1. Literal `Int` → exact constant.
    /// 2. Bounded interval → interval upper bound.
    /// 3. `SIGMIN(SigInt(n), _)` or `SIGMIN(_, SigInt(n))` → `n` (structural
    ///    fallback for cases where interval analysis yields empty, such as
    ///    expressions involving `fSamplingFreq`).
    /// 4. Otherwise → `None` (caller emits an error).
    fn delay_size_for_amount(&self, amount: SigId) -> Result<Option<i32>, SignalFirError> {
        if let Some(c) = self.constant_delay_amount(amount)? {
            return Ok(Some(c));
        }
        if let Some(b) = self.variable_delay_max_bound(amount) {
            return Ok(Some(b));
        }
        Ok(self.min_const_upper_bound(amount))
    }

    /// Computes `next_power_of_two(delay + 1)` — the circular buffer size for
    /// a given maximum delay in samples.  Errors on negative or overflowing input.
    fn pow2limit_for_delay(&self, delay: i32) -> Result<usize, SignalFirError> {
        let delay = usize::try_from(delay).map_err(|_| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("SIGDELAY amount must be >= 0, got {delay}"),
            )
        })?;
        let requested = delay.checked_add(1).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "SIGDELAY amount overflow while sizing delay line",
            )
        })?;
        requested.checked_next_power_of_two().ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("SIGDELAY amount too large to size delay line: {delay}"),
            )
        })
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
        self.sample_statements
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
            self.compute_updates.push(update);
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
        self.sample_statements
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
    /// writes are emitted by `lower_wrtbl` into `sample_statements`.
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
    /// stores into `sample_statements`), then pops the stack.  Subsequent
    /// `SIGPROJ` nodes for the same group skip body evaluation entirely (the
    /// `scheduled_state_updates` dedup guard keyed by `group` SigId ensures
    /// exactly one body-lowering pass per sample).
    ///
    /// **Fast path** (active reference inside a body being lowered): when
    /// `active_recursion_info` finds the group on the stack, the current-slot
    /// value is read directly — no recursion into `lower_signal` occurs, which
    /// breaks the cycle.
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
        if let Some(info) = self.active_recursion_info(group, index_usize)? {
            let real_ty = self.signal_fir_type(node)?;
            self.ensure_iota_state();
            let current_index = {
                let iota = self.current_iota_index();
                self.masked_delay_index(iota, 2)
            };
            let mut b = FirBuilder::new(&mut self.store);
            return Ok(b.load_table(info.name, AccessType::Struct, current_index, real_ty));
        }

        // ── Decode all body signals from the group ──
        let (var, bodies) = self.decode_symbolic_group_bodies(group).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "SIGPROJ group must be SYMREC/SYMREF after de_bruijn_to_sym in Step 2C.2 (expr={})",
                    dump_sig_readable(self.arena, node)
                ),
            )
        })?;

        let canonical_index = if bodies.len() == 1 { 0 } else { index_usize };

        if canonical_index >= bodies.len() {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "SIGPROJ index {index} out of bounds for recursion group with {} bodies",
                    bodies.len()
                ),
            ));
        }

        // ── Allocate recursion arrays for ALL bodies ──
        //
        // Each output slot gets its own array keyed by `(group, index)` in
        // `rec_array_by_group_index`.  This is intentionally separate from the
        // `state_name_by_node` map used by `ensure_state_slot`, so that a
        // `lower_delay_state` call inside the body expression never aliases the
        // group's output carrier — even when the body signal node is the same
        // Faust node as a delay1 input (the root cause of the tf22 bug).
        let mut group_arrays = Vec::with_capacity(bodies.len());
        for (i, body) in bodies.iter().enumerate() {
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
            let info = self.ensure_recursion_array_for_group(group, i, state_ty, init)?;
            group_arrays.push(info);
        }

        // ── Push group context, lower ALL bodies, emit stores ──
        // Use `group` as the dedup key: if we've already scheduled this group,
        // skip the body-lowering pass (another proj of the same group triggered it).
        if self.scheduled_state_updates.insert(group) {
            self.recursion_vars.push(var);
            self.recursion_stack.push(group_arrays.clone());

            for (i, body) in bodies.iter().enumerate() {
                let rhs = self.lower_signal(*body)?;
                // Always emit the store for the group's output carrier array.
                //
                // With `rec_array_by_group_index` separate from `state_name_by_node`,
                // any `lower_delay_state` call inside the body uses a *different* array
                // (keyed by the delay node), so there is no aliasing and no risk of a
                // double-write to the same slot.
                let info = &group_arrays[i];
                // Write body value to circular buffer: state[fIOTA & 1] = rhs
                self.ensure_iota_state();
                let write_index = {
                    let iota = self.current_iota_index();
                    self.masked_delay_index(iota, 2)
                };
                let current_store = {
                    let mut b = FirBuilder::new(&mut self.store);
                    b.store_table(info.name.clone(), AccessType::Struct, write_index, rhs)
                };
                self.sample_statements.push(current_store);
                // No deferred shift needed — fIOTA rotation handles it.
            }

            self.recursion_stack.pop();
            self.recursion_vars.pop();
        }

        // ── Return the result for the requested index ──
        let info = &group_arrays[canonical_index];
        let out_ty = self.signal_fir_type(node)?;
        self.ensure_iota_state();
        let current_index = {
            let iota = self.current_iota_index();
            self.masked_delay_index(iota, 2)
        };
        let out = {
            let mut b = FirBuilder::new(&mut self.store);
            let load = b.load_table(
                info.name.clone(),
                AccessType::Struct,
                current_index,
                info.typ.clone(),
            );
            // SIGPROJ inherits the type of its body: the array type and the
            // node type must always agree after signal_prepare/typeAnnotation.
            debug_assert_eq!(
                info.typ, out_ty,
                "SIGPROJ type mismatch: array={:?}, node={:?}",
                info.typ, out_ty
            );
            load
        };
        Ok(out)
    }

    /// Declares a two-slot `[typ; 2]` recursion array for output slot `index` of
    /// recursion `group`, idempotent.
    ///
    /// Uses a `(group_id, index)` key — separate from the `state_name_by_node`
    /// map used by `ensure_state_slot` — so that delay-state slots created
    /// inside a body expression never alias the group's output carrier array.
    fn ensure_recursion_array_for_group(
        &mut self,
        group: SigId,
        index: usize,
        typ: FirType,
        init: FirId,
    ) -> Result<RecArrayInfo, SignalFirError> {
        let key = (group.as_u32(), index);
        if let Some(info) = self.rec_array_by_group_index.get(&key) {
            return Ok(info.clone());
        }
        let prefix = if typ == FirType::Int32 {
            "iRec"
        } else {
            "fRec"
        };
        // Use group id (+ index suffix for multi-output groups) to form a unique name.
        let name = if index == 0 {
            format!("{prefix}{}", group.as_u32())
        } else {
            format!("{prefix}{}_{}", group.as_u32(), index)
        };
        let array_ty = FirType::Array(Box::new(typ.clone()), 2);
        let mut b = FirBuilder::new(&mut self.store);
        let decl = b.declare_var(name.clone(), array_ty, AccessType::Struct, None);
        self.struct_declarations.push(decl);
        self.register_clear_recursion_array(name.clone(), init);
        let info = RecArrayInfo { name, typ };
        self.rec_array_by_group_index.insert(key, info.clone());
        Ok(info)
    }

    /// Decodes a `SYMREC(var, body_list)` group to all its payload body signals.
    ///
    /// `de_bruijn_to_sym` preserves the list-shaped recursive payload used by
    /// propagated signal groups.  Returns the variable node and a `Vec` of
    /// body signals extracted via `list_to_vec`.
    fn decode_symbolic_group_bodies(&self, group: SigId) -> Option<(SigId, Vec<SigId>)> {
        let (var, body_list) = match_sym_rec(self.arena, group)?;
        let bodies = list_to_vec(self.arena, body_list)?;
        Some((var, bodies))
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

// ---------------------------------------------------------------------------
// Compile-time signal interpreter for computed table generators (SIGGEN).
//
// This is the compile-time equivalent of C++'s `signal2Container()` approach:
// since SIGGEN generators are always 0-input deterministic DSP, we can
// evaluate them directly rather than generating runtime init code.
// ---------------------------------------------------------------------------

/// Interprets a generator signal for `size` steps, returning f64 values.
fn interpret_generator(
    arena: &TreeArena,
    sig: SigId,
    size: usize,
) -> Result<Vec<f64>, SignalFirError> {
    let prepared =
        crate::signal_prepare::prepare_signals_for_fir(arena, &[sig], &UiProgram::empty())
            .map_err(|err| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!("SIGGEN interpreter preparation failed: {err}"),
                )
            })?;
    let prepared_sig = prepared.outputs.first().copied().ok_or_else(|| {
        SignalFirError::new(
            SignalFirErrorCode::UnsupportedSignalNode,
            "SIGGEN interpreter received empty prepared output list",
        )
    })?;
    let mut interp = GeneratorInterpreter::new(&prepared.arena, &prepared.types);
    let mut results = Vec::with_capacity(size);
    for _ in 0..size {
        let val = interp.eval(prepared_sig)?;
        results.push(val);
        interp.advance();
    }
    Ok(results)
}

#[cfg(test)]
pub(super) fn interpret_generator_for_test(
    arena: &TreeArena,
    sig: SigId,
    size: usize,
) -> Result<Vec<f64>, SignalFirError> {
    interpret_generator(arena, sig, size)
}

struct GeneratorInterpreter<'a> {
    arena: &'a TreeArena,
    types: &'a HashMap<SigId, SimpleSigType>,
    /// Recursion group state: maps SYMREC var → (current_values, prev_values).
    rec_state: HashMap<SigId, (Vec<f64>, Vec<f64>)>,
    /// Groups currently being evaluated this step (prevent infinite recursion).
    evaluating: HashSet<SigId>,
    /// Current step number (0-based), used for delay1/prefix of non-recursive signals.
    step: usize,
    /// Previous-step values for non-recursive `Delay1(x)` expressions keyed by
    /// the delayed signal node `x`.
    delay1_prev: HashMap<SigId, f64>,
    /// Current-step values that will become `delay1_prev` on the next advance.
    delay1_current: HashMap<SigId, f64>,
    /// Per-signal history buffer for multi-sample Delay(sig, amount).
    /// Maps signal SigId → ring buffer of past values (index 0 = most recent).
    delay_history: HashMap<SigId, Vec<f64>>,
}

impl<'a> GeneratorInterpreter<'a> {
    fn new(arena: &'a TreeArena, types: &'a HashMap<SigId, SimpleSigType>) -> Self {
        Self {
            arena,
            types,
            rec_state: HashMap::new(),
            evaluating: HashSet::new(),
            step: 0,
            delay1_prev: HashMap::new(),
            delay1_current: HashMap::new(),
            delay_history: HashMap::new(),
        }
    }

    /// Advance one time step: current → prev for all recursion groups.
    fn advance(&mut self) {
        for (cur, prev) in self.rec_state.values_mut() {
            prev.clone_from(cur);
        }
        self.evaluating.clear();
        self.delay1_prev.clone_from(&self.delay1_current);
        self.delay1_current.clear();
        self.step += 1;
    }

    fn simple_type(&self, sig: SigId) -> Option<SimpleSigType> {
        self.types.get(&sig).copied()
    }

    /// Evaluate one signal node, returning its `f64` value for the current step.
    ///
    /// Dispatches over the full signal vocabulary: constants, arithmetic, casts,
    /// unary/binary math, trig, delays, waveform/table reads, and recursion.
    /// Recursion groups maintain `cur` and `prev` state arrays advanced by
    /// `eval_rec_and_project` on each call to `advance`.
    fn eval(&mut self, sig: SigId) -> Result<f64, SignalFirError> {
        // Check for SYMREC(var, body) — symbolic recursion binder
        if let Some((var, body)) = match_sym_rec(self.arena, sig) {
            return self.eval_rec_and_project(var, Some(body), 0);
        }
        // Check for SYMREF(var) — symbolic recursion reference
        if let Some(var) = match_sym_ref(self.arena, sig) {
            return self.read_rec_current(var, 0);
        }

        match match_sig(self.arena, sig) {
            // --- Constants ---
            SigMatch::Int(v) => Ok(v as f64),
            SigMatch::Real(v) => Ok(v),

            // --- Arithmetic / logic ---
            SigMatch::BinOp(op, x, y) => {
                let lhs = self.eval(x)?;
                let rhs = self.eval(y)?;
                Ok(self.eval_binop(sig, op, lhs, rhs))
            }
            SigMatch::Pow(x, y) => Ok(self.eval(x)?.powf(self.eval(y)?)),
            SigMatch::Min(x, y) => Ok(self.eval(x)?.min(self.eval(y)?)),
            SigMatch::Max(x, y) => Ok(self.eval(x)?.max(self.eval(y)?)),

            // --- Casts ---
            SigMatch::FloatCast(x) => self.eval(x),
            SigMatch::IntCast(x) => Ok((self.eval(x)? as i32) as f64),
            SigMatch::BitCast(x) => {
                let v = self.eval(x)?;
                // Reinterpret f32 bits as i32 (C++ bitcast semantics)
                let bits = (v as f32).to_bits();
                Ok((bits as i32) as f64)
            }

            // --- Unary math ---
            SigMatch::Sin(x) => Ok(self.eval(x)?.sin()),
            SigMatch::Cos(x) => Ok(self.eval(x)?.cos()),
            SigMatch::Tan(x) => Ok(self.eval(x)?.tan()),
            SigMatch::Asin(x) => Ok(self.eval(x)?.asin()),
            SigMatch::Acos(x) => Ok(self.eval(x)?.acos()),
            SigMatch::Atan(x) => Ok(self.eval(x)?.atan()),
            SigMatch::Exp(x) => Ok(self.eval(x)?.exp()),
            SigMatch::Log(x) => Ok(self.eval(x)?.ln()),
            SigMatch::Log10(x) => Ok(self.eval(x)?.log10()),
            SigMatch::Sqrt(x) => Ok(self.eval(x)?.sqrt()),
            SigMatch::Abs(x) => Ok(self.eval(x)?.abs()),
            SigMatch::Floor(x) => Ok(self.eval(x)?.floor()),
            SigMatch::Ceil(x) => Ok(self.eval(x)?.ceil()),
            SigMatch::Rint(x) => Ok(self.eval(x)?.round_ties_even()),
            SigMatch::Round(x) => Ok(self.eval(x)?.round()),

            // --- Binary math ---
            SigMatch::Atan2(x, y) => Ok(self.eval(x)?.atan2(self.eval(y)?)),
            SigMatch::Fmod(x, y) => {
                let lhs = self.eval(x)?;
                let rhs = self.eval(y)?;
                Ok(if rhs == 0.0 { 0.0 } else { lhs % rhs })
            }
            SigMatch::Remainder(x, y) => {
                let lhs = self.eval(x)?;
                let rhs = self.eval(y)?;
                Ok(if rhs == 0.0 {
                    0.0
                } else {
                    lhs - (lhs / rhs).round() * rhs
                })
            }

            // --- Selection ---
            SigMatch::Select2(sel, s1, s2) => {
                let cond = self.eval(sel)?;
                if cond != 0.0 {
                    self.eval(s2)
                } else {
                    self.eval(s1)
                }
            }

            // --- Delays ---
            SigMatch::Delay1(x) => self.eval_delay1(x),
            SigMatch::Delay(value, amount) => self.eval_delay(sig, value, amount),
            SigMatch::Prefix(init, value) => {
                if self.step == 0 {
                    self.eval(init)
                } else {
                    self.eval(value)
                }
            }

            // --- Recursion ---
            SigMatch::Proj(idx, group) => self.eval_proj(idx, group),
            SigMatch::Rec(_body) => self.eval_proj(0, sig),

            // --- Passthrough / wrapper nodes ---
            SigMatch::Gen(inner) => self.eval(inner),
            SigMatch::Output(_, inner) => self.eval(inner),
            SigMatch::Lowest(x) | SigMatch::Highest(x) => self.eval(x),
            SigMatch::Attach(x, _) => self.eval(x),
            SigMatch::Enable(x, _) => self.eval(x),
            SigMatch::Control(x, _) => self.eval(x),

            // --- Table access ---
            SigMatch::RdTbl(tbl, idx) => self.eval_rdtbl(tbl, idx),
            SigMatch::Waveform(values) => {
                // Waveform as a signal: return value at current step index
                if values.is_empty() {
                    Ok(0.0)
                } else {
                    self.eval(values[self.step % values.len()])
                }
            }

            // --- Nodes that should never appear in a SIGGEN generator ---
            SigMatch::Input(_) => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "SIGGEN interpreter: Input not allowed (generators are 0-input)",
            )),
            SigMatch::Button(_)
            | SigMatch::Checkbox(_)
            | SigMatch::VSlider(_)
            | SigMatch::HSlider(_)
            | SigMatch::NumEntry(_) => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "SIGGEN interpreter: UI controls not allowed in generators",
            )),
            SigMatch::VBargraph(_, _) | SigMatch::HBargraph(_, _) => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "SIGGEN interpreter: bargraphs not allowed in generators",
            )),
            SigMatch::Soundfile(_)
            | SigMatch::SoundfileLength(_, _)
            | SigMatch::SoundfileRate(_, _)
            | SigMatch::SoundfileBuffer(_, _, _, _) => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                "SIGGEN interpreter: soundfile access not allowed in generators",
            )),
            SigMatch::FConst(_, _, _) | SigMatch::FVar(_, _, _) | SigMatch::FFun(_, _) => {
                Err(SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    "SIGGEN interpreter: foreign functions/constants/variables not supported",
                ))
            }

            _ => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "SIGGEN interpreter: unsupported signal node (expr={})",
                    dump_sig_readable(self.arena, sig)
                ),
            )),
        }
    }

    /// Evaluate Proj(idx, group) — project the idx-th output of a recursion group.
    fn eval_proj(&mut self, idx: i32, group: SigId) -> Result<f64, SignalFirError> {
        let i = idx as usize;

        // SYMREC(var, body) — recursion group definition
        if let Some((var, body)) = match_sym_rec(self.arena, group) {
            return self.eval_rec_and_project(var, Some(body), i);
        }

        // SYMREF(var) — reference to a previously registered recursion group
        if let Some(var) = match_sym_ref(self.arena, group) {
            return self.read_rec_current(var, i);
        }

        // Non-symbolic Rec(body) — use group SigId as key
        if let SigMatch::Rec(body) = match_sig(self.arena, group) {
            return self.eval_rec_and_project(group, Some(body), i);
        }

        Err(SignalFirError::new(
            SignalFirErrorCode::UnsupportedSignalNode,
            format!(
                "SIGGEN interpreter: Proj target is not a recursion group (expr={})",
                dump_sig_readable(self.arena, group)
            ),
        ))
    }

    /// Evaluate a recursion group (if not yet done this step) and return output `idx`.
    fn eval_rec_and_project(
        &mut self,
        var: SigId,
        body: Option<SigId>,
        idx: usize,
    ) -> Result<f64, SignalFirError> {
        // Initialize state if first encounter
        if !self.rec_state.contains_key(&var) {
            let n = if let Some(body) = body {
                self.collect_cons_list(body).len().max(1)
            } else {
                1
            };
            self.rec_state.insert(var, (vec![0.0; n], vec![0.0; n]));
        }

        // If not yet evaluated this step, evaluate the full group
        if !self.evaluating.contains(&var)
            && let Some(body) = body
        {
            self.evaluating.insert(var);
            let body_signals = self.collect_cons_list(body);
            let mut new_values = Vec::with_capacity(body_signals.len());
            for sig in &body_signals {
                new_values.push(self.eval(*sig)?);
            }
            if let Some((cur, _)) = self.rec_state.get_mut(&var) {
                *cur = new_values;
            }
        }

        let (cur, _) = &self.rec_state[&var];
        let canonical_index = if cur.len() == 1 { 0 } else { idx };
        if canonical_index < cur.len() {
            Ok(cur[canonical_index])
        } else {
            Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "SIGGEN interpreter: Proj index {} out of range (group has {} outputs)",
                    idx,
                    cur.len()
                ),
            ))
        }
    }

    /// Evaluate Delay1(x) — read the previous-step value.
    /// x is typically Proj(idx, group) or SYMREF(var).
    fn eval_delay1(&mut self, x: SigId) -> Result<f64, SignalFirError> {
        // SYMREF(var) → read prev[0]
        if let Some(var) = match_sym_ref(self.arena, x) {
            return self.read_rec_prev(var, 0);
        }
        // Proj(idx, group) → read prev[idx], then trigger rec body evaluation
        // so that cur[] is updated and advance() propagates the correct value.
        if let SigMatch::Proj(idx, group) = match_sig(self.arena, x) {
            if let Some((var, body)) = match_sym_rec(self.arena, group) {
                let prev_val = self.read_rec_prev(var, idx as usize)?;
                let _ = self.eval_rec_and_project(var, Some(body), idx as usize);
                return Ok(prev_val);
            }
            if let Some(var) = match_sym_ref(self.arena, group) {
                return self.read_rec_prev(var, idx as usize);
            }
            if let SigMatch::Rec(body) = match_sig(self.arena, group) {
                let prev_val = self.read_rec_prev(group, idx as usize)?;
                let _ = self.eval_rec_and_project(group, Some(body), idx as usize);
                return Ok(prev_val);
            }
        }
        // Fallback: non-recursive delay1 — return the previous-step value of x,
        // while memoizing the current-step value so the next `advance()` can
        // expose it. This matches the single-delay state semantics used by the
        // C++ generator path.
        let prev = self.delay1_prev.get(&x).copied().unwrap_or(0.0);
        if !self.delay1_current.contains_key(&x) {
            let current = self.eval(x)?;
            self.delay1_current.insert(x, current);
        }
        Ok(prev)
    }

    /// Read the current-step value of recursion group output `idx`.
    fn read_rec_current(&self, var: SigId, idx: usize) -> Result<f64, SignalFirError> {
        if let Some((cur, _)) = self.rec_state.get(&var) {
            let canonical_index = if cur.len() == 1 { 0 } else { idx };
            if canonical_index < cur.len() {
                return Ok(cur[canonical_index]);
            }
        }
        // Not yet initialized — return 0.0 (initial state)
        Ok(0.0)
    }

    /// Read the previous-step value of recursion group output `idx`.
    fn read_rec_prev(&self, var: SigId, idx: usize) -> Result<f64, SignalFirError> {
        if let Some((_, prev)) = self.rec_state.get(&var) {
            let canonical_index = if prev.len() == 1 { 0 } else { idx };
            if canonical_index < prev.len() {
                return Ok(prev[canonical_index]);
            }
        }
        // Not yet initialized — return 0.0 (initial state)
        Ok(0.0)
    }

    /// Evaluate Delay(value, amount) — multi-sample delay with history buffer.
    fn eval_delay(
        &mut self,
        sig: SigId,
        value: SigId,
        amount: SigId,
    ) -> Result<f64, SignalFirError> {
        let n = self.eval(amount)? as usize;
        // Evaluate the current value and store in history
        let current = self.eval(value)?;
        let history = self.delay_history.entry(sig).or_default();
        history.push(current);
        // Read value from n steps ago (0 if not enough history)
        if n == 0 {
            Ok(current)
        } else if history.len() > n {
            Ok(history[history.len() - 1 - n])
        } else {
            Ok(0.0)
        }
    }

    /// Evaluate RdTbl(tbl, idx) — read from a table defined as WrTbl or Waveform.
    fn eval_rdtbl(&mut self, tbl: SigId, idx: SigId) -> Result<f64, SignalFirError> {
        let index = self.eval(idx)? as i32;
        // tbl is typically WrTbl(size, generator, nil, nil) or a waveform
        match match_sig(self.arena, tbl) {
            SigMatch::WrTbl(size_sig, gen_sig, _, _) => {
                let size = self.eval(size_sig)? as usize;
                if size == 0 {
                    return Ok(0.0);
                }
                // Interpret the generator to build the table
                let table = interpret_generator(self.arena, gen_sig, size)?;
                let i = ((index % size as i32) + size as i32) as usize % size;
                Ok(table[i])
            }
            SigMatch::Waveform(values) => {
                if values.is_empty() {
                    return Ok(0.0);
                }
                let len = values.len();
                let i = ((index % len as i32) + len as i32) as usize % len;
                self.eval(values[i])
            }
            _ => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "SIGGEN interpreter: RdTbl source not supported (expr={})",
                    dump_sig_readable(self.arena, tbl)
                ),
            )),
        }
    }

    /// Extract elements from a cons-list body into a Vec.
    fn collect_cons_list(&self, body: SigId) -> Vec<SigId> {
        if let Some(elements) = list_to_vec(self.arena, body)
            && !elements.is_empty()
        {
            return elements;
        }
        // Single element (not a cons-list)
        vec![body]
    }

    /// Evaluate one binary operator while preserving prepared integer
    /// arithmetic semantics for generator nodes.
    fn eval_binop(&self, sig: SigId, op: BinOp, lhs: f64, rhs: f64) -> f64 {
        let result_ty = self.simple_type(sig);
        match op {
            BinOp::Add if result_ty == Some(SimpleSigType::Int) => {
                (lhs as i32).wrapping_add(rhs as i32) as f64
            }
            BinOp::Sub if result_ty == Some(SimpleSigType::Int) => {
                (lhs as i32).wrapping_sub(rhs as i32) as f64
            }
            BinOp::Mul if result_ty == Some(SimpleSigType::Int) => {
                (lhs as i32).wrapping_mul(rhs as i32) as f64
            }
            BinOp::Rem if result_ty == Some(SimpleSigType::Int) => {
                let rhs_i = rhs as i32;
                if rhs_i == 0 {
                    0.0
                } else {
                    ((lhs as i32) % rhs_i) as f64
                }
            }
            _ => eval_binop(op, lhs, rhs),
        }
    }
}

/// Evaluate a binary operator on `f64` values.
///
/// Division and remainder by zero return `0.0` (Faust-C++ semantics).
/// Bitwise and shift operators truncate operands to `i32`/`u32` and widen
/// the result back to `f64`.  Used by both the compile-time generator
/// interpreter and `GeneratorInterpreter::eval_binop`.
fn eval_binop(op: BinOp, lhs: f64, rhs: f64) -> f64 {
    match op {
        BinOp::Add => lhs + rhs,
        BinOp::Sub => lhs - rhs,
        BinOp::Mul => lhs * rhs,
        BinOp::Div => {
            if rhs == 0.0 {
                0.0
            } else {
                lhs / rhs
            }
        }
        BinOp::Rem => {
            if rhs == 0.0 {
                0.0
            } else {
                lhs % rhs
            }
        }
        BinOp::Lsh => ((lhs as i32) << (rhs as i32)) as f64,
        BinOp::ARsh => ((lhs as i32) >> (rhs as i32)) as f64,
        BinOp::LRsh => ((lhs as u32) >> (rhs as u32)) as f64,
        BinOp::Gt => {
            if lhs > rhs {
                1.0
            } else {
                0.0
            }
        }
        BinOp::Lt => {
            if lhs < rhs {
                1.0
            } else {
                0.0
            }
        }
        BinOp::Ge => {
            if lhs >= rhs {
                1.0
            } else {
                0.0
            }
        }
        BinOp::Le => {
            if lhs <= rhs {
                1.0
            } else {
                0.0
            }
        }
        BinOp::Eq => {
            if lhs == rhs {
                1.0
            } else {
                0.0
            }
        }
        BinOp::Ne => {
            if lhs != rhs {
                1.0
            } else {
                0.0
            }
        }
        BinOp::And => ((lhs as i32) & (rhs as i32)) as f64,
        BinOp::Or => ((lhs as i32) | (rhs as i32)) as f64,
        BinOp::Xor => ((lhs as i32) ^ (rhs as i32)) as f64,
    }
}
