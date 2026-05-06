//! C++-parity FIR/IIR algebra helpers for signal carrier nodes.
//!
//! # Source provenance (C++)
//! - `compiler/signals/sigFIR.hh`
//! - `compiler/signals/sigFIR.cpp`
//! - `compiler/signals/sigIIR.hh`
//! - `compiler/signals/sigIIR.cpp`
//!
//! # Role
//!
//! This module owns the algebraic helper layer above the raw `SIGFIR` and
//! `SIGIIR` carrier nodes exposed by [`crate::SigBuilder`] and
//! [`crate::SigMatch`]. Keeping it out of `lib.rs` makes the representation
//! layer smaller while preserving one public `signals` API surface through
//! re-exports.

use tlib::TreeArena;

use crate::{BinOp, SigBuilder, SigId, SigMatch, match_sig};

/// Creates an elementary C++-parity FIR node for a fixed delay.
///
/// Source provenance:
/// - C++ `compiler/signals/sigFIR.cpp::makeSigFIR`
///
/// `S@d` becomes `sigFIR([S, 0, ..., 0, 1])` with `d` zero coefficients before
/// the trailing unit tap. Negative delays are not a valid FIR delay and return
/// the ordinary delayed signal, leaving validation to later type/causality
/// passes.
#[must_use]
pub fn make_sig_fir(arena: &mut TreeArena, sig: SigId, delay: i32) -> SigId {
    if delay < 0 {
        let mut b = SigBuilder::new(arena);
        let amount = b.int(delay);
        return b.delay(sig, amount);
    }

    let mut b = SigBuilder::new(arena);
    let mut coefs = Vec::with_capacity(delay as usize + 2);
    coefs.push(sig);
    for _ in 0..delay {
        coefs.push(b.int(0));
    }
    coefs.push(b.int(1));
    b.fir(&coefs)
}

/// Delays a signal while preserving C++ `sigFIR` structure when possible.
///
/// Source provenance:
/// - C++ `compiler/signals/sigFIR.cpp::delaySigFIR`
///
/// Constant non-negative delays shift FIR coefficients. A zero delay returns
/// the original signal. Non-constant or negative delays fall back to an
/// ordinary `delay` node.
#[must_use]
pub fn delay_sig_fir(arena: &mut TreeArena, sig: SigId, amount: SigId) -> SigId {
    let Some(delay) = sig_int_value(arena, amount) else {
        return SigBuilder::new(arena).delay(sig, amount);
    };
    if delay < 0 {
        return SigBuilder::new(arena).delay(sig, amount);
    }
    if delay == 0 {
        return sig;
    }

    if let SigMatch::Fir(coefs) = match_sig(arena, sig) {
        let coefs = coefs.to_vec();
        let mut shifted = Vec::with_capacity(coefs.len() + delay as usize);
        shifted.push(coefs[0]);
        let mut b = SigBuilder::new(arena);
        for _ in 0..delay {
            shifted.push(b.int(0));
        }
        shifted.extend_from_slice(&coefs[1..]);
        b.fir(&shifted)
    } else {
        make_sig_fir(arena, sig, delay)
    }
}

/// Simplifies a `sigFIR` carrier by removing trailing literal zero taps.
///
/// Source provenance:
/// - C++ `compiler/signals/sigFIR.cpp::simplifyFIR`
/// - C++ `compiler/signals/sigFIR.cpp::normalizeFIRCoefs`
///
/// This first Rust step intentionally performs only structural simplification:
/// literal zero base or all-zero taps become `0`; a single remaining tap becomes
/// a plain multiplication. General arithmetic simplification remains owned by
/// the normalize crate.
#[must_use]
pub fn simplify_fir(arena: &mut TreeArena, sig: SigId) -> SigId {
    let SigMatch::Fir(coefs) = match_sig(arena, sig) else {
        return sig;
    };
    let coefs = coefs.to_vec();
    if coefs.len() < 2 || is_zero_sig(arena, coefs[0]) {
        return SigBuilder::new(arena).int(0);
    }

    let mut last_non_zero = None;
    for (idx, coef) in coefs.iter().enumerate().skip(1) {
        if !is_zero_sig(arena, *coef) {
            last_non_zero = Some(idx);
        }
    }

    let Some(last_non_zero) = last_non_zero else {
        return SigBuilder::new(arena).int(0);
    };
    if last_non_zero == 1 {
        return SigBuilder::new(arena).mul(coefs[1], coefs[0]);
    }
    if last_non_zero + 1 < coefs.len() {
        let trimmed = coefs[..=last_non_zero].to_vec();
        return SigBuilder::new(arena).fir(&trimmed);
    }
    sig
}

/// Negates a FIR structurally when the input is a `sigFIR`.
///
/// Source provenance:
/// - C++ `compiler/signals/sigFIR.cpp::negSigFIR`
#[must_use]
pub fn neg_sig_fir(arena: &mut TreeArena, sig: SigId) -> SigId {
    if let SigMatch::Fir(coefs) = match_sig(arena, sig) {
        let coefs = coefs.to_vec();
        let mut b = SigBuilder::new(arena);
        let mut negated = Vec::with_capacity(coefs.len());
        negated.push(coefs[0]);
        for coef in &coefs[1..] {
            negated.push(neg_sig(&mut b, *coef));
        }
        b.fir(&negated)
    } else {
        let mut b = SigBuilder::new(arena);
        neg_sig(&mut b, sig)
    }
}

/// Adds two compatible FIRs or falls back to an ordinary addition.
///
/// Source provenance:
/// - C++ `compiler/signals/sigFIR.cpp::addSigFIR`
///
/// This ports the core same-base case `[S, C...] + [S, D...]`; product
/// divisibility cases are handled by a later L1 step.
#[must_use]
pub fn add_sig_fir(arena: &mut TreeArena, lhs: SigId, rhs: SigId) -> SigId {
    let (SigMatch::Fir(lhs_coefs), SigMatch::Fir(rhs_coefs)) =
        (match_sig(arena, lhs), match_sig(arena, rhs))
    else {
        return SigBuilder::new(arena).add(lhs, rhs);
    };
    let lhs_coefs = lhs_coefs.to_vec();
    let rhs_coefs = rhs_coefs.to_vec();

    if lhs_coefs.is_empty() || rhs_coefs.is_empty() || lhs_coefs[0] != rhs_coefs[0] {
        return SigBuilder::new(arena).add(lhs, rhs);
    }

    let mut b = SigBuilder::new(arena);
    let len = lhs_coefs.len().max(rhs_coefs.len());
    let mut coefs = Vec::with_capacity(len);
    coefs.push(lhs_coefs[0]);
    let zero = b.int(0);
    for idx in 1..len {
        let l = lhs_coefs.get(idx).copied().unwrap_or(zero);
        let r = rhs_coefs.get(idx).copied().unwrap_or(zero);
        coefs.push(add_or_passthrough(&mut b, l, r));
    }
    let fir = b.fir(&coefs);
    simplify_fir(arena, fir)
}

/// Subtracts two FIRs by negating the second operand before addition.
///
/// Source provenance:
/// - C++ `compiler/signals/sigFIR.cpp::subSigFIR`
#[must_use]
pub fn sub_sig_fir(arena: &mut TreeArena, lhs: SigId, rhs: SigId) -> SigId {
    let neg_rhs = neg_sig_fir(arena, rhs);
    add_sig_fir(arena, lhs, neg_rhs)
}

/// Expands a `sigFIR` carrier back to ordinary delayed signal terms.
///
/// Source provenance:
/// - C++ `compiler/signals/sigFIR.cpp::convertFIR2Sig`
#[must_use]
pub fn convert_fir_to_sig(arena: &mut TreeArena, sig: SigId) -> SigId {
    let SigMatch::Fir(coefs) = match_sig(arena, sig) else {
        return sig;
    };
    let coefs = coefs.to_vec();
    if coefs.len() < 2 {
        return SigBuilder::new(arena).int(0);
    }

    let mut b = SigBuilder::new(arena);
    let base = coefs[0];
    let mut result = b.int(0);
    for (idx, coef) in coefs.iter().copied().enumerate().skip(1) {
        if is_zero_sig(b.arena(), coef) {
            continue;
        }
        let delayed = if idx == 1 {
            base
        } else {
            let amount = b.int((idx - 1) as i32);
            b.delay(base, amount)
        };
        let term = if is_one_sig(b.arena(), coef) {
            delayed
        } else {
            b.mul(coef, delayed)
        };
        result = add_or_passthrough(&mut b, result, term);
    }
    result
}

/// Creates an IIR identity for a recursive projection when it targets `rt`.
///
/// Source provenance:
/// - C++ `compiler/signals/sigIIR.cpp::proj2SigIIR`
///
/// If `sig` is the same recursive projection as `rt`, this returns
/// `sigIIR([sig, 0, 1])`. If `sig` belongs to the same recursive group but a
/// different projection, the result is `nil`, matching the C++ helper's
/// "not representable as this scalar IIR" convention. Projections from other
/// groups are independent of `rt` and are returned unchanged.
#[must_use]
pub fn proj_to_sig_iir(arena: &mut TreeArena, rt: SigId, sig: SigId) -> SigId {
    let (SigMatch::Proj(rt_idx, rt_group), SigMatch::Proj(sig_idx, sig_group)) =
        (match_sig(arena, rt), match_sig(arena, sig))
    else {
        return sig;
    };

    if rt == sig {
        let mut b = SigBuilder::new(arena);
        let zero = b.int(0);
        let one = b.int(1);
        b.iir(&[sig, zero, one])
    } else if rt_group == sig_group && rt_idx != sig_idx {
        arena.nil()
    } else {
        sig
    }
}

/// Returns the coefficient vector for an IIR concerned by recursive target `rt`.
///
/// Source provenance:
/// - C++ `compiler/signals/sigIIR.cpp::concernedIIR`
#[must_use]
pub fn concerned_iir(arena: &TreeArena, rt: SigId, sig: SigId) -> Option<Vec<SigId>> {
    match match_sig(arena, sig) {
        SigMatch::Iir(coefs) if coefs.first().copied() == Some(rt) => Some(coefs.to_vec()),
        _ => None,
    }
}

/// Delays an IIR expression when the delay amount is constant.
///
/// Source provenance:
/// - C++ `compiler/signals/sigIIR.cpp::delaySigIIR`
///
/// Delaying a concerned IIR shifts the input term and feedback coefficients.
/// Delaying by an expression that itself is a concerned IIR is not representable
/// as an IIR and returns `nil`.
#[must_use]
pub fn delay_sig_iir(arena: &mut TreeArena, rt: SigId, x: SigId, y: SigId) -> SigId {
    if concerned_iir(arena, rt, y).is_some() {
        return arena.nil();
    }
    let Some(coefs) = concerned_iir(arena, rt, x) else {
        return SigBuilder::new(arena).delay(x, y);
    };
    let Some(delay) = sig_int_value(arena, y) else {
        return arena.nil();
    };
    if delay < 0 {
        return arena.nil();
    }
    delay_iir_coefs(arena, &coefs, delay)
}

/// Adds two IIR expressions concerned by `rt`, or folds an independent term
/// into the input part of one concerned IIR.
///
/// Source provenance:
/// - C++ `compiler/signals/sigIIR.cpp::addSigIIR`
#[must_use]
pub fn add_sig_iir(arena: &mut TreeArena, rt: SigId, x: SigId, y: SigId) -> SigId {
    match (concerned_iir(arena, rt, x), concerned_iir(arena, rt, y)) {
        (Some(cx), Some(cy)) => combine_iir_coefs(arena, &cx, &cy, BinOp::Add),
        (Some(mut cx), None) => {
            let input = add_or_passthrough(&mut SigBuilder::new(arena), cx[1], y);
            cx[1] = input;
            SigBuilder::new(arena).iir(&cx)
        }
        (None, Some(mut cy)) => {
            let input = add_or_passthrough(&mut SigBuilder::new(arena), x, cy[1]);
            cy[1] = input;
            SigBuilder::new(arena).iir(&cy)
        }
        (None, None) => SigBuilder::new(arena).add(x, y),
    }
}

/// Subtracts two IIR expressions concerned by `rt`.
///
/// Source provenance:
/// - C++ `compiler/signals/sigIIR.cpp::subSigIIR`
#[must_use]
pub fn sub_sig_iir(arena: &mut TreeArena, rt: SigId, x: SigId, y: SigId) -> SigId {
    match (concerned_iir(arena, rt, x), concerned_iir(arena, rt, y)) {
        (Some(cx), Some(cy)) => combine_iir_coefs(arena, &cx, &cy, BinOp::Sub),
        (Some(mut cx), None) => {
            cx[1] = SigBuilder::new(arena).sub(cx[1], y);
            SigBuilder::new(arena).iir(&cx)
        }
        (None, Some(mut cy)) => {
            let mut b = SigBuilder::new(arena);
            for coef in cy.iter_mut().skip(1) {
                *coef = neg_sig(&mut b, *coef);
            }
            cy[1] = add_or_passthrough(&mut b, x, cy[1]);
            b.iir(&cy)
        }
        (None, None) => SigBuilder::new(arena).sub(x, y),
    }
}

/// Multiplies an IIR expression by an independent factor.
///
/// Source provenance:
/// - C++ `compiler/signals/sigIIR.cpp::mulSigIIR`
#[must_use]
pub fn mul_sig_iir(arena: &mut TreeArena, rt: SigId, x: SigId, y: SigId) -> SigId {
    match (concerned_iir(arena, rt, x), concerned_iir(arena, rt, y)) {
        (Some(_), Some(_)) => arena.nil(),
        (Some(cx), None) => scale_iir_coefs(arena, &cx, y, BinOp::Mul),
        (None, Some(cy)) => scale_iir_coefs(arena, &cy, x, BinOp::Mul),
        (None, None) => SigBuilder::new(arena).mul(x, y),
    }
}

/// Divides an IIR expression by an independent denominator.
///
/// Source provenance:
/// - C++ `compiler/signals/sigIIR.cpp::divSigIIR`
#[must_use]
pub fn div_sig_iir(arena: &mut TreeArena, rt: SigId, x: SigId, y: SigId) -> SigId {
    match (concerned_iir(arena, rt, x), concerned_iir(arena, rt, y)) {
        (_, Some(_)) => arena.nil(),
        (Some(cx), None) => scale_iir_coefs(arena, &cx, y, BinOp::Div),
        (None, None) => SigBuilder::new(arena).div(x, y),
    }
}

/// Rewrites a FIR applied to a concerned IIR into an IIR over the filtered
/// independent input, when the C++ helper can express it.
///
/// Source provenance:
/// - C++ `compiler/signals/sigIIR.cpp::embeddedIIR`
#[must_use]
pub fn embedded_iir(arena: &mut TreeArena, rt: SigId, fir: SigId) -> SigId {
    let SigMatch::Fir(cfir) = match_sig(arena, fir) else {
        return arena.nil();
    };
    let cfir = cfir.to_vec();
    if cfir.len() < 2 {
        return arena.nil();
    }
    let Some(ciir) = concerned_iir(arena, rt, cfir[0]) else {
        return arena.nil();
    };
    if ciir.len() < 2 {
        return arena.nil();
    }

    let mut b = SigBuilder::new(arena);
    let mut input_fir_coefs = cfir.clone();
    input_fir_coefs[0] = ciir[1];
    let input_fir = b.fir(&input_fir_coefs);

    let mut recursive_iir_coefs = ciir.clone();
    recursive_iir_coefs[1] = b.int(0);
    let recursive_iir = b.iir(&recursive_iir_coefs);

    let mut res = mul_sig_iir(arena, rt, recursive_iir, cfir[1]);
    for (idx, coef) in cfir.iter().copied().enumerate().skip(2) {
        let mut b = SigBuilder::new(arena);
        let amount = b.int((idx - 1) as i32);
        let delayed = delay_sig_iir(arena, rt, recursive_iir, amount);
        let term = mul_sig_iir(arena, rt, delayed, coef);
        res = add_sig_iir(arena, rt, res, term);
    }
    add_sig_iir(arena, rt, res, input_fir)
}

fn sig_int_value(arena: &TreeArena, sig: SigId) -> Option<i32> {
    match match_sig(arena, sig) {
        SigMatch::Int(value) => Some(value),
        _ => None,
    }
}

fn is_zero_sig(arena: &TreeArena, sig: SigId) -> bool {
    match match_sig(arena, sig) {
        SigMatch::Int(0) => true,
        SigMatch::Real(value) => value == 0.0,
        _ => false,
    }
}

fn is_one_sig(arena: &TreeArena, sig: SigId) -> bool {
    match match_sig(arena, sig) {
        SigMatch::Int(1) => true,
        SigMatch::Real(value) => value == 1.0,
        _ => false,
    }
}

fn neg_sig(builder: &mut SigBuilder<'_>, sig: SigId) -> SigId {
    match match_sig(builder.arena(), sig) {
        SigMatch::Int(value) => match value.checked_neg() {
            Some(value) => builder.int(value),
            None => {
                let minus_one = builder.int(-1);
                builder.mul(minus_one, sig)
            }
        },
        SigMatch::Real(value) => builder.real(-value),
        _ => {
            let minus_one = builder.int(-1);
            builder.mul(minus_one, sig)
        }
    }
}

fn add_or_passthrough(builder: &mut SigBuilder<'_>, lhs: SigId, rhs: SigId) -> SigId {
    if is_zero_sig(builder.arena(), lhs) {
        rhs
    } else if is_zero_sig(builder.arena(), rhs) {
        lhs
    } else {
        builder.add(lhs, rhs)
    }
}

fn delay_iir_coefs(arena: &mut TreeArena, coefs: &[SigId], delay: i32) -> SigId {
    if coefs.len() < 2 {
        return arena.nil();
    }
    let mut b = SigBuilder::new(arena);
    let mut shifted = Vec::with_capacity(coefs.len() + delay as usize);
    shifted.push(coefs[0]);
    shifted.push(delay_coef(&mut b, coefs[1], delay));
    for _ in 0..delay {
        shifted.push(b.int(0));
    }
    for coef in &coefs[2..] {
        shifted.push(delay_coef(&mut b, *coef, delay));
    }
    b.iir(&shifted)
}

fn delay_coef(builder: &mut SigBuilder<'_>, coef: SigId, delay: i32) -> SigId {
    if matches!(
        match_sig(builder.arena(), coef),
        SigMatch::Int(_) | SigMatch::Real(_)
    ) {
        coef
    } else {
        let amount = builder.int(delay);
        builder.delay(coef, amount)
    }
}

fn combine_iir_coefs(arena: &mut TreeArena, lhs: &[SigId], rhs: &[SigId], op: BinOp) -> SigId {
    if lhs.is_empty() || rhs.is_empty() || lhs[0] != rhs[0] {
        return arena.nil();
    }
    let mut b = SigBuilder::new(arena);
    let len = lhs.len().max(rhs.len());
    let zero = b.int(0);
    let mut coefs = Vec::with_capacity(len);
    coefs.push(lhs[0]);
    for idx in 1..len {
        let l = lhs.get(idx).copied().unwrap_or(zero);
        let r = rhs.get(idx).copied().unwrap_or(zero);
        let value = match op {
            BinOp::Add => add_or_passthrough(&mut b, l, r),
            BinOp::Sub => {
                if is_zero_sig(b.arena(), r) {
                    l
                } else {
                    b.sub(l, r)
                }
            }
            _ => unreachable!("IIR coefficient combine only supports add/sub"),
        };
        coefs.push(value);
    }
    b.iir(&coefs)
}

fn scale_iir_coefs(arena: &mut TreeArena, coefs: &[SigId], factor: SigId, op: BinOp) -> SigId {
    if coefs.is_empty() {
        return arena.nil();
    }
    let mut b = SigBuilder::new(arena);
    let mut scaled = Vec::with_capacity(coefs.len());
    scaled.push(coefs[0]);
    for coef in &coefs[1..] {
        let value = match op {
            BinOp::Mul => {
                if is_one_sig(b.arena(), factor) {
                    *coef
                } else {
                    b.mul(*coef, factor)
                }
            }
            BinOp::Div => b.div(*coef, factor),
            _ => unreachable!("IIR coefficient scale only supports mul/div"),
        };
        scaled.push(value);
    }
    b.iir(&scaled)
}
