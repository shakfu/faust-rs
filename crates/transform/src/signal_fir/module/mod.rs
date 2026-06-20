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
mod build;
mod core_lowering;
mod rad_formula_builder;
mod setup;
mod state;
mod tables;
mod ui_lowering;
pub(super) use build::build_module;
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
    /// Concatenates the three lifecycle phases into a single statement list,
    /// preserving execution order: `immediate`, then `post_output`, then
    /// `sample_end`.
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
/// For correct gradients the host should call `compute()` with a frame count no
/// larger than this value when using a `SigBlockReverseAD` carrier.
///
/// The tape index is masked (`i0 & (MAX_BRA_TAPE_BLOCK_SIZE - 1)`, see
/// [`SignalToFirLower::bra_tape_index`]), so an over-long block now **wraps
/// safely within the array** (aliased/approximate gradients for the tail)
/// instead of writing out of bounds. The exact fix for arbitrarily long blocks
/// is chunked TBPTT or a dynamically sized tape (analysis W5). The masking
/// relies on this constant being a power of two — enforced just below.
///
/// 8 192 samples is the default upper bound chosen to stay within typical L1/L2
/// cache pressure while leaving room for the usual block sizes used in practice
/// (64, 128, 256, 512, 1024 samples).
const MAX_BRA_TAPE_BLOCK_SIZE: usize = 8192;

// The tape-index mask `i0 & (MAX_BRA_TAPE_BLOCK_SIZE - 1)` is only equivalent to
// a bounds check when the size is a power of two.
const _: () = assert!(MAX_BRA_TAPE_BLOCK_SIZE.is_power_of_two());

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

/// Flags, per output signal, whether it must be computed in the reverse sample loop.
///
/// Returns a mask parallel to `signals`: an entry is `true` when the output is a
/// gradient projection of a `ReverseTimeRec` or a public `BlockReverseAD` group
/// (index ≥ `primal_count`), which run in reverse time. Outputs whose backward
/// work is internal to a causal `loop ~ _` recursion stay forward-time.
fn classify_reverse_time_outputs(arena: &TreeArena, signals: &[SigId]) -> Vec<bool> {
    /// Recursively tests whether `sig`'s subtree contains a reverse-time gradient
    /// projection, stopping at SYMREC boundaries and using `visited` for cycle
    /// safety.
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

// ── Small shared helpers kept at the module root ───────────────────────────

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
