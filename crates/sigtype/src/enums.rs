//! Lattice enums and resolution struct for the Faust signal type system.
//!
//! # C++ source
//! `compiler/signals/sigtype.hh` — `Nature`, `Variability`, `Computability`,
//! `Vectorability`, `Boolean` enums and `res` struct.
//!
//! # Lattice semantics
//! All five enums form a join-semilattice ordered by information content.
//! `join` is bitwise OR on the underlying `u8` discriminant.
//! The discriminant values are chosen so that `a | b` always maps to a valid
//! variant (gaps at 2 for three-level enums are harmless because the only
//! reachable values after OR are 0, 1, and 3).

/// Nature of the signal values — integer vs. floating-point.
///
/// `Any` is a wildcard used only with foreign functions (`ffunction`);
/// no signal ever carries `Any` after type inference.
///
/// # C++ source
/// `enum Nature { kInt = 0, kReal = 1, kAny = 2 }`
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Nature {
    Int = 0,
    Real = 1,
    Any = 2,
}

impl Nature {
    /// Lattice join — equivalent to C++ `n1 | n2` on `int` fields.
    #[inline]
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        Self::from_u8((self as u8) | (other as u8))
    }

    #[inline]
    #[must_use]
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Int,
            1 => Self::Real,
            _ => Self::Any,
        }
    }
}

/// Rate at which signal values change.
///
/// | Variant | Meaning              | C++ constant |
/// |---------|----------------------|--------------|
/// | `Konst` | Compile-time constant | `kKonst = 0` |
/// | `Block` | Changes per block     | `kBlock = 1` |
/// | `Samp`  | Changes per sample    | `kSamp  = 3` |
///
/// The gap at 2 is intentional: `Block | Samp = 1 | 3 = 3 = Samp`. ✓
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Variability {
    Konst = 0,
    Block = 1,
    Samp = 3,
}

impl Variability {
    #[inline]
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        Self::from_u8((self as u8) | (other as u8))
    }

    #[inline]
    #[must_use]
    pub(crate) fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Konst,
            1 => Self::Block,
            _ => Self::Samp,
        }
    }
}

/// When signal values become available during compilation / execution.
///
/// | Variant | Meaning           | C++ constant |
/// |---------|-------------------|--------------|
/// | `Comp`  | Compile-time      | `kComp = 0`  |
/// | `Init`  | Initialisation    | `kInit = 1`  |
/// | `Exec`  | Runtime execution | `kExec = 3`  |
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Computability {
    Comp = 0,
    Init = 1,
    Exec = 3,
}

impl Computability {
    #[inline]
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        Self::from_u8((self as u8) | (other as u8))
    }

    #[inline]
    #[must_use]
    pub(crate) fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Comp,
            1 => Self::Init,
            _ => Self::Exec,
        }
    }
}

/// Whether the signal can be vectorised.
///
/// | Variant    | C++ constant    |
/// |------------|-----------------|
/// | `Vect`     | `kVect     = 0` |
/// | `Scal`     | `kScal     = 1` |
/// | `TrueScal` | `kTrueScal = 3` |
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Vectorability {
    Vect = 0,
    Scal = 1,
    TrueScal = 3,
}

impl Vectorability {
    #[inline]
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        Self::from_u8((self as u8) | (other as u8))
    }

    #[inline]
    #[must_use]
    pub(crate) fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Vect,
            1 => Self::Scal,
            _ => Self::TrueScal,
        }
    }
}

/// Whether the signal carries a boolean (0/1) or a general numeric value.
///
/// | Variant | C++ constant |
/// |---------|--------------|
/// | `Num`   | `kNum  = 0`  |
/// | `Bool`  | `kBool = 1`  |
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Boolean {
    Num = 0,
    Bool = 1,
}

impl Boolean {
    #[inline]
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        if (self as u8) | (other as u8) == 0 {
            Self::Num
        } else {
            Self::Bool
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Res — fixed-point precision
// ─────────────────────────────────────────────────────────────────────────────

/// Fixed-point resolution: position of the least significant bit.
///
/// # C++ source
/// `struct res` in `sigtype.hh`.
///
/// `valid = false` means the resolution has not been computed yet.
/// `index` is the LSB bit position (may be negative for fractional bits).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Res {
    pub valid: bool,
    pub index: i32,
}

impl Res {
    #[must_use]
    pub fn new(index: i32) -> Self {
        Self { valid: true, index }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nature_join() {
        assert_eq!(Nature::Int.join(Nature::Real), Nature::Real);
        assert_eq!(Nature::Real.join(Nature::Int), Nature::Real);
        assert_eq!(Nature::Int.join(Nature::Int), Nature::Int);
        assert_eq!(Nature::Real.join(Nature::Any), Nature::Any);
    }

    #[test]
    fn variability_join() {
        assert_eq!(
            Variability::Konst.join(Variability::Block),
            Variability::Block
        );
        assert_eq!(
            Variability::Block.join(Variability::Samp),
            Variability::Samp
        );
        assert_eq!(
            Variability::Konst.join(Variability::Samp),
            Variability::Samp
        );
        assert_eq!(Variability::Samp.join(Variability::Samp), Variability::Samp);
    }

    #[test]
    fn computability_join() {
        assert_eq!(
            Computability::Comp.join(Computability::Init),
            Computability::Init
        );
        assert_eq!(
            Computability::Init.join(Computability::Exec),
            Computability::Exec
        );
        assert_eq!(
            Computability::Comp.join(Computability::Exec),
            Computability::Exec
        );
    }

    #[test]
    fn vectorability_join() {
        assert_eq!(
            Vectorability::Vect.join(Vectorability::Scal),
            Vectorability::Scal
        );
        assert_eq!(
            Vectorability::Scal.join(Vectorability::TrueScal),
            Vectorability::TrueScal
        );
        assert_eq!(
            Vectorability::Vect.join(Vectorability::TrueScal),
            Vectorability::TrueScal
        );
    }

    #[test]
    fn boolean_join() {
        assert_eq!(Boolean::Num.join(Boolean::Bool), Boolean::Bool);
        assert_eq!(Boolean::Num.join(Boolean::Num), Boolean::Num);
    }

    #[test]
    fn res_default_is_invalid() {
        let r = Res::default();
        assert!(!r.valid);
    }

    #[test]
    fn res_new_is_valid() {
        let r = Res::new(-3);
        assert!(r.valid);
        assert_eq!(r.index, -3);
    }
}
