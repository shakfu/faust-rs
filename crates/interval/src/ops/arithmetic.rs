//! Arithmetic interval operators.
//!
//! # C++ source
//! `intervalAdd.cpp`, `intervalSub.cpp`, `intervalMul.cpp`, `intervalDiv.cpp`,
//! `intervalInv.cpp`, `intervalNeg.cpp`, `intervalAbs.cpp`, `intervalMod.cpp`

use crate::utils::{exact_precision_unary, max_val_abs, max4, min4, sign_max_val_abs};
use crate::{Interval, empty, saturated_precision_add, singleton};

// -------------------------------------------------------------------------
// Add
// -------------------------------------------------------------------------

/// Interval addition.
///
/// For integer intervals (lsb ≥ 0) detects wrap-around at INT boundaries and
/// returns the full integer range when wrapping occurs.
///
/// # C++ source
/// `intervalAdd.cpp`
#[must_use]
pub fn add(x: Interval, y: Interval) -> Interval {
    if x.is_empty() || y.is_empty() {
        return empty();
    }

    let lsb = x.lsb().min(y.lsb());

    if x.lsb() >= 0 && y.lsb() >= 0 {
        let lo = x.lo() + y.lo();
        let hi = x.hi() + y.hi();

        let int_min = i32::MIN as f64;
        let int_max = i32::MAX as f64;

        // Discontinuity at the lower end of the integer range.
        if lo <= int_min - 1.0 && hi >= int_min {
            return Interval::new(int_min, int_max, lsb);
        }
        // Discontinuity at the upper end.
        if lo <= int_max && hi >= int_max + 1.0 {
            return Interval::new(int_min, int_max, lsb);
        }

        let xlo = x.lo() as i32;
        let xhi = x.hi() as i32;
        let ylo = y.lo() as i32;
        let yhi = y.hi() as i32;
        return Interval::new(
            (xlo.wrapping_add(ylo)) as f64,
            (xhi.wrapping_add(yhi)) as f64,
            lsb,
        );
    }

    Interval::new(x.lo() + y.lo(), x.hi() + y.hi(), lsb)
}

// -------------------------------------------------------------------------
// Sub
// -------------------------------------------------------------------------

/// Interval subtraction: `[x.lo - y.hi, x.hi - y.lo]`.
///
/// # C++ source
/// `intervalSub.cpp`
#[must_use]
pub fn sub(x: Interval, y: Interval) -> Interval {
    if x.is_empty() || y.is_empty() {
        return empty();
    }
    Interval::new(x.lo() - y.hi(), x.hi() - y.lo(), x.lsb().min(y.lsb()))
}

// -------------------------------------------------------------------------
// Neg
// -------------------------------------------------------------------------

/// Interval negation: `[-x.hi, -x.lo]`.
///
/// # C++ source
/// `intervalNeg.cpp`
#[must_use]
pub fn neg(x: Interval) -> Interval {
    if x.is_empty() {
        return empty();
    }
    Interval::new(-x.hi(), -x.lo(), x.lsb())
}

// -------------------------------------------------------------------------
// Abs
// -------------------------------------------------------------------------

/// Interval absolute value.
///
/// Handles the INT_MIN overflow edge case for integer intervals.
///
/// # C++ source
/// `intervalAbs.cpp`
#[must_use]
pub fn abs(x: Interval) -> Interval {
    if x.is_empty() {
        return empty();
    }

    if x.lo() >= 0.0 {
        return x;
    }

    let int_min = i32::MIN as f64;
    let int_max = i32::MAX as f64;

    // Integer overflow: abs(INT_MIN) overflows.
    if x.lsb() >= 0 && x.lo() <= int_min {
        let lo = if x.hi() >= 0.0 {
            0.0
        } else {
            x.hi().abs().min(int_max)
        };
        return Interval::new(lo, int_max, x.lsb());
    }

    if x.hi() <= 0.0 {
        return Interval::new(-x.hi(), -x.lo(), x.lsb());
    }

    Interval::new(0.0, x.lo().abs().max(x.hi().abs()), x.lsb())
}

// -------------------------------------------------------------------------
// Mul
// -------------------------------------------------------------------------

/// Special multiplication: treats `inf * 0 = 0`.
#[inline]
fn special_mult(a: f64, b: f64) -> f64 {
    if a == 0.0 || b == 0.0 { 0.0 } else { a * b }
}

/// Interval multiplication.
///
/// For integer intervals detects overflow and returns the full integer range
/// when the product may exceed INT bounds.
///
/// # C++ source
/// `intervalMul.cpp`
#[must_use]
pub fn mul(x: Interval, y: Interval) -> Interval {
    if x.is_empty() || y.is_empty() {
        return empty();
    }

    let a = special_mult(x.lo(), y.lo());
    let b = special_mult(x.lo(), y.hi());
    let c = special_mult(x.hi(), y.lo());
    let d = special_mult(x.hi(), y.hi());

    let lo = min4(a, b, c, d);
    let hi = max4(a, b, c, d);
    let lsb = saturated_precision_add(x.lsb(), y.lsb());

    if x.lsb() >= 0 && y.lsb() >= 0 {
        let xmax = x.lo().abs().max(x.hi().abs());
        let ymax = y.lo().abs().max(y.hi().abs());
        if xmax * ymax >= i32::MAX as f64 {
            return Interval::new(i32::MIN as f64, i32::MAX as f64, lsb);
        }
    }

    Interval::new(lo, hi, lsb)
}

// -------------------------------------------------------------------------
// Inv
// -------------------------------------------------------------------------

/// Interval inverse (1/x).
///
/// Handles zero in the interval by returning unbounded intervals.
///
/// # C++ source
/// `intervalInv.cpp`
#[must_use]
pub fn inv(x: Interval) -> Interval {
    if x.is_empty() {
        return empty();
    }

    let inv_fn: fn(f64) -> f64 = |v| if v == 0.0 { f64::INFINITY } else { 1.0 / v };

    let sign = sign_max_val_abs(x);
    let mut v = max_val_abs(x);

    // If v is infinite, snap to a large-but-finite integer bound.
    if v.is_infinite() {
        v = if sign == -1 {
            i32::MAX as f64
        } else {
            i32::MIN as f64
        };
    }

    let u = (2.0_f64).powi(x.lsb());
    let precision = {
        let p = exact_precision_unary(inv_fn, v, sign as f64 * u);
        if p == i32::MIN {
            // Taylor fallback: 1/(x+u) - 1/x ≈ -u/x²
            (x.lsb() as f64 - 2.0 * v.abs().log2()).floor() as i32
        } else {
            p
        }
    };

    if x.hi() < 0.0 || x.lo() >= 0.0 {
        return Interval::new(1.0 / x.hi(), 1.0 / x.lo(), precision);
    }
    if x.hi() == 0.0 && x.lo() < 0.0 {
        return Interval::new(f64::NEG_INFINITY, 1.0 / x.lo(), precision);
    }
    if x.lo() == 0.0 && x.hi() > 0.0 {
        return Interval::new(1.0 / x.hi(), f64::INFINITY, precision);
    }
    Interval::new(f64::NEG_INFINITY, f64::INFINITY, precision)
}

// -------------------------------------------------------------------------
// Div
// -------------------------------------------------------------------------

/// Interval division: `Mul(x, Inv(y))`.
///
/// # C++ source
/// `intervalDiv.cpp`
#[must_use]
pub fn div(x: Interval, y: Interval) -> Interval {
    mul(x, inv(y))
}

// -------------------------------------------------------------------------
// Mod
// -------------------------------------------------------------------------

/// Interval union helper used locally in Mod.
fn union_interval(a: Interval, b: Interval) -> Interval {
    if a.is_empty() {
        return b;
    }
    if b.is_empty() {
        return a;
    }
    Interval::new(a.lo().min(b.lo()), a.hi().max(b.hi()), a.lsb().min(b.lsb()))
}

/// `fmod(x, y)` for positive x > 0 and positive y > 0.
fn positive_fmod(x: Interval, y: Interval) -> Interval {
    if x.is_empty() || y.is_empty() {
        return empty();
    }
    let n = (x.lo() / y.hi()) as i64;
    let precision = x.lsb().min(y.lsb());

    let hi = x.hi() / (n.saturating_add(1)) as f64;
    if y.hi() <= hi {
        return Interval::new(0.0, y.hi().next_down(), precision);
    }
    if y.lo() <= hi {
        return Interval::new(0.0, hi.next_down(), precision);
    }
    Interval::new(
        x.lo() - n as f64 * y.hi(),
        x.hi() - n as f64 * y.lo(),
        precision,
    )
}

/// Split a positive interval into (negative part, positive part).
fn split_interval(x: Interval) -> (Interval, Interval) {
    if x.lo() >= 0.0 {
        return (empty(), x);
    }
    if x.hi() < 0.0 {
        return (x, empty());
    }
    (
        Interval::new(x.lo(), f64::NEG_INFINITY.next_up(), x.lsb()),
        Interval::new(0.0, x.hi(), x.lsb()),
    )
}

/// Split excluding zero.
fn split_nz(x: Interval) -> (Interval, Interval) {
    if x.lo() >= 0.0 {
        return (empty(), x);
    }
    if x.hi() < 0.0 {
        return (x, empty());
    }
    (
        Interval::new(x.lo(), f64::NEG_INFINITY.next_up(), x.lsb()),
        Interval::new(f64::INFINITY.next_down(), x.hi(), x.lsb()),
    )
}

/// Interval modulo.
///
/// # C++ source
/// `intervalMod.cpp`
#[must_use]
pub fn mod_interval(x: Interval, y: Interval) -> Interval {
    let (xn, xp) = split_interval(x);
    let (yn, yp) = split_nz(y);

    let xnyn = neg(positive_fmod(neg(xn), neg(yn)));
    let xnyp = neg(positive_fmod(neg(xn), yp));
    let xpyn = positive_fmod(xp, neg(yn));
    let xpyp = positive_fmod(xp, yp);

    let precision = x.lsb().min(y.lsb());
    let bb = {
        let bb = union_interval(
            union_interval(
                singleton(x.hi().rem_euclid(y.hi())),
                singleton(x.lo().rem_euclid(y.hi())),
            ),
            union_interval(
                singleton(x.hi().rem_euclid(y.lo())),
                singleton(x.lo().rem_euclid(y.lo())),
            ),
        );
        Interval::new(bb.lo(), bb.hi(), precision)
    };

    union_interval(
        union_interval(union_interval(union_interval(bb, xnyn), xnyp), xpyn),
        xpyp,
    )
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_basic() {
        let r = add(
            Interval::new(0.0, 100.0, -5),
            Interval::new(10.0, 500.0, -5),
        );
        assert_eq!(r.lo(), 10.0);
        assert_eq!(r.hi(), 600.0);
    }

    #[test]
    fn add_empty() {
        assert!(add(empty(), Interval::new(1.0, 2.0, 0)).is_empty());
    }

    #[test]
    fn sub_basic() {
        let r = sub(Interval::new(5.0, 10.0, -5), Interval::new(1.0, 3.0, -5));
        assert_eq!(r.lo(), 2.0);
        assert_eq!(r.hi(), 9.0);
    }

    #[test]
    fn neg_basic() {
        let r = neg(Interval::new(-3.0, 5.0, -5));
        assert_eq!(r.lo(), -5.0);
        assert_eq!(r.hi(), 3.0);
    }

    #[test]
    fn abs_positive() {
        let r = abs(Interval::new(2.0, 5.0, 0));
        assert_eq!(r.lo(), 2.0);
        assert_eq!(r.hi(), 5.0);
    }

    #[test]
    fn abs_crosses_zero() {
        let r = abs(Interval::new(-3.0, 5.0, -5));
        assert_eq!(r.lo(), 0.0);
        assert_eq!(r.hi(), 5.0);
    }

    #[test]
    fn abs_all_negative() {
        let r = abs(Interval::new(-10.0, -2.0, -5));
        assert_eq!(r.lo(), 2.0);
        assert_eq!(r.hi(), 10.0);
    }

    #[test]
    fn mul_basic() {
        let r = mul(Interval::new(-1.0, 1.0, -5), Interval::new(0.0, 1.0, -5));
        assert_eq!(r.lo(), -1.0);
        assert_eq!(r.hi(), 1.0);
    }

    #[test]
    fn mul_saturates_precision_overflow() {
        let r = mul(
            Interval::new(1.0, 1.0, i32::MAX),
            Interval::new(1.0, 1.0, 1),
        );
        assert_eq!(r.lsb(), i32::MAX);
    }

    #[test]
    fn div_basic() {
        let r = div(Interval::new(-2.0, 3.0, -5), Interval::new(1.0, 10.0, -5));
        assert!(r.lo() <= -2.0);
        assert!(r.hi() >= 3.0);
    }

    #[test]
    fn int_cast_test() {
        // Covered in casts.rs but verify mul integer overflow path.
        let r = mul(
            Interval::new(2.0_f64.powi(30), 2.0_f64.powi(30) + 2.0, 2),
            Interval::new(1.0, 2.0, 0),
        );
        assert_eq!(r.lo(), i32::MIN as f64);
    }
}
