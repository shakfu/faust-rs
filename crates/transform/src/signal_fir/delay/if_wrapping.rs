//! If-based wrapping delay-line strategy.
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

use fir::{FirId, FirStore};

use super::SignalFirError;
use super::SignalFirErrorCode;
use super::circular_pow2::DelayArith;

// ─── buffer_size ─────────────────────────────────────────────────────────────

/// Minimum buffer size for the IfWrapping strategy: `delay + 1` (exact).
pub(super) fn buffer_size(max_delay: i32) -> Result<usize, SignalFirError> {
    usize::try_from(max_delay).map(|d| d + 1).map_err(|_| {
        SignalFirError::new(
            SignalFirErrorCode::UnsupportedSignalNode,
            format!("SIGDELAY amount overflow: {max_delay}"),
        )
    })
}

// ─── Emission ─────────────────────────────────────────────────────────────────

/// Computes the read index for an `IfWrapping` delay line:
/// `(counter + size - amount)` with if-based wrap when `≥ size`.
pub(super) fn if_wrapping_read_index(
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
pub(super) fn bump_if_wrapping_counter(
    store: &mut FirStore,
    counter_name: &str,
    size: usize,
) -> FirId {
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

/// Emits the end-of-sample counter advance for one `IfWrapping` delay line.
pub(super) fn emit_if_wrapping_advance(
    store: &mut FirStore,
    counter_name: &str,
    size: usize,
) -> FirId {
    bump_if_wrapping_counter(store, counter_name, size)
}
