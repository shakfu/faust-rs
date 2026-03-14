//! Uniform accessors and `promote*` methods on `SigType`.
//!
//! # C++ source
//! `AudioType` virtual methods in `sigtype.hh`:
//! `nature()`, `variability()`, `computability()`, `vectorability()`,
//! `boolean()`, `getInterval()`, `isMaximal()`, `promoteNature()`, etc.

use interval::Interval;

use crate::enums::{Boolean, Computability, Nature, Res, Variability, Vectorability};
use crate::types::{SigType, SimpleType, TableType, TupletType};

// ─────────────────────────────────────────────────────────────────────────────
// Read accessors
// ─────────────────────────────────────────────────────────────────────────────

impl SigType {
    /// Nature of the signal values (Int / Real / Any).
    #[inline]
    #[must_use]
    pub fn nature(&self) -> Nature {
        match self {
            SigType::Simple(t) => t.nature,
            SigType::Table(t) => t.nature,
            SigType::Tuplet(t) => t.nature,
        }
    }

    /// Rate of change of signal values (Konst / Block / Samp).
    #[inline]
    #[must_use]
    pub fn variability(&self) -> Variability {
        match self {
            SigType::Simple(t) => t.variability,
            SigType::Table(t) => t.variability,
            SigType::Tuplet(t) => t.variability,
        }
    }

    /// Availability during compilation / execution.
    #[inline]
    #[must_use]
    pub fn computability(&self) -> Computability {
        match self {
            SigType::Simple(t) => t.computability,
            SigType::Table(t) => t.computability,
            SigType::Tuplet(t) => t.computability,
        }
    }

    /// Vectorisability of the signal.
    #[inline]
    #[must_use]
    pub fn vectorability(&self) -> Vectorability {
        match self {
            SigType::Simple(t) => t.vectorability,
            SigType::Table(t) => t.vectorability,
            SigType::Tuplet(t) => t.vectorability,
        }
    }

    /// Whether the signal carries boolean (0/1) values.
    #[inline]
    #[must_use]
    pub fn boolean(&self) -> Boolean {
        match self {
            SigType::Simple(t) => t.boolean,
            SigType::Table(t) => t.boolean,
            SigType::Tuplet(t) => t.boolean,
        }
    }

    /// Value bounds of the signal.
    #[inline]
    #[must_use]
    pub fn interval(&self) -> Interval {
        match self {
            SigType::Simple(t) => t.interval,
            SigType::Table(t) => t.interval,
            SigType::Tuplet(t) => t.interval,
        }
    }

    /// Fixed-point resolution (only meaningful for `SimpleType`).
    #[inline]
    #[must_use]
    pub fn res(&self) -> Res {
        match self {
            SigType::Simple(t) => t.res,
            _ => Res::default(),
        }
    }

    /// `true` iff the type is at the top of the lattice
    /// (`Real`, `Samp`, `Exec`).
    ///
    /// # C++ source
    /// `AudioType::isMaximal()`
    #[must_use]
    pub fn is_maximal(&self) -> bool {
        match self {
            SigType::Simple(t) => {
                t.nature == Nature::Real
                    && t.variability == Variability::Samp
                    && t.computability == Computability::Exec
            }
            SigType::Table(t) => t.nature == Nature::Real && t.variability == Variability::Samp,
            SigType::Tuplet(t) => t.components.iter().all(SigType::is_maximal),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// promote* — return a new SigType with one field raised in the lattice
// ─────────────────────────────────────────────────────────────────────────────

impl SigType {
    /// Return a copy with `nature` raised to `join(current, n)`.
    ///
    /// # C++ source
    /// `AudioType::promoteNature(int n)`
    #[must_use]
    pub fn promote_nature(self, n: Nature) -> Self {
        let current = self.nature();
        let joined = current.join(n);
        self.with_nature(joined)
    }

    /// Return a copy with `variability` raised to `join(current, v)`.
    #[must_use]
    pub fn promote_variability(self, v: Variability) -> Self {
        let current = self.variability();
        let joined = current.join(v);
        self.with_variability(joined)
    }

    /// Return a copy with `computability` raised to `join(current, c)`.
    #[must_use]
    pub fn promote_computability(self, c: Computability) -> Self {
        let current = self.computability();
        let joined = current.join(c);
        self.with_computability(joined)
    }

    /// Return a copy with `vectorability` raised to `join(current, v)`.
    #[must_use]
    pub fn promote_vectorability(self, v: Vectorability) -> Self {
        let current = self.vectorability();
        let joined = current.join(v);
        self.with_vectorability(joined)
    }

    /// Return a copy with `boolean` raised to `join(current, b)`.
    #[must_use]
    pub fn promote_boolean(self, b: Boolean) -> Self {
        let current = self.boolean();
        let joined = current.join(b);
        self.with_boolean(joined)
    }

    /// Return a copy with `interval` replaced by `i`.
    ///
    /// # C++ source
    /// `AudioType::promoteInterval(const interval& i)`
    #[must_use]
    pub fn promote_interval(self, i: Interval) -> Self {
        self.with_interval(i)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal field-replacement helpers
// ─────────────────────────────────────────────────────────────────────────────

impl SigType {
    #[must_use]
    pub(crate) fn with_nature(self, nature: Nature) -> Self {
        match self {
            SigType::Simple(t) => SigType::Simple(SimpleType { nature, ..t }),
            SigType::Table(t) => SigType::Table(TableType { nature, ..t }),
            SigType::Tuplet(t) => SigType::Tuplet(TupletType { nature, ..t }),
        }
    }

    #[must_use]
    pub(crate) fn with_variability(self, variability: Variability) -> Self {
        match self {
            SigType::Simple(t) => SigType::Simple(SimpleType { variability, ..t }),
            SigType::Table(t) => SigType::Table(TableType { variability, ..t }),
            SigType::Tuplet(t) => SigType::Tuplet(TupletType { variability, ..t }),
        }
    }

    #[must_use]
    pub(crate) fn with_computability(self, computability: Computability) -> Self {
        match self {
            SigType::Simple(t) => SigType::Simple(SimpleType { computability, ..t }),
            SigType::Table(t) => SigType::Table(TableType { computability, ..t }),
            SigType::Tuplet(t) => SigType::Tuplet(TupletType { computability, ..t }),
        }
    }

    #[must_use]
    pub(crate) fn with_vectorability(self, vectorability: Vectorability) -> Self {
        match self {
            SigType::Simple(t) => SigType::Simple(SimpleType { vectorability, ..t }),
            SigType::Table(t) => SigType::Table(TableType { vectorability, ..t }),
            SigType::Tuplet(t) => SigType::Tuplet(TupletType { vectorability, ..t }),
        }
    }

    #[must_use]
    pub(crate) fn with_boolean(self, boolean: Boolean) -> Self {
        match self {
            SigType::Simple(t) => SigType::Simple(SimpleType { boolean, ..t }),
            SigType::Table(t) => SigType::Table(TableType { boolean, ..t }),
            SigType::Tuplet(t) => SigType::Tuplet(TupletType { boolean, ..t }),
        }
    }

    #[must_use]
    pub(crate) fn with_interval(self, interval: Interval) -> Self {
        match self {
            SigType::Simple(t) => SigType::Simple(SimpleType { interval, ..t }),
            SigType::Table(t) => SigType::Table(TableType { interval, ..t }),
            SigType::Tuplet(t) => SigType::Tuplet(TupletType { interval, ..t }),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Display
// ─────────────────────────────────────────────────────────────────────────────

impl std::fmt::Display for SigType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Format matches C++ print: nature, variability, computability, boolean, interval
        let nature_ch = match self.nature() {
            Nature::Int => 'N',
            Nature::Real => 'R',
            Nature::Any => 'A',
        };
        let var_ch = match self.variability() {
            Variability::Konst => 'K',
            Variability::Block => 'B',
            Variability::Samp => 'S',
        };
        let comp_ch = match self.computability() {
            Computability::Comp => 'C',
            Computability::Init => 'I',
            Computability::Exec => 'E',
        };
        let vec_ch = match self.vectorability() {
            Vectorability::Vect => 'V',
            Vectorability::Scal => 'S',
            Vectorability::TrueScal => 'T',
        };
        let bool_ch = match self.boolean() {
            Boolean::Num => 'N',
            Boolean::Bool => 'B',
        };
        write!(
            f,
            "{nature_ch}{var_ch}{comp_ch}{vec_ch}{bool_ch} {}",
            self.interval()
        )?;
        if let SigType::Table(t) = self {
            write!(f, ":Table({})", t.content)?;
        }
        if let SigType::Tuplet(t) = self {
            write!(f, ":Tuplet(")?;
            for (i, c) in t.components.iter().enumerate() {
                if i > 0 {
                    write!(f, "*")?;
                }
                write!(f, "{c}")?;
            }
            write!(f, ")")?;
        }
        Ok(())
    }
}
