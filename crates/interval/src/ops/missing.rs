//! Placeholder implementations for operations not yet semantically ported.
//!
//! All functions here return `interval(0)` (= `Interval::new(0,0,0)`) matching
//! the C++ placeholder behaviour in `intervalMissing.cpp`.
//!
//! # Parity notes
//! - `Nil`, `FixPointUpdate`, `Input`, `Output`: always placeholder in C++.
//! - `Attach`, `Highest`, `Lowest`, `BitCast`, `Select2`, `Prefix`: placeholder.
//! - `RDTbl`, `WRTbl`, `Gen`, `SoundFile*`, `Waveform`: placeholder.
//! - `ForeignFunction`, `ForeignVar`, `ForeignConst`: placeholder.
//!
//! These must be preserved as placeholders until full semantics are implemented
//! and validated against C++.

use crate::Interval;

fn zero() -> Interval {
    Interval::new(0.0, 0.0, 0)
}

// -------------------------------------------------------------------------
// Fix-point / IO family
// -------------------------------------------------------------------------

/// C++ `Nil()` → `interval(0)`.
#[must_use]
pub fn nil() -> Interval {
    zero()
}

/// C++ `FixPointUpdate` → `interval(0)`.
#[must_use]
pub fn fix_point_update(_x: Interval, _y: Interval) -> Interval {
    zero()
}

/// C++ `Input` → `interval(0)`.
#[must_use]
pub fn input(_c: Interval) -> Interval {
    zero()
}

/// C++ `Output` → `interval(0)`.
#[must_use]
pub fn output(_c: Interval, _y: Interval) -> Interval {
    zero()
}

// -------------------------------------------------------------------------
// Structural
// -------------------------------------------------------------------------

/// C++ `Attach` → `interval(0)`.
#[must_use]
pub fn attach(_x: Interval, _y: Interval) -> Interval {
    zero()
}

/// C++ `Highest` → `interval(0)`.
#[must_use]
pub fn highest(_x: Interval) -> Interval {
    zero()
}

/// C++ `Lowest` → `interval(0)`.
#[must_use]
pub fn lowest(_x: Interval) -> Interval {
    zero()
}

/// C++ `BitCast` → `interval(0)`.
#[must_use]
pub fn bit_cast(_x: Interval) -> Interval {
    zero()
}

/// C++ `Select2` → `interval(0)`.
#[must_use]
pub fn select2(_sel: Interval, _on_false: Interval, _on_true: Interval) -> Interval {
    zero()
}

/// C++ `Prefix` → `interval(0)`.
#[must_use]
pub fn prefix(_x: Interval, _y: Interval) -> Interval {
    zero()
}

// -------------------------------------------------------------------------
// Table family
// -------------------------------------------------------------------------

/// C++ `RDTbl` → `interval(0)`.
#[must_use]
pub fn rd_tbl(_wtbl: Interval, _ri: Interval) -> Interval {
    zero()
}

/// C++ `WRTbl` → `interval(0)`.
#[must_use]
pub fn wr_tbl(_n: Interval, _g: Interval, _wi: Interval, _ws: Interval) -> Interval {
    zero()
}

/// C++ `Gen` → `interval(0)`.
#[must_use]
pub fn r#gen(_x: Interval) -> Interval {
    zero()
}

// -------------------------------------------------------------------------
// Soundfile family
// -------------------------------------------------------------------------

/// C++ `SoundFile` → `interval(0)`.
#[must_use]
pub fn sound_file(_label: Interval) -> Interval {
    zero()
}

/// C++ `SoundFileRate` → `interval(0)`.
#[must_use]
pub fn sound_file_rate(_sf: Interval, _x: Interval) -> Interval {
    zero()
}

/// C++ `SoundFileLength` → `interval(0)`.
#[must_use]
pub fn sound_file_length(_sf: Interval, _x: Interval) -> Interval {
    zero()
}

/// C++ `SoundFileBuffer` → `interval(0)`.
#[must_use]
pub fn sound_file_buffer(_sf: Interval, _x: Interval, _y: Interval, _z: Interval) -> Interval {
    zero()
}

// -------------------------------------------------------------------------
// Waveform
// -------------------------------------------------------------------------

/// C++ `Waveform` → `interval(0)`.
#[must_use]
pub fn waveform(_w: &[Interval]) -> Interval {
    zero()
}

// -------------------------------------------------------------------------
// Foreign function family
// -------------------------------------------------------------------------

/// C++ `ForeignFunction` → `interval(0)`.
#[must_use]
pub fn foreign_function(_ff: &[Interval]) -> Interval {
    zero()
}

/// C++ `ForeignVar` → `interval(0)`.
#[must_use]
pub fn foreign_var(_type_: Interval, _name: Interval, _file: Interval) -> Interval {
    zero()
}

/// C++ `ForeignConst` → `interval(0)`.
#[must_use]
pub fn foreign_const(_type_: Interval, _name: Interval, _file: Interval) -> Interval {
    zero()
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nil_is_zero() {
        let r = nil();
        assert!(r.is_zero());
    }

    #[test]
    fn all_placeholders_return_zero() {
        let z = Interval::new(0.0, 0.0, 0);
        assert_eq!(fix_point_update(z, z), z);
        assert_eq!(input(z), z);
        assert_eq!(output(z, z), z);
        assert_eq!(attach(z, z), z);
        assert_eq!(highest(z), z);
        assert_eq!(lowest(z), z);
        assert_eq!(bit_cast(z), z);
        assert_eq!(select2(z, z, z), z);
        assert_eq!(prefix(z, z), z);
        assert_eq!(rd_tbl(z, z), z);
        assert_eq!(wr_tbl(z, z, z, z), z);
        assert_eq!(r#gen(z), z);
        assert_eq!(sound_file(z), z);
        assert_eq!(sound_file_rate(z, z), z);
        assert_eq!(sound_file_length(z, z), z);
        assert_eq!(sound_file_buffer(z, z, z, z), z);
        assert_eq!(waveform(&[z, z]), z);
        assert_eq!(foreign_function(&[z]), z);
        assert_eq!(foreign_var(z, z, z), z);
        assert_eq!(foreign_const(z, z, z), z);
    }
}
