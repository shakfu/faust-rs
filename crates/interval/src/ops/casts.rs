//! Cast and injection operators.
//!
//! # C++ source
//! `intervalIntCast.cpp`, `intervalFloatCast.cpp`, `intervalIntNum.cpp`,
//! `intervalFloatNum.cpp`, `intervalLabel.cpp`

use crate::{Interval, empty, saturated_int_cast, singleton};

// -------------------------------------------------------------------------
// IntCast
// -------------------------------------------------------------------------

/// Restrict interval to the `i32` range with 0-bit precision.
///
/// # C++ source
/// `intervalIntCast.cpp`
#[must_use]
pub fn int_cast(x: Interval) -> Interval {
    if x.is_empty() {
        return empty();
    }
    Interval::new(
        saturated_int_cast(x.lo()) as f64,
        saturated_int_cast(x.hi()) as f64,
        0,
    )
}

// -------------------------------------------------------------------------
// FloatCast
// -------------------------------------------------------------------------

/// Force float type by ensuring LSB ≤ -1.
///
/// # C++ source
/// `intervalFloatCast.cpp`
#[must_use]
pub fn float_cast(x: Interval) -> Interval {
    if x.is_empty() {
        return empty();
    }
    Interval::new(x.lo(), x.hi(), x.lsb().min(-1))
}

// -------------------------------------------------------------------------
// Literal injections
// -------------------------------------------------------------------------

/// Integer literal interval `[x, x]` with precision 0.
///
/// # C++ source
/// `intervalIntNum.cpp`
#[must_use]
pub fn int_num(x: i32) -> Interval {
    Interval::new(x as f64, x as f64, 0)
}

/// 64-bit integer literal interval `[x, x]` with precision 0.
///
/// # C++ source
/// `intervalIntNum.cpp` (Int64Num)
#[must_use]
pub fn int64_num(x: i64) -> Interval {
    Interval::new(x as f64, x as f64, 0)
}

/// Float literal — singleton interval whose precision represents `x` exactly.
///
/// # C++ source
/// `intervalFloatNum.cpp`
#[must_use]
pub fn float_num(x: f64) -> Interval {
    singleton(x)
}

/// Label: returns an empty interval (labels carry no numeric information).
///
/// # C++ source
/// `intervalLabel.cpp`
#[must_use]
pub fn label(_name: &str) -> Interval {
    empty()
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_cast_truncates() {
        let r = int_cast(Interval::new(-3.8, 4.9, -5));
        assert_eq!(r.lo(), -3.0);
        assert_eq!(r.hi(), 4.0);
        assert_eq!(r.lsb(), 0);
    }

    #[test]
    fn int_cast_inf() {
        let r = int_cast(Interval::new(f64::NEG_INFINITY, f64::INFINITY, -5));
        assert_eq!(r.lo(), -2_147_483_648.0);
        assert_eq!(r.hi(), 2_147_483_647.0);
        assert_eq!(r.lsb(), 0);
    }

    #[test]
    fn float_cast_lsb() {
        let r = float_cast(Interval::new(-1.0, 1.0, 0));
        assert_eq!(r.lsb(), -1);
    }

    #[test]
    fn float_cast_preserves_finer() {
        let r = float_cast(Interval::new(-1.0, 1.0, -5));
        assert_eq!(r.lsb(), -5);
    }

    #[test]
    fn int_num_basic() {
        let r = int_num(42);
        assert!(r.is_const());
        assert_eq!(r.lo(), 42.0);
    }

    #[test]
    fn float_num_basic() {
        let r = float_num(3.14);
        assert!(r.is_const());
    }

    #[test]
    fn label_empty() {
        assert!(label("foo").is_empty());
    }
}
