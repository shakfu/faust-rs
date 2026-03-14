//! Concrete signal type variants.
//!
//! # C++ source
//! `compiler/signals/sigtype.hh` — `SimpleType`, `TableType`, `TupletType`,
//! `AudioType`.
//!
//! # Design notes
//! - Value semantics: `SigType` is `Clone`; no smart pointers needed.
//! - `PartialEq` **ignores `res`** to match C++ equality semantics, which
//!   ensures convergence of the recursive fixed-point type inference loop.
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
            && self.interval == other.interval
        // res intentionally excluded
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
            && self.nature == other.nature
            && self.variability == other.variability
            && self.computability == other.computability
            && self.vectorability == other.vectorability
            && self.boolean == other.boolean
            && self.interval == other.interval
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
            && self.nature == other.nature
            && self.variability == other.variability
            && self.computability == other.computability
            && self.vectorability == other.vectorability
            && self.boolean == other.boolean
            && self.interval == other.interval
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
