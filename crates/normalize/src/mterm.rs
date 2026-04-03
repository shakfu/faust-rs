//! Multiplicative term: `k · x₁^n₁ · x₂^n₂ · ... / (y₁^m₁ · y₂^m₂ · ...)`
//!
//! Ported from C++ `compiler/normalize/mterm.cpp`.
// Internal module — functions will be wired into the simplify/normalform pipeline in Phase 2.
#![allow(dead_code)]
//!
//! # Invariant
//! - The constant coefficient is always in `coef` (a numeric `SigId`).
//! - Non-constant factors live in `factors` with signed integer exponents
//!   (positive = numerator, negative = denominator, zero = removed by `cleanup`).
//!
//! # API mapping status
//! - `mterm()` → [`Mterm::zero`]
//! - `mterm(int k)` / `mterm(double k)` → [`Mterm::from_int`] / [`Mterm::from_real`]
//! - `mterm(Tree t)` → [`Mterm::from_sig`]
//! - `operator*=(Tree)` / `operator/=(Tree)` → [`Mterm::mul_sig`] / [`Mterm::div_sig`]
//! - `operator*=(mterm)` / `operator/=(mterm)` → [`Mterm::mul_mterm`] / [`Mterm::div_mterm`]
//! - `operator+=(mterm)` / `operator-=(mterm)` → [`Mterm::add_mterm`] / [`Mterm::sub_mterm`]
//! - `normalizedTree` → [`Mterm::normalized_tree`]
//! - `signatureTree` → [`Mterm::signature_tree`]
//! - `hasDivisor` → [`Mterm::has_divisor`]
//! - `complexity` → [`Mterm::complexity`]
//! - `gcd(m1,m2)` → [`gcd`]

use std::collections::{BTreeMap, HashMap};

use signals::{BinOp, SigBuilder, SigId, SigMatch, match_sig};
use sigtype::{SigType, Variability};
use tlib::TreeArena;

// ─── Order helper ─────────────────────────────────────────────────────────────

/// Return the variability order of a signal node.
///
/// Maps `Variability::Konst → 0`, `Block → 1`, `Samp → 2`.
/// Defaults to 2 (most conservative = audio-rate) when the signal has no
/// type annotation or its type is not a simple type.
///
/// C++ equivalent: `getSigOrder(t)` from `sigorderrules.hh`.
pub(crate) fn sig_order(types: &HashMap<SigId, SigType>, sig: SigId) -> u8 {
    match types.get(&sig).map(|t| t.variability()) {
        Some(Variability::Konst) => 0,
        Some(Variability::Block) => 1,
        _ => 2,
    }
}

// ─── Numeric helpers ──────────────────────────────────────────────────────────

/// True if `sig` is an integer or real-valued leaf constant.
pub(crate) fn is_num(arena: &TreeArena, sig: SigId) -> bool {
    matches!(match_sig(arena, sig), SigMatch::Int(_) | SigMatch::Real(_))
}

/// True if `sig` is the constant zero (integer 0 or real 0.0).
pub(crate) fn is_zero(arena: &TreeArena, sig: SigId) -> bool {
    match match_sig(arena, sig) {
        SigMatch::Int(0) => true,
        SigMatch::Real(r) => r == 0.0,
        _ => false,
    }
}

/// True if `sig` is the constant one (integer 1 or real 1.0).
pub(crate) fn is_one(arena: &TreeArena, sig: SigId) -> bool {
    match match_sig(arena, sig) {
        SigMatch::Int(1) => true,
        SigMatch::Real(r) => r == 1.0,
        _ => false,
    }
}

/// True if `sig` is the constant −1 (integer −1 or real −1.0).
pub(crate) fn is_minus_one(arena: &TreeArena, sig: SigId) -> bool {
    match match_sig(arena, sig) {
        SigMatch::Int(-1) => true,
        SigMatch::Real(r) => r == -1.0,
        _ => false,
    }
}

/// True if the numeric constant `sig` is strictly negative (< 0).
pub(crate) fn is_negative_num(arena: &TreeArena, sig: SigId) -> bool {
    match match_sig(arena, sig) {
        SigMatch::Int(n) => n < 0,
        SigMatch::Real(r) => r < 0.0,
        _ => false,
    }
}

/// True if the numeric constant `sig` is ≥ 0.
pub(crate) fn is_ge_zero(arena: &TreeArena, sig: SigId) -> bool {
    match match_sig(arena, sig) {
        SigMatch::Int(n) => n >= 0,
        SigMatch::Real(r) => r >= 0.0,
        _ => false,
    }
}

/// True if `a` and `b` numeric constants have the same absolute value.
///
/// C++ equivalent: `sameMagnitude(a, b)`.
pub(crate) fn same_magnitude(arena: &TreeArena, a: SigId, b: SigId) -> bool {
    match (match_sig(arena, a), match_sig(arena, b)) {
        (SigMatch::Int(x), SigMatch::Int(y)) => x.abs() == y.abs(),
        (SigMatch::Real(x), SigMatch::Real(y)) => x.abs() == y.abs(),
        (SigMatch::Int(x), SigMatch::Real(y)) => (x.abs() as f64) == y.abs(),
        (SigMatch::Real(x), SigMatch::Int(y)) => x.abs() == (y.abs() as f64),
        _ => false,
    }
}

/// Negate a numeric constant node.
///
/// C++ equivalent: `minusNum(sig)`.
pub(crate) fn minus_num(arena: &mut TreeArena, sig: SigId) -> SigId {
    enum V {
        I(i32),
        F(f64),
    }
    let v = match match_sig(arena, sig) {
        SigMatch::Int(n) => V::I(-n),
        SigMatch::Real(r) => V::F(-r),
        _ => panic!("minus_num: not a numeric constant"),
    };
    let mut b = SigBuilder::new(arena);
    match v {
        V::I(n) => b.int(n),
        V::F(r) => b.real(r),
    }
}

/// Add two numeric constant nodes and return the result.
///
/// C++ equivalent: `addNums(a, b)`.
pub(crate) fn add_nums(arena: &mut TreeArena, a: SigId, b: SigId) -> SigId {
    enum V {
        I(i32),
        F(f64),
    }
    let (va, vb) = match (match_sig(arena, a), match_sig(arena, b)) {
        (SigMatch::Int(x), SigMatch::Int(y)) => (V::I(x), V::I(y)),
        (SigMatch::Real(x), SigMatch::Int(y)) => (V::F(x), V::F(y as f64)),
        (SigMatch::Int(x), SigMatch::Real(y)) => (V::F(x as f64), V::F(y)),
        (SigMatch::Real(x), SigMatch::Real(y)) => (V::F(x), V::F(y)),
        _ => panic!("add_nums: not numeric"),
    };
    let mut b = SigBuilder::new(arena);
    match (va, vb) {
        (V::I(x), V::I(y)) => b.int(x.wrapping_add(y)),
        (V::F(x), V::F(y)) => b.real(x + y),
        _ => unreachable!(),
    }
}

/// Subtract two numeric constant nodes and return the result.
///
/// C++ equivalent: `subNums(a, b)`.
pub(crate) fn sub_nums(arena: &mut TreeArena, a: SigId, b: SigId) -> SigId {
    enum V {
        I(i32),
        F(f64),
    }
    let (va, vb) = match (match_sig(arena, a), match_sig(arena, b)) {
        (SigMatch::Int(x), SigMatch::Int(y)) => (V::I(x), V::I(y)),
        (SigMatch::Real(x), SigMatch::Int(y)) => (V::F(x), V::F(y as f64)),
        (SigMatch::Int(x), SigMatch::Real(y)) => (V::F(x as f64), V::F(y)),
        (SigMatch::Real(x), SigMatch::Real(y)) => (V::F(x), V::F(y)),
        _ => panic!("sub_nums: not numeric"),
    };
    let mut b = SigBuilder::new(arena);
    match (va, vb) {
        (V::I(x), V::I(y)) => b.int(x.wrapping_sub(y)),
        (V::F(x), V::F(y)) => b.real(x - y),
        _ => unreachable!(),
    }
}

/// Multiply two numeric constant nodes and return the result.
///
/// C++ equivalent: `mulNums(a, b)`.
pub(crate) fn mul_nums(arena: &mut TreeArena, a: SigId, b: SigId) -> SigId {
    enum V {
        I(i32),
        F(f64),
    }
    let (va, vb) = match (match_sig(arena, a), match_sig(arena, b)) {
        (SigMatch::Int(x), SigMatch::Int(y)) => (V::I(x), V::I(y)),
        (SigMatch::Real(x), SigMatch::Int(y)) => (V::F(x), V::F(y as f64)),
        (SigMatch::Int(x), SigMatch::Real(y)) => (V::F(x as f64), V::F(y)),
        (SigMatch::Real(x), SigMatch::Real(y)) => (V::F(x), V::F(y)),
        _ => panic!("mul_nums: not numeric"),
    };
    let mut b = SigBuilder::new(arena);
    match (va, vb) {
        (V::I(x), V::I(y)) => b.int(x.wrapping_mul(y)),
        (V::F(x), V::F(y)) => b.real(x * y),
        _ => unreachable!(),
    }
}

/// Divide two numeric constant nodes.
///
/// Returns an integer if the division is exact (no remainder), otherwise promotes
/// to float. Panics on division by zero.
///
/// C++ equivalent: `divExtendedNums(a, b)`.
pub(crate) fn div_nums(arena: &mut TreeArena, a: SigId, b: SigId) -> SigId {
    enum V {
        I(i32),
        F(f64),
    }
    let (va, vb) = match (match_sig(arena, a), match_sig(arena, b)) {
        (SigMatch::Int(x), SigMatch::Int(y)) => (V::I(x), V::I(y)),
        (SigMatch::Real(x), SigMatch::Int(y)) => (V::F(x), V::F(y as f64)),
        (SigMatch::Int(x), SigMatch::Real(y)) => (V::F(x as f64), V::F(y)),
        (SigMatch::Real(x), SigMatch::Real(y)) => (V::F(x), V::F(y)),
        _ => panic!("div_nums: not numeric"),
    };
    let mut b = SigBuilder::new(arena);
    match (va, vb) {
        (V::I(x), V::I(y)) => {
            assert!(y != 0, "div_nums: division by zero");
            if x % y == 0 {
                b.int(x / y)
            } else {
                b.real(x as f64 / y as f64)
            }
        }
        (V::F(x), V::F(y)) => {
            assert!(y != 0.0, "div_nums: division by zero");
            b.real(x / y)
        }
        _ => unreachable!(),
    }
}

// ─── Sig-pow helpers ──────────────────────────────────────────────────────────

/// If `sig` is `pow(x, n)` with `n` an integer literal, return `Some((x, n))`.
///
/// C++ equivalent: `isSigPow(sig, x, n)`.
pub(crate) fn is_sig_pow(arena: &TreeArena, sig: SigId) -> Option<(SigId, i32)> {
    if let SigMatch::Pow(x, y) = match_sig(arena, sig)
        && let SigMatch::Int(n) = match_sig(arena, y)
    {
        return Some((x, n));
    }
    None
}

/// Construct the node `pow(x, p)`.  Returns `x` itself when `p == 1`.
///
/// C++ equivalent: `sigPow(x, p)`.
pub(crate) fn sig_pow(arena: &mut TreeArena, x: SigId, p: i32) -> SigId {
    if p == 1 {
        return x;
    }
    let n = SigBuilder::new(arena).int(p);
    SigBuilder::new(arena).pow(x, n)
}

// ─── Tree-combination helpers ─────────────────────────────────────────────────

/// Grow a numerator accumulator: `r = r * a`  (or `r = a` if `r` was empty).
fn combine_mul_left(arena: &mut TreeArena, r: &mut Option<SigId>, a: SigId) {
    *r = Some(match *r {
        Some(existing) => SigBuilder::new(arena).mul(existing, a),
        None => a,
    });
}

/// Grow a denominator accumulator: `r = r / a`  (or `r = 1/a` if `r` was empty).
fn combine_div_left(arena: &mut TreeArena, r: &mut Option<SigId>, a: SigId) {
    *r = Some(match *r {
        Some(existing) => SigBuilder::new(arena).div(existing, a),
        None => {
            let one = SigBuilder::new(arena).real(1.0);
            SigBuilder::new(arena).div(one, a)
        }
    });
}

/// Dispatch `f^q` into the numerator (`q>0`) or denominator (`q<0`) of `m`/`d`.
fn combine_mul_div(
    arena: &mut TreeArena,
    m: &mut Option<SigId>,
    d: &mut Option<SigId>,
    f: SigId,
    q: i32,
) {
    debug_assert!(q != 0);
    if q > 0 {
        let pow_term = build_pow_term(arena, f, q);
        combine_mul_left(arena, m, pow_term);
    } else {
        let pow_term = build_pow_term(arena, f, -q);
        combine_mul_left(arena, d, pow_term);
    }
}

/// `f^q` → `pow(f,q)` for `q>1`, or `f` for `q==1`.
fn build_pow_term(arena: &mut TreeArena, f: SigId, q: i32) -> SigId {
    debug_assert!(q > 0);
    if q > 1 { sig_pow(arena, f, q) } else { f }
}

// ─── Mterm ────────────────────────────────────────────────────────────────────

/// A multiplicative term of the form `k · x₁^n₁ · x₂^n₂ · ...`
///
/// Ported from C++ `mterm` class (`compiler/normalize/mterm.hh`).
///
/// The coefficient `coef` is a numeric `SigId` (integer or real constant).
/// `factors` maps each non-constant base signal to its signed integer exponent.
/// Zero exponents are removed by [`Mterm::cleanup`].
#[derive(Clone, Debug)]
pub(crate) struct Mterm {
    /// Numeric constant coefficient (SigInt or SigReal). C++ `fCoef`.
    pub(crate) coef: SigId,
    /// Non-constant factors with signed exponents. C++ `fFactors : map<Tree,int>`.
    /// Uses `BTreeMap` for deterministic order matching `std::map`.
    pub(crate) factors: BTreeMap<SigId, i32>,
}

impl Mterm {
    // ── Constructors ──────────────────────────────────────────────────────────

    /// Create the zero mterm (`k = 0`, no factors).  C++ `mterm()`.
    pub(crate) fn zero(arena: &mut TreeArena) -> Self {
        let coef = SigBuilder::new(arena).int(0);
        Self {
            coef,
            factors: BTreeMap::new(),
        }
    }

    /// Create a constant integer mterm.  C++ `mterm(int k)`.
    pub(crate) fn from_int(arena: &mut TreeArena, k: i32) -> Self {
        let coef = SigBuilder::new(arena).int(k);
        Self {
            coef,
            factors: BTreeMap::new(),
        }
    }

    /// Create a constant real mterm.  C++ `mterm(double k)`.
    pub(crate) fn from_real(arena: &mut TreeArena, k: f64) -> Self {
        let coef = SigBuilder::new(arena).real(k);
        Self {
            coef,
            factors: BTreeMap::new(),
        }
    }

    /// Create a mterm by decomposing a multiplicative signal expression.
    ///
    /// Recursively expands `Mul` and `Div` sub-expressions.  C++ `mterm(Tree t)`.
    pub(crate) fn from_sig(arena: &mut TreeArena, t: SigId) -> Self {
        let mut m = Self::from_int(arena, 1);
        m.mul_sig(arena, t);
        m
    }

    // ── In-place arithmetic ───────────────────────────────────────────────────

    /// Multiply this mterm by an expression tree in place.
    ///
    /// Recursively expands `Mul` and `Div` sub-expressions into `fCoef` or
    /// `fFactors`.  C++ `operator*=(Tree)`.
    pub(crate) fn mul_sig(&mut self, arena: &mut TreeArena, t: SigId) {
        if is_num(arena, t) {
            self.coef = mul_nums(arena, self.coef, t);
        } else {
            match match_sig(arena, t) {
                SigMatch::BinOp(BinOp::Mul, x, y) => {
                    self.mul_sig(arena, x);
                    self.mul_sig(arena, y);
                }
                SigMatch::BinOp(BinOp::Div, x, y) => {
                    self.mul_sig(arena, x);
                    self.div_sig(arena, y);
                }
                _ => {
                    if let Some((x, n)) = is_sig_pow(arena, t) {
                        *self.factors.entry(x).or_insert(0) += n;
                    } else {
                        *self.factors.entry(t).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    /// Divide this mterm by an expression tree in place.
    ///
    /// Recursively expands `Mul` and `Div` sub-expressions.  C++ `operator/=(Tree)`.
    pub(crate) fn div_sig(&mut self, arena: &mut TreeArena, t: SigId) {
        if is_num(arena, t) {
            assert!(!is_zero(arena, t), "Mterm: division by zero");
            self.coef = div_nums(arena, self.coef, t);
        } else {
            match match_sig(arena, t) {
                SigMatch::BinOp(BinOp::Mul, x, y) => {
                    self.div_sig(arena, x);
                    self.div_sig(arena, y);
                }
                SigMatch::BinOp(BinOp::Div, x, y) => {
                    self.div_sig(arena, x);
                    self.mul_sig(arena, y);
                }
                _ => {
                    if let Some((x, n)) = is_sig_pow(arena, t) {
                        *self.factors.entry(x).or_insert(0) -= n;
                    } else {
                        *self.factors.entry(t).or_insert(0) -= 1;
                    }
                }
            }
        }
    }

    /// Add an mterm of the **same** signature in place.
    ///
    /// Only the coefficient changes; the factors must match.  C++ `operator+=(mterm)`.
    pub(crate) fn add_mterm(&mut self, arena: &mut TreeArena, m: &Mterm) {
        if is_zero(arena, m.coef) {
            // adding zero: nothing to do
        } else if is_zero(arena, self.coef) {
            self.coef = m.coef;
            self.factors = m.factors.clone();
        } else {
            debug_assert_eq!(
                self.signature_tree(arena),
                m.signature_tree(arena),
                "add_mterm: mismatched signatures"
            );
            self.coef = add_nums(arena, self.coef, m.coef);
        }
        self.cleanup(arena);
    }

    /// Subtract an mterm of the **same** signature in place.  C++ `operator-=(mterm)`.
    pub(crate) fn sub_mterm(&mut self, arena: &mut TreeArena, m: &Mterm) {
        if is_zero(arena, m.coef) {
            // subtracting zero: nothing to do
        } else if is_zero(arena, self.coef) {
            self.coef = minus_num(arena, m.coef);
            self.factors = m.factors.clone();
        } else {
            debug_assert_eq!(
                self.signature_tree(arena),
                m.signature_tree(arena),
                "sub_mterm: mismatched signatures"
            );
            self.coef = sub_nums(arena, self.coef, m.coef);
        }
        self.cleanup(arena);
    }

    /// Multiply this mterm by another mterm in place.  C++ `operator*=(mterm)`.
    pub(crate) fn mul_mterm(&mut self, arena: &mut TreeArena, m: &Mterm) {
        self.coef = mul_nums(arena, self.coef, m.coef);
        for (&base, &exp) in &m.factors {
            *self.factors.entry(base).or_insert(0) += exp;
        }
        self.cleanup(arena);
    }

    /// Divide this mterm by another mterm in place.  C++ `operator/=(mterm)`.
    pub(crate) fn div_mterm(&mut self, arena: &mut TreeArena, m: &Mterm) {
        assert!(!is_zero(arena, m.coef), "div_mterm: division by zero");
        self.coef = div_nums(arena, self.coef, m.coef);
        for (&base, &exp) in &m.factors {
            *self.factors.entry(base).or_insert(0) -= exp;
        }
        self.cleanup(arena);
    }

    // ── Value operators (return new Mterm) ────────────────────────────────────

    /// Return `self * m` as a new mterm.  C++ `operator*(mterm)`.
    pub(crate) fn mul(&self, arena: &mut TreeArena, m: &Mterm) -> Mterm {
        let mut r = self.clone();
        r.mul_mterm(arena, m);
        r
    }

    /// Return `self / m` as a new mterm.  C++ `operator/(mterm)`.
    pub(crate) fn div(&self, arena: &mut TreeArena, m: &Mterm) -> Mterm {
        let mut r = self.clone();
        r.div_mterm(arena, m);
        r
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// True if this mterm is not zero (coefficient ≠ 0).  C++ `isNotZero`.
    pub(crate) fn is_not_zero(&self, arena: &TreeArena) -> bool {
        !is_zero(arena, self.coef)
    }

    /// True if this mterm's coefficient is strictly negative.  C++ `isNegative`.
    pub(crate) fn is_negative(&self, arena: &TreeArena) -> bool {
        !is_ge_zero(arena, self.coef)
    }

    /// Complexity metric: weighted count of factors.
    ///
    /// Used to compare GCDs.  C++ `complexity()`.
    pub(crate) fn complexity(&self, arena: &TreeArena, types: &HashMap<SigId, SigType>) -> i32 {
        let c_coef = if is_one(arena, self.coef) || is_minus_one(arena, self.coef) {
            0
        } else {
            1
        };
        let c_factors: i32 = self
            .factors
            .iter()
            .map(|(&base, &exp)| (1 + sig_order(types, base) as i32) * exp.abs())
            .sum();
        c_coef + c_factors
    }

    /// True if this mterm can be evenly divided by `n`.
    ///
    /// C++ `hasDivisor(n)`.
    pub(crate) fn has_divisor(&self, arena: &TreeArena, n: &Mterm) -> bool {
        if n.factors.is_empty() {
            // n is a pure number: check same magnitude
            return same_magnitude(arena, self.coef, n.coef);
        }
        for (&f, &v) in &n.factors {
            match self.factors.get(&f) {
                None => return false,
                Some(&u) => {
                    if !contains(u, v) {
                        return false;
                    }
                }
            }
        }
        true
    }

    // ── Canonical tree output ─────────────────────────────────────────────────

    /// Return the signature tree (canonical form with coefficient omitted).
    ///
    /// C++ `signatureTree()`.
    pub(crate) fn signature_tree(&self, arena: &mut TreeArena) -> SigId {
        self.normalized_tree(arena, &HashMap::new(), true, false)
    }

    /// Return the normalized (canonical) tree expression.
    ///
    /// Structure: `((k*(v₁/v₂))*(c₁/c₂))*(s₁/s₂)` grouped by variability order.
    ///
    /// - `signature_mode`: omit the coefficient factor.
    /// - `negative_mode`: invert the sign of the coefficient.
    ///
    /// C++ `normalizedTree(bool signatureMode, bool negativeMode)`.
    pub(crate) fn normalized_tree(
        &self,
        arena: &mut TreeArena,
        types: &HashMap<SigId, SigType>,
        signature_mode: bool,
        negative_mode: bool,
    ) -> SigId {
        if self.factors.is_empty() || is_zero(arena, self.coef) {
            // pure number
            if signature_mode {
                return SigBuilder::new(arena).int(1);
            }
            if negative_mode {
                return minus_num(arena, self.coef);
            }
            return self.coef;
        }

        // Group factors by signal order (0=Konst, 1=Block, 2=Samp)
        // A[o] = numerator tree for order o, B[o] = denominator tree
        let mut a: [Option<SigId>; 3] = [None; 3];
        let mut b: [Option<SigId>; 3] = [None; 3];

        for (&f, &q) in &self.factors {
            if q == 0 {
                continue;
            }
            let order = sig_order(types, f) as usize;
            combine_mul_div(arena, &mut a[order], &mut b[order], f, q);
        }

        // C++ assumes order-0 factors are numeric and stores them in `fCoef`.
        // Rust can still carry non-numeric Konst factors here (for example
        // `float(fSamplingFreq)` after fast-lane simplify), so we must merge
        // the coefficient into the existing order-0 numerator instead of
        // overwriting it.
        let coef_term = if signature_mode {
            None
        } else if negative_mode {
            if is_minus_one(arena, self.coef) {
                None
            } else {
                Some(minus_num(arena, self.coef))
            }
        } else if is_one(arena, self.coef) {
            None
        } else {
            Some(self.coef)
        };
        if let Some(coef) = coef_term {
            combine_mul_left(arena, &mut a[0], coef);
        }

        // Combine each order: R[order] = A[order] / B[order]
        let mut rr: Option<SigId> = None;
        for order in 0..3 {
            match (a[order], b[order]) {
                (Some(num), Some(den)) => {
                    let frac = SigBuilder::new(arena).div(num, den);
                    combine_mul_left(arena, &mut rr, frac);
                }
                (Some(num), None) => combine_mul_left(arena, &mut rr, num),
                (None, Some(den)) => combine_div_left(arena, &mut rr, den),
                (None, None) => {}
            }
        }

        rr.unwrap_or_else(|| SigBuilder::new(arena).int(1))
    }

    // ── Maintenance ───────────────────────────────────────────────────────────

    /// Remove zero-exponent factors and, if coefficient is zero, clear all factors.
    ///
    /// C++ `cleanup()`.
    pub(crate) fn cleanup(&mut self, arena: &TreeArena) {
        if is_zero(arena, self.coef) {
            self.factors.clear();
        } else {
            self.factors.retain(|_, &mut exp| exp != 0);
        }
    }
}

// ─── "contains" relation ──────────────────────────────────────────────────────

/// `a` "contains" `b` if dividing by `b` stays in the same sign direction.
///
/// `3` contains `2` and `-4` contains `-2`, but `3` does not contain `-2`.
/// C++ `contains(int a, int b)`.
pub(crate) fn contains(a: i32, b: i32) -> bool {
    b == 0 || a / b > 0
}

// ─── GCD ──────────────────────────────────────────────────────────────────────

/// Greatest common divisor of two mterms.
///
/// The coefficient is `m1.coef` when they have the same magnitude, otherwise 1.
/// For each factor present in both mterms, the GCD exponent is the "common" value
/// (minimum magnitude in the same sign direction).
///
/// C++ free function `gcd(m1, m2)`.
pub(crate) fn gcd(arena: &mut TreeArena, m1: &Mterm, m2: &Mterm) -> Mterm {
    let coef = if same_magnitude(arena, m1.coef, m2.coef) {
        m1.coef
    } else {
        SigBuilder::new(arena).int(1)
    };
    let mut r = Mterm {
        coef,
        factors: BTreeMap::new(),
    };

    for (&t, &v1) in &m1.factors {
        if let Some(&v2) = m2.factors.get(&t) {
            let c = common_exp(v1, v2);
            if c != 0 {
                r.factors.insert(t, c);
            }
        }
    }
    r
}

/// Return the "common quantity" of two exponents (minimum magnitude in same direction).
///
/// C++ `common(int a, int b)`.
fn common_exp(a: i32, b: i32) -> i32 {
    if a > 0 && b > 0 {
        a.min(b)
    } else if a < 0 && b < 0 {
        a.max(b)
    } else {
        0
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use interval::Interval;
    use sigtype::{Boolean, Computability, Nature, Variability, Vectorability, make_simple};
    use tlib::TreeArena;

    fn arena() -> TreeArena {
        TreeArena::new()
    }

    #[test]
    fn mterm_zero_is_not_not_zero() {
        let mut a = arena();
        let m = Mterm::zero(&mut a);
        assert!(!m.is_not_zero(&a));
    }

    #[test]
    fn mterm_from_int_is_not_zero() {
        let mut a = arena();
        let m = Mterm::from_int(&mut a, 3);
        assert!(m.is_not_zero(&a));
        assert!(!m.is_negative(&a));
    }

    #[test]
    fn mterm_from_int_neg_is_negative() {
        let mut a = arena();
        let m = Mterm::from_int(&mut a, -2);
        assert!(m.is_negative(&a));
    }

    #[test]
    fn mterm_normalized_tree_pure_number() {
        let mut a = arena();
        let m = Mterm::from_int(&mut a, 5);
        let t = m.normalized_tree(&mut a, &HashMap::new(), false, false);
        assert_eq!(match_sig(&a, t), SigMatch::Int(5));
    }

    #[test]
    fn mterm_normalized_tree_negative_mode() {
        let mut a = arena();
        let m = Mterm::from_int(&mut a, 3);
        let t = m.normalized_tree(&mut a, &HashMap::new(), false, true);
        assert_eq!(match_sig(&a, t), SigMatch::Int(-3));
    }

    #[test]
    fn mterm_normalized_tree_keeps_konst_non_numeric_factors_in_order_zero_bucket() {
        let mut a = arena();
        let ty = a.int(0);
        let name = a.symbol("fSamplingFreq");
        let file = a.symbol("<math.h>");
        let sr = SigBuilder::new(&mut a).fconst(ty, name, file);
        let sr_real = SigBuilder::new(&mut a).float_cast(sr);
        let half = SigBuilder::new(&mut a).real(0.5);
        let expr = SigBuilder::new(&mut a).mul(half, sr_real);
        let m = Mterm::from_sig(&mut a, expr);

        let mut types = HashMap::new();
        let konst_real = make_simple(
            Nature::Real,
            Variability::Konst,
            Computability::Init,
            Vectorability::Vect,
            Boolean::Num,
            Interval::new_default(),
        );
        types.insert(sr_real, konst_real);

        let rebuilt = m.normalized_tree(&mut a, &types, false, false);
        let dumped = signals::dump_sig_readable(&a, rebuilt);
        assert!(
            dumped.contains("fSamplingFreq"),
            "order-0 non-numeric factors must survive normalized_tree, got {dumped}"
        );
    }

    #[test]
    fn mterm_from_sig_mul() {
        // from_sig(2 * x) → coef=2, factors={x→1}
        let mut a = arena();
        let x = SigBuilder::new(&mut a).input(0);
        let two = SigBuilder::new(&mut a).int(2);
        let sig = SigBuilder::new(&mut a).mul(two, x);
        let m = Mterm::from_sig(&mut a, sig);
        assert_eq!(match_sig(&a, m.coef), SigMatch::Int(2));
        assert_eq!(m.factors[&x], 1);
    }

    #[test]
    fn mterm_from_sig_div() {
        // from_sig(x / 2) → coef=0.5 (or 1 with factor x^1 and factor 2^-1?)
        // Actually 2 is numeric, so coef = 1/2 = 0.5, factors = {x → 1}
        let mut a = arena();
        let x = SigBuilder::new(&mut a).input(0);
        let two = SigBuilder::new(&mut a).int(2);
        let sig = SigBuilder::new(&mut a).div(x, two);
        let m = Mterm::from_sig(&mut a, sig);
        // coef should be 0.5 (int 1 / int 2 = real 0.5)
        match match_sig(&a, m.coef) {
            SigMatch::Real(r) => assert!((r - 0.5).abs() < 1e-10),
            SigMatch::Int(1) => {} // acceptable if x stays as factor
            other => panic!("unexpected coef: {other:?}"),
        }
    }

    #[test]
    fn mterm_add_same_sig() {
        // 2*x + 3*x = 5*x
        let mut a = arena();
        let x = SigBuilder::new(&mut a).input(0);
        let mut m1 = Mterm::from_sig(&mut a, x);
        let two = SigBuilder::new(&mut a).int(2);
        m1.coef = mul_nums(&mut a, m1.coef, two);
        let _m2 = Mterm::from_sig(&mut a, x);
        let mut m3 = m1.clone();
        let _three = Mterm::from_int(&mut a, 3);
        let mut m_3x = Mterm::from_sig(&mut a, x);
        m_3x.coef = SigBuilder::new(&mut a).int(3);
        m3.add_mterm(&mut a, &m_3x);
        assert_eq!(match_sig(&a, m3.coef), SigMatch::Int(5));
    }

    #[test]
    fn gcd_same_factors() {
        // gcd(2*x, 4*x) = 2*x  (same magnitude? No: 2 ≠ 4, so coef=1; factor x with min(1,1)=1)
        let mut a = arena();
        let x = SigBuilder::new(&mut a).input(0);
        let two = SigBuilder::new(&mut a).int(2);
        let sig1 = SigBuilder::new(&mut a).mul(two, x);
        let m1 = Mterm::from_sig(&mut a, sig1);
        let four = SigBuilder::new(&mut a).int(4);
        let sig2 = SigBuilder::new(&mut a).mul(four, x);
        let m2 = Mterm::from_sig(&mut a, sig2);
        let g = gcd(&mut a, &m1, &m2);
        // coef: sameMagnitude(2, 4) = false → coef = 1
        assert_eq!(match_sig(&a, g.coef), SigMatch::Int(1));
        // factor x: common_exp(1, 1) = 1
        assert_eq!(g.factors.get(&x), Some(&1));
    }

    #[test]
    fn gcd_same_magnitude_coef() {
        // gcd(-3*x, 3*x) → coef = -3 (m1.coef), factor x^1
        let mut a = arena();
        let x = SigBuilder::new(&mut a).input(0);
        let neg3 = SigBuilder::new(&mut a).int(-3);
        let pos3 = SigBuilder::new(&mut a).int(3);
        let sig1 = SigBuilder::new(&mut a).mul(neg3, x);
        let m1 = Mterm::from_sig(&mut a, sig1);
        let sig2 = SigBuilder::new(&mut a).mul(pos3, x);
        let m2 = Mterm::from_sig(&mut a, sig2);
        let g = gcd(&mut a, &m1, &m2);
        assert_eq!(match_sig(&a, g.coef), SigMatch::Int(-3));
        assert_eq!(g.factors.get(&x), Some(&1));
    }

    #[test]
    fn has_divisor_true() {
        let mut a = arena();
        let x = SigBuilder::new(&mut a).input(0);
        let two = SigBuilder::new(&mut a).int(2);
        let sig = SigBuilder::new(&mut a).mul(two, x);
        let m = Mterm::from_sig(&mut a, sig);
        let d = Mterm::from_sig(&mut a, x);
        assert!(m.has_divisor(&a, &d));
    }

    #[test]
    fn has_divisor_false_wrong_factor() {
        let mut a = arena();
        let x = SigBuilder::new(&mut a).input(0);
        let y = SigBuilder::new(&mut a).input(1);
        let m = Mterm::from_sig(&mut a, x);
        let d = Mterm::from_sig(&mut a, y);
        assert!(!m.has_divisor(&a, &d));
    }

    #[test]
    fn is_num_detects_int_and_real() {
        let mut a = arena();
        let i = SigBuilder::new(&mut a).int(0);
        let r = SigBuilder::new(&mut a).real(0.0);
        let x = SigBuilder::new(&mut a).input(0);
        assert!(is_num(&a, i));
        assert!(is_num(&a, r));
        assert!(!is_num(&a, x));
    }

    #[test]
    fn contains_rules() {
        assert!(contains(3, 2)); // 3/2 = 1 > 0
        assert!(contains(-4, -2)); // -4/-2 = 2 > 0
        assert!(!contains(3, -2));
        assert!(!contains(-3, 1));
        assert!(contains(5, 0)); // b==0 => true
    }
}
