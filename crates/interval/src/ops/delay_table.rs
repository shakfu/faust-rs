//! Delay, memory, table, and soundfile interval operators.
//!
//! # C++ source
//! `intervalDelay.cpp`, `intervalMem.cpp`, `intervalMissing.cpp`

use crate::{reunion, Interval};

// -------------------------------------------------------------------------
// Delay / Mem
// -------------------------------------------------------------------------

/// Interval delay: union of `x` with the zero singleton.
///
/// The output can be either the input signal or its delayed version
/// (which starts at 0).
///
/// # C++ source
/// `intervalDelay.cpp`
#[must_use]
pub fn delay(x: Interval, _d: Interval) -> Interval {
    reunion(x, Interval::new(0.0, 0.0, 0))
}

/// Interval memory: union of `x` with zero (same as delay).
///
/// # C++ source
/// `intervalMem.cpp`
#[must_use]
pub fn mem(x: Interval) -> Interval {
    reunion(x, Interval::new(0.0, 0.0, 0))
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{empty, Interval};

    #[test]
    fn delay_positive_interval() {
        let r = delay(Interval::new(1.0, 5.0, -5), empty());
        assert_eq!(r.lo(), 0.0);
        assert_eq!(r.hi(), 5.0);
    }

    #[test]
    fn mem_includes_zero() {
        let r = mem(Interval::new(2.0, 4.0, -5));
        assert!(r.has(0.0));
    }

    #[test]
    fn mem_negative_interval() {
        let r = mem(Interval::new(-3.0, -1.0, -5));
        assert_eq!(r.lo(), -3.0);
        assert_eq!(r.hi(), 0.0);
    }
}
