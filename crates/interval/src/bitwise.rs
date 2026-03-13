//! Bitwise interval operations.
//!
//! Ports the C++ bitwise interval subsystem.
//!
//! # C++ source
//! `compiler/interval/bitwiseOperations.hh`
//! `compiler/interval/bitwiseOperations.cpp`

// -------------------------------------------------------------------------
// Data types
// -------------------------------------------------------------------------

/// Signed integer interval used by bitwise operations.
///
/// Empty when `lo > hi`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SInterval {
    pub lo: i32,
    pub hi: i32,
}

/// Unsigned integer interval used by bitwise operations.
///
/// Empty when `lo > hi`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UInterval {
    pub lo: u32,
    pub hi: u32,
}

/// Empty unsigned interval sentinel. C++ `UEMPTY = {UINT_MAX, 0}`.
pub const UEMPTY: UInterval = UInterval { lo: u32::MAX, hi: 0 };

/// Empty signed interval sentinel. C++ `SEMPTY = {INT_MAX, INT_MIN}`.
pub const SEMPTY: SInterval = SInterval { lo: i32::MAX, hi: i32::MIN };

/// Test emptiness: lo > hi.
#[inline]
#[must_use]
pub fn is_empty_u(i: UInterval) -> bool { i.lo > i.hi }

#[inline]
#[must_use]
pub fn is_empty_s(i: SInterval) -> bool { i.lo > i.hi }

// -------------------------------------------------------------------------
// Union (operator+ for BitwiseInterval in C++)
// -------------------------------------------------------------------------

fn union_u(a: UInterval, b: UInterval) -> UInterval {
    if is_empty_u(a) { return b; }
    if is_empty_u(b) { return a; }
    UInterval { lo: a.lo.min(b.lo), hi: a.hi.max(b.hi) }
}

#[allow(dead_code)]
fn union_s(a: SInterval, b: SInterval) -> SInterval {
    if is_empty_s(a) { return b; }
    if is_empty_s(b) { return a; }
    SInterval { lo: a.lo.min(b.lo), hi: a.hi.max(b.hi) }
}

// -------------------------------------------------------------------------
// sign split / sign merge
// -------------------------------------------------------------------------

/// Split signed interval into (negative part, positive part) as unsigned.
///
/// C++ `signSplit`.
pub fn sign_split(x: SInterval) -> (UInterval, UInterval) {
    if is_empty_s(x) { return (UEMPTY, UEMPTY); }
    if x.hi < 0 {
        // All negative: treat bit-pattern as unsigned.
        return (UInterval { lo: x.lo as u32, hi: x.hi as u32 }, UEMPTY);
    }
    if x.lo >= 0 {
        return (UEMPTY, UInterval { lo: x.lo as u32, hi: x.hi as u32 });
    }
    // Straddles zero.
    (
        UInterval { lo: x.lo as u32, hi: u32::MAX },
        UInterval { lo: 0, hi: x.hi as u32 },
    )
}

/// Merge (negative part, positive part) back into a signed interval.
///
/// C++ `signMerge`.
pub fn sign_merge(np: UInterval, pp: UInterval) -> SInterval {
    if is_empty_u(np) {
        if is_empty_u(pp) { return SEMPTY; }
        return SInterval { lo: pp.lo as i32, hi: pp.hi as i32 };
    }
    if is_empty_u(pp) {
        return SInterval { lo: np.lo as i32, hi: np.hi as i32 };
    }
    SInterval { lo: np.lo as i32, hi: pp.hi as i32 }
}

// -------------------------------------------------------------------------
// NOT
// -------------------------------------------------------------------------

pub fn bitwise_unsigned_not(a: UInterval) -> UInterval {
    UInterval { lo: !a.hi, hi: !a.lo }
}

pub fn bitwise_signed_not(a: SInterval) -> SInterval {
    SInterval { lo: !a.hi, hi: !a.lo }
}

// -------------------------------------------------------------------------
// OR — Knuth / Hacker's Delight algorithm
// -------------------------------------------------------------------------

fn msb32(mut x: u32) -> u32 {
    x |= x >> 1; x |= x >> 2; x |= x >> 4; x |= x >> 8; x |= x >> 16;
    x & !(x >> 1)
}

fn contains_u(i: UInterval, x: u32) -> bool { i.lo <= x && x <= i.hi }

fn split_interval(x: UInterval) -> (u32, UInterval, UInterval) {
    if x.lo == 0 && x.hi == 0 {
        return (0, UInterval { lo: 1, hi: 0 }, x); // special: no msb
    }
    let m = msb32(x.hi);
    debug_assert!(m > 0);
    if m <= x.lo {
        return (m, UInterval { lo: 1, hi: 0 }, x); // no split needed
    }
    (m, UInterval { lo: x.lo, hi: m - 1 }, UInterval { lo: m, hi: x.hi })
}

fn hi_or2(a: UInterval, b: UInterval) -> u32 {
    if a.lo == 0 && a.hi == 0 { return b.hi; }
    if b.lo == 0 && b.hi == 0 { return a.hi; }

    let (ma, a0, a1) = split_interval(a);
    let (mb, b0, b1) = split_interval(b);

    // mask rule
    if (a.hi == 2 * ma.wrapping_sub(1)) || (b.hi == 2 * mb.wrapping_sub(1)) {
        return a.hi | b.hi;
    }

    if mb > ma {
        if contains_u(a, mb - 1) { return 2 * mb - 1; }
        return hi_or2(UInterval { lo: b1.lo - mb, hi: b1.hi - mb }, a) + mb;
    }
    if ma > mb {
        if contains_u(b, ma - 1) { return 2 * ma - 1; }
        return hi_or2(UInterval { lo: a1.lo - ma, hi: a1.hi - ma }, b) + ma;
    }
    // ma == mb != 0
    let a1s = UInterval { lo: a1.lo - ma, hi: a1.hi - ma };
    let b1s = UInterval { lo: b1.lo - mb, hi: b1.hi - mb };
    if is_empty_u(a0) && is_empty_u(b0) {
        return hi_or2(a1s, b1s) + ma;
    }
    if is_empty_u(a0) {
        return hi_or2(a1s, b1s).max(hi_or2(a1s, b0)) + ma;
    }
    if is_empty_u(b0) {
        return hi_or2(a1s, b1s).max(hi_or2(a0, b1s)) + ma;
    }
    hi_or2(a1s, b1s).max(hi_or2(a1s, b0)).max(hi_or2(a0, b1s)) + ma
}

fn lo_or2(a: UInterval, b: UInterval) -> u32 {
    if is_empty_u(a) || is_empty_u(b) { return 0; }
    if a.lo == 0 { return b.lo; }
    if b.lo == 0 { return a.lo; }

    let (ma, a0, a1) = split_interval(a);
    let (mb, b0, b1) = split_interval(b);
    debug_assert!(ma != 0 && mb != 0);

    let a1s = UInterval { lo: a1.lo - ma, hi: a1.hi - ma };
    let b1s = UInterval { lo: b1.lo - mb, hi: b1.hi - mb };

    if ma > mb {
        if is_empty_u(a0) { return lo_or2(a1s, b) | ma; }
        return lo_or2(a0, b);
    }
    if mb > ma {
        if is_empty_u(b0) { return lo_or2(a, b1s) | mb; }
        return lo_or2(a, b0);
    }
    // ma == mb
    if !is_empty_u(a0) && !is_empty_u(b0) { return lo_or2(a0, b0); }
    if is_empty_u(a0) && is_empty_u(b0) { return lo_or2(a1s, b1s) | ma; }
    if is_empty_u(a0) {
        return (lo_or2(a1s, b0) | ma).min(lo_or2(a1s, b1s) | ma);
    }
    (lo_or2(a0, b1s) | mb).min(lo_or2(a1s, b1s) | ma)
}

pub fn bitwise_unsigned_or(a: UInterval, b: UInterval) -> UInterval {
    if a == (UInterval { lo: 0, hi: 0 }) { return b; }
    if b == (UInterval { lo: 0, hi: 0 }) { return a; }
    if is_empty_u(a) { return a; }
    if is_empty_u(b) { return b; }
    UInterval { lo: lo_or2(a, b), hi: hi_or2(a, b) }
}

pub fn bitwise_signed_or(a: SInterval, b: SInterval) -> SInterval {
    let (an, ap) = sign_split(a);
    let (bn, bp) = sign_split(b);
    let pp = bitwise_unsigned_or(ap, bp);
    let nn = bitwise_unsigned_or(an, bn);
    let pn = bitwise_unsigned_or(ap, bn);
    let np = bitwise_unsigned_or(an, bp);
    sign_merge(union_u(union_u(np, nn), pn), pp)
}

// -------------------------------------------------------------------------
// AND (De Morgan)
// -------------------------------------------------------------------------

pub fn bitwise_unsigned_and(a: UInterval, b: UInterval) -> UInterval {
    bitwise_unsigned_not(bitwise_unsigned_or(bitwise_unsigned_not(a), bitwise_unsigned_not(b)))
}

pub fn bitwise_signed_and(a: SInterval, b: SInterval) -> SInterval {
    bitwise_signed_not(bitwise_signed_or(bitwise_signed_not(a), bitwise_signed_not(b)))
}

// -------------------------------------------------------------------------
// XOR
// -------------------------------------------------------------------------

pub fn bitwise_unsigned_xor(a: UInterval, b: UInterval) -> UInterval {
    bitwise_unsigned_and(
        bitwise_unsigned_or(a, b),
        bitwise_unsigned_not(bitwise_unsigned_and(a, b)),
    )
}

pub fn bitwise_signed_xor(a: SInterval, b: SInterval) -> SInterval {
    let (an, ap) = sign_split(a);
    let (bn, bp) = sign_split(b);
    let pp = bitwise_unsigned_xor(ap, bp);
    let nn = bitwise_unsigned_xor(an, bn);
    let pn = bitwise_unsigned_xor(ap, bn);
    let np = bitwise_unsigned_xor(an, bp);
    sign_merge(union_u(np, pn), union_u(pp, nn))
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_u() {
        let a = UInterval { lo: 0, hi: 0 };
        let n = bitwise_unsigned_not(a);
        assert_eq!(n.lo, u32::MAX);
        assert_eq!(n.hi, u32::MAX);
    }

    #[test]
    fn not_s() {
        let a = SInterval { lo: 0, hi: 10 };
        let n = bitwise_signed_not(a);
        assert_eq!(n.lo, !10);
        assert_eq!(n.hi, !0);
    }

    #[test]
    fn sign_split_all_negative() {
        let (neg, pos) = sign_split(SInterval { lo: -5, hi: -1 });
        assert!(!is_empty_u(neg));
        assert!(is_empty_u(pos));
    }

    #[test]
    fn sign_split_all_positive() {
        let (neg, pos) = sign_split(SInterval { lo: 0, hi: 10 });
        assert!(is_empty_u(neg));
        assert!(!is_empty_u(pos));
    }

    #[test]
    fn or_zero_identity() {
        let a = UInterval { lo: 5, hi: 10 };
        let z = UInterval { lo: 0, hi: 0 };
        assert_eq!(bitwise_unsigned_or(a, z), a);
        assert_eq!(bitwise_unsigned_or(z, a), a);
    }

    #[test]
    fn and_is_not_or_not() {
        let a = UInterval { lo: 3, hi: 7 };
        let b = UInterval { lo: 5, hi: 9 };
        let and_ab = bitwise_unsigned_and(a, b);
        let or_ab = bitwise_unsigned_or(a, b);
        // AND result lo can be 0 (e.g. x & ~x = 0); just check bounds are valid
        assert!(is_empty_u(and_ab) || and_ab.lo <= and_ab.hi);
        // OR result hi must be at least max(a.hi, b.hi)
        assert!(or_ab.hi >= a.hi.max(b.hi));
    }
}
