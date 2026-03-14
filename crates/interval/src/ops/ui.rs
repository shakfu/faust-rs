//! User interface element interval operators.
//!
//! # C++ source
//! `intervalButton.cpp`, `intervalCheckbox.cpp`,
//! `intervalVSlider.cpp`, `intervalHSlider.cpp`, `intervalNumEntry.cpp`,
//! `intervalMissing.cpp` (HBargraph, VBargraph)

use crate::{Interval, empty};

// -------------------------------------------------------------------------
// Button / Checkbox
// -------------------------------------------------------------------------

/// Button: returns `[0, 1]` with 0-bit precision.
///
/// # C++ source
/// `intervalButton.cpp`
#[must_use]
pub fn button(_name: Interval) -> Interval {
    Interval::new(0.0, 1.0, 0)
}

/// Checkbox: returns `[0, 1]` with 0-bit precision.
///
/// # C++ source
/// `intervalCheckbox.cpp`
#[must_use]
pub fn checkbox(_name: Interval) -> Interval {
    Interval::new(0.0, 1.0, 0)
}

// -------------------------------------------------------------------------
// Sliders
// -------------------------------------------------------------------------

/// Slider precision helper: the precision needed to represent slider values.
///
/// Elements are of the form `lo + k*step ≤ hi` (k integer).
/// Precision = min of step lsb, lo lsb, and log2(step.lo) when step > 0.
fn slider_lsb(lo: Interval, step: Interval) -> i32 {
    let mut lsb = step.lsb().min(lo.lsb());
    if step.lo() > 0.0 {
        lsb = lsb.min(step.lo().log2() as i32);
    }
    lsb
}

/// VSlider: returns `[lo.lo, hi.hi]` with slider precision.
///
/// # C++ source
/// `intervalVSlider.cpp`
#[must_use]
pub fn vslider(
    _name: Interval,
    _init: Interval,
    lo: Interval,
    hi: Interval,
    step: Interval,
) -> Interval {
    if _init.is_empty() || lo.is_empty() || hi.is_empty() || step.is_empty() {
        return empty();
    }
    Interval::new(lo.lo(), hi.hi(), slider_lsb(lo, step))
}

/// HSlider: same semantics as VSlider.
///
/// # C++ source
/// `intervalHSlider.cpp`
#[must_use]
pub fn hslider(
    _name: Interval,
    _init: Interval,
    lo: Interval,
    hi: Interval,
    step: Interval,
) -> Interval {
    vslider(_name, _init, lo, hi, step)
}

/// NumEntry: same semantics as VSlider.
///
/// # C++ source
/// `intervalNumEntry.cpp`
#[must_use]
pub fn num_entry(
    _name: Interval,
    _init: Interval,
    lo: Interval,
    hi: Interval,
    step: Interval,
) -> Interval {
    vslider(_name, _init, lo, hi, step)
}

// -------------------------------------------------------------------------
// Bargraphs (placeholder — C++ returns interval(0))
// -------------------------------------------------------------------------

/// HBargraph — placeholder returning zero singleton.
///
/// # C++ source
/// `intervalMissing.cpp`
#[must_use]
pub fn hbargraph(_name: Interval, _lo: Interval, _hi: Interval) -> Interval {
    Interval::new(0.0, 0.0, 0)
}

/// VBargraph — placeholder returning zero singleton.
///
/// # C++ source
/// `intervalMissing.cpp`
#[must_use]
pub fn vbargraph(_name: Interval, _lo: Interval, _hi: Interval) -> Interval {
    Interval::new(0.0, 0.0, 0)
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Interval, empty};

    #[test]
    fn button_range() {
        let r = button(empty());
        assert_eq!(r.lo(), 0.0);
        assert_eq!(r.hi(), 1.0);
        assert_eq!(r.lsb(), 0);
    }

    #[test]
    fn vslider_range() {
        let name = empty();
        let init = Interval::new(0.5, 0.5, -10);
        let lo = Interval::new(0.0, 0.0, -10);
        let hi = Interval::new(1.0, 1.0, -10);
        let step = Interval::new(0.1, 0.1, -10);
        let r = vslider(name, init, lo, hi, step);
        assert_eq!(r.lo(), 0.0);
        assert_eq!(r.hi(), 1.0);
    }

    #[test]
    fn vslider_empty_lo() {
        let name = empty();
        let r = vslider(
            name,
            empty(),
            empty(),
            Interval::new(1.0, 1.0, 0),
            Interval::new(0.1, 0.1, 0),
        );
        assert!(r.is_empty());
    }
}
