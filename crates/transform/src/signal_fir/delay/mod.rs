//! Circular delay-line sizing, geometry, and state management for the FIR fast-lane.
//!
//! Faust's `@(n)` operator maps to one of three delay-line strategies, selected
//! by the delay amount relative to two thresholds (`-mcd` / `-dlt`):
//!
//! ```text
//! [1, max_copy_delay)                    → Shift         (shift/copy; no fIOTA)
//! [max_copy_delay, delay_line_threshold) → CircularPow2  (shared fIOTA + mask)
//! [delay_line_threshold, ∞)              → IfWrapping    (per-line counter)
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
//! 4. [`DelayKind`]: the single cohesive strategy enum — carries its own buffer
//!    sizing, fixed-delay emission, and single-sample emission.  Replaces the
//!    former `RingDelayModel` / `DelayStrategyEmitter` trait split.
//! 5. [`DelayLoweringCtx`]: borrow bundle for lowering-time FIR emission; the
//!    two public thin wrappers [`emit_fixed_delay_for_line`] /
//!    [`emit_delay1_for_line`] delegate to `DelayKind` methods.
//!
//! # Strategy descriptions
//!
//! ## Shift/copy (`DelayKind::Shift`, delays ≤ `-mcd`, default 16)
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
//! ## Power-of-two circular (`DelayKind::CircularPow2`, default middle range)
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
//! ## If-based wrapping (`DelayKind::IfWrapping`, delays > `-dlt`)
//!
//! Uses an exact-size buffer (size = `max_delay + 1`) with a dedicated
//! per-line integer counter.  The counter wraps to zero via an `if` comparison
//! instead of a bitmask, saving memory for non-power-of-two delay sizes at the
//! cost of a branch per write.
//!
//! ```text
//! buf[idx] = current_value;
//! read:  buf[(idx + size - N) select2-wrapped]
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
//! The planning split is:
//!
//! - [`plan_delays`] produces a [`DelayPlan`] with both per-carrier max delays
//!   and recursion-output sizing metadata in one DAG walk.
//! - `prepare_delay_lines` allocates each line through [`DelayManager::ensure_delay_line`].
//! - `ensure_recursion_array_for_group` in `module.rs` consumes the recursion-output
//!   analysis to size recursion carriers.
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
//! | [`DelayKind`] | enum | Strategy + all per-strategy FIR emission behaviour |
//! | [`DelayLineInfo`] | struct | Metadata for one allocated delay buffer |
//! | [`DelayManager`] | struct | Owns delay-exclusive state; scan + alloc methods |
//! | [`DelayFirCtx`] | struct | Borrow bundle for FIR-emitting methods |
//! | [`GlobalCircularCursor`] | struct | Shared `fIOTA` cursor service for circular storage |
//! | [`plan_delays`] | free fn | Unified single-pass delay planner |
//! | [`DelayPlan`] | struct | Pure-data delay decision; input to allocation |
//!
//! The complementary **stateful orchestration** layer (`lower_fixed_delay`,
//! `lower_delay_state`, recursion-carrier resolution, and `lower_signal`
//! dispatch) remains in `module.rs`.
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

// ─── DelayLineInfo ────────────────────────────────────────────────────────────

/// Metadata for one allocated delay-line DSP-struct array.
///
/// The array is named `fVec<id>` (real) or `iVec<id>` (integer), declared
/// during `prepare_delay_lines` and zeroed in `instanceClear`.
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
    pub(super) strategy: DelayKind,
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

// ─── DelayKind ────────────────────────────────────────────────────────────────

/// Buffer-geometry strategy for a single allocated delay line, with all
/// per-strategy FIR emission behaviour attached.
///
/// This is the single source of truth for each strategy: buffer sizing, fixed
/// delay reads/writes, single-sample reads/writes, and end-of-sample advances
/// all live here.  There are no shared cross-strategy enums or impossible
/// branches — each arm only ever touches the state it owns.
///
/// | Variant | Buffer | Pointer |
/// |---------|--------|---------|
/// | `Shift` | exact `N+1` | none (shift loop) |
/// | `CircularPow2` | `next_pow2(N+1)` | shared `fIOTA`, masked |
/// | `IfWrapping` | exact `N+1` | per-line `fIdx*`, if-wrap |
#[derive(Clone, Debug)]
pub(super) enum DelayKind {
    /// Shift/copy: buffer shifted one slot per sample; new value at index 0;
    /// read at index = delay amount.  No `fIOTA`.  Buffer size = `delay + 1`.
    Shift,
    /// Power-of-two circular buffer driven by the shared `fIOTA` counter.
    /// Buffer size = `next_power_of_two(delay + 1)`.
    CircularPow2,
    /// Per-line if-based wrapping counter; exact buffer size (`delay + 1`).
    /// Each line has its own `fIdx<sig_id>` struct variable.
    IfWrapping {
        /// Name of the per-line counter variable, e.g. `fIdx42`.
        counter_name: String,
    },
}

impl DelayKind {
    // ── Sizing ────────────────────────────────────────────────────────────────

    /// Minimum buffer size for a maximum delay of `max_delay` samples.
    pub(super) fn buffer_size(&self, max_delay: i32) -> Result<usize, SignalFirError> {
        match self {
            DelayKind::Shift => usize::try_from(max_delay).map(|d| d + 1).map_err(|_| {
                SignalFirError::new(
                    SignalFirErrorCode::UnsupportedSignalNode,
                    format!("SIGDELAY amount overflow: {max_delay}"),
                )
            }),
            DelayKind::CircularPow2 => pow2limit_for_delay(max_delay),
            DelayKind::IfWrapping { .. } => {
                usize::try_from(max_delay).map(|d| d + 1).map_err(|_| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        format!("SIGDELAY amount overflow: {max_delay}"),
                    )
                })
            }
        }
    }

    // ── Lowering ──────────────────────────────────────────────────────────────

    /// Emits one `SIGDELAY(value, amount)` read/write sequence.
    ///
    /// When `schedule_write` is true, emits the store before the read and
    /// schedules any shift copies / advances.
    pub(super) fn emit_fixed_delay(
        &self,
        ctx: &mut DelayLoweringCtx<'_>,
        line: &DelayLineInfo,
        current: FirId,
        amount: FirId,
        read_ty: FirType,
        schedule_write: bool,
    ) -> FirId {
        match self {
            // ── Shift ──────────────────────────────────────────────────────
            DelayKind::Shift => {
                if schedule_write {
                    let store_0 = emit_store_at_zero(ctx.store, &line.name, current);
                    ctx.immediate_statements.push(store_0);
                    let delay_n = i32::try_from(line.size).unwrap_or(i32::MAX) - 1;
                    if delay_n <= 2 {
                        let copies = emit_unrolled_shift_copies(
                            ctx.store,
                            &line.name,
                            delay_n,
                            read_ty.clone(),
                        );
                        ctx.post_output_statements.extend(copies);
                    } else {
                        let shift = emit_shift_loop(ctx, &line.name, delay_n, read_ty.clone());
                        ctx.post_output_statements.push(shift);
                    }
                }
                let mut b = FirBuilder::new(ctx.store);
                b.load_table(line.name.clone(), AccessType::Struct, amount, read_ty)
            }

            // ── CircularPow2 ───────────────────────────────────────────────
            DelayKind::CircularPow2 => {
                if schedule_write {
                    let write_index = GlobalCircularCursor.current_index(ctx.store, line.size);
                    let mut b = FirBuilder::new(ctx.store);
                    ctx.immediate_statements.push(b.store_table(
                        line.name.clone(),
                        AccessType::Struct,
                        write_index,
                        current,
                    ));
                }
                let read_index = GlobalCircularCursor.delayed_index(ctx.store, amount, line.size);
                let mut b = FirBuilder::new(ctx.store);
                b.load_table(line.name.clone(), AccessType::Struct, read_index, read_ty)
            }

            // ── IfWrapping ─────────────────────────────────────────────────
            DelayKind::IfWrapping { counter_name } => {
                if schedule_write {
                    let write_index = {
                        let mut b = FirBuilder::new(ctx.store);
                        b.load_var(counter_name, AccessType::Struct, FirType::Int32)
                    };
                    let mut b = FirBuilder::new(ctx.store);
                    ctx.immediate_statements.push(b.store_table(
                        line.name.clone(),
                        AccessType::Struct,
                        write_index,
                        current,
                    ));
                }
                let read_index = if_wrapping_read_index(ctx.store, counter_name, amount, line.size);
                let mut b = FirBuilder::new(ctx.store);
                b.load_table(line.name.clone(), AccessType::Struct, read_index, read_ty)
            }
        }
    }

    /// Emits one `Delay1(value)` read/write sequence (amount = 1 sample).
    pub(super) fn emit_delay1(
        &self,
        ctx: &mut DelayLoweringCtx<'_>,
        line: &DelayLineInfo,
        current: FirId,
        read_ty: FirType,
        schedule_write: bool,
    ) -> FirId {
        match self {
            // Shift: read from index 1 (same write path as fixed delay).
            DelayKind::Shift => {
                if schedule_write {
                    let store_0 = emit_store_at_zero(ctx.store, &line.name, current);
                    ctx.immediate_statements.push(store_0);
                    let delay_n = i32::try_from(line.size).unwrap_or(i32::MAX) - 1;
                    if delay_n <= 2 {
                        let copies = emit_unrolled_shift_copies(
                            ctx.store,
                            &line.name,
                            delay_n,
                            read_ty.clone(),
                        );
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
            // Ring strategies: Delay1 is fixed_delay with amount = 1.
            DelayKind::CircularPow2 | DelayKind::IfWrapping { .. } => {
                let one = {
                    let mut b = FirBuilder::new(ctx.store);
                    b.int32(1)
                };
                self.emit_fixed_delay(ctx, line, current, one, read_ty, schedule_write)
            }
        }
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

// ─── Delay lowering context ──────────────────────────────────────────────────

/// Borrow bundle for strategy-local FIR emission during lowering.
pub(super) struct DelayLoweringCtx<'a> {
    pub(super) store: &'a mut FirStore,
    pub(super) immediate_statements: &'a mut Vec<FirId>,
    pub(super) post_output_statements: &'a mut Vec<FirId>,
    pub(super) next_loop_var_id: &'a mut usize,
}

/// Thin wrapper: emits one `SIGDELAY(value, amount)` sequence.
///
/// Delegates immediately to [`DelayKind::emit_fixed_delay`]; callers in
/// `module/` are unchanged.
pub(super) fn emit_fixed_delay_for_line(
    ctx: &mut DelayLoweringCtx<'_>,
    line: &DelayLineInfo,
    current: FirId,
    amount: FirId,
    read_ty: FirType,
    schedule_write: bool,
) -> FirId {
    line.strategy
        .emit_fixed_delay(ctx, line, current, amount, read_ty, schedule_write)
}

/// Thin wrapper: emits one `Delay1(value)` sequence.
///
/// Delegates immediately to [`DelayKind::emit_delay1`]; callers in
/// `module/` are unchanged.
pub(super) fn emit_delay1_for_line(
    ctx: &mut DelayLoweringCtx<'_>,
    line: &DelayLineInfo,
    current: FirId,
    read_ty: FirType,
    schedule_write: bool,
) -> FirId {
    line.strategy
        .emit_delay1(ctx, line, current, read_ty, schedule_write)
}

/// Emits the end-of-sample counter advance for one `IfWrapping` delay line.
fn emit_if_wrapping_advance(store: &mut FirStore, counter_name: &str, size: usize) -> FirId {
    bump_if_wrapping_counter(store, counter_name, size)
}

// ─── FIR emission helpers shared with strategy emitters ─────────────────────

/// Thin arithmetic layer so index-formula functions read like the doc-comments.
///
/// Each method creates one FIR node via `FirBuilder` and returns its `FirId`.
/// The emitted nodes are byte-for-byte identical to what raw `FirBuilder` calls
/// produce — this is a legibility-only wrapper, not an optimization.
struct DelayArith<'a>(&'a mut FirStore);

impl<'a> DelayArith<'a> {
    fn i32c(&mut self, v: i32) -> FirId {
        FirBuilder::new(self.0).int32(v)
    }
    fn load_counter(&mut self, name: &str) -> FirId {
        FirBuilder::new(self.0).load_var(name, AccessType::Struct, FirType::Int32)
    }
    fn add(&mut self, a: FirId, b: FirId) -> FirId {
        FirBuilder::new(self.0).binop(FirBinOp::Add, a, b, FirType::Int32)
    }
    fn sub(&mut self, a: FirId, b: FirId) -> FirId {
        FirBuilder::new(self.0).binop(FirBinOp::Sub, a, b, FirType::Int32)
    }
    fn ge(&mut self, a: FirId, b: FirId) -> FirId {
        FirBuilder::new(self.0).binop(FirBinOp::Ge, a, b, FirType::Int32)
    }
    fn and_mask(&mut self, idx: FirId, mask: FirId) -> FirId {
        FirBuilder::new(self.0).binop(FirBinOp::And, idx, mask, FirType::Int32)
    }
    fn select2(&mut self, cond: FirId, then_: FirId, else_: FirId) -> FirId {
        FirBuilder::new(self.0).select2(cond, then_, else_, FirType::Int32)
    }
    fn store_counter(&mut self, name: &str, val: FirId) -> FirId {
        FirBuilder::new(self.0).store_var(name, AccessType::Struct, val)
    }
}

/// Applies the power-of-two ring-buffer mask: `index & (size - 1)`.
pub(super) fn masked_delay_index(store: &mut FirStore, index: FirId, size: usize) -> FirId {
    let mut e = DelayArith(store);
    let mask = e.i32c(i32::try_from(size.saturating_sub(1)).unwrap_or(i32::MAX));
    e.and_mask(index, mask)
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
    let mut e = DelayArith(store);
    let counter = e.load_counter(counter_name);
    let size_fir = e.i32c(size_i32);
    let plus_size = e.add(counter, size_fir);
    let raw = e.sub(plus_size, amount);
    let size_fir2 = e.i32c(size_i32);
    let cond = e.ge(raw, size_fir2);
    let size_fir3 = e.i32c(size_i32);
    let adjusted = e.sub(raw, size_fir3);
    e.select2(cond, adjusted, raw)
}

/// Emits `counter = (counter + 1 >= size) ? 0 : counter + 1` for an
/// `IfWrapping` delay line counter advance.
fn bump_if_wrapping_counter(store: &mut FirStore, counter_name: &str, size: usize) -> FirId {
    let size_i32 = i32::try_from(size).unwrap_or(i32::MAX);
    let mut e = DelayArith(store);
    let counter = e.load_counter(counter_name);
    let one = e.i32c(1);
    let next = e.add(counter, one);
    let size_fir = e.i32c(size_i32);
    let cond = e.ge(next, size_fir);
    let zero = e.i32c(0);
    let wrapped = e.select2(cond, zero, next);
    e.store_counter(counter_name, wrapped)
}

// ─── DelayPlan ────────────────────────────────────────────────────────────────

/// The complete delay decision for one module, produced by a single DAG walk.
///
/// `DelayPlan` is a pure-data value with no FIR side-effects.  It collects
/// exactly the two maps that the two existing tree walks ([`DelayManager::analyze_signals`]
/// and [`DelayManager::scan_signals`]) build independently:
///
/// - `lines` — the per-carrier maximum owned delay (≡ the `max_delays` map
///   returned by `scan_signals`).
/// - `rec_outputs` — the recursion-output sizing metadata (≡ the
///   `rec_output_analysis` map filled by `analyze_signals`).
///
/// Produced by [`plan_delays`]; consumed by `prepare_delay_lines` and
/// `ensure_recursion_array_for_group`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DelayPlan {
    /// Standalone delay lines to allocate: carried signal → required max delay.
    ///
    /// Equivalent to `max_delays` returned by [`DelayManager::scan_signals`].
    pub(super) lines: HashMap<SigId, i32>,
    /// Recursion-output sizing metadata: `(rec_var_id, proj_index)` → entry.
    ///
    /// Equivalent to `DelayManager::rec_output_analysis` filled by
    /// [`DelayManager::analyze_signals`].
    pub(super) rec_outputs: HashMap<(u32, usize), DelayAnalysisEntry>,
}

// ─── plan_delays ──────────────────────────────────────────────────────────────

/// Unified single-pass replacement for `analyze_signals` + `scan_signals`.
///
/// Produces a [`DelayPlan`] containing BOTH maps in one traversal of the
/// prepared signal DAG.  Has no FIR side-effects.
///
/// # Algorithm
///
/// The pass runs the *accumulating* traversal from `analyze_signals` (tracking
/// path-accumulated delay, memoised by `best_seen_delay` so a node is re-visited
/// when reached with a strictly larger accumulated delay).  On the FIRST visit to
/// each node (tracked by `scanned: HashSet<SigId>`), it additionally performs the
/// *scan-style* recording of the per-carrier maximum owned delay (`plan.lines`),
/// using the same guards as `scan_signals`:
///
/// - zero-delay nodes are skipped,
/// - `!is_recursion_delay_chain` guard for both `Delay` and `Delay1`,
/// - `max_copy_delay >= 1` gate for `Delay1`.
///
/// This is correct because per-carrier max-delay recording does not depend on
/// the accumulated delay — it only depends on the delay amount at the `Delay`
/// node itself and on whether the carried value is a recursion chain.
pub(super) fn plan_delays(
    arena: &TreeArena,
    sig_types: &HashMap<SigId, SigType>,
    signals: &[SigId],
    options: &DelayOptions,
) -> Result<DelayPlan, SignalFirError> {
    DelayPlanner::new(arena, sig_types, options).run(signals)
}

/// Pure-function equivalent of `DelayManager::is_recursion_delay_chain` that
/// does not need `&self`.
fn is_recursion_delay_chain_static(arena: &TreeArena, value: SigId) -> bool {
    let mut current = value;
    while let SigMatch::Delay1(inner) = match_sig(arena, current) {
        current = inner;
    }
    let SigMatch::Proj(_, group) = match_sig(arena, current) else {
        return false;
    };
    match_sym_ref(arena, group).is_some() || match_sym_rec(arena, group).map(|_| ()).is_some()
}

// ─── DelayPlanner ─────────────────────────────────────────────────────────────

/// Single-pass visitor that builds a [`DelayPlan`] without threading 8
/// arguments through every recursive call.
///
/// The shared state (`arena`, `sig_types`, `options`, `plan`,
/// `best_seen_delay`, `scanned`) is held on the struct, so recursive calls
/// reduce to `self.node(sig, accum)` / `self.child(child)`.
struct DelayPlanner<'a> {
    arena: &'a TreeArena,
    sig_types: &'a HashMap<SigId, SigType>,
    options: &'a DelayOptions,
    plan: DelayPlan,
    best_seen_delay: HashMap<SigId, i32>,
    scanned: HashSet<SigId>,
}

impl<'a> DelayPlanner<'a> {
    fn new(
        arena: &'a TreeArena,
        sig_types: &'a HashMap<SigId, SigType>,
        options: &'a DelayOptions,
    ) -> Self {
        Self {
            arena,
            sig_types,
            options,
            plan: DelayPlan::default(),
            best_seen_delay: HashMap::new(),
            scanned: HashSet::new(),
        }
    }

    /// Entry point: walk every root signal and return the finished plan.
    fn run(mut self, signals: &[SigId]) -> Result<DelayPlan, SignalFirError> {
        for &sig in signals {
            self.node(sig, 0)?;
        }
        Ok(self.plan)
    }

    /// Core recursive visitor.
    ///
    /// Combines the accumulating logic of `analyze_node` (tracking
    /// `accumulated_delay` along paths through `Delay` / `Delay1` / `Prefix`)
    /// with the first-visit scan-recording logic of `scan_node`.
    fn node(&mut self, sig: SigId, accumulated_delay: i32) -> Result<(), SignalFirError> {
        // Accumulating-pass memoisation: skip if already visited with >= delay.
        if let Some(prev) = self.best_seen_delay.get(&sig)
            && *prev >= accumulated_delay
        {
            return Ok(());
        }
        self.best_seen_delay.insert(sig, accumulated_delay);

        // Accumulating pass: record rec-output analysis.
        if accumulated_delay > 0 {
            self.record_rec_output(sig, accumulated_delay);
        }

        // First-visit scan pass: record per-carrier max owned delay.
        if self.scanned.insert(sig) {
            self.scan_once(sig)?;
        }

        match match_sig(self.arena, sig) {
            SigMatch::Delay(value, amount) => {
                let Some(delay) = delay_size_for_amount(self.arena, self.sig_types, amount)? else {
                    return Err(SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "SIGDELAY requires a constant integer amount or a signal with a bounded non-negative interval",
                    ));
                };
                self.node(value, accumulated_delay.saturating_add(delay))?;
                self.node(amount, 0)?;
                return Ok(());
            }
            SigMatch::Delay1(value) => {
                self.node(value, accumulated_delay.saturating_add(1))?;
                return Ok(());
            }
            SigMatch::Prefix(init, value) => {
                self.node(value, accumulated_delay.saturating_add(1))?;
                self.node(init, 0)?;
                return Ok(());
            }
            SigMatch::Proj(_, group) => {
                if let Some((_var, body_list)) = match_sym_rec(self.arena, group) {
                    let bodies = list_to_vec(self.arena, body_list).ok_or_else(|| {
                        SignalFirError::new(
                            SignalFirErrorCode::UnsupportedSignalNode,
                            "malformed symbolic recursion body list during delay planning",
                        )
                    })?;
                    for body in bodies {
                        self.node(body, 0)?;
                    }
                    return Ok(());
                }
            }
            _ => {}
        }

        let node = self.arena.node(sig).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("missing prepared signal node {}", sig.as_u32()),
            )
        })?;
        let children: Vec<SigId> = node.children.as_slice().to_vec();
        for child in children {
            self.child(child)?;
        }
        Ok(())
    }

    /// Walks a child node, handling list children the same way as `analyze_child`
    /// and `scan_child`.
    fn child(&mut self, child: SigId) -> Result<(), SignalFirError> {
        if self.arena.is_list(child) {
            let mut list = child;
            while !self.arena.is_nil(list) {
                let head = self.arena.hd(list).ok_or_else(|| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "malformed prepared signal list during delay planning",
                    )
                })?;
                self.node(head, 0)?;
                list = self.arena.tl(list).ok_or_else(|| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "malformed prepared signal list during delay planning",
                    )
                })?;
            }
            Ok(())
        } else {
            self.node(child, 0)
        }
    }

    /// Records per-carrier scan information on the first visit to `sig`.
    ///
    /// Mirrors the body of `scan_node`, but operates on `plan.lines` instead of
    /// a local `max_delays` map.
    fn scan_once(&mut self, sig: SigId) -> Result<(), SignalFirError> {
        if let SigMatch::Delay(value, amount) = match_sig(self.arena, sig) {
            match delay_size_for_amount(self.arena, self.sig_types, amount)? {
                Some(0) => {}
                Some(delay) => {
                    if !is_recursion_delay_chain_static(self.arena, value) {
                        let entry = self.plan.lines.entry(value).or_insert(0);
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
        if let SigMatch::Delay1(value) = match_sig(self.arena, sig)
            && self.options.max_copy_delay >= 1
            && !is_recursion_delay_chain_static(self.arena, value)
        {
            let entry = self.plan.lines.entry(value).or_insert(0);
            if 1 > *entry {
                *entry = 1;
            }
        }
        Ok(())
    }

    /// Records recursion-output delay analysis for `sig` at `accumulated_delay`,
    /// mirroring `DelayManager::record_rec_output_delay_analysis`.
    fn record_rec_output(&mut self, sig: SigId, accumulated_delay: i32) {
        let SigMatch::Proj(index, group) = match_sig(self.arena, sig) else {
            return;
        };
        let rec_var = match match_sym_ref(self.arena, group) {
            Some(var) => Some(var),
            None => match_sym_rec(self.arena, group).map(|(var, _)| var),
        };
        let Some(var) = rec_var else {
            return;
        };
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let entry = self
            .plan
            .rec_outputs
            .entry((var.as_u32(), index))
            .or_default();
        entry.max_delay = entry.max_delay.max(accumulated_delay);
        entry.delay_count = entry.delay_count.saturating_add(1);
    }
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

    /// Returns a clone of the delay options.
    pub(super) fn options(&self) -> DelayOptions {
        self.options.clone()
    }

    // ── Allocation ───────────────────────────────────────────────────────────

    /// Declares the struct array for one delay line, idempotent.
    ///
    /// Selects a [`DelayKind`] based on `delay` and [`DelayOptions`]:
    ///
    /// - `delay < max_copy_delay` → [`DelayKind::Shift`] (exact size, no fIOTA)
    /// - `max_copy_delay ≤ delay < delay_line_threshold` → [`DelayKind::CircularPow2`]
    ///   (power-of-two size, fIOTA declared via `ctx`)
    /// - `delay ≥ delay_line_threshold` → [`DelayKind::IfWrapping`] (exact size,
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
            DelayKind::Shift
        } else if delay_u < self.options.delay_line_threshold {
            DelayKind::CircularPow2
        } else {
            DelayKind::IfWrapping {
                counter_name: format!("fIdx{}", carried.as_u32()),
            }
        };

        // Compute required buffer size via the unified DelayKind method.
        let required_size = strategy.buffer_size(delay)?;

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
            DelayKind::CircularPow2 => ctx.ensure_iota(),
            DelayKind::IfWrapping { counter_name } => {
                ctx.ensure_if_wrapping_counter(counter_name.clone());
            }
            DelayKind::Shift => {}
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
            if let DelayKind::IfWrapping { counter_name } = &info.strategy {
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

    /// Replaces the internal rec-output analysis map with the one from a [`DelayPlan`].
    ///
    /// Called by `prepare_delay_lines` after switching to the unified [`plan_delays`] walk.
    pub(super) fn set_rec_output_analysis(
        &mut self,
        rec_outputs: HashMap<(u32, usize), DelayAnalysisEntry>,
    ) {
        self.rec_output_analysis = rec_outputs;
    }
}
