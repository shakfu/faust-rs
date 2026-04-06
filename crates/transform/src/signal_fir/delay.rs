//! Circular delay-line sizing and geometry for the FIR fast-lane.
//!
//! Faust's `@(n)` operator maps to a **circular ring buffer** of power-of-two
//! size in the generated C++/FIR output.  This module provides the pure
//! *analysis layer* for that model: deciding how large a buffer needs to be
//! and resolving the delay amount from the signal tree.
//!
//! # Circular buffer model
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
//! The power-of-two constraint means the mask `(S - 1)` is always a single
//! bitwise AND, matching C++ Faust's `signalFIRCompiler::writeReadDelay`.
//!
//! # Recursion + delay merging
//!
//! When the pattern `SIGDELAY(Delay1(Proj(i, SYMREF(v))), N)` appears, the
//! recursion array for output `i` of variable `v` is sized to hold the full
//! chain (`N + 1` samples) so no separate `fVec` is needed.  The scan pass
//! in `module.rs` calls [`delay_size_for_amount`] and records the merged size
//! on `rec_group_max_delay`; [`ensure_recursion_array_for_group`] consumes it.
//!
//! # Scope of this module
//!
//! This module covers **sizing decisions** — pure functions with no FIR
//! side-effects:
//!
//! | Function | Purpose |
//! |----------|---------|
//! | [`pow2limit_for_delay`] | `next_power_of_two(N + 1)` with overflow checks |
//! | [`constant_delay_amount`] | Extract a literal `SIGINT` delay amount |
//! | [`variable_delay_max_bound`] | Derive a bound from interval analysis |
//! | [`min_const_upper_bound`] | Structural fallback: `SIGMIN(SigInt(n), _)` |
//! | [`delay_size_for_amount`] | Unified resolver (tries all three above) |
//!
//! The complementary **FIR materialization** layer — allocating struct arrays,
//! emitting `fIOTA` loads/stores, building read/write index expressions —
//! lives in `module.rs` as `impl SignalToFirLower` methods, because those
//! methods need mutable access to the lowering engine's statement lists and
//! FIR node store.
//!
//! Source provenance (C++):
//! - `compiler/transform/signalFIRCompiler.hh` — `DelayLine`, `allocateDelayLineAux`
//! - `compiler/transform/signalFIRCompiler.cpp` — `compileSigDelay`, `writeReadDelay`,
//!   `checkDelayInterval`

use std::collections::HashMap;

use signals::{SigId, SigMatch, match_sig};
use sigtype::{SigType, check_delay_interval};
use tlib::TreeArena;

use super::error::{SignalFirError, SignalFirErrorCode};

// ─── DelayLineInfo ────────────────────────────────────────────────────────────

/// Fixed-size circular delay-line resource used by fast-lane `SIGDELAY`.
///
/// Stores the metadata for one allocated DSP-struct array that implements a
/// ring buffer.  The array is named `fVec<id>` (real) or `iVec<id>` (integer),
/// declared once during [`prepare_delay_lines`](super::module) pre-scan, and
/// zeroed in `instanceClear`.
///
/// Source provenance (C++):
/// - `compiler/transform/signalFIRCompiler.hh` (`DelayLine`, `allocateDelayLineAux`)
/// - `compiler/transform/signalFIRCompiler.cpp` (`compileSigDelay`, `writeReadDelay`)
#[derive(Clone, Debug)]
pub(super) struct DelayLineInfo {
    /// Generated DSP-struct array variable name (e.g. `fVec42`).
    pub(super) name: String,
    /// Allocated size in elements (always a power of two ≥ 1).
    pub(super) size: usize,
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
