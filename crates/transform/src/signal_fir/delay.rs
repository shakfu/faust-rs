//! Circular delay-line sizing, geometry, and state management for the FIR fast-lane.
//!
//! Faust's `@(n)` operator maps to one of three delay-line strategies, selected
//! by the delay amount relative to two thresholds (`-mcd` / `-dlt`):
//!
//! ```text
//! [0, max_copy_delay]               → ShiftModel   (shift/copy; no fIOTA)
//! (max_copy_delay, delay_line_threshold] → CircularPow2Model (default fIOTA + mask)
//! (delay_line_threshold, ∞)             → IfWrappingModel   (per-line counter)
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
//! 4. [`DelayLineModel`] + [`CircularPow2Model`]: buffer-geometry abstraction
//!    (retained for completeness; selection is via [`DelayStrategy`]).
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
//! When the pattern `SIGDELAY(Delay1(Proj(i, SYMREF(v))), N)` appears, the
//! recursion array for output `i` of variable `v` is sized to hold the full
//! chain (`N + 1` samples) so no separate `fVec` is needed.  The scan pass
//! ([`DelayManager::scan_signals`]) calls [`delay_size_for_amount`] and records
//! the merged size on `rec_group_max_delay`; `ensure_recursion_array_for_group`
//! in `module.rs` consumes it via [`DelayManager::rec_max_delay`].
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
//! | [`DelayLineModel`] | trait | Buffer geometry abstraction |
//! | [`CircularPow2Model`] | struct | Power-of-two implementation |
//!
//! The complementary **FIR materialization** layer for index expressions
//! (`current_iota_index`, `delayed_iota_index`, `masked_delay_index`,
//! `bump_iota`) and state-coupled methods (`lower_fixed_delay`,
//! `lower_delay_state`) lives in `module.rs` as `impl SignalToFirLower`
//! methods, because those methods need mutable access to the lowering engine's
//! full state including the recursive `lower_signal` dispatcher.
//!
//! Source provenance (C++):
//! - `compiler/transform/signalFIRCompiler.hh` — `DelayLine`, `allocateDelayLineAux`
//! - `compiler/transform/signalFIRCompiler.cpp` — `compileSigDelay`, `writeReadDelay`,
//!   `checkDelayInterval`

use std::collections::{HashMap, HashSet};

use fir::{AccessType, FirBinOp, FirBuilder, FirId, FirStore, FirType};
use signals::{SigId, SigMatch, match_sig};
use sigtype::{SigType, check_delay_interval};
use tlib::{TreeArena, match_sym_ref};

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

// ─── DelayLineModel ───────────────────────────────────────────────────────────

/// Abstraction over the buffer-geometry strategy used for circular delay lines.
///
/// The default implementation is [`CircularPow2Model`] (power-of-two size,
/// bitwise-AND masking), matching C++ Faust's `signalFIRCompiler`.  Alternative
/// models (exact-size modulo, segmented for very long delays) can be added by
/// implementing this trait.
///
/// All `FirId` arguments and return values are nodes in the shared [`FirStore`];
/// callers must ensure they pass the same store that was used when building those
/// nodes.
#[allow(dead_code)]
pub(super) trait DelayLineModel {
    /// Minimum buffer size in elements for a maximum delay of `max_delay` samples.
    fn buffer_size(&self, max_delay: i32) -> Result<usize, SignalFirError>;

    /// FIR expression: index of the current write slot.
    ///
    /// `iota` is a loaded `FirId` for `fIOTA`.
    fn write_index(&self, store: &mut FirStore, iota: FirId, size: usize) -> FirId;

    /// FIR expression: index of the slot that is `amount` samples behind the
    /// write pointer.
    fn read_index(
        &self,
        store: &mut FirStore,
        iota: FirId,
        amount: FirId,
        size: usize,
    ) -> FirId;

    /// FIR expression: `fIOTA + 1` (advance write pointer by one step).
    fn bump(&self, store: &mut FirStore, iota: FirId) -> FirId;
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

impl DelayLineModel for CircularPow2Model {
    fn buffer_size(&self, max_delay: i32) -> Result<usize, SignalFirError> {
        pow2limit_for_delay(max_delay)
    }

    fn write_index(&self, store: &mut FirStore, iota: FirId, size: usize) -> FirId {
        let mask =
            FirBuilder::new(store).int32(i32::try_from(size.saturating_sub(1)).unwrap_or(i32::MAX));
        FirBuilder::new(store).binop(FirBinOp::And, iota, mask, FirType::Int32)
    }

    fn read_index(
        &self,
        store: &mut FirStore,
        iota: FirId,
        amount: FirId,
        size: usize,
    ) -> FirId {
        let raw = FirBuilder::new(store).binop(FirBinOp::Sub, iota, amount, FirType::Int32);
        let mask =
            FirBuilder::new(store).int32(i32::try_from(size.saturating_sub(1)).unwrap_or(i32::MAX));
        FirBuilder::new(store).binop(FirBinOp::And, raw, mask, FirType::Int32)
    }

    fn bump(&self, store: &mut FirStore, iota: FirId) -> FirId {
        let one = FirBuilder::new(store).int32(1);
        FirBuilder::new(store).binop(FirBinOp::Add, iota, one, FirType::Int32)
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
        if *self.uses_iota {
            return;
        }
        *self.uses_iota = true;
        let zero = {
            let mut b = FirBuilder::new(self.store);
            b.int32(0)
        };
        let decl = {
            let mut b = FirBuilder::new(self.store);
            b.declare_var("fIOTA", FirType::Int32, AccessType::Struct, None)
        };
        self.struct_declarations.push(decl);
        if self.clear_init_seen.insert("fIOTA".to_owned()) {
            let mut b = FirBuilder::new(self.store);
            self.clear_statements
                .push(b.store_var("fIOTA", AccessType::Struct, zero));
        }
    }

    /// Generates a fresh loop-variable name using the shared monotonic counter.
    pub(super) fn fresh_loop_var(&mut self, prefix: &str) -> String {
        let name = format!("{prefix}{}", *self.next_loop_var_id);
        *self.next_loop_var_id += 1;
        name
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
            b.declare_var(counter_name.clone(), FirType::Int32, AccessType::Struct, None)
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
                    format!(
                        "cannot zero-init delay-line for signal {}",
                        sig.as_u32()
                    ),
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

// ─── DelayManager ─────────────────────────────────────────────────────────────

/// Owns all delay-line exclusive state and provides scan + allocation entry points.
///
/// Five fields form the delay manager's state:
///
/// | Field | Type | Role |
/// |-------|------|------|
/// | `options` | [`DelayOptions`] | `-mcd` / `-dlt` strategy thresholds |
/// | `delay_lines` | `HashMap<SigId, DelayLineInfo>` | Allocated buffers, keyed by carried signal |
/// | `rec_group_max_delay` | `HashMap<(u32, usize), i32>` | Max merged delay per recursion output |
/// | `scheduled_delay_writes` | `HashSet<SigId>` | Dedup guard for per-sample delay writes |
///
/// # Scan / allocation flow
///
/// 1. `SignalToFirLower::prepare_delay_lines` calls [`Self::scan_signals`], which
///    returns a `max_delays` map of carried signals → their maximum observed delay.
/// 2. `prepare_delay_lines` then calls `ensure_delay_line_decl` for each entry,
///    which in turn calls [`Self::ensure_delay_line`] with a [`DelayFirCtx`].
/// 3. During lowering, `lower_fixed_delay` dispatches on the [`DelayStrategy`]
///    stored in the returned [`DelayLineInfo`] to emit the correct write/read FIR.
/// 4. `ensure_recursion_array_for_group` calls [`Self::rec_max_delay`] to size
///    recursion arrays that serve as delay buffers.
pub(super) struct DelayManager {
    /// Strategy selection thresholds (`-mcd` / `-dlt` options).
    options: DelayOptions,
    /// Allocated delay buffers, keyed by carried-signal id.
    delay_lines: HashMap<SigId, DelayLineInfo>,
    /// Maximum total delay for each recursion output that uses the merged pattern
    /// `SIGDELAY(Delay1(Proj(i, SYMREF(var))), N)`.
    rec_group_max_delay: HashMap<(u32, usize), i32>,
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
            rec_group_max_delay: HashMap::new(),
            scheduled_delay_writes: HashSet::new(),
        }
    }

    // ── Scan pass ────────────────────────────────────────────────────────────

    /// Pre-scans `signals` to collect the maximum delay per carried signal and
    /// record recursion-feedback-through-delay1 merge patterns.
    ///
    /// Returns a map `carried_signal → max_delay` for all `SIGDELAY` nodes found
    /// in the forest.  Entries where the pattern was merged into a recursion array
    /// are NOT included (the recursion array handles them via [`Self::rec_max_delay`]).
    ///
    /// This method has no FIR side-effects — it only reads `arena` and `sig_types`
    /// and writes to `self.rec_group_max_delay`.
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
                    let merged = self.try_record_rec_delay(arena, value, delay);
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

    /// Detects `SIGDELAY(Delay1(Proj(i, SYMREF(var))), N)` and records the
    /// total delay `N + 1` on `rec_group_max_delay`.
    ///
    /// Returns `true` if the pattern matched (caller should skip `fVec` allocation
    /// for this carried signal); `false` otherwise.
    fn try_record_rec_delay(&mut self, arena: &TreeArena, value: SigId, delay: i32) -> bool {
        let SigMatch::Delay1(inner) = match_sig(arena, value) else {
            return false;
        };
        let SigMatch::Proj(proj_index, group) = match_sig(arena, inner) else {
            return false;
        };
        let Some(var) = match_sym_ref(arena, group) else {
            return false;
        };
        let total = delay + 1; // N (explicit) + 1 (Delay1)
        let key = (var.as_u32(), proj_index as usize);
        let entry = self.rec_group_max_delay.entry(key).or_insert(0);
        if total > *entry {
            *entry = total;
        }
        true
    }

    // ── Allocation ───────────────────────────────────────────────────────────

    /// Declares the struct array for one delay line, idempotent.
    ///
    /// Selects a [`DelayStrategy`] based on `delay` and [`DelayOptions`]:
    ///
    /// - `delay ≤ max_copy_delay` → [`DelayStrategy::Shift`] (exact size, no fIOTA)
    /// - `max_copy_delay < delay ≤ delay_line_threshold` → [`DelayStrategy::CircularPow2`]
    ///   (power-of-two size, fIOTA declared via `ctx`)
    /// - `delay > delay_line_threshold` → [`DelayStrategy::IfWrapping`] (exact size,
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
        let strategy = if delay_u <= self.options.max_copy_delay {
            DelayStrategy::Shift
        } else if delay_u <= self.options.delay_line_threshold {
            DelayStrategy::CircularPow2
        } else {
            DelayStrategy::IfWrapping {
                counter_name: format!("fIdx{}", carried.as_u32()),
            }
        };

        // Compute required buffer size.
        let required_size = match &strategy {
            DelayStrategy::Shift | DelayStrategy::IfWrapping { .. } => {
                usize::try_from(delay).map_err(|_| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        format!("SIGDELAY amount overflow: {delay}"),
                    )
                })? + 1
            }
            DelayStrategy::CircularPow2 => CircularPow2Model.buffer_size(delay)?,
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

    /// Returns `(counter_name, buffer_size)` pairs for all `IfWrapping` delay lines.
    ///
    /// Called by `build_module` after signal lowering to emit per-line counter
    /// advance statements at the end of the sample loop.
    pub(super) fn if_wrapping_lines(&self) -> impl Iterator<Item = (&str, usize)> {
        self.delay_lines.values().filter_map(|info| {
            if let DelayStrategy::IfWrapping { counter_name } = &info.strategy {
                Some((counter_name.as_str(), info.size))
            } else {
                None
            }
        })
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
    #[allow(dead_code)]
    pub(super) fn get_delay_line(&self, carried: SigId) -> Option<&DelayLineInfo> {
        self.delay_lines.get(&carried)
    }

    /// Returns the maximum merged delay recorded for a recursion output.
    ///
    /// Called by `ensure_recursion_array_for_group` in `module.rs` to size the
    /// recursion array large enough to serve the delay chain.
    pub(super) fn rec_max_delay(&self, var_id: u32, index: usize) -> Option<i32> {
        self.rec_group_max_delay.get(&(var_id, index)).copied()
    }
}
