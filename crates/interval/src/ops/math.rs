//! Mathematical function interval operators.
//!
//! # C++ source
//! `intervalCeil.cpp`, `intervalFloor.cpp`, `intervalRound.cpp`,
//! `intervalRint.cpp`, `intervalExp.cpp`, `intervalLog.cpp`,
//! `intervalLog10.cpp`, `intervalSqrt.cpp`, `intervalPow.cpp`,
//! `intervalRemainder.cpp`

use crate::{empty, intersection, reunion, Interval};
use crate::utils::exact_precision_unary;
use crate::saturated_int_cast;

// -------------------------------------------------------------------------
// Ceil
// -------------------------------------------------------------------------

/// Interval ceiling: `[ceil(lo), ceil(hi)]` with LSB = -1.
///
/// # C++ source
/// `intervalCeil.cpp`
#[must_use]
pub fn ceil(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }
    Interval::new(x.lo().ceil(), x.hi().ceil(), -1)
}

// -------------------------------------------------------------------------
// Floor
// -------------------------------------------------------------------------

/// Interval floor: `[floor(lo), floor(hi)]` with LSB = -1.
///
/// # C++ source
/// `intervalFloor.cpp`
#[must_use]
pub fn floor(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }
    Interval::new(x.lo().floor(), x.hi().floor(), -1)
}

// -------------------------------------------------------------------------
// Round
// -------------------------------------------------------------------------

/// Interval round-to-nearest with precision `max(0, x.lsb())`.
///
/// # C++ source
/// `intervalRound.cpp`
#[must_use]
pub fn round(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }
    Interval::new(x.lo().round(), x.hi().round(), 0i32.max(x.lsb()))
}

// -------------------------------------------------------------------------
// Rint
// -------------------------------------------------------------------------

/// Interval round-to-nearest-integer (banker's rounding) with precision
/// `max(0, x.lsb())`.
///
/// # C++ source
/// `intervalRint.cpp`
#[must_use]
pub fn rint(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }
    // rint rounds to nearest, ties to even.
    Interval::new(x.lo().round_ties_even(), x.hi().round_ties_even(), 0i32.max(x.lsb()))
}

// -------------------------------------------------------------------------
// Exp
// -------------------------------------------------------------------------

/// Interval exponential.
///
/// # C++ source
/// `intervalExp.cpp`
#[must_use]
pub fn exp(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    // Precision is worst at the lowest bound (smallest exp value → smallest
    // absolute change per ulp).
    let v = x.lo();
    let mut precision = exact_precision_unary(f64::exp, v, u);
    if precision == i32::MIN {
        // exp'(x) = exp(x) → precision ≈ lsb + log2(exp(v))
        precision = (x.lsb() as f64 + v * std::f64::consts::LOG2_E).floor() as i32;
    }

    Interval::new(x.lo().exp(), x.hi().exp(), precision)
}

// -------------------------------------------------------------------------
// Log
// -------------------------------------------------------------------------

/// Interval natural logarithm (domain `(0, +∞]`).
///
/// # C++ source
/// `intervalLog.cpp`
#[must_use]
pub fn log(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }
    let lo = x.lo().max(f64::MIN_POSITIVE);
    if lo > x.hi() { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    // Precision worst at the highest bound where slope = 1/x is smallest.
    let v = x.hi();
    let mut precision = exact_precision_unary(f64::ln, v, u);
    if precision == i32::MIN {
        // log'(x) = 1/x
        precision = (x.lsb() as f64 - v.abs().log2()).floor() as i32;
    }

    Interval::new(lo.ln(), x.hi().ln(), precision)
}

// -------------------------------------------------------------------------
// Log10
// -------------------------------------------------------------------------

/// Interval base-10 logarithm (domain `(0, +∞]`).
///
/// # C++ source
/// `intervalLog10.cpp`
#[must_use]
pub fn log10(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }
    let lo = x.lo().max(f64::MIN_POSITIVE);
    if lo > x.hi() { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    let v = x.hi();
    let mut precision = exact_precision_unary(f64::log10, v, u);
    if precision == i32::MIN {
        precision = (x.lsb() as f64 - v.abs().log2() - std::f64::consts::LOG2_10.log2()).floor() as i32;
    }

    Interval::new(lo.log10(), x.hi().log10(), precision)
}

// -------------------------------------------------------------------------
// Sqrt
// -------------------------------------------------------------------------

/// Interval square root (domain `[0, +∞]`).
///
/// # C++ source
/// `intervalSqrt.cpp`
#[must_use]
pub fn sqrt(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }
    let lo = x.lo().max(0.0);
    if lo > x.hi() { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    // Precision worst at the highest bound.
    let v = x.hi();
    let mut precision = exact_precision_unary(f64::sqrt, v, u);
    if precision == i32::MIN {
        // sqrt'(x) = 1/(2*sqrt(x))
        let sv = v.sqrt();
        precision = if sv == 0.0 {
            i32::MIN / 2
        } else {
            (x.lsb() as f64 - (2.0 * sv).log2()).floor() as i32
        };
    }

    Interval::new(lo.sqrt(), x.hi().sqrt(), precision)
}

// -------------------------------------------------------------------------
// Pow helpers
// -------------------------------------------------------------------------

fn ipow_scalar(x: Interval, k: i32) -> Interval {
    debug_assert!(k >= 0);
    if k == 0 { return Interval::new(1.0, 1.0, 0); }

    let precision = x.lsb().saturating_mul(k);
    // Even exponent: result is always non-negative.
    if k % 2 == 0 {
        let z0 = x.lo().powi(k);
        let z1 = x.hi().powi(k);
        let lo = if x.has_zero() { 0.0 } else { z0.min(z1) };
        return Interval::new(lo, z0.max(z1), precision);
    }
    // Odd exponent: monotone.
    Interval::new(x.lo().powi(k), x.hi().powi(k), precision)
}

fn i_pow(x: Interval, y: Interval) -> Interval {
    if x.is_empty() || y.is_empty() { return empty(); }
    let y0 = 0i32.max(saturated_int_cast(y.lo()));
    let y1 = 0i32.max(saturated_int_cast(y.hi()));
    let mut z = ipow_scalar(x, y0);
    if y1 > y0 {
        z = reunion(z, ipow_scalar(x, y0 + 1));
        z = reunion(z, ipow_scalar(x, y1 - 1));
        z = reunion(z, ipow_scalar(x, y1));
    }
    z
}

fn f_pow(x: Interval, y: Interval) -> Interval {
    if x.is_empty() || y.is_empty() { return empty(); }
    // x > 0: x^y = exp(y * log(x))
    use crate::ops::arithmetic::mul as amul;
    use super::math::{exp as aexp, log as alog};
    aexp(amul(y, alog(x)))
}

// -------------------------------------------------------------------------
// Pow
// -------------------------------------------------------------------------

/// Interval power `x^y`.
///
/// Decomposes into:
/// - `x^0 = 1` when `y` contains 0
/// - `0^y = 0` when `x` contains 0
/// - float power via `exp(y * log(x))` for positive `x`
/// - integer power for negative `x`
///
/// # C++ source
/// `intervalPow.cpp`
#[must_use]
pub fn pow(x: Interval, y: Interval) -> Interval {
    let mut z = empty();

    let pos_lo = f64::MIN_POSITIVE.next_up();
    let neg_hi = (-f64::MIN_POSITIVE).next_down();

    let xp = intersection(x, Interval::new(pos_lo, f64::INFINITY, 0));
    let xn = intersection(x, Interval::new(f64::NEG_INFINITY, neg_hi, 0));

    if y.has_zero() {
        z = reunion(z, Interval::new(1.0, 1.0, 0));
    }
    if x.has_zero() {
        z = reunion(z, Interval::new(0.0, 0.0, 0));
    }
    if !xp.is_empty() {
        z = reunion(z, f_pow(xp, y));
    }
    if !xn.is_empty() {
        z = reunion(z, i_pow(xn, y));
    }
    z
}

// -------------------------------------------------------------------------
// Remainder (placeholder)
// -------------------------------------------------------------------------

/// Interval remainder — placeholder returning empty.
///
/// C++ `intervalRemainder.cpp` is also a placeholder.
#[must_use]
pub fn remainder(_x: Interval) -> Interval {
    empty()
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Interval;

    #[test]
    fn ceil_basic() {
        let r = ceil(Interval::new(1.1, 2.9, -5));
        assert_eq!(r.lo(), 2.0);
        assert_eq!(r.hi(), 3.0);
        assert_eq!(r.lsb(), -1);
    }

    #[test]
    fn floor_basic() {
        let r = floor(Interval::new(1.1, 2.9, -5));
        assert_eq!(r.lo(), 1.0);
        assert_eq!(r.hi(), 2.0);
        assert_eq!(r.lsb(), -1);
    }

    #[test]
    fn exp_monotone() {
        let r = exp(Interval::new(0.0, 1.0, -10));
        assert!((r.lo() - 1.0).abs() < 1e-9);
        assert!((r.hi() - 1.0_f64.exp()).abs() < 1e-9);
    }

    #[test]
    fn log_basic() {
        let r = log(Interval::new(1.0, std::f64::consts::E, -10));
        assert!((r.lo() - 0.0).abs() < 1e-9);
        assert!((r.hi() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sqrt_basic() {
        let r = sqrt(Interval::new(0.0, 4.0, -10));
        assert_eq!(r.lo(), 0.0);
        assert_eq!(r.hi(), 2.0);
    }

    #[test]
    fn pow_zero_exponent() {
        let r = pow(Interval::new(2.0, 5.0, -5), Interval::new(0.0, 0.0, 0));
        assert!(r.has(1.0));
    }

    #[test]
    fn remainder_placeholder() {
        assert!(remainder(Interval::new(1.0, 5.0, 0)).is_empty());
    }
}
