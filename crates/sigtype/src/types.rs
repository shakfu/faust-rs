//! Concrete signal type variants.
//!
//! # C++ source
//! `compiler/signals/sigtype.hh` — `SimpleType`, `TableType`, `TupletType`,
//! `AudioType`.
//!
//! # Design notes
//! - Value semantics: `SigType` is `Clone`; no smart pointers needed.
//! - `PartialEq` matches the C++ `operator==(Type, Type)` semantics:
//!   `SimpleType` ignores `res` and `lsb`, `TableType` compares only content,
//!   and `TupletType` compares only components. This looser equality is
//!   required for recursive type inference to converge like the C++ compiler.
//! - Memoization by pointer identity (C++ `P<AudioType>`) is replaced by
//!   structural `PartialEq`.

use interval::Interval;

use crate::enums::{Boolean, Computability, Nature, Res, Variability, Vectorability};

// ─────────────────────────────────────────────────────────────────────────────
// SimpleType
// ─────────────────────────────────────────────────────────────────────────────

/// A scalar signal type carrying all five lattice qualifiers plus interval and
/// fixed-point resolution.
///
/// # C++ source
/// `class SimpleType : public AudioType`
#[derive(Clone, Debug)]
pub struct SimpleType {
    pub nature: Nature,
    pub variability: Variability,
    pub computability: Computability,
    pub vectorability: Vectorability,
    pub boolean: Boolean,
    pub interval: Interval,
    /// Fixed-point resolution — ignored by `PartialEq` for convergence.
    pub res: Res,
}

impl PartialEq for SimpleType {
    fn eq(&self, other: &Self) -> bool {
        self.nature == other.nature
            && self.variability == other.variability
            && self.computability == other.computability
            && self.vectorability == other.vectorability
            && self.boolean == other.boolean
            && self.interval.lo() == other.interval.lo()
            && self.interval.hi() == other.interval.hi()
        // `res` and interval `lsb` intentionally excluded to match C++.
    }
}

impl Eq for SimpleType {}

// ─────────────────────────────────────────────────────────────────────────────
// TableType
// ─────────────────────────────────────────────────────────────────────────────

/// A signal type representing a readable/writable table.
///
/// The aggregate lattice qualifiers are derived from the content type during
/// construction (see `crate::factory::make_table_type`).
///
/// # C++ source
/// `class TableType : public AudioType`
#[derive(Clone, Debug)]
pub struct TableType {
    /// Element type stored in the table.
    pub content: Box<SigType>,
    pub nature: Nature,
    pub variability: Variability,
    pub computability: Computability,
    pub vectorability: Vectorability,
    pub boolean: Boolean,
    pub interval: Interval,
}

impl PartialEq for TableType {
    fn eq(&self, other: &Self) -> bool {
        self.content == other.content
    }
}

impl Eq for TableType {}

// ─────────────────────────────────────────────────────────────────────────────
// TupletType
// ─────────────────────────────────────────────────────────────────────────────

/// A product type grouping multiple signal types (used for recursive groups).
///
/// The aggregate qualifiers are the `join` of all component qualifiers.
///
/// # C++ source
/// `class TupletType : public AudioType`
#[derive(Clone, Debug)]
pub struct TupletType {
    /// Component types in order.
    pub components: Vec<SigType>,
    /// Aggregate nature (join of all components).
    pub nature: Nature,
    /// Aggregate variability.
    pub variability: Variability,
    /// Aggregate computability.
    pub computability: Computability,
    /// Aggregate vectorability.
    pub vectorability: Vectorability,
    /// Aggregate boolean.
    pub boolean: Boolean,
    /// Reunion of all component intervals.
    pub interval: Interval,
}

impl PartialEq for TupletType {
    fn eq(&self, other: &Self) -> bool {
        self.components == other.components
    }
}

impl Eq for TupletType {}

// ─────────────────────────────────────────────────────────────────────────────
// SigType — unified enum
// ─────────────────────────────────────────────────────────────────────────────

/// Unified signal type — either a scalar, a table, or a tuplet.
///
/// Replaces the C++ `P<AudioType>` smart pointer / dynamic-dispatch hierarchy
/// with a plain Rust enum.  Comparisons are structural (ignoring `res`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SigType {
    Simple(SimpleType),
    Table(TableType),
    Tuplet(TupletType),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_type_equality_ignores_lsb_and_res_like_cpp() {
        let a = SimpleType {
            nature: Nature::Real,
            variability: Variability::Samp,
            computability: Computability::Exec,
            vectorability: Vectorability::Vect,
            boolean: Boolean::Num,
            interval: Interval::new(-1.0, 1.0, -24),
            res: Res::new(-24),
        };
        let b = SimpleType {
            interval: Interval::new(-1.0, 1.0, -512),
            res: Res::new(-512),
            ..a.clone()
        };
        assert_eq!(a, b);
    }

    #[test]
    fn table_type_equality_only_checks_content_like_cpp() {
        let content = Box::new(SigType::Simple(SimpleType {
            nature: Nature::Int,
            variability: Variability::Konst,
            computability: Computability::Comp,
            vectorability: Vectorability::Vect,
            boolean: Boolean::Num,
            interval: Interval::new(0.0, 7.0, 0),
            res: Res::default(),
        }));
        let a = TableType {
            content: content.clone(),
            nature: Nature::Int,
            variability: Variability::Konst,
            computability: Computability::Init,
            vectorability: Vectorability::Vect,
            boolean: Boolean::Num,
            interval: Interval::new(0.0, 7.0, 0),
        };
        let b = TableType {
            content,
            nature: Nature::Real,
            variability: Variability::Samp,
            computability: Computability::Exec,
            vectorability: Vectorability::Scal,
            boolean: Boolean::Bool,
            interval: Interval::new(-1.0, 1.0, -24),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn tuplet_type_equality_only_checks_components_like_cpp() {
        let component = SigType::Simple(SimpleType {
            nature: Nature::Int,
            variability: Variability::Konst,
            computability: Computability::Comp,
            vectorability: Vectorability::Vect,
            boolean: Boolean::Num,
            interval: Interval::new(0.0, 1.0, 0),
            res: Res::default(),
        });
        let a = TupletType {
            components: vec![component.clone()],
            nature: Nature::Int,
            variability: Variability::Konst,
            computability: Computability::Comp,
            vectorability: Vectorability::Vect,
            boolean: Boolean::Num,
            interval: Interval::new(0.0, 1.0, 0),
        };
        let b = TupletType {
            components: vec![component],
            nature: Nature::Real,
            variability: Variability::Samp,
            computability: Computability::Exec,
            vectorability: Vectorability::Scal,
            boolean: Boolean::Bool,
            interval: Interval::new(-1.0, 1.0, -24),
        };
        assert_eq!(a, b);
    }
}
