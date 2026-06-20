//! Circular delay-line sizing, geometry, and state management for the FIR fast-lane.
//!
//! Faust's `@(n)` operator maps to one of three delay-line strategies, selected
//! by the delay amount relative to two thresholds (`-mcd` / `-dlt`):
//!
//! ```text
//! [1, max_copy_delay)              → ShiftModel   (shift/copy; no fIOTA)
//! [max_copy_delay, delay_line_threshold) → CircularPow2Model (default fIOTA + mask)
//! [delay_line_threshold, ∞)             → IfWrappingModel   (per-line counter)
//! ```
//!
//! This module provides:
//!
//! 1. The pure *sizing/analysis layer*: deciding how large a buffer needs to be
//!    and resolving the delay amount from the signal tree.
//! 2. The [`DelayManager`] component: owns all delay-exclusive state (allocated
//!    lines, recursion-merged delays, write-scheduling dedup) and provides the
//!    scan and allocation entry points.
//! 3. [`DelayFirCtx`]: a borrow bundle assembled from disjoint fields of
//!    `SignalToFirLower` and passed to `DelayManager` methods that emit FIR nodes.
//! 4. [`RingDelayModel`] + concrete ring-buffer implementations
//!    ([`CircularPow2Model`], [`IfWrappingModel`]): normalized buffer-geometry
//!    abstraction for pointer-driven delay lines.
//! 5. [`DelayLoweringCtx`] + [`DelayStrategyEmitter`]: strategy-local FIR
//!    emission layer used by `module.rs` to delegate concrete delay reads/writes.
//!
//! # Strategy descriptions
//!
//! ## Shift/copy (`ShiftModel`, delays ≤ `-mcd`, default 16)
//!
//! Each sample, all buffer elements are shifted one slot toward the high end,
//! and the new value is placed at index 0.  Read is a direct load at index
//! equal to the delay amount.  No `fIOTA` is used.
//!
//! ```text
//! buf[size-1] = buf[size-2]; ... buf[1] = buf[0];  (shift loop)
//! buf[0] = current_value;
//! read:  buf[N]
//! ```
//!
//! ## Power-of-two circular (`CircularPow2Model`, default middle range)
//!
//! Every active delay line is backed by one DSP-struct array (`fVec*` or
//! `iVec*`) of size `S = next_power_of_two(max_delay + 1)`.  A shared
//! integer counter `fIOTA` advances by 1 each sample and serves as the write
//! pointer.  Reads use a masked offset: `array[(fIOTA - N) & (S - 1)]`.
//!
//! ```text
//! write: array[fIOTA & (S-1)]  = current_value;
//! read:  array[(fIOTA - N) & (S-1)]
//! end-of-sample: fIOTA = fIOTA + 1;
//! ```
//!
//! ## If-based wrapping (`IfWrappingModel`, delays > `-dlt`)
//!
//! Uses an exact-size buffer (size = `max_delay + 1`) with a dedicated
//! per-line integer counter.  The counter wraps to zero via an `if` comparison
//! instead of a bitmask, saving memory for non-power-of-two delay sizes at the
//! cost of a branch per write.
//!
//! ```text
//! buf[idx] = current_value;
//! read:  buf[(idx + size - N) select2-wrapped]   (see IfWrappingModel)
//! end-of-sample: idx = (idx + 1 >= size) ? 0 : idx + 1;
//! ```
//!
//! # Recursion + delay merging
//!
//! When a recursion output is consumed through a delay chain
//! `Delay1^k(Proj(i, group))` — either from an active `SYMREF` feedback edge or
//! from a top-level `SYMREC` projection — the recursion array for output `i`
//! is sized to hold the full delayed history so no separate `fVec` is needed.
//!
//! The planning split is now:
//!
//! - [`DelayManager::analyze_signals`] records the canonical maximum delayed
//!   access per recursion output
//! - [`DelayManager::scan_signals`] records direct delay-line ownership for
//!   non-recursive carried signals and legacy direct merge bookkeeping
//! - `ensure_recursion_array_for_group` in `module.rs` consumes the accumulated
//!   recursion-output analysis to size recursion carriers
//!
//! Standalone `Delay1(x)` nodes that use the shift strategy are also recorded
//! during the same scan so their buffer geometry is chosen once up front and
//! later reused by the lowering phase without allocation side effects.
//!
//! # Scope of this module
//!
//! | Item | Kind | Purpose |
//! |------|------|---------|
//! | [`pow2limit_for_delay`] | free fn | `next_power_of_two(N + 1)` with overflow checks |
//! | [`constant_delay_amount`] | free fn | Extract a literal `SIGINT` delay amount |
//! | [`variable_delay_max_bound`] | free fn | Derive a bound from interval analysis |
//! | [`min_const_upper_bound`] | free fn | Structural fallback: `SIGMIN(SigInt(n), _)` |
//! | [`delay_size_for_amount`] | free fn | Unified resolver (tries all three above) |
//! | [`DelayOptions`] | struct | `-mcd` / `-dlt` threshold options |
//! | [`DelayStrategy`] | enum | Which buffer geometry is used for a line |
//! | [`DelayLineInfo`] | struct | Metadata for one allocated delay buffer |
//! | [`DelayManager`] | struct | Owns delay-exclusive state; scan + alloc methods |
//! | [`DelayFirCtx`] | struct | Borrow bundle for FIR-emitting methods |
//! | [`GlobalCircularCursor`] | struct | Shared `fIOTA` cursor service for circular storage |
//! | [`RingDelayModel`] | trait | Ring-buffer geometry abstraction |
//! | [`CircularPow2Model`] | struct | Power-of-two implementation |
//! | [`IfWrappingModel`] | struct | Exact-size if-wrapping implementation |
//! | [`DelayStrategyEmitter`] | trait | Strategy-local lowering interface |
//!
//! The complementary **stateful orchestration** layer (`lower_fixed_delay`,
//! `lower_delay_state`, recursion-carrier resolution, and `lower_signal`
//! dispatch) remains in `module.rs`, while the strategy-specific FIR emission
//! primitives now live here behind [`DelayStrategyEmitter`].
//!
//! Source provenance (C++):
//! - `compiler/transform/signalFIRCompiler.hh` — `DelayLine`, `allocateDelayLineAux`
//! - `compiler/transform/signalFIRCompiler.cpp` — `compileSigDelay`, `writeReadDelay`,
//!   `checkDelayInterval`

use std::collections::{HashMap, HashSet};

use fir::helpers::{emit_reverse_array_shift_loop, fresh_loop_var};
use fir::{AccessType, FirBinOp, FirBuilder, FirId, FirStore, FirType};
use signals::{SigId, SigMatch, match_sig};
use sigtype::{SigType, check_delay_interval};
use tlib::{TreeArena, list_to_vec, match_sym_rec, match_sym_ref};

use crate::signal_prepare::SimpleSigType;

use super::error::{SignalFirError, SignalFirErrorCode};

// ─── DelayOptions ─────────────────────────────────────────────────────────────

/// Delay-line strategy selection thresholds.
///
/// Mirror of the Faust `-mcd` / `-dlt` compiler options:
///
/// - `-mcd N` (max-copy-delay, default 16): delays ≤ N use the shift/copy
///   strategy (no `fIOTA`).
/// - `-dlt N` (delay-line threshold, default `u32::MAX`): delays > N use the
///   if-based wrapping strategy; delays in `(mcd, dlt]` use the default
///   power-of-two circular strategy.
#[derive(Clone, Debug)]
pub(super) struct DelayOptions {
    /// Shift/copy model upper bound (inclusive).  Default: 16.
    pub(super) max_copy_delay: u32,
    /// If-based wrapping model lower bound (exclusive).  Default: `u32::MAX`
    /// (disabled; all non-copy delays use the circular-pow2 model).
    pub(super) delay_line_threshold: u32,
}

impl Default for DelayOptions {
    fn default() -> Self {
        Self {
            max_copy_delay: 16,
            delay_line_threshold: u32::MAX,
        }
    }
}

// ─── DelayStrategy ────────────────────────────────────────────────────────────

/// Buffer-geometry strategy for a single allocated delay line.
///
/// Selected once per carried signal by [`DelayManager::ensure_delay_line`]
/// based on the maximum observed delay amount and [`DelayOptions`].
#[derive(Clone, Debug)]
pub(super) enum DelayStrategy {
    /// Shift/copy: contents shifted by one each sample; new value stored at
    /// index 0; read at index = delay.  No `fIOTA`.  Buffer size = `delay + 1`.
    Shift,
    /// Power-of-two circular buffer shared with the global `fIOTA` counter.
    /// Buffer size = `next_power_of_two(delay + 1)`.
    CircularPow2,
    /// Per-line if-based wrapping counter; exact buffer size (`delay + 1`).
    /// Each line has its own `fIdx<sig_id>` struct variable.
    IfWrapping {
        /// Name of the per-line counter variable, e.g. `fIdx42`.
        counter_name: String,
    },
}

// ─── DelayLineInfo ────────────────────────────────────────────────────────────

/// Metadata for one allocated delay-line DSP-struct array.
///
/// The array is named `fVec<id>` (real) or `iVec<id>` (integer), declared
/// during the [`DelayManager::scan_signals`] / `prepare_delay_lines` pre-scan
/// and zeroed in `instanceClear`.
///
/// Source provenance (C++):
/// - `compiler/transform/signalFIRCompiler.hh` (`DelayLine`, `allocateDelayLineAux`)
/// - `compiler/transform/signalFIRCompiler.cpp` (`compileSigDelay`, `writeReadDelay`)
#[derive(Clone, Debug)]
pub(super) struct DelayLineInfo {
    /// Generated DSP-struct array variable name (e.g. `fVec42`).
    pub(super) name: String,
    /// Allocated buffer size in elements.
    ///
    /// For `CircularPow2` this is always a power of two ≥ 1.
    /// For `Shift` and `IfWrapping` this is `max_delay + 1` (exact).
    pub(super) size: usize,
    /// Buffer-geometry strategy selected for this line.
    pub(super) strategy: DelayStrategy,
}

/// Read-only delay-analysis metadata for one signal carrier.
///
/// This is the first Rust-side equivalent of the C++ occurrence/delay analysis:
/// it records the maximum accumulated delay observed on a signal and how many
/// delayed accesses reached that carrier during the scan.
///
/// The data is intentionally kept separate from FIR resource allocation so
/// future planning steps can consume it without side effects.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DelayAnalysisEntry {
    /// Largest accumulated delayed access observed on this carrier.
    pub(super) max_delay: i32,
    /// Number of delayed accesses observed on this carrier.
    pub(super) delay_count: u32,
}

// ─── pow2limit_for_delay ─────────────────────────────────────────────────────

/// Computes `next_power_of_two(delay + 1)` — the circular buffer size for
/// a given maximum delay in samples.
///
/// A delay of `N` samples requires reading `N` positions behind the write
/// pointer.  With a power-of-two size `S`, the mask `S - 1` covers all valid
/// offsets `0..=N`, so the minimum `S` is `next_power_of_two(N + 1)`.
///
/// # Errors
///
/// Returns [`SignalFirErrorCode::UnsupportedSignalNode`] if `delay` is
/// negative, if `delay + 1` overflows `usize`, or if the result would exceed
/// `usize::MAX` (i.e. `delay >= usize::MAX / 2`).
///
/// | `delay` | result |
/// |---------|--------|
/// | `0` | `1` (passthrough needs 1 slot) |
/// | `1` | `2` |
/// | `3` | `4` |
/// | `4` | `8` |
/// | `10` | `16` |
pub(super) fn pow2limit_for_delay(delay: i32) -> Result<usize, SignalFirError> {
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

// ─── Delay amount resolution ──────────────────────────────────────────────────

/// Returns the constant integer value of `sig` if it is a `SIGINT` literal,
/// otherwise `None`.
///
/// This is the fast path for compile-time constant delay amounts (e.g. `x @ 3`
/// after constant propagation).  The returned value is the exact delay in
/// samples; callers should pass it directly to [`pow2limit_for_delay`].
pub(super) fn constant_delay_amount(
    arena: &TreeArena,
    sig: SigId,
) -> Result<Option<i32>, SignalFirError> {
    match match_sig(arena, sig) {
        SigMatch::Int(value) => Ok(Some(value)),
        _ => Ok(None),
    }
}

/// Returns the interval upper-bound used to size the delay line for a
/// variable delay amount, mirroring C++ `checkDelayInterval`.
///
/// Accepts any signal whose full type annotation has a non-empty, bounded
/// (finite `hi`), non-negative interval.  `hi == 0` is the zero-delay
/// passthrough case.
///
/// Returns `None` when:
/// - `sig` has no type entry in `sig_types`,
/// - the interval is empty or unbounded (infinite `hi`), or
/// - `hi < 0` (negative delay, semantically invalid).
///
/// # C++ correspondence
///
/// Mirrors `signalFIRCompiler.cpp::checkDelayInterval`, which rejects any
/// delay with an interval whose upper bound cannot be determined or is
/// negative.
pub(super) fn variable_delay_max_bound(
    sig_types: &HashMap<SigId, SigType>,
    sig: SigId,
) -> Option<i32> {
    let ty = sig_types.get(&sig)?;
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
/// # When this fires
///
/// With correct `FConst` typing (`Interval::new_default()` rather than
/// `empty()`), `fSamplingFreq`-based expressions (e.g. `ma.SR`) produce a
/// finite bounded interval through standard interval algebra and do not reach
/// this fallback.  This method acts as defence-in-depth for any remaining
/// case where interval analysis yields an empty or unbounded result.
pub(super) fn min_const_upper_bound(arena: &TreeArena, sig: SigId) -> Option<i32> {
    let SigMatch::Min(lhs, rhs) = match_sig(arena, sig) else {
        return None;
    };
    let as_nonneg_int = |id: SigId| -> Option<i32> {
        if let SigMatch::Int(n) = match_sig(arena, id)
            && n >= 0
        {
            return Some(n);
        }
        None
    };
    as_nonneg_int(lhs).or_else(|| as_nonneg_int(rhs))
}

/// Resolves the delay line allocation size for a delay `amount` signal.
///
/// Tries three strategies in order of specificity:
///
/// 1. **Literal `SIGINT`** — exact compile-time constant via
///    [`constant_delay_amount`].
/// 2. **Bounded interval** — interval upper bound from the type annotator
///    via [`variable_delay_max_bound`].  Covers slider-driven and
///    `fSamplingFreq`-derived amounts after type propagation.
/// 3. **Structural `SIGMIN` fallback** — conservative upper bound from
///    `SIGMIN(SigInt(n), _)` patterns via [`min_const_upper_bound`].
///    Defence-in-depth for cases where interval analysis still yields empty.
///
/// Returns `None` when no bound can be determined; the caller should report
/// an `UnsupportedSignalNode` error.
///
/// Returns `Some(0)` for a zero-delay (passthrough) amount.
pub(super) fn delay_size_for_amount(
    arena: &TreeArena,
    sig_types: &HashMap<SigId, SigType>,
    amount: SigId,
) -> Result<Option<i32>, SignalFirError> {
    if let Some(c) = constant_delay_amount(arena, amount)? {
        return Ok(Some(c));
    }
    if let Some(b) = variable_delay_max_bound(sig_types, amount) {
        return Ok(Some(b));
    }
    Ok(min_const_upper_bound(arena, amount))
}

// ─── GlobalCircularCursor ────────────────────────────────────────────────────

/// Shared runtime cursor used by all global masked circular-storage paths.
///
/// Today this is materialized as the persistent struct field `fIOTA`. It is
/// shared by `CircularPow2` delay lines and by circular recursion carriers
/// lowered from `module.rs`.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct GlobalCircularCursor;

impl GlobalCircularCursor {
    /// Declares and clears the shared `fIOTA` state, idempotent.
    pub(super) fn ensure_state(self, ctx: &mut DelayFirCtx<'_>) {
        if *ctx.uses_iota {
            return;
        }
        *ctx.uses_iota = true;
        let zero = {
            let mut b = FirBuilder::new(ctx.store);
            b.int32(0)
        };
        let decl = {
            let mut b = FirBuilder::new(ctx.store);
            b.declare_var("fIOTA", FirType::Int32, AccessType::Struct, None)
        };
        ctx.struct_declarations.push(decl);
        if ctx.clear_init_seen.insert("fIOTA".to_owned()) {
            let mut b = FirBuilder::new(ctx.store);
            ctx.clear_statements
                .push(b.store_var("fIOTA", AccessType::Struct, zero));
        }
    }

    /// Loads the current cursor value from the DSP struct.
    pub(super) fn load(self, store: &mut FirStore) -> FirId {
        let mut b = FirBuilder::new(store);
        b.load_var("fIOTA", AccessType::Struct, FirType::Int32)
    }

    /// Computes the masked current write index `fIOTA & (size - 1)`.
    pub(super) fn current_index(self, store: &mut FirStore, size: usize) -> FirId {
        let iota = self.load(store);
        masked_delay_index(store, iota, size)
    }

    /// Computes the masked delayed read index `(fIOTA - amount) & (size - 1)`.
    pub(super) fn delayed_index(self, store: &mut FirStore, amount: FirId, size: usize) -> FirId {
        let iota = self.load(store);
        let raw = {
            let mut b = FirBuilder::new(store);
            b.binop(FirBinOp::Sub, iota, amount, FirType::Int32)
        };
        masked_delay_index(store, raw, size)
    }

    /// Emits `fIOTA = fIOTA + 1` to advance the cursor by one sample.
    pub(super) fn emit_advance(self, store: &mut FirStore) -> FirId {
        let next = {
            let iota = self.load(store);
            let one = {
                let mut b = FirBuilder::new(store);
                b.int32(1)
            };
            let mut b = FirBuilder::new(store);
            b.binop(FirBinOp::Add, iota, one, FirType::Int32)
        };
        let mut b = FirBuilder::new(store);
        b.store_var("fIOTA", AccessType::Struct, next)
    }
}

// ─── RingDelayModel ───────────────────────────────────────────────────────────

/// Runtime write-pointer source used by ring-buffer delay strategies.
#[derive(Clone, Copy, Debug)]
pub(super) enum DelayRuntimeState<'a> {
    /// Shared global `fIOTA` counter.
    GlobalIota,
    /// Per-line `fIdx*` counter.
    Counter(&'a str),
}

impl DelayRuntimeState<'_> {
    fn load_current_index(self, store: &mut FirStore) -> FirId {
        match self {
            Self::GlobalIota => GlobalCircularCursor.load(store),
            Self::Counter(name) => {
                let mut b = FirBuilder::new(store);
                b.load_var(name, AccessType::Struct, FirType::Int32)
            }
        }
    }
}

/// Abstraction over pointer-driven ring-buffer geometries used by delay lines.
///
/// Both [`CircularPow2Model`] and [`IfWrappingModel`] share the same conceptual
/// contract:
///
/// - choose a backing buffer size,
/// - compute a current write slot,
/// - compute a delayed read slot,
/// - advance the runtime pointer state by one sample.
pub(super) trait RingDelayModel {
    /// Minimum buffer size in elements for a maximum delay of `max_delay` samples.
    fn buffer_size(&self, max_delay: i32) -> Result<usize, SignalFirError>;

    /// FIR expression: index of the current write slot.
    fn write_index(&self, store: &mut FirStore, state: DelayRuntimeState<'_>, size: usize)
    -> FirId;

    /// FIR expression: index of the slot that is `amount` samples behind the
    /// write pointer.
    fn read_index(
        &self,
        store: &mut FirStore,
        state: DelayRuntimeState<'_>,
        amount: FirId,
        size: usize,
    ) -> FirId;

    /// FIR statement that advances the pointer state by one sample.
    fn emit_advance(
        &self,
        store: &mut FirStore,
        state: DelayRuntimeState<'_>,
        size: usize,
    ) -> FirId;
}

// ─── CircularPow2Model ────────────────────────────────────────────────────────

/// Power-of-two circular buffer geometry.
///
/// Buffer size = `next_power_of_two(max_delay + 1)`.
/// Write index = `fIOTA & (size - 1)`.
/// Read index  = `(fIOTA - amount) & (size - 1)`.
/// Bump        = `fIOTA + 1`.
///
/// This is the only model currently implemented; it matches C++ Faust's
/// `signalFIRCompiler::writeReadDelay` exactly.
pub(super) struct CircularPow2Model;

impl RingDelayModel for CircularPow2Model {
    fn buffer_size(&self, max_delay: i32) -> Result<usize, SignalFirError> {
        pow2limit_for_delay(max_delay)
    }

    fn write_index(
        &self,
        store: &mut FirStore,
        state: DelayRuntimeState<'_>,
        size: usize,
    ) -> FirId {
        match state {
            DelayRuntimeState::GlobalIota => GlobalCircularCursor.current_index(store, size),
            DelayRuntimeState::Counter(_) => {
                let iota = state.load_current_index(store);
                masked_delay_index(store, iota, size)
            }
        }
    }

    fn read_index(
        &self,
        store: &mut FirStore,
        state: DelayRuntimeState<'_>,
        amount: FirId,
        size: usize,
    ) -> FirId {
        match state {
            DelayRuntimeState::GlobalIota => {
                GlobalCircularCursor.delayed_index(store, amount, size)
            }
            DelayRuntimeState::Counter(_) => {
                let iota = state.load_current_index(store);
                let raw = FirBuilder::new(store).binop(FirBinOp::Sub, iota, amount, FirType::Int32);
                masked_delay_index(store, raw, size)
            }
        }
    }

    fn emit_advance(
        &self,
        store: &mut FirStore,
        state: DelayRuntimeState<'_>,
        _size: usize,
    ) -> FirId {
        debug_assert!(matches!(state, DelayRuntimeState::GlobalIota));
        GlobalCircularCursor.emit_advance(store)
    }
}

// ─── IfWrappingModel ─────────────────────────────────────────────────────────

/// Exact-size circular buffer with an if-wrapping per-line counter.
pub(super) struct IfWrappingModel;

impl RingDelayModel for IfWrappingModel {
    fn buffer_size(&self, max_delay: i32) -> Result<usize, SignalFirError> {
        usize::try_from(max_delay)
            .map(|delay| delay + 1)
            .map_err(|_| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!("SIGDELAY amount overflow: {max_delay}"),
                )
            })
    }

    fn write_index(
        &self,
        store: &mut FirStore,
        state: DelayRuntimeState<'_>,
        _size: usize,
    ) -> FirId {
        state.load_current_index(store)
    }

    fn read_index(
        &self,
        store: &mut FirStore,
        state: DelayRuntimeState<'_>,
        amount: FirId,
        size: usize,
    ) -> FirId {
        let DelayRuntimeState::Counter(counter_name) = state else {
            debug_assert!(false, "IfWrappingModel requires a per-line counter");
            return GlobalCircularCursor.load(store);
        };
        if_wrapping_read_index(store, counter_name, amount, size)
    }

    fn emit_advance(
        &self,
        store: &mut FirStore,
        state: DelayRuntimeState<'_>,
        size: usize,
    ) -> FirId {
        let DelayRuntimeState::Counter(counter_name) = state else {
            debug_assert!(false, "IfWrappingModel requires a per-line counter");
            return GlobalCircularCursor.emit_advance(store);
        };
        bump_if_wrapping_counter(store, counter_name, size)
    }
}

// ─── DelayFirCtx ─────────────────────────────────────────────────────────────

/// Borrowed context bundle for delay-line FIR emission.
///
/// Assembled from disjoint fields of `SignalToFirLower` using Rust's field-level
/// split-borrow facility.  Because the `delay: DelayManager` field of
/// `SignalToFirLower` is NOT included here, callers can hold both a
/// `&mut DelayManager` and a `&mut DelayFirCtx` simultaneously.
///
/// # Construction
///
/// Construct via an explicit struct literal at each call site in `module.rs`:
///
/// ```rust,ignore
/// let mut ctx = DelayFirCtx {
///     store: &mut self.store,
///     real_ty: self.real_ty.clone(),
///     types: self.types,
///     struct_declarations: &mut self.struct_declarations,
///     clear_statements: &mut self.clear_statements,
///     clear_init_seen: &mut self.clear_init_seen,
///     next_loop_var_id: &mut self.next_loop_var_id,
///     uses_iota: &mut self.uses_iota,
/// };
/// self.delay.ensure_delay_line(carried, delay, &mut ctx)?;
/// ```
///
/// **Do not** construct via a `&mut self` method call — that would borrow all of
/// `self` and prevent the simultaneous borrow of `self.delay`.
pub(super) struct DelayFirCtx<'a> {
    pub(super) store: &'a mut FirStore,
    pub(super) real_ty: FirType,
    pub(super) types: &'a HashMap<SigId, SimpleSigType>,
    pub(super) struct_declarations: &'a mut Vec<FirId>,
    pub(super) clear_statements: &'a mut Vec<FirId>,
    pub(super) clear_init_seen: &'a mut HashSet<String>,
    pub(super) next_loop_var_id: &'a mut usize,
    pub(super) uses_iota: &'a mut bool,
}

impl<'a> DelayFirCtx<'a> {
    /// Returns the FIR element type for a delay-line carrier signal.
    pub(super) fn signal_elem_type(&self, carried: SigId) -> Result<FirType, SignalFirError> {
        match self.types.get(&carried) {
            Some(SimpleSigType::Int) => Ok(FirType::Int32),
            Some(SimpleSigType::Real) => Ok(self.real_ty.clone()),
            Some(SimpleSigType::Sound) => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "signal {} cannot use a soundfile handle as delay-line element type",
                    carried.as_u32()
                ),
            )),
            None => Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("missing prepared type for signal {}", carried.as_u32()),
            )),
        }
    }

    /// Declares the `fIOTA` circular-buffer position counter, idempotent.
    ///
    /// Sets `*uses_iota = true`, emits the struct declaration, and registers a
    /// `instanceClear` assignment `fIOTA = 0`.
    pub(super) fn ensure_iota(&mut self) {
        GlobalCircularCursor.ensure_state(self);
    }

    /// Generates a fresh loop-variable name using the shared monotonic counter.
    pub(super) fn fresh_loop_var(&mut self, prefix: &str) -> String {
        fresh_loop_var(self.next_loop_var_id, prefix)
    }

    /// Declares the per-line `fIdx<id>` counter for an `IfWrapping` delay line,
    /// idempotent.
    ///
    /// Emits the struct declaration and an `instanceClear` assignment `counter = 0`.
    pub(super) fn ensure_if_wrapping_counter(&mut self, counter_name: String) {
        if !self.clear_init_seen.insert(counter_name.clone()) {
            return;
        }
        let zero = {
            let mut b = FirBuilder::new(self.store);
            b.int32(0)
        };
        let decl = {
            let mut b = FirBuilder::new(self.store);
            b.declare_var(
                counter_name.clone(),
                FirType::Int32,
                AccessType::Struct,
                None,
            )
        };
        self.struct_declarations.push(decl);
        let mut b = FirBuilder::new(self.store);
        self.clear_statements
            .push(b.store_var(counter_name, AccessType::Struct, zero));
    }

    /// Emits an `instanceClear` zeroing loop for a delay-line array, idempotent.
    ///
    /// Uses `clear_init_seen` for deduplication.  The element zero value is
    /// derived from `sig`'s `SimpleSigType`: `Int32` → `0i32`, `Real` → `0.0`.
    pub(super) fn register_delay_clear(
        &mut self,
        name: String,
        size: usize,
        sig: SigId,
    ) -> Result<(), SignalFirError> {
        if !self.clear_init_seen.insert(name.clone()) {
            return Ok(());
        }
        let loop_var = self.fresh_loop_var("lDelay");
        let upper = {
            let mut b = FirBuilder::new(self.store);
            b.int32(i32::try_from(size).map_err(|_| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!("delay line size conversion overflow: {size}"),
                )
            })?)
        };
        let zero = match self.types.get(&sig) {
            Some(SimpleSigType::Int) => {
                let mut b = FirBuilder::new(self.store);
                b.int32(0)
            }
            Some(SimpleSigType::Real) => {
                let mut b = FirBuilder::new(self.store);
                match self.real_ty {
                    FirType::Float64 => b.float64(0.0),
                    _ => b.float32(0.0),
                }
            }
            _ => {
                return Err(SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!("cannot zero-init delay-line for signal {}", sig.as_u32()),
                ));
            }
        };
        let body = {
            let index = {
                let mut b = FirBuilder::new(self.store);
                b.load_var(loop_var.clone(), AccessType::Loop, FirType::Int32)
            };
            let store_node = {
                let mut b = FirBuilder::new(self.store);
                b.store_table(name, AccessType::Struct, index, zero)
            };
            let mut b = FirBuilder::new(self.store);
            b.block(&[store_node])
        };
        let mut b = FirBuilder::new(self.store);
        self.clear_statements
            .push(b.simple_for_loop(loop_var, upper, body, false));
        Ok(())
    }
}

// ─── Delay lowering context + strategy emitters ─────────────────────────────

/// Borrow bundle for strategy-local FIR emission during lowering.
pub(super) struct DelayLoweringCtx<'a> {
    pub(super) store: &'a mut FirStore,
    pub(super) immediate_statements: &'a mut Vec<FirId>,
    pub(super) post_output_statements: &'a mut Vec<FirId>,
    pub(super) next_loop_var_id: &'a mut usize,
}

/// Strategy-local FIR emission API used by `module.rs`.
pub(super) trait DelayStrategyEmitter {
    /// Emits one `SIGDELAY(value, amount)` read/write sequence.
    fn emit_fixed_delay(
        &self,
        ctx: &mut DelayLoweringCtx<'_>,
        line: &DelayLineInfo,
        current: FirId,
        amount: FirId,
        read_ty: FirType,
        schedule_write: bool,
    ) -> FirId;

    /// Emits one `Delay1(value)` read/write sequence.
    fn emit_delay1(
        &self,
        ctx: &mut DelayLoweringCtx<'_>,
        line: &DelayLineInfo,
        current: FirId,
        read_ty: FirType,
        schedule_write: bool,
    ) -> FirId;
}

/// Shift/copy delay lowering strategy.
pub(super) struct ShiftDelayStrategyEmitter;

impl DelayStrategyEmitter for ShiftDelayStrategyEmitter {
    fn emit_fixed_delay(
        &self,
        ctx: &mut DelayLoweringCtx<'_>,
        line: &DelayLineInfo,
        current: FirId,
        amount: FirId,
        read_ty: FirType,
        schedule_write: bool,
    ) -> FirId {
        if schedule_write {
            let store_0 = emit_store_at_zero(ctx.store, &line.name, current);
            ctx.immediate_statements.push(store_0);
            let delay_n = i32::try_from(line.size).unwrap_or(i32::MAX) - 1;
            if delay_n <= 2 {
                let copies =
                    emit_unrolled_shift_copies(ctx.store, &line.name, delay_n, read_ty.clone());
                ctx.post_output_statements.extend(copies);
            } else {
                let shift = emit_shift_loop(ctx, &line.name, delay_n, read_ty.clone());
                ctx.post_output_statements.push(shift);
            }
        }
        let mut b = FirBuilder::new(ctx.store);
        b.load_table(line.name.clone(), AccessType::Struct, amount, read_ty)
    }

    fn emit_delay1(
        &self,
        ctx: &mut DelayLoweringCtx<'_>,
        line: &DelayLineInfo,
        current: FirId,
        read_ty: FirType,
        schedule_write: bool,
    ) -> FirId {
        if schedule_write {
            let store_0 = emit_store_at_zero(ctx.store, &line.name, current);
            ctx.immediate_statements.push(store_0);
            let delay_n = i32::try_from(line.size).unwrap_or(i32::MAX) - 1;
            if delay_n <= 2 {
                let copies =
                    emit_unrolled_shift_copies(ctx.store, &line.name, delay_n, read_ty.clone());
                ctx.post_output_statements.extend(copies);
            } else {
                let shift = emit_shift_loop(ctx, &line.name, delay_n, read_ty.clone());
                ctx.post_output_statements.push(shift);
            }
        }
        let one = {
            let mut b = FirBuilder::new(ctx.store);
            b.int32(1)
        };
        let mut b = FirBuilder::new(ctx.store);
        b.load_table(line.name.clone(), AccessType::Struct, one, read_ty)
    }
}

/// Ring-buffer-based delay lowering strategy backed by a [`RingDelayModel`].
pub(super) struct RingDelayStrategyEmitter<M> {
    model: M,
}

impl<M> RingDelayStrategyEmitter<M> {
    pub(super) fn new(model: M) -> Self {
        Self { model }
    }
}

impl<M: RingDelayModel> DelayStrategyEmitter for RingDelayStrategyEmitter<M> {
    fn emit_fixed_delay(
        &self,
        ctx: &mut DelayLoweringCtx<'_>,
        line: &DelayLineInfo,
        current: FirId,
        amount: FirId,
        read_ty: FirType,
        schedule_write: bool,
    ) -> FirId {
        let state = runtime_state_for_line(line);
        if schedule_write {
            let write_index = self.model.write_index(ctx.store, state, line.size);
            let mut b = FirBuilder::new(ctx.store);
            ctx.immediate_statements.push(b.store_table(
                line.name.clone(),
                AccessType::Struct,
                write_index,
                current,
            ));
        }
        let read_index = self.model.read_index(ctx.store, state, amount, line.size);
        let mut b = FirBuilder::new(ctx.store);
        b.load_table(line.name.clone(), AccessType::Struct, read_index, read_ty)
    }

    fn emit_delay1(
        &self,
        ctx: &mut DelayLoweringCtx<'_>,
        line: &DelayLineInfo,
        current: FirId,
        read_ty: FirType,
        schedule_write: bool,
    ) -> FirId {
        let one = {
            let mut b = FirBuilder::new(ctx.store);
            b.int32(1)
        };
        self.emit_fixed_delay(ctx, line, current, one, read_ty, schedule_write)
    }
}

fn runtime_state_for_line(line: &DelayLineInfo) -> DelayRuntimeState<'_> {
    match &line.strategy {
        DelayStrategy::CircularPow2 => DelayRuntimeState::GlobalIota,
        DelayStrategy::IfWrapping { counter_name } => DelayRuntimeState::Counter(counter_name),
        DelayStrategy::Shift => {
            debug_assert!(false, "shift delay lines do not have ring runtime state");
            DelayRuntimeState::GlobalIota
        }
    }
}

/// Dispatches `SIGDELAY` lowering to the strategy-specific emitter.
pub(super) fn emit_fixed_delay_for_line(
    ctx: &mut DelayLoweringCtx<'_>,
    line: &DelayLineInfo,
    current: FirId,
    amount: FirId,
    read_ty: FirType,
    schedule_write: bool,
) -> FirId {
    match line.strategy {
        DelayStrategy::Shift => ShiftDelayStrategyEmitter.emit_fixed_delay(
            ctx,
            line,
            current,
            amount,
            read_ty,
            schedule_write,
        ),
        DelayStrategy::CircularPow2 => RingDelayStrategyEmitter::new(CircularPow2Model)
            .emit_fixed_delay(ctx, line, current, amount, read_ty, schedule_write),
        DelayStrategy::IfWrapping { .. } => RingDelayStrategyEmitter::new(IfWrappingModel)
            .emit_fixed_delay(ctx, line, current, amount, read_ty, schedule_write),
    }
}

/// Dispatches `Delay1` lowering to the strategy-specific emitter.
pub(super) fn emit_delay1_for_line(
    ctx: &mut DelayLoweringCtx<'_>,
    line: &DelayLineInfo,
    current: FirId,
    read_ty: FirType,
    schedule_write: bool,
) -> FirId {
    match line.strategy {
        DelayStrategy::Shift => {
            ShiftDelayStrategyEmitter.emit_delay1(ctx, line, current, read_ty, schedule_write)
        }
        DelayStrategy::CircularPow2 => RingDelayStrategyEmitter::new(CircularPow2Model)
            .emit_delay1(ctx, line, current, read_ty, schedule_write),
        DelayStrategy::IfWrapping { .. } => RingDelayStrategyEmitter::new(IfWrappingModel)
            .emit_delay1(ctx, line, current, read_ty, schedule_write),
    }
}

/// Emits the end-of-sample counter advance for one `IfWrapping` delay line.
fn emit_if_wrapping_advance(store: &mut FirStore, counter_name: &str, size: usize) -> FirId {
    IfWrappingModel.emit_advance(store, DelayRuntimeState::Counter(counter_name), size)
}

// ─── FIR emission helpers shared with strategy emitters ─────────────────────

/// Applies the power-of-two ring-buffer mask: `index & (size - 1)`.
pub(super) fn masked_delay_index(store: &mut FirStore, index: FirId, size: usize) -> FirId {
    let mask = {
        let mut b = FirBuilder::new(store);
        b.int32(i32::try_from(size.saturating_sub(1)).unwrap_or(i32::MAX))
    };
    let mut b = FirBuilder::new(store);
    b.binop(FirBinOp::And, index, mask, FirType::Int32)
}

/// Emits `buf[0] = new_value` — the immediate write for the Shift strategy.
fn emit_store_at_zero(store: &mut FirStore, name: &str, new_value: FirId) -> FirId {
    let zero = {
        let mut b = FirBuilder::new(store);
        b.int32(0)
    };
    let mut b = FirBuilder::new(store);
    b.store_table(name, AccessType::Struct, zero, new_value)
}

/// Emits unrolled shift copies for a Shift delay line with `delay ≤ 2`.
///
/// Returns individual store instructions in high-to-low order:
/// - delay=1: `[buf[1] = buf[0]]`
/// - delay=2: `[buf[2] = buf[1], buf[1] = buf[0]]`
fn emit_unrolled_shift_copies(
    store: &mut FirStore,
    name: &str,
    delay: i32,
    elem_ty: FirType,
) -> Vec<FirId> {
    let delay_usize = usize::try_from(delay).unwrap_or(0);
    let mut copies = Vec::with_capacity(delay_usize);
    for j in (1..=delay_usize).rev() {
        let j_idx = {
            let mut b = FirBuilder::new(store);
            b.int32(i32::try_from(j).unwrap_or(i32::MAX))
        };
        let j_minus_1_idx = {
            let mut b = FirBuilder::new(store);
            b.int32(i32::try_from(j - 1).unwrap_or(i32::MAX))
        };
        let loaded = {
            let mut b = FirBuilder::new(store);
            b.load_table(name, AccessType::Struct, j_minus_1_idx, elem_ty.clone())
        };
        let stored = {
            let mut b = FirBuilder::new(store);
            b.store_table(name, AccessType::Struct, j_idx, loaded)
        };
        copies.push(stored);
    }
    copies
}

/// Emits a reverse `ForLoop` shift for a Shift delay line with `delay ≥ 3`.
///
/// Generates:
/// ```text
/// for (int j = delay; j > 0; j = j + -1)
///     buf[j] = buf[j - 1];
/// ```
fn emit_shift_loop(
    ctx: &mut DelayLoweringCtx<'_>,
    name: &str,
    delay: i32,
    elem_ty: FirType,
) -> FirId {
    emit_reverse_array_shift_loop(
        ctx.store,
        ctx.next_loop_var_id,
        "j",
        name,
        delay,
        elem_ty,
        AccessType::Struct,
    )
}

/// Computes the read index for an `IfWrapping` delay line:
/// `(counter + size - amount)` with if-based wrap when `≥ size`.
fn if_wrapping_read_index(
    store: &mut FirStore,
    counter_name: &str,
    amount: FirId,
    size: usize,
) -> FirId {
    let size_i32 = i32::try_from(size).unwrap_or(i32::MAX);
    let counter = {
        let mut b = FirBuilder::new(store);
        b.load_var(counter_name, AccessType::Struct, FirType::Int32)
    };
    let size_fir = {
        let mut b = FirBuilder::new(store);
        b.int32(size_i32)
    };
    let plus_size = {
        let mut b = FirBuilder::new(store);
        b.binop(FirBinOp::Add, counter, size_fir, FirType::Int32)
    };
    let raw = {
        let mut b = FirBuilder::new(store);
        b.binop(FirBinOp::Sub, plus_size, amount, FirType::Int32)
    };
    let cond = {
        let sf = {
            let mut b = FirBuilder::new(store);
            b.int32(size_i32)
        };
        let mut b = FirBuilder::new(store);
        b.binop(FirBinOp::Ge, raw, sf, FirType::Int32)
    };
    let adjusted = {
        let sf = {
            let mut b = FirBuilder::new(store);
            b.int32(size_i32)
        };
        let mut b = FirBuilder::new(store);
        b.binop(FirBinOp::Sub, raw, sf, FirType::Int32)
    };
    let mut b = FirBuilder::new(store);
    b.select2(cond, adjusted, raw, FirType::Int32)
}

/// Emits `counter = (counter + 1 >= size) ? 0 : counter + 1` for an
/// `IfWrapping` delay line counter advance.
fn bump_if_wrapping_counter(store: &mut FirStore, counter_name: &str, size: usize) -> FirId {
    let size_i32 = i32::try_from(size).unwrap_or(i32::MAX);
    let counter = {
        let mut b = FirBuilder::new(store);
        b.load_var(counter_name, AccessType::Struct, FirType::Int32)
    };
    let one = {
        let mut b = FirBuilder::new(store);
        b.int32(1)
    };
    let next = {
        let mut b = FirBuilder::new(store);
        b.binop(FirBinOp::Add, counter, one, FirType::Int32)
    };
    let cond = {
        let sf = {
            let mut b = FirBuilder::new(store);
            b.int32(size_i32)
        };
        let mut b = FirBuilder::new(store);
        b.binop(FirBinOp::Ge, next, sf, FirType::Int32)
    };
    let zero = {
        let mut b = FirBuilder::new(store);
        b.int32(0)
    };
    let wrapped = {
        let mut b = FirBuilder::new(store);
        b.select2(cond, zero, next, FirType::Int32)
    };
    let mut b = FirBuilder::new(store);
    b.store_var(counter_name, AccessType::Struct, wrapped)
}

// ─── DelayManager ─────────────────────────────────────────────────────────────

/// Owns all delay-line exclusive state and provides scan + allocation entry points.
///
/// Four fields form the delay manager's state:
///
/// | Field | Type | Role |
/// |-------|------|------|
/// | `options` | [`DelayOptions`] | `-mcd` / `-dlt` strategy thresholds |
/// | `delay_lines` | `HashMap<SigId, DelayLineInfo>` | Allocated buffers, keyed by carried signal |
/// | `rec_output_analysis` | `HashMap<(u32, usize), DelayAnalysisEntry>` | Read-only accumulated delay metadata per recursion output |
/// | `scheduled_delay_writes` | `HashSet<SigId>` | Dedup guard for per-sample delay writes |
///
/// # Scan / allocation flow
///
/// 1. `SignalToFirLower::prepare_delay_lines` first calls
///    [`Self::analyze_signals`] to collect read-only accumulated delay metadata
///    for recursion outputs.
/// 2. `prepare_delay_lines` then calls [`Self::scan_signals`], which returns a
///    `max_delays` map of carried signals → their maximum observed owned delay.
/// 3. `prepare_delay_lines` allocates each owned delay line through
///    [`Self::ensure_delay_line`] using a [`DelayFirCtx`].
/// 4. During lowering, `module.rs` keeps orchestration and recursion-specific
///    cases, but delegates strategy-local FIR emission to
///    [`emit_fixed_delay_for_line`] / [`emit_delay1_for_line`].
/// 5. `ensure_recursion_array_for_group` consumes the read-only accumulated
///    recursion-output analysis to size recursion arrays that also serve as
///    merged delay buffers.
pub(super) struct DelayManager {
    /// Strategy selection thresholds (`-mcd` / `-dlt` options).
    options: DelayOptions,
    /// Allocated delay buffers, keyed by carried-signal id.
    delay_lines: HashMap<SigId, DelayLineInfo>,
    /// Read-only accumulated delay metadata keyed by `(rec_var_id, proj_index)`.
    rec_output_analysis: HashMap<(u32, usize), DelayAnalysisEntry>,
    /// Dedup guard: ensures the delay-write store for a given carried signal is
    /// emitted at most once per sample, even when the same signal is used by
    /// multiple `SIGDELAY` reads.
    scheduled_delay_writes: HashSet<SigId>,
}

impl DelayManager {
    /// Creates a fresh `DelayManager` for one module compilation.
    pub(super) fn new(options: DelayOptions) -> Self {
        Self {
            options,
            delay_lines: HashMap::new(),
            rec_output_analysis: HashMap::new(),
            scheduled_delay_writes: HashSet::new(),
        }
    }

    /// Returns the configured maximum copy-shift delay threshold.
    pub(super) fn max_copy_delay(&self) -> u32 {
        self.options.max_copy_delay
    }

    // ── Scan pass ────────────────────────────────────────────────────────────

    /// Computes read-only accumulated delay metadata for the reachable signal forest.
    ///
    /// This is the first Rust-side equivalent of the C++ delay-analysis pass:
    /// it walks the prepared signal DAG with an accumulated delay counter and
    /// records the maximum delayed access observed on each reachable carrier.
    ///
    /// The traversal rules are intentionally narrow:
    ///
    /// - `SIGDELAY(value, amount)` adds the proven upper bound of `amount`
    /// - `SIGDELAY1(value)` adds `1`
    /// - `SIGPREFIX(init, value)` adds `1` on the carried state edge only
    /// - non-delay computation nodes reset the accumulator to `0`
    ///
    /// Recursion outputs are tracked by their canonical `(var_id, proj_index)`
    /// identity so later planning steps can size the owning recursion carrier
    /// directly.
    pub(super) fn analyze_signals(
        &mut self,
        arena: &TreeArena,
        sig_types: &HashMap<SigId, SigType>,
        signals: &[SigId],
    ) -> Result<(), SignalFirError> {
        self.rec_output_analysis.clear();
        let mut best_seen_delay = HashMap::new();
        for &sig in signals {
            self.analyze_node(sig, 0, arena, sig_types, &mut best_seen_delay)?;
        }
        Ok(())
    }

    /// Pre-scans `signals` to collect the maximum delay per carried signal and
    /// record recursion-feedback-through-delay1 merge patterns.
    ///
    /// Returns a map `carried_signal → max_delay` for all delay buffers that must
    /// be pre-allocated before lowering:
    ///
    /// - general `SIGDELAY(value, amount)` lines keyed by `value`,
    /// - standalone `Delay1(value)` lines keyed by `value` when the shift
    ///   strategy is enabled (`max_copy_delay >= 1`).
    ///
    /// Entries where the pattern was merged into a recursion array are NOT
    /// included (the recursion array is now sized from the read-only
    /// accumulated delay analysis).
    ///
    /// This method has no FIR side-effects — it only reads `arena` and `sig_types`
    /// and returns the discovered per-carrier maximum delays.
    pub(super) fn scan_signals(
        &mut self,
        arena: &TreeArena,
        sig_types: &HashMap<SigId, SigType>,
        signals: &[SigId],
    ) -> Result<HashMap<SigId, i32>, SignalFirError> {
        let mut max_delays: HashMap<SigId, i32> = HashMap::new();
        let mut seen: HashSet<SigId> = HashSet::new();
        for sig in signals {
            self.scan_node(*sig, arena, sig_types, &mut seen, &mut max_delays)?;
        }
        Ok(max_delays)
    }

    fn analyze_node(
        &mut self,
        sig: SigId,
        accumulated_delay: i32,
        arena: &TreeArena,
        sig_types: &HashMap<SigId, SigType>,
        best_seen_delay: &mut HashMap<SigId, i32>,
    ) -> Result<(), SignalFirError> {
        if let Some(prev) = best_seen_delay.get(&sig)
            && *prev >= accumulated_delay
        {
            return Ok(());
        }
        best_seen_delay.insert(sig, accumulated_delay);

        if accumulated_delay > 0 {
            self.record_rec_output_delay_analysis(arena, sig, accumulated_delay);
        }

        match match_sig(arena, sig) {
            SigMatch::Delay(value, amount) => {
                let Some(delay) = delay_size_for_amount(arena, sig_types, amount)? else {
                    return Err(SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "SIGDELAY requires a constant integer amount or a signal with a bounded non-negative interval",
                    ));
                };
                self.analyze_node(
                    value,
                    accumulated_delay.saturating_add(delay),
                    arena,
                    sig_types,
                    best_seen_delay,
                )?;
                self.analyze_node(amount, 0, arena, sig_types, best_seen_delay)?;
                return Ok(());
            }
            SigMatch::Delay1(value) => {
                self.analyze_node(
                    value,
                    accumulated_delay.saturating_add(1),
                    arena,
                    sig_types,
                    best_seen_delay,
                )?;
                return Ok(());
            }
            SigMatch::Prefix(init, value) => {
                self.analyze_node(
                    value,
                    accumulated_delay.saturating_add(1),
                    arena,
                    sig_types,
                    best_seen_delay,
                )?;
                self.analyze_node(init, 0, arena, sig_types, best_seen_delay)?;
                return Ok(());
            }
            SigMatch::Proj(_, group) => {
                if let Some((_var, body_list)) = match_sym_rec(arena, group) {
                    let bodies = list_to_vec(arena, body_list).ok_or_else(|| {
                        SignalFirError::new(
                            SignalFirErrorCode::UnsupportedSignalNode,
                            "malformed symbolic recursion body list during delay analysis",
                        )
                    })?;
                    for body in bodies {
                        self.analyze_node(body, 0, arena, sig_types, best_seen_delay)?;
                    }
                    return Ok(());
                }
            }
            _ => {}
        }

        let node = arena.node(sig).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("missing prepared signal node {}", sig.as_u32()),
            )
        })?;
        for child in node.children.as_slice() {
            self.analyze_child(*child, arena, sig_types, best_seen_delay)?;
        }
        Ok(())
    }

    fn analyze_child(
        &mut self,
        child: SigId,
        arena: &TreeArena,
        sig_types: &HashMap<SigId, SigType>,
        best_seen_delay: &mut HashMap<SigId, i32>,
    ) -> Result<(), SignalFirError> {
        if arena.is_list(child) {
            let mut list = child;
            while !arena.is_nil(list) {
                let head = arena.hd(list).ok_or_else(|| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "malformed prepared signal list during delay analysis",
                    )
                })?;
                self.analyze_node(head, 0, arena, sig_types, best_seen_delay)?;
                list = arena.tl(list).ok_or_else(|| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "malformed prepared signal list during delay analysis",
                    )
                })?;
            }
            Ok(())
        } else {
            self.analyze_node(child, 0, arena, sig_types, best_seen_delay)
        }
    }

    fn record_rec_output_delay_analysis(
        &mut self,
        arena: &TreeArena,
        sig: SigId,
        accumulated_delay: i32,
    ) {
        let SigMatch::Proj(index, group) = match_sig(arena, sig) else {
            return;
        };
        let rec_var = match match_sym_ref(arena, group) {
            Some(var) => Some(var),
            None => match_sym_rec(arena, group).map(|(var, _)| var),
        };
        let Some(var) = rec_var else {
            return;
        };
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let entry = self
            .rec_output_analysis
            .entry((var.as_u32(), index))
            .or_default();
        entry.max_delay = entry.max_delay.max(accumulated_delay);
        entry.delay_count = entry.delay_count.saturating_add(1);
    }

    fn scan_node(
        &mut self,
        sig: SigId,
        arena: &TreeArena,
        sig_types: &HashMap<SigId, SigType>,
        seen: &mut HashSet<SigId>,
        max_delays: &mut HashMap<SigId, i32>,
    ) -> Result<(), SignalFirError> {
        if !seen.insert(sig) {
            return Ok(());
        }
        if let SigMatch::Delay(value, amount) = match_sig(arena, sig) {
            match delay_size_for_amount(arena, sig_types, amount)? {
                Some(0) => {}
                Some(delay) => {
                    let merged = self.is_recursion_delay_chain(arena, value);
                    if !merged {
                        let entry = max_delays.entry(value).or_insert(0);
                        if delay > *entry {
                            *entry = delay;
                        }
                    }
                }
                None => {
                    return Err(SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "SIGDELAY requires a constant integer amount or a signal with a bounded non-negative interval",
                    ));
                }
            }
        }
        if let SigMatch::Delay1(value) = match_sig(arena, sig)
            && self.options.max_copy_delay >= 1
            && !self.is_recursion_delay_chain(arena, value)
        {
            let entry = max_delays.entry(value).or_insert(0);
            if 1 > *entry {
                *entry = 1;
            }
        }
        let node = arena.node(sig).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("missing prepared signal node {}", sig.as_u32()),
            )
        })?;
        for child in node.children.as_slice() {
            self.scan_child(*child, arena, sig_types, seen, max_delays)?;
        }
        Ok(())
    }

    fn scan_child(
        &mut self,
        child: SigId,
        arena: &TreeArena,
        sig_types: &HashMap<SigId, SigType>,
        seen: &mut HashSet<SigId>,
        max_delays: &mut HashMap<SigId, i32>,
    ) -> Result<(), SignalFirError> {
        if arena.is_list(child) {
            let mut list = child;
            while !arena.is_nil(list) {
                let head = arena.hd(list).ok_or_else(|| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "malformed prepared signal list while scanning delay lines",
                    )
                })?;
                self.scan_node(head, arena, sig_types, seen, max_delays)?;
                list = arena.tl(list).ok_or_else(|| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "malformed prepared signal list while scanning delay lines",
                    )
                })?;
            }
            Ok(())
        } else {
            self.scan_node(child, arena, sig_types, seen, max_delays)
        }
    }

    fn is_recursion_delay_chain(&self, arena: &TreeArena, value: SigId) -> bool {
        self.unwrap_recursion_delay_chain(arena, value).is_some()
    }

    fn unwrap_recursion_delay_chain(
        &self,
        arena: &TreeArena,
        value: SigId,
    ) -> Option<(SigId, i32)> {
        let mut current = value;
        let mut implicit_delay = 0i32;
        while let SigMatch::Delay1(inner) = match_sig(arena, current) {
            implicit_delay = implicit_delay.saturating_add(1);
            current = inner;
        }
        let SigMatch::Proj(_, group) = match_sig(arena, current) else {
            return None;
        };
        match match_sym_ref(arena, group) {
            Some(_) => Some((current, implicit_delay)),
            None => match_sym_rec(arena, group).map(|_| (current, implicit_delay)),
        }
    }

    // ── Allocation ───────────────────────────────────────────────────────────

    /// Declares the struct array for one delay line, idempotent.
    ///
    /// Selects a [`DelayStrategy`] based on `delay` and [`DelayOptions`]:
    ///
    /// - `delay < max_copy_delay` → [`DelayStrategy::Shift`] (exact size, no fIOTA)
    /// - `max_copy_delay ≤ delay < delay_line_threshold` → [`DelayStrategy::CircularPow2`]
    ///   (power-of-two size, fIOTA declared via `ctx`)
    /// - `delay ≥ delay_line_threshold` → [`DelayStrategy::IfWrapping`] (exact size,
    ///   per-line `fIdx<id>` counter declared via `ctx`)
    ///
    /// On first call for `carried`, emits the struct declaration and registers an
    /// `instanceClear` zeroing loop via `ctx`.  Subsequent calls for the same
    /// `carried` return the cached info; an error is returned if the cached size is
    /// smaller than what the current delay requires.
    pub(super) fn ensure_delay_line(
        &mut self,
        carried: SigId,
        delay: i32,
        ctx: &mut DelayFirCtx<'_>,
    ) -> Result<DelayLineInfo, SignalFirError> {
        if delay < 0 {
            return Err(SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("SIGDELAY amount must be >= 0, got {delay}"),
            ));
        }
        // Select strategy based on delay amount and options.
        let delay_u = delay as u32;
        let strategy = if delay_u < self.options.max_copy_delay {
            DelayStrategy::Shift
        } else if delay_u < self.options.delay_line_threshold {
            DelayStrategy::CircularPow2
        } else {
            DelayStrategy::IfWrapping {
                counter_name: format!("fIdx{}", carried.as_u32()),
            }
        };

        // Compute required buffer size.
        let required_size = match &strategy {
            DelayStrategy::Shift => {
                usize::try_from(delay).map_err(|_| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        format!("SIGDELAY amount overflow: {delay}"),
                    )
                })? + 1
            }
            DelayStrategy::CircularPow2 => CircularPow2Model.buffer_size(delay)?,
            DelayStrategy::IfWrapping { .. } => IfWrappingModel.buffer_size(delay)?,
        };

        let elem_type = ctx.signal_elem_type(carried)?;

        if let Some(existing) = self.delay_lines.get(&carried) {
            if existing.size < required_size {
                return Err(SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!(
                        "internal fast-lane delay-line sizing mismatch for signal {}: \
                         existing size {} < required {}",
                        carried.as_u32(),
                        existing.size,
                        required_size
                    ),
                ));
            }
            return Ok(existing.clone());
        }

        // Strategy-specific ancillary declarations.
        match &strategy {
            DelayStrategy::CircularPow2 => ctx.ensure_iota(),
            DelayStrategy::IfWrapping { counter_name } => {
                ctx.ensure_if_wrapping_counter(counter_name.clone());
            }
            DelayStrategy::Shift => {}
        }

        let prefix = if elem_type == FirType::Int32 {
            "iVec"
        } else {
            "fVec"
        };
        let name = format!("{prefix}{}", carried.as_u32());
        let array_ty = FirType::Array(Box::new(elem_type), required_size);
        let decl = {
            let mut b = FirBuilder::new(ctx.store);
            b.declare_var(name.clone(), array_ty, AccessType::Struct, None)
        };
        ctx.struct_declarations.push(decl);
        ctx.register_delay_clear(name.clone(), required_size, carried)?;
        let info = DelayLineInfo {
            name,
            size: required_size,
            strategy,
        };
        self.delay_lines.insert(carried, info.clone());
        Ok(info)
    }

    /// Emits all generic delay-subsystem end-of-sample updates.
    ///
    /// This centralizes the runtime maintenance required by delay strategies
    /// and by the shared global circular cursor:
    ///
    /// - advance the shared `fIOTA` counter when any circular-pow2 line exists
    /// - advance every per-line `IfWrapping` counter
    pub(super) fn emit_sample_end_updates(
        &self,
        store: &mut FirStore,
        uses_iota: bool,
    ) -> Vec<FirId> {
        let mut updates = Vec::new();
        if uses_iota {
            updates.push(GlobalCircularCursor.emit_advance(store));
        }
        updates.extend(self.delay_lines.values().filter_map(|info| {
            if let DelayStrategy::IfWrapping { counter_name } = &info.strategy {
                Some(emit_if_wrapping_advance(store, counter_name, info.size))
            } else {
                None
            }
        }));
        updates
    }

    // ── Query / dedup accessors ───────────────────────────────────────────────

    /// Schedules the delay write for `carried` if not yet scheduled.
    ///
    /// Returns `true` on the first call for a given `carried` (the write store
    /// should be emitted); `false` on subsequent calls (dedup — write already
    /// scheduled earlier in this sample).
    pub(super) fn schedule_delay_write(&mut self, carried: SigId) -> bool {
        self.scheduled_delay_writes.insert(carried)
    }

    /// Returns the allocated delay line for `carried`, if any.
    pub(super) fn get_delay_line(&self, carried: SigId) -> Option<&DelayLineInfo> {
        self.delay_lines.get(&carried)
    }

    /// Returns read-only delay-analysis metadata for one recursion output.
    pub(super) fn rec_output_analysis(
        &self,
        var_id: u32,
        index: usize,
    ) -> Option<&DelayAnalysisEntry> {
        self.rec_output_analysis.get(&(var_id, index))
    }
}
