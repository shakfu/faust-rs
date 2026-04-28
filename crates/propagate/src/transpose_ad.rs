//! Structural scaffolding for phase-E1 recursive RAD transposition.
//!
//! # Source provenance
//! Original Rust design for RAD phase E1, documented in
//! `porting/reverse-ad-rad-implementation-plan-2026-04-27-en.md` section
//! "19. Feasibility analysis for stateful RAD".
//!
//! # Scope
//! This module is deliberately not wired into `rad(...)` lowering yet.  It
//! builds the small, testable core needed by the future E1 implementation:
//! given one affine LTI `DEBRUIJNREC` group, extract the linear state
//! transition matrix and build the transposed recursive group.
//!
//! The emitted group uses `input(i)` as the incoming cotangent for primal
//! output lane `i`, plus the transposed feedback from the previous adjoint
//! state. That is only an internal structural representation; a later phase
//! must still define the block/tape convention that supplies those inputs in
//! time-reversed order before this can become user-visible `rad(...)`
//! behavior.
//!
//! Current conservative limits:
//! - accepted recursive-state terms: `Proj(slot, DEBRUIJNREF(1))`, sums,
//!   differences, and multiplication/division by state-independent constant
//!   expressions;
//! - independent driving terms are ignored because they do not contribute to
//!   the state-to-state transpose;
//! - temporal operators over recursive state are rejected until the block
//!   evaluation convention fixes their adjoint placement.

use std::fmt::{Display, Formatter};

use crate::stateful_rad::{RadRecLinearity, classify_de_bruijn_rec_group};
use signals::{BinOp, SigBuilder, SigId, SigMatch, match_sig};
use tlib::{TreeArena, de_bruijn_rec, de_bruijn_ref, list_to_vec, match_de_bruijn_rec};

/// Error returned by the phase-E1 transposition scaffold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransposeAdError {
    /// The input was not a `DEBRUIJNREC(body)` group.
    NotRecursiveGroup,
    /// The recursive body list was malformed.
    MalformedBody,
    /// The classifier did not prove the group is LTI.
    NotLinearLti,
    /// The affine extractor found a recursive-state term outside the current
    /// narrow E1 scaffold.
    UnsupportedLinearTerm,
    /// A temporal operator over recursive state needs the future block/tape
    /// convention before it can be transposed safely.
    TemporalTermNeedsBlockConvention,
    /// A projected recursive slot did not fit the group arity.
    SlotOutOfRange,
}

impl Display for TransposeAdError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRecursiveGroup => f.write_str("not a DEBRUIJNREC group"),
            Self::MalformedBody => f.write_str("malformed DEBRUIJNREC body list"),
            Self::NotLinearLti => f.write_str("recursive group is not classified as LinearLti"),
            Self::UnsupportedLinearTerm => {
                f.write_str("unsupported affine LTI term in recursive body")
            }
            Self::TemporalTermNeedsBlockConvention => {
                f.write_str("temporal recursive term needs a block/tape convention")
            }
            Self::SlotOutOfRange => f.write_str("recursive projection slot is out of range"),
        }
    }
}

impl std::error::Error for TransposeAdError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LinearTerm {
    source_output: usize,
    state_slot: usize,
    coeff: SigId,
}

/// Builds the transposed affine LTI recursive group for future RAD phase E1.
///
/// Mapping status: `adapted`, preparatory only. This mirrors the system
/// transposition rules in the RAD plan but intentionally does not change
/// current `rad(...)` behavior. Callers must not treat the returned group as
/// executable reverse-mode code until the surrounding block/tape evaluation
/// convention exists.
pub fn transpose_lti_de_bruijn_rec_scaffold(
    arena: &mut TreeArena,
    group: SigId,
) -> Result<SigId, TransposeAdError> {
    let body = match_de_bruijn_rec(arena, group).ok_or(TransposeAdError::NotRecursiveGroup)?;
    if classify_de_bruijn_rec_group(arena, group) != Some(RadRecLinearity::LinearLti) {
        return Err(TransposeAdError::NotLinearLti);
    }
    let branches = list_to_vec(arena, body).ok_or(TransposeAdError::MalformedBody)?;
    let arity = branches.len();
    let mut terms = Vec::new();
    let one = SigBuilder::new(arena).real(1.0);

    for (source_output, branch) in branches.into_iter().enumerate() {
        extract_affine_state_terms(arena, branch, 1, one, source_output, arity, &mut terms)?;
    }

    let rec_ref = de_bruijn_ref(arena, 1);
    let mut transposed_branches = Vec::with_capacity(arity);
    for target_slot in 0..arity {
        let target_i32 =
            i32::try_from(target_slot).map_err(|_| TransposeAdError::SlotOutOfRange)?;
        let mut expr = SigBuilder::new(arena).input(target_i32);
        for term in terms.iter().filter(|term| term.state_slot == target_slot) {
            let source_i32 =
                i32::try_from(term.source_output).map_err(|_| TransposeAdError::SlotOutOfRange)?;
            let prev_adj = SigBuilder::new(arena).proj(source_i32, rec_ref);
            let scaled = SigBuilder::new(arena).mul(term.coeff, prev_adj);
            expr = SigBuilder::new(arena).add(expr, scaled);
        }
        transposed_branches.push(expr);
    }

    let body = tlib::vec_to_list(arena, &transposed_branches);
    Ok(de_bruijn_rec(arena, body))
}

fn extract_affine_state_terms(
    arena: &mut TreeArena,
    sig: SigId,
    current_level: i64,
    coeff: SigId,
    source_output: usize,
    arity: usize,
    out: &mut Vec<LinearTerm>,
) -> Result<(), TransposeAdError> {
    match match_sig(arena, sig) {
        SigMatch::Proj(slot, group)
            if tlib::match_de_bruijn_ref(arena, group) == Some(current_level) =>
        {
            let state_slot = usize::try_from(slot).map_err(|_| TransposeAdError::SlotOutOfRange)?;
            if state_slot >= arity {
                return Err(TransposeAdError::SlotOutOfRange);
            }
            out.push(LinearTerm {
                source_output,
                state_slot,
                coeff,
            });
            Ok(())
        }
        SigMatch::BinOp(BinOp::Add, x, y) => {
            extract_affine_state_terms(arena, x, current_level, coeff, source_output, arity, out)?;
            extract_affine_state_terms(arena, y, current_level, coeff, source_output, arity, out)
        }
        SigMatch::BinOp(BinOp::Sub, x, y) => {
            extract_affine_state_terms(arena, x, current_level, coeff, source_output, arity, out)?;
            let neg_one = SigBuilder::new(arena).real(-1.0);
            let neg_coeff = SigBuilder::new(arena).mul(coeff, neg_one);
            extract_affine_state_terms(
                arena,
                y,
                current_level,
                neg_coeff,
                source_output,
                arity,
                out,
            )
        }
        SigMatch::BinOp(BinOp::Mul, x, y) => {
            let x_depends = contains_current_rec_ref(arena, x, current_level);
            let y_depends = contains_current_rec_ref(arena, y, current_level);
            match (x_depends, y_depends) {
                (true, false) => {
                    let scaled = SigBuilder::new(arena).mul(coeff, y);
                    extract_affine_state_terms(
                        arena,
                        x,
                        current_level,
                        scaled,
                        source_output,
                        arity,
                        out,
                    )
                }
                (false, true) => {
                    let scaled = SigBuilder::new(arena).mul(coeff, x);
                    extract_affine_state_terms(
                        arena,
                        y,
                        current_level,
                        scaled,
                        source_output,
                        arity,
                        out,
                    )
                }
                (false, false) => Ok(()),
                (true, true) => Err(TransposeAdError::UnsupportedLinearTerm),
            }
        }
        SigMatch::BinOp(BinOp::Div, x, y) => {
            if contains_current_rec_ref(arena, y, current_level) {
                return Err(TransposeAdError::UnsupportedLinearTerm);
            }
            if contains_current_rec_ref(arena, x, current_level) {
                let scaled = SigBuilder::new(arena).div(coeff, y);
                extract_affine_state_terms(
                    arena,
                    x,
                    current_level,
                    scaled,
                    source_output,
                    arity,
                    out,
                )
            } else {
                Ok(())
            }
        }
        SigMatch::Delay1(x) | SigMatch::Delay(x, _) | SigMatch::Prefix(_, x)
            if contains_current_rec_ref(arena, x, current_level) =>
        {
            Err(TransposeAdError::TemporalTermNeedsBlockConvention)
        }
        SigMatch::FloatCast(x)
        | SigMatch::Output(_, x)
        | SigMatch::Lowest(x)
        | SigMatch::Highest(x) => {
            extract_affine_state_terms(arena, x, current_level, coeff, source_output, arity, out)
        }
        _ if contains_current_rec_ref(arena, sig, current_level) => {
            Err(TransposeAdError::UnsupportedLinearTerm)
        }
        _ => Ok(()),
    }
}

fn contains_current_rec_ref(arena: &TreeArena, sig: SigId, current_level: i64) -> bool {
    if tlib::match_de_bruijn_ref(arena, sig) == Some(current_level) {
        return true;
    }
    if let Some(body) = match_de_bruijn_rec(arena, sig) {
        return contains_current_rec_ref(arena, body, current_level + 1);
    }
    let Some(node) = arena.node(sig) else {
        return false;
    };
    node.children
        .as_slice()
        .iter()
        .copied()
        .any(|child| contains_current_rec_ref(arena, child, current_level))
}

#[cfg(test)]
mod tests {
    use super::{TransposeAdError, transpose_lti_de_bruijn_rec_scaffold};
    use signals::{BinOp, SigBuilder, SigId, SigMatch, match_sig};
    use tlib::{
        TreeArena, de_bruijn_rec, de_bruijn_ref, list_to_vec, match_de_bruijn_rec, vec_to_list,
    };

    fn rec_group(arena: &mut TreeArena, branches: &[SigId]) -> SigId {
        let body = vec_to_list(arena, branches);
        de_bruijn_rec(arena, body)
    }

    fn projected_slot(arena: &TreeArena, sig: SigId) -> Option<usize> {
        let SigMatch::Proj(slot, group) = match_sig(arena, sig) else {
            return None;
        };
        if tlib::match_de_bruijn_ref(arena, group) == Some(1) {
            usize::try_from(slot).ok()
        } else {
            None
        }
    }

    fn branch_contains_prev_slot(arena: &TreeArena, sig: SigId, slot: usize) -> bool {
        if projected_slot(arena, sig) == Some(slot) {
            return true;
        }
        match match_sig(arena, sig) {
            SigMatch::BinOp(BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div, x, y) => {
                branch_contains_prev_slot(arena, x, slot)
                    || branch_contains_prev_slot(arena, y, slot)
            }
            _ => false,
        }
    }

    #[test]
    fn scaffold_transposes_cross_coupled_affine_lti_group() {
        let mut arena = TreeArena::new();
        let ref1 = de_bruijn_ref(&mut arena, 1);
        let (branch0, branch1) = {
            let mut b = SigBuilder::new(&mut arena);
            let prev0 = b.proj(0, ref1);
            let prev1 = b.proj(1, ref1);
            let half = b.real(0.5);
            let scaled_prev1 = b.mul(half, prev1);
            let input = b.input(0);
            let branch0 = b.add(input, scaled_prev1);
            let branch1 = b.sub(prev0, input);
            (branch0, branch1)
        };
        let group = rec_group(&mut arena, &[branch0, branch1]);

        let transposed =
            transpose_lti_de_bruijn_rec_scaffold(&mut arena, group).expect("LTI group");
        let body = match_de_bruijn_rec(&arena, transposed).expect("transposed group");
        let branches = list_to_vec(&arena, body).expect("body list");

        assert_eq!(branches.len(), 2);
        assert!(
            branch_contains_prev_slot(&arena, branches[0], 1),
            "target adjoint slot 0 must receive original row 1"
        );
        assert!(
            branch_contains_prev_slot(&arena, branches[1], 0),
            "target adjoint slot 1 must receive original row 0"
        );
    }

    #[test]
    fn scaffold_rejects_time_varying_and_nonlinear_groups() {
        let mut arena = TreeArena::new();
        let ref1 = de_bruijn_ref(&mut arena, 1);
        let ltv_group = {
            let mut b = SigBuilder::new(&mut arena);
            let prev = b.proj(0, ref1);
            let coeff = b.input(0);
            let branch = b.mul(coeff, prev);
            rec_group(&mut arena, &[branch])
        };
        assert_eq!(
            transpose_lti_de_bruijn_rec_scaffold(&mut arena, ltv_group),
            Err(TransposeAdError::NotLinearLti)
        );

        let nonlinear_group = {
            let mut b = SigBuilder::new(&mut arena);
            let prev = b.proj(0, ref1);
            let branch = b.sin(prev);
            rec_group(&mut arena, &[branch])
        };
        assert_eq!(
            transpose_lti_de_bruijn_rec_scaffold(&mut arena, nonlinear_group),
            Err(TransposeAdError::NotLinearLti)
        );
    }

    #[test]
    fn scaffold_keeps_temporal_lti_terms_blocked() {
        let mut arena = TreeArena::new();
        let ref1 = de_bruijn_ref(&mut arena, 1);
        let group = {
            let mut b = SigBuilder::new(&mut arena);
            let prev = b.proj(0, ref1);
            let branch = b.delay1(prev);
            rec_group(&mut arena, &[branch])
        };

        assert_eq!(
            transpose_lti_de_bruijn_rec_scaffold(&mut arena, group),
            Err(TransposeAdError::TemporalTermNeedsBlockConvention)
        );
    }
}
