//! Delay-line strategy selection options.
//!
//! [`DelayOptions`] mirrors the Faust `-mcd` / `-dlt` compiler options and
//! drives the strategy selector inside [`super::DelayManager::ensure_delay_line`].

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
pub(crate) struct DelayOptions {
    /// Shift/copy model upper bound (inclusive).  Default: 16.
    pub(crate) max_copy_delay: u32,
    /// If-based wrapping model lower bound (exclusive).  Default: `u32::MAX`
    /// (disabled; all non-copy delays use the circular-pow2 model).
    pub(crate) delay_line_threshold: u32,
}

impl Default for DelayOptions {
    fn default() -> Self {
        Self {
            max_copy_delay: 16,
            delay_line_threshold: u32::MAX,
        }
    }
}
