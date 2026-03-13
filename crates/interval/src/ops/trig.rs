//! Trigonometric and hyperbolic interval operators.
//!
//! # C++ source
//! `intervalSin.cpp`, `intervalCos.cpp`, `intervalTan.cpp`,
//! `intervalAcos.cpp`, `intervalAcosh.cpp`, `intervalAsin.cpp`,
//! `intervalAsinh.cpp`, `intervalAtan.cpp`, `intervalAtan2.cpp`,
//! `intervalAtanh.cpp`, `intervalCosh.cpp`, `intervalSinh.cpp`,
//! `intervalTanh.cpp`

use std::f64::consts::{PI, FRAC_PI_2};
use crate::{empty, Interval};
use crate::utils::exact_precision_unary;

// -------------------------------------------------------------------------
// Sin
// -------------------------------------------------------------------------

/// Interval sine.
///
/// # C++ source
/// `intervalSin.cpp`
#[must_use]
pub fn sin(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }

    // Initial precision estimate at x = π/2 (half-integer).
    let u = (2.0_f64).powi(x.lsb());
    let mut precision = exact_precision_unary(f64::sin, FRAC_PI_2, u);
    if precision == i32::MIN {
        precision = 2 * x.lsb() - 1;
    }

    if x.size() >= 2.0 * PI {
        return Interval::new(-1.0, 1.0, precision);
    }

    // Normalise lo to [0, 2π).
    let mut l = x.lo() % (2.0 * PI);
    if l < 0.0 { l += 2.0 * PI; }
    let i = Interval::new(l, l + x.size(), x.lsb());

    let a = i.lo().sin();
    let b = i.hi().sin();
    let mut lo = a.min(b);
    let mut hi = a.max(b);

    // Critical points: sin peaks at π/2 + 2kπ, troughs at 3π/2 + 2kπ.
    if i.has(FRAC_PI_2) || i.has(5.0 * FRAC_PI_2) { hi = 1.0; }
    if i.has(3.0 * FRAC_PI_2) || i.has(7.0 * FRAC_PI_2) { lo = -1.0; }

    // Refine precision at the bound closest to a half-integer.
    let mut v = FRAC_PI_2;
    if i.hi() < FRAC_PI_2 {
        v = x.hi();
    } else if (i.lo() > FRAC_PI_2 && i.hi() < 3.0 * FRAC_PI_2)
           || (i.lo() > 3.0 * FRAC_PI_2 && i.hi() < 5.0 * FRAC_PI_2)
    {
        let delta_hi = (i.hi() / PI + 0.5).ceil() - i.hi() / PI;
        let delta_lo = i.lo() / PI - (i.lo() / PI - 0.5).floor();
        if delta_lo > delta_hi { v = x.hi(); } else { v = x.lo(); }
    }

    precision = exact_precision_unary(f64::sin, v, u);
    if precision == i32::MIN {
        if v != FRAC_PI_2 {
            precision = x.lsb() + v.cos().abs().log2().floor() as i32;
        } else {
            precision = 2 * x.lsb() - 1;
        }
    }

    Interval::new(lo, hi, precision)
}

// -------------------------------------------------------------------------
// Cos
// -------------------------------------------------------------------------

/// Interval cosine.
///
/// # C++ source
/// `intervalCos.cpp`
#[must_use]
pub fn cos(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    let mut precision = exact_precision_unary(f64::cos, 0.0, u);
    if precision == i32::MIN { precision = 2 * x.lsb() - 1; }

    if x.size() >= 2.0 * PI {
        return Interval::new(-1.0, 1.0, precision);
    }

    let mut l = x.lo() % (2.0 * PI);
    if l < 0.0 { l += 2.0 * PI; }
    let i = Interval::new(l, l + x.size(), x.lsb());

    let a = i.lo().cos();
    let b = i.hi().cos();
    let mut lo = a.min(b);
    let mut hi = a.max(b);

    // Critical points: cos peaks at 0 + 2kπ, troughs at π + 2kπ.
    if i.has(0.0) || i.has(2.0 * PI) { hi = 1.0; }
    if i.has(PI) || i.has(3.0 * PI) { lo = -1.0; }

    let mut v = 0.0_f64;
    if i.hi() < PI || (i.lo() > PI && i.hi() < 2.0 * PI) {
        let delta_hi = (x.hi() / PI).ceil() - x.hi() / PI;
        let delta_lo = x.lo() / PI - (x.lo() / PI).floor();
        if delta_hi < delta_lo { v = x.hi(); } else { v = x.lo(); }
    }

    precision = exact_precision_unary(f64::cos, v, u);
    if precision == i32::MIN {
        if v != 0.0 {
            precision = x.lsb() + v.sin().abs().log2().floor() as i32;
        } else {
            precision = 2 * x.lsb() - 1;
        }
    }

    Interval::new(lo, hi, precision)
}

// -------------------------------------------------------------------------
// Tan
// -------------------------------------------------------------------------

/// Interval tangent.
///
/// Returns unbounded `[-∞, +∞]` when the interval crosses an asymptote.
///
/// # C++ source
/// `intervalTan.cpp`
#[must_use]
pub fn tan(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    let v = x.hi(); // worst-case precision near π/2
    let mut precision = exact_precision_unary(f64::tan, v, u);
    if precision == i32::MIN {
        // tan'(x) = 1/cos²(x); precision ≈ lsb - 2*log2(|cos(v)|)
        let cos_v = v.cos();
        if cos_v == 0.0 {
            precision = i32::MIN / 2;
        } else {
            precision = (x.lsb() as f64 - 2.0 * cos_v.abs().log2()).floor() as i32;
        }
    }

    if x.size() >= PI {
        return Interval::new(f64::NEG_INFINITY, f64::INFINITY, precision);
    }

    // Normalise lo to [0, π).
    let mut l = x.lo() % PI;
    if l < 0.0 { l += PI; }
    let hi_n = l + x.size();

    // If the interval contains π/2 (asymptote), result is unbounded.
    if l < FRAC_PI_2 && hi_n > FRAC_PI_2 {
        return Interval::new(f64::NEG_INFINITY, f64::INFINITY, precision);
    }

    Interval::new(x.lo().tan(), x.hi().tan(), precision)
}

// -------------------------------------------------------------------------
// Acos
// -------------------------------------------------------------------------

/// Interval arc cosine (domain `[-1, 1]` → `[0, π]`).
///
/// # C++ source
/// `intervalAcos.cpp`
#[must_use]
pub fn acos(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }
    let lo = x.lo().max(-1.0);
    let hi = x.hi().min(1.0);
    if lo > hi { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    // Worst precision is at |x| closest to 1 where slope is steepest.
    let v = if x.lo().abs() > x.hi().abs() { x.lo() } else { x.hi() };
    let mut precision = exact_precision_unary(f64::acos, v, u);
    if precision == i32::MIN {
        // acos'(x) = -1/sqrt(1-x²)
        let denom = (1.0 - v * v).sqrt();
        precision = if denom == 0.0 {
            i32::MIN / 2
        } else {
            (x.lsb() as f64 - denom.log2()).floor() as i32
        };
    }

    // acos is decreasing.
    Interval::new(hi.acos(), lo.acos(), precision)
}

// -------------------------------------------------------------------------
// Acosh
// -------------------------------------------------------------------------

/// Interval inverse hyperbolic cosine (domain `[1, +∞]`).
///
/// # C++ source
/// `intervalAcosh.cpp`
#[must_use]
pub fn acosh(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }
    let lo = x.lo().max(1.0);
    if lo > x.hi() { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    let v = lo; // precision is worst near the lower bound
    let mut precision = exact_precision_unary(f64::acosh, v, u);
    if precision == i32::MIN {
        let denom = (v * v - 1.0).sqrt();
        precision = if denom == 0.0 {
            i32::MIN / 2
        } else {
            (x.lsb() as f64 - denom.log2()).floor() as i32
        };
    }

    Interval::new(lo.acosh(), x.hi().acosh(), precision)
}

// -------------------------------------------------------------------------
// Asin
// -------------------------------------------------------------------------

/// Interval arc sine (domain `[-1, 1]` → `[-π/2, π/2]`).
///
/// # C++ source
/// `intervalAsin.cpp`
#[must_use]
pub fn asin(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }
    let lo = x.lo().max(-1.0);
    let hi = x.hi().min(1.0);
    if lo > hi { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    let v = if lo.abs() > hi.abs() { lo } else { hi };
    let mut precision = exact_precision_unary(f64::asin, v, u);
    if precision == i32::MIN {
        let denom = (1.0 - v * v).sqrt();
        precision = if denom == 0.0 {
            i32::MIN / 2
        } else {
            (x.lsb() as f64 - denom.log2()).floor() as i32
        };
    }

    Interval::new(lo.asin(), hi.asin(), precision)
}

// -------------------------------------------------------------------------
// Asinh
// -------------------------------------------------------------------------

/// Interval inverse hyperbolic sine.
///
/// # C++ source
/// `intervalAsinh.cpp`
#[must_use]
pub fn asinh(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    let v = if x.lo().abs() > x.hi().abs() { x.lo() } else { x.hi() };
    let mut precision = exact_precision_unary(f64::asinh, v, u);
    if precision == i32::MIN {
        let denom = (1.0 + v * v).sqrt();
        precision = (x.lsb() as f64 - denom.log2()).floor() as i32;
    }

    Interval::new(x.lo().asinh(), x.hi().asinh(), precision)
}

// -------------------------------------------------------------------------
// Atan
// -------------------------------------------------------------------------

/// Interval arc tangent.
///
/// # C++ source
/// `intervalAtan.cpp`
#[must_use]
pub fn atan(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    // Precision is worst at the smallest |x| (steepest slope 1/(1+x²)).
    let v = if x.lo().abs() < x.hi().abs() { x.lo() } else { x.hi() };
    let mut precision = exact_precision_unary(f64::atan, v, u);
    if precision == i32::MIN {
        // atan'(x) = 1/(1+x²)
        let denom = 1.0 + v * v;
        precision = (x.lsb() as f64 - denom.log2()).floor() as i32;
    }

    Interval::new(x.lo().atan(), x.hi().atan(), precision)
}

// -------------------------------------------------------------------------
// Atan2
// -------------------------------------------------------------------------

/// Interval two-argument arc tangent.
///
/// Returns `[-π, π]` when the result is potentially discontinuous
/// (interval crosses the negative x-axis).
///
/// # C++ source
/// `intervalAtan2.cpp`
#[must_use]
pub fn atan2(y: Interval, x: Interval) -> Interval {
    if y.is_empty() || x.is_empty() { return empty(); }

    // atan2 ranges [-π, π]; if x can be negative and y straddles 0,
    // the result can jump discontinuously.
    if x.has_zero() || (x.lo() < 0.0 && y.has_zero()) {
        return Interval::new(-PI, PI, -24);
    }

    // Evaluate at the four corners.
    let a = y.lo().atan2(x.lo());
    let b = y.lo().atan2(x.hi());
    let c = y.hi().atan2(x.lo());
    let d = y.hi().atan2(x.hi());

    let lo = a.min(b).min(c.min(d));
    let hi = a.max(b).max(c.max(d));

    let u = (2.0_f64).powi(y.lsb().min(x.lsb()));
    // Cannot pass a capturing closure to exact_precision_unary; compute inline.
    let diff = ((y.lo() + u).atan2(x.lo()) - y.lo().atan2(x.lo())).abs();
    let mut precision = if diff == 0.0 { i32::MIN } else { diff.log2().floor() as i32 };
    if precision == i32::MIN {
        precision = y.lsb().min(x.lsb()) - 1;
    }

    Interval::new(lo, hi, precision)
}

// -------------------------------------------------------------------------
// Atanh
// -------------------------------------------------------------------------

/// Interval inverse hyperbolic tangent (domain `(-1, 1)`).
///
/// # C++ source
/// `intervalAtanh.cpp`
#[must_use]
pub fn atanh(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }
    let lo = x.lo().max(-1.0 + f64::EPSILON);
    let hi = x.hi().min(1.0 - f64::EPSILON);
    if lo > hi { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    let v = if lo.abs() > hi.abs() { lo } else { hi };
    let mut precision = exact_precision_unary(f64::atanh, v, u);
    if precision == i32::MIN {
        let denom = 1.0 - v * v;
        precision = if denom <= 0.0 {
            i32::MIN / 2
        } else {
            (x.lsb() as f64 - denom.log2()).floor() as i32
        };
    }

    Interval::new(lo.atanh(), hi.atanh(), precision)
}

// -------------------------------------------------------------------------
// Cosh
// -------------------------------------------------------------------------

/// Interval hyperbolic cosine.
///
/// # C++ source
/// `intervalCosh.cpp`
#[must_use]
pub fn cosh(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    // cosh'(x) = sinh(x); worst at |x| max.
    let v = if x.lo().abs() > x.hi().abs() { x.lo() } else { x.hi() };
    let mut precision = exact_precision_unary(f64::cosh, v, u);
    if precision == i32::MIN {
        precision = (x.lsb() as f64 + v.sinh().abs().log2()).floor() as i32;
    }

    // cosh has minimum at x=0.
    if x.has_zero() {
        return Interval::new(1.0, x.lo().cosh().max(x.hi().cosh()), precision);
    }
    Interval::new(x.lo().cosh().min(x.hi().cosh()), x.lo().cosh().max(x.hi().cosh()), precision)
}

// -------------------------------------------------------------------------
// Sinh
// -------------------------------------------------------------------------

/// Interval hyperbolic sine.
///
/// # C++ source
/// `intervalSinh.cpp`
#[must_use]
pub fn sinh(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }

    // sinh is monotone; precision same as cosh at the same bound.
    let u = (2.0_f64).powi(x.lsb());
    let v = if x.lo().abs() > x.hi().abs() { x.lo() } else { x.hi() };
    let mut precision = exact_precision_unary(f64::sinh, v, u);
    if precision == i32::MIN {
        precision = (x.lsb() as f64 + v.cosh().log2()).floor() as i32;
    }

    Interval::new(x.lo().sinh(), x.hi().sinh(), precision)
}

// -------------------------------------------------------------------------
// Tanh
// -------------------------------------------------------------------------

/// Interval hyperbolic tangent.
///
/// # C++ source
/// `intervalTanh.cpp`
#[must_use]
pub fn tanh(x: Interval) -> Interval {
    if x.is_empty() { return empty(); }

    let u = (2.0_f64).powi(x.lsb());
    // tanh'(x) = 1/cosh²(x); worst at smallest |x|.
    let v = if x.lo().abs() < x.hi().abs() { x.lo() } else { x.hi() };
    let mut precision = exact_precision_unary(f64::tanh, v, u);
    if precision == i32::MIN {
        let c = v.cosh();
        precision = (x.lsb() as f64 - 2.0 * c.log2()).floor() as i32;
    }

    Interval::new(x.lo().tanh(), x.hi().tanh(), precision)
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;
    use crate::Interval;

    #[test]
    fn sin_full_period() {
        let r = sin(Interval::new(0.0, 2.0 * PI, -5));
        assert!(r.lo() <= -0.99);
        assert!(r.hi() >= 0.99);
    }

    #[test]
    fn cos_full_period() {
        let r = cos(Interval::new(0.0, 2.0 * PI, -5));
        assert!(r.lo() <= -0.99);
        assert!(r.hi() >= 0.99);
    }

    #[test]
    fn acos_domain() {
        let r = acos(Interval::new(-1.0, 1.0, -10));
        assert!((r.lo() - 0.0).abs() < 1e-9);
        assert!((r.hi() - PI).abs() < 1e-9);
    }

    #[test]
    fn asin_domain() {
        let r = asin(Interval::new(-1.0, 1.0, -10));
        assert!((r.lo() - (-PI / 2.0)).abs() < 1e-9);
        assert!((r.hi() - PI / 2.0).abs() < 1e-9);
    }

    #[test]
    fn cosh_zero_minimum() {
        let r = cosh(Interval::new(-1.0, 1.0, -10));
        assert!((r.lo() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sinh_monotone() {
        let r = sinh(Interval::new(-1.0, 1.0, -10));
        assert!((r.lo() - (-1.0_f64).sinh()).abs() < 1e-9);
        assert!((r.hi() - 1.0_f64.sinh()).abs() < 1e-9);
    }
}
