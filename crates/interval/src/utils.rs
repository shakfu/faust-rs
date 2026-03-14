//! Utility helpers for interval algebra.
//!
//! # C++ source
//! `compiler/interval/utils.hh`, `compiler/interval/precision_utils.hh`,
//! `compiler/interval/check.hh` (exactPrecisionUnary)

use crate::Interval;

/// Minimum of four f64 values. C++ `itv::min4(double,double,double,double)`.
#[inline]
#[must_use]
pub fn min4(a: f64, b: f64, c: f64, d: f64) -> f64 {
    a.min(b).min(c.min(d))
}

/// Maximum of four f64 values. C++ `itv::max4(double,double,double,double)`.
#[inline]
#[must_use]
pub fn max4(a: f64, b: f64, c: f64, d: f64) -> f64 {
    a.max(b).max(c.max(d))
}

/// The bound of `x` with the smallest absolute value.
///
/// C++ `itv::minValAbs`.
#[inline]
#[must_use]
pub fn min_val_abs(x: Interval) -> f64 {
    if x.lo().abs() < x.hi().abs() {
        x.lo()
    } else {
        x.hi()
    }
}

/// The bound of `x` with the largest absolute value.
///
/// C++ `itv::maxValAbs`.
#[inline]
#[must_use]
pub fn max_val_abs(x: Interval) -> f64 {
    if x.lo().abs() < x.hi().abs() {
        x.hi()
    } else {
        x.lo()
    }
}

/// Direction of the interior at the minimum absolute value bound.
///
/// Returns `1` when `|lo| < |hi|` (interior is towards hi), `-1` otherwise.
///
/// C++ `itv::signMinValAbs`.
#[inline]
#[must_use]
pub fn sign_min_val_abs(x: Interval) -> i32 {
    if x.lo().abs() < x.hi().abs() { 1 } else { -1 }
}

/// Direction of the interior at the maximum absolute value bound.
///
/// Returns `-1` when `|lo| < |hi|` (interior is away from hi), `1` otherwise.
///
/// C++ `itv::signMaxValAbs`.
#[inline]
#[must_use]
pub fn sign_max_val_abs(x: Interval) -> i32 {
    if x.lo().abs() < x.hi().abs() { -1 } else { 1 }
}

/// Compute the output precision of a unary function `f` at point `x` with
/// unit error `u`.
///
/// Returns `i32::MIN` when `f(x + u) == f(x)` (no detectable change), which
/// signals callers to fall back to a Taylor approximation.
///
/// # C++ source
/// `exactPrecisionUnary` in `compiler/interval/precision_utils.hh`
#[must_use]
pub fn exact_precision_unary(f: fn(f64) -> f64, x: f64, u: f64) -> i32 {
    let diff = (f(x + u) - f(x)).abs();
    if diff == 0.0 {
        i32::MIN
    } else {
        diff.log2().floor() as i32
    }
}

/// Truncate `x` to precision `lsb`. C++ `truncate(double, int)`.
#[must_use]
pub fn truncate(x: f64, lsb: i32) -> f64 {
    let u = (2.0_f64).powi(lsb);
    u * (x / u).floor()
}

/// Compute the LSB of a floating-point number.
/// Returns the position floored at -24. C++ `lsb_number(double)`.
#[must_use]
pub fn lsb_number(x: f64) -> i32 {
    let mut precision: i32 = -24;
    #[allow(clippy::while_immutable_condition)] // x is not mutated; loop exits via break/return
    while x != 0.0 {
        let factor = (2.0_f64).powi(-precision - 1);
        if (x * factor).floor() == x * factor {
            precision += 1;
        } else {
            break;
        }
    }
    precision
}
