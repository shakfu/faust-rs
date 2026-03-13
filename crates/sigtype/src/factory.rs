//! Factory functions for constructing `SigType` values.
//!
//! # C++ source
//! `makeSimpleType`, `makeTableType`, `makeTupletType` in `sigtype.cpp`.
//!
//! In C++ these functions use a global memoization table
//! (`gGlobal->gMemoizedTypes`) to ensure pointer identity for equivalent
//! types, enabling O(1) equality checks.  In Rust we use value semantics and
//! structural `PartialEq`, so no memoization is needed.

use interval::Interval;

use crate::enums::{Boolean, Computability, Nature, Res, Variability, Vectorability};
use crate::ops::{merge_boolean, merge_computability, merge_interval, merge_nature,
                 merge_variability, merge_vectorability};
use crate::types::{SimpleType, SigType, TableType, TupletType};

// ─────────────────────────────────────────────────────────────────────────────
// SimpleType factory
// ─────────────────────────────────────────────────────────────────────────────

/// Construct a `SimpleType` with default `Res`.
///
/// # C++ source
/// `makeSimpleType(int n, int v, int c, int vec, int b, const interval& i)`
#[must_use]
pub fn make_simple(
    nature: Nature,
    variability: Variability,
    computability: Computability,
    vectorability: Vectorability,
    boolean: Boolean,
    interval: Interval,
) -> SigType {
    SigType::Simple(SimpleType {
        nature,
        variability,
        computability,
        vectorability,
        boolean,
        interval,
        res: Res::default(),
    })
}

/// Construct a `SimpleType` with an explicit `Res`.
///
/// # C++ source
/// `makeSimpleType(int n, int v, int c, int vec, int b, const interval& i, const res& lsb)`
#[must_use]
pub fn make_simple_with_res(
    nature: Nature,
    variability: Variability,
    computability: Computability,
    vectorability: Vectorability,
    boolean: Boolean,
    interval: Interval,
    res: Res,
) -> SigType {
    SigType::Simple(SimpleType { nature, variability, computability, vectorability, boolean, interval, res })
}

// ─────────────────────────────────────────────────────────────────────────────
// TableType factory
// ─────────────────────────────────────────────────────────────────────────────

/// Construct a `TableType` whose aggregate qualifiers are derived from
/// `content`.
///
/// # C++ source
/// `makeTableType(const Type& ct)` (basic constructor)
///
/// ```text
/// TableType(t): nature = t.nature, variability = kKonst, computability = kInit,
///              vectorability = kVect, boolean = kNum, interval = t.interval
/// ```
#[must_use]
pub fn make_table_type(content: SigType) -> SigType {
    let nature        = content.nature();
    let variability   = Variability::Konst;
    let computability = Computability::Init;
    let vectorability = Vectorability::Vect;
    let boolean       = Boolean::Num;
    let interval      = content.interval();
    SigType::Table(TableType {
        content: Box::new(content),
        nature,
        variability,
        computability,
        vectorability,
        boolean,
        interval,
    })
}

/// Construct a `TableType` with explicit aggregate qualifiers.
///
/// # C++ source
/// `makeTableType(const Type& ct, int n, int v, int c, int vec, int b, const interval& i)`
#[must_use]
pub fn make_table_type_with(
    content: SigType,
    nature: Nature,
    variability: Variability,
    computability: Computability,
    vectorability: Vectorability,
    boolean: Boolean,
    interval: Interval,
) -> SigType {
    SigType::Table(TableType {
        content: Box::new(content),
        nature, variability, computability, vectorability, boolean, interval,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// TupletType factory
// ─────────────────────────────────────────────────────────────────────────────

/// Construct a `TupletType` by aggregating the lattice qualifiers of all
/// components.
///
/// An empty `components` slice is valid (empty tuplet).
///
/// # C++ source
/// `makeTupletType(ConstTypes vt)`
#[must_use]
pub fn make_tuplet(components: Vec<SigType>) -> SigType {
    let nature        = merge_nature(&components);
    let variability   = merge_variability(&components);
    let computability = merge_computability(&components);
    let vectorability = merge_vectorability(&components);
    let boolean       = merge_boolean(&components);
    let interval      = merge_interval(&components);
    SigType::Tuplet(TupletType {
        components,
        nature,
        variability,
        computability,
        vectorability,
        boolean,
        interval,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Convenience constructor: maximal type
// ─────────────────────────────────────────────────────────────────────────────

/// The top element of the type lattice: `Real, Samp, Exec, Vect, Num`.
///
/// Used to initialise recursive types before the fixed-point loop.
#[must_use]
pub fn make_maximal() -> SigType {
    make_simple(
        Nature::Real,
        Variability::Samp,
        Computability::Exec,
        Vectorability::Vect,
        Boolean::Num,
        interval::Interval::new_default(),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::*;

    #[test]
    fn make_simple_round_trip() {
        let itv = interval::singleton(1.0);
        let t = make_simple(Nature::Int, Variability::Konst, Computability::Comp,
                            Vectorability::Vect, Boolean::Num, itv);
        assert_eq!(t.nature(), Nature::Int);
        assert_eq!(t.variability(), Variability::Konst);
        assert_eq!(t.interval(), itv);
    }

    #[test]
    fn make_table_derives_from_content() {
        let content = make_simple(Nature::Real, Variability::Konst, Computability::Comp,
                                  Vectorability::Vect, Boolean::Num,
                                  interval::singleton(0.5));
        let tbl = make_table_type(content);
        // Table basic ctor freezes variability to Konst, computability to Init
        assert_eq!(tbl.nature(), Nature::Real);
        assert_eq!(tbl.variability(), Variability::Konst);
        assert_eq!(tbl.computability(), Computability::Init);
    }

    #[test]
    fn make_tuplet_aggregates() {
        let a = make_simple(Nature::Int,  Variability::Konst, Computability::Comp,
                            Vectorability::Vect, Boolean::Num, interval::singleton(0.0));
        let b = make_simple(Nature::Real, Variability::Samp,  Computability::Exec,
                            Vectorability::Vect, Boolean::Num, interval::singleton(1.0));
        let tup = make_tuplet(vec![a, b]);
        assert_eq!(tup.nature(), Nature::Real);
        assert_eq!(tup.variability(), Variability::Samp);
        assert_eq!(tup.computability(), Computability::Exec);
    }

    #[test]
    fn make_maximal_is_maximal() {
        assert!(make_maximal().is_maximal());
    }
}
